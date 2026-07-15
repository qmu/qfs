---
created_at: 2026-07-11T12:15:27+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash: 5e4b858
category: Added
depends_on:
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
---

# One-statement Gmail-attachment → specific Drive folder transfer, verified and taught

## Overview

Prove the mission's transfer capability in the Gmail→Drive direction: **one qfs statement** that
selects an attachment's bytes from `/mail` and upserts them into a *specific* Google Drive folder.
The mirror direction (Drive→Gmail attach-and-send) already shipped as the ARRAY_AGG(STRUCT)
cross-service pipe, and the machinery exists end-to-end: Gmail exposes attachment bytes on demand
(`attachments.get`), Drive resolves a folder path to `(folder_id, drive_id)` for upload, and
`materialize_pipeline_source` re-executes the read at `--commit` and embeds rows into the write
effect. This ticket wires the remaining seams (attachment-bytes projection into the Drive
upload row shape), covers it hermetically, and lands the taught recipe.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout (applies to all code work)
- `workaholic:implementation` / `policies/coding-standards.md` — style conventions (applies to all code work)
- `workaholic:implementation` / `policies/type-driven-design.md` — the attachment row shape and the Drive upload row shape must meet in the shared struct vocabulary, not via string glue
- `workaholic:design` / `policies/data-sovereignty.md` — bytes move directly source→destination within one statement; nothing is cached

## Key Files

- `packages/qfs/crates/driver-gmail/src/client.rs` - get_attachment (attachments.get, base64url decode), part tree with attachmentId/size
- `packages/qfs/crates/driver-gmail/src/path.rs` - MailPath::Attachment (/mail/<msg>/<att>), read-only
- `packages/qfs/crates/driver-gdrive/src/effect.rs` - folder_id resolution, parent-targeted create-only INSERT, MIME/name/bytes columns
- `packages/qfs/crates/exec/src/lib.rs` - materialize_pipeline_source (cross-driver read → write-effect rows, 10k-row cap)
- `docs/cookbook/` - gmail/gdrive cookbook articles (gen-skills sources)

## Related History

The reverse pipe shipped and is the pattern to mirror; folder targeting and attachment reads are in place.

- [20260701192440-cross-service-drive-to-gmail-attach-and-send.md](.workaholic/tickets/archive/work-20260629-110121/20260701192440-cross-service-drive-to-gmail-attach-and-send.md) - Drive→Gmail attach-and-send in one ARRAY_AGG(STRUCT) statement (the mirror)
- [20260707043312-drive-blob-upload-report-copy.md](.workaholic/tickets/archive/work-20260707-045409/20260707043312-drive-blob-upload-report-copy.md) - Drive parent-id/folder targeting
- [20260630203000-epic-replace-gmail-ftp-gdrive-ftp.md](.workaholic/tickets/archive/work-20260705-173620/20260630203000-epic-replace-gmail-ftp-gdrive-ftp.md) - the parity EPIC this continues

## Implementation Steps

1. Write the target statement first (spec by example): `INSERT INTO /drive/<account>/<folder-path> SELECT filename AS name, mime, bytes FROM /mail/<account>/<msg-id>/<att-id>` (exact form per current grammar) and check what the planner/materializer reject today.
2. Fix the gaps: attachment-bytes column projection into the Drive upload row shape (name/mime/bytes), MailPath::Attachment as a materializable single-row source, error message quality when the folder path names a file.
3. Hermetic end-to-end test: mock Gmail message with attachment + mock Drive; assert the committed multipart body carries the identical bytes and the parent folder id resolved from the path.
4. Cookbook recipe in the gmail (or cross-service) article: "save this attachment into that Drive folder", parse-checked by the ratchet; regenerate docs/skills.

## Quality Gate

**Acceptance criteria**

- The one-statement transfer commits hermetically with byte-identical content and correct parent folder resolution.
- A wrong destination (file path, nonexistent folder) fails with a structured error at PREVIEW, not at commit.

**Verification method**

- `cargo test --workspace` green including the new cross-service test; `gen-docs --check` / `gen-skills --check` clean.

**Gate**

- Hermetic suite green is the /drive approval gate; the live round (real Gmail attachment → real Drive folder, then read back) runs owner-attended and is recorded afterwards.

## Considerations

- The 10,000-row materialization cap is irrelevant here (single blob) but large attachments stress the row channel — note observed size limits (`packages/qfs/crates/exec/src/lib.rs`)
- Keep the projection vocabulary identical to the shipped mirror pipe so both directions read as one idiom (docs)
