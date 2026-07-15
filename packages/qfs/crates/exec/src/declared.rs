//! blueprint §13 **tier 2** — declared-view **body evaluation** (the execution half). Reading a
//! declared mount is not a native wire fetch (tier 1); it is *executing the view's stored pipeline*:
//! the body's confined `/http/<self>/…` source is fetched over the wire, the body's remaining pipe
//! ops run through the real engine, and the declared `OF` type shapes the delivered rows. One rule —
//! "a declared view IS its stored query" — dissolves the tier-1 parity gaps (envelope unwrapping is
//! an ordinary `|> EXPAND`, the mount path decouples from the wire endpoint, the type is enforced).
//!
//! This lives in qfs-exec (which already owns query execution above the spine) rather than the
//! binary, so the binary stays off the lower spine (`qfs-parser`/`qfs-engine`): the driver-specific
//! **fetch is injected as a closure** by the caller (the binary's `RestReadDriver`, over its confined
//! applier). The body was stored by the surface desugar as the serde JSON of a parsed
//! `Statement::Query`; this module rehydrates it via serde (no re-parse) and reuses the SHIPPED
//! lowering (`qfs_pushdown::lower_query` + `partition_by_source`) and engine
//! (`qfs_engine::MiniEvaluator`) — the same machinery the main read path runs.
//!
//! ## Confinement (defense in depth)
//! The body's source MUST address the driver's own `/http/<name>` namespace, re-checked here before
//! the fetch closure is ever called (the load-time structural check already dropped foreign-host
//! declarations, and the caller's applier confines the wire host at `send_one`).

use qfs_core::{
    CfsError, Column, ColumnType, Fields, PushdownProfile, Row, RowBatch, Schema, Value,
};
use qfs_engine::{eval_value, CombineEngine, MiniEvaluator, ScanResults};
use qfs_parser::{EffectBody, Expr, PipeOp, Pipeline, Source, Statement};
use qfs_pushdown::{lower_query, lower_scalar, partition_by_source, SourceId, SourceRegistry};

/// The single column a `FOLLOW <field>` stage delivers: the followed URL's raw response bytes.
/// Mirrors the blob convention of the local driver (`content` carries the bytes).
pub const FOLLOW_CONTENT_COL: &str = "content";

/// The synthetic source id every declared-view body scan routes to — the wire namespace. The body is
/// fetched by the injected closure (not a registered read driver), so the id only needs to be stable;
/// `PushdownProfile::None` keeps every op residual (run by the engine over the fetched batch), which
/// is exactly what tier 2 wants.
const WIRE_SOURCE: &str = "http";

/// One declared view, resolved for evaluation: its mount-path template (`/slack/history` or
/// `/chatwork/rooms/{room}/messages`), its stored body, and — if it declared `OF <type>` — the
/// column names that type promises (the delivered contract the result is shaped to).
#[derive(Debug, Clone)]
pub struct ViewSpec {
    /// The mount-path template (`{param}` segments bind at read time).
    pub template: String,
    /// The stored body: serde JSON of a parsed `Statement::Query`.
    pub body: String,
    /// The `OF <type>` column names, if the view declared a type.
    pub of_columns: Option<Vec<String>>,
    /// The `OF <type>`'s row-local refinement predicate (blueprint §5.4), if the type carried a
    /// `WHERE`. Enforced as per-row MEMBERSHIP over each delivered row.
    pub of_refinement: Option<Expr>,
}

/// One declared write/CALL mapping, resolved for evaluation (blueprint §13 tier 2 — the write
/// twin of [`ViewSpec`]): its mount-path template (`/slack/post` or
/// `/chatwork/rooms/{room}/messages`) and its stored effect body (an `INSERT INTO /http/<self>/…
/// VALUES (<expr>)` statement, serde JSON of a parsed `Statement::Effect`).
#[derive(Debug, Clone)]
pub struct MapSpec {
    /// The mount-path template the universal write verb addresses (`{param}` segments bind at
    /// apply time from the incoming effect path).
    pub template: String,
    /// The stored body: serde JSON of a parsed `Statement::Effect` whose `VALUES (<expr>)` maps a
    /// bound `row` to the wire body.
    pub body: String,
}

/// The evaluated wire write of a declared MAP (blueprint §13 tier 2): the confined
/// `/rest/<driver>/<resource>` path the applier POSTs to, and one wire-body [`Value`] per incoming
/// row (the map's `VALUES (<expr>)` evaluated with `row` bound to that row). The caller (the
/// binary's confined applier) encodes each body through the driver codec and performs the I/O —
/// evaluation stays pure, exactly as the read side injects its wire fetch as a closure.
#[derive(Debug, Clone)]
pub struct MapWrite {
    /// The `/rest/<driver>/<resource>` path the applier resolves to the confined wire endpoint.
    pub rest_path: String,
    /// One evaluated wire body per incoming row, in input order.
    pub bodies: Vec<Value>,
    /// The declared wire-body encoding (`… |> ENCODE multipart VALUES (row)`, blueprint §13 /
    /// ticket 20260711121526), `None` for the default JSON encode. The caller's applier resolves
    /// the named encoding (an unknown name is its structured refusal, never a silent JSON).
    pub encoding: Option<String>,
}

