//! The **networked read adapters** — the read counterparts of [`crate::shell::LocalReadDriver`],
//! hosted in the `qfs` binary crate. Each wraps a credentialed driver client behind the async
//! [`qfs_exec::ReadDriver`] seam so a `FROM /github/.../pulls`
//! executes through the read executor, the same way `LocalReadDriver` services `FROM /local/...`.
//!
//! ## Why the adapters live in the BINARY (the same CO-t29-4 topology as the local read facet)
//! `ReadDriver` is a `qfs-exec` type, and the driver crates must stay OFF `qfs-exec` (the
//! dep-direction confinement guard: a `qfs-runtime` consumer must be a leaf). qfs-exec cannot
//! depend on the driver crates either. The binary is the one node that is BOTH an allowlisted
//! runtime consumer AND a terminal sink, so the adapter that bridges the driver's pure
//! `read_rows` into the async `ReadDriver` lives here — exactly like `LocalReadDriver`,
//! `SysReadDriver`, and `ClaudeReadDriver`. The path→plan→fetch→decode logic itself lives INSIDE
//! each driver crate (`qfs_driver_github::read_rows`), so this
//! adapter only owns the async boundary + the error mapping; it never re-derives the read logic.
//!
//! ## Fail closed (the ticket's honesty bar)
//! The adapter is registered (by [`crate::shell::run_engine_and_reads`]) only when the shared
//! [`crate::clients`] builder yields a credentialed client — i.e. the operator is configured and
//! the t54 cloud bind gate passed. When it is registered but the credential cannot be resolved at
//! request time (no token, locked store), the underlying client returns a structured auth error
//! and this adapter surfaces it as a [`CfsError`] carrying the driver's stable secret-free `code`
//! — **never** an empty `RowBatch`, never a panic. The SECRET never crosses this seam (the driver
//! errors are secret-free by construction; the planted-canary tests in each driver assert this).

use std::sync::Arc;

use qfs_core::{
    CfsError, Column, ColumnType, Name, Path, RequestContext, Row, RowBatch, Schema, Value,
};
use qfs_driver_cf::{artifacts_repos_schema, kv_table_schema, CfDriver, CfNode};
use qfs_driver_ga::{GaDriver, QuerySpec as GaQuerySpec};
use qfs_driver_gdrive::GDriveClient;
use qfs_driver_git::{blobfs, relational, GitDriver, GitNode, GitPath};
use qfs_driver_github::GitHubClient;
use qfs_driver_gmail::GmailClient;
use qfs_driver_objstore::{object_listing_schema, ObjDriver, ObjNode};
use qfs_driver_sql::{QuerySpec, SqlDriver};
use qfs_exec::ReadDriver;
use qfs_pushdown::{PushedQuery, ScanNode};

/// Run a BLOCKING cloud read OFF the async executor's thread (EPIC `20260630203030`). Every cloud
/// read facet's client drives the shared reqwest transport via its own `block_on`; calling that from
/// within the async read executor nests tokio runtimes and PANICS ("Cannot start a runtime from
/// within a runtime"). Execute the closure on a DEDICATED OS thread that carries no tokio context
/// (via [`std::thread::scope`], so it may borrow the client + scan by reference), and reduce a panic
/// on that thread to a structured, secret-free [`CfsError`] rather than tearing down the process.
/// (Objstore has an equivalent inline guard in [`ObjReadDriver::scan`]; this is the shared form for
/// the gmail/gdrive/ga/github facets, which all reach the same transport when run LIVE.)
fn read_off_runtime<F>(
    err_path: &str,
    panic_reason: &'static str,
    read: F,
) -> Result<RowBatch, CfsError>
where
    F: FnOnce() -> Result<RowBatch, CfsError> + Send,
{
    std::thread::scope(|s| s.spawn(read).join()).unwrap_or_else(|_| {
        Err(CfsError::InvalidPath {
            path: err_path.to_string(),
            reason: panic_reason,
        })
    })
}

/// Apply a driver-computed **truthful residual** — the part of the pushed `WHERE` the backend could
/// not express exactly — over a facet's returned rows (t20 over-fetch-then-filter). A `None`
/// residual means the backend's own query already means exactly the predicate, so the batch is
/// left untouched.
///
/// This is what a facet declaring [`qfs_exec::ReadDriver::honors_pushed_filter`] owes: it opts out
/// of the executor's blanket re-filter (because it narrows the row shape to a pushed projection, or
/// accepts backend search pseudo-columns the described schema does not carry), so it must enforce
/// the predicate itself. Facets that do NOT declare it need nothing here — the executor re-applies
/// the whole pushed predicate for them.
fn apply_driver_residual(batch: RowBatch, residual: Option<qfs_types::Predicate>) -> RowBatch {
    match residual {
        Some(p) => qfs_exec::apply_residual(batch, &p),
        None => batch,
    }
}

/// Refuse a pushed `WHERE` that names a column the returned rows do not carry and the backend does
/// not know as a **search pseudo-column** — the unknown-column check for a facet that declares
/// [`qfs_exec::ReadDriver::honors_pushed_filter`] and therefore never passes through the executor's
/// checked seam.
///
/// Without it, a typo'd column falls into the driver's residual, matches no row, and the query
/// answers `rows: []` at exit 0 — indistinguishable from an honest miss (ticket 20260717180100).
/// `search_columns` is the driver's own list of names it filters on that no row carries, so a
/// legitimate `fullText`/`label` predicate is not mistaken for a typo.
fn refuse_unknown_where_column(
    batch: &RowBatch,
    predicate: Option<&qfs_types::Predicate>,
    search_columns: &[&str],
    path: &str,
) -> Result<(), CfsError> {
    let Some(p) = predicate else {
        return Ok(());
    };
    qfs_exec::check_where_columns(&batch.schema, p, search_columns).map_err(|_| {
        CfsError::InvalidPath {
            path: path.to_string(),
            reason: "unknown_column",
        }
    })
}

/// Apply the WHOLE pushed `WHERE` over a facet's returned rows — the eager form for a backend that
/// expresses no part of it and whose every predicate column is a real, described column
/// (`/github`). The read executor applies the same predicate again for these facets (they do not
/// declare [`qfs_exec::ReadDriver::honors_pushed_filter`]), so this narrows at the seam that knows
/// the fetch while the executor remains the guarantee. Idempotent; a `None` predicate is a no-op.
fn apply_pushed_filter(batch: RowBatch, predicate: Option<&qfs_types::Predicate>) -> RowBatch {
    match predicate {
        Some(p) => qfs_exec::apply_residual(batch, p),
        None => batch,
    }
}

/// The GitHub read facet: adapts [`qfs_driver_github::read_rows`] (the pure-then-I/O
/// path→plan→fetch→decode composition) to qfs-exec's async [`ReadDriver`] seam. Owns the
/// credentialed [`GitHubClient`] the shared builder constructed; no vendor type crosses the seam —
/// only the owned [`ScanNode`] in and the owned [`RowBatch`] out.
pub struct GitHubReadDriver {
    client: Arc<dyn GitHubClient>,
}

impl GitHubReadDriver {
    /// Build the read adapter over an injected credentialed client.
    #[must_use]
    pub fn new(client: Arc<dyn GitHubClient>) -> Self {
        Self { client }
    }
}

#[async_trait::async_trait]
impl ReadDriver for GitHubReadDriver {
    async fn scan(&self, scan: &ScanNode, _ctx: &RequestContext) -> Result<RowBatch, CfsError> {
        // The ScanNode carries the full addressed VFS path (t28 pushdown threading) + the pushed
        // predicate; the driver's read_rows owns the parse → ReadPlan → list → decode composition.
        let predicate = scan.pushed.filter.as_ref();
        // Off the async runtime: the client's reqwest transport drives its own `block_on` (t203030).
        read_off_runtime(&scan.path, "github_read_panicked", || {
            let batch = qfs_driver_github::read_rows(self.client.as_ref(), &scan.path, predicate)
                .map_err(|e| {
                // A networked read failure (auth/transport/API/decode/path) becomes a
                // structured, secret-free CfsError carrying the driver's stable code.
                CfsError::InvalidPath {
                    path: scan.path.clone(),
                    reason: e.code(),
                }
            })?;
            // GitHub's list APIs express only a few filters as query params; an arbitrary `WHERE`
            // (e.g. `number == 5`) has none, so the driver over-returns. Every predicate column
            // here is a real, described column, so applying the WHOLE pushed predicate is exact.
            // The read executor re-applies it too (this facet does not declare
            // `honors_pushed_filter`) — that is the backstop for every facet that forgets; this
            // call narrows eagerly, at the seam that knows the fetch.
            Ok(apply_pushed_filter(batch, predicate))
        })
    }
}

