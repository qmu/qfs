---
type: Mission
title: support CREATE AGENT semantics that introduce a new user principal with query functions, scheduled launch, and access control to resources
slug: support-create-agent-semantics-that-introduce-a-new-user-principal-with-query-functions-scheduled-launch-and-access-control-to-resources
status: active
created_at: 2026-07-12T04:34:26+09:00
author: a@qmu.jp
assignee: a@qmu.jp
tickets: []
stories: []
concerns: []
gate_type:
gate_target: /blueprint
gate_assert: North star, not a machine check — an agent is a principal with its own grant and audit trail, and the blueprint's agent-model chapter rules it. Verified per ticket, not by reading the page.
---

# support CREATE AGENT semantics that introduce a new user principal with query functions, scheduled launch, and access control to resources

## Goal

Make an **agent** a first-class qfs citizen: `CREATE AGENT <name> …` introduces a NEW PRINCIPAL —
an identity distinct from the operator who created it — that can hold query functions, launch on
a schedule, and reach exactly the resources its policy grants (owner directive, 2026-07-12).

Today every qfs execution runs as the operator: one vault, one policy context, one audit
identity. That is the wrong shape for delegated automation — an AI agent (or any unattended
process) should act as *itself*, under *its own* least-privilege grant, leaving *its own* audit
trail. The building blocks already ship and this mission composes them into one declarative
surface:

1. **A new principal.** The t57 policy axes (`FOR <subject>`, `AT <scope>`, `member_of`,
   `DecisionContext`) already model subjects; the directories driver models identities. An agent
   becomes a durable, named subject the policy engine gates on — never an ambient alias for the
   operator.
2. **Query functions.** The agent's capabilities are qfs queries — saved, named, previewable
   plans (the `LET`-lambda / saved-plan vocabulary), declared in the language itself the way
   declared drivers are. What an agent *can do* is readable as data, exactly like `/server` and
   `/sys/drivers` rows ("the server is a driver": config is data, fetchable and reconcilable).
3. **Scheduled launch.** The daemon now owns the "when" (the v0.0.59 cron sweeper: pure
   `fire_due` decision, live committer, durable `last_run`, `/server/jobs/<name>/runs`
   read-back). An agent's launch rides the SAME chain — a cadence fires the agent's plan under
   the agent's OWN principal, through the same policy gate and IrreversibleGuard, never a new
   scheduler.
4. **Access control to resources.** Default-deny stays the floor: an agent with no policy can do
   nothing. Grants name resources by path pattern (the existing `ALLOW <verbs> ON <glob>` rule
   grammar), and the agent's identity — not the operator's — is what the gate and the audit
   ledger see.

## Scope

**Done when** every acceptance item below is ticked: the blueprint records the ruled agent model
(identity, storage, gating, secret posture), `CREATE AGENT` parses and desugars onto the
closed-core registry shape, an agent's query functions and scheduled launch execute under the
agent principal through the shipped gate chain, resource access is enforced default-deny against
the agent subject, and the whole loop is verified live at least once in an owner-attended round.

**Out of scope:**

- Multi-tenant / network identity federation (OIDC for agents, cross-daemon agent identity) —
  the principal is daemon-local this mission.
- Agent-to-agent delegation chains and privilege-escalation semantics beyond a single
  operator→agent grant.
- New model-calling capability: agents *use* the shipped `transform`/`switch` stages; this
  mission adds no new model seam.
- A console/dashboard face for agents (read-back is the relational surface first).

## Acceptance

### Design (blueprint-first)

- [ ] The blueprint gains a ruled agent-model chapter: what an agent IS (a principal, not a
      process), where its rows live (`/server/agents` vs `/sys/…` — decided with reasons), how
      its identity reaches `DecisionContext`/`Subject`, and its secret posture (an agent never
      holds the operator's vault; grants reference handles) (ticket to be cut at /ticket time)

### Grammar + registry

- [ ] `CREATE AGENT <name> …` parses on the closed core (contextual identifiers, keyword freeze
      intact), desugars to registry rows like every other binding, and round-trips through
      dump/restore; `DESCRIBE` lists agents credential-free (ticket to be cut at /ticket time)

### Query functions

- [ ] An agent declares named query functions (saved plans) readable as data; invoking one
      previews by default and commits through the standard gates (ticket to be cut at /ticket
      time)

### Scheduled launch

- [ ] An agent with a launch cadence fires on the shipped daemon sweeper UNDER THE AGENT
      PRINCIPAL — its runs land on the agent's own run-history read-back, hermetic-first
      (ticket to be cut at /ticket time)

### Access control

- [ ] The policy gate evaluates the AGENT as subject: a resource the agent's policy does not
      grant is denied (default-deny floor) even when the operator could reach it; the audit
      ledger records the agent identity on every fired plan (ticket to be cut at /ticket time)

### Live proof

- [ ] One owner-attended live round: a scheduled agent with a narrow grant runs a real query
      function end to end, its denied over-reach visibly recorded (ticket to be cut at /ticket
      time)

## Changelog

- 2026-07-12 — mission created; goal/scope/acceptance drafted for owner review — mission.md
- 2026-07-16 — Gate demoted from `documentation` to none (owner directive: a mission's gate should
  be thin at the start and revised as its tickets run). Two reasons it was never a real check.
  A documentation gate verifies that **someone wrote the right words**, not that the product works —
  and the words here are hand-written prose, so it passes whenever the page is edited to agree with
  itself. Worse, the sibling `claude-code-sessions-…` mission proved the failure mode is not
  hypothetical: `DESCRIBE` and `docs/drivers.md` render from `compiled_describe_registry`
  (`describe.rs:283`), which never touches the mount registry, so a docs gate there would pass
  **today** against a driver that cannot be read at all. Separately, `gate.sh` returns no port for
  any mission here, so a live gate could not be driven either. `gate_target`/`gate_assert` are kept
  as the mission's north star; the real verification lives in each ticket's Quality Gate, which is
  written when the source has actually been read.
- 2026-07-16 — **These six acceptance items have never been re-litigated against the source.** They
  were drafted on 2026-07-12 from the owner directive, in the same "write the checklist up front from
  a summary" style that the sibling `declared-drivers-…` mission used — and when that one was
  checked against the source on 2026-07-16, **three of its seven items were wrong**: one named a
  parser that was already correct, one described a mount as a cred-free placeholder when it is a
  live compiled driver, and a correction to a third over-credited a splitter that is blind to
  escapes, path tokens, `#` comments and line numbers. Treat the items below as headings, not
  findings. Re-check each against the code before cutting its ticket; do not paraphrase them into a
  ticket.
