---
created_at: 2026-08-13T02:47:53+09:00
status: done
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

## Final Report

Development completed as planned, with the direction split per operation rather than taken wholesale
either way — the ticket asked which way to close the gap, and the three operations do not have the
same answer.

### The direction, and the evidence for each half

**Detach — implemented in the declaration.** `files.delete` takes the file id and nothing else, so
it is one request, which is exactly what a declared map is. It ships as

```
CREATE MAP REMOVE /slack/{ws}/files/{file} AS
  INSERT INTO /http/slack/files.delete VALUES ({file: path.file}) IRREVERSIBLE;
```

The body is an `INSERT` because Slack's methods are POSTs — the same map-verb/body-verb split the
five CALL maps already ride, and the new test asserts it (a `REMOVE` body would issue
`DELETE /files.delete`, which Slack does not serve).

**Download — removed from the article.** Slack serves file bytes from `url_private`, which requires
the app's bearer token. The only primitive that could fetch it is the declared `FOLLOW` stage, and
`follow_bytes` (`crates/driver-http/src/applier.rs`, ~100) injects **no auth by design**: "the
follow URL is self-authorizing … sending the driver's credential to the URL's (foreign) host would
leak it". That is why the Chatwork precedent works — its `download_url` is self-authorizing — and
why Slack's is not a matter of writing one more view. An authorized download would need a new
primitive, which is a ruling of its own, not this ticket's.

**Upload — removed from the article.** Slack's current flow is three calls
(`files.getUploadURLExternal` → PUT the bytes out-of-band → `files.completeUploadExternal`) and
`files.upload` is retired for new apps. A declared map evaluates to **one** wire body
(`eval_map_body` → one `MapWrite`), so the flow is not expressible today, unlike Chatwork's
single-POST multipart upload.

Removing two taught operations is a taught-surface break, so the four plugin `version` fields moved
`0.19.4 → 0.20.0` (minor), and `cross-service.md`'s Slack tip — which pointed at the deleted upload
section — now names the gap and its reason instead of a recipe.

### Step 1, confirmed from the binary rather than the source

`describe` cannot answer for a declared mount at all, which is its own live defect
(`20260728085253`), so the probes below are what a declared driver can actually be asked:

```
$ qfs describe /slack/acme/files/F0123
{"error":{"code":"unknown_mount","kind":"capability","message":"no driver is mounted for `/slack/acme/files/F0123` (describe registry)","path":"/slack/acme/files/F0123"}}

$ qfs apply crates/skill/assets/examples/slack_driver.qfs      # before this change
qfs: apply committed: 16 effect(s) — 16 added, 0 changed, 0 destroyed.
$ qfs apply crates/skill/assets/examples/slack_driver.qfs      # after
qfs: apply committed: 17 effect(s) — 17 added, 0 changed, 0 destroyed.

$ qfs run "remove /slack/acme/files/F0123"
{"preview":{"rows":[{"id":0,"verb":"REMOVE","target":{"driver":"slack","path":"/slack/acme/files/F0123"},"affected":"unknown","irreversible":true}],"irreversible":[0],…},"committed":false}
{"error":{"code":"commit_required","kind":"commit_required","message":"destructive set-wide plan: re-run with --commit (or a trailing COMMIT) to apply"}}
EXIT=4
```

**What could NOT be verified here, stated rather than implied.** A `/slack/…` read needs a connected
workspace, and connecting one is an OAuth account flow this environment has no credentials for:

```
$ qfs connect /slack/acme --driver slack --secret env:SLACK_FAKE_TOKEN
qfs: error: cloud driver `slack` needs --account <label> — run `qfs account add slack <label>` first
```

So the listings the article teaches were not exercised against live Slack by this run; they are
unchanged by it, and the twin's existing row-equivalence tests still cover them. The detach's wire
behaviour IS proved, hermetically, at the level that decides correctness — the exact POST URL and
body — in `shipped_slack_detach_map_fires_files_delete_behind_the_gate`.

### Quality Gate

**Criteria.** Every `/slack/…` path and verb the article teaches now resolves against the shipped
declaration or no longer appears; the `skill_description` advertises only what exists; a test beside
the `slack_twin_*` family exercises the file write; `gen-skills --check` exits 0 with no hand-edited
`SKILL.md`; the plugin version moved minor for the taught-surface removal.

**Verification.** The probes above; `cargo test --workspace` → 2724 passed, 0 failed (was 2723 —
the new detach test); `cargo run -p xtask -- gen-skills --check` → exit 0; `gen-docs --check` → 0;
`check-migrations` → 0; `cargo clippy --workspace --all-targets -- -D warnings` → 0;
`cargo fmt --all --check` → 0.

### Discovered Insights

- **Insight**: a declared map's verb and its body's verb are two different things — the map's verb
  is what an incoming statement matches on, the body's verb is what reaches the wire
  (`MapWrite::wire_kind`).
  **Context**: this is what lets a `REMOVE` map fire a POST, which every Slack write needs, and it
  is not obvious from the existing declarations because the Chatwork and Cloudflare maps happen to
  agree with their bodies. Getting it wrong is silent in review and loud at the wire.
- **Insight**: `FOLLOW`'s credential-free rule is a security property, not an omission
  (`driver-http/src/applier.rs`), so "add a blob view like Chatwork's" is not portable to any
  service whose file URL is bearer-authorized.
  **Context**: worth checking before promising a download for a new declared driver — the question
  is whether the delivered URL is self-authorizing, not whether the service has a file API.
- **Insight**: an unmapped declared write previews as an ordinary effect row rather than refusing —
  measured while confirming this ticket's own gap, and minted as `20260816213014`.
  **Context**: it is the mechanism that let this article's false upload survive every hand-check:
  the recipe previewed green because nothing between the parser and the applier asks whether a map
  exists.
