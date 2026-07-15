//! The **/sys half of the provisioning universe** (blueprint §16, Decision X, increment 2):
//! the system/project-DB config collections `qfs dump` covers, minus the amended exclusions.
//!
//! [`SysState`] mirrors the dump's record set — declared drivers (`sys_drivers`), sys policies
//! (`sys_policies`), settings (`sys_settings`), and project path bindings (`path_binding`) —
//! **minus** the collections the amendment places outside the universe entirely:
//!
//! - **Secretish settings are EXCLUDED, not redacted** (never emitted, never diffed, never
//!   destroyed by absence) — the shared [`qfs_core::secretish_setting_key`] predicate is the one
//!   list dump/restore/provisioning all read.
//! - **Billing and `sys_ddl_events` are outside the universe structurally**: [`SysState`] has no
//!   collection for them, so no diff — and therefore no authoritative destroy — can ever name
//!   them.
//!
//! Policies are keyed by name **within their store**: [`SysState::policies`] (`sys_policies`)
//! and `ServerState::policies` (`/server/policies`) are two collections, never conflated.
//! [`ConfigState`] is the two-store document universe the emitter/loader/differ operate on.

use std::collections::BTreeMap;

use qfs_core::{secretish_setting_key, Value};
use qfs_server::ServerState;

use crate::proj::ProjRow;

/// The whole provisioning config universe: the running daemon's `/server` store plus the
/// system/project-DB `/sys` store. One document (`emit`/`load`) and one diff cover both.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConfigState {
    /// The `/server` self-config store (endpoints/triggers/jobs/views/policies/webhooks).
    pub server: ServerState,
    /// The `/sys` system/project-DB store (drivers/policies/settings/path bindings).
    pub sys: SysState,
}

impl ConfigState {
    /// An empty universe (the diff baseline for a fresh deployment).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// The `/sys` store's config projection: the dump-covered collections minus the excluded ones
/// (billing / ddl events have no collection here; secretish settings are filtered at every
/// boundary). Each collection is a name-keyed [`BTreeMap`] so emission and diff are
/// deterministic, mirroring `ServerState`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SysState {
    /// `sys_drivers` — declared third-party drivers (blueprint §13), keyed by name. The `auth`
    /// descriptor names a SCHEME, never a token (the credential-free-script contract).
    pub drivers: BTreeMap<String, SysDriverRow>,
    /// `sys_policies` — the system-DB policy store, keyed by name. Distinct from
    /// `/server/policies` (two stores, never conflated).
    pub policies: BTreeMap<String, SysPolicyRow>,
    /// `sys_settings` — **non-secretish only** (key → value). A secretish key must never enter
    /// this map; every producer filters through [`qfs_core::secretish_setting_key`].
    pub settings: BTreeMap<String, String>,
    /// `path_binding` (project DB) — defined-path bindings keyed by path (`/chat`, …).
    /// `secret_ref` is a REFERENCE (`env:`/`vault:`), never a value.
    pub bindings: BTreeMap<String, PathBindingRow>,
    /// `sys_transforms` — transform-predicate definitions (blueprint §15), keyed by name. Exposed
    /// as the top-level `/transform` mount (NOT `/sys/transforms`); definition text + selectors +
    /// a secret REFERENCE only (the derived mode is never emitted — it is a pure function of `input`).
    pub transforms: BTreeMap<String, TransformRow>,
}

/// One declared-driver row (`sys_drivers`). Declaration text + selectors only — no credential
/// column exists (`auth` names a scheme).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SysDriverRow {
    /// The declaration name (the row key).
    pub name: String,
    /// The declaration kind (`driver`/`type`/`view`/`map`).
    pub kind: String,
    /// The service base URL, if declared.
    pub base_url: Option<String>,
    /// The auth SCHEME descriptor (JSON text) — a scheme name, never a token.
    pub auth: Option<String>,
    /// The pagination descriptor (JSON text).
    pub pagination: Option<String>,
    /// The declared row type name (`OF <type>`).
    pub of_type: Option<String>,
    /// The declared verb (for a `MAP`).
    pub verb: Option<String>,
    /// The declaration body (JSON text).
    pub body: Option<String>,
    /// Whether the mapped procedure is declared irreversible.
    pub irreversible: bool,
}

