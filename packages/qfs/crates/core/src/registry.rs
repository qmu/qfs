//! The three **open registries** (blueprint §3): paths/mounts, functions +
//! procedures, and codecs. These are the governance mechanism — "a new backend =
//! zero keywords" — so they must sit in the shared engine glue that both the CLI and
//! the server resolve through.
//!
//! Each registry is generic over a **trait object** (`Arc<dyn Driver>` /
//! `Arc<dyn Codec>` / an owned `ProcSig`), not over concrete types
//! (fidelity guard G2): a new driver (E4) implements the trait and calls `register`
//! — it touches zero core types. All three share the identical `new` / `register` /
//! `resolve` shape and use `BTreeMap` for deterministic iteration (test stability).
//! Empty at E0; the unit tests prove empty / round-trip / duplicate / absent.

use std::collections::BTreeMap;
use std::sync::Arc;

use qfs_codec::Codec;
use qfs_driver::{CfsError, Driver, ProcSig};
use qfs_types::TransformDefs;

/// The closed, reserved set of **scope realms** (decision P / blueprint §1.3). A path names
/// three axes — *scope* (whose), *service* (what), *coordinate* (when) — and its root is
/// always exactly one of these realms. Four are **plural collections** that take a single
/// principal segment (`/members/alice/…`); two are **singletons** (`/me`, `/sys`).
///
/// The set is closed for the same reason the keyword set is frozen: it is what makes the
/// `(scope, service)` split decidable (the two §1.3 rules — reserved realm names + single
/// principal arity). Adding a realm is a deliberate governance event, never an incidental
/// driver-mount or user binding. Three guards keep it honoured:
///
/// 1. [`MountRegistry::register`] rejects a driver mount named after a realm (governance).
/// 2. [`peel_scope`] resolves a path's leading realm before routing the service path.
/// 3. [`resolve_name`] ranks a reserved realm name **above** every user-introduced name
///    (a `LET` binding, a connection) so a realm is never shadowed.
pub const RESERVED_REALMS: [&str; 6] = ["members", "projects", "hosts", "directories", "me", "sys"];

/// One of the closed set of [`RESERVED_REALMS`] (decision P / §1.3) — the *scope* axis of
/// a path. A path resolves to exactly one realm; a bare path (`/sql/pg/orders`) is sugar
/// for the self realm [`Realm::Me`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Realm {
    /// `/members/<principal>/…` — another human's world (needs a `POLICY` grant).
    Members,
    /// `/projects/<principal>/…` — a project/team world.
    Projects,
    /// `/hosts/<principal>/…` — an agent-fabric host world (M7).
    Hosts,
    /// `/directories/<principal>/…` — a directory/collection world.
    Directories,
    /// `/me/…` — the caller's own world; the realm a bare path desugars to.
    Me,
    /// `/sys/…` — the admin realm. The one **driver-backed** realm (its mount and realm
    /// coincide): the `/sys` driver serves it, so a driver *may* mount here.
    Sys,
}

impl Realm {
    /// The realm a leading path segment names, or `None` if the segment is an ordinary
    /// (non-realm) name. This is the single source of truth for "is this a realm".
    #[must_use]
    pub fn from_segment(segment: &str) -> Option<Self> {
        Some(match segment {
            "members" => Self::Members,
            "projects" => Self::Projects,
            "hosts" => Self::Hosts,
            "directories" => Self::Directories,
            "me" => Self::Me,
            "sys" => Self::Sys,
            _ => return None,
        })
    }

    /// The realm's canonical segment spelling (`members`, `me`, …).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Members => "members",
            Self::Projects => "projects",
            Self::Hosts => "hosts",
            Self::Directories => "directories",
            Self::Me => "me",
            Self::Sys => "sys",
        }
    }

    /// Whether this is a **plural collection** realm — it takes exactly one principal
    /// segment (`/members/alice/…`). The singletons `me`/`sys` take none. Single-principal
    /// arity is one of the two §1.3 rules that keep `(scope, service)` decidable.
    #[must_use]
    pub const fn takes_principal(self) -> bool {
        matches!(
            self,
            Self::Members | Self::Projects | Self::Hosts | Self::Directories
        )
    }

    /// Whether the realm's service root is itself **driver-backed** — the admin realm
    /// `/sys`, whose realm and mount coincide. It is the one realm a driver may mount
    /// under (the [`MountRegistry::register`] governance exempts it), and [`peel_scope`]
    /// keeps the whole `/sys/…` path as the service path so the sys driver still routes it.
    #[must_use]
    pub const fn is_driver_backed(self) -> bool {
        matches!(self, Self::Sys)
    }
}

/// A resolved **scope** (decision P / §1.3): the realm a path lives in plus, for a
/// collection realm, the single principal segment that selects *whose* world it is. The
/// singletons carry no principal (`Me`/`Sys`). Threaded downstream so later stages
/// (credential resolution, `POLICY`) know *whose* world a node belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathScope {
    /// The realm axis of the path.
    pub realm: Realm,
    /// The principal segment for a collection realm (`alice` in `/members/alice/…`), or
    /// `None` for the `me`/`sys` singletons.
    pub principal: Option<String>,
}

impl PathScope {
    /// The self scope — what a bare path (`/sql/pg/orders`) desugars to.
    #[must_use]
    pub fn me() -> Self {
        Self {
            realm: Realm::Me,
            principal: None,
        }
    }
}

/// The outcome of peeling a leading scope realm off a path ([`peel_scope`]): the resolved
/// [`PathScope`] and the remaining **service** path (with a leading `/`) to route against
/// driver mounts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeResolution {
    /// The resolved scope (whose world).
    pub scope: PathScope,
    /// The remaining service path (what), routed against the [`MountRegistry`].
    pub service: String,
}

/// The structured, machine-readable failure of scope resolution (blueprint §6). The two arms
/// are exactly the §1.3 boundary violations: a collection realm used without its single
/// principal, and a reserved realm name appearing in *service* position (a cross-realm
/// reference). Both are rejected rather than silently routed — relaxing either rule is
/// what reintroduces the ambiguity §1.3 calls out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathScopeError {
    /// A plural-collection realm (`/members`, …) appeared without its single principal
    /// segment, so *whose* world is undetermined.
    MissingPrincipal {
        /// The collection realm that lacked a principal.
        realm: &'static str,
    },
    /// A reserved realm name appeared in **service** position (e.g. `/me/members/…`) — a
    /// path names exactly one realm, so re-entering a realm from inside a service path is
    /// rejected rather than guessed.
    CrossRealm {
        /// The realm name found in service position.
        realm: &'static str,
        /// The full offending path.
        path: String,
    },
}