/// The §13 declared-driver read facet (blueprint tier 2): reading a declared mount **evaluates the
/// view's stored body** — the confined `/http/<self>/…` source is fetched over the reconstructed
/// [`qfs_driver_http::RestApiConfig`], the body's remaining pipe ops run through the shipped engine,
/// and the declared `OF` type shapes the rows (see [`crate::declared_eval`]). Host confinement + auth
/// ride INSIDE the applier (`send_one`); no vendor type crosses the seam. The `ScanNode` path arrives
/// remapped to `/rest/<name>/<mount-resource>` (via [`crate::mount_adapter::MountReadDriver`]); the
/// facet recovers the mount-relative view path, matches it against its declared views (binding
/// `{param}` segments), and evaluates that view's body — so the mount path is decoupled from the wire
/// endpoint (the body names the wire).
pub struct RestReadDriver {
    applier: qfs_driver_http::RestApplier,
    driver_name: String,
    views: Vec<qfs_exec::declared::ViewSpec>,
}

impl RestReadDriver {
    /// Build the read adapter over the declared driver's reconstructed applier plus its resolved
    /// view specs (mount-path template, stored body, `OF`-type columns).
    #[must_use]
    pub(crate) fn new(
        applier: qfs_driver_http::RestApplier,
        driver_name: String,
        views: Vec<qfs_exec::declared::ViewSpec>,
    ) -> Self {
        Self {
            applier,
            driver_name,
            views,
        }
    }
}

#[async_trait::async_trait]
impl ReadDriver for RestReadDriver {
    async fn scan(&self, scan: &ScanNode, _ctx: &RequestContext) -> Result<RowBatch, CfsError> {
        // Off the async runtime: the live reqwest transport drives its own `block_on` (t203030).
        read_off_runtime(&scan.path, "rest_read_panicked", || {
            // Recover the mount-relative view path and match it against the declared views, binding
            // any `{param}` segments. A path matching no declared view is a structured invalid path.
            let view_path = qfs_exec::declared::view_path_of_scan(&scan.path);
            let (spec, params) = self
                .views
                .iter()
                .find_map(|v| {
                    qfs_exec::declared::match_template(&v.template, &view_path)
                        .map(|params| (v, params))
                })
                .ok_or_else(|| CfsError::InvalidPath {
                    path: scan.path.clone(),
                    reason: "no declared view matches this path",
                })?;
            // qfs-exec owns the parser/engine-dependent evaluation (the binary stays off the lower
            // spine); the driver-specific wire read is injected here as a closure over the confined
            // applier — the leading `DECODE` is the applier's own codec.
            // §13.1 G2: lower the pushed predicate/limit through the view's DECLARED pushdown map.
            // What the map calls EXACT drops out of the residual; a PREFILTER entry is pushed AND
            // kept, and anything unmatched stays wholly residual — re-applied locally below, so the
            // facet returns exactly the pushed query's result (the `SqlReadDriver` discipline).
            let lowered = spec.pushdown.as_ref().map(|map| {
                qfs_exec::declared::lower_declared_pushdown(
                    map,
                    scan.pushed.filter.as_ref(),
                    scan.pushed.limit.and_then(|n| i64::try_from(n).ok()),
                )
            });
            let wire_params = lowered.as_ref().map_or(&[][..], |l| l.params.as_slice());
            let mut batch = qfs_exec::declared::eval_view_body(
                &spec.body,
                &self.driver_name,
                &view_path,
                spec.of_columns.as_deref(),
                spec.of_refinement.as_ref(),
                &params,
                wire_params,
                |rest_path, post_body| {
                    // §13.1 G1: a `Some` post_body is a declared read-over-POST — POST the encoded
                    // wire body and decode the response; `None` is the ordinary GET read.
                    let result = match post_body {
                        Some(body) => {
                            qfs_driver_http::rest_read_rows_post(&self.applier, rest_path, &body)
                        }
                        None => qfs_driver_http::rest_read_rows(&self.applier, rest_path),
                    };
                    result.map_err(|e| CfsError::InvalidPath {
                        path: rest_path.to_string(),
                        reason: e.code(),
                    })
                },
                // The §13 FOLLOW second fetch: raw bytes off the delivered URL, no auth, the
                // URL's host data-admitted for exactly this request (applier::follow_bytes).
                |url| {
                    self.applier
                        .follow_bytes(url)
                        .map_err(|e| CfsError::InvalidPath {
                            path: view_path.clone(),
                            reason: e.code(),
                        })
                },
            )?;
            // A pushed WHERE left the local plan, so the facet must deliver exactly the pushed
            // query's result: re-filter the declared map's truthful residual over the fetched rows.
            if let Some(residual) = lowered.as_ref().and_then(|l| l.residual.as_ref()) {
                batch = qfs_exec::apply_residual(batch, residual);
            }
            // The REST pushdown declares LIMIT (and, for a view with a declared PUSHDOWN map, WHERE);
            // enforce the pushed cap locally too — a declared wire `limit` parameter is a page size,
            // not a promise the service returns at most that many rows.
            if let Some(limit) = scan.pushed.limit {
                batch
                    .rows
                    .truncate(usize::try_from(limit).unwrap_or(usize::MAX));
            }
            Ok(batch)
        })
    }
}

/// The SQL read facet: adapts [`SqlDriver::execute_query`] (compile the pushed
/// projection/`WHERE`/`ORDER BY`/`LIMIT` into ONE native parameterized `SELECT`, run it, return the
/// rows + the residual predicate SQL could not faithfully render) to qfs-exec's async [`ReadDriver`]
/// seam. This is the "filters push **into** the database" path — the native `SELECT` does the
/// pushable work; the residual (e.g. a `LIKE`/regex the dialect can't express exactly) is re-filtered
/// locally via [`qfs_exec::apply_residual`] so the returned rows are exactly the pushed query's
/// result before the engine runs the remaining cross-source residual. Unlike the cloud facets this
/// is hermetic against a SQLite file — no network, no credential.
pub struct SqlReadDriver {
    driver: Arc<SqlDriver>,
}

/// The Cloudflare read facet for `/cf` D1/KV/Queues.
pub struct CfReadDriver {
    driver: Arc<CfDriver>,
}

impl SqlReadDriver {
    /// Build the read adapter over a live [`SqlDriver`] (its connection registry already
    /// introspected the catalog).
    #[must_use]
    pub fn new(driver: Arc<SqlDriver>) -> Self {
        Self { driver }
    }
}

impl CfReadDriver {
    /// Build the read adapter over a live [`CfDriver`].
    #[must_use]
    pub fn new(driver: Arc<CfDriver>) -> Self {
        Self { driver }
    }
}

/// Translate the planner's owned [`PushedQuery`] into the SQL compiler's [`QuerySpec`] — the pushed
/// projection (column names), `WHERE` predicate, `ORDER BY`, and `LIMIT` the native `SELECT` runs.
fn query_spec_from_pushed(pushed: &PushedQuery) -> QuerySpec {
    let projection = pushed.project.as_ref().map_or_else(Vec::new, |cols| {
        cols.iter().map(|c| c.as_str().to_string()).collect()
    });
    let mut spec = QuerySpec::new(projection);
    if let Some(predicate) = &pushed.filter {
        spec = spec.with_predicate(predicate.clone());
    }
    for order in &pushed.order {
        spec = spec.order_by(order.column.as_str(), order.descending);
    }
    if let Some(limit) = pushed.limit {
        spec = spec.with_limit(i64::try_from(limit).unwrap_or(i64::MAX));
    }
    spec
}

/// Narrow `batch` to exactly the columns in `cols` (the pushed projection), in that order — the
/// facet's job once a projection was pushed (no local Project op remains to do it). Columns absent
/// from the batch are skipped (the SELECT is the source of truth; a missing column never panics).
fn project_batch(batch: &RowBatch, cols: &[Name]) -> RowBatch {
    let picks: Vec<usize> = cols
        .iter()
        .filter_map(|name| {
            batch
                .schema
                .columns
                .iter()
                .position(|c| c.name.as_str() == name.as_str())
        })
        .collect();
    let schema = Schema::new(
        picks
            .iter()
            .map(|i| batch.schema.columns[*i].clone())
            .collect(),
    );
    let rows = batch
        .rows
        .iter()
        .map(|r| {
            Row::new(
                picks
                    .iter()
                    .map(|i| r.values.get(*i).cloned().unwrap_or(Value::Null))
                    .collect(),
            )
        })
        .collect();
    RowBatch::new(schema, rows)
}

/// The object-storage read facet (t-203070): adapts the in-house [`ObjDriver`]'s `ls` (native S3
/// `list_objects_v2` with prefix/delimiter pushdown) and `get` (streaming download) to qfs-exec's
/// async [`ReadDriver`] seam. A `/<scheme>/<bucket>` (or prefix) node LISTS its objects into the
/// canonical [`object_listing_schema`]; a concrete `/<scheme>/<bucket>/<key>` node DOWNLOADS that
/// object's bytes into a one-row `content` batch (mirroring the local/git/drive content reads, so
/// `… |> decode <fmt>` works). The live SigV4 backend is injected (built by `crate::commit`); no
/// credential or vendor type crosses this seam.
pub struct ObjReadDriver {
    driver: Arc<ObjDriver>,
}

