---
created_at: 2026-07-07T18:26:10+09:00
author: a@qmu.jp
type: enhancement
layer: [UX, Infrastructure]
effort:
commit_hash: 906b702
category: Added
depends_on:
mission:
---

# Document a Chatwork qfs integration example that works with Google Drive

## Overview

Add a docs example showing how qfs can integrate Chatwork through the declared-driver surface and use it with an already-mounted Google Drive path. The example must be truthful about the current implementation state: cookbook placement is acceptable only for commands verified against the shipped runtime; otherwise the docs must label the Chatwork script as a declared-integration example and avoid claiming live behavior.

The local qfs environment already lists `/drive gdrive`, `/mail gmail`, and `/slack slack` from `qfs connect --list`. After qfs auth was completed, `qfs run "/drive/my |> select name, mime_type |> limit 5" --json` returned real My Drive rows (PDF and Markdown files, correctly shaped with name and mime_type).

The benchmark workflow is: use My Drive as the Drive target, load a Chatwork API token from `.env`, find a Chatwork room by a specified room name, read the latest message in that room, and send a Slack message asking a team member to investigate that Chatwork message. The local `.env` now contains `CHATWORK_API_TOKEN`; the value was not read or printed. The docs and implementation must preview every write before any commit.

## Policies

The standard engineering policies that govern this ticket:

- `workaholic:implementation` / `policies/directory-structure.md` - place user-facing docs under the existing `docs/` cookbook or guide structure.
- `workaholic:implementation` / `policies/coding-standards.md` - applies if tests, doc extraction code, or parser fixtures are changed.
- `workaholic:implementation` / `policies/objective-documentation.md` - every documented qfs command must describe verified behavior, not aspirational behavior.
- `workaholic:implementation` / `policies/test.md` - examples should be parser-checked and, where credentials are available, verified against the real service.
- `workaholic:design` / `policies/vendor-neutrality.md` - Chatwork should be expressed through declared-driver/config surfaces rather than hard-coded product-specific Rust unless implementation discovery proves that is not currently possible.
- `workaholic:operation` / `policies/ci-cd.md` - use local repository commands and live probes as the approval evidence, not unverified prose review.

## Key Files

- `docs/cookbook/gdrive.md` - existing Google Drive setup and Drive path recipes.
- `docs/guide/connect.md` - existing service connection instructions and Google account setup.
- `docs/cookbook/index.md` - cookbook index if a Chatwork page is added.
- `docs/.vitepress/config.mts` - sidebar/nav if a new docs page is added.
- `docs/blueprint.md` - current Chatwork declared-driver example and self-hosting design notes.
- `packages/qfs/crates/parser/src/tests.rs` - parser coverage for `CREATE DRIVER chatwork`, parameterized views, and maps.
- `packages/qfs/crates/exec/src/declared.rs` - declared view/map runtime tests using Chatwork path examples.
- `.env` / `.env.example` - if a sample env file is added, name the Chatwork token `CHATWORK_API_TOKEN`.

## Related History

Past tickets show that Drive and Google account setup have been wired and documented, while Chatwork currently appears as a declared-driver example rather than a cookbook chapter.

- [20260630010000-wire-google-drive-read.md](.workaholic/tickets/archive/work-20260629-110121/20260630010000-wire-google-drive-read.md) - wired `/drive` reads through the live Google Drive driver.
- [20260630203040-gmail-gdrive-to-qfs-guidance-doc.md](.workaholic/tickets/archive/work-20260629-110121/20260630203040-gmail-gdrive-to-qfs-guidance-doc.md) - established the rule that Google docs must be reproducible from the docs alone.
- [20260704145136-declared-driver-surface.md](.workaholic/tickets/archive/work-20260705-032203/20260704145136-declared-driver-surface.md) - introduced the declared-driver surface that Chatwork should use.
- [20260704145137-declared-driver-evaluator.md](.workaholic/tickets/archive/work-20260705-032203/20260704145137-declared-driver-evaluator.md) - implemented evaluation behavior for declared views/maps.

## Implementation Steps

1. Verify the current shipped surface before writing docs:
   - Run `qfs connect --list` and confirm `/drive gdrive` is mounted.
   - Run `qfs run "/drive/my |> select name, mime_type |> limit 5" --json` in the owner environment and record that My Drive returns rows.
   - Run the existing parser/runtime tests that cover the Chatwork declared-driver examples.
2. Decide the docs placement from verification:
   - If Chatwork declared-driver install/connect/read/write works end to end, add `docs/cookbook/chatwork.md` and link it from `docs/cookbook/index.md` and `docs/.vitepress/config.mts`.
   - If only parsing/evaluation examples are shipped, add a guide section for declared integrations instead of a cookbook chapter, and clearly state that the example is not yet a live Chatwork connector.
3. Include a credential-free Chatwork declaration example based on the existing blueprint shape:
   - `CREATE DRIVER chatwork AT 'https://api.chatwork.com/v2' AUTH HEADER 'x-chatworktoken'`
   - `CREATE TYPE /type/chatwork/message (...)`
   - `CREATE VIEW /chatwork/rooms AS ...`
   - `CREATE VIEW /chatwork/rooms/{room}/messages OF /type/chatwork/message AS ...`
   - `CREATE MAP INSERT /chatwork/rooms/{room}/messages AS ...`
   The docs must explain that token values are loaded from `.env` as `CHATWORK_API_TOKEN`, then sealed into qfs or passed into the declared-driver account path without embedding the token in the `.qfs` script.