impl PathScopeError {
    /// A stable, machine-readable code an AI-facing caller branches on (blueprint §6).
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::MissingPrincipal { .. } => "missing_principal",
            Self::CrossRealm { .. } => "cross_realm",
        }
    }
}

/// The implicit **local host** principal under the `/hosts` realm (decision P / §1.3, blueprint
/// §8's "local is an implicit host"): `/hosts/local/<svc>/…` addresses THIS machine's own service
/// mounts. It is the one host principal that is executable today — a non-local host rides the
/// (unwired) tunnel seam and fails closed. Mirrors the binary's client-side hosts registry, which
/// seeds `local` and cannot remove it.
pub const LOCAL_HOST: &str = "local";

/// The structured, machine-readable failure of **host-realm path canonicalization**
/// ([`MountRegistry::canonicalize_host_path`], blueprint §6 error posture). Owner ruling
/// 2026-07-16 (mission `claude-code-sessions-are-queryable-and-steerable-as-qfs-paths`):
/// `/hosts/<host>/<svc>/…` is the canonical address of a host-scoped service, and a mount marked
/// [`MountRegistry::require_host_realm`] retires its bare top-level spelling — the retired alias
/// fails with a pointer at the canonical form, never a silent second path.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum HostScopeError {
    /// `/hosts` appeared without its single host principal, so *which machine* is undetermined
    /// (the `MissingPrincipal` arm of [`peel_scope`], surfaced with the local-host remedy).
    MissingHost {
        /// The offending path.
        path: String,
    },
    /// `/hosts/<host>/…` named a host other than the implicit [`LOCAL_HOST`]. Remote hosts are
    /// **not yet executable** — the cross-machine hop rides the t63 tunnel and re-checks POLICY
    /// at the destination, a documented seam that is not wired (the `require_known_host`
    /// precedent: fail closed, never a silent non-functional route).
    RemoteHostNotExecutable {
        /// The non-local host principal as written.
        host: String,
        /// The offending path.
        path: String,
    },
    /// The service path under `/hosts/<host>` re-entered a reserved realm (§1.3: a path names
    /// exactly one realm) — the same rejection [`peel_scope`] already makes, carried through.
    CrossRealm {
        /// The realm name found in service position.
        realm: &'static str,
        /// The offending path.
        path: String,
    },
    /// A bare (realm-less) path addressed a mount that is reachable **only under the hosts
    /// realm** ([`MountRegistry::require_host_realm`]). The bare spelling is retired (owner
    /// ruling 2026-07-16, honouring t64); the error names the canonical form.
    RetiredBarePath {
        /// The retired bare path as written.
        path: String,
        /// The canonical `/hosts/<host>/…` spelling of the same node (local form).
        canonical: String,
    },
}

impl HostScopeError {
    /// A stable, machine-readable code an AI-facing caller branches on (blueprint §6).
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::MissingHost { .. } => "missing_principal",
            Self::RemoteHostNotExecutable { .. } => "remote_host_not_executable",
            Self::CrossRealm { .. } => "cross_realm",
            Self::RetiredBarePath { .. } => "retired_path",
        }
    }
}

impl std::fmt::Display for HostScopeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingHost { path } => write!(
                f,
                "`{path}`: the /hosts realm needs a host segment — `/hosts/{LOCAL_HOST}/…` \
                 addresses this machine"
            ),
            Self::RemoteHostNotExecutable { host, path } => write!(
                f,
                "`{path}`: host `{host}` is not executable from here — remote hosts are not yet \
                 wired (the tunnel seam is future work); use `/hosts/{LOCAL_HOST}/…` for this \
                 machine"
            ),
            Self::CrossRealm { realm, path } => write!(
                f,
                "`{path}`: the service path re-enters the reserved realm `{realm}` — a path \
                 names exactly one realm"
            ),
            Self::RetiredBarePath { path, canonical } => write!(
                f,
                "`{path}` is retired — this surface is canonical under the hosts realm; use \
                 `{canonical}` (owner ruling 2026-07-16)"
            ),
        }
    }
}

/// How a bare leading **name** resolves under the decision-P precedence ladder (§1.3).
/// A reserved realm name is fixed and outranks every user-introduced name; below it sit
/// (in order) a lexical `LET` binding (t60), a driver mount, and a connection — then
/// nothing. The ranking is what guarantees a realm is **never shadowed** by a binding or
/// a connection, and that a `LET`-bound name (the lexical realm) wins over a mount realm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameRealm {
    /// A [`RESERVED_REALMS`] name — fixed realm, highest precedence, never shadowed.
    Reserved(Realm),
    /// A `LET` lexical binding in scope (t60) — the lexical realm.
    Lexical,
    /// A driver mount root — the mount realm.
    Mount,
    /// A connection (`<provider>/<account>`) key within the scope.
    Connection,
    /// None of the above — a typo'd / unbound bare name.
    Unbound,
}

/// Resolve a bare leading **name** under the decision-P precedence ladder (§1.3):
/// `Reserved` > `Lexical` (`LET`) > `Mount` > `Connection` > `Unbound`. A reserved realm
/// name short-circuits **above** `is_let_bound` and `is_connection`, so it can never be
/// shadowed by a user binding or a connection of the same spelling; a lexical binding
/// outranks a mount of the same spelling (t60's "consult the binding scope before the
/// mount registry").
#[must_use]
pub fn resolve_name(
    name: &str,
    is_let_bound: bool,
    is_mount: bool,
    is_connection: bool,
) -> NameRealm {
    if let Some(realm) = Realm::from_segment(name) {
        return NameRealm::Reserved(realm);
    }
    if is_let_bound {
        return NameRealm::Lexical;
    }
    if is_mount {
        return NameRealm::Mount;
    }
    if is_connection {
        return NameRealm::Connection;
    }
    NameRealm::Unbound
}

