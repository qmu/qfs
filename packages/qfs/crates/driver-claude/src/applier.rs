//! [`ClaudeApplier`] — the `/claude` driver's apply leg (blueprint §7). It lowers a write effect
//! node into the two gated mutations this driver ships: `INSERT INTO /claude/sessions` (a session
//! LAUNCH — irreversible, ticket 20260717010600, routed to the [`SessionLauncher`] seam) and
//! `INSERT INTO /claude/sessions/<id>/instructions` (a REVERSIBLE append that steers a running
//! agent, routed to the [`SessionSource`]). Every other write is rejected here
//! (belt-and-suspenders over the parse-time capability gate): neither node accepts `UPDATE`/
//! `REMOVE`.
//!
//! The real I/O happens in the injected [`SessionSource`] (read + steer) and [`SessionLauncher`]
//! (spawn) — both binary-side; the applier is a pure router over the owned effect node, so it is
//! stateless and `&self`-applies through the runtime's [`SharedApplier`] bridge.
//!
//! ## Safety floor (blueprint §15, decision W supersedes decision K)
//! Steering is an `Insert` (a reversible append) — it adds a message to the agent's instruction
//! log, it never removes session state. A session LAUNCH is also an `Insert`, but IRREVERSIBLE (the
//! turn runs, the spend lands), so the planner flags it and `COMMIT` gates it behind the
//! irreversible ack. "Stop the agent", if ever added, would be a separate `EffectKind::Remove`
//! (irreversible → extra acknowledgement), NEVER a silent reversible op. The launch spawns a
//! process via a configured binary; NEITHER path hosts an LLM call (the model-calling surface is
//! `|> transform` — §15 / decision W — in `qfs-driver-transform` + the binary, never this façade).

use qfs_plan::{AppliedEffect, ApplyError, EffectKind, EffectNode, PlanApplier};
use qfs_runtime::{EffectError, EffectOutput, SharedApplier};

use std::sync::Arc;

use crate::backend::{ClaudeError, LaunchSpec, SessionLauncher, SessionSource};
use crate::schema::{instruction_session, node_for_path, ClaudeNode};

/// The synchronous `/claude` apply leg. Holds the injected session source (read + steer) and the
/// optional launcher seam (session launch) behind `Arc`s (so the leg is cheap to clone and
/// `&self`-apply). Stateless across calls.
#[derive(Clone)]
pub struct ClaudeApplier {
    source: Arc<dyn SessionSource>,
    /// The launcher for `INSERT INTO /claude/sessions` (a session launch). `None` = fail-closed:
    /// a binary with no launcher configured refuses the spawn rather than launching nothing.
    launcher: Option<Arc<dyn SessionLauncher>>,
}

impl ClaudeApplier {
    /// Build an applier over an injected [`SessionSource`] (the binary's on-disk implementation).
    /// No launcher is wired by default — a session launch fails closed until [`Self::with_launcher`]
    /// supplies one (the opt-in posture of the whole `/claude` write surface).
    #[must_use]
    pub fn new(source: Arc<dyn SessionSource>) -> Self {
        Self {
            source,
            launcher: None,
        }
    }

    /// Wire the launcher seam (`INSERT INTO /claude/sessions` → spawn a new session). The real
    /// launcher (`Command::new(<configured binary>)`) lives binary-side; hermetic tests inject a
    /// fake that records the spawn without spending anything.
    #[must_use]
    pub fn with_launcher(mut self, launcher: Arc<dyn SessionLauncher>) -> Self {
        self.launcher = Some(launcher);
        self
    }

    /// Route one effect node: resolve the `/claude` node, gate the verb, and apply. Two writes are
    /// permitted — `INSERT INTO /claude/sessions` (a session LAUNCH, irreversible, via the launcher
    /// seam) and `INSERT INTO /claude/sessions/<id>/instructions` (a reversible steer). Everything
    /// else is a structured rejection (defence in depth over the parse-time capability gate: even a
    /// hand-built plan cannot `UPDATE`/`REMOVE` here).
    fn apply_node(&self, node: &EffectNode) -> Result<u64, ClaudeError> {
        let path = node.target.path.as_str();
        let claude_node = node_for_path(path).ok_or_else(|| ClaudeError::UnknownNode {
            path: path.to_string(),
        })?;

        match (&node.kind, claude_node) {
            // The launch: spawn a new session (irreversible). Fail-closed without a launcher.
            (EffectKind::Insert, ClaudeNode::Sessions) => {
                let launcher = self
                    .launcher
                    .as_ref()
                    .ok_or(ClaudeError::LaunchNotConfigured)?;
                let spec = LaunchSpec::from_row_batch(&node.args)?;
                // The launcher returns the new session id; the runtime effect channel carries only
                // an affected count, so the id surfaces through the seam (proven hermetically) and
                // the plan's `RETURNING id` schema types the projection. One session launched.
                launcher.launch(&spec)?;
                Ok(1)
            }
            // The steer: append a steering instruction to a session's log (reversible).
            (EffectKind::Insert, ClaudeNode::Instructions) => {
                let session =
                    instruction_session(path).ok_or_else(|| ClaudeError::UnknownNode {
                        path: path.to_string(),
                    })?;
                self.source.append_instruction(&session, &node.args)
            }
            // Everything else: UPDATE/REMOVE on either node. Reject in the applier too (defence in
            // depth over the parse-time capability gate).
            (kind, n) => Err(ClaudeError::Unsupported {
                node: n.label(),
                verb: static_verb_label(kind),
            }),
        }
    }
}

