//! blueprint §13 — the declared-driver **evaluator** (the half that turns `/sys/drivers` rows into a
//! live mount when connected). The surface ticket (145136) desugared `CREATE DRIVER`/`TYPE`/`VIEW`/
//! `MAP` scripts into `/sys/drivers` rows; this loads those rows back into an in-memory model and
//! reconstructs the shipped `qfs-driver-http` wire config (`RestApiConfig` — a **lift, not an
//! invention**), so a `CONNECT /chatwork TO chatwork` resolves the declared driver into a real mount.
//!
//! ## Two-source registry, compiled wins
//! A `CONNECT … TO <name>` resolves `<name>` against **compiled ∪ declared** drivers. The compiled
//! set is probed first ([`crate::describe::cred_free_driver`]); only an *unknown* compiled name falls
//! through to [`declared_driver`], so a compiled driver always wins a name collision and the shadowed
//! declared driver is reported (never silently shadowed).
//!
//! ## DESCRIBE stays pure
//! A declared driver mounts cred-free for describe (a `MockHttpClient` + an empty secret store, the
//! `cred_free_driver` "rest" arm's shape): `Driver::describe` reads only the static introspective
//! half, so `DESCRIBE /chatwork/…` performs **zero network I/O** — the mock client is never touched.
//!
//! Wire execution, `{param}` view expansion, host confinement, and MAP lowering live in the
//! exec-layer declared evaluator plus the binary read/apply facets; this module is the loader +
//! the two-source describe registration.

use std::sync::Arc;

use qfs_driver_http::{
    AuthStrategy, Pagination, ResourceMap, RestApiConfig, RestDriver, RestVerb, SecretRef,
};
use qfs_secrets::{ConnectionRecord, CredentialKey, Secret, SecretError, Secrets};

/// One declared driver, assembled from its `/sys/drivers` rows: the `kind='driver'` row plus every
/// `kind='view'`/`kind='map'` row whose node path mounts under the driver's name.
#[derive(Debug, Clone)]
pub(crate) struct DeclaredDriver {
    /// The driver name (the `CONNECT … TO <name>` target and the mount's leading segment).
    pub name: String,
    /// The wire base URL (`AT '<url>'`).
    pub base_url: String,
    /// The auth scheme descriptor JSON (never a token) — parsed into an [`AuthStrategy`] on build.
    pub auth: String,
    /// The pagination descriptor JSON, if declared.
    pub pagination: Option<String>,
    /// The declared `SELECT` nodes (views): each maps a mount path to a wire read.
    pub views: Vec<DeclaredNode>,
    /// The declared write/CALL mappings.
    pub maps: Vec<DeclaredMap>,
}

/// A declared view node (`kind='view'`): its mount path, its `OF <type>` contract, and its stored
/// body pipeline (serde JSON of a parsed `Statement`, rehydrated at eval time).
#[derive(Debug, Clone)]
pub(crate) struct DeclaredNode {
    pub path: String,
    // The outward `OF <type>` contract: conformance (§13, 145138) and tier-2 body evaluation
    // (`declared_eval::view_specs`) shape the delivered rows to this type's columns.
    pub of_type: Option<String>,
    pub body: String,
}

/// A declared write/CALL mapping (`kind='map'`): its node path, the mapped verb, the stored wire
/// effect body, and the per-mapping irreversibility flag.
#[derive(Debug, Clone)]
pub(crate) struct DeclaredMap {
    pub path: String,
    pub verb: String,
    pub body: String,
    // `irreversible` (the per-mapping gate flag): a MAP marked IRREVERSIBLE lifts onto the describe
    // mount's resource config (`resources()`), so the planner sets `EffectNode::irreversible` and
    // PREVIEW/COMMIT gate the write like a `REMOVE` (ticket per-map-irreversible-write-facet).
    pub irreversible: bool,
}

/// A project path binding that connects a declared driver to a mount. The optional `secret_ref`
/// comes from `CONNECT <path> TO <driver> SECRET '<ref>'` and is resolved lazily at request time.
#[derive(Debug, Clone)]
pub(crate) struct DeclaredMount {
    pub path: String,
    pub driver: DeclaredDriver,
    pub secret_ref: Option<String>,
    /// The connection's bound account label (`CONNECT … ACCOUNT '<label>'`) — the account an
    /// `AUTH ACCOUNT '<provider>'` driver resolves its live bearer from (`None` → `default`).
    pub account: Option<String>,
}

/// Load every declared driver from the System DB `sys_drivers` table (best-effort, cred-free — a pure
/// local read, no network). Returns an empty list when no System DB resolves (a fresh host has no
/// declared drivers). View/map rows associate to a driver by their path's **leading segment** (a view
/// `/chatwork/rooms` belongs to driver `chatwork`).
pub(crate) fn load_declared_drivers() -> Vec<DeclaredDriver> {
    let Ok(Some(sys)) = crate::store::open_system_db() else {
        return Vec::new();
    };
    let conn = sys.into_db().into_connection();
    let mut drivers = load_from_conn(&conn).unwrap_or_default();
    // §13 host confinement (STRUCTURAL): drop any declared driver whose view/map body addresses a
    // FOREIGN `/http/<x>` wire namespace (`<x>` ≠ its own name) — the anti-exfiltration boundary,
    // enforced at load so a malicious declaration never becomes a live mount. Reported, not silent.
    drivers.retain(|d| {
        let ok = d.confined();
        if !ok {
            tracing::warn!(
                driver = %d.name,
                "declared driver dropped: a view/map body addresses a foreign host (§13 confinement)"
            );
        }
        ok
    });
    drivers
}

/// Row shape read back from `sys_drivers` (mirrors the desugar's columns, plus the rowid the
/// newest-wins resolution keys on).
struct DriverRow {
    id: i64,
    kind: String,
    name: String,
    base_url: Option<String>,
    auth: Option<String>,
    pagination: Option<String>,
    of_type: Option<String>,
    verb: Option<String>,
    body: Option<String>,
    irreversible: bool,
}

fn load_from_conn(conn: &rusqlite::Connection) -> Result<Vec<DeclaredDriver>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT kind, name, base_url, auth, pagination, of_type, verb, body, irreversible, id \
         FROM sys_drivers ORDER BY id",
    )?;
    let rows: Vec<DriverRow> = stmt
        .query_map([], |r| {
            Ok(DriverRow {
                kind: r.get(0)?,
                name: r.get(1)?,
                base_url: r.get(2)?,
                auth: r.get(3)?,
                pagination: r.get(4)?,
                of_type: r.get(5)?,
                verb: r.get(6)?,
                body: r.get(7)?,
                irreversible: r.get::<_, i64>(8)? != 0,
                id: r.get(9)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(assemble(rows))
}

/// Group flat `sys_drivers` rows into per-driver models. A `driver` row seeds a [`DeclaredDriver`];
/// `view`/`map` rows attach to the driver named by their path's leading segment. Rows that name no
/// known driver are dropped (fail-open — one dangling declaration cannot sink the registry).
///
/// **Newest row per `(kind, name, verb)` wins** (owner ruling 2026-07-16), matching
/// `types_from_conn`'s `ORDER BY id DESC`: installs now replace on that key, but a registry from
/// the append era still carries superseded duplicates, and resolving them oldest-first is what
/// silently kept a stale declaration live after a re-install. Ascending id order is preserved
/// among the survivors, so distinct declarations still assemble in install order.
fn assemble(rows: Vec<DriverRow>) -> Vec<DeclaredDriver> {
    let mut newest: std::collections::HashMap<(&str, &str, &str), i64> =
        std::collections::HashMap::new();
    for r in &rows {
        let key = (
            r.kind.as_str(),
            r.name.as_str(),
            r.verb.as_deref().unwrap_or(""),
        );
        let e = newest.entry(key).or_insert(r.id);
        if r.id > *e {
            *e = r.id;
        }
    }
    let survives = |r: &DriverRow| {
        newest[&(
            r.kind.as_str(),
            r.name.as_str(),
            r.verb.as_deref().unwrap_or(""),
        )] == r.id
    };

    let mut drivers: Vec<DeclaredDriver> = rows
        .iter()
        .filter(|r| r.kind == "driver" && survives(r))
        .map(|r| DeclaredDriver {
            name: r.name.clone(),
            base_url: r.base_url.clone().unwrap_or_default(),
            auth: r
                .auth
                .clone()
                .unwrap_or_else(|| r#"{"kind":"none"}"#.to_string()),
            pagination: r.pagination.clone(),
            views: Vec::new(),
            maps: Vec::new(),
        })
        .collect();

    for r in &rows {
        if !survives(r) {
            continue;
        }
        match r.kind.as_str() {
            "view" => {
                if let Some(d) =
                    leading_segment(&r.name).and_then(|seg| find_mut(&mut drivers, seg))
                {
                    d.views.push(DeclaredNode {
                        path: r.name.clone(),
                        of_type: r.of_type.clone(),
                        body: r.body.clone().unwrap_or_default(),
                    });
                }
            }
            "map" => {
                if let Some(d) =
                    leading_segment(&r.name).and_then(|seg| find_mut(&mut drivers, seg))
                {
                    d.maps.push(DeclaredMap {
                        path: r.name.clone(),
                        verb: r.verb.clone().unwrap_or_default(),
                        body: r.body.clone().unwrap_or_default(),
                        irreversible: r.irreversible,
                    });
                }
            }
            // `type` rows are the outward contract a view delivers `OF`; the live-eval half reads
            // them by path. `driver` rows are already seeded above.
            _ => {}
        }
    }
    drivers
}

fn find_mut<'a>(drivers: &'a mut [DeclaredDriver], name: &str) -> Option<&'a mut DeclaredDriver> {
    drivers.iter_mut().find(|d| d.name == name)
}

/// The leading path segment of a node path (`/chatwork/rooms/{room}` → `chatwork`).
fn leading_segment(path: &str) -> Option<&str> {
    path.trim_start_matches('/')
        .split('/')
        .next()
        .filter(|s| !s.is_empty())
}

impl DeclaredDriver {
    /// The wire host this driver is confined to (the host of its `AT` base URL). Used by the
    /// live-eval half's host-confinement guard.
    pub(crate) fn host(&self) -> Option<String> {
        host_of(&self.base_url)
    }

    /// Host confinement (STRUCTURAL, plan/load-time): every declared view/map body may address ONLY
    /// this driver's own `/http/<name>` wire namespace. A body addressing any other `/http/<x>` (a
    /// different service) is the anti-exfiltration violation — an LLM-generated script is
    /// structurally unable to read one service and write to another.
    fn confined(&self) -> bool {
        self.views
            .iter()
            .all(|v| body_confined(&self.name, &v.body))
            && self.maps.iter().all(|m| body_confined(&self.name, &m.body))
    }

    /// Reconstruct the shipped [`RestApiConfig`] this driver declares — a **lift** of the
    /// `sys_drivers` row onto the wire engine. `auth`/`pagination` JSON descriptors map onto the
    /// closed `AuthStrategy`/`Pagination` sums; `resources` are derived from the view/map nodes
    /// (leading segment → the verbs those nodes declare). The auth `SecretRef` points at this
    /// driver's own namespace (the token lives in the account layer, never in the row).
    pub(crate) fn rest_config(&self) -> RestApiConfig {
        let mut config = RestApiConfig::new(self.base_url.clone(), self.resources())
            .with_auth(self.auth_strategy());
        if let Some(p) = self.pagination.as_deref().and_then(parse_pagination) {
            config = config.with_pagination(p);
        }
        // §13 host confinement: pin the wire client to this driver's own declared host, so its
        // pipeline is structurally unable to reach another service (post-pagination/override too).
        if let Some(h) = self.host() {
            config = config.with_allowed_host(h);
        }
        // Some live APIs (GitHub) reject requests carrying no User-Agent; every declared driver
        // identifies itself with the versioned binary UA. driver-http can't compose this (it only
        // knows its own crate version), so the app layer sets it as a default header.
        config.with_header("User-Agent", format!("qfs/{}", crate::version::VERSION))
    }

    fn auth_strategy(&self) -> AuthStrategy {
        let secret_ref = SecretRef::new(self.name.clone(), "default");
        parse_auth(&self.auth, secret_ref)
    }

    /// Aggregate the driver's view/map nodes into `ResourceMap`s keyed by the resource's leading
    /// segment (the segment after the driver name). A view contributes `SELECT`; a map contributes
    /// its mapped verb.
    fn resources(&self) -> Vec<ResourceMap> {
        // (segment, supported verbs, irreversible subset). An IRREVERSIBLE-marked MAP adds its verb
        // to the irreversible subset so the describe mount reports it via `write_irreversible`.
        let mut by_segment: Vec<(String, Vec<RestVerb>, Vec<RestVerb>)> = Vec::new();
        let mut add = |segment: String, verb: RestVerb, irreversible: bool| {
            if let Some(entry) = by_segment.iter_mut().find(|(s, ..)| *s == segment) {
                if !entry.1.contains(&verb) {
                    entry.1.push(verb);
                }
                if irreversible && !entry.2.contains(&verb) {
                    entry.2.push(verb);
                }
            } else {
                let irr = if irreversible { vec![verb] } else { Vec::new() };
                by_segment.push((segment, vec![verb], irr));
            }
        };
        for v in &self.views {
            if let Some(seg) = resource_segment(&self.name, &v.path) {
                add(seg.to_string(), RestVerb::Select, false);
            }
        }
        for m in &self.maps {
            if let (Some(seg), Some(verb)) =
                (resource_segment(&self.name, &m.path), map_verb(&m.verb))
            {
                add(seg.to_string(), verb, m.irreversible);
            }
        }
        by_segment
            .into_iter()
            .map(|(seg, verbs, irr)| ResourceMap::new(seg, verbs).with_irreversible_verbs(irr))
            .collect()
    }
}

/// The resource segment of a node path relative to its driver mount (`chatwork`, `/chatwork/rooms/…`
/// → `rooms`). `None` if the path does not mount under the driver.
fn resource_segment<'a>(driver: &str, path: &'a str) -> Option<&'a str> {
    let rest = path.trim_start_matches('/').strip_prefix(driver)?;
    rest.trim_start_matches('/')
        .split('/')
        .next()
        .filter(|s| !s.is_empty())
}

