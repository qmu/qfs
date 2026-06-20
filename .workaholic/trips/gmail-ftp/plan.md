---
instruction: "to create gmail version of gdrive-ftp (../gdrive-ftp), so same concept but handle not files but email, same dir structure, and experience"
phase: coding
step: concurrent-launch
iteration: 1
updated_at: 2026-06-20T20:15:30+09:00
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

## Progress

- [x] planning/artifact-generation — Planner authored `directions/direction-v1.md`; Architect `models/model-v1.md`; Constructor `designs/design-v1.md`.
- [x] planning/one-turn-review — three round-1 reviews; Planner requested revision of Design, Constructor requested revision of Model; navigation divergence surfaced.
- [x] planning/respond-to-feedback — Constructor accepted → `design-v2.md` (2-level); Architect accepted → `model-v2.md` (2-level + N+1 cost model). No escalation.
- [x] planning/moderation — Lead Amendment 1 fixes navigation, rm/mkdir semantics, scope, and OAuth. Consensus gate passed; plan fixed.
- [ ] coding/concurrent-launch — Constructor implements + internal tests; Planner builds dev env + E2E scenarios; Architect discovers codebase.