/// One system-DB policy row (`sys_policies`). Keyed by name **within the /sys store**.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SysPolicyRow {
    /// The policy name (the row key).
    pub name: String,
    /// The allowed verb set, as stored text (e.g. `SELECT`).
    pub allow: Option<String>,
    /// The target path glob (e.g. `/sql/*`).
    pub target: Option<String>,
}

/// One defined-path binding row (`path_binding`, project DB). Selectors + metadata only; an
/// alias row carries `alias_of` and no driver, a full binding carries a driver.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathBindingRow {
    /// The defined path (the row key), e.g. `/chat`.
    pub path: String,
    /// The bound driver kind (a full binding); `None` for an alias.
    pub driver: Option<String>,
    /// The non-secret locator (`AT '<loc>'`).
    pub at: Option<String>,
    /// The secret REFERENCE (`env:`/`vault:`), never a value.
    pub secret_ref: Option<String>,
    /// The alias target path (an alias binding); `None` for a full binding.
    pub alias_of: Option<String>,
    /// The owning qfs host (ADR 0008); absent = the implicit `local`.
    pub host: Option<String>,
    /// The bound service-account LABEL (never a token).
    pub account: Option<String>,
    /// The bound app label.
    pub app: Option<String>,
}

/// One transform-definition row (`sys_transforms`, blueprint §15). Definition text + selectors + a
/// secret REFERENCE only — the derived mode is NOT a column (it is a pure function of `input`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TransformRow {
    /// The definition name (the row key).
    pub name: String,
    /// The declared INPUT schema as column-descriptor JSON.
    pub input: String,
    /// The declared OUTPUT schema as column-descriptor JSON.
    pub output: String,
    /// The model provider selector (never a token).
    pub provider: String,
    /// The model name/id.
    pub model: String,
    /// The optional effort/budget hint.
    pub effort: Option<String>,
    /// The optional secret REFERENCE (`env:`/`vault:`), never a value.
    pub secret_ref: Option<String>,
}

/// Which config collection a reconcile op targets — the dump-covered set minus the excluded
/// collections (billing / ddl events are unrepresentable here by design). Mostly `/sys/*`, plus the
/// top-level `/transform` definitions (blueprint §15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SysCollection {
    /// `sys_drivers` ↔ `/sys/drivers`.
    Drivers,
    /// `sys_policies` ↔ `/sys/policies`.
    Policies,
    /// `sys_settings` ↔ `/sys/settings` (non-secretish only).
    Settings,
    /// `path_binding` ↔ `/sys/paths`.
    Paths,
    /// `sys_transforms` ↔ the top-level `/transform` mount (blueprint §15, decision W).
    Transforms,
}

impl SysCollection {
    /// The collection segment (`drivers`/`policies`/…/`transforms`).
    #[must_use]
    pub const fn segment(self) -> &'static str {
        match self {
            Self::Drivers => "drivers",
            Self::Policies => "policies",
            Self::Settings => "settings",
            Self::Paths => "paths",
            Self::Transforms => "transforms",
        }
    }

    /// The fully-qualified mount path. `/sys/<segment>` for the administration collections; the
    /// top-level `/transform` for transform definitions (NOT under `/sys`).
    #[must_use]
    pub fn path(self) -> String {
        match self {
            Self::Transforms => "/transform".to_string(),
            _ => format!("/sys/{}", self.segment()),
        }
    }

    /// The row-key column of this collection (`name`/`key`/`path`) — what a `Remove` op's
    /// key-only projection carries.
    #[must_use]
    pub const fn key_column(self) -> &'static str {
        match self {
            Self::Drivers | Self::Policies | Self::Transforms => "name",
            Self::Settings => "key",
            Self::Paths => "path",
        }
    }
}

/// The config collections in the fixed order the emitter and diff engine walk them.
pub(crate) const SYS_COLLECTIONS: [SysCollection; 5] = [
    SysCollection::Drivers,
    SysCollection::Policies,
    SysCollection::Settings,
    SysCollection::Paths,
    SysCollection::Transforms,
];