/// Map a declared map verb label to the wire `RestVerb` (a `CALL …` mapping has no direct verb here).
fn map_verb(verb: &str) -> Option<RestVerb> {
    match verb {
        "INSERT" => Some(RestVerb::Insert),
        "UPSERT" => Some(RestVerb::Upsert),
        "REMOVE" => Some(RestVerb::Remove),
        // SELECT maps are unusual; UPDATE (PATCH) and CALL are out of the wire verb set here.
        _ => None,
    }
}

/// Parse the auth scheme descriptor JSON into an [`AuthStrategy`]. Unknown / oauth2 schemes fall back
/// to `None` for the cred-free/describe path (oauth2 is a §13 park until the consent flow is wired).
fn parse_auth(json: &str, secret_ref: SecretRef) -> AuthStrategy {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return AuthStrategy::None;
    };
    match v.get("kind").and_then(|k| k.as_str()) {
        Some("bearer") => AuthStrategy::Bearer { secret_ref },
        Some("header") => {
            let name = v
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or_default()
                .to_string();
            AuthStrategy::Header { name, secret_ref }
        }
        // `AUTH ACCOUNT '<provider>'` — the bearer is an existing account provider's live credential.
        // The wire coordinate is `(provider, "default")`, a STABLE key the binary's account-backed
        // secrets adapter (bound to the connection's real account at commit) matches and resolves —
        // running an OAuth refresh where the provider needs one. The declaration holds only the
        // provider name; no token, no per-driver SECRET.
        Some("account") => {
            let provider = v
                .get("provider")
                .and_then(|p| p.as_str())
                .unwrap_or_default()
                .to_string();
            AuthStrategy::Account {
                secret_ref: SecretRef::new(provider.clone(), "default"),
                provider,
            }
        }
        _ => AuthStrategy::None,
    }
}

/// Parse the pagination descriptor JSON into a [`Pagination`]. The grammar tags cursor/link; the
/// serde tag for link is `link_header`, so the `"link"` tag is bridged here.
fn parse_pagination(json: &str) -> Option<Pagination> {
    let v = serde_json::from_str::<serde_json::Value>(json).ok()?;
    let max_pages = v
        .get("max_pages")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(1) as u32;
    match v.get("kind").and_then(|k| k.as_str())? {
        "cursor" => {
            let next_field = v.get("next_field")?.as_str()?.to_string();
            let param = v.get("param")?.as_str()?.to_string();
            Some(Pagination::Cursor {
                next_field,
                param,
                max_pages,
            })
        }
        "link" | "link_header" => Some(Pagination::LinkHeader { max_pages }),
        _ => None,
    }
}

/// The host component of a base URL (`https://api.chatwork.com/v2` → `api.chatwork.com`). Best-effort
/// string parse (no url crate dep here): strips the scheme, then takes up to the first `/`, `?`, or
/// port `:`.
pub(crate) fn host_of(base_url: &str) -> Option<String> {
    let after_scheme = base_url
        .split_once("://")
        .map_or(base_url, |(_, rest)| rest);
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(after_scheme);
    let host = authority.split('@').next_back().unwrap_or(authority);
    let host = host.split(':').next().unwrap_or(host);
    (!host.is_empty()).then(|| host.to_string())
}

/// Whether a stored body (serde JSON of a parsed `Statement`) addresses ONLY the driver's own
/// `/http/<driver_name>` wire namespace. An empty body (a type has none) is vacuously confined; an
/// unparseable body, or any `/http/<other>` path, is unconfined (FAIL CLOSED — the anti-exfiltration
/// boundary rejects the untrusted declaration).
fn body_confined(driver_name: &str, body_json: &str) -> bool {
    if body_json.is_empty() {
        return true;
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body_json) else {
        return false;
    };
    json_paths_confined(&v, driver_name)
}

/// Walk a serialized-AST JSON value: every path node (an object carrying a `segments` array of
/// `{name}` objects) whose first segment is `http` must have `<driver_name>` as its second segment.
fn json_paths_confined(v: &serde_json::Value, driver_name: &str) -> bool {
    match v {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::Array(segs)) = map.get("segments") {
                let names: Vec<&str> = segs
                    .iter()
                    .filter_map(|s| s.get("name").and_then(serde_json::Value::as_str))
                    .collect();
                if names.first() == Some(&"http") && names.get(1).copied() != Some(driver_name) {
                    return false;
                }
            }
            map.values()
                .all(|val| json_paths_confined(val, driver_name))
        }
        serde_json::Value::Array(arr) => {
            arr.iter().all(|val| json_paths_confined(val, driver_name))
        }
        _ => true,
    }
}

/// The connect-created mounts whose `driver_id` names a declared (`/sys/drivers`) driver. Compiled
/// names are skipped (compiled wins). Empty when nothing is connected to a declared driver — nothing
/// is pre-mounted.
pub(crate) fn declared_mounts() -> Vec<DeclaredMount> {
    let declared = load_declared_drivers();
    if declared.is_empty() {
        return Vec::new();
    }
    let Ok(Some(sys)) = crate::store::open_system_db() else {
        return Vec::new();
    };
    let conn = sys.into_db().into_connection();
    let bindings = crate::path_binding::db_list_bindings(&conn).unwrap_or_default();
    bindings
        .into_iter()
        .filter(|b| b.alias_of.is_none())
        .filter_map(|b| {
            let id = b.driver_id.as_deref()?;
            // Compiled wins: a name the compiled registry knows is never served by a declaration.
            if crate::describe::cred_free_driver(id).is_some() {
                return None;
            }
            let d = declared.iter().find(|d| d.name == id)?.clone();
            Some(DeclaredMount {
                path: b.path,
                driver: d,
                secret_ref: b.secret_ref,
                account: b.account,
            })
        })
        .collect()
}

/// The mount remap for a declared driver connected at `binding_path` (blueprint §13). The stock
/// `RestDriver` speaks `/rest/<api>/<resource>`, so the declared mount maps `<binding>/<resource>` →
/// `/rest/<name>/<resource>` — the driver name is the synthetic `<api>` segment (ignored by URL
/// resolution, which joins `base_url` + the resource segments). This is what makes a declared mount's
/// capabilities + reads + writes resolve (a single-segment remap would collapse to `/rest/<resource>`
/// and resolve empty capabilities).
pub(crate) fn declared_remap(
    binding_path: &str,
    driver_name: &str,
) -> Option<crate::mount_adapter::MountRemap> {
    crate::mount_adapter::MountRemap::new_prefixed(
        binding_path,
        &format!("/rest/{driver_name}"),
        "rest",
    )
    .ok()
}

/// Build the cred-free **describe** mount for a declared driver connected at `binding_path`: the stock
/// `RestDriver` (MockHttp + empty secrets — describe is pure) wrapped in the `/rest/<name>` remap so
/// `DESCRIBE`/capabilities of `<binding>/<resource>` resolve. Compiled drivers are probed first by the
/// caller, so this is reached only for a declared-only name (compiled wins a collision).
pub(crate) fn declared_describe_mount(
    binding_path: &str,
    d: &DeclaredDriver,
) -> Option<crate::mount_adapter::MountDriver> {
    let json = qfs_core::CodecRegistry::with_builtins()
        .resolve("json")
        .ok()?;
    let driver: Arc<dyn qfs_core::Driver> = Arc::new(RestDriver::new(
        d.rest_config(),
        json,
        Arc::new(qfs_driver_http::MockHttpClient::new()),
        Arc::new(qfs_secrets::InMemoryStore::new()),
    ));
    let remap = declared_remap(binding_path, &d.name)?;
    Some(crate::mount_adapter::MountDriver::with_remap(remap, driver))
}

// ---------------------------------------------------------------------------
// §13 conformance — §5's drift check aimed OUTWARD (blueprint §13, ticket 145138)
// ---------------------------------------------------------------------------

/// A declared type: its `/type/…` path and the column NAMES it declares — the outward contract a
/// declared view delivers `OF`. The set-difference reconciliation below is the SAME machinery as a
/// table's catalog drift (§5), aimed at a service the binary never compiled.
#[derive(Debug, Clone)]
pub struct DeclaredType {
    pub path: String,
    pub columns: Vec<String>,
    /// The optional row-local refinement predicate (blueprint §5.4), parsed back from the body
    /// object's `where` slot. Enforced as per-row MEMBERSHIP at the declared-view `OF` boundary.
    pub refinement: Option<qfs_exec::Expr>,
}

/// Load the declared types (`kind='type'` rows) from `sys_drivers` — a pure local read, no network.
#[must_use]
pub fn load_declared_types() -> Vec<DeclaredType> {
    let Ok(Some(sys)) = crate::store::open_system_db() else {
        return Vec::new();
    };
    let conn = sys.into_db().into_connection();
    types_from_conn(&conn).unwrap_or_default()
}

