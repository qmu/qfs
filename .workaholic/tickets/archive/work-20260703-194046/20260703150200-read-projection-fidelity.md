---
created_at: 2026-07-03T15:02:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort: 2h
commit_hash: bb402de
category: Changed
depends_on: []
---

# Read fidelity: user labels list as raw ids; draft attachments read back empty

Live parity check vs gmail-ftp/gdrive-ftp (owner-authorized, 2026-07-03, v0.0.17):

1. ~~Drive single-file read drops size/md5~~ — **FIXED on the drive-write-parity branch**
   (20260703150000): `content_batch` now carries name/mime_type/size/md5/content.
2. **User labels list as raw ids**: `/mail |> select name` returns `Label_5`, `Label_11` for
   user labels where gmail-ftp shows their display names (non-ASCII, `id != name`). Reading
   `/mail/<display-name>` works (verbatim label: search), so only the listing is wrong.
3. **Draft attachments read back empty**: a draft created with an `attachments` array commits
   fine, but both read paths (`/mail/drafts/<id>` and `/mail/draft/<id>`) return
   `attachments: []`. **Ground truth confirmed by the owner in the Gmail UI (2026-07-03):
   hello.txt IS attached** — the draft multipart build is correct; the attachments-column
   EXTRACTION on the read paths is what is broken.

## Fix

1. Thread size/md5 through the single-file (blob) read projection.
2. Map label ids to display names in the mailbox-root listing (the labels.list response carries
   both).
3. Determine ground truth for the draft attachment (Gmail UI / users.drafts.get raw), then fix
   whichever side lies — the draft multipart build or the attachments column extraction — and
   assert round-trip in a hermetic mock test.

## Key files

- `packages/qfs/crates/driver-gdrive/` (single-read projection), `crates/driver-gmail/`
  (labels listing, draft build, attachments extraction), `docs/cookbook/{gmail,gdrive}.md`

## Quality Gate

- The cookbook's 5-column download recipe returns all five columns live.
- `/mail |> select name` shows display names for user labels.
- A draft created with an attachment lists that attachment on read-back (mock-tested; live
  confirmed once by the owner).
