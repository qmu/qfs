//! The §13 declared-driver **apply facet** (blueprint tier 2 — the write twin of
//! [`crate::read_facets::RestReadDriver`]): applying a universal write verb on a declared mount
//! **evaluates the matching MAP's stored body**. A `CREATE MAP INSERT /slack/post AS INSERT INTO
//! /http/slack/chat.postMessage VALUES ({channel: row.channel, text: row.text})` maps each incoming
//! row into the exact wire body Slack expects — the mount path (`/slack/post`) decoupled from the
//! wire method (`chat.postMessage`), exactly as a tier-2 view decouples its mount from its endpoint.
//!
//! ## Where the logic lives (the same topology as the read facet)
//! The evaluation itself (rehydrate the stored effect, confine its `/http/<self>/…` target, lower
//! the `VALUES (<expr>)` mapping, evaluate it per incoming row) lives in
//! [`qfs_exec::declared::eval_map_body`] — off the binary's lower spine. This facet owns only the
//! async boundary + the glue: recover the mount-relative path, match it against the declared maps,
//! encode each evaluated wire body, and POST it through the **stock confined REST applier** (the
//! wrapped `inner` bridge), whose `send_one` chokepoint pins the host and injects auth. Purity
//! holds: the mapping constructs the wire effect; only the applier performs I/O at COMMIT.
//!
//! ## `CALL` dispatch (blueprint §13.1 G5)
//! A declared `CREATE MAP CALL <drv>.<action> (<sig>) /<node> AS INSERT INTO /http/<drv>/<method>
//! VALUES (…)` is dispatched here too: a `CALL` effect selects the map whose declared **action**
//! matches the called procedure (never merely its mount path — the five Slack CALLs share one mount
//! path with the post map), binds the call's argument row as `row`, and issues the wire write the
//! body names. `Call` is not a kind the generic REST driver services, so the wire effect carries the
//! body's OWN verb ([`qfs_exec::declared::MapWrite::wire_kind`]) rather than the CALL kind.
//!
//! ## Fallbacks (tier-1 compatibility, fail-honest)
//! A write matching **no** declared map, or matching a map with an **empty** stored body (a tier-1
//! map that declares only the verb, mount path == wire resource), delegates to the stock applier
//! unchanged. A map whose body fails to evaluate is a terminal effect error — never a silent POST.
//! A `CALL` that matches no declared CALL map likewise reaches the stock applier, which refuses the
//! procedure verb terminally: an unmapped CALL fails, it never POSTs something else.

use std::sync::Arc;

use qfs_core::EffectKind;
use qfs_exec::declared::MapSpec;
use qfs_runtime::{ApplyCx, ApplyDriver, EffectError, EffectInput, EffectOutput};

/// The declared-driver write facet: wraps the stock REST apply bridge and, for a write whose mount
/// path matches a declared MAP with a stored body, rewrites the effect into the confined wire
/// POST(s) the map body evaluates to (one per incoming row).
pub struct RestApplyDriver {
    /// The stock REST apply bridge (the confined `RestApplier` behind the async seam). All wire
    /// I/O — including host confinement + auth — happens inside it.
    inner: Arc<dyn ApplyDriver>,
    /// The declared driver's own name (`slack`) — the `/http/<name>` namespace map bodies confine to.
    driver_name: String,
    /// The declared write mappings (mount-path template + stored `VALUES (<expr>)` body).
    maps: Vec<MapSpec>,
}

impl RestApplyDriver {
    /// Build the write facet over the stock apply bridge plus the driver's resolved map specs.
    #[must_use]
    pub(crate) fn new(
        inner: Arc<dyn ApplyDriver>,
        driver_name: String,
        maps: Vec<MapSpec>,
    ) -> Self {
        Self {
            inner,
            driver_name,
            maps,
        }
    }
}

