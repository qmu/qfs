//! `qfs agent run` — invoke an agent's **query function** (blueprint §19 axis C): a named saved
//! plan on the `/server/agents` surface, built + gated + committed through the SAME preview/commit
//! pipeline a JOB run uses, with ONE difference — the plan is evaluated under the **agent's own**
//! [`qfs_host::DecisionContext::for_agent`], never the operator's (blueprint §19 axis B).
//!
//! This is the binary's composition root for the invocation, injected into `qfs-cmd` via the
//! [`qfs_cmd::AgentLauncher`] (the `JobLauncher` pattern), so `qfs-cmd` stays off
//! `qfs-host`/`qfs-exec`/`qfs-runtime`.
//!
//! ## The safety floor (identical to `qfs job run`, plus the agent subject)
//!   1. **PREVIEW by default** — without `--commit` it prints the effect summary and applies nothing.
//!   2. **Policy gate under the AGENT subject** — the agent's bound `POLICY` is resolved and the
//!      built plan is gated by `evaluate_with_context(..., DecisionContext::for_agent(agent))` BEFORE
//!      any apply. A function touching an ungranted path is denied with the agent named in the
//!      reason (default-deny / atomic abort). No policy / a dangling ref ⇒ fail-closed default-deny.
//!   3. **IrreversibleGuard** — an agent function commit is unattended ([`RunMode::Server`]), so an
//!      irreversible `REMOVE` / `CALL` needs the explicit `--commit-irreversible` ack, fail-closed —
//!      the ruled property (blueprint §19): an agent never fires an irreversible plan unattended.
//!   4. **The real applier** — the commit runs through [`crate::commit::apply_plan`], the same
//!      runtime interpreter `qfs run --commit` uses. No new execution semantics — the §5.9
//!      pure-lambda effects ban stands: an agent function is a gated statement, not a lambda.

use qfs_cmd::{AgentAction, AgentRequest};
use qfs_core::{Ack, IrreversibleGuard, PlanSpec, RunMode};
use qfs_host::DecisionContext;

/// The injected [`qfs_cmd::AgentLauncher`] body: route a parsed `qfs agent <verb>` request. Returns
/// the process exit code; never panics.
#[must_use]
pub fn run_agent_request(req: &AgentRequest) -> i32 {
    match req.action {
        AgentAction::Run => run_agent(req),
    }
}