impl ObjReadDriver {
    /// Build the read adapter over a live [`ObjDriver`] (its [`qfs_driver_objstore::ObjRegistry`]
    /// carries the SigV4 [`qfs_driver_objstore::HttpBackend`] for the configured bucket).
    #[must_use]
    pub fn new(driver: Arc<ObjDriver>) -> Self {
        Self { driver }
    }
}

/// The single-row object-content batch: `key` + the raw bytes under the well-known `content` column
/// (the name the engine's `DECODE` reads, matching the local/git/drive content reads).
fn object_content_batch(key: &str, bytes: Vec<u8>) -> RowBatch {
    let schema = Schema::new(vec![
        Column::new("key", ColumnType::Text, false),
        Column::new("content", ColumnType::Bytes, true),
    ]);
    let row = Row::new(vec![Value::Text(key.to_string()), Value::Bytes(bytes)]);
    RowBatch::new(schema, vec![row])
}

/// S3/R2 accept **no** backend search pseudo-column: `list_objects_v2` filters by key prefix and
/// nothing else, and every column a listing row carries is in [`object_listing_schema`]. The empty
/// list is therefore the honest argument to [`refuse_unknown_where_column`] — stated explicitly so
/// a future backend filter (a tag/metadata query) has an obvious place to declare itself instead of
/// being smuggled in as a silent drop.
const OBJ_SEARCH_COLUMNS: &[&str] = &[];

/// The same for `/cf`'s KV / queue / artifact branches: those APIs take a key prefix or a count cap
/// and no filter expression, so every predicate column must be a real, described column.
const CF_SEARCH_COLUMNS: &[&str] = &[];

/// The blocking object-storage read: parse the node, then LIST (bucket) or DOWNLOAD (object). The
/// SigV4 backend's reqwest transport drives its own runtime via `block_on`, so the caller runs this
/// off the async executor's thread (see [`ObjReadDriver::scan`]).
fn obj_scan(
    driver: &ObjDriver,
    path_str: &str,
    filter: Option<&qfs_core::Predicate>,
    limit: Option<u64>,
) -> Result<RowBatch, CfsError> {
    let invalid = |reason: &'static str| CfsError::InvalidPath {
        path: path_str.to_string(),
        reason,
    };
    let path = Path::new(path_str);
    match ObjNode::parse(&path).map_err(|e| invalid(e.code()))? {
        // A concrete object key downloads its content (one `content` row). The pushed `WHERE` is
        // enforced HERE (this facet declares `honors_pushed_filter`, so the executor stands back):
        // before, a predicate on a concrete key was neither pushed nor applied anywhere, so
        // `/s3/<bucket>/<key> |> where key == '<other>'` DELIVERED the row it excluded.
        ObjNode::Object { key, .. } => {
            let bytes = driver
                .get(&path, None)
                .map_err(|e| invalid(e.code()))?
                .into_bytes();
            let batch = object_content_batch(&key, bytes);
            refuse_unknown_where_column(&batch, filter, OBJ_SEARCH_COLUMNS, path_str)?;
            Ok(match filter {
                Some(predicate) => qfs_exec::apply_residual(batch, predicate),
                None => batch,
            })
        }
        // A bucket/prefix lists objects (native prefix/delimiter pushdown + truthful residual).
        ObjNode::Bucket { .. } => {
            let pushdown = ObjDriver::plan_ls(filter, None);
            let (page, residual) = driver
                .ls(&path, &pushdown, None)
                .map_err(|e| invalid(e.code()))?;
            let batch = RowBatch::new(object_listing_schema(), page.to_rows());
            // The listing schema is fully described and S3 has NO search pseudo-column, so a
            // predicate naming anything else is a typo: refuse it rather than let it fall into the
            // residual, match no object, and answer `rows: []` at exit 0 (the defect this mission
            // exists to kill — the `honors_pushed_filter` opt-out bypasses the executor's check).
            refuse_unknown_where_column(&batch, filter, OBJ_SEARCH_COLUMNS, path_str)?;
            let mut batch = match residual {
                Some(predicate) => qfs_exec::apply_residual(batch, &predicate),
                None => batch,
            };
            if let Some(limit) = limit {
                batch
                    .rows
                    .truncate(usize::try_from(limit).unwrap_or(usize::MAX));
            }
            Ok(batch)
        }
        // The mount root (no bucket) is not a readable node.
        _ => Err(invalid("objstore_root_not_readable")),
    }
}

#[async_trait::async_trait]
impl ReadDriver for ObjReadDriver {
    async fn scan(&self, scan: &ScanNode, _ctx: &RequestContext) -> Result<RowBatch, CfsError> {
        let path_str = scan.path.clone();
        let filter = scan.pushed.filter.clone();
        let limit = scan.pushed.limit;
        let err_path = scan.path.clone();
        // The SigV4 backend's reqwest transport drives its OWN runtime via `block_on`; running it
        // inside the async read executor would nest runtimes (a panic). Execute the blocking read on
        // a DEDICATED OS thread that has no tokio context. (Every cloud read facet shares this
        // transport — objstore is the first wired to run live; the others need the same when used
        // live, relevant to the EPIC `20260630203030` live verification.)
        let joined = std::thread::scope(|s| {
            s.spawn(|| obj_scan(&self.driver, &path_str, filter.as_ref(), limit))
                .join()
        });
        match joined {
            Ok(result) => result,
            Err(_) => Err(CfsError::InvalidPath {
                path: err_path,
                reason: "objstore_read_panicked",
            }),
        }
    }

    /// `obj_scan` applies the driver's truthful `plan_ls` residual over the listed objects (and the
    /// whole predicate over a downloaded object's one row), so the returned rows already satisfy
    /// the pushed `WHERE` exactly — and it makes the executor's refusal itself first
    /// ([`refuse_unknown_where_column`]), so opting out of the checked seam does not reintroduce
    /// the silent drop it exists to prevent.
    fn honors_pushed_filter(&self) -> bool {
        true
    }
}

#[async_trait::async_trait]
impl ReadDriver for SqlReadDriver {
    async fn scan(&self, scan: &ScanNode, _ctx: &RequestContext) -> Result<RowBatch, CfsError> {
        let spec = query_spec_from_pushed(&scan.pushed);
        let path = Path::new(&scan.path);
        // `execute_query` returns the rows, the truthful residual, AND the schema the SELECT actually
        // produced: the (residual-expanded) projection when a projection pushed, else the full catalog
        // schema. The driver KEEPS every column the residual reads, so the residual re-filter below is
        // always sound; the projection narrow (last) drops the extra columns again.
        let (rows, residual, out_schema) =
            self.driver
                .execute_query(&path, &spec)
                .map_err(|e| CfsError::InvalidPath {
                    path: scan.path.clone(),
                    reason: e.code(),
                })?;
        let batch = RowBatch::new(out_schema, rows);
        // The driver applied the faithfully-renderable part natively; re-filter the residual locally
        // so the rows are exactly the pushed query's result. This facet OWNS that enforcement
        // (`honors_pushed_filter` below): it narrows the batch to the pushed projection, so the
        // executor's blanket re-filter could not see the predicate's columns to re-check them.
        let mut batch = match residual {
            Some(predicate) => qfs_exec::apply_residual(batch, &predicate),
            None => batch,
        };
        // Enforce the pushed `LIMIT` AFTER the residual re-filter. `compile` emits a native SQL
        // `LIMIT` only when nothing is residual (else it would under-fetch); when a residual remained
        // the backend returned the unlimited set, so the cap is applied here. A no-op when the
        // backend already limited (it returned ≤ limit rows).
        if let Some(limit) = scan.pushed.limit {
            batch
                .rows
                .truncate(usize::try_from(limit).unwrap_or(usize::MAX));
        }
        // A pushed PROJECTION leaves NO local Project op in the plan, so the facet must deliver
        // exactly the requested columns. The SELECT may have over-fetched residual columns (above), so
        // narrow to the requested projection LAST — after the residual re-filter has used them.
        if let Some(project) = &scan.pushed.project {
            batch = project_batch(&batch, project);
        }
        Ok(batch)
    }

    /// The native `SELECT` runs the faithfully-renderable part of the `WHERE` and `scan` re-filters
    /// the compiler's truthful residual above — before narrowing to the pushed projection, which is
    /// exactly why the executor cannot re-check this facet from outside (a `where` on a column the
    /// `SELECT` does not return would find no column to compare).
    fn honors_pushed_filter(&self) -> bool {
        true
    }
}