/// Match a concrete mount path against a view-path template, binding `{param}` segments. Returns the
/// bound `(name, value)` pairs when every non-template segment matches and the segment counts agree;
/// `None` otherwise. A template segment `{room}` binds the concrete segment in that position.
#[must_use]
pub fn match_template(template: &str, concrete: &str) -> Option<Vec<(String, String)>> {
    let t: Vec<&str> = template.trim_matches('/').split('/').collect();
    let c: Vec<&str> = concrete.trim_matches('/').split('/').collect();
    if t.len() != c.len() {
        return None;
    }
    let mut params = Vec::new();
    for (ts, cs) in t.iter().zip(c.iter()) {
        if is_param(ts) {
            params.push((ts[1..ts.len() - 1].to_string(), (*cs).to_string()));
        } else if ts != cs {
            return None;
        }
    }
    Some(params)
}

/// Recover a declared driver's mount-relative view path from the inner remapped scan path: the
/// declared read facet is registered behind a `/rest/<name>` remap, so an inbound scan arrives as
/// `/rest/<name>/<rest…>`; the view template is `/<name>/<rest…>`. Strips the synthetic `/rest`.
#[must_use]
pub fn view_path_of_scan(scan_path: &str) -> String {
    match scan_path.strip_prefix("/rest") {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => rest.to_string(),
        _ => scan_path.to_string(),
    }
}

/// Whether a path segment is a `{param}` template placeholder.
fn is_param(seg: &str) -> bool {
    seg.len() >= 2 && seg.starts_with('{') && seg.ends_with('}')
}

/// Evaluate a declared view body and return the shaped rows (blueprint §13 tier 2). The `fetch`
/// closure performs the driver-specific wire read for a `/rest/<name>/<resource>` path (the caller's
/// confined applier); everything else — rehydrate, confine, run the body's ops through the engine,
/// shape to the `OF` type — is here.
///
/// Steps: rehydrate the stored `Statement::Query`; render + `{param}`-substitute its `/http/…` source
/// and **confine** it to the driver's own namespace; `fetch` the resource (the wire codec performs
/// the leading `DECODE`); run the body's remaining ops (`EXPAND`, `WHERE`, `SELECT`, …) through the
/// shipped lowering + engine; then shape the result to the declared `OF` type's columns.
///
/// # Errors
/// [`CfsError::InvalidPath`] if the body is not a rehydratable read query, its source is outside the
/// driver's `/http/<name>` namespace (the confinement violation), or the fetch / engine fails.
// 8 args: the view identity (name/path/OF columns/refinement/params) plus the TWO injected wire
// closures (fetch + follow) — grouping them would only move the count into a one-shot struct.
#[allow(clippy::too_many_arguments)]
pub fn eval_view_body<F, G>(
    body_json: &str,
    driver_name: &str,
    view_path: &str,
    of_columns: Option<&[String]>,
    of_refinement: Option<&Expr>,
    params: &[(String, String)],
    fetch: F,
    follow: G,
) -> Result<RowBatch, CfsError>
where
    F: FnOnce(&str) -> Result<RowBatch, CfsError>,
    G: FnOnce(&str) -> Result<Vec<u8>, CfsError>,
{
    let invalid = |reason: &'static str| CfsError::InvalidPath {
        path: view_path.to_string(),
        reason,
    };

    // 0. A declared `OF` contract that resolves to ZERO columns is unreadable, loudly: the type
    //    row is missing, or its stored body predates the §5.4 object shape (a pre-break install).
    //    Shaping to an empty column list would silently project every delivered value away — the
    //    live zero-column defect (ticket 20260712005100) — so refuse with the fix in hand instead.
    if of_columns.is_some_and(<[String]>::is_empty) {
        return Err(invalid(
            "the view's declared OF type resolves no columns (its /sys/drivers type row is \
             missing or predates the current body shape) — re-install the driver declaration",
        ));
    }

    // 1. Rehydrate the stored body (serde, no re-parse). A view body is a read query.
    let stmt: Statement = serde_json::from_str(body_json)
        .map_err(|_| invalid("declared view body is not rehydratable"))?;
    let Statement::Query(pipeline) = stmt else {
        return Err(invalid("declared view body is not a read query"));
    };

    // 2. Render + {param}-substitute the wire source, and CONFINE it to `/http/<name>/…`.
    let Source::Path(src) = &pipeline.source else {
        return Err(invalid("declared view body must read a wire path"));
    };
    let source_path = render_source_path(&src.segments, params);
    let wire_resource = confined_wire_resource(&source_path, driver_name)
        .ok_or_else(|| invalid("declared view body addresses a foreign host (§13 confinement)"))?;

    // 3. Fetch the wire resource over the caller's confined applier (leading `DECODE` = the applier
    //    codec). The stock `/rest/<name>/<resource>` addressing the applier resolves.
    let rest_path = format!("/rest/{driver_name}/{wire_resource}");
    let fetched = fetch(&rest_path)?;

    // 4. Split the body at its `FOLLOW <field>` stage, if any (blueprint §13, ticket
    //    20260711121526): the ops BEFORE it run over the fetched batch as usual, then the named
    //    field of the (single) delivered row becomes the URL of the second GET — performed by the
    //    injected `follow` closure, which carries NO driver credentials (the URL is
    //    self-authorizing) — and its raw bytes become a one-row `content` batch the ops AFTER the
    //    stage (usually none) run over.
    let follow_at = pipeline
        .ops
        .iter()
        .position(|op| matches!(op, PipeOp::Follow(_)));
    let evaluated = match follow_at {
        None => run_body_ops(&pipeline, fetched).map_err(|reason| CfsError::InvalidPath {
            path: view_path.to_string(),
            reason,
        })?,
        Some(at) => {
            if pipeline.ops[at + 1..]
                .iter()
                .any(|op| matches!(op, PipeOp::Follow(_)))
            {
                return Err(invalid(
                    "a declared view body may carry at most one FOLLOW stage",
                ));
            }
            let pre = Pipeline {
                source: pipeline.source.clone(),
                ops: pipeline.ops[..at].to_vec(),
            };
            let delivered =
                run_body_ops(&pre, fetched).map_err(|reason| CfsError::InvalidPath {
                    path: view_path.to_string(),
                    reason,
                })?;
            let PipeOp::Follow(fref) = &pipeline.ops[at] else {
                unreachable!("position() matched a Follow op");
            };
            let url = follow_url(&delivered, &fref.field).map_err(invalid)?;
            let bytes = follow(&url)?;
            let batch = RowBatch::new(
                Schema::new(vec![Column::new(
                    FOLLOW_CONTENT_COL,
                    ColumnType::Bytes,
                    false,
                )]),
                vec![Row::new(vec![Value::Bytes(bytes)])],
            );
            let rest_ops = pipeline.ops[at + 1..].to_vec();
            if rest_ops.is_empty() {
                batch
            } else {
                let post = Pipeline {
                    source: pipeline.source.clone(),
                    ops: rest_ops,
                };
                run_body_ops(&post, batch).map_err(|reason| CfsError::InvalidPath {
                    path: view_path.to_string(),
                    reason,
                })?
            }
        }
    };

    // 5. Shape to the declared `OF` type (the delivered contract) — project to its columns, then
    //    membership-check each delivered row against the type's refinement (blueprint §5.4).
    shape_to_type(evaluated, of_columns, of_refinement, view_path)
}