/// Peel a leading scope realm off `path`, splitting it into `(scope, service)` (decision
/// P / §1.3). Recognizes a leading [`Realm`]: a collection realm consumes **exactly one**
/// principal segment; the `me` singleton is peeled to its remainder; the driver-backed
/// `sys` singleton keeps the whole `/sys/…` path as the service path (its mount and realm
/// coincide); and a bare path (no realm prefix) is sugar for the self realm [`Realm::Me`]
/// with the whole path as the service path. The service path is then routed against the
/// driver mounts by [`MountRegistry::resolve_path`].
///
/// # Errors
/// [`PathScopeError::MissingPrincipal`] if a collection realm has no principal segment;
/// [`PathScopeError::CrossRealm`] if the service path re-enters a reserved realm.
pub fn peel_scope(path: &str) -> Result<ScopeResolution, PathScopeError> {
    let trimmed = path.trim_start_matches('/');
    let segments: Vec<&str> = if trimmed.is_empty() {
        Vec::new()
    } else {
        trimmed.split('/').collect()
    };

    let Some(&first) = segments.first() else {
        // The empty / root path is the self realm.
        return Ok(ScopeResolution {
            scope: PathScope::me(),
            service: "/".to_string(),
        });
    };

    let Some(realm) = Realm::from_segment(first) else {
        // A bare path is sugar for `/me/…`; the whole path is the service path.
        return Ok(ScopeResolution {
            scope: PathScope::me(),
            service: join_service(&segments),
        });
    };

    // The driver-backed admin realm: realm and mount coincide, so the whole `/sys/…`
    // path stays the service path (the sys driver routes it as before).
    if realm.is_driver_backed() {
        return Ok(ScopeResolution {
            scope: PathScope {
                realm,
                principal: None,
            },
            service: join_service(&segments),
        });
    }

    if realm.takes_principal() {
        let Some(&principal) = segments.get(1) else {
            return Err(PathScopeError::MissingPrincipal {
                realm: realm.as_str(),
            });
        };
        let service = &segments[2..];
        reject_cross_realm(service, path)?;
        return Ok(ScopeResolution {
            scope: PathScope {
                realm,
                principal: Some(principal.to_string()),
            },
            service: join_service(service),
        });
    }

    // The `me` singleton: peel `/me`; the remainder is the service path.
    let service = &segments[1..];
    reject_cross_realm(service, path)?;
    Ok(ScopeResolution {
        scope: PathScope::me(),
        service: join_service(service),
    })
}

/// The first path segment of `path` (empty for `/` or the empty path). Pure string work.
fn leading_segment(path: &str) -> &str {
    path.trim_start_matches('/').split('/').next().unwrap_or("")
}

/// Render a service segment slice back into a `/seg/seg` path (a `/` for the empty slice).
fn join_service(segments: &[&str]) -> String {
    if segments.is_empty() {
        return "/".to_string();
    }
    let mut s = String::new();
    for seg in segments {
        s.push('/');
        s.push_str(seg);
    }
    s
}

/// Reject a service path whose leading segment re-enters a reserved realm (§1.3: a path
/// names exactly one realm).
fn reject_cross_realm(service: &[&str], full: &str) -> Result<(), PathScopeError> {
    if let Some(&first) = service.first() {
        if let Some(inner) = Realm::from_segment(first) {
            return Err(PathScopeError::CrossRealm {
                realm: inner.as_str(),
                path: full.to_string(),
            });
        }
    }
    Ok(())
}

/// Registry of path mounts → drivers (blueprint §3, "paths"). Keyed by mount string
/// (`/mail`, `/s3`, …).
///
/// `Clone` is cheap (the map holds `Arc<dyn Driver>`), enabling the t28 shell completer to
/// hand an owned snapshot to a timeout-bounded scan thread without holding a borrow across the
/// thread boundary.
#[derive(Default, Clone)]
pub struct MountRegistry {
    mounts: BTreeMap<String, Arc<dyn Driver>>,
    /// The resolved transform definitions available at plan time (blueprint §15, decision W). Empty
    /// unless the binary populates it from the System DB before planning — a `|> transform <name>`
    /// stage resolves its OUTPUT schema + mode here (the pure planner/evaluator cannot read the DB).
    transform_defs: TransformDefs,
    /// The resolved declared-type definitions available at plan time (blueprint §5.4/§5.6), keyed by
    /// canonical `/type/<name>` path. Empty unless the binary populates it from the System DB before
    /// planning — a `|> of <name>` assertion resolves the named type's structural schema + refinement
    /// here (the pure planner/evaluator cannot read the DB), exactly like [`transform_defs`].
    ///
    /// [`transform_defs`]: MountRegistry::transform_defs
    declared_types: DeclaredTypeDefs,
    /// Mounts reachable **only under the hosts realm** (`/hosts/<host>/<svc>/…`) — their bare
    /// top-level spelling is retired (owner ruling 2026-07-16; today `/claude` is the one
    /// member). Enforced by [`MountRegistry::canonicalize_host_path`], the canonicalization the
    /// planner, the write evaluator, and DESCRIBE all run before routing.
    host_realm_only: std::collections::BTreeSet<String>,
}

/// The resolved declared-type definitions the planner/evaluator resolve `|> of <name>` assertions
/// against (blueprint §5.6), keyed by canonical `/type/<name>` path. The binary builds these from
/// the System DB (`kind='type'` rows) before planning; empty when no System DB resolves, so a named
/// `of` then fails with a structured "unresolved type" error rather than silently passing through.
pub type DeclaredTypeDefs = BTreeMap<String, crate::ddl::types::ResolvedTypeDef>;

impl MountRegistry {
    /// An empty mount registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Install the resolved transform definitions the planner/evaluator resolve `|> transform`
    /// stages against (the binary builds these from the System DB before planning).
    pub fn set_transform_defs(&mut self, defs: TransformDefs) {
        self.transform_defs = defs;
    }

    /// The resolved transform definitions (empty unless populated). Read by the lowering
    /// (`plan_pipeline`) and the schema fold (`Evaluator`) to resolve a `|> transform <name>` stage.
    #[must_use]
    pub fn transform_defs(&self) -> &TransformDefs {
        &self.transform_defs
    }

