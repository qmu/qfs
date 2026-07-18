---
created_at: 2026-07-18T20:33:30+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Added
depends_on: []
mission: support-create-agent-semantics-that-introduce-a-new-user-principal-with-query-functions-scheduled-launch-and-access-control-to-resources
---

# Write the ruled agent-model chapter in docs/blueprint.md: an agent is a principal, not a process

## Overview

Add the agent-model chapter to `docs/blueprint.md` recording the owner's rulings (2026-07-18)
on all five axes of agent semantics, WITH the reasons for each and the rejected alternatives,
written in the blueprint's existing decided/implemented marker style. The chapter is the single
living design record every later ticket in this mission implements against.

The five ruled axes, each named with the concrete seam it constrains:

1. **Row home — /server/agents binding rows.** An agent's declared shape lands as
   ServerBindingDdl rows on the `/server/agents` surface (`core/src/ddl/server.rs`), read back
   beside `/server/jobs`. Rejected: a durable `/sys` identity now — recorded instead as the
   FUTURE federation seam (promotion of an agent binding to a durable `/sys` principal).
2. **Subject — a new Subject::Agent variant.** The agent is a first-class policy subject via
   `Subject::Agent` plus `DecisionContext::for_agent` (`server/src/policy/model.rs`,
   `context.rs`), NOT a reused user/role. Rejected: modelling the agent as a service user —
   it would blur the audit identity and the default-deny reasoning.
3. **Query functions — saved-plan registry rows.** An agent function is a named saved plan
   (the JobDecl `DO <plan>` body shape WITHOUT a cadence), a GATED statement — not a lambda.
   The §5.9 pure-lambda effects ban STANDS. Rejected: functions as pure lambdas — they would
   escape the preview/commit gate.
4. **Fire chain — DecisionContext threaded through qfs_watchtower::Committer.** The pure
   enforcer runs `evaluate_with_context` under the agent subject; the fire path is
   `qfs/src/sweeper.rs` + `watchtower::Committer`. IrreversibleGuard (RunMode::Server +
   Ack::Absent) already refuses irreversible plans on a timer — recorded as a ruled property:
   **an agent can NEVER fire an irreversible plan unattended.**
5. **Secret posture — policy-subject only, daemon-mediated.** No second vault; the agent's
   reach is exactly its `ALLOW…AT` grants evaluated against its subject, at the §8 store
   boundary. Rejected: a per-agent credential store — recorded instead as the FUTURE seam
   (per-agent credential handles).

Out-of-scope items (federation, delegation chains, per-agent credential handles) are recorded
as named seams, not omissions.

## Policies

- Blueprint over ADR pile: one living design chapter in `docs/blueprint.md`, no new numbered ADRs — git holds the history.
- Design decisions need full writeup: state each ruling, its reasons, its rejected alternatives, and the file it constrains — not a compressed summary.
- Experimental, no backward compatibility: record hard breaks as correct; do not hedge with migration/deprecation framing.

## Quality Gate

1. The chapter names each of the five rulings with its reasons AND its rejected alternatives.
2. Each ruling cites the concrete source file it constrains (`core/src/ddl/server.rs`, `server/src/policy/{model,context,enforce}.rs`, `qfs/src/sweeper.rs` + `watchtower::Committer`, the §8 store boundary).
3. The ruled irreversible property is recorded verbatim (RunMode::Server + Ack::Absent → an agent never fires an irreversible plan unattended).
4. Out-of-scope items (federation, delegation chains, per-agent credential handles) appear as named FUTURE seams.
5. No later ticket in this mission contradicts the chapter.
6. `cargo run -p xtask -- gen-docs --check` still passes (the blueprint is hand-written; generated reference docs are untouched).
7. Verification: chapter review against these rulings.

## Considerations

- The blueprint is hand-authored prose; do not run it through gen-docs — only confirm gen-docs still passes so the generated reference docs did not drift.
- Match the existing decided/implemented marker convention already used in `docs/blueprint.md`; do not invent a new heading style.
- This chapter is the contract for tickets 20260718203331…203335; write the seam names exactly as those tickets cite them so implementers land on the same files.
