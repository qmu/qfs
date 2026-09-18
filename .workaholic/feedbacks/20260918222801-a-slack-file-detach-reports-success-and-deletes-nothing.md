---
type: Feedback
title: A Slack file detach reports success and deletes nothing
kind: concern
source: development
subject: observer_ai:qfs session
created_at: 2026-09-18T22:28:01+09:00
author: a@qmu.jp
supersedes: 
---

# A Slack file detach reports success and deletes nothing

# A Slack file detach reports success and deletes nothing

## Description

`remove /slack/<ws>/files/<id>` commits, returns `committed: true` behind the irreversible gate,
and the file is still there. Measured on 2026-09-18 against `/slack-cc-for-qmu` with qfs v0.0.137,
on a file this same account had uploaded minutes earlier:

- `remove /slack-cc-for-qmu/x/files/F0C2QR843PX --commit --commit-irreversible` → committed, affected `unknown`
- the workspace file listing still returns `F0C2QR843PX` three times over eight minutes, and the
  listing is fresh — it picked up a newer upload in the same window
- `/slack-cc-for-qmu/x/files/F0C2QR843PX/content` still returns the bytes
- a second identical remove reports `committed: true` again

So the wire call is being made and its answer is not being read. Slack reports an application
failure with HTTP 200 and `{ok: false, error: ...}`; the declared REMOVE map posts to
`files.delete` and the effect leg counts the exchange rather than the envelope. Whatever the
underlying refusal is — the likely candidates are the token lacking `files:write` for deletion, or
Slack refusing deletion of a file uploaded through the external-upload flow — the operator is told
the opposite of what happened, on an IRREVERSIBLE verb where being told the truth matters most.

## How to Fix

Make the declared write path read Slack's `ok` envelope for this map the way the scoped file
primitives already do (`applier/slack_file.rs`, `applier/slack_upload.rs` both map `ok: false`
onto their own refusal codes), so a refused detach fails the effect instead of reporting an
affected row. Then find the actual refusal for this case and name it.
