---
created_at: 2026-07-03T15:01:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort: 2h
commit_hash: c61c4a1
category: Changed
depends_on: []
---

# Gmail drafts: positional INSERT and set-wide REMOVE fail at commit

Live parity check vs gmail-ftp (owner-authorized, 2026-07-03, v0.0.17). Two documented recipes
preview fine and fail only at commit — independently reproduced by a skills-only agent run:

1. **Positional draft INSERT** — the cookbook's headline recipe
   `insert into /mail/drafts values ('a@b', 'subj', 'body')` commits with
   `malformed INSERT effect: draft has no 'to' recipients`. The named-columns form
   `values (to, subject, body) (...)` works. The positional column mapping for the drafts
   collection never lands on `to`.
2. **Set-wide REMOVE on drafts** — the cookbook's "Trash by subject/sender" pattern
   `remove /mail/drafts where ...` commits with
   `CapabilityDenied { driver: mail, verb: REMOVE at "/mail/drafts" }`, while
   `qfs describe /mail/drafts` reports `remove: true` — breaking the documented
   "describe never lies about verbs" contract. Single-path `remove /mail/drafts/<id>` works.

## Fix

Map positional drafts VALUES onto (to, subject, body[, attachments]) — or reject positional at
PARSE/RESOLVE time with a clear error naming the named-columns form; make set-wide REMOVE on
drafts work (enumerate matching ids, trash each) or have describe/preview stop advertising it.
Either way, preview and apply must agree, and the cookbook must show only forms that commit.

## Key files

- `packages/qfs/crates/driver-gmail/` (draft INSERT effect build, REMOVE applier),
  `crates/qfs/src/commit.rs`, `docs/cookbook/gmail.md`

## Quality Gate

- The cookbook's draft-create and trash recipes commit successfully live (hermetic: effect-build
  unit tests for the positional mapping and set-wide REMOVE lowering).
- `describe` verb claims match what the applier accepts for every /mail node.
