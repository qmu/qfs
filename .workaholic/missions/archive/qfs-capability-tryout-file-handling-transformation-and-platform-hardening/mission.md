---
type: Mission
title: qfs capability tryout: file handling, transformation, and platform hardening
slug: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
status: achieved
created_at: 2026-07-11T04:41:19+09:00
author: a@qmu.jp
tickets: []
stories: []
concerns: []
---

# qfs capability tryout: file handling, transformation, and platform hardening

## Goal

Prove — with the owner's real accounts and real API keys, not hermetic mocks — that qfs delivers
its promise as the one pipe-SQL surface for cross-service work, in the three areas that matter
next on the roadmap (owner directive, 2026-07-11):

1. **File handling as a first-class capability.** Attachments must move freely across the
   services people actually use — Email (Gmail), Slack, Chatwork, and Google Drive — in both
   directions: attach, detach, transfer, and reply-with-attachment. Transform (`|> transform`,
   shipped in v0.0.42 behind the one model-call seam) must compose with real provider keys into
   practical chains: text generation on every major provider, PDF→text→Drive pipelines, chained
   transforms, and AI-routed tool choice via a switch predicate in the pipe.
2. **Less platform, more language.** Reduce the binary's dependency footprint (assess how far
   past attempts got and how much further is realistic), and push drivers out of compiled Rust
   into qfs-query declarations for both OAuth-style and API-key-style APIs — the DECLARED driver
   model becoming the normal way to add a service.
3. **A server that schedules, safely.** Server scheduling semantics are back in scope (owner
   changed their mind: we need this), and the whole surface must be assured free of command
   execution risk — no path from query text or fetched data to arbitrary process execution.

Live rounds touch real Gmail/Drive/Slack/Chatwork data and real paid API keys, so every
capability lands hermetic-first and its live tryout is an explicit owner-attended step
(per the qfs-env-has-live-cloud-accounts ground rule).

## Scope

**Done when** every acceptance item below is ticked: the file-handling and transformation
tryouts each verified live at least once with the real account/key, the dependency and
driver-rewrite investigations produce recorded findings (adopt-with-plan or defer-with-reasoning),
server scheduling semantics are designed and implemented, and a command-execution-risk assurance
is recorded as tests or a governance lock.

**Out of scope:**

- New service integrations beyond the four named (Email/Gmail, Slack, Chatwork, Google Drive) —
  Chatwork itself may need a driver first, which is then in scope only to the extent file
  handling needs it.
- General performance work, UI/console work, and the CONNECT/path_binding registry epic except
  where the driver-rewrite investigation depends on it.
- Autonomous live probing: live rounds run only in owner-attended sessions.

## Acceptance

### Capability tryout: file handling (all with the real account)