/// The follow URL of a delivered batch: exactly ONE row (a follow over zero or many rows is
/// ambiguous — fail closed) whose named field is text. Returns the structured reason otherwise.
fn follow_url(delivered: &RowBatch, field: &str) -> Result<String, &'static str> {
    let [row] = delivered.rows.as_slice() else {
        return Err("FOLLOW requires exactly one delivered row to take the URL field from");
    };
    let idx = delivered
        .schema
        .columns
        .iter()
        .position(|c| c.name == field)
        .ok_or("the FOLLOW field was not delivered by the preceding stages")?;
    match row.values.get(idx) {
        Some(Value::Text(url)) if !url.is_empty() => Ok(url.clone()),
        _ => Err("the FOLLOW field must carry a non-empty text URL"),
    }
}

/// Evaluate a declared MAP body against the incoming write rows and return the wire write
/// (blueprint §13 tier 2 — the write twin of [`eval_view_body`]). The map body is a stored `INSERT
/// INTO /http/<self>/<resource> VALUES (<expr>)` statement; `<expr>` (`row` passthrough, or a
/// `{field: row.col, …}` struct literal) is evaluated **per incoming row** with `row` bound to that
/// row, constructing the wire body the applier POSTs. Purity holds: this constructs the wire
/// effect; the caller's confined applier performs the I/O at COMMIT.
///
/// Steps: rehydrate the stored `Statement::Effect`; render + `{param}`-substitute its `/http/…`
/// target and **confine** it to the driver's own namespace; lower the single `VALUES` expression to
/// a per-row scalar via the shipped [`lower_scalar`]; then, for each incoming row, bind it as a
/// single `row` struct column and evaluate the expression via the shipped [`eval_value`].
///
/// # Errors
/// [`CfsError::InvalidPath`] if the body is not a rehydratable single-expression `VALUES` write, its
/// target is outside the driver's `/http/<name>` namespace (the confinement violation), or the
/// expression is not a per-row scalar the engine can evaluate.
pub fn eval_map_body(
    body_json: &str,
    driver_name: &str,
    map_path: &str,
    params: &[(String, String)],
    incoming: &RowBatch,
) -> Result<MapWrite, CfsError> {
    let invalid = |reason: &'static str| CfsError::InvalidPath {
        path: map_path.to_string(),
        reason,
    };

    // 1. Rehydrate the stored body (serde, no re-parse). A map body is a single-row `VALUES (<expr>)`
    //    write effect (one wire body per incoming row; the row list is the write's own — not here).
    let stmt: Statement = serde_json::from_str(body_json)
        .map_err(|_| invalid("declared map body is not rehydratable"))?;
    let Statement::Effect(effect) = stmt else {
        return Err(invalid("declared map body is not a write effect"));
    };
    // The body is a `VALUES` write, either bare or wrapped in the `|> ENCODE <fmt>` upload form
    // (blueprint §13, ticket 20260711121526) — the parser desugared `INSERT INTO … |> ENCODE
    // multipart VALUES (row)` onto the `EffectBody::Pipeline` shape (a VALUES source with one
    // encode stage). The named encoding rides on the returned [`MapWrite`] for the applier.
    let (values, encoding) =
        match &effect.body {
            EffectBody::Values(values) => (values, None),
            EffectBody::Pipeline(p) => {
                let Source::Values(values) = &p.source else {
                    return Err(invalid("declared map body must be a VALUES write"));
                };
                match p.ops.as_slice() {
                    [PipeOp::Encode(codec)] => (values, Some(codec.fmt.clone())),
                    _ => return Err(invalid(
                        "declared map pipeline body must be exactly one ENCODE stage over VALUES",
                    )),
                }
            }
            EffectBody::SetWhere { .. } => {
                return Err(invalid("declared map body must be a VALUES write"))
            }
        };
    let expr = match values.rows.as_slice() {
        [row] => match row.as_slice() {
            [expr] => expr,
            _ => {
                return Err(invalid(
                    "declared map VALUES must carry exactly one wire-body expression",
                ))
            }
        },
        _ => return Err(invalid("declared map VALUES must carry exactly one row")),
    };

    // 2. Render + {param}-substitute the wire target, and CONFINE it to `/http/<name>/…` (the same
    //    anti-exfiltration boundary the read side enforces before any fetch).
    let target_path = render_source_path(&effect.target.segments, params);
    let wire_resource = confined_wire_resource(&target_path, driver_name)
        .ok_or_else(|| invalid("declared map body addresses a foreign host (§13 confinement)"))?;
    let rest_path = format!("/rest/{driver_name}/{wire_resource}");

    // 3. Lower the single `VALUES` expression to a per-row scalar (the SHIPPED lowering a computed
    //    `SELECT` term uses — one lowering, both paths).
    let scalar =
        lower_scalar(expr).map_err(|_| invalid("declared map body is not a per-row expression"))?;

    // 4. Evaluate per incoming row: bind the row as a single `row` struct column, then run the
    //    scalar through the SHIPPED per-row evaluator. `row` passthrough yields the whole struct; a
    //    `{f: row.col}` literal navigates its fields — the wire body the applier encodes + POSTs.
    let schema = Schema::new(vec![Column::new("row", ColumnType::Unknown, true)]);
    let bodies = incoming
        .rows
        .iter()
        .map(|r| {
            let row_struct = Value::Struct(Fields::new(
                incoming
                    .schema
                    .columns
                    .iter()
                    .zip(&r.values)
                    .map(|(c, v)| (c.name.clone(), v.clone()))
                    .collect(),
            ));
            eval_value(&scalar, &schema, &Row::new(vec![row_struct]))
        })
        .collect();

    Ok(MapWrite {
        rest_path,
        bodies,
        encoding,
    })
}