4. Add the benchmark workflow the owner actually needs:
   - Read My Drive with `/drive/my` as the target Drive path.
   - Given a room-name fragment from the owner, find the matching Chatwork room.
   - Read the latest message in that Chatwork room.
   - Compose a Slack message to the relevant team channel/member asking them to investigate the latest Chatwork message, including the room name/id, message id/time, and a short message excerpt.
   - Preview the Slack post first; commit only after explicit approval.
5. Include a minimal `.env` contract in docs:
   - `CHATWORK_API_TOKEN=<token value>` for Chatwork.
   - Verify presence without printing the value, e.g. test that `.env` contains a `CHATWORK_API_TOKEN=` entry.
   - Reuse the existing Slack qfs account/mount instead of introducing `SLACK_TOKEN` unless the implementation needs to provision Slack from scratch.
6. Update docs tests or parser fixtures so fenced `qfs` blocks either parse or are intentionally marked as non-runnable explanation blocks.
7. Keep Google Drive setup instructions linked to `docs/guide/connect.md` and `docs/cookbook/gdrive.md` instead of duplicating secrets or OAuth details.

## Quality Gate

**Acceptance criteria** - the checkable conditions that must hold:

- The docs contain one Chatwork integration example that states whether it is runnable today or a declared-driver design/example only.
- The docs contain one benchmark workflow that reads `/drive/my`, finds a Chatwork room by name, reads the latest Chatwork message, and previews a Slack investigation request.
- No documentation tells users to paste Chatwork or Google secrets into source files, query files, shell history, or command arguments.
- The Chatwork token env var is named `CHATWORK_API_TOKEN` everywhere it is documented.
- If a new cookbook page is added, it is linked from both the cookbook index and VitePress sidebar.
- The Drive setup path reflects the current local state: `/drive` is mounted and `/drive/my` live reads work after qfs auth.

**Verification method** - the commands/tests/probes that prove them:

- `qfs connect --list` shows `/drive gdrive`.
- `qfs run "/drive/my |> select name, mime_type |> limit 5" --json` returns real Drive rows.
- `.env` contains `CHATWORK_API_TOKEN=` and verification output never includes the token value.
- With `CHATWORK_API_TOKEN` loaded from `.env`, the Chatwork room lookup can identify a room by the owner-specified name and read its newest message.
- The Slack investigation request is previewed before any commit.
- Parser/runtime tests covering Chatwork declared-driver statements pass.
- The docs build succeeds with the repo's docs build command.
- Any live Chatwork post is first run as a preview and committed only to a test room after owner approval.

**Gate** - what must pass before approval:

- Docs build is green.
- Chatwork examples are either parser/runtime verified or explicitly labelled non-runnable.
- Drive read is verified against `/drive/my` in the owner environment.
- The benchmark is demonstrated through preview: latest Chatwork message found, Slack investigation request rendered, no Slack write committed without approval.
- If a live Chatwork token and room are supplied, any Chatwork write is previewed before commit and committed only after explicit approval.

## Considerations

- Information still needed from the owner before live verification: the Chatwork room-name fragment to search for, the Slack target channel/member, and explicit approval before committing any Slack or Chatwork write.
- The Chatwork API token is already present in `.env` as `CHATWORK_API_TOKEN`. Do not print it, commit it, or pass it on argv.
- Do not invent a `qfs account add chatwork` command unless implementation discovery proves that exact command is supported.
- The current `docs/cookbook/` pattern is for shipped behavior. If Chatwork is not live end to end, keep the example outside the cookbook or mark it as non-runnable.
- `qfs describe /drive --json` already reports `/drive` as a `blob_namespace` with `SELECT` and `LS`, and `/drive/my` has now returned live rows, so the Drive side can be documented as a current mounted path.

## Final Report

Implemented a guide-level Chatwork + Drive benchmark page and linked it from the docs sidebar and README. The guide documents the verified `/drive/my` probe, the Chatwork declared-driver declarations, the vault-backed `chatwork/work` credential flow, room lookup, latest-message retrieval, and Slack investigation-request preview shape. `.env.example` names `CHATWORK_API_TOKEN`, while `.gitignore` keeps real `.env` files out of git.

The live declared-driver path now carries the path binding's `secret_ref` into both read and commit registration. `CONNECT /chatwork TO chatwork SECRET 'vault:chatwork/work'` resolves through the encrypted qfs vault at request time, while `env:CHATWORK_API_TOKEN` remains supported as a bootstrap reference. `qfs account add chatwork work` now accepts Chatwork as a token-backed provider, stores the token from stdin, and lists `chatwork/work` as metadata only.

Verification completed: `cargo test --manifest-path packages/qfs/Cargo.toml -p qfs account::tests`, `cargo test --manifest-path packages/qfs/Cargo.toml -p qfs declared_secret_ref_store`, `cargo test --manifest-path packages/qfs/Cargo.toml -p qfs declared_driver_reads_and_writes_end_to_end_hermetically`, `cargo test --manifest-path packages/qfs/Cargo.toml -p qfs-parser full_chatwork_script_parses_statement_for_statement`, and `npm run docs:build`. Live probes verified `/drive/my` rows, imported `CHATWORK_API_TOKEN` into the qfs vault without printing it, rebound `/chatwork` to `vault:chatwork/work`, and listed nine `くむ` Chatwork rooms without sourcing `.env`.
