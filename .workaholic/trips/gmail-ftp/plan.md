---
instruction: "to create gmail version of gdrive-ftp (../gdrive-ftp), so same concept but handle not files but email, same dir structure, and experience"
phase: complete
step: done
iteration: 1
updated_at: 2026-06-20T20:47:00+09:00
---

# Trip Plan

## Initial Idea

to create gmail version of gdrive-ftp (../gdrive-ftp), so same concept but handle not files but email, same dir structure, and experience

## Plan Amendments

### Amendment 1 (Lead, 2026-06-20) — Consensus reached, plan fixed

- **Navigation model (canonical):** 2-level — `root → label → message`, attachments as leaves inside a message. Thread is opt-in via a `threadId` field and `id:thread:<id>` addressing plus `.mbox` export. `Ref.Kind ∈ {label, message}`. (Architect + Constructor converged; Constructor abandoned his own 3-level design after verifying Gmail `list` endpoints return IDs only — a third tier is a second N+1 round-trip for no quota gain.)
- **`rm` semantics:** `rm <message>` trashes a single message (reversible), never a whole thread implicitly; thread trash requires explicit `id:thread:<id>`.
- **`mkdir`:** `mkdir <name>` creates a Gmail user label.
- **OAuth scope:** least-privilege union `gmail.modify` + `gmail.compose`; never full `mail.google.com` or hard-delete.
- **v1 scope decision (Lead tie-break):** v1 ships **navigation + retrieval + safe staging** — `auth`, navigate (label→message), `ls/cd/pwd/find`, `get` (message `.eml` / attachment / `.txt`), `rm` (trash), `mkdir` (create label), `put` (create **draft** only), local `lcd/lls/lpwd`, and audit logging. The **irreversible `send` verb and `label`/`unlabel` membership verbs are deferred to v1.1.** Rationale: 2 of 3 artifacts (Planner direction + Architect model) defer `send`; drafts are reversible, send is not; this matches the "no accidental irreversible actions in v1" safety bar. Constructor's design-v2 may define these verbs but must mark them deferred/stubbed, not wired into v1 dispatch.

### Amendment 2 (Lead, 2026-06-20) — Coding-review resolutions (Iteration 1)

- **Discoverable deferred stubs (resolves Architect concern E2):** `send`/`label`/`unlabel` MAY remain registered in the live dispatch table and `help` as inert stubs that return a "deferred to v1.1" notice, carry no send scope, and perform no mutation. This refines Amendment 1's "not wired into dispatch": the intent was no *functional/send path*, which holds. Keeping them discoverable better serves the "same experience" roadmap promise. No code change required.
- **S5b JSON envelope bug (resolves Planner E2E PARTIAL):** Constructor to route pre-auth `fatal` failures (`main.go`) through the JSON error envelope when `-json` is set, so unauthenticated/credentials failures emit `{"error":…}` instead of plain text. This is a JSON-contract correctness fix and is in scope for Iteration 1.
- **Documented-as-intended (no change):** eager-auth gating `help` behind credentials mirrors gdrive-ftp parity; empty-audit-prints-nothing in piped text mode is a cosmetic backlog item, not an Iteration-1 blocker.

## Progress

- [x] planning/artifact-generation — Planner authored `directions/direction-v1.md`; Architect `models/model-v1.md`; Constructor `designs/design-v1.md`.
- [x] planning/one-turn-review — three round-1 reviews; Planner requested revision of Design, Constructor requested revision of Model; navigation divergence surfaced.
- [x] planning/respond-to-feedback — Constructor accepted → `design-v2.md` (2-level); Architect accepted → `model-v2.md` (2-level + N+1 cost model). No escalation.
- [x] planning/moderation — Lead Amendment 1 fixes navigation, rm/mkdir semantics, scope, and OAuth. Consensus gate passed; plan fixed.
- [x] coding/concurrent-launch — Constructor implemented gmail-ftp (65 tests green); Planner authored E2E plan + validated toolchain; Architect authored review checklist.
- [x] coding/review-and-testing — Architect approved w/ minor suggestions (all structural risks cleared); Planner E2E 9 PASS / 1 PARTIAL (S5b).
- [x] coding/iteration-1 — Constructor fixed S5b (JSON envelope via single `exitErr` helper); Architect re-reviewed (no regressions); Planner re-tested (S5b PARTIAL→PASS). Consensus: Approve.
- [x] complete/done — gmail-ftp v1 delivered: builds clean, `go vet`/`gofmt` clean, 66 tests passing; E2E smoke 10/10. Backlog (non-blocking): batched metadata fetch + intra-command cache; one-shot `friendlyErr` normalization in `-json`; empty-audit cosmetic in piped text mode.