/// Render a body-source segment list to a `/seg/seg` path, substituting bound `{param}` values.
/// A segment may carry a query-string suffix (`{file}?create_download_url=1`, blueprint §13 wire
/// paths): the `{param}` head substitutes, the query rides behind it verbatim.
fn render_source_path(segments: &[qfs_parser::PathSegment], params: &[(String, String)]) -> String {
    let mut out = String::new();
    for seg in segments {
        out.push('/');
        let (head, query) = match seg.name.split_once('?') {
            Some((head, query)) => (head, Some(query)),
            None => (seg.name.as_str(), None),
        };
        if is_param(head) {
            let key = &head[1..head.len() - 1];
            match params.iter().find(|(k, _)| k == key) {
                Some((_, v)) => out.push_str(v),
                None => out.push_str(head),
            }
        } else {
            out.push_str(head);
        }
        if let Some(query) = query {
            out.push('?');
            out.push_str(query);
        }
    }
    out
}

/// The wire resource of a confined body source: `/http/<driver>/<resource…>` → `<resource…>`. `None`
/// when the source is not under the driver's own `/http/<driver>` namespace (the confinement rule).
fn confined_wire_resource(source_path: &str, driver_name: &str) -> Option<String> {
    let segs: Vec<&str> = source_path.trim_matches('/').split('/').collect();
    if segs.len() < 3 || segs[0] != "http" || segs[1] != driver_name {
        return None;
    }
    Some(segs[2..].join("/"))
}

/// Lower the body pipeline's ops and run them through the engine over the already-fetched batch. The
/// source resolves to the synthetic wire source (`None` pushdown), so `partition_by_source` leaves
/// every op as a residual the [`MiniEvaluator`] executes over `fetched`.
fn run_body_ops(
    pipeline: &qfs_parser::Pipeline,
    fetched: RowBatch,
) -> Result<RowBatch, &'static str> {
    let source_of = |_segs: &[String]| SourceId::new(WIRE_SOURCE);
    let schema_of = |_src: &SourceId| Schema::empty();
    // A declared-driver view body carries no `|> transform` stage (and if one appeared it would be
    // unresolvable here) — resolve none.
    let transform_of = |_: &str| None::<qfs_core::ResolvedTransform>;
    let logical = lower_query(pipeline, &source_of, &schema_of, &transform_of)
        .map_err(|_| "declared view body could not be lowered")?;
    let mut reg = SourceRegistry::new();
    reg.register(SourceId::new(WIRE_SOURCE), PushdownProfile::None);
    let physical = partition_by_source(&logical, &reg)
        .map_err(|_| "declared view body could not be planned")?;
    MiniEvaluator::new()
        .execute(&physical, ScanResults::new(vec![fetched]))
        .map_err(|_| "declared view body evaluation failed")
}