- [x] Attach/detach files over Email (Gmail), Slack message, and Google Drive — Slack bytes
      upload closes the last parity gap
      (#20260711121525-slack-file-bytes-upload-attach-detach-parity.md) — live-proven 2026-07-13
      (owner-attended, v0.0.62): a 56-byte file uploaded channel-less to `/slack-me/qmu/files`,
      listed newest-first, downloaded byte-exact (sha256 match), then detached with `remove
      /slack-me/qmu/files/<id>`. The detach exposed and this branch fixed a capability/verb mismatch
      (ticket 20260713234132)
- [x] Attach/detach files over Chatwork via the new declared driver
      (#20260711121526-chatwork-declared-driver-with-file-handling.md) — live-proven 2026-07-13
      (round 10: ENCODE multipart upload + FOLLOW blob download, byte-exact round-trip; the
      Chatwork API has no file delete, so detach is N/A by service design)
- [x] Transfer an attached file from Gmail to a specific Google Drive directory in one qfs
      statement (#20260711121527-gmail-attachment-to-drive-folder-transfer.md) — live-proven
      2026-07-13 (owner-attended, v0.0.62): `/mail/inbox/<msg>/att0 |> select filename as name, mime
      as mime_type, content as bytes |> insert into /drive/my/qfs-extract-test` — the 1579-byte
      `invite.ics` landed byte-exact in Drive (sha256 match). The round exposed that Gmail's
      `attachmentId` is ephemeral; this branch made the attachment addressable by the stable index
      `att<N>`, resolving the fresh id inside the read (ticket 20260713234133)
- [x] Reply with an attached file sourced from Google Drive / Slack / Chatwork
      (#20260711121528-reply-with-attachment-cross-service.md) — live-proven 2026-07-13 (Drive
      source, round 4; Slack/Chatwork file-reply remain recorded follow-ups by design)

### Transformation (all with real API keys)

- [x] Simple text generation verified against every major provider (Anthropic, OpenAI, Google)
      (#20260711121529-live-model-providers-anthropic-openai-google.md) — live-proven 2026-07-13
      (round 7: Anthropic in T8, OpenAI gpt-4o-mini, Google gemini-flash-latest; reasoning-model
      param/budget defects ticketed 20260713140000)
- [x] A query that does PDF → text → save to Google Drive
      (#20260711121530-pdf-extraction-to-drive-pipeline.md) — live-proven 2026-07-13 (round 5,
      one statement, real PDF through Anthropic into Drive; ran via the struct bypass — the
      taught recipe shape is blocked by ticket 20260713120000)
- [x] Transformation chain: multiple transform stages composed in one pipe
      (#20260711121531-transform-chain-composition.md) — live-proven 2026-07-13 (round 6, two
      real Anthropic stages chained under one consent; gmail-source commit gap ticketed
      20260713123000)
- [x] Switch predicate in the pipe letting the model choose which tool/branch to run
      (#20260711121532-switch-predicate-model-routing.md) — live-proven 2026-07-12 (round 2, T8
      switch re-run, owner-attended): inbox top 3 → `transform triage` (anthropic/haiku) → `switch
      route` sent the two alert-looking subjects to the Drive-file arm (INSERT drive affected 2) and
      one to the self-draft arm (INSERT mail drafts affected 1) — the model chose the branch.
      Verified against the round-2 evidence on
      archive/work-20260712-015928/20260712005000-drive-multi-row-insert-silent-loss.md.

### Platform hardening and direction

- [x] Dependency reduction: recorded assessment of how far past attempts went and how much
      further is achievable, with an adopt-with-plan or defer-with-reasoning ruling
      (#20260711121533-dependency-reduction-execution.md) — ruled 2026-07-14 (v0.0.62): blueprint
      §11 dated re-measurement — 50 members / 30 shipped direct deps (both flat vs v0.0.54), zero
      new crates from the mission window, tree re-baselined 356/363 (reproducible cargo-tree
      method); per-lever ruling **removable-today ≈ 0** (heavy roots already feature-trimmed,
      tracing-subscriber defer-with-reasoning, async-trait monitored exit, dup versions
      upstream-owned)
- [x] Drivers rewritten as qfs query declarations: an OAuth-style API proven end-to-end
      (#20260711121534-oauth-style-declared-driver-rewrite.md) — live-proven 2026-07-13 (round 8:
      /ghdecl read with AUTH ACCOUNT bearer injection, private repos returned, User-Agent accepted)
- [x] Drivers rewritten as qfs query declarations: an API-key-style API proven end-to-end
      (Chatwork is the proof vehicle)
      (#20260711121526-chatwork-declared-driver-with-file-handling.md) — live-proven 2026-07-12/13
      (round 1 read half + round 10 file upload/download)
- [x] Server scheduling semantics designed and implemented (owner reversal of t65: needed)
      (#20260711121535-server-scheduling-semantics-revisit.md) — live-proven 2026-07-13
      (round 9 re-run on v0.0.61: `qfs serve` sweeper fired a 1m `/local` upsert JOB within <1s,
      `outcome=fired affected=1`, tick file written, durable `last_run` survived a restart with no
      spurious re-fire and the restarted daemon resumed the schedule; evidence on the archived
      sweeper ticket 20260713130000)
- [x] Command-execution-risk assurance recorded: tests or a governance lock showing no path
      from query text or fetched data to process execution
      (#20260711121536-command-execution-risk-assurance.md) — verified 2026-07-14 (already landed
      in commit 6b4b29f): `exec_inventory.rs` enforces an exact `(file, program)` spawn allowlist +
      a no-shell lock, `driver-git` argument-hygiene tests neutralize git option-injection
      (`qualify_ref`/`Oid::parse`), and blueprint §17 records the inventory, defenses, data-path
      argument, and same-PR standing rule. Gates green (exec_inventory 2/2, driver-git 39/39)

## Changelog

<!-- Append-only, dated timeline relating this mission's tickets and reports over time.
     One line per event ("- YYYY-MM-DD — event — filename"); never rewrite past lines. -->
- 2026-07-11 — ticket archived — 20260711121529-live-model-providers-anthropic-openai-google.md
- 2026-07-11 — ticket archived — 20260711121527-gmail-attachment-to-drive-folder-transfer.md
- 2026-07-11 — ticket archived — 20260711121525-slack-file-bytes-upload-attach-detach-parity.md
- 2026-07-11 — ticket archived — 20260711121526-chatwork-declared-driver-with-file-handling.md
- 2026-07-12 — concern deferred (stuck) — 33-resolved-moved-to-archive.md
- 2026-07-12 — concern deferred (stuck) — 33-partially-addressed-carry-updated.md
- 2026-07-12 — concern deferred (stuck) — 33-new-two-live-round-driver-defects.md
- 2026-07-12 — concern deferred (stuck) — 33-new-remaining-owner-attended-live-rounds.md
- 2026-07-12 — concern deferred (stuck) — 33-new-declared-model-and-scheduling-follow.md
- 2026-07-12 — concern deferred (stuck) — 33-new-scope-cuts-and-monitored-items.md
- 2026-07-12 — concern deferred (stuck) — 33-carried-unchanged-from-prior-prs-not.md
- 2026-07-12 — concern deferred (stuck) — 34-resolved.md
- 2026-07-12 — concern deferred (stuck) — 34-new-duplicate-declaration-rows-still-resolve.md
- 2026-07-12 — concern deferred (stuck) — 34-carried-unchanged-from-prior-prs-not.md
- 2026-07-12 — story reported — work-20260712-032443.md
- 2026-07-12 — concern deferred (stuck) — 35-new-follow-redirect-refused-by-confined-transport.md
- 2026-07-12 — concern deferred (stuck) — 35-new-policyless-denied-job-refires-every-sweep.md
- 2026-07-12 — concern partially resolved (sweeper + FOLLOW/ENCODE landed, PR #35) — 33-new-declared-model-and-scheduling-follow.md
- 2026-07-12 — PR #35 merged, v0.0.59 tagged and released — work-20260712-032443.md
- 2026-07-12 — live round 1 passed (chatwork declared read half, 91 rooms with values on v0.0.59) — 20260711121526-chatwork-declared-driver-with-file-handling.md
- 2026-07-12 — live round 2 passed (T8 switch re-run: affected 2 = 2 Drive files; fail-stop + INSERT-never-replaces proven live) — 20260712005000-drive-multi-row-insert-silent-loss.md
- 2026-07-12 — defect found live, ticketed (Drive folder UPSERT lacks INSERT's per-row decode) — 20260712150000-drive-folder-upsert-per-row-parity.md
- 2026-07-13 — live round 3 partial (post/read-back work but token was the bot; user-token re-add pending) — 20260712050000-resume-owner-attended-live-rounds.md
- 2026-07-13 — defect found live, ticketed (Slack users WHERE silently dropped) — 20260713101500-slack-users-where-silently-dropped.md
- 2026-07-13 — live round 3 passed (user-token self-DM post reads back as U03S55GC3, the owner's identity; file-bytes live remainder noted) — 20260711121525-slack-file-bytes-upload-attach-detach-parity.md
- 2026-07-13 — live round 4 passed, acceptance ticked (reply-with-attachment: Drive PDF into a real self thread, one statement) — 20260711121528-reply-with-attachment-cross-service.md
- 2026-07-13 — live round 5 passed, acceptance ticked (real PDF through Anthropic into Drive, model-named output proves document receipt; struct bypass required) — 20260711121530-pdf-extraction-to-drive-pipeline.md
- 2026-07-13 — three defects found live, ticketed (blob plan schema omits content; Drive folder UPDATE drops WHERE and mutates the folder; id: addressing broken / space names unaddressable) — 20260713120000, 20260713120100, 20260713120200
- 2026-07-13 — live round 6 passed, acceptance ticked (two-stage chain, negative handshake probe included) — 20260711121531-transform-chain-composition.md
- 2026-07-13 — defect found live, ticketed (pure-read transform commit cannot service the Gmail READ) — 20260713123000-gmail-read-not-serviced-pure-transform-commit.md
- 2026-07-13 — live round 8 passed, acceptance ticked (declared /ghdecl read, AUTH ACCOUNT bearer, private repos live) — 20260711121534-oauth-style-declared-driver-rewrite.md
- 2026-07-13 — live round 9 FAILED, blocking defect ticketed (sweeper stores AST-JSON, fire_due lexes it as text — no job has ever fired; runs ledger + api read-back proven working) — 20260713130000-sweeper-job-body-format-mismatch.md
- 2026-07-13 — live round 10 passed, two acceptances ticked (Chatwork multipart upload + FOLLOW download byte-exact; API-key-style declared driver end-to-end) — 20260711121526-chatwork-declared-driver-with-file-handling.md
- 2026-07-13 — live round 7 passed, acceptance ticked (all three providers live; reasoning-model param/budget defects found) — 20260711121529-live-model-providers-anthropic-openai-google.md
- 2026-07-13 — defect found live, ticketed (transform provider layer breaks on reasoning models: OpenAI max_tokens 400, Gemini thinking-budget empty output) — 20260713140000-transform-provider-params-reasoning-models.md
- 2026-07-13 — concern resolved (live /gdrive upload + read-back proven, rounds 2/4/5) — 25-live-google-drive-upload-was-not.md
- 2026-07-13 — story reported, PR #36 opened (knowledge-only, binary stays v0.0.59) — work-20260712-114152.md
- 2026-07-13 — 8 live-round defects fixed + archived (sweeper firing, Drive UPDATE/UPSERT/id:, Slack/GitHub WHERE, /local content, Gmail read-terminal transform, reasoning-model params), v0.0.60, each hermetically locked — work-20260713-150833
- 2026-07-13 — story reported, PR #37 opened (v0.0.60; server-scheduling live round 9 re-run still owner-attended) — work-20260713-150833.md
- 2026-07-13 — ticket archived — 20260712050000-resume-owner-attended-live-rounds.md
- 2026-07-13 — ticket archived — 20260713150000-carry-live-rounds-done-ship-branch-and-backlog.md
- 2026-07-13 — ticket archived — 20260713190432-driver-fs-content-column-parity.md
- 2026-07-13 — ticket archived — 20260713191608-gdrive-content-schema-unification.md
- 2026-07-13 — live round 9 PASSED on re-run, server-scheduling acceptance ticked (v0.0.61 `qfs serve` sweeper fired a 1m /local upsert JOB in <1s, outcome=fired affected=1, tick file written, durable last_run survived restart and the schedule resumed) — 20260713130000-sweeper-job-body-format-mismatch.md
- 2026-07-13 — switch-predicate acceptance verify-and-ticked from round-2 evidence (model `transform triage` → `switch route` chose the Drive-file vs self-draft arm live, owner-attended 2026-07-12) — 20260711121532-switch-predicate-model-routing.md
- 2026-07-13 — concern resolved (unstuck) — 37-new-driver-fs-content-omission.md
- 2026-07-13 — concern resolved (unstuck) — 37-new-drive-select-content-schema-divergence.md
- 2026-07-13 — concern resolved (unstuck) — 37-new-server-scheduling-live-acceptance.md
- 2026-07-13 — story reported — work-20260713-185925.md
- 2026-07-13 — concern deferred (stuck) — 38-updated-carried-from-pr-37-per.md
- 2026-07-13 — concern deferred (stuck) — 38-carried-unchanged-the-standing-open-concerns.md
- 2026-07-13 — live round PASSED, Slack attach/detach acceptance ticked (byte-exact upload/download + detach; capability/verb mismatch fixed) — 20260713234132-slack-file-detach-verb-mismatch.md
- 2026-07-13 — live round PASSED, Gmail→Drive transfer acceptance ticked (one statement, byte-exact; ephemeral attachmentId → stable att<N> index addressing) — 20260713234133-gmail-attachment-id-not-exposed.md
- 2026-07-14 — ticket archived — 20260713234132-slack-file-detach-verb-mismatch.md
- 2026-07-14 — ticket archived — 20260713234133-gmail-attachment-id-not-exposed.md
- 2026-07-14 — story reported — work-20260713-233938.md
- 2026-07-14 — concern deferred (stuck) — slack-workspace-namespace-still-advertises-verb.md
- 2026-07-14 — concern deferred (stuck) — carried-unchanged-the-standing-open-concerns.md
- 2026-07-14 — acceptance ticked, dependency-reduction ruling recorded (removable-today ≈ 0; blueprint §11 v0.0.62 re-measure) — 20260711121533-dependency-reduction-execution.md
- 2026-07-14 — acceptance ticked, command-execution-risk assurance verified already landed (exec_inventory lock + git hygiene + blueprint §17; commit 6b4b29f) — 20260711121536-command-execution-risk-assurance.md
- 2026-07-14 — mission achieved — mission.md
- 2026-07-15 — ticket archived — 20260714120000-effect-selector-uniform-migration.md
- 2026-07-15 — concern resolved (unstuck) — per-row-drive-folder-rename-needs.md
