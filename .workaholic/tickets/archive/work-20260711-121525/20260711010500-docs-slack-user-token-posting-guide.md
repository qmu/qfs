---
created_at: 2026-07-11T01:05:00+09:00
author: a@qmu.jp
type: housekeeping
layer: [UX]
effort: 2h
commit_hash:
category: Added
depends_on:
mission:
---

# Docs: guide for posting to Slack as yourself (user token) and the multi-user proxy pattern

## Overview

Extend the Slack cookbook (`docs/cookbook/slack.md`) with a guide verified in production against a
real Slack workspace on 2026-07-11: how to make qfs post **as a human user** (not as the bot), and
how to scale that to a whole team where each member's AI posts on their own behalf.

**No driver change is needed** — this is documentation only. `driver-slack` passes the stored
credential to `chat.postMessage` as a Bearer token verbatim and never validates the `xoxb-`/`xoxp-`
prefix (the prefixes appear only in test fixtures / the golden-test secret patterns). Slack's API
semantics do the rest: a user token (`xoxp-`) makes `chat.postMessage` appear as that person, no
bot badge.

## What the guide should cover

### 1. Posting as yourself (single user)

- On the Slack app config (api.slack.com/apps): add **User Token Scopes** `chat:write`
  (+ `channels:history` if the user also wants tail reads to work), then **Reinstall to
  Workspace** and authorize as the target user → copy the **User OAuth Token** (`xoxp-…`).
- Register it as a second qfs account and mount it on its own defined path, keeping the bot mount
  intact:

  ```sh
  printf %s "$XOXP_TOKEN" | qfs account add slack me
  qfs connect /slack-me --driver slack --account me
  ```

- Post: `insert into /slack-me/<ws>/<channel>/messages values (text) ('…')` — lands as the person.
- Caveat to document: the app settings page only ever shows the **installer's** user token; every
  other member must go through a real OAuth consent flow (authorize URL → code → `oauth.v2.access`).

### 2. Team-wide proxy pattern (everyone's AI posts as themselves)

- One workspace app, `user_scope=chat:write`; each member authorizes once and gets their own
  `xoxp-` token. A minimal localhost-redirect bootstrap script (same shape as a Gmail OAuth
  code-flow helper) makes this a "open URL, click approve" step per member.
- **Credential topology is the design decision — document both shapes and their trust boundaries:**
  1. *Per-member vault (recommended)*: each member runs `qfs account add slack …` on their own
     machine; only their own AI can speak as them.
  2. *Central box*: all tokens in one operator's vault. Must be called out as **impersonation
     power concentrated in one host** — treat like a production secret store, minimal scopes,
     explicit consent from every member, rotate all tokens if the host is compromised.
- Operational notes: agree on disclosure norms (whether AI-authored messages are marked), and
  revocation is per-user from Slack's app management page.

### 3. Cookbook accuracy fix (observed 2026-07-11, v0.0.10 binary)

The current cookbook example appends with a bare positional insert:

```qfs
insert into /slack/acme/general/messages values ('Deploy finished ✅')
```

This **previews** fine but **fails at `--commit`** with
`malformed INSERT effect …: a message needs 'text'` — the positional value does not bind to the
`text` column. The working form is the named-column shape:

```qfs
insert into /slack/acme/general/messages values (text) ('Deploy finished ✅')
```

Either fix the docs to use the named form throughout, or (better, separate ticket if taken) make
the decoder bind a single positional value to `text` so preview and commit agree. The guide should
use whichever form is decided as canonical.

## Quality Gate

- `docs/cookbook/slack.md` gains a "Post as yourself (user token)" section and a "Team proxy
  pattern" section covering the two credential topologies and their trust trade-offs.
- Every qfs statement in the new sections is verified against a live workspace (preview + commit).
- The positional-vs-named `values` discrepancy is resolved one way or the other (docs fixed at
  minimum; decoder fix split out if chosen).

## Considerations

- Verified end-to-end on 2026-07-11: bot-token mount at `/slack` (account `team`), named-column
  insert committed successfully to a channel; read-back failed only for missing history scope on
  the bot token — worth a note in the guide's scopes table.
- Slack ToS permits user tokens for automation acting on the user's behalf; the guide should still
  recommend the disclosure-norms conversation for AI-authored messages.