/// `qfs agent run <config> <agent> [--commit] [--commit-irreversible]`: boot the config, rehydrate
/// the agent's saved query function, build it, gate it under the AGENT's subject (policy +
/// irreversible), and commit it ONCE through the real applier. PREVIEW (no apply) unless `--commit`.
fn run_agent(req: &AgentRequest) -> i32 {
    let cfg = match qfs_host::agents_from_config(&req.config) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("qfs agent run: {e}");
            return 1;
        }
    };
    let Some(agent) = cfg.find(&req.name) else {
        eprintln!(
            "qfs agent run: no agent named {:?} in {}",
            req.name,
            req.config.display()
        );
        return 1;
    };
    if !agent.has_function() {
        eprintln!(
            "qfs agent run: agent {:?} declares no query function (a `DO <plan>` body)",
            req.name
        );
        return 1;
    }

    // Rehydrate the saved query-function body (canonical PlanSpec, NO re-parse) into its statement.
    let spec = match PlanSpec::from_canonical(&agent.plan_canonical) {
        Ok(spec) => spec,
        Err(e) => {
            eprintln!("qfs agent run: saved function not rehydratable: {e}");
            return 1;
        }
    };

    // Build the plan against the run engine (resolve + plan construction only, no I/O).
    let (engine, _reads, _safety) = crate::shell::run_engine_and_reads();
    let plan = match qfs_exec::build_plan(spec.statement(), &engine) {
        Ok(plan) => plan,
        Err(e) => {
            eprintln!("qfs agent run: plan build failed: {e}");
            return e.exit_code().code();
        }
    };

    // Policy gate UNDER THE AGENT SUBJECT (blueprint §19 axis B): resolve the agent's bound POLICY
    // and gate the built plan with `DecisionContext::for_agent` — an agent function commits by the
    // agent's grants, never the invoking operator's. Fail-closed default-deny / atomic abort on
    // deny (the commit is never reached, so ZERO effects apply).
    let policy = qfs_host::resolve_policy(agent.policy.as_deref(), &cfg.policies);
    let ctx = DecisionContext::for_agent(&agent.name);
    let gate = qfs_host::gate_plan_with_context(&policy, &plan, &ctx);
    if !gate.is_allow() {
        eprintln!(
            "qfs agent run: policy denied: {}",
            gate.deny_reason().unwrap_or_default()
        );
        return 1;
    }

    // PREVIEW by default (the preview→commit floor): print the secret-free effect summary and exit
    // without applying.
    if !req.commit {
        if !req.quiet {
            println!(
                "PREVIEW agent '{}' function (policy {}, {} effect(s); nothing applied — pass --commit):",
                agent.name,
                agent.policy.as_deref().unwrap_or("<none>"),
                gate.effects.len()
            );
            for e in &gate.effects {
                println!("  {e}");
            }
        }
        return 0;
    }

    // IrreversibleGuard (blueprint §19 ruled property): an agent function commit is UNATTENDED
    // (`RunMode::Server`) — an irreversible REMOVE / declared-irreversible CALL needs the explicit
    // `--commit-irreversible` ack, fail-closed without it. An agent never fires an irreversible plan
    // unattended.
    let ack = if req.commit_irreversible {
        Ack::Granted
    } else {
        Ack::Absent
    };
    if let Err(needs) = IrreversibleGuard::require_ack(&plan, RunMode::Server, ack) {
        eprintln!("qfs agent run: {}", needs.reason());
        return 1;
    }

    // Commit ONCE through the real runtime applier (the same path `qfs run --commit` uses — no
    // privileged shortcut, no new execution semantics).
    match crate::commit::apply_plan(&plan) {
        Ok(()) => {
            if !req.quiet {
                println!(
                    "COMMITTED agent '{}' function ({} effect(s) applied)",
                    agent.name,
                    gate.effects.len()
                );
            }
            0
        }
        Err(e) => {
            eprintln!("qfs agent run: commit failed: {e}");
            e.exit_code().code()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::{Path, PathBuf};

    fn write_config(dir: &Path, src: &str) -> PathBuf {
        let path = dir.join("app.qfs");
        let mut f = std::fs::File::create(&path).expect("create config");
        f.write_all(src.as_bytes()).expect("write config");
        path
    }

    fn req(config: PathBuf, name: &str, commit: bool, ack: bool) -> AgentRequest {
        AgentRequest {
            action: AgentAction::Run,
            config,
            name: name.to_string(),
            commit,
            commit_irreversible: ack,
            json: false,
            format: None,
            quiet: true,
        }
    }

    /// PREVIEW by default (blueprint §19 axis C): no `--commit` applies nothing, even with a
    /// permissive agent policy — invocation without `--commit` produces zero effects.
    #[test]
    fn agent_run_previews_without_commit() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let out = dir.path().join("preview.txt");
        let cfg = write_config(
            dir.path(),
            &format!(
                "CREATE POLICY narrow ALLOW UPSERT ON local;\n\
                 CREATE AGENT triage DO UPSERT INTO /local{} VALUES ('x') POLICY narrow;\n",
                out.display()
            ),
        );
        let code = run_agent(&req(cfg, "triage", false, false));
        assert_eq!(code, 0, "preview exits 0");
        assert!(!out.exists(), "PREVIEW applies nothing (zero effects)");
    }

    /// WITH `--commit`, an in-grant function commits ONCE through the agent-subject gate + the real
    /// applier — the file is written.
    #[test]
    fn agent_run_commits_an_in_grant_function() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let out = dir.path().join("done.txt");
        let cfg = write_config(
            dir.path(),
            &format!(
                "CREATE POLICY narrow ALLOW UPSERT ON local;\n\
                 CREATE AGENT triage DO UPSERT INTO /local{} VALUES ('scheduled') POLICY narrow;\n",
                out.display()
            ),
        );
        let code = run_agent(&req(cfg, "triage", true, false));
        assert_eq!(code, 0, "an in-grant agent function commits");
        assert_eq!(
            std::fs::read_to_string(&out).expect("the committed file exists"),
            "scheduled"
        );
    }

    /// A function touching an UNGRANTED path is DENIED under the agent subject — atomic abort, zero
    /// effects. (The agent's policy grants UPSERT on `local` but the function writes to an ungranted
    /// driver.)
    #[test]
    fn agent_run_denies_an_ungranted_function() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let out = dir.path().join("blocked.txt");
        std::fs::write(&out, b"safe").unwrap();
        let cfg = write_config(
            dir.path(),
            // The agent may UPSERT on `s3` only; its function writes to `local` — denied.
            &format!(
                "CREATE POLICY narrow ALLOW UPSERT ON s3;\n\
                 CREATE AGENT triage DO UPSERT INTO /local{} VALUES ('x') POLICY narrow;\n",
                out.display()
            ),
        );
        let code = run_agent(&req(cfg, "triage", true, true));
        assert_eq!(code, 1, "an out-of-grant function is denied");
        assert_eq!(
            std::fs::read_to_string(&out).expect("file untouched"),
            "safe",
            "atomic abort: nothing applied"
        );
    }

    /// An agent with NO attached policy is fail-closed default-denied (no grant ⇒ deny), even for a
    /// reversible function.
    #[test]
    fn agent_run_no_policy_is_default_denied() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let out = dir.path().join("nopolicy.txt");
        let cfg = write_config(
            dir.path(),
            &format!(
                "CREATE AGENT triage DO UPSERT INTO /local{} VALUES ('x');\n",
                out.display()
            ),
        );
        let code = run_agent(&req(cfg, "triage", true, false));
        assert_eq!(code, 1, "no policy ⇒ fail-closed default-deny");
        assert!(!out.exists(), "nothing applied");
    }

    /// A missing agent / a function-less agent are clean errors, not panics.
    #[test]
    fn agent_run_missing_or_functionless_errors() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let cfg = write_config(dir.path(), "CREATE AGENT bare;\n");
        assert_eq!(run_agent(&req(cfg.clone(), "absent", true, true)), 1);
        assert_eq!(
            run_agent(&req(cfg, "bare", true, true)),
            1,
            "a function-less agent is a clean error"
        );
    }
}