/// Shape a batch to a declared `OF` type: project to exactly its columns, in order (a column the
/// service did not deliver becomes `Null`; a delivered column the type does not name is dropped),
/// then — if the type carries a `WHERE` refinement (blueprint §5.4) — membership-check each
/// delivered row against it. This makes the declared type the delivered contract: `conformance`
/// on the shaped result passes when the body is right, and a delivered row that violates the
/// refinement is a structured [`CfsError::TypeMembership`] refusal. With no `OF` type, the batch
/// passes through unchanged (a refinement is only meaningful alongside its columns).
///
/// # Errors
/// [`CfsError::TypeMembership`] if any delivered row fails the `OF` type's refinement predicate.
fn shape_to_type(
    batch: RowBatch,
    of_columns: Option<&[String]>,
    of_refinement: Option<&Expr>,
    view_path: &str,
) -> Result<RowBatch, CfsError> {
    let Some(cols) = of_columns else {
        return Ok(batch);
    };
    let positions: Vec<Option<usize>> = cols
        .iter()
        .map(|c| batch.schema.columns.iter().position(|bc| &bc.name == c))
        .collect();
    let schema = Schema::new(
        cols.iter()
            .enumerate()
            .map(|(i, name)| {
                let ty = positions[i]
                    .map_or(ColumnType::Unknown, |p| batch.schema.columns[p].ty.clone());
                Column::new(name.clone(), ty, true)
            })
            .collect(),
    );
    let mut rows = Vec::with_capacity(batch.rows.len());
    for row in batch.rows {
        let shaped = Row::new(
            positions
                .iter()
                .map(|p| {
                    p.and_then(|i| row.values.get(i).cloned())
                        .unwrap_or(Value::Null)
                })
                .collect(),
        );
        // Per-row MEMBERSHIP: a delivered row that fails the refinement is refused (blueprint §5.4).
        if let Some(pred) = of_refinement {
            qfs_core::check_membership(&schema, pred, &shaped).map_err(|e| {
                CfsError::TypeMembership {
                    path: view_path.to_string(),
                    detail: e.to_string(),
                }
            })?;
        }
        rows.push(shaped);
    }
    Ok(RowBatch::new(schema, rows))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    /// Decode a JSON body into a `RowBatch` exactly as the wire fetch would (the applier's codec).
    fn decode(bytes: &[u8]) -> RowBatch {
        qfs_core::CodecRegistry::with_builtins()
            .resolve("json")
            .expect("json codec")
            .decode(bytes)
            .expect("json decodes")
    }

    /// The follow closure for bodies that carry no FOLLOW stage (the common case).
    fn no_follow(_url: &str) -> Result<Vec<u8>, CfsError> {
        panic!("the follow closure must NOT run for a body with no FOLLOW stage")
    }

    #[test]
    fn match_template_binds_params_and_rejects_mismatches() {
        assert_eq!(
            match_template("/slack/history", "/slack/history"),
            Some(vec![])
        );
        assert_eq!(
            match_template(
                "/chatwork/rooms/{room}/messages",
                "/chatwork/rooms/123/messages"
            ),
            Some(vec![("room".to_string(), "123".to_string())])
        );
        assert_eq!(match_template("/slack/history", "/slack/other"), None);
        assert_eq!(match_template("/slack/history", "/slack/history/x"), None);
    }

    #[test]
    fn view_path_of_scan_strips_the_rest_remap() {
        assert_eq!(view_path_of_scan("/rest/slack/history"), "/slack/history");
        assert_eq!(
            view_path_of_scan("/rest/chatwork/rooms/1/messages"),
            "/chatwork/rooms/1/messages"
        );
        assert_eq!(view_path_of_scan("/slack/history"), "/slack/history");
    }

    #[test]
    fn confined_wire_resource_pins_to_the_own_host() {
        assert_eq!(
            confined_wire_resource("/http/slack/conversations.history", "slack").as_deref(),
            Some("conversations.history")
        );
        assert_eq!(
            confined_wire_resource("/http/chatwork/rooms/1/messages", "chatwork").as_deref(),
            Some("rooms/1/messages")
        );
        assert_eq!(confined_wire_resource("/http/evil/steal", "slack"), None);
        assert_eq!(confined_wire_resource("/mail/inbox", "slack"), None);
    }

    #[test]
    fn eval_view_body_unwraps_the_envelope_and_shapes_to_the_of_type() {
        // Tier 2: the body's `|> EXPAND messages` unwraps Slack's `{ok, messages}` envelope, and the
        // `OF` type shapes the result — so conformance would pass (the tier-1 recorded gap dissolves).
        let body = serde_json::to_string(
            &qfs_parser::parse_statement(
                "/http/slack/conversations.history |> DECODE json |> EXPAND messages",
            )
            .unwrap(),
        )
        .unwrap();
        let of = vec!["ts".to_string(), "user".to_string(), "text".to_string()];

        let mut fetched_path = None;
        let batch = eval_view_body(&body, "slack", "/slack/history", Some(&of), None, &[], |path| {
            fetched_path = Some(path.to_string());
            Ok(decode(
                br#"{"ok":true,"messages":[{"ts":"1","user":"U1","text":"hi"},{"ts":"2","user":"U2","text":"yo"}]}"#,
            ))
        }, no_follow)
        .expect("reads");

        let cols: Vec<&str> = batch
            .schema
            .columns
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(cols, vec!["ts", "user", "text"], "shaped to the OF type");
        assert_eq!(batch.rows.len(), 2, "two messages, not one envelope row");
        // The mount path decoupled from the wire endpoint (the fetch addressed the dotted method).
        assert_eq!(
            fetched_path.as_deref(),
            Some("/rest/slack/conversations.history")
        );
    }

    #[test]
    fn eval_view_body_shapes_a_bare_array_to_the_of_type() {
        // The decode→typed-view seam with a REAL-shaped Chatwork `/rooms` body (ticket
        // 20260712005100): a bare JSON array of objects, surplus API fields included. The `OF`
        // shaping must project to the declared columns WITH their values — the live round saw the
        // right row count with every value lost.
        let body = serde_json::to_string(
            &qfs_parser::parse_statement("/http/chatwork/rooms |> DECODE json").unwrap(),
        )
        .unwrap();
        let of = vec!["room_id".to_string(), "name".to_string()];
        let batch = eval_view_body(
            &body,
            "chatwork",
            "/chatwork/rooms",
            Some(&of),
            None,
            &[],
            |_path| {
                Ok(decode(
                    br#"[{"room_id":1,"name":"general","type":"group","sticky":false,"unread_num":3},
                        {"room_id":2,"name":"dev","type":"group","sticky":true,"unread_num":0}]"#,
                ))
            },
            no_follow,
        )
        .expect("reads");
        let cols: Vec<&str> = batch
            .schema
            .columns
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(cols, vec!["room_id", "name"], "shaped to the OF columns");
        assert_eq!(batch.rows.len(), 2);
        assert_eq!(
            batch.rows[0].values,
            vec![Value::Int(1), Value::Text("general".into())],
            "the declared columns carry their delivered VALUES, not nulls"
        );
    }

    #[test]
    fn eval_view_body_refuses_an_of_contract_with_no_columns() {
        // A declared OF type that resolves to ZERO columns (missing row, or a stale pre-§5.4
        // array body) must refuse loudly — shaping to it would silently project every value away
        // (the live zero-column defect, ticket 20260712005100). The fetch must never run.
        let body = serde_json::to_string(
            &qfs_parser::parse_statement("/http/chatwork/rooms |> DECODE json").unwrap(),
        )
        .unwrap();
        let of: Vec<String> = Vec::new();
        let err = eval_view_body(
            &body,
            "chatwork",
            "/chatwork/rooms",
            Some(&of),
            None,
            &[],
            |_path| panic!("the fetch must NOT run for an unreadable OF contract"),
            no_follow,
        )
        .unwrap_err();
        match err {
            CfsError::InvalidPath { reason, .. } => assert!(
                reason.contains("re-install"),
                "the error names the fix: {reason}"
            ),
            other => panic!("expected InvalidPath, got {other:?}"),
        }
    }

    #[test]
    fn eval_view_body_rejects_a_foreign_source_before_any_fetch() {
        let body = serde_json::to_string(
            &qfs_parser::parse_statement("/http/evil/steal |> DECODE json").unwrap(),
        )
        .unwrap();
        let err = eval_view_body(
            &body,
            "slack",
            "/slack/x",
            None,
            None,
            &[],
            |_path| -> Result<RowBatch, CfsError> {
                panic!("the fetch must NOT run for a foreign source");
            },
            no_follow,
        )
        .unwrap_err();
        match err {
            CfsError::InvalidPath { reason, .. } => {
                assert!(
                    reason.contains("confinement"),
                    "confinement reason: {reason}"
                );
            }
            other => panic!("expected InvalidPath, got {other:?}"),
        }
    }

    #[test]
    fn eval_view_body_binds_a_param_into_the_wire_path() {
        let body = serde_json::to_string(
            &qfs_parser::parse_statement("/http/chatwork/rooms/{room}/messages |> DECODE json")
                .unwrap(),
        )
        .unwrap();
        let params = match_template(
            "/chatwork/rooms/{room}/messages",
            "/chatwork/rooms/123/messages",
        )
        .expect("matches");

        let mut fetched_path = None;
        let _batch = eval_view_body(
            &body,
            "chatwork",
            "/chatwork/rooms/123/messages",
            None,
            None,
            &params,
            |path| {
                fetched_path = Some(path.to_string());
                Ok(decode(br#"[{"message_id":"m1","body":"hi"}]"#))
            },
            no_follow,
        )
        .expect("reads");
        assert_eq!(
            fetched_path.as_deref(),
            Some("/rest/chatwork/rooms/123/messages"),
            "the bound {{room}} substituted into the wire path"
        );
    }

    /// Extract the refinement `Expr` a `CREATE TYPE … WHERE …` stores in its body object's `where`
    /// slot — the exact predicate the resolved `DeclaredType.refinement` would carry.
    fn refinement_of(create_type: &str) -> Expr {
        let qfs_parser::Statement::Effect(effect) =
            qfs_parser::parse_statement(create_type).unwrap()
        else {
            panic!("expected an effect");
        };
        let qfs_parser::EffectBody::Values(values) = &effect.body else {
            panic!("expected VALUES");
        };
        let cols = values.columns.as_ref().unwrap();
        let idx = cols.iter().position(|c| c == "body").unwrap();
        let qfs_parser::Expr::Lit(qfs_parser::Literal::Str(body)) = &values.rows[0][idx] else {
            panic!("string body");
        };
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        serde_json::from_value(v.get("where").unwrap().clone()).unwrap()
    }

    #[test]
    fn eval_view_body_enforces_the_of_type_refinement_membership() {
        // §5.4: the `OF` type carries `WHERE user LIKE 'U%'`. A conforming batch (both messages have
        // `U…` users) passes; a batch with a violating row is a structured TypeMembership refusal.
        let body = serde_json::to_string(
            &qfs_parser::parse_statement(
                "/http/slack/conversations.history |> DECODE json |> EXPAND messages",
            )
            .unwrap(),
        )
        .unwrap();
        let of = vec!["ts".to_string(), "user".to_string(), "text".to_string()];
        let refinement = refinement_of("CREATE TYPE msg (user text) WHERE user LIKE 'U%'");

        // Conforming: every delivered `user` matches `U%`.
        let ok = eval_view_body(
            &body,
            "slack",
            "/slack/history",
            Some(&of),
            Some(&refinement),
            &[],
            |_path| {
                Ok(decode(
                    br#"{"ok":true,"messages":[{"ts":"1","user":"U1","text":"hi"}]}"#,
                ))
            },
            no_follow,
        )
        .expect("a conforming row is a member");
        assert_eq!(ok.rows.len(), 1);

        // Violating: `user` = `bot` is not `U%` — a structured membership refusal.
        let err = eval_view_body(
            &body,
            "slack",
            "/slack/history",
            Some(&of),
            Some(&refinement),
            &[],
            |_path| {
                Ok(decode(
                    br#"{"ok":true,"messages":[{"ts":"2","user":"bot","text":"yo"}]}"#,
                ))
            },
            no_follow,
        )
        .unwrap_err();
        match err {
            CfsError::TypeMembership { path, detail } => {
                assert_eq!(path, "/slack/history");
                assert!(
                    detail.contains("user"),
                    "names the constrained column: {detail}"
                );
            }
            other => panic!("expected TypeMembership, got {other:?}"),
        }
    }

    /// Build a one-row incoming write batch (the rows a `VALUES` write carries).
    fn incoming(cols: &[(&str, &str)]) -> RowBatch {
        RowBatch::new(
            Schema::new(
                cols.iter()
                    .map(|(name, _)| Column::new(*name, ColumnType::Text, false))
                    .collect(),
            ),
            vec![Row::new(
                cols.iter()
                    .map(|(_, v)| Value::Text((*v).to_string()))
                    .collect(),
            )],
        )
    }

    #[test]
    fn eval_map_body_shapes_the_wire_body_from_the_incoming_row() {
        // Tier 2 write: `VALUES ({channel: row.channel, text: row.text})` maps an incoming row into
        // the exact `{channel, text}` body Slack's chat.postMessage expects (the tier-1 gap: a bare
        // `VALUES (row)` could only pass the whole row through). The target decouples from the mount.
        let body = serde_json::to_string(
            &qfs_parser::parse_statement(
                "INSERT INTO /http/slack/chat.postMessage VALUES ({channel: row.channel, text: row.text})",
            )
            .unwrap(),
        )
        .unwrap();
        let write = eval_map_body(
            &body,
            "slack",
            "/slack/post",
            &[],
            &incoming(&[("channel", "C1"), ("text", "hi")]),
        )
        .expect("evals");

        // The mount path (`/slack/post`) decoupled from the wire method (`chat.postMessage`).
        assert_eq!(write.rest_path, "/rest/slack/chat.postMessage");
        assert_eq!(write.bodies.len(), 1);
        match &write.bodies[0] {
            Value::Struct(fields) => {
                assert_eq!(fields.get("channel"), Some(&Value::Text("C1".to_string())));
                assert_eq!(fields.get("text"), Some(&Value::Text("hi".to_string())));
            }
            other => panic!("expected a struct wire body, got {other:?}"),
        }
    }

    #[test]
    fn eval_map_body_passes_the_whole_row_through_for_a_bare_row_expr() {
        // The tier-1 form `VALUES (row)`: `row` binds the whole incoming row as a struct, so the
        // wire body is every incoming column verbatim (the passthrough this generalises).
        let body = serde_json::to_string(
            &qfs_parser::parse_statement("INSERT INTO /http/slack/chat.postMessage VALUES (row)")
                .unwrap(),
        )
        .unwrap();
        let write = eval_map_body(
            &body,
            "slack",
            "/slack/post",
            &[],
            &incoming(&[("channel", "C9"), ("text", "yo")]),
        )
        .expect("evals");
        match &write.bodies[0] {
            Value::Struct(fields) => {
                assert_eq!(fields.get("channel"), Some(&Value::Text("C9".to_string())));
                assert_eq!(fields.get("text"), Some(&Value::Text("yo".to_string())));
            }
            other => panic!("expected the whole row as a struct, got {other:?}"),
        }
    }

    #[test]
    fn eval_view_body_follow_performs_the_second_fetch_and_delivers_bytes() {
        // The §13 declared download shape (ticket 20260711121526): metadata GET → FOLLOW the
        // delivered `download_url` → the raw bytes as a one-row `content` batch. The follow
        // closure sees the exact URL the service minted (a FOREIGN host — that is the point).
        let body = serde_json::to_string(
            &qfs_parser::parse_statement(
                "/http/chatwork/rooms/{room}/files/{file}?create_download_url=1 \
                 |> DECODE json |> FOLLOW download_url",
            )
            .unwrap(),
        )
        .unwrap();
        let params = vec![
            ("room".to_string(), "1".to_string()),
            ("file".to_string(), "9".to_string()),
        ];
        let mut followed = None;
        let batch = eval_view_body(
            &body,
            "chatwork",
            "/chatwork/rooms/1/files/9/blob",
            None,
            None,
            &params,
            |path| {
                // The query-string suffix rides behind the substituted {file} template.
                assert_eq!(path, "/rest/chatwork/rooms/1/files/9?create_download_url=1");
                Ok(decode(
                    br#"[{"file_id":9,"download_url":"https://appdata.example.com/tmp/xyz"}]"#,
                ))
            },
            |url| {
                followed = Some(url.to_string());
                Ok(b"FILEBYTES".to_vec())
            },
        )
        .expect("follows");
        assert_eq!(
            followed.as_deref(),
            Some("https://appdata.example.com/tmp/xyz"),
            "the second GET hits the delivered URL"
        );
        assert_eq!(batch.schema.columns.len(), 1);
        assert_eq!(batch.schema.columns[0].name, FOLLOW_CONTENT_COL);
        assert_eq!(
            batch.rows[0].values[0],
            Value::Bytes(b"FILEBYTES".to_vec()),
            "the raw response bytes are the delivered content"
        );
    }

    #[test]
    fn eval_view_body_follow_refuses_ambiguity_and_missing_fields() {
        let body = serde_json::to_string(
            &qfs_parser::parse_statement("/http/chatwork/files |> DECODE json |> FOLLOW url")
                .unwrap(),
        )
        .unwrap();
        // Two delivered rows: which URL? Fail closed, the follow closure never runs.
        let err = eval_view_body(
            &body,
            "chatwork",
            "/chatwork/files",
            None,
            None,
            &[],
            |_| Ok(decode(br#"[{"url":"https://a"},{"url":"https://b"}]"#)),
            |_| panic!("no follow on an ambiguous batch"),
        )
        .unwrap_err();
        match err {
            CfsError::InvalidPath { reason, .. } => {
                assert!(reason.contains("exactly one"), "names the rule: {reason}")
            }
            other => panic!("expected InvalidPath, got {other:?}"),
        }
        // The named field was not delivered at all.
        let err = eval_view_body(
            &body,
            "chatwork",
            "/chatwork/files",
            None,
            None,
            &[],
            |_| Ok(decode(br#"[{"other":"x"}]"#)),
            |_| panic!("no follow without the field"),
        )
        .unwrap_err();
        match err {
            CfsError::InvalidPath { reason, .. } => {
                assert!(
                    reason.contains("not delivered"),
                    "names the field gap: {reason}"
                )
            }
            other => panic!("expected InvalidPath, got {other:?}"),
        }
    }

    #[test]
    fn eval_map_body_carries_the_declared_encode_multipart() {
        // The §13 upload shape: the parser desugared `|> ENCODE multipart VALUES (row)` onto the
        // pipeline body; the evaluator surfaces the encoding on the MapWrite for the applier.
        let body = serde_json::to_string(
            &qfs_parser::parse_statement(
                "INSERT INTO /http/chatwork/rooms/{room}/files |> ENCODE multipart VALUES (row)",
            )
            .unwrap(),
        )
        .unwrap();
        let write = eval_map_body(
            &body,
            "chatwork",
            "/chatwork/rooms/1/files",
            &[("room".to_string(), "1".to_string())],
            &incoming(&[("filename", "a.txt"), ("message", "hi")]),
        )
        .expect("evals");
        assert_eq!(write.rest_path, "/rest/chatwork/rooms/1/files");
        assert_eq!(write.encoding.as_deref(), Some("multipart"));
        assert_eq!(write.bodies.len(), 1);
        // A bare VALUES map carries no encoding (the default JSON stays the default).
        let bare = serde_json::to_string(
            &qfs_parser::parse_statement("INSERT INTO /http/chatwork/rooms VALUES (row)").unwrap(),
        )
        .unwrap();
        let write = eval_map_body(
            &bare,
            "chatwork",
            "/chatwork/rooms",
            &[],
            &incoming(&[("x", "1")]),
        )
        .expect("evals");
        assert_eq!(write.encoding, None);
    }

    #[test]
    fn eval_map_body_rejects_a_foreign_target() {
        // §13 confinement (write side): a map body writing to a FOREIGN `/http/<other>` host is the
        // anti-exfiltration violation — rejected before any body is evaluated.
        let body = serde_json::to_string(
            &qfs_parser::parse_statement("INSERT INTO /http/evil/steal VALUES (row)").unwrap(),
        )
        .unwrap();
        let err = eval_map_body(&body, "slack", "/slack/post", &[], &incoming(&[("x", "1")]))
            .unwrap_err();
        match err {
            CfsError::InvalidPath { reason, .. } => {
                assert!(
                    reason.contains("confinement"),
                    "confinement reason: {reason}"
                )
            }
            other => panic!("expected InvalidPath, got {other:?}"),
        }
    }
}