fn cf_scan(driver: &CfDriver, scan: &ScanNode) -> Result<RowBatch, CfsError> {
    let invalid = |reason: &'static str| CfsError::InvalidPath {
        path: scan.path.clone(),
        reason,
    };
    let path = Path::new(&scan.path);
    match CfNode::parse(&path).map_err(|e| invalid(e.code()))? {
        // D1 needs no check HERE: `compile` validates the whole `WHERE` against the table catalog
        // and returns a structured `unknown_column` before any row is fetched (sql-core's
        // `compile`), so the refusal already happens one layer down. The branches below have no
        // such compiler and get the check inline.
        CfNode::D1Table { db, table } => {
            let spec = query_spec_from_pushed(&scan.pushed);
            let (rows, residual) = driver
                .execute_d1_query(&path, &spec)
                .map_err(|e| invalid(e.code()))?;
            let schema = driver
                .registry()
                .d1(&db)
                .and_then(|handle| handle.table(&table, &scan.path))
                .map(|table| table.describe_schema())
                .map_err(|e| invalid(e.code()))?;
            let mut batch = RowBatch::new(schema, rows);
            if let Some(predicate) = residual {
                batch = qfs_exec::apply_residual(batch, &predicate);
            }
            if let Some(project) = &scan.pushed.project {
                batch = project_batch(&batch, project);
            }
            Ok(batch)
        }
        CfNode::KvNamespace { ns } => {
            let limit = scan.pushed.limit.and_then(|n| u32::try_from(n).ok());
            let keys = driver
                .kv_list_keys(&ns, None, limit)
                .map_err(|e| invalid(e.code()))?;
            let rows = keys
                .into_iter()
                .map(|key| Row::new(vec![Value::Text(key), Value::Null]))
                .collect();
            let mut batch = RowBatch::new(kv_table_schema(), rows);
            // `kv_list_keys` takes no predicate, so the WHOLE pushed `WHERE` is unpushed here.
            // Apply it before the projection narrows the columns away — this facet declares
            // `honors_pushed_filter`, so the executor will not do it for us. CHECKED first: an
            // unknown column would otherwise match no row and answer `rows: []` at exit 0.
            refuse_unknown_where_column(
                &batch,
                scan.pushed.filter.as_ref(),
                CF_SEARCH_COLUMNS,
                &scan.path,
            )?;
            if let Some(predicate) = &scan.pushed.filter {
                batch = qfs_exec::apply_residual(batch, predicate);
            }
            if let Some(project) = &scan.pushed.project {
                batch = project_batch(&batch, project);
            }
            Ok(batch)
        }
        CfNode::KvKey { ns, key } => {
            let rows = driver
                .kv_get(&ns, &key)
                .map_err(|e| invalid(e.code()))?
                .map(|entry| vec![entry.to_kv_row()])
                .unwrap_or_default();
            let mut batch = RowBatch::new(kv_table_schema(), rows);
            refuse_unknown_where_column(
                &batch,
                scan.pushed.filter.as_ref(),
                CF_SEARCH_COLUMNS,
                &scan.path,
            )?;
            if let Some(predicate) = &scan.pushed.filter {
                batch = qfs_exec::apply_residual(batch, predicate);
            }
            if let Some(project) = &scan.pushed.project {
                batch = project_batch(&batch, project);
            }
            Ok(batch)
        }
        // Consumer PULL retired to the DECLARED `cloudflare.qfs` read-over-POST view (blueprint
        // §13.1 G1 / §13.3 — the honest-tiering exception closed). The compiled `/cf` queue is
        // append-only now, so a read is a structured refusal that names where the surface moved,
        // never an empty batch. (`capabilities` already denies SELECT; this is defense in depth.)
        CfNode::Queue { .. } => Err(invalid(
            "the compiled /cf queue is append-only; read a queue through the declared \
             /cloudflare/accounts/<account>/queues/<queue>/messages/pull view",
        )),
        CfNode::Artifacts => {
            let rows = driver
                .artifact_repos()
                .map_err(|e| invalid(e.code()))?
                .into_iter()
                .map(|repo| repo.to_row())
                .collect();
            let mut batch = RowBatch::new(artifacts_repos_schema(), rows);
            refuse_unknown_where_column(
                &batch,
                scan.pushed.filter.as_ref(),
                CF_SEARCH_COLUMNS,
                &scan.path,
            )?;
            if let Some(predicate) = &scan.pushed.filter {
                batch = qfs_exec::apply_residual(batch, predicate);
            }
            if let Some(limit) = scan.pushed.limit {
                batch
                    .rows
                    .truncate(usize::try_from(limit).unwrap_or(usize::MAX));
            }
            if let Some(project) = &scan.pushed.project {
                batch = project_batch(&batch, project);
            }
            Ok(batch)
        }
        CfNode::ArtifactRepo { namespace, name } => {
            let rows = driver
                .artifact_repo(&namespace, &name)
                .map_err(|e| invalid(e.code()))?
                .map(|repo| vec![repo.to_row()])
                .unwrap_or_default();
            let mut batch = RowBatch::new(artifacts_repos_schema(), rows);
            refuse_unknown_where_column(
                &batch,
                scan.pushed.filter.as_ref(),
                CF_SEARCH_COLUMNS,
                &scan.path,
            )?;
            if let Some(predicate) = &scan.pushed.filter {
                batch = qfs_exec::apply_residual(batch, predicate);
            }
            if let Some(project) = &scan.pushed.project {
                batch = project_batch(&batch, project);
            }
            Ok(batch)
        }
        _ => Err(invalid("cf_node_not_readable")),
    }
}

#[async_trait::async_trait]
impl ReadDriver for CfReadDriver {
    async fn scan(&self, scan: &ScanNode, _ctx: &RequestContext) -> Result<RowBatch, CfsError> {
        read_off_runtime(&scan.path, "cf_read_panicked", || {
            cf_scan(&self.driver, scan)
        })
    }

    /// Every `cf_scan` branch enforces the pushed `WHERE` itself — the D1 branch via the SQL
    /// compiler's truthful residual, the KV / queue / artifact branches by applying the whole
    /// predicate — and each does so BEFORE narrowing to the pushed projection, which is why the
    /// executor cannot re-check this facet from outside. Each branch also makes the refusal the
    /// executor would have made: D1 through the compiler's catalog validation, the others through
    /// [`refuse_unknown_where_column`], so an unknown column is refused, never matched by nothing.
    fn honors_pushed_filter(&self) -> bool {
        true
    }
}

/// The history-node fetch cap: relational reads (`commits`/`changes`/`blame`) need a concrete
/// bound, so the facet fetches up to this many rows and lets the engine residual apply the real
/// `WHERE`/`LIMIT` (the git pushdown profile declares nothing pushable — correctness over
/// optimization, mirroring the SQL facet). Generous enough for ordinary histories.
const GIT_READ_CAP: usize = 10_000;

/// An honest read facet for a cloud source whose reads fundamentally need a live, authenticated
/// account (mail / drive / analytics / object stores). It always fails with a clear, actionable
/// reason — "connect your account" — instead of leaving the source UNREGISTERED, which surfaces the
/// internal-sounding `unknown_source` ("no read driver registered") to a fresh user (the t5 honesty
/// fix). When the real networked read facet lands (t6/t7) it is registered over this one for a
/// credentialed operator; until then every reader gets the same actionable nudge, never empty rows.
pub struct ConnectAccountReadDriver {
    reason: &'static str,
}

impl ConnectAccountReadDriver {
    /// Build the facet with a service-specific actionable `reason` (a stable `&'static str` that
    /// the executor renders as the error message — secret-free, machine-legible).
    #[must_use]
    pub fn new(reason: &'static str) -> Self {
        Self { reason }
    }
}

#[async_trait::async_trait]
impl ReadDriver for ConnectAccountReadDriver {
    async fn scan(&self, scan: &ScanNode, _ctx: &RequestContext) -> Result<RowBatch, CfsError> {
        Err(CfsError::InvalidPath {
            path: scan.path.clone(),
            reason: self.reason,
        })
    }
}

/// The Gmail read facet (t7): adapts [`qfs_driver_gmail::read_rows`] (parse the `/mail/<label>` or
/// `/mail/drafts` path → search the label's message ids → fetch each into the canonical
/// `MailMessage` rows) to the async [`ReadDriver`] seam — the structural twin of [`GitHubReadDriver`]
/// over the credentialed [`GmailClient`]. Network: the composition is proven hermetically by
/// driver-gmail's mock-client test; a real read needs a live OAuth account (registered over the
/// connect-account fallback only when the operator is connected and the bind gate passes).
pub struct GmailReadDriver {
    client: Arc<dyn GmailClient>,
}

impl GmailReadDriver {
    /// Build the read adapter over an injected credentialed [`GmailClient`].
    #[must_use]
    pub fn new(client: Arc<dyn GmailClient>) -> Self {
        Self { client }
    }
}

