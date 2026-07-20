//! Agent query-function extraction (blueprint §19 axis C): project a booted `.qfs` config's
//! `/server/agents` rows into the owned [`AgentSpec`]s the binary invokes with `qfs agent run`.
//!
//! An agent is a **new user principal**, not a process (blueprint §19). Its query function is a
//! **named saved plan** — the `JobDecl` `DO <plan>` body shape WITHOUT a cadence. This module boots
//! the config through the t30 `qfs_server::Runtime` (behind `host-daemon`, like [`crate::job`]) and
//! reads each [`qfs_server::AgentDef`]'s saved `plan` body (the canonical [`qfs_core::PlanSpec`]
//! source the binary rehydrates + builds) plus its bound least-privilege `policy` handle.
//!
//! The invocation gate is the SAME preview/commit chain a JOB run uses, with ONE difference: the
//! plan is evaluated under the **agent's** [`qfs_server::DecisionContext::for_agent`], never the
//! operator's — an agent function commits under the agent's grants (blueprint §19 axis B). This
//! module NEVER fires anything itself.

use std::path::Path;

use qfs_server::{AgentDef, PolicyTable};

/// One saved `/server/agents` row, projected into owned, vendor-free strings — the **invokable
/// agent function**. Carries the saved query-function body (`plan_canonical`, the canonical
/// [`qfs_core::PlanSpec`] source the binary rehydrates + builds) and the bound least-privilege
/// `policy` handle the agent-subject commit is gated under.
#[derive(Debug, Clone)]
pub struct AgentSpec {
    /// The agent name (the `/server/agents` row key; the `Subject::Agent` identity the commit is
    /// gated under, and the handle `qfs agent run <config> <name>` uses).
    pub name: String,
    /// The bound `POLICY <name>` handle the agent-subject commit is gated under. `None` ⇒ no policy
    /// attached ⇒ fail-closed default-deny at run time (the least-privilege floor).
    pub policy: Option<String>,
    /// The saved query-function body as canonical [`qfs_core::PlanSpec`] source. The binary
    /// rehydrates it (`PlanSpec::from_canonical`, no re-parse), builds the plan, gates it under the
    /// agent subject, and commits once. Empty for a function-less agent.
    pub plan_canonical: String,
}

impl AgentSpec {
    /// Whether this agent declares a query function (a non-empty saved plan).
    #[must_use]
    pub fn has_function(&self) -> bool {
        !self.plan_canonical.is_empty()
    }
}

/// The agents of a booted config plus its live `/server/policies` table — everything an
/// `qfs agent run` invocation needs to resolve an agent's bound policy and gate its function under
/// the agent subject.
pub struct ConfigAgents {
    /// The `/server/agents` rows, projected into owned [`AgentSpec`]s.
    pub agents: Vec<AgentSpec>,
    /// The `/server/policies` table (`name → PolicyDef`) the bound `policy` handle resolves against.
    pub policies: PolicyTable,
}

impl ConfigAgents {
    /// Find an agent by name (the handle `qfs agent run` invokes it by).
    #[must_use]
    pub fn find(&self, name: &str) -> Option<&AgentSpec> {
        self.agents.iter().find(|a| a.name == name)
    }
}

/// Boot a `.qfs` config (the in-memory parse→lower→COMMIT path) and extract its `/server/agents`
/// rows + policy table. The terminal binary calls this so it never names `qfs-server` directly (the
/// dep-direction guard), exactly like [`crate::job::jobs_from_config`].
///
/// # Errors
/// A secret-free, line-located error string on any read / parse / lower / commit failure.
pub fn agents_from_config(config: &Path) -> Result<ConfigAgents, String> {
    let mut rt = qfs_server::Runtime::new();
    rt.boot(config).map_err(|e| format!("boot: {e}"))?;
    let state = rt.snapshot();
    let agents = state.agents.values().map(agent_spec).collect();
    Ok(ConfigAgents {
        agents,
        policies: state.policies,
    })
}

/// Project one [`AgentDef`] into the owned [`AgentSpec`] (saved query-function body → canonical
/// source). No firing, no rehydration here — the binary owns the rehydrate + build + gate + commit.
fn agent_spec(def: &AgentDef) -> AgentSpec {
    AgentSpec {
        name: def.name.clone(),
        policy: def.policy.clone(),
        plan_canonical: def.plan.as_str().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Write a `.qfs` config to a tempfile and boot it.
    fn boot(src: &str) -> ConfigAgents {
        let mut f = tempfile::NamedTempFile::new().expect("tempfile");
        f.write_all(src.as_bytes()).expect("write config");
        agents_from_config(f.path()).expect("boot config")
    }

    #[test]
    fn extracts_an_agent_function_and_its_policy() {
        // blueprint §19 axis C: the agent's query function round-trips as a `/server/agents` row —
        // its saved plan body + bound policy, credential-free.
        let cfg = boot(
            "CREATE POLICY narrow ALLOW INSERT ON local;\n\
             CREATE AGENT triage DO INSERT INTO /local/tmp/a.txt VALUES ('x') POLICY narrow;\n",
        );
        assert_eq!(cfg.agents.len(), 1, "one saved agent row");
        let agent = cfg.find("triage").expect("agent by name");
        assert_eq!(agent.policy.as_deref(), Some("narrow"));
        assert!(
            agent.has_function(),
            "the saved query function body is carried"
        );
        assert!(cfg.policies.contains_key("narrow"), "policy table carried");
    }

    #[test]
    fn missing_agent_is_none_not_a_panic() {
        let cfg = boot("CREATE AGENT triage;\n");
        assert!(cfg.find("absent").is_none());
        let a = cfg.find("triage").expect("triage");
        assert!(!a.has_function(), "a function-less agent has no saved plan");
    }
}
