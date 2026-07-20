---
created_at: 2026-07-18T20:33:32+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Added
depends_on: [20260718203330-agent-model-blueprint-chapter.md, 20260718203331-create-agent-grammar-registry.md]
mission: support-create-agent-semantics-that-introduce-a-new-user-principal-with-query-functions-scheduled-launch-and-access-control-to-resources
---

# Subject::Agent in the t57 policy model; the enforcer and audit ledger see the agent identity

## Overview

Make the agent a first-class policy subject so the pure enforcer and the audit ledger see the
agent identity distinct from any operator.

Concrete work:

- Extend `server/src/policy/model.rs` with the `Subject::Agent` variant.
- Add a `DecisionContext::for_agent` constructor in `context.rs`.
- Resolve `FOR <agent-subject>` through the existing `ast.rs:547` FOR clause (no new grammar for
  the FOR target beyond routing to the agent subject).
- `evaluate_with_context` (`enforce.rs:211`) stays PURE — no I/O added; the default-deny floor
  holds: an agent with no matching rule is denied even on a path where
  `DecisionContext::for_user(operator)` is allowed.
- The `AuditLedger` line (`host/src/daemon.rs:129`) and `JobRunRecord` gain the firing principal,
  recorded secret-free.

## Policies

- Default-deny fail-closed floor: an unmatched agent subject is denied; never inherit operator grants.
- Secret-free audit lines: the firing principal is recorded as identity only, never credential material.
- Design decisions need full writeup: the deny_reason must legibly name the agent subject.

## Quality Gate

1. A hermetic test proves the mission's literal sentence: a path the operator context reaches is DENIED to the agent context (default-deny) with a legible `deny_reason` naming the agent subject.
2. `ALLOW … ON <driver> AT <glob> FOR <agent>` grants narrow path-scoped reach (`ScopeGlob`/`PathScope` unchanged).
3. Audit ledger lines for agent-fired plans carry the agent identity, secret-free.
4. `evaluate` stays pure (no I/O added).
5. Existing anonymous/user/role tests are unmodified and green.
6. Verification: `cargo test -p qfs-server -p qfs-host` — pure enforcer tests with injected contexts, no network.

## Considerations

- Follow the blueprint chapter (20260718203330) subject ruling: a NEW `Subject::Agent` variant, not a reused user/role.
- Keep the enforcer split intact — the subject change must not introduce I/O into `evaluate_with_context`.
- The firing-principal field on `JobRunRecord` is consumed by the sweeper ticket (203334) for run-history read-back; keep the shape compatible.