#[async_trait::async_trait]
impl ReadDriver for GmailReadDriver {
    async fn scan(&self, scan: &ScanNode, _ctx: &RequestContext) -> Result<RowBatch, CfsError> {
        let predicate = scan.pushed.filter.as_ref();
        // Off the async runtime: the credentialed GmailClient drives the shared reqwest transport's
        // own `block_on`, which would nest tokio runtimes on the executor thread (t203030).
        read_off_runtime(&scan.path, "gmail_read_panicked", || {
            let batch = qfs_driver_gmail::read_rows(
                self.client.as_ref(),
                &scan.path,
                predicate,
                scan.pushed.limit,
            )
            .map_err(|e| CfsError::InvalidPath {
                path: scan.path.clone(),
                reason: e.code(),
            })?;
            // Gmail's `q=` renders only part of a `WHERE` exactly (`label:`/`is:unread`); `from:`,
            // `subject:`, `after:`/`before:` are LOOSER substring/date-granular matches, and the
            // driver reports precisely that lossy part as its residual. Apply it here so the rows
            // are exactly the predicate's result — before, the residual was computed and thrown
            // away, and a lossy match returned rows the `WHERE` did not select.
            refuse_unknown_where_column(
                &batch,
                predicate,
                qfs_driver_gmail::query::SEARCH_COLUMNS,
                &scan.path,
            )?;
            Ok(apply_driver_residual(
                batch,
                qfs_driver_gmail::query::unpushed_residual(predicate),
            ))
        })
    }

    /// The facet applies Gmail's own truthful residual (above). The executor cannot re-check this
    /// one from outside: a Gmail `WHERE` may name **search pseudo-columns** the message schema does
    /// not carry (`label`, `is_unread`), and a `date` bound is compared against the driver's
    /// **coerced** epoch-ms literal rather than the date string the caller wrote.
    fn honors_pushed_filter(&self) -> bool {
        true
    }
}

/// The Google Drive read facet: adapts [`qfs_driver_gdrive::read_rows`] (parse the `/drive/...` path
/// → walk folder names to Drive file ids → list the resolved folder's children into `FileMeta`
/// rows) to the async [`ReadDriver`] seam — the structural twin of [`GmailReadDriver`] over the
/// credentialed [`GDriveClient`]. Hermetically proven by driver-gdrive's mock-client walk test; a
/// real read needs a live OAuth account (registered over the connect-account fallback only when the
/// operator is connected and the bind gate passes).
pub struct DriveReadDriver {
    client: Arc<dyn GDriveClient>,
}

impl DriveReadDriver {
    /// Build the read adapter over an injected credentialed [`GDriveClient`].
    #[must_use]
    pub fn new(client: Arc<dyn GDriveClient>) -> Self {
        Self { client }
    }
}

#[async_trait::async_trait]
impl ReadDriver for DriveReadDriver {
    async fn scan(&self, scan: &ScanNode, _ctx: &RequestContext) -> Result<RowBatch, CfsError> {
        let predicate = scan.pushed.filter.as_ref();
        read_off_runtime(&scan.path, "gdrive_read_panicked", || {
            let batch = qfs_driver_gdrive::read_rows(self.client.as_ref(), &scan.path, predicate)
                .map_err(|e| CfsError::InvalidPath {
                path: scan.path.clone(),
                reason: e.code(),
            })?;
            // Drive's `q` renders `name =`, `mimeType =`, `trashed =` and a parent scope exactly;
            // EVERY other predicate — `id ==` above all, plus `LIKE`/`OR`/`IN`/`BETWEEN` and the
            // second-granular `modifiedTime` bound — is unpushed or lossy, and the driver reports
            // it as the residual. Applying it here is the fix for the reported defect: a
            // `/drive/<folder> |> where id == '<absent>'` used to return the COMPLETE unfiltered
            // listing at exit 0, because the residual was computed and then discarded.
            refuse_unknown_where_column(
                &batch,
                predicate,
                qfs_driver_gdrive::query::SEARCH_COLUMNS,
                &scan.path,
            )?;
            Ok(apply_driver_residual(
                batch,
                qfs_driver_gdrive::query::unpushed_residual(predicate),
            ))
        })
    }

    /// The facet applies Drive's own truthful residual (above). The executor cannot re-check this
    /// one from outside: a Drive `WHERE` may name **search pseudo-columns** the file schema does
    /// not carry (`text`/`full_text` → `fullText contains`, `parent` → `'<id>' in parents`).
    fn honors_pushed_filter(&self) -> bool {
        true
    }
}

/// The Google Analytics read facet: adapts [`GaDriver::execute_query`] (parse the `/ga/<property>`
/// path → fetch the property catalog → compile the pushed projection/`WHERE`/`ORDER BY`/`LIMIT` into
/// one GA4 `runReport` → project the response onto its typed schema) to qfs-exec's async
/// [`ReadDriver`] seam. GA is the two-step (catalog + report) analog of the SQL facet; the residual
/// (a `contains`/regex GA cannot express exactly) is re-filtered locally. Hermetically proven by
/// driver-ga's mock-client tests; a real read needs a live OAuth account.
pub struct GaReadDriver {
    driver: Arc<GaDriver>,
}

impl GaReadDriver {
    /// Build the read adapter over a live [`GaDriver`] (its injected client carries the auth).
    #[must_use]
    pub fn new(driver: Arc<GaDriver>) -> Self {
        Self { driver }
    }
}

/// Translate the planner's owned [`PushedQuery`] into the GA compiler's [`GaQuerySpec`] — the pushed
/// projection (dimension/metric names), `WHERE`, `ORDER BY`, and `LIMIT` the `runReport` runs.
fn ga_query_spec_from_pushed(pushed: &PushedQuery) -> GaQuerySpec {
    let projection = pushed.project.as_ref().map_or_else(Vec::new, |cols| {
        cols.iter().map(|c| c.as_str().to_string()).collect()
    });
    let mut spec = GaQuerySpec::new(projection);
    if let Some(predicate) = &pushed.filter {
        spec = spec.with_predicate(predicate.clone());
    }
    for order in &pushed.order {
        spec = spec.order_by(order.column.as_str(), order.descending);
    }
    if let Some(limit) = pushed.limit {
        spec = spec.with_limit(i64::try_from(limit).unwrap_or(i64::MAX));
    }
    spec
}

#[async_trait::async_trait]
impl ReadDriver for GaReadDriver {
    async fn scan(&self, scan: &ScanNode, _ctx: &RequestContext) -> Result<RowBatch, CfsError> {
        let spec = ga_query_spec_from_pushed(&scan.pushed);
        // Off the async runtime: the GA client drives the shared reqwest transport's own `block_on`
        // (the GA4 `runReport` call), which would nest tokio runtimes on the executor thread (t203030).
        read_off_runtime(&scan.path, "ga_read_panicked", || {
            let path = Path::new(&scan.path);
            let (rows, schema, residual) =
                self.driver
                    .execute_query(&path, &spec)
                    .map_err(|e| CfsError::InvalidPath {
                        path: scan.path.clone(),
                        reason: e.code(),
                    })?;
            let batch = RowBatch::new(schema, rows);
            let mut batch = match residual {
                Some(predicate) => qfs_exec::apply_residual(batch, &predicate),
                None => batch,
            };
            // Enforce the pushed `LIMIT` after the residual re-filter (GA's `compile` emits a native
            // `limit` only when nothing is residual, mirroring the SQL facet — see its `scan`).
            if let Some(limit) = scan.pushed.limit {
                batch
                    .rows
                    .truncate(usize::try_from(limit).unwrap_or(usize::MAX));
            }
            Ok(batch)
        })
    }

    /// `scan` re-filters GA's truthful residual above. The executor cannot re-check this facet from
    /// outside: a GA4 `runReport` returns exactly the requested dimensions/metrics, so the rows are
    /// already narrowed to the pushed projection and a filtered-on dimension may not be among them
    /// — and adding one would change the report's aggregation granularity, not just its columns.
    fn honors_pushed_filter(&self) -> bool {
        true
    }
}

/// Build a [`RowBatch`] from typed DTO rows via their `schema()` + `to_row()` (the git relational
/// nodes: `commits`/`changes`/`refs`/`tags`/`reflog`/`blame`).
fn dto_batch<T>(schema: Schema, rows: &[T], to_row: impl Fn(&T) -> Row) -> RowBatch {
    RowBatch::new(schema, rows.iter().map(to_row).collect())
}

/// The git read facet: adapts the in-house object reader (`relational` history nodes +
/// `blobfs` tree/blob reads, ADR-0003 — no `gix`) to qfs-exec's async [`ReadDriver`] seam. Reads a
/// repository's commits / changes / refs / tags / reflog / blame and versioned-tree listings at any
/// `@<ref>` coordinate. Hermetic against a local `.git` (no network); the engine residual applies
/// `WHERE`/projection/`LIMIT`.
pub struct GitReadDriver {
    driver: Arc<GitDriver>,
}

impl GitReadDriver {
    /// Build the read adapter over a live [`GitDriver`] (its [`qfs_driver_git::RepoResolver`] maps
    /// each `/git/<repo>` segment to a resolved repository).
    #[must_use]
    pub fn new(driver: Arc<GitDriver>) -> Self {
        Self { driver }
    }
}