    /// Install the resolved declared-type definitions the planner/evaluator resolve `|> of <name>`
    /// assertions against (blueprint §5.6; the binary builds these from the System DB's `kind='type'`
    /// rows before planning).
    pub fn set_declared_types(&mut self, defs: DeclaredTypeDefs) {
        self.declared_types = defs;
    }

    /// The resolved declared-type definitions (empty unless populated). Read by the schema fold
    /// (`Evaluator`) to resolve a `|> of <name>` assertion's structural schema + refinement.
    #[must_use]
    pub fn declared_types(&self) -> &DeclaredTypeDefs {
        &self.declared_types
    }

    /// Register a driver under its declared mount.
    ///
    /// Enforces the decision-P **governance rule** (§1.3): a driver mount must not be
    /// named after a reserved scope realm ([`RESERVED_REALMS`]), because the
    /// `(scope, service)` split a path resolves through is only decidable if no mount
    /// shadows a realm. The sole driver-backed realm `/sys` is exempt (its realm and
    /// mount coincide — see [`Realm::is_driver_backed`]).
    ///
    /// # Errors
    /// [`CfsError::ReservedRealmMount`] if the mount's leading segment shadows a (non
    /// driver-backed) realm; [`CfsError::DuplicateRegistration`] if the mount is already
    /// taken.
    pub fn register(&mut self, driver: Arc<dyn Driver>) -> Result<(), CfsError> {
        let key = driver.mount().to_string();
        let leading = key.trim_start_matches('/').split('/').next().unwrap_or("");
        if let Some(realm) = Realm::from_segment(leading) {
            if !realm.is_driver_backed() {
                return Err(CfsError::ReservedRealmMount {
                    mount: key,
                    realm: realm.as_str(),
                });
            }
        }
        if self.mounts.contains_key(&key) {
            return Err(CfsError::DuplicateRegistration(key));
        }
        self.mounts.insert(key, driver);
        Ok(())
    }

    /// Register `driver` under an EXPLICIT `alias` mount, in ADDITION to its declared `mount()`.
    /// Used for a **deprecated path alias** kept working for one release (e.g. `/ga` →
    /// `/google-analytics`): the same driver answers both prefixes, so an old path still routes while
    /// the canonical mount (what `mount()` returns, and what the docs render) is the new name. The
    /// alias is a runtime-routing entry only — it is NOT a second `mount()`, so introspection/docs
    /// keep showing the canonical name. (The general user-facing `CREATE ALIAS` mechanism is separate
    /// future work; this is the built-in deprecation shim.)
    ///
    /// # Errors
    /// [`CfsError::ReservedRealmMount`] if `alias`'s leading segment shadows a non-driver-backed
    /// realm; [`CfsError::DuplicateRegistration`] if `alias` is already taken.
    pub fn register_alias(&mut self, alias: &str, driver: Arc<dyn Driver>) -> Result<(), CfsError> {
        let key = alias.to_string();
        let leading = key.trim_start_matches('/').split('/').next().unwrap_or("");
        if let Some(realm) = Realm::from_segment(leading) {
            if !realm.is_driver_backed() {
                return Err(CfsError::ReservedRealmMount {
                    mount: key,
                    realm: realm.as_str(),
                });
            }
        }
        if self.mounts.contains_key(&key) {
            return Err(CfsError::DuplicateRegistration(key));
        }
        self.mounts.insert(key, driver);
        Ok(())
    }

    /// Resolve a mount to its driver.
    ///
    /// # Errors
    /// [`CfsError::UnknownMount`] if no driver is registered for the mount.
    pub fn resolve(&self, mount: &str) -> Result<Arc<dyn Driver>, CfsError> {
        self.mounts
            .get(mount)
            .cloned()
            .ok_or_else(|| CfsError::UnknownMount(mount.to_string()))
    }

    /// Route a full path to the driver whose mount is the **longest prefix** of it,
    /// returning that driver and the remaining **sub-path** (the path with the matched
    /// mount and its trailing `/` stripped). Overlapping mounts (`/g` and `/git`)
    /// resolve to the longest match, so `/git/repo@ref/x` routes to the `/git` driver
    /// with sub-path `repo@ref/x` (never to `/g`).
    ///
    /// A mount matches only at a path **boundary**: it must equal the path, or the path
    /// must continue with `/` after it — so `/git` does not capture `/gitlab/x`. Returns
    /// `None` when no mount is a boundary-prefix of `path` (the caller raises
    /// [`CfsError::UnknownMount`] with context it owns).
    ///
    /// **The hosts realm peels here** (decision P / §1.3; owner ruling 2026-07-16): a
    /// `/hosts/local/<svc>/…` path addresses this machine's own service mounts, so the
    /// `/hosts/local` realm prefix is stripped via [`peel_scope`] and the remaining service
    /// path routes as usual — the general `/hosts/<h>/<svc>` rule, never a per-driver special
    /// case (no mount may itself start with a realm segment, so the peel cannot shadow one).
    /// A non-local host has no executable mount here and resolves to `None` (fail closed; the
    /// structured `remote_host_not_executable` error surfaces from
    /// [`MountRegistry::canonicalize_host_path`], which planning/eval/DESCRIBE run first).
    #[must_use]
    pub fn resolve_path(&self, path: &str) -> Option<(Arc<dyn Driver>, String)> {
        if leading_segment(path) == "hosts" {
            return match peel_scope(path) {
                Ok(res) if res.scope.principal.as_deref() == Some(LOCAL_HOST) => {
                    self.resolve_service_path(&res.service)
                }
                _ => None,
            };
        }
        self.resolve_service_path(path)
    }

    /// The raw longest-prefix router over the mount table — [`resolve_path`] after realm
    /// peeling (and the bare-path check [`canonicalize_host_path`] uses, which must NOT peel).
    ///
    /// [`resolve_path`]: MountRegistry::resolve_path
    /// [`canonicalize_host_path`]: MountRegistry::canonicalize_host_path
    fn resolve_service_path(&self, path: &str) -> Option<(Arc<dyn Driver>, String)> {
        let mut best: Option<(&String, &Arc<dyn Driver>)> = None;
        for (mount, driver) in &self.mounts {
            let matches = path == mount
                || path
                    .strip_prefix(mount.as_str())
                    .is_some_and(|rest| rest.starts_with('/'));
            if matches && best.is_none_or(|(b, _)| mount.len() > b.len()) {
                best = Some((mount, driver));
            }
        }
        best.map(|(mount, driver)| {
            let sub = path
                .strip_prefix(mount.as_str())
                .unwrap_or("")
                .trim_start_matches('/')
                .to_string();
            (Arc::clone(driver), sub)
        })
    }