/// The stable `&'static str` label for an effect kind (the structured-error field is `&'static`).
fn static_verb_label(kind: &EffectKind) -> &'static str {
    match kind {
        EffectKind::Read => "READ",
        EffectKind::List => "LIST",
        EffectKind::Insert => "INSERT",
        EffectKind::Upsert => "UPSERT",
        EffectKind::Update => "UPDATE",
        EffectKind::Remove => "REMOVE",
        EffectKind::Call(_) => "CALL",
        _ => "WRITE",
    }
}

impl SharedApplier for ClaudeApplier {
    fn apply_shared(&self, node: &EffectNode) -> Result<EffectOutput, EffectError> {
        let affected = self
            .apply_node(node)
            .map_err(|e| EffectError::terminal(e.to_string()))?;
        Ok(EffectOutput::new(node.id, affected))
    }
}

impl PlanApplier for ClaudeApplier {
    /// The introspective `qfs_driver::Driver::applier()` seam (t09). Stateless, so it delegates to
    /// the same `&self` core as [`SharedApplier::apply_shared`]; the structured [`ClaudeError`] is
    /// reduced to the plan crate's owned `(id, reason)` shape — secret-free by construction.
    fn apply(&mut self, node: &EffectNode) -> Result<AppliedEffect, ApplyError> {
        let affected = self
            .apply_node(node)
            .map_err(|e| ApplyError::new(node.id, e.to_string()))?;
        Ok(AppliedEffect::new(node.id, affected))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use qfs_plan::{DriverId, NodeId, Target, VfsPath};
    use qfs_types::{Column, ColumnType, Row, RowBatch, Schema, Value};
    use std::sync::Mutex;

    use crate::schema::claude_node_schema;

    /// An in-memory fake source (no disk, no creds, no LLM): records the (session, instruction)
    /// pairs it was asked to append, so the applier's ROUTING can be proven without the binary's
    /// on-disk impl.
    #[derive(Default)]
    struct FakeSource {
        appended: Mutex<Vec<(String, RowBatch)>>,
    }

    impl SessionSource for FakeSource {
        fn scan_sessions(&self) -> Result<RowBatch, ClaudeError> {
            Ok(RowBatch::new(
                claude_node_schema(ClaudeNode::Sessions),
                vec![],
            ))
        }
        fn scan_instructions(&self, _session: &str) -> Result<RowBatch, ClaudeError> {
            Ok(RowBatch::new(
                claude_node_schema(ClaudeNode::Instructions),
                vec![],
            ))
        }
        fn append_instruction(&self, session: &str, row: &RowBatch) -> Result<u64, ClaudeError> {
            self.appended
                .lock()
                .unwrap()
                .push((session.to_string(), row.clone()));
            Ok(1)
        }
    }

    /// An in-memory fake launcher (no process, no spend): records every [`LaunchSpec`] it was asked
    /// to launch and hands back a canned session id, so the applier's LAUNCH routing + payload
    /// extraction can be proven without spawning a real session.
    #[derive(Default)]
    struct FakeLauncher {
        launched: Mutex<Vec<LaunchSpec>>,
    }

    impl SessionLauncher for FakeLauncher {
        fn launch(&self, spec: &LaunchSpec) -> Result<String, ClaudeError> {
            self.launched.lock().unwrap().push(spec.clone());
            Ok("s-new-42".to_string())
        }
    }

    fn instruction_row() -> RowBatch {
        let schema = Schema::new(vec![Column::new("instruction", ColumnType::Text, false)]);
        RowBatch::new(
            schema,
            vec![Row::new(vec![Value::Text(
                "focus on the failing test".into(),
            )])],
        )
    }

    /// An `INSERT INTO /claude/sessions (cwd, prompt [, name])` VALUES payload — the row the
    /// planner lowers, columns named by the explicit list.
    fn launch_row(cwd: &str, prompt: &str, name: Option<&str>) -> RowBatch {
        let mut cols = vec![
            Column::new("cwd", ColumnType::Text, false),
            Column::new("prompt", ColumnType::Text, false),
        ];
        let mut vals = vec![Value::Text(cwd.into()), Value::Text(prompt.into())];
        if let Some(n) = name {
            cols.push(Column::new("name", ColumnType::Text, true));
            vals.push(Value::Text(n.into()));
        }
        RowBatch::new(Schema::new(cols), vec![Row::new(vals)])
    }

    fn effect(kind: EffectKind, path: &str, args: RowBatch) -> EffectNode {
        EffectNode::new(
            NodeId(0),
            kind,
            Target::new(DriverId::new("claude"), VfsPath::new(path)),
        )
        .with_args(args)
    }

    #[test]
    fn insert_into_instructions_routes_to_the_source() {
        let source = Arc::new(FakeSource::default());
        let applier = ClaudeApplier::new(source.clone());
        let node = effect(
            EffectKind::Insert,
            "/claude/sessions/current/instructions",
            instruction_row(),
        );
        let out = applier
            .apply_shared(&node)
            .expect("instruction append applies");
        assert_eq!(out.affected, 1);
        let appended = source.appended.lock().unwrap();
        assert_eq!(appended.len(), 1, "row reached the source");
        assert_eq!(appended[0].0, "current", "the session id was routed");
    }

    #[test]
    fn launch_into_sessions_routes_to_the_launcher() {
        let launcher = Arc::new(FakeLauncher::default());
        let applier =
            ClaudeApplier::new(Arc::new(FakeSource::default())).with_launcher(launcher.clone());
        let node = effect(
            EffectKind::Insert,
            "/claude/sessions",
            launch_row("/home/dev/proj", "run the tests", Some("nightly")),
        );
        let out = applier.apply_shared(&node).expect("a launch applies");
        assert_eq!(out.affected, 1, "one session launched");
        let launched = launcher.launched.lock().unwrap();
        assert_eq!(launched.len(), 1, "the spec reached the launcher");
        assert_eq!(
            launched[0],
            LaunchSpec {
                cwd: "/home/dev/proj".into(),
                prompt: "run the tests".into(),
                name: Some("nightly".into()),
            },
            "cwd/prompt/name are extracted as DATA and passed through the seam"
        );
    }

    #[test]
    fn launch_accepts_an_omitted_name() {
        let launcher = Arc::new(FakeLauncher::default());
        let applier =
            ClaudeApplier::new(Arc::new(FakeSource::default())).with_launcher(launcher.clone());
        let node = effect(
            EffectKind::Insert,
            "/claude/sessions",
            launch_row("/home/dev/proj", "triage the backlog", None),
        );
        applier
            .apply_shared(&node)
            .expect("a nameless launch applies");
        assert_eq!(launcher.launched.lock().unwrap()[0].name, None);
    }

    #[test]
    fn launch_without_a_launcher_fails_closed() {
        // Fail-closed: no launcher wired ⇒ the sessions INSERT is refused, never a silent no-op.
        let applier = ClaudeApplier::new(Arc::new(FakeSource::default()));
        let node = effect(
            EffectKind::Insert,
            "/claude/sessions",
            launch_row("/home/dev/proj", "go", None),
        );
        let err = applier.apply_shared(&node).unwrap_err();
        // The structured error names the fail-closed reason and carries no secret.
        let msg = err.to_string();
        assert!(msg.contains("not configured"), "{msg}");
    }

    #[test]
    fn launch_with_a_malformed_payload_is_rejected() {
        // A launch missing the required `cwd`/`prompt` columns is refused (never spawned blank),
        // even with a launcher wired.
        let applier = ClaudeApplier::new(Arc::new(FakeSource::default()))
            .with_launcher(Arc::new(FakeLauncher::default()));
        let no_prompt = RowBatch::new(
            Schema::new(vec![Column::new("cwd", ColumnType::Text, false)]),
            vec![Row::new(vec![Value::Text("/home/dev/proj".into())])],
        );
        let node = effect(EffectKind::Insert, "/claude/sessions", no_prompt);
        assert!(applier.apply_shared(&node).is_err());
    }

    #[test]
    fn update_or_remove_on_instructions_is_rejected_in_the_applier() {
        // The append-log is append-only: "stop the agent" is NOT a silent reversible op. A hand-built
        // UPDATE/REMOVE is rejected in the applier (no source mutation happens).
        let applier = ClaudeApplier::new(Arc::new(FakeSource::default()));
        for kind in [EffectKind::Update, EffectKind::Remove] {
            let node = effect(
                kind,
                "/claude/sessions/current/instructions",
                RowBatch::new(Schema::new(vec![]), vec![]),
            );
            assert!(
                applier.apply_shared(&node).is_err(),
                "instructions log must reject a non-append write in the applier"
            );
        }
    }
}