#[async_trait::async_trait]
impl ReadDriver for GitReadDriver {
    async fn scan(&self, scan: &ScanNode, _ctx: &RequestContext) -> Result<RowBatch, CfsError> {
        let invalid = |reason: &'static str| CfsError::InvalidPath {
            path: scan.path.clone(),
            reason,
        };
        let gp = GitPath::parse(&scan.path).map_err(|e| invalid(e.code()))?;
        let repo = self
            .driver
            .repos()
            .repo(&gp.repo)
            .map_err(|e| invalid(e.code()))?;
        let r = gp.reference.as_str();
        use qfs_driver_git::{BlameRow, ChangeRow, CommitRow, RefRow, ReflogRow};
        let batch = match &gp.node {
            GitNode::Commits => {
                let rows =
                    relational::commits(repo, r, GIT_READ_CAP).map_err(|e| invalid(e.code()))?;
                dto_batch(CommitRow::schema(), &rows, CommitRow::to_row)
            }
            GitNode::Changes => {
                let rows =
                    relational::changes(repo, r, GIT_READ_CAP).map_err(|e| invalid(e.code()))?;
                dto_batch(ChangeRow::schema(), &rows, ChangeRow::to_row)
            }
            GitNode::Refs => {
                let rows = relational::refs(repo);
                dto_batch(RefRow::schema(), &rows, RefRow::to_row)
            }
            GitNode::Tags => {
                let rows = relational::tags(repo);
                dto_batch(RefRow::schema(), &rows, RefRow::to_row)
            }
            GitNode::Reflog => {
                let rows = relational::reflog(repo, r);
                dto_batch(ReflogRow::schema(), &rows, ReflogRow::to_row)
            }
            GitNode::Blame { file } => {
                let rows =
                    blobfs::blame(repo, r, file, GIT_READ_CAP).map_err(|e| invalid(e.code()))?;
                dto_batch(BlameRow::schema(), &rows, BlameRow::to_row)
            }
            // A blob PATH addresses either a file (→ a `content` row) or a directory (→ the tree
            // listing); `blobfs::read` dispatches on which at read time (a file path resolved to a
            // blob node that `ls` alone rejected with `invalid_path`).
            GitNode::Blob { path } => blobfs::read(repo, r, path).map_err(|e| invalid(e.code()))?,
            GitNode::Root => blobfs::ls(repo, r, "").map_err(|e| invalid(e.code()))?,
            // GitNode is #[non_exhaustive]: a future node kind has no read wiring yet.
            _ => return Err(invalid("unsupported_git_node")),
        };
        Ok(batch)
    }
}

#[cfg(test)]
mod tests {
    //! Hermetic adapter tests — no socket, no real credential. The happy path drives the adapter
    //! over each driver's in-memory MOCK client (proving the async seam threads the path + predicate
    //! through `read_rows` and returns the decoded rows). The fail-closed path drives the adapter
    //! over the REAL `RestGitHubClient` backed by an EMPTY secret store, proving a credential-less
    //! networked read returns a structured auth error — not empty rows, not a panic.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use qfs_driver_github::{MockGitHubClient, RestGitHubClient, TransportError};
    use qfs_driver_http::{HttpRequest, HttpResponse};
    use qfs_pushdown::PushedQuery;
    use qfs_secrets::{ConnectionId, CredentialKey, InMemoryStore, Secrets};
    use qfs_types::{Schema, Value};

    /// A `ScanNode` over `path` with no pushed query (the bare collection read tests use).
    fn scan_for(path: &str) -> ScanNode {
        ScanNode {
            source: qfs_pushdown::SourceId::new("test"),
            path: path.to_string(),
            pushed: PushedQuery::default(),
            schema: Schema::new(Vec::new()),
            materialize_content: false,
        }
    }

    /// A transport that must never be called — the fail-closed test proves auth fails BEFORE any
    /// wire exchange, so reaching `send` is itself the failure.
    struct NeverCalled;
    impl qfs_driver_github::HttpTransport for NeverCalled {
        fn send(&self, _req: &HttpRequest) -> Result<HttpResponse, TransportError> {
            panic!("the transport must not be reached: auth must fail closed first");
        }
    }

    #[tokio::test]
    async fn github_adapter_reads_a_collection_through_the_mock_client() {
        let client = MockGitHubClient::new().with_list(serde_json::json!([
            { "number": 7, "title": "t", "state": "open", "user": { "login": "octocat" },
              "head": { "ref": "f", "sha": "s" }, "base": { "ref": "main" }, "merged": false },
        ]));
        let driver = GitHubReadDriver::new(Arc::new(client));
        let batch = driver
            .scan(
                &scan_for("/github/octocat/hello/pulls"),
                &RequestContext::anonymous(),
            )
            .await
            .unwrap();
        assert_eq!(batch.rows.len(), 1);
        assert_eq!(batch.rows[0].values[0], Value::Int(7));
    }

