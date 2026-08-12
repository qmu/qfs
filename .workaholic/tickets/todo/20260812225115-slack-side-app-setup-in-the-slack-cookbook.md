---
created_at: 2026-08-12T22:51:15+09:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
mission:
merge_policy: review
claim: work-20260813-023827
---

# Document what to create on the Slack side before connecting a workspace

## Overview

`docs/cookbook/slack.md` tells a reader the three `qfs` commands that bind a Slack token to a mount,
but never where that token comes from. The only vendor-console material in the article — "Post as
yourself (a user token)" (lines 195–224) — documents the **user** token (`xoxp-`) and assumes an app
already exists. Nothing in `docs/`, and nothing in `.workaholic/` history, documents the bot-side
path a person actually walks when adding a workspace: create the app, grant Bot Token Scopes,
install it, copy the `xoxb-` token — and then invite the app into the channel it must read, without
which every tail read comes back empty for a reason the article gives no way to diagnose.

Add a `## What to create on the Slack side` section immediately before `## Setup`, so the article
runs vendor-side prerequisites → `qfs` commands, matching the shape `docs/cookbook/gmail.md` already
uses (`### 0. Prerequisites` naming the console and the exact artifact). Correct the article's scope
table in the same pass: it currently lists only `chat:write` and `channels:history`, while the
article elsewhere teaches file listings, DM files, and the user directory, which need
`files:read`, `im:write`, `channels:read`, and `users:read`.

The article is the generation source for `plugins/qfs/skills/qfs-slack/SKILL.md`, so this change is
skill-affecting: regenerate and bump the plugin version in the same PR.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — the content belongs in the two
  existing files (`docs/cookbook/slack.md`, `docs/guide/connect.md`). Do **not** create a new
  `docs/cookbook/*.md`: an article with `skill_name` frontmatter generates a *new* skill and pulls in
  a marketplace `skills[]` entry plus a `.claude/skills/<name>` symlink, both asserted by
  `gen_skills.rs`.
- `workaholic:implementation` / `policies/objective-documentation.md` — every step names the actual
  console control and the observable result ("the page shows a **Bot User OAuth Token** beginning
  `xoxb-`"), never an evaluative adjective; state *why* `/invite` is required, because that reason is
  invisible from the commands.
- `workaholic:implementation` / `policies/command-scripts.md` — regeneration and drift checking go
  through the existing named commands (`cargo run -p xtask -- gen-skills [--check]`), never a hand
  edit of the generated skill.
- `workaholic:operation` / `policies/ci-cd.md` — `gen-skills --check` has **no** step in
  `.github/workflows/ci.yml`; it is a local/ship-flow gate and must be run explicitly, with the raw
  exit code recorded rather than a bare "green".
- `workaholic:design` / `policies/defense-in-depth.md` — restrictive defaults: grant the minimum
  scope for the work and say what each scope buys, so a reader can omit the one they do not need.
  The article's existing "A mount posts fine without `channels:history`" sentence is the model.
- `workaholic:design` / `policies/access-control.md` — the bot token is the simple sufficient
  default; the user token is reached for only when the requirement (a message attributed to a person)
  cannot be met otherwise. Keep "which token, which scope, which capability" readable in the single
  `### Scopes at a glance` table rather than restated in three places.
- `workaholic:design` / `policies/self-explanatory-ui.md` — use Slack's own labels verbatim so the
  reader can match text to screen, and connect each Slack-side failure mode to its corrective action
  (empty tail read → missing `channels:history` **or** the app is not in the channel).
- `workaholic:design` / `policies/data-sovereignty.md` — the section that teaches the grant also
  names the withdrawal: uninstall/revoke from Slack's app-management page, and the qfs-side removal
  of the account and mount.
- `workaholic:safety` / `policies/standard.md` — no real token in the article; minimal scopes, never
  a broad long-lived grant; the token reaches qfs on **stdin**, never argv, never a repo file. A
  reader landing directly on the new section must not be able to learn a
  `qfs account add slack <token>` argv form from it.
- `workaholic:planning` / `policies/accessibility-first.md` — the section needs its own stable
  heading; its slug is simultaneously the VitePress anchor `connect.md` links to and the landmark an
  agent reads inside the generated skill.
- `workaholic:planning` / `policies/terminology.md` — reuse the article's vocabulary exactly
  (operator, account label, mount, workspace, append log) and Slack's exactly (**Bot Token Scopes**,
  not "bot permissions"). Do not conflate the app-level token (`xoxa-`) with the bot token (`xoxb-`);
  they are different objects.

## Key Files

- `docs/cookbook/slack.md` - PRIMARY EDIT (258 lines). Insert the new `##` section between the
  "Slack isn't reachable until you connect…" bridge line (52) and `## Setup` (54). Existing sections
  to cross-reference, not restate: `## Post as yourself (a user token)` (195–224) and
  `### Scopes at a glance` (247–257). Container syntax in use: `::: tip`, `::: tip <title>`,
  `::: warning <title>`.