/// The config projection of a declared-driver row. Optional columns are present-only, so an
/// absent field and a `NULL` column compare (and emit) identically.
#[must_use]
pub fn sys_driver_proj(row: &SysDriverRow) -> ProjRow {
    let mut p = ProjRow::default();
    p.set_text("name", &row.name);
    p.set_text("kind", &row.kind);
    set_opt(&mut p, "base_url", row.base_url.as_deref());
    set_opt(&mut p, "auth", row.auth.as_deref());
    set_opt(&mut p, "pagination", row.pagination.as_deref());
    set_opt(&mut p, "of_type", row.of_type.as_deref());
    set_opt(&mut p, "verb", row.verb.as_deref());
    set_opt(&mut p, "body", row.body.as_deref());
    p.set("irreversible", Value::Bool(row.irreversible));
    p
}

/// The config projection of a sys policy row.
#[must_use]
pub fn sys_policy_proj(row: &SysPolicyRow) -> ProjRow {
    let mut p = ProjRow::default();
    p.set_text("name", &row.name);
    set_opt(&mut p, "allow", row.allow.as_deref());
    set_opt(&mut p, "target", row.target.as_deref());
    p
}

/// The config projection of one (non-secretish) setting.
#[must_use]
pub fn sys_setting_proj(key: &str, value: &str) -> ProjRow {
    let mut p = ProjRow::default();
    p.set_text("key", key);
    p.set_text("value", value);
    p
}

/// The config projection of a path-binding row.
#[must_use]
pub fn path_binding_proj(row: &PathBindingRow) -> ProjRow {
    let mut p = ProjRow::default();
    p.set_text("path", &row.path);
    set_opt(&mut p, "driver", row.driver.as_deref());
    set_opt(&mut p, "at", row.at.as_deref());
    set_opt(&mut p, "secret_ref", row.secret_ref.as_deref());
    set_opt(&mut p, "alias_of", row.alias_of.as_deref());
    set_opt(&mut p, "host", row.host.as_deref());
    set_opt(&mut p, "account", row.account.as_deref());
    set_opt(&mut p, "app", row.app.as_deref());
    p
}

/// The config projection of a transform-definition row. The derived `mode` is deliberately NOT
/// projected — it is a pure function of `input`, so emitting it would be redundant drift.
#[must_use]
pub fn sys_transform_proj(row: &TransformRow) -> ProjRow {
    let mut p = ProjRow::default();
    p.set_text("name", &row.name);
    p.set_text("input", &row.input);
    p.set_text("output", &row.output);
    p.set_text("provider", &row.provider);
    p.set_text("model", &row.model);
    set_opt(&mut p, "effort", row.effort.as_deref());
    set_opt(&mut p, "secret_ref", row.secret_ref.as_deref());
    p
}

fn set_opt(p: &mut ProjRow, key: &str, value: Option<&str>) {
    if let Some(v) = value {
        if !v.is_empty() {
            p.set_text(key, v);
        }
    }
}

/// The name→projection map of one `/sys` collection, keyed and sorted by row key. Secretish
/// settings are filtered here too (defense in depth: even a programmatically-built [`SysState`]
/// carrying one can never reach the emitter or the diff).
pub(crate) fn sys_collection_projs(
    sys: &SysState,
    coll: SysCollection,
) -> BTreeMap<String, ProjRow> {
    match coll {
        SysCollection::Drivers => sys
            .drivers
            .iter()
            .map(|(k, r)| (k.clone(), sys_driver_proj(r)))
            .collect(),
        SysCollection::Policies => sys
            .policies
            .iter()
            .map(|(k, r)| (k.clone(), sys_policy_proj(r)))
            .collect(),
        SysCollection::Settings => sys
            .settings
            .iter()
            .filter(|(k, _)| !secretish_setting_key(k))
            .map(|(k, v)| (k.clone(), sys_setting_proj(k, v)))
            .collect(),
        SysCollection::Paths => sys
            .bindings
            .iter()
            .map(|(k, r)| (k.clone(), path_binding_proj(r)))
            .collect(),
        SysCollection::Transforms => sys
            .transforms
            .iter()
            .map(|(k, r)| (k.clone(), sys_transform_proj(r)))
            .collect(),
    }
}
