---
created_at: 2026-08-13T02:47:53+09:00
author: a@qmu.jp
assignees: []
depends_on:
mission:
merge_policy: review
claim: work-20260816-211551
---

# The Slack cookbook teaches a file upload, download, and detach the driver does not implement

## Overview

`docs/cookbook/slack.md` documents three Slack file operations that have no implementation in the
tree:

- **Download** — "Download one by its id — `/slack/acme/files/F0123` returns a `content` column
  carrying the bytes" (article, *Files shared in a channel or DM*). There is no
  `/slack/{ws}/files/{id}` view in the declared driver.
- **Upload** — the whole `## Upload a file to Slack (and detach)` section, built on
  `UPSERT INTO /slack/<ws>/files` with a `{filename, mime, bytes}` row and an optional `channel`.
  There is no `CREATE MAP UPSERT` for any `/slack/…` path.
- **Detach** — `remove /slack/acme/files/F0123` behind the irreversible gate. There is no
  `CREATE MAP REMOVE` for any `/slack/…` path.

The compiled `driver-slack` crate was deleted when `/slack` became declared-only
(`20260724014200-retire-the-compiled-slack-driver.md`, commit b2ac31f). The declared twin
(`packages/qfs/crates/skill/assets/examples/slack_driver.qfs`) reproduces the *listing* views —
`/slack/{ws}/files` and `/slack/{ws}/{channel}/files` over `files.list` — plus the message reads,
the post map, and five CALLs. It carries no blob read and no file write, and the twin's equivalence
tests (`packages/qfs/crates/qfs/src/declared_driver.rs`,
`slack_twin_replies_reactions_files_and_users_are_row_equivalent`) assert only the listings.