- `docs/guide/connect.md` - SECONDARY EDIT. `## GitHub & Slack — a token` (112–127). The house link
  form on this page is site-absolute with an anchor — line 106 reads
  `the [Gmail cookbook Setup](/cookbook/gmail#setup)` — so add
  `[…](/cookbook/slack#what-to-create-on-the-slack-side)`. Keep the page short: the walkthrough lives
  in the cookbook.
- `docs/cookbook/gmail.md` - HOUSE PATTERN. `### 0. Prerequisites` (80–85): a short bullet list
  naming the console with an inline link and the exact artifact to create.
- `docs/cookbook/gdrive.md` - The repo's precedent for cross-referencing instead of duplicating
  (links `/cookbook/gmail#setup` twice rather than restating the console steps).
- `packages/qfs/crates/skill/assets/examples/slack_driver.qfs` - TRUTH SOURCE for every scope claim.
  The channel tail view reads `/http/slack/conversations.history?channel={channel}` (→
  `channels:history`, and the app must be a channel member or Slack answers `not_in_channel`); the
  post map writes `/http/slack/chat.postMessage` (→ `chat:write`). It also uses
  `conversations.list` (channels view and every name→id lookup → `channels:read`), `users.list`
  (→ `users:read`), `files.list` (→ `files:read`), and `conversations.open` (DM → `im:write`).
- `packages/qfs/xtask/src/gen_skills.rs` - The generator. `parse_skill_source` reads the two-key
  frontmatter and takes **everything after it** as the body; `render_skill` emits new frontmatter +
  that body verbatim. Whatever is written into the article lands byte-for-byte in the skill.
- `packages/qfs/crates/test/tests/cookbook_skills.rs` - The ratchet. `extract_statements` (34–60)
  matches only fences opening with ```` ```qfs ````; ```` ```sh ````/```` ```text ```` are ignored.
  A `MIN_STATEMENTS = 45` floor guards the extractor.
- `plugins/qfs/skills/qfs-slack/SKILL.md` - GENERATED. Regenerate; never hand-edit.
- `plugins/qfs/.claude-plugin/plugin.json` - Plugin version field 1 (currently `0.19.0`).
- `plugins/qfs/.codex-plugin/plugin.json` - Plugin version field 2 (currently `0.19.0`).
- `.claude-plugin/marketplace.json` - **REPO ROOT**, not under `plugins/qfs/`. Carries version
  fields 3 and 4 (line 3 marketplace-level, line 13 inside `plugins[0]`), both `0.19.0`.
- `packages/qfs/crates/qfs/Cargo.toml` - Binary version, line 3, currently `0.0.95` → `0.0.96` per
  the every-shipped-PR rule.
- `packages/qfs/crates/qfs/src/shell.rs` - Lines 677–680 carry the Slack connect hint text, which
  matches the article's Setup commands verbatim; the new section hands off to Setup rather than
  restating those commands.

## Related History

The Slack article's only vendor-console material was authored deliberately for the **user** token and
scoped the bot-side walkthrough out; the docs-edit + regenerate + version-bump shape has an exact
precedent, and the plugin re-versioning rule exists precisely because a regenerated skill does not
reach an installed cache without it.

- [20260711010500-docs-slack-user-token-posting-guide.md](.workaholic/tickets/archive/work-20260711-121525/20260711010500-docs-slack-user-token-posting-guide.md) -
  Authored "Post as yourself", "Team proxy pattern", and "Scopes at a glance" (the direct ancestor).
  It scoped itself to the `xoxp-` path; it also set the standard that a new Slack docs section is
  verified against a live workspace before shipping.
- [20260630010120-connect-each-service-guide.md](.workaholic/tickets/archive/work-20260629-110121/20260630010120-connect-each-service-guide.md) -
  Created `docs/guide/connect.md` and its "GitHub & Slack — a token" section. Establishes the
  division to preserve: connect.md is the short per-service how-to, the cookbook holds the
  walkthrough; guide pages are hand-authored and outside `gen-docs --check`.
- [20260703150400-plugin-cache-staleness.md](.workaholic/tickets/archive/work-20260704-181053/20260703150400-plugin-cache-staleness.md) -
  Origin of the plugin re-versioning rule: an installed skill cache keeps serving stale Setup text
  until the plugin version moves.
- [20260724014200-retire-the-compiled-slack-driver.md](.workaholic/tickets/archive/work-20260803-213737/20260724014200-retire-the-compiled-slack-driver.md) -
  `/slack` is declared-only since commit b2ac31f; the compiled `driver-slack` crate is gone. Relevant
  because the new section sits directly above the Setup block whose commands must still be true under
  the declared driver.
- [20260725143000-cookbook-ratchet-only-parses-it-must-typecheck.md](.workaholic/tickets/todo/20260725143000-cookbook-ratchet-only-parses-it-must-typecheck.md) -
  Open todo documenting the ratchet's reach: parse-only, ```` ```qfs ```` fences only. Consequence
  here: the new section gets no automated coverage at all.