    /// Mark `mount` as reachable **only under the hosts realm**: its bare top-level spelling is
    /// retired and [`MountRegistry::canonicalize_host_path`] fails it with a `retired_path`
    /// pointer at the canonical `/hosts/<host>{mount}` form (owner ruling 2026-07-16 — one
    /// canonical address per surface; a retired alias fails with a pointer, never a silent
    /// second path). Registration-site configuration, like the mount itself.
    pub fn require_host_realm(&mut self, mount: &str) {
        self.host_realm_only.insert(mount.to_string());
    }

    /// Whether `mount` was marked host-realm-only via [`MountRegistry::require_host_realm`].
    #[must_use]
    pub fn is_host_realm_only(&self, mount: &str) -> bool {
        self.host_realm_only.contains(mount)
    }

    /// Canonicalize an addressed path for routing under the host-realm path canon (decision P /
    /// §1.3; owner ruling 2026-07-16): a `/hosts/<host>/<svc>/…` path peels to its service path
    /// when `<host>` is the implicit [`LOCAL_HOST`] (this machine), fails closed for any other
    /// host (`remote_host_not_executable` — the tunnel seam is not wired), and a **bare** path
    /// addressing a [`MountRegistry::require_host_realm`] mount fails with `retired_path`
    /// naming the canonical spelling. Every other path passes through unchanged — non-hosts
    /// realms keep their existing behaviour (no accidental widening).
    ///
    /// # Errors
    /// [`HostScopeError`] per the rules above.
    pub fn canonicalize_host_path(&self, path: &str) -> Result<String, HostScopeError> {
        if leading_segment(path) == "hosts" {
            return match peel_scope(path) {
                Ok(res) => match res.scope.principal.as_deref() {
                    Some(LOCAL_HOST) => Ok(res.service),
                    Some(host) => Err(HostScopeError::RemoteHostNotExecutable {
                        host: host.to_string(),
                        path: path.to_string(),
                    }),
                    // `hosts` is a collection realm: peel_scope always yields a principal or
                    // the MissingPrincipal error, so this arm is unreachable — kept total.
                    None => Err(HostScopeError::MissingHost {
                        path: path.to_string(),
                    }),
                },
                Err(PathScopeError::MissingPrincipal { .. }) => Err(HostScopeError::MissingHost {
                    path: path.to_string(),
                }),
                Err(PathScopeError::CrossRealm { realm, .. }) => Err(HostScopeError::CrossRealm {
                    realm,
                    path: path.to_string(),
                }),
            };
        }
        // A bare path: reject a retired spelling of a host-realm-only mount (raw match — the
        // whole point is that this path did NOT come through the hosts realm).
        if let Some((driver, _)) = self.resolve_service_path(path) {
            if self.host_realm_only.contains(driver.mount()) {
                return Err(HostScopeError::RetiredBarePath {
                    path: path.to_string(),
                    canonical: format!("/hosts/{LOCAL_HOST}{path}"),
                });
            }
        }
        Ok(path.to_string())
    }

    /// Iterate every registered driver (deterministic mount order). Used by name
    /// resolution (t06) to find which drivers ship a given prelude alias when deciding
    /// receiver-typed alias scope / ambiguity.
    pub fn drivers(&self) -> impl Iterator<Item = &Arc<dyn Driver>> {
        self.mounts.values()
    }

    /// Number of registered mounts.
    #[must_use]
    pub fn len(&self) -> usize {
        self.mounts.len()
    }

    /// Whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.mounts.is_empty()
    }
}

/// Registry of functions + `CALL` procedures (blueprint §3, "functions /
/// procedures"). One registry because both alias functions and procedures are
/// receiver-typed, registry-resolved, and keyword-free. Keyed by qualified name
/// (e.g. `mail.send`). Stores the [`ProcSig`] declaration (params, irreversible,
/// returns, requires_scopes — t13) only.
#[derive(Default)]
pub struct ProcRegistry {
    procs: BTreeMap<String, ProcSig>,
}

impl ProcRegistry {
    /// An empty procedure registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a procedure under a qualified name (e.g. `mail.send`).
    ///
    /// # Errors
    /// [`CfsError::DuplicateRegistration`] if the name is already taken.
    pub fn register(&mut self, qualified_name: &str, decl: ProcSig) -> Result<(), CfsError> {
        if self.procs.contains_key(qualified_name) {
            return Err(CfsError::DuplicateRegistration(qualified_name.to_string()));
        }
        self.procs.insert(qualified_name.to_string(), decl);
        Ok(())
    }

    /// Resolve a qualified procedure name to its declaration.
    ///
    /// # Errors
    /// [`CfsError::UnknownProcedure`] if the name is not registered.
    pub fn resolve(&self, qualified_name: &str) -> Result<&ProcSig, CfsError> {
        self.procs
            .get(qualified_name)
            .ok_or_else(|| CfsError::UnknownProcedure(qualified_name.to_string()))
    }

    /// Number of registered procedures.
    #[must_use]
    pub fn len(&self) -> usize {
        self.procs.len()
    }

    /// Whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.procs.is_empty()
    }
}

/// Registry of codecs (blueprint §3, "codecs"). Keyed by format (`json`, `yaml`, …).
#[derive(Default)]
pub struct CodecRegistry {
    codecs: BTreeMap<String, Arc<dyn Codec>>,
}

impl CodecRegistry {
    /// An empty codec registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A registry pre-loaded with the six builtin codecs (`json`, `jsonl`, `yaml`,
    /// `toml`, `csv`, `md+frontmatter`) from `qfs-codec` (t15). This is the default the
    /// engine resolves `DECODE`/`ENCODE fmt` through; a backend extends it via
    /// [`CodecRegistry::register`] (a new codec = zero keywords, blueprint §3).
    ///
    /// The builtins have distinct format names, so registration never collides; the
    /// `expect_used` lint is satisfied without an `unwrap` because the only error arm
    /// ([`CfsError::DuplicateRegistration`]) is structurally unreachable here.
    #[must_use]
    pub fn with_builtins() -> Self {
        let mut reg = Self::new();
        for codec in qfs_codec::builtin_codecs() {
            // Builtin format names are unique by construction; ignore the (unreachable)
            // duplicate error rather than panic, keeping lib code panic-free.
            let _ = reg.register(codec);
        }
        reg
    }