Because the article is the generation source for `plugins/qfs/skills/qfs-slack/SKILL.md`, an agent
loading that skill is being taught three operations the binary cannot perform. The cookbook ratchet
does not catch it: it checks that a ```qfs recipe **parses**, not that its path resolves to a
declared view or map.

Decide which way to close the gap — implement the three operations in the declaration, or remove
them from the article — and make the article and the driver agree.

## Policies

- `workaholic:implementation` / `policies/objective-documentation.md` — the governing policy:
  documentation describes the actual behavior of the code, not the intended behavior at the time of
  writing. Three sections currently describe a retired implementation.
- `workaholic:implementation` / `policies/directory-structure.md` — whichever way this closes, the
  change lands in the existing article and the existing declaration asset, not a new file.
- `workaholic:planning` / `policies/accessibility-first.md` — the same knowledge reaches a human
  reader and an AI agent from one source; a false recipe reaches both.
- `workaholic:design` / `policies/access-control.md` — if the upload is reimplemented, the scope it
  needs (`files:write`) joins the article's capability→scope table rather than being assumed.

## Key Files

- `docs/cookbook/slack.md` - The three affected passages: the download sentence at the end of
  *Files shared in a channel or DM*, and the whole *Upload a file to Slack (and detach)* section.
  Also the capability→scope table in *Scopes at a glance*, which deliberately carries no
  `files:write` row today because nothing writes files.
- `packages/qfs/crates/skill/assets/examples/slack_driver.qfs` - The declared driver. Has
  `CREATE VIEW /slack/{ws}/files` and `/slack/{ws}/{channel}/files` over `files.list`; has no
  `{id}` blob view, no `MAP UPSERT`, no `MAP REMOVE`.
- `packages/qfs/crates/skill/assets/examples/chatwork.qfs` - The worked precedent for both halves:
  a blob download via the `FOLLOW` stage and an upload via `ENCODE multipart`. If the Slack
  operations are reimplemented, this is the shape to copy — Slack's upload is a three-call external
  flow (`files.getUploadURLExternal` → PUT the bytes → `files.completeUploadExternal`), so check
  whether the declaration DSL can express it before committing to that direction.
- `packages/qfs/crates/qfs/src/declared_driver.rs` - The twin equivalence tests; the files test
  covers listings only.
- `packages/qfs/crates/test/tests/cookbook_skills.rs` - Shows why this survived: recipes are
  parse-checked, never resolved against the declared registry. Ticket
  `20260725143000-cookbook-ratchet-only-parses-it-must-typecheck.md` is the standing proposal to
  close that class of gap; this ticket is one instance of it.

## Related History

- [20260724014200-retire-the-compiled-slack-driver.md](.workaholic/tickets/archive/work-20260803-213737/20260724014200-retire-the-compiled-slack-driver.md) -
  Retired the compiled crate on the strength of row-equivalence for the reads. The file write and
  blob read appear not to have been part of that equivalence set, which is how the article outlived
  its implementation.
- [20260725143000-cookbook-ratchet-only-parses-it-must-typecheck.md](.workaholic/tickets/todo/20260725143000-cookbook-ratchet-only-parses-it-must-typecheck.md) -
  The general form of the hole this fell through.

## Implementation Steps

1. Confirm the gap from the binary rather than from the source: build the workspace binary and run
   `qfs describe /slack/acme/files/F0123` and `qfs describe /slack/acme/files`, and preview an
   `upsert into /slack/acme/files` and a `remove /slack/acme/files/F0123`. Record the raw refusals.
   (This ticket's Overview is a source reading; the describe output is the proof.)
2. Decide the direction and record the reasoning in the Final Report: reimplement in the declaration
   versus remove from the article. Weigh what `chatwork.qfs` proves is expressible — a `FOLLOW`
   download and an `ENCODE multipart` upload — against Slack's three-call external upload flow,
   which may not be expressible as a single declared map today.
3. Carry out the decision so the article and the driver agree: either add the blob view, the
   `MAP UPSERT`, and the `MAP REMOVE` (with an equivalence or behavior test beside the existing twin
   tests), or delete the three passages and any recipe that depends on them.
4. If the operations stay, add a `files:write` row to the article's *Scopes at a glance* table and
   name the upload's Slack scope correctly. If they go, check the article's `skill_description`
   frontmatter, which currently advertises "list and download the files shared in a channel or DM …
   upload a file's bytes and detach (delete) it".
5. Regenerate the skills (`cargo run -p xtask -- gen-skills`) and bump the four plugin `version`
   fields — a taught-surface removal is a **minor** bump, an added implementation a patch.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- Every `/slack/…` path and verb the article teaches resolves against the shipped binary: each one
  either returns rows / previews an effect, or no longer appears in the article.
- The article's `skill_description` frontmatter advertises only operations that exist.
- If the operations are reimplemented: a test beside the existing `slack_twin_*` tests exercises the
  blob read and the file write, and the article names the Slack scope each needs.
- `cargo run -p xtask -- gen-skills --check` exits 0 and no `SKILL.md` was hand-edited.
- The plugin version moved in the direction the change warrants (minor if a taught surface was
  removed).

**Verification method** — the commands/tests/probes that prove them:

- `qfs describe` on each documented Slack path, plus a `--dry-run` preview of the upload and the
  remove, with the raw output recorded.
- `cargo test --workspace` green, including any new declared-driver test.
- `cargo run -p xtask -- gen-skills --check` — raw exit code.

**Gate** — what must pass before approval:

- No path or verb remains in `docs/cookbook/slack.md` that the binary rejects, demonstrated by the
  describe/preview output rather than asserted.
- The workspace gates are green with raw exit codes shown.

## Considerations

- Removing the sections is a **taught-surface break** for the `qfs-slack` skill — a minor plugin
  bump, and installed caches keep teaching the retired recipes until it lands
  (`plugins/qfs/.claude-plugin/plugin.json` and the three sibling fields).
- Slack's modern upload is a three-call flow with an out-of-band PUT; the retired compiled driver
  implemented it in Rust. Whether the declaration DSL can express that today is the crux of step 2
  and should be settled before any promise is made in the article
  (`packages/qfs/crates/skill/assets/examples/slack_driver.qfs`).
- The same class of drift may exist in other cookbook articles that outlived a compiled driver;
  worth a sweep once the ratchet ticket lands
  (`packages/qfs/crates/test/tests/cookbook_skills.rs`).
