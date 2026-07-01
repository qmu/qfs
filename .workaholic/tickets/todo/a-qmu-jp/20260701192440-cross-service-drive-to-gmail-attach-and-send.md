---
created_at: 2026-07-01T19:24:40+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category:
depends_on: [20260701192439-query-array-struct-bytes-literals-gmail-draft-attachments.md]
---

# Cross-service Drive→Gmail attach-and-send recipe

## Overview

**Sub-ticket of EPIC `.workaholic/tickets/todo/a-qmu-jp/20260630203000-epic-replace-gmail-ftp-gdrive-ftp.md`.** This is the **dogfooding payoff**: the exact workflow that motivated the epic — *download a file from Google Drive, attach it to a Gmail message, and send it* — in one qfs statement, with a documented recipe.

Once the foundation ticket (`20260701192439`) lands the `Array`/`Struct`/`Bytes` literals and the Gmail draft accepts an attachments column, the remaining gap is **composition**: a Drive read (`driver-gdrive/src/read.rs::content_batch` already yields `name`/`mime_type`/`content(Bytes)` for a single file) must be shaped into the draft's `attachments` `Array(Struct{filename, mime, bytes})` column and fed to `INSERT INTO /mail/drafts`. Discovery found `EffectBody::Pipeline` already supports `INSERT ... FROM <query>` (folded by `eval.rs::effect_input_schema`), but there is **no rows→Array pack** primitive — the inverse of `EXPAND` (`engine/src/eval.rs` ~385-406 implements `EXPAND`; no pack exists). Building the attachments array from N Drive rows needs that aggregate, plus column renames (`content`→`bytes`, `name`→`filename`, `mime_type`→`mime`).

Experimental posture (memory): straight additive change, no compat shims. Patch bump on the shipped PR.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — applies to all code work.
- `workaholic:implementation` / `policies/coding-standards.md` — applies to all code work.
- `workaholic:implementation` / `policies/type-driven-design.md` — the pack aggregate is a typed rows→`Array(Struct)` transform; express shape errors as `Result`.
- `workaholic:implementation` / `policies/domain-layer-separation.md` — cross-service work is composed at the query/engine layer over the two drivers' paths; one driver must not import another.
- `workaholic:planning` / `policies/ai-native-future.md` — the recipe must be expressible in the one grammar and the describe→preview→commit loop; the send stays behind the explicit irreversible `CALL`/COMMIT gate.
- **Anti-drift (CLAUDE.md):** the cross-service recipe goes in a cookbook article (`docs/cookbook/cross-service.md` and/or `gmail.md`) — parse-checked by `cookbook_skills.rs`; regenerate SKILL.md via `gen-skills`.

## Key Files

- `packages/qfs/crates/engine/src/eval.rs` - `EXPAND` (~385-406); add the inverse rows→`Array(Struct)` pack/collect aggregate.
- `packages/qfs/crates/core/src/typeck.rs` - type the pack aggregate (rows → `Array(Struct{...})`); higher-order/aggregate typing (~303-380).
- `packages/qfs/crates/core/src/eval.rs` - `effect_input_schema` / `INSERT ... FROM` pipeline folding (~777-818).
- `packages/qfs/crates/driver-gdrive/src/read.rs` - `content_batch` (~192-204); the source `content(Bytes)` column.
- `packages/qfs/crates/driver-gmail/src/effect.rs` - `attachments_col` (~289-329); the sink column contract.
- `docs/cookbook/cross-service.md` - add the Drive→Gmail attach-and-send recipe.
- `docs/guide/replace-gmail-gdrive-ftp.md` - add a cross-service row for the compose→attach→send chain.

## Related History

- [20260630203020-gdrive-ftp-parity-gaps.md](.workaholic/tickets/archive/work-20260629-110121/20260630203020-gdrive-ftp-parity-gaps.md) - Wired the single-file Drive content download (`content_batch`) this recipe reads from.
- [20260630203010-gmail-ftp-parity-gaps.md](.workaholic/tickets/archive/work-20260629-110121/20260630203010-gmail-ftp-parity-gaps.md) - The Gmail draft/send write leg the recipe targets.

## Implementation Steps

1. Add a rows→`Array(Struct)` pack aggregate in `engine/src/eval.rs` (inverse of `EXPAND`), with typing in `core/src/typeck.rs`.
2. Support the column shaping (rename/select `content`→`bytes`, `name`→`filename`, `mime_type`→`mime`) within the sub-pipeline feeding `INSERT INTO /mail/drafts FROM (...)`.
3. Verify `effect_input_schema` folds the packed column into the draft's `attachments` schema.
4. Add a hermetic test: a mock Drive read → pack → `INSERT INTO /mail/drafts` produces a `MailDraft` with the Drive file's bytes as an attachment (asserted via `MockGmailClient` + mock Drive client; PREVIEW does zero I/O).
5. Write the `docs/cookbook/cross-service.md` recipe and the migration-guide row; run `gen-docs` / `gen-skills`.

## Quality Gate

**Acceptance criteria:**

- A single qfs statement of the form `INSERT INTO /mail/drafts FROM (/drive/... |> select/pack ...)` (final syntax per implementation) parses, type-checks, and yields a draft whose `attachments` array carries the Drive file's bytes/filename/mime.
- Followed by `CALL mail.send`, the chain drafts-then-sends behind the explicit irreversible gate (`--commit --commit-irreversible` in one-shot).
- The cross-service recipe passes the `cookbook_skills.rs` parse-check ratchet.
- Patch version bumped.

**Verification method** (from `packages/qfs`, `TMPDIR` redirected, `command rm`):

- `cargo build/test/clippy/fmt` all green, including the new engine pack aggregate and cross-service mock test.
- `cargo run -p xtask -- gen-docs --check` and `gen-skills --check` green.
- **Live proof:** against the real Google account, run the documented recipe to download an actual Drive file, attach it, and send; confirm the received Gmail carries the correct file.

**Gate** — hermetic suite + both `--check` gates green AND the live Drive→Gmail attach-and-send verified end-to-end in-session.

## Considerations

- Depends on `20260701192439` (literals + draft attachment column) — do not start before it lands.
- The pack aggregate is the genuinely new primitive here; keep it general (rows→`Array(Struct)`), not Gmail-specific, so it lives in the engine, not a driver (`domain-layer-separation`).
- Memory limits: a large Drive file becomes a `Value::Bytes` cell flowing through the effect pipeline — confirm no unnecessary copies; note any practical size ceiling in the cookbook.
- Irreversible send must remain reachable only via explicit `CALL mail.send` + `--commit-irreversible` (`ai-native-future` — observable/interruptible).