    #[tokio::test]
    async fn sql_adapter_reads_a_table_and_pushes_where_into_the_database() {
        use qfs_types::{CmpOp, ColRef, Literal, Predicate};
        let (path, driver) = crate::sql::seeded_test_driver(
            "orders",
            "CREATE TABLE orders (id INTEGER PRIMARY KEY, customer TEXT NOT NULL, total INTEGER NOT NULL);\
             INSERT INTO orders (customer,total) VALUES ('alice',50),('bob',150),('carol',250);",
        );
        let read = SqlReadDriver::new(Arc::new(driver));

        // Bare scan: every row, every column (the catalog-schema derivation path).
        let all = read
            .scan(
                &scan_for("/sql/orders/orders"),
                &RequestContext::anonymous(),
            )
            .await
            .unwrap();
        assert_eq!(all.rows.len(), 3, "all seeded rows");
        assert_eq!(all.schema.columns.len(), 3, "id, customer, total");

        // Pushed WHERE total > 100: the native SELECT filters IN the database to bob + carol.
        let pushed = PushedQuery {
            filter: Some(Predicate::Cmp(
                ColRef::col("total"),
                CmpOp::Gt,
                Literal::Int(100),
            )),
            ..PushedQuery::default()
        };
        let scan = ScanNode {
            source: qfs_pushdown::SourceId::new("sql"),
            path: "/sql/orders/orders".to_string(),
            pushed,
            schema: Schema::new(Vec::new()),
            materialize_content: false,
        };
        let filtered = read
            .scan(&scan, &RequestContext::anonymous())
            .await
            .unwrap();
        assert_eq!(filtered.rows.len(), 2, "WHERE total>100 keeps bob + carol");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn connect_account_facet_errors_actionably_not_unknown_source() {
        // A cloud source with no offline read returns an ACTIONABLE error (carrying the connect
        // hint) rather than leaving the source unregistered (the internal-sounding unknown_source).
        let facet =
            ConnectAccountReadDriver::new("connect a Google account to read mail — run signup");
        let err = facet
            .scan(&scan_for("/mail/inbox"), &RequestContext::anonymous())
            .await
            .unwrap_err();
        match err {
            CfsError::InvalidPath { reason, path } => {
                assert!(
                    reason.contains("connect"),
                    "actionable connect hint: {reason}"
                );
                assert_eq!(path, "/mail/inbox");
            }
            other => panic!("expected an actionable InvalidPath, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn git_adapter_reads_commits_refs_and_tree_hermetically() {
        // An in-memory committed fixture (no git CLI, no network): one commit of one file, mirroring
        // the driver-git fixture's object construction. Proves the facet's per-node dispatch +
        // schema for commits / refs / tree listings.
        use qfs_driver_git::{
            serialize_tree, GitApplier, GitDriver, LooseObjectDb, ObjectDb, Repo, RepoResolver,
            Tree, TreeEntry,
        };
        use qfs_driver_git::{ObjectKind, Oid};
        let mut db = LooseObjectDb::new();
        let blob = db.insert_object(ObjectKind::Blob, b"hello\n");
        let tree = Tree {
            entries: vec![TreeEntry {
                mode: "100644".to_string(),
                name: "a.txt".to_string(),
                oid: blob,
            }],
        };
        let tree_oid = db.insert_object(ObjectKind::Tree, &serialize_tree(&tree));
        let commit_payload = format!(
            "tree {}\nauthor T <t@t> 1700000000 +0000\ncommitter T <t@t> 1700000000 +0000\n\nfirst\n",
            tree_oid.as_str()
        );
        let commit = db.insert_object(ObjectKind::Commit, commit_payload.as_bytes());
        let _ = Oid::parse(commit.as_str()); // commit oid is a valid sha (sanity)
        let mut repo = Repo::new(Arc::new(db) as Arc<dyn ObjectDb>);
        repo.set_ref("refs/heads/main", commit.clone());
        repo.set_ref("main", commit.clone());
        repo.set_ref("HEAD", commit);
        let resolver = RepoResolver::new().with_repo("r", repo);
        let driver = GitDriver::new(resolver, GitApplier::new());
        let read = GitReadDriver::new(Arc::new(driver));

        let commits = read
            .scan(&scan_for("/git/r/commits"), &RequestContext::anonymous())
            .await
            .unwrap();
        assert_eq!(commits.rows.len(), 1, "one commit on HEAD");
        assert!(commits
            .schema
            .columns
            .iter()
            .any(|c| c.name.as_str() == "message"));

        let refs = read
            .scan(&scan_for("/git/r/refs"), &RequestContext::anonymous())
            .await
            .unwrap();
        assert!(
            refs.rows
                .iter()
                .any(|row| matches!(&row.values[0], Value::Text(s) if s.contains("main"))),
            "refs lists the main branch"
        );

        let tree_listing = read
            .scan(&scan_for("/git/r/"), &RequestContext::anonymous())
            .await
            .unwrap();
        assert!(
            tree_listing
                .rows
                .iter()
                .any(|row| matches!(&row.values[0], Value::Text(s) if s == "a.txt")),
            "the HEAD tree lists a.txt"
        );

        // t20260630203100: a blob FILE path reads its CONTENT (one row + a `content` column) rather
        // than erroring `invalid_path` like the listing-only `ls` did — mirroring a `/local/<file>`
        // read so `/git/r/a.txt |> decode <fmt>` has bytes to decode.
        let blob_content_col = |batch: &RowBatch| -> Vec<u8> {
            assert_eq!(
                batch.rows.len(),
                1,
                "a single blob reads as one content row"
            );
            let idx = batch
                .schema
                .columns
                .iter()
                .position(|c| c.name.as_str() == "content")
                .expect("a blob FILE read carries a `content` column");
            match &batch.rows[0].values[idx] {
                Value::Bytes(b) => b.clone(),
                other => panic!("the `content` column must be bytes, got {other:?}"),
            }
        };
        let file = read
            .scan(&scan_for("/git/r/a.txt"), &RequestContext::anonymous())
            .await
            .unwrap();
        assert_eq!(
            blob_content_col(&file),
            b"hello\n",
            "the content column holds the blob's exact bytes"
        );
        // Combined with an explicit `@<ref>` coordinate (`/git/<repo>@<ref>/<file>`).
        let at_ref = read
            .scan(&scan_for("/git/r@main/a.txt"), &RequestContext::anonymous())
            .await
            .unwrap();
        assert_eq!(
            blob_content_col(&at_ref),
            b"hello\n",
            "the same blob reads at an explicit @<ref>"
        );

        // A non-existent file path still fails closed with a structured invalid_path (not content).
        let missing = read
            .scan(&scan_for("/git/r/nope.txt"), &RequestContext::anonymous())
            .await;
        assert!(
            matches!(missing, Err(CfsError::InvalidPath { .. })),
            "a missing blob path is a structured invalid_path, got {missing:?}"
        );
    }

    #[tokio::test]
    async fn github_read_without_credentials_fails_closed_with_an_auth_error() {
        // A registered read facet whose credential cannot be resolved (empty store) returns a
        // structured auth error at request time — NOT an empty batch, NOT a panic. The transport is
        // never reached (auth resolution precedes any wire exchange).
        let store: Arc<dyn Secrets> = Arc::new(InMemoryStore::new());
        let cred = CredentialKey::new(
            qfs_secrets::DriverId("github".to_string()),
            ConnectionId::new("default").unwrap(),
        );
        let client = RestGitHubClient::new(Arc::new(NeverCalled), store, cred);
        let driver = GitHubReadDriver::new(Arc::new(client));
        let err = driver
            .scan(
                &scan_for("/github/octocat/hello/pulls"),
                &RequestContext::anonymous(),
            )
            .await
            .unwrap_err();
        // The structured CfsError carries the driver's stable auth code as its reason (secret-free).
        match err {
            CfsError::InvalidPath { reason, .. } => assert_eq!(reason, "github_auth"),
            other => panic!("expected a structured auth path error, got {other:?}"),
        }
    }

    /// A `ScanNode` over `path` carrying a pushed `WHERE` — the shape the planner hands a facet
    /// whose driver declares `where_: true`.
    fn scan_with_filter(path: &str, predicate: qfs_types::Predicate) -> ScanNode {
        let mut scan = scan_for(path);
        scan.pushed.filter = Some(predicate);
        scan
    }

    /// `<col> == '<value>'` as the planner's typed predicate.
    fn col_eq(col: &str, value: &str) -> qfs_types::Predicate {
        qfs_types::Predicate::Cmp(
            qfs_types::ColRef::col(col),
            qfs_types::CmpOp::Eq,
            qfs_types::Literal::Text(value.to_string()),
        )
    }

    /// A two-file My Drive listing behind the mock client.
    fn drive_listing() -> qfs_driver_gdrive::MockDriveClient {
        qfs_driver_gdrive::MockDriveClient::new().with_list_page(qfs_driver_gdrive::FilePage {
            files: vec![
                qfs_driver_gdrive::FileMeta::for_test("f1", "alpha.pdf", "application/pdf", vec![]),
                qfs_driver_gdrive::FileMeta::for_test("f2", "beta.pdf", "application/pdf", vec![]),
            ],
            next_page_token: None,
        })
    }

    /// The `id` column of each returned row.
    fn ids_of(batch: &RowBatch) -> Vec<String> {
        batch
            .rows
            .iter()
            .map(|r| match &r.values[0] {
                Value::Text(s) => s.to_string(),
                v => panic!("expected a text id, got {v:?}"),
            })
            .collect()
    }

    #[tokio::test]
    async fn drive_where_on_an_absent_id_returns_no_rows_not_the_unfiltered_listing() {
        // The reported defect (ticket 20260723020055): Drive's `q` cannot express `id == …` at all,
        // so `files.list` returns the WHOLE folder listing. Before the facet applied the driver's
        // truthful residual, those rows were delivered verbatim — a filtered query answering with
        // the complete unfiltered listing at exit 0.
        let absent = DriveReadDriver::new(Arc::new(drive_listing()))
            .scan(
                &scan_with_filter("/drive/my", col_eq("id", "no-such-file-id")),
                &RequestContext::anonymous(),
            )
            .await
            .unwrap();
        assert!(
            absent.rows.is_empty(),
            "an absent id matches nothing — got {:?}",
            ids_of(&absent)
        );

        // The other direction: a present id returns exactly its one row, so the residual narrows
        // rather than empties. (A fresh client per read: the mock hands out each seeded list page
        // once.)
        let present = DriveReadDriver::new(Arc::new(drive_listing()))
            .scan(
                &scan_with_filter("/drive/my", col_eq("id", "f2")),
                &RequestContext::anonymous(),
            )
            .await
            .unwrap();
        assert_eq!(ids_of(&present), vec!["f2".to_string()]);

        // And with no predicate the listing is untouched.
        let all = DriveReadDriver::new(Arc::new(drive_listing()))
            .scan(&scan_for("/drive/my"), &RequestContext::anonymous())
            .await
            .unwrap();
        assert_eq!(ids_of(&all), vec!["f1".to_string(), "f2".to_string()]);
    }

    #[tokio::test]
    async fn drive_pushes_the_exact_name_term_and_keeps_the_rows_it_returns() {
        // `name = 'x'` IS exact in Drive's query language, so it pushes with NO residual: the facet
        // must not re-filter rows the backend already selected (proving the residual is truthful,
        // not a blanket re-filter that would fight the pushdown).
        let mock = Arc::new(drive_listing());
        let driver = DriveReadDriver::new(mock.clone());
        let batch = driver
            .scan(
                &scan_with_filter("/drive/my", col_eq("name", "alpha.pdf")),
                &RequestContext::anonymous(),
            )
            .await
            .unwrap();
        // The mock returns both files regardless; the point is that the facet KEEPS them, because
        // the exact `name =` term was Drive's to enforce.
        assert_eq!(
            batch.rows.len(),
            2,
            "an exactly-pushed term leaves no residual"
        );
        let pushed = mock.recorded().into_iter().any(|c| {
            matches!(&c, qfs_driver_gdrive::RecordedCall::ListFiles { query, .. }
                if query.contains("name = 'alpha.pdf'"))
        });
        assert!(pushed, "the exact term reached Drive's `q`");
    }

    #[tokio::test]
    async fn drive_where_on_an_unknown_column_is_refused_not_an_empty_listing() {
        // The `/drive` half of ticket 20260717180100. A facet that declares
        // `honors_pushed_filter` never reaches the executor's checked seam, so it makes the same
        // refusal itself: a typo'd column would otherwise land in the driver's residual, match no
        // row, and answer `rows: []` at exit 0 — the malformed question answered with "none".
        let driver = DriveReadDriver::new(Arc::new(drive_listing()));
        let err = driver
            .scan(
                &scan_with_filter("/drive/my", col_eq("nosuchcol", "x")),
                &RequestContext::anonymous(),
            )
            .await
            .expect_err("an unknown column is refused");
        match err {
            CfsError::InvalidPath { reason, .. } => assert_eq!(reason, "unknown_column"),
            other => panic!("expected a structured unknown-column error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn drive_where_on_a_backend_search_pseudo_column_still_runs() {
        // The refusal must not catch Drive's own search vocabulary: `text` maps to
        // `fullText contains`, which no file row carries as a column. Refusing it would break a
        // real capability in the name of catching typos.
        let batch = DriveReadDriver::new(Arc::new(drive_listing()))
            .scan(
                &scan_with_filter("/drive/my", col_eq("text", "quarterly")),
                &RequestContext::anonymous(),
            )
            .await
            .expect("a search pseudo-column is accepted");
        // It is a LOSSY pre-filter, so the residual re-check runs and (over these fixture rows,
        // which carry no such column) matches nothing — the point is that it executed.
        assert!(batch.rows.is_empty());
    }

    #[test]
    fn the_drive_facet_declares_it_enforces_the_pushed_filter_itself() {
        // The declaration is what tells the executor to stand back; it is only honest because the
        // facet applies the driver's residual above. Drive needs it: a `WHERE` may name the
        // `text`/`full_text` search pseudo-columns the file schema does not carry, which a blanket
        // executor-side re-filter would resolve to nothing and drop every row.
        assert!(DriveReadDriver::new(Arc::new(drive_listing())).honors_pushed_filter());
    }

    // ------------------------------------------------------------------------------------------
    // The other two `honors_pushed_filter` opt-outs: `/s3` + `/r2` and `/cf`'s KV / queue /
    // artifact branches. Both bypass the executor's checked seam, so both owe the same refusal —
    // the mission's law ("a predicate is honored or refused, never dropped") is only true across
    // ALL drivers if these make it themselves. The shipped facet inventory omitted them.
    // ------------------------------------------------------------------------------------------

    /// The reason of a structured facet error (every refusal here is an `InvalidPath`).
    fn reason_of(err: &CfsError) -> &'static str {
        match err {
            CfsError::InvalidPath { reason, .. } => reason,
            other => panic!("expected a structured InvalidPath, got {other:?}"),
        }
    }

    /// An `/s3` read facet over a two-object `assets` bucket behind the in-memory mock backend.
    fn s3_facet() -> ObjReadDriver {
        use qfs_driver_objstore::{Bucket, ListPage, MockObjectBackend, ObjRegistry, ObjectMeta};
        let backend = Arc::new(
            MockObjectBackend::new()
                .with_list_page(ListPage::new(vec![
                    ObjectMeta::new("logs/a.json", 10),
                    ObjectMeta::new("logs/b.json", 20),
                ]))
                .with_get_body(b"{}".to_vec()),
        );
        let registry = ObjRegistry::new().with_bucket("assets", Bucket::new(backend));
        ObjReadDriver::new(Arc::new(ObjDriver::new(
            qfs_driver_objstore::Scheme::S3,
            registry,
        )))
    }

    #[tokio::test]
    async fn s3_where_on_an_unknown_column_is_refused_not_an_empty_listing() {
        // `/s3` declares `where_: true` AND `honors_pushed_filter`, so the executor stands back and
        // the driver's `plan_ls` residual is the only thing that sees the predicate. A typo'd
        // column fell into that residual, matched no object, and answered `rows: []` at exit 0 —
        // exactly the defect this mission exists to kill, on a facet its inventory missed.
        let err = s3_facet()
            .scan(
                &scan_with_filter("/s3/assets", col_eq("nosuchcol", "x")),
                &RequestContext::anonymous(),
            )
            .await
            .expect_err("an unknown column is refused");
        assert_eq!(reason_of(&err), "unknown_column");
    }

    #[tokio::test]
    async fn s3_where_on_a_real_column_still_narrows_the_listing() {
        // The other direction: the refusal must not swallow a legitimate predicate. `key =` is
        // pushed as a superset prefix and kept as a truthful residual, so the facet narrows to the
        // one matching object.
        let batch = s3_facet()
            .scan(
                &scan_with_filter("/s3/assets", col_eq("key", "logs/b.json")),
                &RequestContext::anonymous(),
            )
            .await
            .expect("a real column is honored");
        assert_eq!(batch.rows.len(), 1);
        assert_eq!(
            batch.rows[0].values[0],
            Value::Text("logs/b.json".to_string())
        );
    }

    #[tokio::test]
    async fn s3_object_read_enforces_the_pushed_where_over_its_delivered_row() {
        // A concrete key downloads one `content` row. The predicate was neither pushed nor applied
        // anywhere, and the executor was told to stand back — so an excluding `WHERE` DELIVERED the
        // row it excluded (a wrong answer, worse than an empty one).
        let excluded = s3_facet()
            .scan(
                &scan_with_filter("/s3/assets/logs/a.json", col_eq("key", "logs/other.json")),
                &RequestContext::anonymous(),
            )
            .await
            .expect("a real column is honored");
        assert!(
            excluded.rows.is_empty(),
            "an excluding predicate must not deliver the row"
        );

        // …and the matching one still comes back, so this narrows rather than empties.
        let matched = s3_facet()
            .scan(
                &scan_with_filter("/s3/assets/logs/a.json", col_eq("key", "logs/a.json")),
                &RequestContext::anonymous(),
            )
            .await
            .expect("a real column is honored");
        assert_eq!(matched.rows.len(), 1);

        // An unknown column on the same node is refused, not silently matched by nothing.
        let err = s3_facet()
            .scan(
                &scan_with_filter("/s3/assets/logs/a.json", col_eq("nosuchcol", "x")),
                &RequestContext::anonymous(),
            )
            .await
            .expect_err("an unknown column is refused");
        assert_eq!(reason_of(&err), "unknown_column");
    }

    /// A `/cf` read facet with a KV namespace, a queue, and an artifacts namespace behind mocks.
    fn cf_facet() -> CfReadDriver {
        use qfs_driver_cf::{CfRegistry, MockCfBackend, NoopArtifactTokenSealer};
        let kv = Arc::new(
            MockCfBackend::new().with_kv_keys(vec!["alpha".to_string(), "beta".to_string()]),
        );
        // The queue is registered with an EMPTY backend deliberately: the compiled queue is
        // append-only since the pull was retired to the declared `cloudflare.qfs` view, so no
        // fixture message can be read back through it. It stays in the registry only so the
        // retirement refusal below has a real node to address.
        let queue = Arc::new(MockCfBackend::new());
        let artifacts = Arc::new(MockCfBackend::new().with_artifact_namespace("default"));
        let registry = CfRegistry::new()
            .with_kv("cache", kv)
            .with_queue("jobs", queue)
            .with_artifacts(artifacts, Arc::new(NoopArtifactTokenSealer));
        CfReadDriver::new(Arc::new(CfDriver::new(registry)))
    }

    #[tokio::test]
    async fn cf_kv_where_on_an_unknown_column_is_refused_not_an_empty_listing() {
        let err = cf_facet()
            .scan(
                &scan_with_filter("/cf/kv/cache", col_eq("nosuchcol", "x")),
                &RequestContext::anonymous(),
            )
            .await
            .expect_err("an unknown column is refused");
        assert_eq!(reason_of(&err), "unknown_column");
    }

    #[tokio::test]
    async fn cf_kv_where_on_a_real_column_still_narrows_the_keys() {
        let batch = cf_facet()
            .scan(
                &scan_with_filter("/cf/kv/cache", col_eq("key", "beta")),
                &RequestContext::anonymous(),
            )
            .await
            .expect("a real column is honored");
        assert_eq!(batch.rows.len(), 1);
        assert_eq!(batch.rows[0].values[0], Value::Text("beta".to_string()));
    }

    #[tokio::test]
    async fn cf_artifacts_refuses_an_unknown_where_column() {
        for path in ["/cf/artifacts"] {
            let result = cf_facet()
                .scan(
                    &scan_with_filter(path, col_eq("nosuchcol", "x")),
                    &RequestContext::anonymous(),
                )
                .await;
            match result {
                Err(err) => assert_eq!(reason_of(&err), "unknown_column", "at `{path}`"),
                Ok(batch) => panic!(
                    "`{path}` answered {} row(s) instead of refusing an unknown column",
                    batch.rows.len()
                ),
            }
        }
    }

    #[tokio::test]
    async fn cf_queue_read_is_refused_and_names_the_declared_view() {
        // The predecessor of this test asserted the compiled queue tail narrowed by a real column.
        // That surface is retired: the pull moved to the declared `cloudflare.qfs` read-over-POST
        // view and the compiled queue is append-only. The coverage is kept, inverted — a read must
        // REFUSE and say where the surface went, so the retirement can never decay into an empty
        // batch that reads as "no messages".
        let err = cf_facet()
            .scan(
                &scan_with_filter("/cf/queue/jobs", col_eq("id", "m2")),
                &RequestContext::anonymous(),
            )
            .await
            .expect_err("the retired compiled queue read is refused");
        let msg = format!("{err}");
        assert!(
            msg.contains("append-only"),
            "the refusal must say the compiled queue is append-only, got: {msg}"
        );
        assert!(
            msg.contains("/cloudflare/"),
            "the refusal must name the declared view that replaced it, got: {msg}"
        );
    }
}