    /// Register a codec under its declared format.
    ///
    /// # Errors
    /// [`CfsError::DuplicateRegistration`] if the format is already taken.
    pub fn register(&mut self, codec: Arc<dyn Codec>) -> Result<(), CfsError> {
        let key = codec.fmt().to_string();
        if self.codecs.contains_key(&key) {
            return Err(CfsError::DuplicateRegistration(key));
        }
        self.codecs.insert(key, codec);
        Ok(())
    }

    /// Resolve a format to its codec.
    ///
    /// # Errors
    /// [`CfsError::UnknownCodec`] if no codec is registered for the format.
    pub fn resolve(&self, fmt: &str) -> Result<Arc<dyn Codec>, CfsError> {
        self.codecs
            .get(fmt)
            .cloned()
            .ok_or_else(|| CfsError::UnknownCodec(fmt.to_string()))
    }

    /// Number of registered codecs.
    #[must_use]
    pub fn len(&self) -> usize {
        self.codecs.len()
    }

    /// Whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.codecs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qfs_codec::{Codec, RowBatch};
    use qfs_driver::{Archetype, Capabilities, NodeDesc, Path, PushdownProfile, VersionSupport};
    use qfs_plan::{AppliedEffect, ApplyError, EffectNode, PlanApplier};
    use qfs_types::Schema;

    /// A no-I/O applier the fake driver hands back through the `applier()` seam.
    #[derive(Default)]
    struct NoopApplier;
    impl PlanApplier for NoopApplier {
        fn apply(&mut self, node: &EffectNode) -> Result<AppliedEffect, ApplyError> {
            Ok(AppliedEffect::new(node.id, 0))
        }
    }

    struct FakeDriver {
        mount: &'static str,
        pushdown: PushdownProfile,
        applier: NoopApplier,
    }
    impl FakeDriver {
        fn new() -> Self {
            Self::at("/fake")
        }
        fn at(mount: &'static str) -> Self {
            Self {
                mount,
                pushdown: PushdownProfile::None,
                applier: NoopApplier,
            }
        }
    }
    impl Driver for FakeDriver {
        fn mount(&self) -> &str {
            self.mount
        }
        fn describe(&self, _p: &Path) -> Result<NodeDesc, CfsError> {
            let _ = NodeDesc::new(Archetype::BlobNamespace, Schema::empty());
            Err(CfsError::NotImplemented {
                feature: "describe",
            })
        }
        fn capabilities(&self, _p: &Path) -> Capabilities {
            Capabilities::default()
        }
        fn procedures(&self) -> &[ProcSig] {
            &[]
        }
        fn pushdown(&self) -> &PushdownProfile {
            &self.pushdown
        }
        fn version_support(&self, _p: &Path) -> VersionSupport {
            VersionSupport::None
        }
        fn applier(&self) -> &dyn PlanApplier {
            &self.applier
        }
    }

    struct FakeCodec;
    impl Codec for FakeCodec {
        fn fmt(&self) -> &str {
            "fake"
        }
        fn decode(&self, _b: &[u8]) -> Result<RowBatch, CfsError> {
            Err(CfsError::NotImplemented { feature: "decode" })
        }
        fn encode(&self, _r: &RowBatch) -> Result<Vec<u8>, CfsError> {
            Err(CfsError::NotImplemented { feature: "encode" })
        }
    }

    #[test]
    fn mount_registry_empty_then_roundtrip_then_duplicate_then_absent() {
        let mut reg = MountRegistry::new();
        assert!(reg.is_empty());
        assert!(matches!(
            reg.resolve("/fake"),
            Err(CfsError::UnknownMount(_))
        ));

        reg.register(Arc::new(FakeDriver::new())).unwrap();
        assert_eq!(reg.len(), 1);
        assert_eq!(reg.resolve("/fake").unwrap().mount(), "/fake");

        let dup = reg.register(Arc::new(FakeDriver::new()));
        assert!(matches!(dup, Err(CfsError::DuplicateRegistration(_))));
    }

    /// O1 — the longest-mount-prefix router: overlapping mounts (`/g`, `/git`) resolve to
    /// the longest match, the matched mount is stripped to a sub-path, and an unmatched
    /// path returns `None`. Also proves the boundary rule (`/git` ≠ `/gitlab/...`).
    #[test]
    fn resolve_path_picks_longest_mount_prefix() {
        let mut reg = MountRegistry::new();
        reg.register(Arc::new(FakeDriver::at("/g"))).unwrap();
        reg.register(Arc::new(FakeDriver::at("/git"))).unwrap();

        // Longest match wins: /git, not /g.
        let (driver, sub) = reg.resolve_path("/git/repo@ref/x").unwrap();
        assert_eq!(driver.mount(), "/git");
        assert_eq!(sub, "repo@ref/x");

        // The shorter mount still routes its own subtree.
        let (driver, sub) = reg.resolve_path("/g/foo").unwrap();
        assert_eq!(driver.mount(), "/g");
        assert_eq!(sub, "foo");

        // Exact-mount path yields an empty sub-path.
        let (driver, sub) = reg.resolve_path("/git").unwrap();
        assert_eq!(driver.mount(), "/git");
        assert_eq!(sub, "");

        // Boundary rule: /git must not capture /gitlab/* — and with no /gitlab mount,
        // there is no boundary-prefix at all, so it is unmatched.
        assert!(reg.resolve_path("/gitlab/x").is_none());

        // Wholly unmatched path → None.
        assert!(reg.resolve_path("/s3/bucket/key").is_none());
    }

