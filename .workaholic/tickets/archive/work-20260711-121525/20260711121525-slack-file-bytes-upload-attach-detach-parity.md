---
created_at: 2026-07-11T12:15:25+09:00
author: a@qmu.jp
type: enhancement
layer: [Infrastructure, Domain]
effort:
commit_hash: e4b9ab1
category: Added
depends_on:
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
---

# Slack file bytes upload + attach/detach parity across Gmail, Slack, and Drive

## Overview

Close the file-handling parity gaps so "attach/detach a file" works uniformly over the three
compiled messaging/storage services. Gmail already ships attach/detach on every draft/send/reply
form (41e0ce4) and thread reply (197c851); Drive ships blob upload/download with folder
targeting; Slack ships the `/slack/<ws>/files` blob namespace with download and delete — but
Slack **upload is a text bridge only** (`files.upload` via codecs; the bytes path was parked as
E5/t15). This ticket lands real bytes upload for Slack files and then verifies the attach/detach
loop (upload bytes → list → download bytes → delete) is uniform across `/mail`, `/slack`, and
`/drive`, recording any remaining asymmetry as follow-up tickets rather than silently absorbing it.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout (applies to all code work)
- `workaholic:implementation` / `policies/coding-standards.md` — style conventions (applies to all code work)
- `workaholic:implementation` / `policies/domain-layer-separation.md` — the Slack upload stays a thin driver-effect translation; no Slack SDK/HTTP types leak past `driver-slack`
- `workaholic:design` / `policies/data-sovereignty.md` — file bytes are user-owned data in transit; no caching beyond the statement's execution
- `workaholic:implementation` / `policies/test.md` — hermetic mock coverage for the multipart upload; the live round is owner-gated

## Key Files

- `packages/qfs/crates/driver-slack/src/effect.rs` - UploadFile (files.upload, multipart; content is text-bridge today — the bytes path to build) and DeleteFile
- `packages/qfs/crates/driver-slack/src/path.rs` - `/slack/<ws>/files` blob namespace (ls/cp/rm), per-file downloadable blob, dms/<user>/files listing
- `packages/qfs/crates/driver-gmail/src/effect.rs` - the shipped Gmail attachments decoder (Array(Struct{filename,mime,bytes})) — the parity reference shape
- `packages/qfs/crates/driver-gdrive/src/effect.rs` - Drive blob upload with MIME/name/bytes columns — the other parity reference
- `packages/qfs/crates/exec/src/lib.rs` - materialize_pipeline_source, the cross-service row channel uploads ride

## Related History

The gmail-ftp/gdrive-ftp parity EPIC drove file handling to Gmail/Drive parity; Slack file listing/download landed later, leaving upload as the last text-only edge.

- [20260709010931-gmail-attach-detach-every-draft-send-form.md](.workaholic/tickets/archive/work-20260708-171710/20260709010931-gmail-attach-detach-every-draft-send-form.md) - Gmail attach/detach on every send form (the parity bar this ticket matches)
- [20260708000000-slack-file-share-dm-linkage.md](.workaholic/tickets/archive/work-20260708-171710/20260708000000-slack-file-share-dm-linkage.md) - Slack file namespace listing/download (the read half this ticket completes)
- [20260707043312-drive-blob-upload-report-copy.md](.workaholic/tickets/archive/work-20260707-045409/20260707043312-drive-blob-upload-report-copy.md) - Drive blob upload + folder targeting

## Implementation Steps