fn types_from_conn(conn: &rusqlite::Connection) -> Result<Vec<DeclaredType>, rusqlite::Error> {
    // Newest declaration first: a re-installed type (same name, later id) must WIN the by-path
    // lookup in `declared_eval::view_specs`, matching the `ORDER BY id DESC` the describe path
    // already uses — this is what lets `qfs run -f <driver>.qfs` heal a stale pre-§5.4 type row
    // (ticket 20260712005100).
    let mut stmt =
        conn.prepare("SELECT name, body FROM sys_drivers WHERE kind = 'type' ORDER BY id DESC")?;
    let rows = stmt
        .query_map([], |r| {
            let path: String = r.get(0)?;
            let body: Option<String> = r.get(1)?;
            Ok((path, body))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .map(|(path, body)| {
            let body = body.as_deref().unwrap_or("");
            DeclaredType {
                path,
                columns: type_column_names(body),
                refinement: type_refinement(body),
            }
        })
        .collect())
}

/// The column names declared by a `CREATE TYPE` body (blueprint §5.4: a JSON OBJECT with a
/// `columns` array of `{name,type,…}` objects and a `where` predicate slot).
fn type_column_names(body_json: &str) -> Vec<String> {
    serde_json::from_str::<serde_json::Value>(body_json)
        .ok()
        .and_then(|v| v.get("columns").and_then(|c| c.as_array()).cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|c| c.get("name").and_then(|n| n.as_str()).map(String::from))
        .collect()
}

/// Load the declared types (`kind='type'` rows) resolved into the plan-time [`DeclaredTypeDefs`]
/// registry (blueprint §5.6): each type body is resolved to its structural schema + refinement, with
/// named column types resolved against the same catalog. Installed on the engine mounts so a
/// `|> of <name>` assertion resolves its type at plan time (the pure planner/evaluator cannot read
/// the System DB — the exact `transform_defs` pattern). Empty when no System DB resolves, so a named
/// `of` then fails with a structured "unresolved type" error rather than silently passing through.
#[must_use]
pub fn load_declared_type_defs() -> qfs_core::DeclaredTypeDefs {
    let Ok(Some(sys)) = crate::store::open_system_db() else {
        return qfs_core::DeclaredTypeDefs::new();
    };
    let conn = sys.into_db().into_connection();
    type_defs_from_conn(&conn).unwrap_or_default()
}

fn type_defs_from_conn(
    conn: &rusqlite::Connection,
) -> Result<qfs_core::DeclaredTypeDefs, rusqlite::Error> {
    // Newest declaration first (`ORDER BY id DESC`): a re-installed type (same name, later id) must
    // WIN the by-path body lookup, matching `types_from_conn` and the describe path.
    let mut stmt =
        conn.prepare("SELECT name, body FROM sys_drivers WHERE kind = 'type' ORDER BY id DESC")?;
    let rows = stmt
        .query_map([], |r| {
            let path: String = r.get(0)?;
            let body: Option<String> = r.get(1)?;
            Ok((path, body))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    // Body-by-path map for nested named-column resolution; first-seen (newest) wins.
    let mut bodies: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();
    for (path, body) in rows {
        bodies
            .entry(path)
            .or_insert_with(|| body.unwrap_or_default());
    }
    let lookup = |path: &str| bodies.get(path).cloned();
    let mut defs = qfs_core::DeclaredTypeDefs::new();
    for (path, body) in &bodies {
        // A malformed or unresolvable body is skipped (not installed) rather than aborting the whole
        // registry — the named `of` against it then reports `of_type_unresolved`, the honest signal.
        if let Ok(resolved) = qfs_core::ddl::types::resolve_type_def(body, lookup) {
            defs.insert(path.clone(), resolved);
        }
    }
    Ok(defs)
}

/// The refinement predicate declared by a `CREATE TYPE` body's `where` slot, rehydrated to an
/// `Expr` (blueprint §5.4). `None` when the type declared no `WHERE` (the slot is `null`) or the
/// body is malformed — a missing refinement is simply "no membership contract".
fn type_refinement(body_json: &str) -> Option<qfs_exec::Expr> {
    let body: serde_json::Value = serde_json::from_str(body_json).ok()?;
    let where_slot = body.get("where")?;
    if where_slot.is_null() {
        return None;
    }
    serde_json::from_value(where_slot.clone()).ok()
}

/// A conformance report: §5's drift, structured. `missing` = columns the declared type promises but
/// the live service did NOT deliver; `extra` = columns delivered but NOT declared. Empty both = the
/// declared contract conforms to what the service returns — the acceptance test an LLM (and a user)
/// runs after generating a script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConformanceReport {
    pub of_type: String,
    pub missing: Vec<String>,
    pub extra: Vec<String>,
}

impl ConformanceReport {
    /// Whether the declared type conforms exactly to the delivered rows (no drift).
    #[must_use]
    pub fn conforms(&self) -> bool {
        self.missing.is_empty() && self.extra.is_empty()
    }
}

/// Reconcile a declared type's `columns` against the columns a live read actually delivered — the
/// set-difference §5 uses for a table's catalog, aimed outward at the wire. Kept a plain public API
/// (not test-only) so an agent iterating on a generated script can run the same check ad hoc.
#[must_use]
pub fn conformance(
    of_type: &str,
    type_columns: &[String],
    delivered: &qfs_core::RowBatch,
) -> ConformanceReport {
    let delivered_cols: Vec<String> = delivered
        .schema
        .columns
        .iter()
        .map(|c| c.name.to_string())
        .collect();
    ConformanceReport {
        of_type: of_type.to_string(),
        missing: type_columns
            .iter()
            .filter(|c| !delivered_cols.contains(c))
            .cloned()
            .collect(),
        extra: delivered_cols
            .iter()
            .filter(|c| !type_columns.contains(c))
            .cloned()
            .collect(),
    }
}

/// The shared secrets store a live declared driver resolves its auth `SecretRef` through. A
/// `CONNECT ... SECRET '<ref>'` path binding is lifted into the driver's default auth key, so the
/// generated `SecretRef(driver, "default")` can resolve `env:<VAR>` / `vault:<driver>/<conn>` at use
/// time. Without a path-level secret reference, the binary's credential store is used directly.
pub(crate) fn declared_secrets(
    d: &DeclaredDriver,
    secret_ref: Option<&str>,
    account: Option<&str>,
) -> Arc<dyn qfs_secrets::Secrets> {
    let vault: Arc<dyn Secrets> = match crate::connection::open_store_for_commit() {
        Some(store) => Arc::new(store),
        None => Arc::new(qfs_secrets::InMemoryStore::new()),
    };
    // `AUTH ACCOUNT '<provider>'`: the live bearer comes from the shared provider account, not a
    // per-driver SECRET. Resolve the declared coordinate `(provider, "default")` to the vault's
    // stored bearer at `(provider, <connected account>)` — the account-referenced auth the declared
    // model previously lacked. (Providers whose stored credential is a static bearer — github,
    // slack, chatwork, cf — work through this directly.)
    if let Some(provider) = account_auth_provider(&d.auth) {
        let account = account
            .filter(|s| !s.is_empty())
            .unwrap_or("default")
            .to_string();
        return Arc::new(AccountBearerSecrets {
            provider,
            account,
            vault,
        });
    }
    let Some(reference) = secret_ref.filter(|s| !s.is_empty()) else {
        return vault;
    };
    let Ok(connection) = qfs_secrets::ConnectionId::new("default") else {
        return vault;
    };
    Arc::new(DeclaredSecretRefStore {
        expected: CredentialKey::new(qfs_secrets::DriverId(d.name.clone()), connection),
        reference: reference.to_string(),
        vault,
    })
}

/// The provider named by an `AUTH ACCOUNT '<provider>'` descriptor (`{"kind":"account",...}`), or
/// `None` for any other auth kind.
fn account_auth_provider(auth: &str) -> Option<String> {
    let v = serde_json::from_str::<serde_json::Value>(auth).ok()?;
    if v.get("kind").and_then(|k| k.as_str()) != Some("account") {
        return None;
    }
    v.get("provider")
        .and_then(|p| p.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// The [`Secrets`] adapter an `AUTH ACCOUNT '<provider>'` declared driver resolves its bearer
/// through. The declared [`AuthStrategy::Account`] resolves the STABLE coordinate
/// `(provider, "default")`; this adapter matches it and returns the shared provider account's stored
/// bearer at `(provider, <connected account>)`. The token stays in the vault — the declaration and
/// its `/sys/drivers` row carry only the provider name. A missing account fails closed with a
/// structured, account-naming error (never a silent unauthenticated call).
struct AccountBearerSecrets {
    provider: String,
    account: String,
    vault: Arc<dyn Secrets>,
}

impl Secrets for AccountBearerSecrets {
    fn get(&self, key: &CredentialKey) -> Result<Secret, SecretError> {
        let expected = CredentialKey::new(
            qfs_secrets::DriverId(self.provider.clone()),
            qfs_secrets::ConnectionId::new("default")
                .map_err(|e| SecretError::Backend(e.to_string()))?,
        );
        if key != &expected {
            return Err(SecretError::NotFound(key.clone()));
        }
        let account_key = CredentialKey::new(
            qfs_secrets::DriverId(self.provider.clone()),
            qfs_secrets::ConnectionId::new(&self.account)
                .map_err(|e| SecretError::Backend(e.to_string()))?,
        );
        self.vault.get(&account_key).map_err(|_| {
            SecretError::Backend(format!(
                "AUTH ACCOUNT '{p}' has no stored account '{a}' — run `qfs account add {p} {a}` \
                 (the token stays in the vault; the declaration carries only the provider)",
                p = self.provider,
                a = self.account,
            ))
        })
    }

    fn put(&self, _key: &CredentialKey, _value: Secret) -> Result<(), SecretError> {
        Err(SecretError::Backend(
            "AUTH ACCOUNT secrets adapter is read-only".to_string(),
        ))
    }

    fn remove(&self, _key: &CredentialKey) -> Result<(), SecretError> {
        Err(SecretError::Backend(
            "AUTH ACCOUNT secrets adapter is read-only".to_string(),
        ))
    }

    fn list(
        &self,
        driver: Option<&qfs_secrets::DriverId>,
    ) -> Result<Vec<ConnectionRecord>, SecretError> {
        if driver.is_some_and(|driver| driver.0 != self.provider) {
            return Ok(Vec::new());
        }
        self.vault.list(driver)
    }
}

struct DeclaredSecretRefStore {
    expected: CredentialKey,
    reference: String,
    vault: Arc<dyn Secrets>,
}

impl Secrets for DeclaredSecretRefStore {
    fn get(&self, key: &CredentialKey) -> Result<Secret, SecretError> {
        if key != &self.expected {
            return Err(SecretError::NotFound(key.clone()));
        }
        crate::secret_ref::resolve_secret_ref(&self.reference, self.vault.as_ref())
            .map_err(|e| SecretError::Backend(format!("declared driver secret reference: {e}")))
    }

    fn put(&self, _key: &CredentialKey, _value: Secret) -> Result<(), SecretError> {
        Err(SecretError::Backend(
            "declared driver secret reference store is read-only".to_string(),
        ))
    }

    fn remove(&self, _key: &CredentialKey) -> Result<(), SecretError> {
        Err(SecretError::Backend(
            "declared driver secret reference store is read-only".to_string(),
        ))
    }

    fn list(
        &self,
        driver: Option<&qfs_secrets::DriverId>,
    ) -> Result<Vec<ConnectionRecord>, SecretError> {
        if driver.is_some_and(|driver| driver != &self.expected.driver) {
            return Ok(Vec::new());
        }
        self.vault.list(driver)
    }
}

/// A live HTTP transport for one declared driver's wire calls: a reqwest client whose
/// **redirect policy is pinned to the driver's confined host** (blueprint §13 tier 2), so a
/// 30x hop cannot leave the boundary the `send_one` guard enforces — reqwest would otherwise
/// follow the redirect before the guard sees the target.
pub(crate) fn declared_http_client(d: &DeclaredDriver) -> Arc<dyn qfs_driver_http::HttpClient> {
    let hosts = host_of(&d.base_url).map(|h| vec![h]).unwrap_or_default();
    Arc::new(qfs_driver_http::ReqwestClient::with_confined_hosts(
        30, hosts,
    ))
}

/// Build a LIVE `RestDriver` for a declared driver (real transport + the shared secrets store) — the
/// read/apply facets. The reconstructed `RestApiConfig` carries the host-confinement `allowed_hosts`,
/// so its wire pipeline is pinned to its own declared host. Hermetic tests inject a `MockHttpClient`
/// + an in-memory secret store here.
pub(crate) fn live_rest_driver(
    d: &DeclaredDriver,
    client: Arc<dyn qfs_driver_http::HttpClient>,
    secrets: Arc<dyn qfs_secrets::Secrets>,
) -> Option<RestDriver> {
    let json = qfs_core::CodecRegistry::with_builtins()
        .resolve("json")
        .ok()?;
    Some(RestDriver::new(d.rest_config(), json, client, secrets))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    fn base_row(kind: &str, name: &str) -> DriverRow {
        DriverRow {
            id: 0,
            kind: kind.into(),
            name: name.into(),
            base_url: None,
            auth: None,
            pagination: None,
            of_type: None,
            verb: None,
            body: None,
            irreversible: false,
        }
    }

    fn chatwork_driver() -> DeclaredDriver {
        DeclaredDriver {
            name: "chatwork".to_string(),
            base_url: "https://api.chatwork.com/v2".to_string(),
            auth: r#"{"kind":"header","name":"x-chatworktoken"}"#.to_string(),
            pagination: None,
            views: Vec::new(),
            maps: Vec::new(),
        }
    }

    fn ghdecl_account_driver() -> DeclaredDriver {
        DeclaredDriver {
            name: "ghdecl".to_string(),
            base_url: "https://api.github.com".to_string(),
            auth: r#"{"kind":"account","provider":"github"}"#.to_string(),
            pagination: None,
            views: Vec::new(),
            maps: Vec::new(),
        }
    }

    #[test]
    fn auth_account_lifts_to_an_account_strategy_at_the_provider_coordinate() {
        // The `{"kind":"account","provider":"github"}` descriptor lifts to `AuthStrategy::Account`
        // whose coordinate is the SHARED provider account `(github, default)` — NOT the declared
        // driver's own `(ghdecl, default)` namespace. That is what reuses the existing github account.
        let d = ghdecl_account_driver();
        let strategy = parse_auth(&d.auth, SecretRef::new(d.name.clone(), "default"));
        match strategy {
            AuthStrategy::Account {
                provider,
                secret_ref,
            } => {
                assert_eq!(provider, "github");
                assert_eq!(secret_ref, SecretRef::new("github", "default"));
            }
            other => panic!("expected AuthStrategy::Account, got {other:?}"),
        }
    }

    #[test]
    fn account_bearer_secrets_resolves_the_connected_account_and_fails_closed() {
        // The account adapter maps the declared coordinate `(provider, default)` to the vault's
        // stored bearer at `(provider, <connected account>)`, and fails CLOSED (structured, account-
        // naming) when the account is absent — never a silent unauthenticated call.
        let vault = Arc::new(qfs_secrets::InMemoryStore::new());
        let stored = CredentialKey::new(
            qfs_secrets::DriverId("github".to_string()),
            qfs_secrets::ConnectionId::new("work").unwrap(),
        );
        vault.put(&stored, Secret::from("gh-pat-123")).unwrap();

        let adapter = AccountBearerSecrets {
            provider: "github".to_string(),
            account: "work".to_string(),
            vault: vault.clone(),
        };
        // The strategy resolves the stable `(provider, default)` coordinate → the connected account's token.
        let declared_coord = CredentialKey::new(
            qfs_secrets::DriverId("github".to_string()),
            qfs_secrets::ConnectionId::new("default").unwrap(),
        );
        assert_eq!(
            adapter.get(&declared_coord).unwrap().expose_str(),
            Some("gh-pat-123"),
            "the connected github account's bearer resolves at wire time"
        );

        // A different coordinate is not this adapter's account.
        let other = CredentialKey::new(
            qfs_secrets::DriverId("slack".to_string()),
            qfs_secrets::ConnectionId::new("default").unwrap(),
        );
        assert!(matches!(adapter.get(&other), Err(SecretError::NotFound(_))));

        // A missing account fails CLOSED with a structured, account-naming error.
        let missing = AccountBearerSecrets {
            provider: "github".to_string(),
            account: "absent".to_string(),
            vault,
        };
        match missing.get(&declared_coord) {
            Err(SecretError::Backend(msg)) => {
                assert!(msg.contains("github") && msg.contains("absent"));
            }
            other => panic!("expected a closed structured error, got {other:?}"),
        }
    }

    #[test]
    fn declared_secrets_builds_the_account_adapter_for_account_auth() {
        // An account-auth declared driver gets the account-backed adapter (no per-driver SECRET), and
        // resolving through it reaches the connected account's stored bearer.
        let _g = crate::testenv::env_guard();
        let d = ghdecl_account_driver();
        // No commit store in the test env → the adapter is built over an in-memory vault; we assert
        // its SHAPE (account resolution + fail-closed), the resolution itself is covered above.
        let secrets = declared_secrets(&d, None, Some("work"));
        let declared_coord = CredentialKey::new(
            qfs_secrets::DriverId("github".to_string()),
            qfs_secrets::ConnectionId::new("default").unwrap(),
        );
        // No github account is stored in this empty env → fail closed, naming the account.
        match secrets.get(&declared_coord) {
            Err(SecretError::Backend(msg)) => {
                assert!(msg.contains("github") && msg.contains("work"))
            }
            other => {
                panic!("expected a closed structured error for a missing account, got {other:?}")
            }
        }
    }

    #[test]
    fn declared_secret_ref_store_resolves_env_secret_for_default_auth() {
        let _g = crate::testenv::env_guard();
        let var = "QFS_DECLARED_CHATWORK_TOKEN_TEST";
        std::env::set_var(var, "cw-test-token");
        let d = chatwork_driver();
        let secrets = declared_secrets(&d, Some(&format!("env:{var}")), None);
        let key = CredentialKey::new(
            qfs_secrets::DriverId("chatwork".to_string()),
            qfs_secrets::ConnectionId::new("default").unwrap(),
        );
        let got = secrets.get(&key).unwrap();
        assert_eq!(got.expose_str(), Some("cw-test-token"));
        std::env::remove_var(var);
    }

    #[test]
    fn declared_secret_ref_store_rejects_a_different_auth_key() {
        let _g = crate::testenv::env_guard();
        let var = "QFS_DECLARED_CHATWORK_TOKEN_MISMATCH_TEST";
        std::env::set_var(var, "cw-test-token");
        let d = chatwork_driver();
        let secrets = declared_secrets(&d, Some(&format!("env:{var}")), None);
        let key = CredentialKey::new(
            qfs_secrets::DriverId("slack".to_string()),
            qfs_secrets::ConnectionId::new("default").unwrap(),
        );
        let err = secrets.get(&key).unwrap_err();
        assert_eq!(err.code(), "secret_not_found");
        std::env::remove_var(var);
    }

    #[test]
    fn assemble_groups_views_and_maps_under_their_driver() {
        let rows = vec![
            DriverRow {
                base_url: Some("https://api.chatwork.com/v2".into()),
                auth: Some(r#"{"kind":"header","name":"x-chatworktoken"}"#.into()),
                ..base_row("driver", "chatwork")
            },
            DriverRow {
                of_type: Some("/type/chatwork/message".into()),
                body: Some("{\"pipe\":true}".into()),
                ..base_row("view", "/chatwork/rooms/{room}/messages")
            },
            DriverRow {
                verb: Some("INSERT".into()),
                body: Some("{\"effect\":true}".into()),
                irreversible: true,
                ..base_row("map", "/chatwork/rooms/{room}/messages")
            },
            // A view for an UNKNOWN driver is dropped (fail-open), not attached anywhere.
            base_row("view", "/other/thing"),
        ];
        let drivers = assemble(rows);
        assert_eq!(drivers.len(), 1);
        let d = &drivers[0];
        assert_eq!(d.name, "chatwork");
        assert_eq!(d.base_url, "https://api.chatwork.com/v2");
        assert_eq!(d.views.len(), 1);
        assert_eq!(
            d.views[0].of_type.as_deref(),
            Some("/type/chatwork/message")
        );
        assert_eq!(d.views[0].body, "{\"pipe\":true}");
        assert_eq!(d.maps.len(), 1);
        assert_eq!(d.maps[0].verb, "INSERT");
        assert_eq!(d.maps[0].body, "{\"effect\":true}");
        assert!(d.maps[0].irreversible);
    }

    #[test]
    fn host_of_extracts_the_authority() {
        assert_eq!(
            host_of("https://api.chatwork.com/v2").as_deref(),
            Some("api.chatwork.com")
        );
        assert_eq!(
            host_of("http://localhost:8080/x").as_deref(),
            Some("localhost")
        );
        assert_eq!(host_of("api.x.io/p").as_deref(), Some("api.x.io"));
        let d = DeclaredDriver {
            name: "c".into(),
            base_url: "https://h.example/v".into(),
            auth: r#"{"kind":"none"}"#.into(),
            pagination: None,
            views: vec![],
            maps: vec![],
        };
        assert_eq!(d.host().as_deref(), Some("h.example"));
    }

    #[test]
    fn rest_config_lifts_auth_pagination_and_resources() {
        let d = DeclaredDriver {
            name: "chatwork".into(),
            base_url: "https://api.chatwork.com/v2".into(),
            auth: r#"{"kind":"header","name":"x-chatworktoken"}"#.into(),
            pagination: Some(
                r#"{"kind":"cursor","next_field":"next","param":"cursor","max_pages":50}"#.into(),
            ),
            views: vec![DeclaredNode {
                path: "/chatwork/rooms".into(),
                of_type: None,
                body: "{}".into(),
            }],
            maps: vec![DeclaredMap {
                path: "/chatwork/rooms".into(),
                verb: "INSERT".into(),
                body: "{}".into(),
                irreversible: false,
            }],
        };
        let cfg = d.rest_config();
        assert_eq!(cfg.base_url, "https://api.chatwork.com/v2");
        assert!(
            matches!(cfg.auth, AuthStrategy::Header { ref name, .. } if name == "x-chatworktoken")
        );
        assert!(matches!(
            cfg.pagination,
            Pagination::Cursor { max_pages: 50, .. }
        ));
        // One resource `rooms` aggregating SELECT (from the view) and INSERT (from the map).
        assert_eq!(cfg.resources.len(), 1);
        assert_eq!(cfg.resources[0].segment, "rooms");
        assert!(
            cfg.resources[0].supports(RestVerb::Select)
                && cfg.resources[0].supports(RestVerb::Insert)
        );
        // A reversible map leaves the resource ungated.
        assert!(!cfg.resources[0].is_irreversible(RestVerb::Insert));
        // Every declared driver carries the versioned binary User-Agent (GitHub's live API
        // rejects UA-less requests).
        assert!(cfg
            .default_headers
            .iter()
            .any(|(n, v)| n == "User-Agent" && *v == format!("qfs/{}", crate::version::VERSION)));
    }

    #[test]
    fn irreversible_map_lifts_onto_the_resource_config() {
        // An INSERT map marked IRREVERSIBLE and a reversible UPSERT map on a second resource.
        let d = DeclaredDriver {
            name: "slack".into(),
            base_url: "https://slack.example".into(),
            auth: r#"{"kind":"none"}"#.into(),
            pagination: None,
            views: vec![],
            maps: vec![
                DeclaredMap {
                    path: "/slack/post".into(),
                    verb: "INSERT".into(),
                    body: String::new(),
                    irreversible: true,
                },
                DeclaredMap {
                    path: "/slack/notes".into(),
                    verb: "UPSERT".into(),
                    body: String::new(),
                    irreversible: false,
                },
            ],
        };
        let cfg = d.rest_config();
        let post = cfg.resource_for_segment("post").expect("post resource");
        assert!(
            post.is_irreversible(RestVerb::Insert),
            "an IRREVERSIBLE INSERT map gates its verb at plan time"
        );
        let notes = cfg.resource_for_segment("notes").expect("notes resource");
        assert!(
            !notes.is_irreversible(RestVerb::Upsert),
            "a reversible map leaves its verb ungated"
        );
    }

    #[test]
    fn describe_of_a_declared_driver_does_zero_network() {
        let d = DeclaredDriver {
            name: "chatwork".into(),
            base_url: "https://api.chatwork.com/v2".into(),
            auth: r#"{"kind":"none"}"#.into(),
            pagination: None,
            views: vec![DeclaredNode {
                path: "/chatwork/rooms".into(),
                of_type: None,
                body: "{}".into(),
            }],
            maps: vec![],
        };
        let json = qfs_core::CodecRegistry::with_builtins()
            .resolve("json")
            .unwrap();
        let mock = Arc::new(qfs_driver_http::MockHttpClient::new());
        let driver = RestDriver::new(
            d.rest_config(),
            json,
            mock.clone(),
            Arc::new(qfs_secrets::InMemoryStore::new()),
        );
        let _ = qfs_core::DescribeReport::from_driver(&driver, &qfs_core::Path::new("/rest/rooms"))
            .expect("declared driver describes");
        assert!(
            mock.recorded().is_empty(),
            "DESCRIBE of a declared driver must perform zero network I/O"
        );
    }

    /// A chatwork fixture with a `rooms` resource (SELECT view + INSERT map).
    fn chatwork_fixture() -> DeclaredDriver {
        DeclaredDriver {
            name: "chatwork".into(),
            base_url: "https://api.chatwork.com/v2".into(),
            auth: r#"{"kind":"none"}"#.into(),
            pagination: None,
            views: vec![DeclaredNode {
                path: "/chatwork/rooms".into(),
                of_type: None,
                body: String::new(),
            }],
            maps: vec![DeclaredMap {
                path: "/chatwork/rooms".into(),
                verb: "INSERT".into(),
                body: String::new(),
                irreversible: false,
            }],
        }
    }

    fn batch_with_cols(cols: &[&str]) -> qfs_core::RowBatch {
        use qfs_core::{Column, ColumnType, Schema};
        qfs_core::RowBatch::new(
            Schema::new(
                cols.iter()
                    .map(|c| Column::new(*c, ColumnType::Text, true))
                    .collect(),
            ),
            vec![],
        )
    }

    #[test]
    fn conformance_reconciles_a_declared_type_against_delivered_columns() {
        // §5's drift check aimed OUTWARD. Positive: the delivered columns match the declared type.
        let ty = vec![
            "message_id".to_string(),
            "body".to_string(),
            "send_time".to_string(),
        ];
        let ok = conformance(
            "/type/chatwork/message",
            &ty,
            &batch_with_cols(&["message_id", "body", "send_time"]),
        );
        assert!(ok.conforms(), "matching delivered rows conform: {ok:?}");

        // Negative: the live service dropped `send_time` and added `mtime` → structured drift.
        let drift = conformance(
            "/type/chatwork/message",
            &ty,
            &batch_with_cols(&["message_id", "body", "mtime"]),
        );
        assert!(!drift.conforms());
        assert_eq!(drift.missing, vec!["send_time".to_string()]);
        assert_eq!(drift.extra, vec!["mtime".to_string()]);
        assert_eq!(drift.of_type, "/type/chatwork/message");
    }

    #[test]
    fn type_column_names_parses_the_type_body_json() {
        // §5.4: the body is a JSON OBJECT `{"columns":[…],"where":<Expr|null>}`.
        let body = r#"{"columns":[{"name":"message_id","type":"text"},{"name":"body","type":"text"}],"where":null}"#;
        assert_eq!(
            type_column_names(body),
            vec!["message_id".to_string(), "body".to_string()]
        );
        assert!(type_column_names("garbage").is_empty());
        // A `null` `where` slot is "no membership contract".
        assert!(type_refinement(body).is_none());
    }

    #[test]
    fn type_refinement_rehydrates_the_where_predicate() {
        // The `where` slot carries the serialized refinement `Expr` (a `LIKE value '%@%'`, the exact
        // JSON the CREATE TYPE desugar emits); `type_refinement` rehydrates it back to a `Like` node.
        let body = r#"{"columns":[{"name":"value","type":"text","nullable":true,"primary_key":false,"unique":false}],"where":{"Like":{"expr":{"Col":"value"},"pattern":{"Lit":{"Str":"%@%"}}}}}"#;
        let refinement = type_refinement(body).expect("refinement rehydrates");
        assert!(
            matches!(refinement, qfs_exec::Expr::Like { .. }),
            "the refinement is a LIKE predicate, got {refinement:?}"
        );
        // A `null` `where` slot rehydrates to no refinement.
        let bare = r#"{"columns":[{"name":"value","type":"text"}],"where":null}"#;
        assert!(type_refinement(bare).is_none());
    }

    #[test]
    fn body_confinement_rejects_a_foreign_http_host() {
        // The stored body is serde JSON of a parsed Statement; a path node is an object with a
        // `segments` array of `{name}`. A `/http/<own>` body is confined; a `/http/<other>` is not.
        let own =
            r#"{"source":{"segments":[{"name":"http"},{"name":"chatwork"},{"name":"rooms"}]}}"#;
        let foreign =
            r#"{"source":{"segments":[{"name":"http"},{"name":"evil"},{"name":"steal"}]}}"#;
        assert!(body_confined("chatwork", own), "own host is confined");
        assert!(
            !body_confined("chatwork", foreign),
            "a foreign host is rejected"
        );
        assert!(
            body_confined("chatwork", ""),
            "an empty body is vacuously confined"
        );
        assert!(
            !body_confined("chatwork", "not json"),
            "an unparseable body fails closed"
        );
        // A driver whose view body addresses a foreign host is not confined.
        let mut d = chatwork_fixture();
        d.views[0].body = foreign.to_string();
        assert!(
            !d.confined(),
            "a driver with a foreign-host view body is untrusted"
        );
    }

    #[test]
    fn capabilities_resolve_through_the_declared_mount() {
        // The remap fix: a declared mount at `/chatwork` resolves resource `rooms`'s SELECT (view) +
        // INSERT (map). A single-segment remap would resolve EMPTY here (the bug this closes).
        use qfs_core::{Path, Verb};
        let mount = declared_describe_mount("/chatwork", &chatwork_fixture()).expect("mounts");
        let p = Path::new("/chatwork/rooms");
        assert!(
            qfs_core::check_capability(&mount, &p, Verb::Select).is_ok(),
            "SELECT resolves through the declared mount"
        );
        assert!(
            qfs_core::check_capability(&mount, &p, Verb::Insert).is_ok(),
            "INSERT resolves through the declared mount"
        );
    }

    /// Seed the slack/default bearer token into a fresh store so a declared read/write reaches the
    /// wire (hermetic — the injected mock client never touches the network).
    fn seeded_slack_secrets() -> Arc<dyn qfs_secrets::Secrets> {
        use qfs_secrets::Secrets as _;
        let store = qfs_secrets::InMemoryStore::new();
        store
            .put(
                &qfs_secrets::CredentialKey::new(
                    qfs_secrets::DriverId::new("slack"),
                    qfs_secrets::ConnectionId::new("default").unwrap(),
                ),
                qfs_secrets::Secret::from("xoxb-test-token"),
            )
            .unwrap();
        Arc::new(store)
    }

    #[test]
    fn slack_twin_read_is_row_equivalent_to_the_compiled_driver() {
        // The tier-2 acceptance bar (blueprint §13): the DECLARED slack twin's read delivers rows
        // ROW-EQUIVALENT to the COMPILED driver's on the SAME two-page envelope fixture — closing the
        // five tier-1 parity parks (envelope unwrap, nested cursor, weak typing, dotted mount, body
        // shape). What tier 1 could only RECORD as a gap now holds as an equality. Three homogeneous
        // messages arrive across TWO pages via Slack's nested `response_metadata.next_cursor`.
        let msg = |ts: &str, user: &str, text: &str| serde_json::json!({ "ts": ts, "user": user, "text": text });

        // Compiled driver: the MockSlackClient returns the merged messages envelope.
        let compiled = {
            let client = qfs_driver_slack::MockSlackClient::new().with_list(serde_json::json!({
                "messages": [msg("1", "U1", "hi"), msg("2", "U2", "yo"), msg("3", "U3", "hey")]
            }));
            qfs_driver_slack::read_rows(&client, "/slack/acme/#general/messages", None)
                .expect("compiled reads")
        };

        // Declared twin: the tier-2 view (`… |> DECODE json |> EXPAND messages`) over a real
        // two-page envelope, driven through the reconstructed applier (which follows the nested
        // cursor across both pages), then shaped to the 5-column `OF /type/slack/message`.
        let d = DeclaredDriver {
            name: "slack".into(),
            base_url: "https://slack.com/api".into(),
            auth: r#"{"kind":"bearer"}"#.into(),
            pagination: Some(
                r#"{"kind":"cursor","next_field":"response_metadata.next_cursor","param":"cursor","max_pages":50}"#
                    .into(),
            ),
            views: vec![DeclaredNode {
                path: "/slack/history".into(),
                of_type: Some("/type/slack/message".into()),
                body: String::new(),
            }],
            maps: vec![],
        };
        let mock = Arc::new(qfs_driver_http::MockHttpClient::new());
        mock.push_response(qfs_driver_http::HttpResponse::new(
            200,
            br#"{"ok":true,"messages":[{"ts":"1","user":"U1","text":"hi"},{"ts":"2","user":"U2","text":"yo"}],"response_metadata":{"next_cursor":"PAGE2"}}"#.to_vec(),
        ));
        mock.push_response(qfs_driver_http::HttpResponse::new(
            200,
            br#"{"ok":true,"messages":[{"ts":"3","user":"U3","text":"hey"}],"response_metadata":{"next_cursor":""}}"#.to_vec(),
        ));
        let client: Arc<dyn qfs_driver_http::HttpClient> = mock.clone();
        let driver = live_rest_driver(&d, client, seeded_slack_secrets()).expect("live twin");

        let of: Vec<String> = ["ts", "user", "text", "thread_ts", "subtype"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        let view_body = serde_json::to_string(
            &qfs_exec::parse("/http/slack/conversations.history |> DECODE json |> EXPAND messages")
                .unwrap(),
        )
        .unwrap();
        let declared = qfs_exec::declared::eval_view_body(
            &view_body,
            "slack",
            "/slack/history",
            Some(&of),
            None,
            &[],
            |path| {
                qfs_driver_http::rest_read_rows(driver.rest_applier(), path).map_err(|e| {
                    qfs_core::CfsError::InvalidPath {
                        path: path.to_string(),
                        reason: e.code(),
                    }
                })
            },
            |_url| panic!("no FOLLOW stage in this body"),
        )
        .expect("declared reads");

        // Both followed the nested cursor across two pages (the second GET carried `cursor=PAGE2`).
        assert_eq!(
            mock.recorded().len(),
            2,
            "the nested cursor drove a second page"
        );
        assert!(
            mock.recorded()[1].url.contains("cursor=PAGE2"),
            "page 2 carried the nested `response_metadata.next_cursor`: {}",
            mock.recorded()[1].url
        );

        // ROW EQUIVALENCE: same delivered column NAMES + same row VALUES (sorted by ts). Compare
        // names + values ONLY, not type/nullability metadata — the compiled schema pins types while
        // the declared `OF` shaping late-binds them (Unknown/nullable); the tier-2 bar is the
        // DELIVERED ROWS being equal, and homogeneous `{ts,user,text}` messages make thread_ts /
        // subtype `Null` in BOTH (compiled: empty→Null; declared: absent col→Null).
        let names = |b: &qfs_core::RowBatch| {
            b.schema
                .columns
                .iter()
                .map(|c| c.name.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(&declared),
            names(&compiled),
            "same delivered column names"
        );
        let sorted = |b: &qfs_core::RowBatch| {
            let mut rows: Vec<Vec<qfs_core::Value>> =
                b.rows.iter().map(|r| r.values.clone()).collect();
            rows.sort_by(|a, z| format!("{:?}", a.first()).cmp(&format!("{:?}", z.first())));
            rows
        };
        assert_eq!(declared.rows.len(), 3, "three messages across two pages");
        assert_eq!(
            sorted(&declared),
            sorted(&compiled),
            "the declared twin's rows are row-equivalent to the compiled driver's"
        );
    }

    #[test]
    fn slack_twin_post_map_shapes_the_wire_body() {
        // Tier-2 write (park #5 — POST body shape): the declared MAP `VALUES ({channel: row.channel,
        // text: row.text})` maps an incoming row into the EXACT `{channel, text}` body Slack's
        // chat.postMessage expects — asserted on the recorded MockHttp request body. The mount path
        // (`/slack/post`) decoupled from the dotted wire method (`chat.postMessage`) the body names.
        //
        // HONEST write-side parity: the compiled driver additionally stamps a deterministic
        // `client_msg_id` idempotency key (crates/driver-slack `chat.postMessage`) the declarative
        // body does not express — a documented compiled-only refinement, not a conversion gap. The
        // declared MAP faithfully expresses the message's semantic content (channel + text).
        let map_body = serde_json::to_string(
            &qfs_exec::parse(
                "INSERT INTO /http/slack/chat.postMessage VALUES ({channel: row.channel, text: row.text})",
            )
            .unwrap(),
        )
        .unwrap();
        let incoming = qfs_core::RowBatch::new(
            qfs_core::Schema::new(vec![
                qfs_core::Column::new("channel", qfs_core::ColumnType::Text, false),
                qfs_core::Column::new("text", qfs_core::ColumnType::Text, false),
            ]),
            vec![qfs_core::Row::new(vec![
                qfs_core::Value::Text("#general".into()),
                qfs_core::Value::Text("ship it".into()),
            ])],
        );
        let write =
            qfs_exec::declared::eval_map_body(&map_body, "slack", "/slack/post", &[], &incoming)
                .expect("map evaluates");
        assert_eq!(write.rest_path, "/rest/slack/chat.postMessage");

        // Drive the evaluated body through the confined applier and assert the POSTed wire body.
        let d = DeclaredDriver {
            name: "slack".into(),
            base_url: "https://slack.com/api".into(),
            auth: r#"{"kind":"bearer"}"#.into(),
            pagination: None,
            views: vec![],
            maps: vec![DeclaredMap {
                path: "/slack/post".into(),
                verb: "INSERT".into(),
                body: map_body.clone(),
                irreversible: false,
            }],
        };
        let mock = Arc::new(qfs_driver_http::MockHttpClient::new());
        mock.push_response(qfs_driver_http::HttpResponse::new(
            200,
            br#"{"ok":true}"#.to_vec(),
        ));
        let client: Arc<dyn qfs_driver_http::HttpClient> = mock.clone();
        let driver = live_rest_driver(&d, client, seeded_slack_secrets()).expect("live twin");

        use qfs_runtime::SharedApplier as _;
        let node = qfs_core::EffectNode::new(
            qfs_core::NodeId(0),
            qfs_core::EffectKind::Insert,
            qfs_core::Target::new(
                qfs_core::DriverId::new("rest"),
                qfs_core::VfsPath::new(&write.rest_path),
            ),
        )
        .with_args(qfs_driver_http::http_body_args(&write.bodies[0]));
        driver
            .rest_applier()
            .apply_shared(&node)
            .expect("the twin posts");

        let req = &mock.recorded()[0];
        assert_eq!(req.method, qfs_driver_http::HttpMethod::Post);
        assert_eq!(req.url, "https://slack.com/api/chat.postMessage");
        let posted: serde_json::Value =
            serde_json::from_slice(req.body.as_deref().expect("a POST body")).expect("valid JSON");
        assert_eq!(
            posted,
            serde_json::json!({ "channel": "#general", "text": "ship it" }),
            "the MAP shaped the row into the exact chat.postMessage body"
        );
    }

    #[tokio::test]
    async fn declared_driver_reads_and_writes_end_to_end_hermetically() {
        use qfs_core::{DriverId, EffectKind, EffectNode, NodeId, PlanBuilder, Target, VfsPath};
        use qfs_runtime::{CapabilitySet, DriverRegistry, Interpreter};

        let d = chatwork_fixture();
        let mock = Arc::new(qfs_driver_http::MockHttpClient::new());
        mock.push_response(qfs_driver_http::HttpResponse::new(
            200,
            br#"[{"room_id":1},{"room_id":2}]"#.to_vec(),
        ));
        mock.push_response(qfs_driver_http::HttpResponse::new(201, b"{}".to_vec()));
        let client: Arc<dyn qfs_driver_http::HttpClient> = mock.clone();
        let secrets: Arc<dyn qfs_secrets::Secrets> = Arc::new(qfs_secrets::InMemoryStore::new());
        let driver = live_rest_driver(&d, client, secrets).expect("live driver");
        let remap = declared_remap("/chatwork", "chatwork").expect("remap");
        let bridge = qfs_driver_http::rest_apply_driver(&driver);
        let registry = DriverRegistry::new().with(
            remap.outer_id(),
            Arc::new(crate::mount_adapter::MountApplyDriver::new(
                remap,
                Arc::new(bridge),
            )),
        );
        let interp = Interpreter::with_defaults(registry);

        // A READ over `/chatwork/rooms` → GET the DECLARED host + resource path (the remap resolves
        // the resource; the confinement pins the host).
        let mut b = PlanBuilder::new();
        b.push(EffectNode::new(
            NodeId(0),
            EffectKind::Read,
            Target::new(DriverId::new("chatwork"), VfsPath::new("/chatwork/rooms")),
        ));
        let caps = CapabilitySet::none().grant(DriverId::new("chatwork"), &EffectKind::Read);
        let outcome = interp.commit(b.build(), &caps).await.expect("read commits");
        assert!(outcome.is_complete(), "the GET leg applied: {outcome:?}");
        assert_eq!(mock.recorded()[0].url, "https://api.chatwork.com/v2/rooms");

        // A parameterized MAP WRITE (INSERT) over `/chatwork/rooms/42/messages` → POST base + the
        // resource path with the `{room}` segment passed through.
        let mut b2 = PlanBuilder::new();
        b2.push(EffectNode::new(
            NodeId(1),
            EffectKind::Insert,
            Target::new(
                DriverId::new("chatwork"),
                VfsPath::new("/chatwork/rooms/42/messages"),
            ),
        ));
        let caps2 = CapabilitySet::none().grant(DriverId::new("chatwork"), &EffectKind::Insert);
        let out2 = interp
            .commit(b2.build(), &caps2)
            .await
            .expect("write commits");
        assert!(out2.is_complete(), "the POST leg applied: {out2:?}");
        let post = &mock.recorded()[1];
        assert_eq!(post.method, qfs_driver_http::HttpMethod::Post);
        assert_eq!(
            post.url, "https://api.chatwork.com/v2/rooms/42/messages",
            "the {{room}} segment passes through to the wire"
        );
    }

    #[tokio::test]
    async fn declared_map_write_evaluates_the_body_through_the_full_commit_stack() {
        use qfs_core::{
            Column, ColumnType, DriverId, EffectKind, EffectNode, NodeId, PlanBuilder, Row,
            RowBatch, Schema, Target, Value, VfsPath,
        };
        use qfs_runtime::{CapabilitySet, DriverRegistry, Interpreter};

        // The slack twin's post MAP live through the FULL commit stack (interpreter →
        // MountApplyDriver → the §13 write facet `RestApplyDriver` → the confined applier). An
        // INSERT on the MOUNT node `/slack/post` carrying a {channel,text} row must evaluate the
        // map body and POST the shaped `{channel,text}` to the WIRE method chat.postMessage — the
        // mount path decoupled from the wire method (what tier 1 could not do).
        let map_body = serde_json::to_string(
            &qfs_exec::parse(
                "INSERT INTO /http/slack/chat.postMessage VALUES ({channel: row.channel, text: row.text})",
            )
            .unwrap(),
        )
        .unwrap();
        let d = DeclaredDriver {
            name: "slack".into(),
            base_url: "https://slack.com/api".into(),
            auth: r#"{"kind":"bearer"}"#.into(),
            pagination: None,
            views: vec![],
            maps: vec![DeclaredMap {
                path: "/slack/post".into(),
                verb: "INSERT".into(),
                body: map_body,
                irreversible: false,
            }],
        };
        let mock = Arc::new(qfs_driver_http::MockHttpClient::new());
        mock.push_response(qfs_driver_http::HttpResponse::new(
            200,
            br#"{"ok":true}"#.to_vec(),
        ));
        let client: Arc<dyn qfs_driver_http::HttpClient> = mock.clone();
        let driver = live_rest_driver(&d, client, seeded_slack_secrets()).expect("live twin");

        // Wire exactly as `crate::commit` does: the stock bridge, wrapped in the §13 write facet,
        // wrapped in the mount remap.
        let remap = declared_remap("/slack", "slack").expect("remap");
        let bridge = qfs_driver_http::rest_apply_driver(&driver);
        let facet = crate::apply_facets::RestApplyDriver::new(
            Arc::new(bridge),
            "slack".to_string(),
            crate::declared_eval::map_specs(&d),
        );
        let registry = DriverRegistry::new().with(
            remap.outer_id(),
            Arc::new(crate::mount_adapter::MountApplyDriver::new(
                remap,
                Arc::new(facet),
            )),
        );
        let interp = Interpreter::with_defaults(registry);

        let incoming = RowBatch::new(
            Schema::new(vec![
                Column::new("channel", ColumnType::Text, false),
                Column::new("text", ColumnType::Text, false),
            ]),
            vec![Row::new(vec![
                Value::Text("#general".into()),
                Value::Text("ship it".into()),
            ])],
        );
        let mut b = PlanBuilder::new();
        b.push(
            EffectNode::new(
                NodeId(0),
                EffectKind::Insert,
                Target::new(DriverId::new("slack"), VfsPath::new("/slack/post")),
            )
            .with_args(incoming),
        );
        let caps = CapabilitySet::none().grant(DriverId::new("slack"), &EffectKind::Insert);
        let outcome = interp
            .commit(b.build(), &caps)
            .await
            .expect("write commits");
        assert!(
            outcome.is_complete(),
            "the mapped POST applied: {outcome:?}"
        );

        let post = &mock.recorded()[0];
        assert_eq!(post.method, qfs_driver_http::HttpMethod::Post);
        assert_eq!(
            post.url, "https://slack.com/api/chat.postMessage",
            "the map body's wire method, not the mount path"
        );
        let body: serde_json::Value =
            serde_json::from_slice(post.body.as_deref().expect("a POST body")).expect("valid JSON");
        assert_eq!(
            body,
            serde_json::json!({ "channel": "#general", "text": "ship it" }),
            "the facet evaluated the MAP body into the shaped wire object"
        );
    }

    #[tokio::test]
    async fn declared_follow_download_reads_bytes_through_the_read_facet() {
        // The §13 FOLLOW download (ticket 20260711121526) through the REAL read facet: the
        // metadata GET (auth-carrying, own host) delivers a `download_url` on a FOREIGN host;
        // the follow GET hits exactly that URL, carries NO credential, and its raw bytes are
        // the delivered `content` row.
        let blob_body = serde_json::to_string(
            &qfs_exec::parse(
                "/http/chatwork/rooms/{room}/files/{file}?create_download_url=1 \
                 |> DECODE json |> FOLLOW download_url",
            )
            .unwrap(),
        )
        .unwrap();
        let d = DeclaredDriver {
            name: "chatwork".into(),
            base_url: "https://api.chatwork.com/v2".into(),
            auth: r#"{"kind":"header","name":"x-chatworktoken"}"#.into(),
            pagination: None,
            views: vec![DeclaredNode {
                path: "/chatwork/rooms/{room}/files/{file}/blob".into(),
                of_type: None,
                body: blob_body,
            }],
            maps: vec![],
        };
        let mock = Arc::new(qfs_driver_http::MockHttpClient::new());
        mock.push_response(qfs_driver_http::HttpResponse::new(
            200,
            br#"[{"file_id":9,"download_url":"https://appdata.chatwork.com/tmp/xyz?sig=abc"}]"#
                .to_vec(),
        ));
        mock.push_response(qfs_driver_http::HttpResponse::new(
            200,
            b"RAWFILEBYTES".to_vec(),
        ));
        let client: Arc<dyn qfs_driver_http::HttpClient> = mock.clone();
        let secrets = {
            use qfs_secrets::Secrets as _;
            let store = qfs_secrets::InMemoryStore::new();
            store
                .put(
                    &qfs_secrets::CredentialKey::new(
                        qfs_secrets::DriverId::new("chatwork"),
                        qfs_secrets::ConnectionId::new("default").unwrap(),
                    ),
                    qfs_secrets::Secret::from("cw-secret-token"),
                )
                .unwrap();
            let arc: Arc<dyn qfs_secrets::Secrets> = Arc::new(store);
            arc
        };
        let driver = live_rest_driver(&d, client, secrets).expect("live driver");

        let facet = crate::read_facets::RestReadDriver::new(
            driver.rest_applier().clone(),
            "chatwork".to_string(),
            crate::declared_eval::view_specs(&d, &[]),
        );
        let scan = qfs_pushdown::ScanNode {
            source: qfs_pushdown::SourceId::new("chatwork"),
            path: "/rest/chatwork/rooms/1/files/9/blob".into(),
            pushed: qfs_pushdown::PushedQuery::default(),
            schema: qfs_core::Schema::empty(),
        };
        let batch = qfs_exec::ReadDriver::scan(&facet, &scan)
            .await
            .expect("blob view reads");
        assert_eq!(batch.schema.columns[0].name, "content");
        assert_eq!(
            batch.rows[0].values[0],
            qfs_core::Value::Bytes(b"RAWFILEBYTES".to_vec()),
            "the follow GET's raw bytes are the delivered content"
        );

        let recorded = mock.recorded();
        assert_eq!(recorded.len(), 2, "metadata GET + follow GET");
        assert_eq!(
            recorded[0].url, "https://api.chatwork.com/v2/rooms/1/files/9?create_download_url=1",
            "the metadata GET carries the query-string suffix behind the {{file}} template"
        );
        assert!(
            recorded[0]
                .headers
                .iter()
                .any(|(n, _)| n.eq_ignore_ascii_case("x-chatworktoken")),
            "the own-host metadata GET carries the driver credential"
        );
        assert_eq!(
            recorded[1].url, "https://appdata.chatwork.com/tmp/xyz?sig=abc",
            "the follow GET hits exactly the delivered URL (a foreign host)"
        );
        assert!(
            !recorded[1]
                .headers
                .iter()
                .any(|(n, _)| n.eq_ignore_ascii_case("x-chatworktoken")),
            "NO credential leaves the declared host on the follow GET"
        );
    }

    #[tokio::test]
    async fn declared_multipart_upload_posts_the_form_through_the_full_commit_stack() {
        // The §13 ENCODE multipart upload (ticket 20260711121526) through the FULL commit stack
        // (interpreter → mount remap → write facet → confined applier): the map's declared
        // encoding turns the incoming row into a multipart/form-data POST with the
        // boundary-bearing Content-Type header.
        use qfs_core::{
            Column, ColumnType, DriverId, EffectKind, EffectNode, NodeId, PlanBuilder, Row,
            RowBatch, Schema, Target, Value, VfsPath,
        };
        use qfs_runtime::{CapabilitySet, DriverRegistry, Interpreter};

        let map_body = serde_json::to_string(
            &qfs_exec::parse(
                "INSERT INTO /http/chatwork/rooms/{room}/files |> ENCODE multipart VALUES (row)",
            )
            .unwrap(),
        )
        .unwrap();
        let d = DeclaredDriver {
            name: "chatwork".into(),
            base_url: "https://api.chatwork.com/v2".into(),
            auth: r#"{"kind":"none"}"#.into(),
            pagination: None,
            views: vec![],
            maps: vec![DeclaredMap {
                path: "/chatwork/rooms/{room}/files".into(),
                verb: "INSERT".into(),
                body: map_body,
                irreversible: false,
            }],
        };
        let mock = Arc::new(qfs_driver_http::MockHttpClient::new());
        mock.push_response(qfs_driver_http::HttpResponse::new(200, b"{}".to_vec()));
        let client: Arc<dyn qfs_driver_http::HttpClient> = mock.clone();
        let secrets: Arc<dyn qfs_secrets::Secrets> = Arc::new(qfs_secrets::InMemoryStore::new());
        let driver = live_rest_driver(&d, client, secrets).expect("live driver");

        let remap = declared_remap("/chatwork", "chatwork").expect("remap");
        let bridge = qfs_driver_http::rest_apply_driver(&driver);
        let facet = crate::apply_facets::RestApplyDriver::new(
            Arc::new(bridge),
            "chatwork".to_string(),
            crate::declared_eval::map_specs(&d),
        );
        let registry = DriverRegistry::new().with(
            remap.outer_id(),
            Arc::new(crate::mount_adapter::MountApplyDriver::new(
                remap,
                Arc::new(facet),
            )),
        );
        let interp = Interpreter::with_defaults(registry);

        let incoming = RowBatch::new(
            Schema::new(vec![
                Column::new("file", ColumnType::Bytes, false),
                Column::new("filename", ColumnType::Text, false),
                Column::new("message", ColumnType::Text, false),
            ]),
            vec![Row::new(vec![
                Value::Bytes(b"PDFDATA".to_vec()),
                Value::Text("report.pdf".into()),
                Value::Text("monthly".into()),
            ])],
        );
        let mut b = PlanBuilder::new();
        b.push(
            EffectNode::new(
                NodeId(0),
                EffectKind::Insert,
                Target::new(
                    DriverId::new("chatwork"),
                    VfsPath::new("/chatwork/rooms/42/files"),
                ),
            )
            .with_args(incoming),
        );
        let caps = CapabilitySet::none().grant(DriverId::new("chatwork"), &EffectKind::Insert);
        let outcome = interp
            .commit(b.build(), &caps)
            .await
            .expect("upload commits");
        assert!(
            outcome.is_complete(),
            "the multipart POST applied: {outcome:?}"
        );

        let post = &mock.recorded()[0];
        assert_eq!(post.method, qfs_driver_http::HttpMethod::Post);
        assert_eq!(post.url, "https://api.chatwork.com/v2/rooms/42/files");
        let content_type = post
            .headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case("content-type"))
            .map(|(_, v)| v.clone())
            .expect("the POST carries a Content-Type");
        let boundary = content_type
            .strip_prefix("multipart/form-data; boundary=")
            .expect("multipart content type with boundary");
        let body = String::from_utf8_lossy(post.body.as_deref().expect("a POST body")).to_string();
        assert!(
            body.contains("name=\"file\"; filename=\"report.pdf\""),
            "the bytes field is the filename-named file part: {body}"
        );
        assert!(body.contains("PDFDATA"));
        assert!(body.contains("name=\"message\"\r\n\r\nmonthly"));
        assert!(body.ends_with(&format!("--{boundary}--\r\n")));
    }

    // ---------------------------------------------------------------------------
    // Ticket 20260708023259 — the SHIPPED declared /cloudflare driver
    // ---------------------------------------------------------------------------

    /// The `cloudflare` declared driver's shape (token-scoped `zones` + account-scoped `accounts`).
    /// Bodies for param-free paths are the real confined wire pipelines; param paths use the
    /// vacuously-confined empty body (the shipped script's real param bodies are parse-checked in
    /// `shipped_cloudflare_script_installs_statement_for_statement`).
    fn cloudflare_fixture() -> DeclaredDriver {
        let zones_body = serde_json::to_string(
            &qfs_exec::parse("/http/cloudflare/zones |> DECODE json |> EXPAND result").unwrap(),
        )
        .unwrap();
        DeclaredDriver {
            name: "cloudflare".into(),
            base_url: "https://api.cloudflare.com/client/v4".into(),
            auth: r#"{"kind":"bearer"}"#.into(),
            pagination: None,
            views: vec![
                DeclaredNode {
                    path: "/cloudflare/zones".into(),
                    of_type: Some("/type/cloudflare/zone".into()),
                    body: zones_body,
                },
                DeclaredNode {
                    path: "/cloudflare/zones/{zone}/dns_records".into(),
                    of_type: Some("/type/cloudflare/dns_record".into()),
                    body: String::new(),
                },
                DeclaredNode {
                    path: "/cloudflare/accounts/{account}/queues".into(),
                    of_type: None,
                    body: String::new(),
                },
            ],
            maps: vec![DeclaredMap {
                path: "/cloudflare/zones/{zone}/dns_records".into(),
                verb: "INSERT".into(),
                body: String::new(),
                irreversible: false,
            }],
        }
    }

    #[test]
    fn cloudflare_declared_driver_loads_confined_with_two_source_registry() {
        use qfs_core::{Path, Verb};
        let d = cloudflare_fixture();
        // §13 host confinement: every body addresses only /http/cloudflare — so it survives load.
        assert!(
            d.confined(),
            "a cloudflare body must address only /http/cloudflare"
        );

        let cfg = d.rest_config();
        assert!(
            matches!(cfg.auth, AuthStrategy::Bearer { .. }),
            "AUTH BEARER lifts to the bearer strategy"
        );
        // Token-scoped `zones` aggregates SELECT (view) + INSERT (map); account-scoped paths
        // collapse under their leading `accounts` segment.
        let zones = cfg.resource_for_segment("zones").expect("zones resource");
        assert!(
            zones.supports(RestVerb::Select) && zones.supports(RestVerb::Insert),
            "zones supports read + the write-pattern seed"
        );
        assert!(
            cfg.resource_for_segment("accounts").is_some(),
            "the account-scoped resource is present"
        );

        // Cred-free describe: capabilities resolve through the declared mount with ZERO network
        // (the mount is MockHttp-backed; describe reads only the static introspective half).
        let mount = declared_describe_mount("/cloudflare", &d).expect("describe mount");
        assert!(
            qfs_core::check_capability(&mount, &Path::new("/cloudflare/zones"), Verb::Select)
                .is_ok(),
            "SELECT /cloudflare/zones resolves cred-free"
        );

        // §13 two-source registry, compiled wins its own name: the COMPILED /cf coexists and is
        // never shadowed; `cloudflare` is declared-only (no compiled driver of that name), so the
        // declaration is the one that serves the mount.
        assert!(
            crate::describe::cred_free_driver("cf").is_some(),
            "the compiled /cf driver coexists with the declared /cloudflare"
        );
        assert!(
            crate::describe::cred_free_driver("cloudflare").is_none(),
            "no compiled `cloudflare` driver shadows the declaration"
        );

        // No secret ever surfaces from the loaded driver (the token lives in the account layer).
        let dump = format!("{d:?}");
        assert!(!dump.contains("Bearer ") && !dump.to_lowercase().contains("sk-"));
    }

    #[test]
    fn shipped_cloudflare_script_installs_statement_for_statement() {
        // The SHIPPED asset: split like the config splitter (strip `--` trailing + `#` whole-line
        // comments, split on `;`), then assert every statement PARSES on the shipped grammar — the
        // install lands /sys/drivers rows with zero network (the parser crate separately proves each
        // CREATE DRIVER/VIEW/MAP desugars to /sys/drivers).
        let script = qfs_skill::CLOUDFLARE_DRIVER;
        let mut stmts: Vec<String> = Vec::new();
        let mut cur = String::new();
        for raw in script.lines() {
            let line = if raw.trim_start().starts_with('#') {
                ""
            } else {
                raw.split("--").next().unwrap_or("")
            };
            let mut rest = line;
            while let Some(pos) = rest.find(';') {
                cur.push_str(&rest[..pos]);
                if !cur.trim().is_empty() {
                    stmts.push(cur.trim().to_string());
                }
                cur.clear();
                rest = &rest[pos + 1..];
            }
            if !rest.is_empty() {
                cur.push_str(rest);
                cur.push('\n');
            }
        }
        if !cur.trim().is_empty() {
            stmts.push(cur.trim().to_string());
        }

        assert_eq!(
            stmts.len(),
            9,
            "1 driver + 2 types + 5 views + 1 map: {stmts:?}"
        );
        for s in &stmts {
            assert!(
                qfs_exec::parse(s).is_ok(),
                "a shipped cloudflare statement must parse: {s}"
            );
        }
        // Host-confinement floor over the shipped bytes: every /http/ wire reference is
        // /http/cloudflare/ (a foreign host would be dropped at load, so it must never ship).
        assert!(script.contains("/http/cloudflare/"));
        assert_eq!(
            script.matches("/http/").count(),
            script.matches("/http/cloudflare/").count(),
            "every /http/ occurrence addresses the cloudflare host"
        );
    }

    #[test]
    fn shipped_chatwork_script_installs_statement_for_statement() {
        // The SHIPPED Chatwork asset: same install-splitter as the config path (strip `--` trailing +
        // `#` whole-line comments, split on `;`), then assert every EXECUTABLE statement parses on the
        // shipped grammar. The recorded-findings comment block (the file download/upload gaps) rides
        // as `--` comments, so it is stripped and never counted as a statement.
        let script = qfs_skill::CHATWORK_DRIVER;
        let mut stmts: Vec<String> = Vec::new();
        let mut cur = String::new();
        for raw in script.lines() {
            let line = if raw.trim_start().starts_with('#') {
                ""
            } else {
                raw.split("--").next().unwrap_or("")
            };
            let mut rest = line;
            while let Some(pos) = rest.find(';') {
                cur.push_str(&rest[..pos]);
                if !cur.trim().is_empty() {
                    stmts.push(cur.trim().to_string());
                }
                cur.clear();
                rest = &rest[pos + 1..];
            }
            if !rest.is_empty() {
                cur.push_str(rest);
                cur.push('\n');
            }
        }
        if !cur.trim().is_empty() {
            stmts.push(cur.trim().to_string());
        }

        assert_eq!(
            stmts.len(),
            10,
            "1 driver + 3 types + 4 views (incl. the FOLLOW blob) + 2 maps (incl. the multipart \
             upload): {stmts:?}"
        );
        for s in &stmts {
            assert!(
                qfs_exec::parse(s).is_ok(),
                "a shipped chatwork statement must parse: {s}"
            );
        }
        // The API-key auth carries only the header NAME — never a token value.
        assert!(script.contains("AUTH HEADER 'x-chatworktoken'"));
        assert!(!script.contains("Bearer "));
        // Host-confinement floor: every /http/ wire reference addresses the chatwork host only.
        assert!(script.contains("/http/chatwork/"));
        assert_eq!(
            script.matches("/http/").count(),
            script.matches("/http/chatwork/").count(),
            "every /http/ occurrence addresses the chatwork host"
        );
    }

    #[test]
    fn shipped_github_account_script_installs_credential_free_with_account_auth() {
        // The SHIPPED OAuth-style asset (ticket 20260711121534): same install-splitter, then assert
        // every statement parses AND the declaration is credential-free — its auth is an ACCOUNT
        // REFERENCE (`AUTH ACCOUNT 'github'`), never a token, so the /sys/drivers row carries only the
        // provider name.
        let script = qfs_skill::GITHUB_ACCOUNT_DRIVER;
        let mut stmts: Vec<String> = Vec::new();
        let mut cur = String::new();
        for raw in script.lines() {
            let line = if raw.trim_start().starts_with('#') {
                ""
            } else {
                raw.split("--").next().unwrap_or("")
            };
            let mut rest = line;
            while let Some(pos) = rest.find(';') {
                cur.push_str(&rest[..pos]);
                if !cur.trim().is_empty() {
                    stmts.push(cur.trim().to_string());
                }
                cur.clear();
                rest = &rest[pos + 1..];
            }
            if !rest.is_empty() {
                cur.push_str(rest);
                cur.push('\n');
            }
        }
        if !cur.trim().is_empty() {
            stmts.push(cur.trim().to_string());
        }

        assert_eq!(stmts.len(), 5, "1 driver + 2 types + 2 views: {stmts:?}");
        for s in &stmts {
            assert!(
                qfs_exec::parse(s).is_ok(),
                "a shipped github_account statement must parse: {s}"
            );
        }
        // Account-referenced auth: names the provider, never a token/secret/bearer value.
        assert!(script.contains("AUTH ACCOUNT 'github'"));
        assert!(!script.contains("Bearer ") && !script.to_lowercase().contains("secret '"));
        // Host-confinement floor: every /http/ wire reference addresses the ghdecl host only.
        assert!(script.contains("/http/ghdecl/"));
        assert_eq!(
            script.matches("/http/").count(),
            script.matches("/http/ghdecl/").count(),
            "every /http/ occurrence addresses the ghdecl host"
        );
    }

    // ---- ticket 20260712005100: stale pre-§5.4 type rows must not silently drop columns -----

    /// The live defect's premise, locked: a pre-§5.4 type row body (a bare JSON ARRAY of column
    /// objects, as the retired desugar stored it) parses to NO columns under the current object
    /// shape — which `view_specs` encodes as `Some(vec![])` and `eval_view_body` refuses loudly.
    #[test]
    fn stale_pre_54_array_type_body_parses_to_no_columns() {
        let legacy = r#"[{"name":"room_id","nullable":true,"primary_key":true,"type":"int","unique":false}]"#;
        assert!(type_column_names(legacy).is_empty());
        let current = r#"{"columns":[{"name":"room_id","nullable":true,"primary_key":true,"type":"int","unique":false}],"where":null}"#;
        assert_eq!(type_column_names(current), vec!["room_id".to_string()]);
    }

    /// A view whose declared OF type resolves to nothing yields `of_columns: Some(vec![])` — the
    /// loud-refusal encoding — never a silent pass-through and never a panic.
    #[test]
    fn view_specs_encode_an_unresolvable_of_type_as_empty_columns() {
        let d = DeclaredDriver {
            name: "chatwork".into(),
            base_url: "https://api.chatwork.com/v2".into(),
            auth: r#"{"kind":"header","name":"x-chatworktoken"}"#.into(),
            pagination: None,
            views: vec![DeclaredNode {
                path: "/chatwork/rooms".into(),
                of_type: Some("/type/chatwork/room".into()),
                body: "{}".into(),
            }],
            maps: vec![],
        };
        let specs = crate::declared_eval::view_specs(&d, &[]);
        assert_eq!(specs[0].of_columns, Some(Vec::new()));
    }

    /// Re-installing a declaration must HEAL a stale type row: the newest same-name row (highest
    /// id) wins the lookup, matching the describe path's `ORDER BY id DESC`.
    #[test]
    fn types_from_conn_prefers_the_newest_declaration() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE sys_drivers (id INTEGER PRIMARY KEY, kind TEXT, name TEXT, body TEXT);
             INSERT INTO sys_drivers (kind, name, body) VALUES
               ('type', '/type/chatwork/room', '[{\"name\":\"room_id\"}]'),
               ('type', '/type/chatwork/room',
                '{\"columns\":[{\"name\":\"room_id\",\"type\":\"int\",\"nullable\":true,\"primary_key\":true,\"unique\":false},{\"name\":\"name\",\"type\":\"text\",\"nullable\":false,\"primary_key\":false,\"unique\":false}],\"where\":null}');",
        )
        .unwrap();
        let types = types_from_conn(&conn).unwrap();
        let hit = types
            .iter()
            .find(|t| t.path == "/type/chatwork/room")
            .expect("type resolves");
        assert_eq!(
            hit.columns,
            vec!["room_id".to_string(), "name".to_string()],
            "the re-installed (newest) declaration wins over the stale array-body row"
        );
    }

    /// Re-installing must heal EVERY row kind, not just `type`: with duplicate rows on disk
    /// (ascending ids = install order, differing bodies — the shape a real registry accumulated),
    /// the newest row per `(kind, name, verb)` wins assembly. A `view` and a `map` sharing a name
    /// stay distinct, as do two `map`s differing only in verb.
    #[test]
    fn duplicate_declaration_rows_resolve_newest_per_key() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE sys_drivers (
                 id INTEGER PRIMARY KEY, kind TEXT, name TEXT, base_url TEXT, auth TEXT,
                 pagination TEXT, of_type TEXT, verb TEXT, body TEXT, irreversible INTEGER
             );
             INSERT INTO sys_drivers (kind, name, base_url, auth, verb, body, irreversible) VALUES
               ('driver', 'chatwork', 'https://old.example',  '{\"kind\":\"none\"}', NULL, NULL, 0),
               ('view',   '/chatwork/rooms', NULL, NULL, NULL, 'OLD-VIEW', 0),
               ('map',    '/chatwork/rooms/{room}/messages', NULL, NULL, 'INSERT', 'OLD-MAP', 0),
               ('driver', 'chatwork', 'https://new.example',  '{\"kind\":\"none\"}', NULL, NULL, 0),
               ('view',   '/chatwork/rooms', NULL, NULL, NULL, 'NEW-VIEW', 0),
               ('map',    '/chatwork/rooms/{room}/messages', NULL, NULL, 'INSERT', 'NEW-MAP', 0),
               ('map',    '/chatwork/rooms/{room}/messages', NULL, NULL, 'REMOVE', 'OTHER-VERB', 1),
               ('view',   '/chatwork/rooms/{room}/messages', NULL, NULL, NULL, 'VIEW-SHARING-NAME', 0);",
        )
        .unwrap();

        let drivers = load_from_conn(&conn).unwrap();
        assert_eq!(drivers.len(), 1, "one driver entry, not one per install");
        let d = &drivers[0];
        assert_eq!(
            d.base_url, "https://new.example",
            "the re-installed driver row wins"
        );

        let view_bodies: Vec<&str> = d.views.iter().map(|v| v.body.as_str()).collect();
        assert!(
            view_bodies.contains(&"NEW-VIEW") && !view_bodies.contains(&"OLD-VIEW"),
            "the re-installed view body wins: {view_bodies:?}"
        );
        assert!(
            view_bodies.contains(&"VIEW-SHARING-NAME"),
            "a view sharing a map's name is its own declaration: {view_bodies:?}"
        );
        assert_eq!(d.views.len(), 2, "one row per view key: {view_bodies:?}");

        let map_bodies: Vec<&str> = d.maps.iter().map(|m| m.body.as_str()).collect();
        assert!(
            map_bodies.contains(&"NEW-MAP") && !map_bodies.contains(&"OLD-MAP"),
            "the re-installed map body wins: {map_bodies:?}"
        );
        assert!(
            map_bodies.contains(&"OTHER-VERB"),
            "a map differing only in verb is its own declaration: {map_bodies:?}"
        );
        assert_eq!(d.maps.len(), 2, "one row per map key: {map_bodies:?}");
    }
}