    #[test]
    fn resolve_path_routes_a_multi_segment_user_mount() {
        // DESIGN SPIKE — EPIC 20260701100000 (defined paths), keystone 20260701100010, decision #5.
        // A user "defined path" is a MULTI-SEGMENT mount; resolve_path must route it by the same
        // boundary-aware longest-prefix rule WITHOUT any router change — de-risking the premise that
        // recursive `/<folder>/<folder>/<resource>` paths route through the existing registry. (The
        // sibling premise — a driver keeping a CANONICAL id() ≠ its user mount so per-driver parsers
        // see `/<id>/<sub>` unchanged — is already proven in production by the `/ga` alias.)
        let mut reg = MountRegistry::new();
        reg.register(Arc::new(FakeDriver::at("/work/reports")))
            .unwrap();
        // A nested resource under the multi-segment mount routes, stripping the WHOLE mount.
        let (_d, sub) = reg.resolve_path("/work/reports/2026/q3.csv").unwrap();
        assert_eq!(sub, "2026/q3.csv");
        // The exact multi-segment mount resolves with an empty sub-path.
        assert_eq!(reg.resolve_path("/work/reports").unwrap().1, "");
        // Boundary rule holds for multi-segment mounts: a sibling sharing only a textual prefix of
        // the LAST segment is not captured.
        assert!(reg.resolve_path("/work/reportskeeping/x").is_none());
        // A shorter overlapping mount loses to the longer multi-segment one (longest-prefix).
        reg.register(Arc::new(FakeDriver::at("/work"))).unwrap();
        let (d2, sub2) = reg.resolve_path("/work/reports/x").unwrap();
        assert_eq!(
            d2.mount(),
            "/work/reports",
            "longest multi-segment mount wins"
        );
        assert_eq!(sub2, "x");
        // …while a different child of the shorter mount still routes to it.
        assert_eq!(reg.resolve_path("/work/budget").unwrap().0.mount(), "/work");
    }

    /// The host-realm peel in `resolve_path` (decision P / owner ruling 2026-07-16):
    /// `/hosts/local/<svc>/…` routes to the same mount as the bare service path — the GENERAL
    /// `/hosts/<h>/<svc>` rule (proven over an ordinary fake mount, not a claude special-case) —
    /// while a non-local host, a missing host segment, and a cross-realm service path all
    /// resolve to `None` (fail closed; the structured error comes from canonicalization).
    #[test]
    fn resolve_path_peels_the_local_host_realm() {
        let mut reg = MountRegistry::new();
        reg.register(Arc::new(FakeDriver::at("/fake"))).unwrap();

        let (driver, sub) = reg.resolve_path("/hosts/local/fake/rows").unwrap();
        assert_eq!(driver.mount(), "/fake");
        assert_eq!(sub, "rows");
        // The exact peeled mount resolves with an empty sub-path.
        assert_eq!(reg.resolve_path("/hosts/local/fake").unwrap().1, "");

        // A non-local host has no executable mount here (the tunnel seam is not wired).
        assert!(reg.resolve_path("/hosts/qfs.cloud/fake/rows").is_none());
        // `/hosts` with no principal, and a cross-realm service path, both fail closed.
        assert!(reg.resolve_path("/hosts").is_none());
        assert!(reg.resolve_path("/hosts/local/members/alice").is_none());
    }

    /// The host-realm path canon (`canonicalize_host_path`, owner ruling 2026-07-16): the local
    /// host peels to the service path; a non-local host is `remote_host_not_executable`; a
    /// missing host is `missing_principal`; a cross-realm service path is `cross_realm`; a bare
    /// spelling of a `require_host_realm` mount is `retired_path` NAMING the canonical form; and
    /// every other bare path passes through unchanged (no accidental widening).
    #[test]
    fn canonicalize_host_path_rules() {
        let mut reg = MountRegistry::new();
        reg.register(Arc::new(FakeDriver::at("/fake"))).unwrap();
        reg.register(Arc::new(FakeDriver::at("/plain"))).unwrap();
        reg.require_host_realm("/fake");
        assert!(reg.is_host_realm_only("/fake"));
        assert!(!reg.is_host_realm_only("/plain"));

        // The local host peels to the service path — for ANY mount, marked or not.
        assert_eq!(
            reg.canonicalize_host_path("/hosts/local/fake/rows")
                .unwrap(),
            "/fake/rows"
        );
        assert_eq!(
            reg.canonicalize_host_path("/hosts/local/plain/x").unwrap(),
            "/plain/x"
        );

        // A non-local host fails closed with the structured remote error.
        let err = reg
            .canonicalize_host_path("/hosts/qfs.cloud/fake/rows")
            .unwrap_err();
        assert_eq!(err.code(), "remote_host_not_executable");
        assert!(err.to_string().contains("qfs.cloud"));

        // `/hosts` with no host segment.
        let err = reg.canonicalize_host_path("/hosts").unwrap_err();
        assert_eq!(err.code(), "missing_principal");

        // The service path must not re-enter a reserved realm (§1.3, the existing rejection).
        let err = reg
            .canonicalize_host_path("/hosts/local/members/alice")
            .unwrap_err();
        assert_eq!(err.code(), "cross_realm");

        // The retired bare spelling of a host-realm-only mount names the canonical form.
        let err = reg.canonicalize_host_path("/fake/rows").unwrap_err();
        assert_eq!(err.code(), "retired_path");
        assert!(matches!(
            &err,
            HostScopeError::RetiredBarePath { canonical, .. } if canonical == "/hosts/local/fake/rows"
        ));
        assert!(err.to_string().contains("/hosts/local/fake/rows"));

        // An unmarked mount's bare path passes through unchanged, as does an unrouted path.
        assert_eq!(reg.canonicalize_host_path("/plain/x").unwrap(), "/plain/x");
        assert_eq!(
            reg.canonicalize_host_path("/nowhere/x").unwrap(),
            "/nowhere/x"
        );
    }

    #[test]
    fn proc_registry_empty_then_roundtrip_then_duplicate_then_absent() {
        let mut reg = ProcRegistry::new();
        assert!(reg.is_empty());
        assert!(matches!(
            reg.resolve("mail.send"),
            Err(CfsError::UnknownProcedure(_))
        ));

        let decl = ProcSig::new("send");
        reg.register("mail.send", decl.clone()).unwrap();
        assert_eq!(reg.len(), 1);
        assert_eq!(reg.resolve("mail.send").unwrap().name, "send");

        let dup = reg.register("mail.send", decl);
        assert!(matches!(dup, Err(CfsError::DuplicateRegistration(_))));
    }