1. Replace the text-bridge UploadFile body with a real bytes multipart upload (Slack `files.getUploadURLExternal`/`files.completeUploadExternal` flow if the legacy `files.upload` is sunset — check the current API), keeping the effect row shape `{filename, mime, bytes, channel}` aligned with Gmail's Attachment struct vocabulary.
2. Wire detach: confirm `REMOVE /slack/<ws>/files/<id>` (DeleteFile) remains the irreversible-gated delete; add the missing hermetic test if uncovered.
3. Add a parity test matrix (hermetic, mock HTTP): upload bytes → list shows the file → download returns identical bytes → delete removes it, for `/slack`; assert the same loop already passes for `/drive` and the Gmail attachment read/send forms.
4. Update `docs/cookbook/slack.md` with the file upload/detach recipes (coordinate with the queued docs ticket 20260711010500 — different sections, avoid clobbering) and regenerate skills (`cargo run -p xtask -- gen-skills`).
5. Record any remaining asymmetry (e.g. DM file upload, size caps) as Considerations, not scope creep.

## Quality Gate

**Acceptance criteria**

- A single `UPSERT INTO /slack/<ws>/files` statement with a bytes column commits a real multipart upload (hermetic mock asserts multipart body and filename/MIME propagation).
- The upload→list→download→delete loop passes hermetically for Slack with byte-identical round-trip.
- Parity: the same statement vocabulary (bytes/filename/mime columns) is accepted for `/mail` attachments, `/slack/files`, and `/drive` uploads.

**Verification method**

- `cargo test --workspace` green including the new driver-slack effect tests; `cargo run -p xtask -- gen-docs --check` and `gen-skills --check` clean.

**Gate**

- Hermetic suite green is the /drive approval gate. The live Slack upload/detach round runs in a separate owner-attended session and is recorded on this ticket afterwards (per the hermetic-first policy answer, 2026-07-11).

## Considerations

- Slack has sunset legacy `files.upload` for new apps; the external-upload two-step flow changes the effect's HTTP shape (`packages/qfs/crates/driver-slack/src/effect.rs`)
- Keep the bytes column vocabulary identical to Gmail's `Attachment` struct so cross-service SELECT→UPSERT composes without projection gymnastics (`packages/qfs/crates/driver-gmail/src/schema.rs`)
- The queued docs ticket 20260711010500 also edits `docs/cookbook/slack.md`; land sections independently

## Live Round Evidence

### Round 3 — Slack user-token post over /slack-me (2026-07-13, owner-attended, PASSED)

- **Binary:** qfs 0.0.59 (c30fa0a). New mount: `/slack-me` (driver slack) bound to account
  `slack me` — a User OAuth Token (`xoxp-`) minted with user scopes `chat:write`, `im:write`,
  `im:history`, `users:read` (+ files/channels for later rounds).
- **Statement:** `insert into /slack-me/qmu/dms/U03S55GC3/messages values (text) ('…')` — a
  self-DM, previewed (`affected 1`, reversible) then committed.
- **The proof:** read-back of the DM listed the message text verbatim with
  **`user: U03S55GC3` (tamura_yoshiya)** — the post rides the owner's own identity, not a bot.
- **First attempt caught by read-back (identity assert):** the initially-pasted "user" token was
  actually the app's bot token — the post committed fine but read back as `U0BFLKVB66N`
  (`qfs_integration`, is_bot true). `affected 1` alone would have hidden this; **read-back must
  assert the writer's identity, not just the payload**. Owner minted a real `xoxp-` token
  (Slack app → OAuth & Permissions → User Token Scopes → Reinstall; an accidental `admin` scope
  first blocked the reinstall as Enterprise-only and was removed) and re-posted: PASSED.
- **Defect found along the way (ticketed 20260713101500):** `/slack-me/qmu/users` silently drops
  the WHERE stage (text and bool filters return all 31 rows while select projection applies) —
  the cookbook-taught `users |> where name == '…'` shape returns wrong rows.
- **Scope note:** this round proves the user-token *message post*. The Slack file-bytes upload
  over the user mount (the checkbox-60 "Slack bytes" parity gap this ticket shipped hermetically)
  still wants its live user-token run — the `files:write` scope is already consented for it.
- **Residue:** two test messages in the owner's self-DM (one bot-identity, one user-identity).
