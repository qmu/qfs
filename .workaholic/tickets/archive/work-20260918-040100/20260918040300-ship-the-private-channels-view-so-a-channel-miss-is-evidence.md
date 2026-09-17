---
created_at: 2026-09-18T04:03:00+09:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
feedback: [20260917212108-fb-the-loop-s-slack-destination-does-not-exist-so-nothing-reaches-a-person.md]
merge_policy:
verification_handoff: 
---

# Ship the `private-channels` view so a channel miss is evidence

## Overview

`qfs` can list a Slack workspace's **public** channels and nothing else. The shipped declaration
has one discovery view:

```
CREATE VIEW /slack/{ws}/channels OF slack/channel AS
  /http/slack/conversations.list |> DECODE json |> EXPAND channels;
```

Slack's `conversations.list` defaults to `types=public_channel`, so this view is structurally
blind to private channels. The consequence is not a missing feature but a **wrong answer**: asked
whether a channel exists, qfs returns an empty result that reads as "no such channel" when the
honest answer is "not in the half I can see". `docs/cookbook/slack.md` already carries a paragraph
warning agents not to trust the miss, and points at a `private-channels` view it describes as
"absent from the shipped driver asset" — the documentation has been compensating for the gap
instead of the driver closing it.

Measured on 2026-09-18: five connected Slack mounts, `channels` read on every one, `dev-qfs` absent
from all five. That is a settled zero only for public channels; no surface qfs ships could have
answered it for a private one.

## Policies

- `workaholic:design` — information must be reachable through the paths agents actually use; a
  view that silently covers half its namespace is a correctness problem, not a coverage gap
- `workaholic:implementation` / `policies/coding-standards.md` — style and structure conventions

## Key Files

- `packages/qfs/crates/skill/assets/examples/slack_driver.qfs` — the shipped declaration. Views
  are lifted from these bytes at test time (`shipped_views`), so adding one needs no fixture edit.
- `docs/cookbook/slack.md` — the discovery section that documents the gap, and the scope table
  that already names `groups:read`.
- `plugins/qfs/skills/qfs-slack/SKILL.md` — generated from the cookbook article; regenerate, never
  hand-edit.

## Implementation Steps

1. Declare `/slack/{ws}/private-channels OF slack/channel` against
   `conversations.list?types=private_channel` as a **sibling** view, not a `types=` widening of
   `/channels` (see Considerations).
2. Rewrite the cookbook's discovery section: two views, why they are two, and what a
   `missing_scope` on the private one means versus an empty result.
3. `cargo run -p xtask -- gen-skills` to propagate to `qfs-slack/SKILL.md`.
4. Bump all four plugin `version` fields (minor — the taught discovery surface changed) and the
   patch in `packages/qfs/crates/qfs/Cargo.toml`.

## Quality Gate

**Acceptance criteria**

- `/slack/<ws>/private-channels` is declared by the shipped asset, typed `slack/channel`, and
  parses on the shipped grammar.
- `/slack/<ws>/channels` is byte-unchanged, so no existing install loses its public listing.
- The cookbook no longer describes `private-channels` as absent, and the generated skill matches.

**Verification method**

- `cd packages/qfs && cargo test --workspace` (the cookbook/skills ratchet parses every recipe)
- `cd packages/qfs && cargo run -p xtask -- gen-skills --check`
- `python3 scripts/check-plugin.py`
- Live: install the declaration and read `private-channels` on a mount whose token holds
  `groups:read`.

## Considerations

- **Why a sibling view and not `types=public_channel,private_channel`.** A token without
  `groups:read` fails the whole call with `missing_scope`. Widening the existing view would trade
  a working public listing for a hard error on every channel-scoped token — a regression for the
  common case to serve the rarer one. Two views degrade independently.
- **This does not make every private channel visible.** `conversations.list` returns only
  conversations the token's app is actually in. The view removes the *structural* blindness; app
  membership remains the operator's act, and the cookbook says so.
- **Existing installs.** The declaration is installed by running the script, so a mount installed
  before this change keeps the old view set until it is re-run. That is stated in the cookbook
  rather than papered over — qfs is experimental and takes hard breaks, and a migration for a
  script the operator re-runs would cost more than it saves.