#[async_trait::async_trait]
impl ApplyDriver for RestApplyDriver {
    async fn apply_one(
        &self,
        effect: &EffectInput,
        cx: &ApplyCx,
    ) -> Result<EffectOutput, EffectError> {
        // Match the (remapped) effect path against the declared maps, binding `{param}` segments.
        // The inbound path arrives as `/rest/<name>/<resource>` (the `MountApplyDriver` remap); the
        // map template is the mount-relative `/<name>/<resource>`, so the `/rest` prefix is stripped.
        let mount_path = qfs_exec::declared::view_path_of_scan(effect.target.path.as_str());
        // A CALL selects by the PROCEDURE it names (the mount remap re-qualified it inward, so the
        // action is the tail of the proc id); a universal write selects by mount path among the
        // non-CALL maps only.
        let called = call_action_of(&effect.kind);
        let matched = self.maps.iter().find_map(|m| {
            if !map_answers(m, called) {
                return None;
            }
            qfs_exec::declared::match_template(&m.template, &mount_path).map(|params| (m, params))
        });
        let Some((spec, params)) = matched else {
            // No declared map matches this write — the stock applier POSTs it as-is (tier-1).
            return self.inner.apply_one(effect, cx).await;
        };
        if spec.body.is_empty() {
            // A tier-1 map (verb only, no body transformation): mount path == wire resource.
            return self.inner.apply_one(effect, cx).await;
        }

        // Evaluate the map body per incoming row → the confined `/rest/<name>/<resource>` path and
        // one wire body each. A body that cannot be evaluated is a terminal effect error.
        let write = qfs_exec::declared::eval_map_body(
            &spec.body,
            &self.driver_name,
            &mount_path,
            &params,
            &effect.args,
        )
        .map_err(|e| {
            EffectError::terminal(format!("declared map body did not evaluate: {}", e.code()))
        })?;

        // POST each evaluated body through the stock confined applier, rewriting the effect to carry
        // the pre-encoded body under `__http_body` and to target the wire resource the body named.
        // The map's declared `ENCODE <fmt>` picks the encoder: `multipart` (the §13 upload
        // primitive — the args also carry the boundary-bearing Content-Type header override),
        // no encoding = the default JSON object. An unknown name is a terminal refusal, never a
        // silent JSON.
        let mut affected = 0u64;
        for body in &write.bodies {
            let mut wire = effect.clone();
            wire.target.path = qfs_core::VfsPath::new(&write.rest_path);
            if called.is_some() {
                // The procedure kind stops here: the wire leg is the write the map body declares.
                wire.kind = write.wire_kind.clone();
            }
            wire.args = match write.encoding.as_deref() {
                None => qfs_driver_http::http_body_args(body),
                Some("multipart") => {
                    qfs_driver_http::http_multipart_args(body).map_err(EffectError::terminal)?
                }
                Some(other) => {
                    return Err(EffectError::terminal(format!(
                        "declared map names unknown wire encoding `{other}` (supported: multipart)"
                    )))
                }
            };
            affected += self.inner.apply_one(&wire, cx).await?.affected;
        }
        Ok(EffectOutput::new(effect.id, affected))
    }
}

/// The unqualified action a `CALL` effect names (`rest.react` → `react`), or `None` for a universal
/// write verb. The proc id arrives already re-qualified inward by the mount remap, so only the tail
/// after the last `.` is the procedure's own name.
fn call_action_of(kind: &EffectKind) -> Option<&str> {
    match kind {
        EffectKind::Call(proc) => {
            let qualified = proc.as_str();
            Some(qualified.rsplit('.').next().unwrap_or(qualified))
        }
        _ => None,
    }
}

/// Whether a declared map answers this effect. A `CREATE MAP CALL <drv>.<action>` map answers ONLY
/// the CALL of that action; a universal-verb map answers only a universal write. Without the split
/// a plain `INSERT` would be served by whichever map was declared FIRST on the mount path — and the
/// five Slack CALL maps share `/slack/{ws}/{channel}/messages` with the post map.
fn map_answers(m: &MapSpec, called: Option<&str>) -> bool {
    match (qfs_exec::declared::call_action(&m.verb), called) {
        (Some(declared), Some(called)) => declared == called,
        (None, None) => true,
        _ => false,
    }
}