    #[test]
    fn codec_registry_empty_then_roundtrip_then_duplicate_then_absent() {
        let mut reg = CodecRegistry::new();
        assert!(reg.is_empty());
        assert!(matches!(
            reg.resolve("fake"),
            Err(CfsError::UnknownCodec(_))
        ));

        reg.register(Arc::new(FakeCodec)).unwrap();
        assert_eq!(reg.len(), 1);
        assert_eq!(reg.resolve("fake").unwrap().fmt(), "fake");

        let dup = reg.register(Arc::new(FakeCodec));
        assert!(matches!(dup, Err(CfsError::DuplicateRegistration(_))));
    }

    // ---- Decision P (t71): scope realms, governance, reserved-name resolution --------

    /// Governance (§1.3): a driver mount named after a reserved realm is rejected at
    /// registration — that is what keeps the `(scope, service)` split decidable. The
    /// driver-backed admin realm `/sys` is the sole exception (realm and mount coincide).
    #[test]
    fn register_rejects_a_driver_mount_that_shadows_a_realm() {
        let mut reg = MountRegistry::new();
        for realm in ["/members", "/projects", "/hosts", "/directories", "/me"] {
            let err = reg.register(Arc::new(FakeDriver::at(realm))).unwrap_err();
            assert_eq!(err.code(), "reserved_realm_mount");
            assert!(matches!(err, CfsError::ReservedRealmMount { mount, .. } if mount == realm));
        }
        // The driver-backed `/sys` realm is exempt — its realm and mount coincide.
        reg.register(Arc::new(FakeDriver::at("/sys"))).unwrap();
        assert_eq!(reg.resolve("/sys").unwrap().mount(), "/sys");
        // A non-realm mount is unaffected.
        reg.register(Arc::new(FakeDriver::at("/mail"))).unwrap();
    }

    /// A path resolves to the right realm by precedence: a collection realm consumes one
    /// principal and strips it from the service path; `/me/…` peels to its remainder; the
    /// driver-backed `/sys/…` keeps the whole path; a bare path is sugar for `/me`.
    #[test]
    fn peel_scope_resolves_each_realm_by_precedence() {
        // Collection realm + single principal → scope carries the principal; service is
        // the stripped remainder.
        let r = peel_scope("/members/alice/gmail/inbox").unwrap();
        assert_eq!(r.scope.realm, Realm::Members);
        assert_eq!(r.scope.principal.as_deref(), Some("alice"));
        assert_eq!(r.service, "/gmail/inbox");

        // `/me` singleton peels to its remainder.
        let r = peel_scope("/me/google/work/gmail/inbox").unwrap();
        assert_eq!(r.scope, PathScope::me());
        assert_eq!(r.service, "/google/work/gmail/inbox");

        // The driver-backed `/sys` realm keeps the whole path as the service path so the
        // sys driver still routes it.
        let r = peel_scope("/sys/audit").unwrap();
        assert_eq!(r.scope.realm, Realm::Sys);
        assert_eq!(r.scope.principal, None);
        assert_eq!(r.service, "/sys/audit");

        // A bare path is sugar for the self realm; the whole path is the service path.
        let r = peel_scope("/sql/pg/orders").unwrap();
        assert_eq!(r.scope, PathScope::me());
        assert_eq!(r.service, "/sql/pg/orders");

        // A one-level `*` is a legal principal (glob over the collection, §1.3 step 4).
        let r = peel_scope("/members/*/gmail/inbox").unwrap();
        assert_eq!(r.scope.principal.as_deref(), Some("*"));
        assert_eq!(r.service, "/gmail/inbox");
    }

    /// A cross-realm / ambiguous reference is a **structured** error, never a silent
    /// route: a collection realm without a principal, and a service path that re-enters a
    /// realm, are exactly the two §1.3 boundary violations.
    #[test]
    fn peel_scope_rejects_cross_realm_and_missing_principal() {
        let err = peel_scope("/members").unwrap_err();
        assert_eq!(err.code(), "missing_principal");
        assert!(matches!(err, PathScopeError::MissingPrincipal { realm } if realm == "members"));

        // A service path may not re-enter a realm (a path names exactly one realm).
        let err = peel_scope("/me/members/alice").unwrap_err();
        assert_eq!(err.code(), "cross_realm");
        assert!(matches!(err, PathScopeError::CrossRealm { realm, .. } if realm == "members"));
    }

    /// Reserved-name resolution (§1.3): a reserved realm name resolves to its fixed realm
    /// and is **not** shadowed by a `LET` binding or a connection of the same spelling;
    /// and the lexical (`LET`) realm outranks the mount realm (t60 precedence).
    #[test]
    fn resolve_name_ranks_realm_above_binding_and_lexical_above_mount() {
        // A reserved realm wins even when a binding AND a connection share its spelling.
        assert_eq!(
            resolve_name("sys", /*let*/ true, /*mount*/ true, /*conn*/ true),
            NameRealm::Reserved(Realm::Sys),
        );
        assert_eq!(
            resolve_name("members", true, false, true),
            NameRealm::Reserved(Realm::Members),
        );
        // A lexical `LET` binding outranks a mount of the same spelling (t60).
        assert_eq!(
            resolve_name("orders", /*let*/ true, /*mount*/ true, /*conn*/ false),
            NameRealm::Lexical,
        );
        // With no binding, the mount realm wins over a connection.
        assert_eq!(resolve_name("orders", false, true, true), NameRealm::Mount,);
        // A connection name (no realm/binding/mount) resolves to the connection realm.
        assert_eq!(
            resolve_name("work", false, false, true),
            NameRealm::Connection
        );
        // A typo is unbound.
        assert_eq!(
            resolve_name("ghost", false, false, false),
            NameRealm::Unbound
        );
    }

    /// t15 — `with_builtins` resolves all six builtin codecs by name, and an unknown
    /// format returns a structured `UnknownCodec` (not a panic).
    #[test]
    fn codec_registry_with_builtins_resolves_all_six() {
        let reg = CodecRegistry::with_builtins();
        assert_eq!(reg.len(), 6);
        for fmt in ["json", "jsonl", "yaml", "toml", "csv", "md"] {
            assert_eq!(reg.resolve(fmt).unwrap().fmt(), fmt);
        }
        assert!(matches!(
            reg.resolve("parquet"),
            Err(CfsError::UnknownCodec(_))
        ));
    }
}