- [20260725143100-faq-under-describes-exit-2-and-the-new-refusals.md](.workaholic/tickets/todo/20260725143100-faq-under-describes-exit-2-and-the-new-refusals.md) -
  Open todo with the same docs-only + gen-skills + version-bump gate shape; reuse it.

## Implementation Steps

1. Read the current `docs/cookbook/slack.md` end to end and confirm the insertion point: the new
   `## What to create on the Slack side` goes between the "Slack isn't reachable…" bridge line and
   `## Setup`. Confirm the adjacent Setup block is still true under the declared-only `/slack`
   driver (its three commands should match the connect hint in `shell.rs:677-680`); fix it in this
   pass if it has drifted.
2. Write the new section as an ordered list in the voice of the existing "Post as yourself" steps —
   bold Slack labels, one observable result per step:
   **api.slack.com/apps** → **Create New App** → **From scratch** → name it and pick the workspace →
   **OAuth & Permissions** → add the **Bot Token Scopes** → **Install to Workspace** → approve →
   copy the **Bot User OAuth Token** (`xoxb-…`).
3. State the `/invite` requirement with its reason and its symptom: the channel tail is
   `conversations.history`, which answers `not_in_channel` for a channel the app has not joined, so
   `/invite @<app>` in each channel to be read is required and a *missing* invite looks exactly like
   a tail read that returns no rows. Put the symptom→cause→fix mapping where a reader hitting it will
   find it (a `::: warning` container is the article's device for this).
4. Extend `### Scopes at a glance` into a capability→scope table covering everything the article
   teaches, rather than adding a second table: `chat:write` (post), `channels:history` (read the
   channel tail), `channels:read` (the channels listing and every name→id lookup), `users:read` (the
   user directory / looking up a DM peer's `U…` id), `files:read` (channel and DM file listings and
   downloads), `im:write` (opening a DM). Verify each mapping against
   `packages/qfs/crates/skill/assets/examples/slack_driver.qfs` before writing it — the driver is the
   only authority on which API method a path actually calls. Keep the least-privilege framing: each
   row says what it buys, so a reader grants only what they use.
5. Cross-reference rather than duplicate: the new section links to
   `## Post as yourself (a user token)` for the `xoxp-` path and to `### Scopes at a glance` for the
   table; it does not repeat the `::: tip Prerequisites — an operator, an account, a mount` container
   that opens Setup, and it does not restate the three Setup commands.
6. Name the reversal in the same section: uninstalling the app / revoking the token from Slack's
   app-management page, and the qfs-side removal of the account and mount. **Verify the actual CLI
   verbs against `qfs account --help` and `qfs connect --help` before writing them** — do not assume
   `account remove` / `disconnect` exist under those names.
7. Make it explicit that a *second* workspace is a second account label and a second mount path of
   the reader's choosing (`qfs account add slack <label>` / `qfs connect /slack-<label> …`) — the
   Setup example's `default` label and `/slack` path are one instance, not a fixed name. This is the
   case that motivated the ticket.
8. Add the outbound link in `docs/guide/connect.md`'s "GitHub & Slack — a token" section, site-
   absolute with the anchor, matching line 106's form. Keep that page short — no console steps there.
9. If the section changes what the skill covers, update `skill_description` in the article
   frontmatter accordingly (it becomes the generated skill's `description`).
10. Regenerate: `cargo run -p xtask -- gen-skills`, then confirm `cargo run -p xtask -- gen-skills
    --check` exits 0. Never hand-edit `plugins/qfs/skills/qfs-slack/SKILL.md`; confirm by diff that
    it changed only where the article changed.
11. Bump the plugin version `0.19.0 → 0.19.1` in all four fields —
    `plugins/qfs/.claude-plugin/plugin.json`, `plugins/qfs/.codex-plugin/plugin.json`, and both
    fields in repo-root `.claude-plugin/marketplace.json`. Patch, not minor: additive prose, no
    taught surface retired.
12. Bump the binary patch version `0.0.95 → 0.0.96` in `packages/qfs/crates/qfs/Cargo.toml`.
13. Run the live verification below and record its before/after `/invite` observation in the PR.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- A person who has never created a Slack app can add a brand-new workspace to qfs following **only**
  the new section plus the existing Setup block — no Slack documentation, no outside help — and reach
  a channel tail read that returns rows.
- The new section exists as its own `##` heading immediately before `## Setup` in
  `docs/cookbook/slack.md`, and its auto-generated slug is the anchor `docs/guide/connect.md` links
  to; the link resolves to that heading on the running docs site.
- Every Slack control is named with the console's own label (**Create New App**, **From scratch**,
  **OAuth & Permissions**, **Bot Token Scopes**, **Install to Workspace**, **Bot User OAuth Token**),
  and each step states its observable result.
- The `/invite` requirement is stated with its reason (`conversations.history` answers
  `not_in_channel` for a non-member app) **and** its symptom (a tail read returning no rows).
- `### Scopes at a glance` maps each capability the article teaches to its scope — `chat:write`,
  `channels:history`, `channels:read`, `users:read`, `files:read`, `im:write` — with no second,
  competing table anywhere in the article, and every mapping traceable to a call in
  `slack_driver.qfs`.
- No token value appears on argv anywhere in the new text; the stdin form is the only one shown. No
  real token appears in the diff.
- The revocation path is named, with CLI verbs confirmed to exist against `--help` output.
- `plugins/qfs/skills/qfs-slack/SKILL.md` is byte-identical to a fresh render of the article, and
  carries no hand edit.
- All four plugin version fields read `0.19.1`; `packages/qfs/crates/qfs/Cargo.toml` reads `0.0.96`.

**Verification method** — the commands/tests/probes that prove them:

- **Live, in a real workspace** (the decisive check — the section has no automated coverage): follow
  the drafted section verbatim to create the app in an actual new Slack workspace, grant only the
  scopes it names, install, and pipe the `xoxb-` token in on stdin; `qfs connect` it to its own mount
  path; then run a channel tail read **before** inviting the app to the channel and **after** —
  recording both outcomes. The before/after pair is the direct proof of the section's central claim
  and of the symptom→cause mapping. Any step whose label or result differs from the console is
  corrected in the article before the PR.
- `cargo run -p xtask -- gen-skills && cargo run -p xtask -- gen-skills --check` — record the raw
  exit code (0). This gate has no CI step; it only runs if run.
- `cargo test -p <the test crate> --test cookbook_skills` green (the ratchet does not cover the new
  prose, but must not regress; the `MIN_STATEMENTS = 45` floor still has to hold).
- `git diff` review of `plugins/qfs/skills/qfs-slack/SKILL.md` confirming the change is exactly the
  regenerated article body.
- The anchor is checked **by eye** on the running docs site: `docs/.vitepress/config.mts` sets
  `ignoreDeadLinks: true`, so a wrong anchor fails silently.
- `cargo fmt --all --check` and `cargo clippy --workspace --all-targets -- -D warnings` for the
  version-bump edit, per the standing ship gates.

**Gate** — what must pass before approval:

- The live verification is done and its before/after `/invite` observation is written into the PR —
  not "looks right", but what the read actually returned in each state.
- `gen-skills --check` exit 0, shown as a raw exit code.
- The cookbook parse test is green.
- All four plugin version fields and the binary version are bumped in the same PR.
- The release scan is clean — no credential value anywhere in the branch diff.

## Considerations

- The ratchet gives this change **zero** coverage: `extract_statements` matches only ```` ```qfs ````
  fences, so prose and ```` ```sh ```` blocks are never checked
  (`packages/qfs/crates/test/tests/cookbook_skills.rs` lines 34-60). This is precisely why the live
  verification is the gate rather than a formality.
- The marketplace manifest is at **repo root** `.claude-plugin/marketplace.json`, not under
  `plugins/qfs/`. Two of the four version fields live there (lines 3 and 13).
- Whatever is written lands verbatim in the generated skill, including VitePress containers and
  site-absolute links — an agent reading `SKILL.md` sees `::: warning` markup and `/cookbook/…`
  paths. That is the article's existing house style; keep it consistent rather than inventing a
  skill-only variant (`packages/qfs/xtask/src/gen_skills.rs`, `render_skill`).
- The two-scope minimum is true only for the post + channel-tail happy path. Writing it as the
  blanket answer would leave a reader who tries the file listing or a DM stuck on an unexplained
  failure — that is the gap step 4 closes
  (`packages/qfs/crates/skill/assets/examples/slack_driver.qfs`).
- Slack's app-level token (`xoxa-`) is a different object from the bot token (`xoxb-`); the section
  must not present them as alternatives.
- `docs/cookbook/faq.md` carries the same three-step cloud-connect narrative and is also a generated
  skill; if the new section changes how connecting a workspace is described, check the FAQ still
  agrees (`docs/cookbook/faq.md` lines 32, 93-96).
- The existing "Post as yourself" section's scope sentence and the new bot-side text must not
  disagree about `chat:write` / `channels:history` (`docs/cookbook/slack.md` lines 202-204).
