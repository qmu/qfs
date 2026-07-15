---
created_at: 2026-07-07T04:33:12+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain, Infrastructure]
effort: 2h
commit_hash: 29e627f
category: Added
depends_on: []
mission:
---

# Fix Drive blob upload commit paths for local report copies

## Overview

Uploading a generated Markdown report from a local repository into Google Drive exposed inconsistent qfs behavior across the documented Drive write paths. Small literal `UPSERT` writes can preview and commit when run outside the sandbox, but a larger Markdown report with apostrophes and Markdown punctuation could not be made to round-trip through the one-shot literal grammar. The interactive `cp` path produced a correct-looking local-read plus Drive-upsert plan and reported `COMMITTED`, but the destination file was absent from `/drive/my` afterward.

The expected operator behavior is that a local file can be copied to Drive through qfs with the normal describe → preview → commit loop, without hand-escaping the full file as a query literal and without false-positive commits.

## Policies

- `workaholic:planning` / `policies/accessibility-first.md` — qfs is an AI/operator control plane; common file handoff tasks must be reachable by agents without fragile manual escaping.
- `workaholic:design` / `policies/self-explanatory-ui.md` — the preview/commit loop must report effects that match the committed result.
- `workaholic:design` / `policies/data-sovereignty.md` — Drive upload is a user-data export/copy path and must be reliable and verifiable.
- `workaholic:implementation` / `policies/directory-structure.md` — fix should live in the existing driver / exec / shell boundaries rather than adding ad hoc Drive-specific behavior in the CLI surface.
- `workaholic:implementation` / `policies/coding-standards.md` — failures must be structured and name the actual unsupported operation or parsing cause.
- `workaholic:implementation` / `policies/test.md` — add regression coverage for local-to-Drive copy planning and commit behavior, including Markdown payloads with apostrophes.
- `workaholic:operation` / `policies/ci-cd.md` — the regression should be reproducible locally without depending on an untracked operator session where possible.

## Key Files

- `packages/qfs/crates/cmd/src/lib.rs` - one-shot and interactive command surfaces; real commit wiring.
- `packages/qfs/crates/exec/src/lib.rs` - plan construction, preview, and commit execution path.
- `packages/qfs/crates/qfs/src/commit.rs` - effect application / driver registry routing.
- `packages/qfs/crates/driver-gdrive/` - Google Drive blob namespace driver and write applier.
- `packages/qfs/crates/driver-local/` and/or `packages/qfs/crates/driver-fs/` - local file read side of cross-driver copy.
- `packages/qfs/crates/skill/assets/SKILL.md` and `plugins/qfs/skills/qfs-gdrive/SKILL.md` - documented Drive write behavior.

## Related History

The repository docs already describe write-source materialization at the commit boundary and call out cross-driver blob copy as a boundary that must be explicit. This bug appears to sit exactly on that boundary.

- `docs/blueprint.md` § "Write-source materialization at the commit boundary" - states that write sources should materialize at commit time and that genuine driver read effects remain distinct.
- `packages/qfs/crates/skill/assets/SKILL.md` § "drive — blob_namespace" - documents one-shot `UPSERT INTO /drive/... values ('report-bytes')` and says `cp` is interactive-shell-only sugar.

## Reproduction Notes

Environment where observed:

- Repository: a separate local repository holding generated Markdown reports
- Drive mount: `/drive -> gdrive (a@qmu.jp)`
- Source file: `/home/user/reports/model-comparison.md`
- Destination attempted: `/drive/my/model-comparison.en.md`

Observed cases:

1. One-shot literal upload of the Japanese Markdown report succeeded after running outside the sandbox:

   ```sh
   node -e "... read reports/model-comparison.ja.md ..." | qfs run - --commit --format table
   ```

   Result: `COMMITTED`, and `/drive/my |> where name == 'model-comparison.ja.md'` returned the uploaded file.

2. One-shot literal upload of the English Markdown report failed during parsing / command interpretation even after escaping apostrophes with backslashes. One observed failure:

   ```text
   error[usage]: relative path `a` is not allowed in one-shot mode; use an absolute path (`/driver/...`) or an `id:` form (at a)
   ```

   This suggests the statement body escaped out of the string literal and qfs parsed report prose as query syntax. The English report contains Markdown text such as `provider's`, `Bessel's`, quoted `'none'`, and `isn't`.

3. Attempting to avoid literal embedding with a local-source `drive.copy` call previewed but failed at commit:

   ```sh
   qfs run "/local/home/user/reports/model-comparison.md |> call drive.copy(parent_path => '/drive/my', name => 'model-comparison.en.md')" --commit --format table
   ```

   Actual:

   ```text
   error[commit_failed]: Terminal { reason: "CALL drive.copy is not supported by the local FS driver" }
   ```

   This may be correct if `drive.copy` is same-Drive only, but the preview should make that boundary clear or reject the plan earlier.

4. Interactive shell `cp` produced a local-read plus Drive-upsert plan and reported committed, but no destination file appeared in Drive:

   ```sh
   printf 'cp /local/home/user/reports/model-comparison.md /drive/my/model-comparison.en.md\nCOMMIT\n' | qfs
   ```

   Actual output:

   ```text
   PREVIEW: 2 effect(s)
     #0 READ -> local:/local/home/user/reports/model-comparison.md [affected ?]
     #1 UPSERT -> drive:/drive/my/model-comparison.en.md [affected ?]
     total affected: ?
   type COMMIT to apply
   COMMITTED (1 effect plan(s)):
   COMMITTED:
   PREVIEW: 2 effect(s)
     #0 READ -> local:/local/home/user/reports/model-comparison.md [affected ?]
     #1 UPSERT -> drive:/drive/my/model-comparison.en.md [affected ?]
     total affected: ?
   ```

   Verification immediately afterward:

   ```sh
   qfs run "/drive/my |> where name == 'model-comparison.en.md' |> select name, mime_type, size, modified_time" --format table
   ```

   returned zero rows. A root listing of `/drive/my` also did not show the file.

## Implementation Steps

1. Reproduce the three write paths with a local Markdown fixture that contains apostrophes, backticks, Japanese text, and long tables.
2. Decide and document the supported contract for local-to-Drive copy:
   - either interactive `cp local -> drive` must materialize the local bytes at commit and apply a Drive UPSERT, or
   - it must fail at preview time with a structured unsupported-cross-driver-copy error naming the supported alternative.
3. Fix one-shot literal parsing or document the exact escape grammar. If literal upload remains supported, add a regression proving apostrophes and backslashes inside Markdown do not escape into query syntax.
4. Fix commit reporting so `COMMITTED` is only printed after the Drive object is actually created or replaced. A failed Drive apply must return nonzero and include the underlying driver error.
5. Add a regression test around the interactive `cp` lowering path. The assertion should cover both the effect plan and the post-commit observable file listing/read.
6. Update qfs Drive skill/docs so AI agents know the reliable way to upload a local file to Drive. Avoid requiring agents to embed large file bodies in qfs string literals when a byte-copy path exists.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- A local Markdown file containing apostrophes, backticks, non-ASCII text, and tables can be copied to `/drive/my/<name>.md` through a documented qfs path, then read/listed from `/drive/my` with the expected name, MIME type, and nonzero size.
- If same-Drive `drive.copy` cannot accept a local source, qfs rejects that plan at preview or commit with a structured error that names the source driver and the unsupported procedure boundary.
- Interactive `cp local -> drive` does not report `COMMITTED` unless the destination object exists afterward.
- One-shot Drive `UPSERT ... values (...)` either has documented escaping that handles apostrophes/backslashes or returns a structured parse error that points at the string-literal issue without parsing report prose as paths.

**Verification method** — the commands/tests/probes that prove them:

- Add hermetic parser / lowering tests for Markdown string literals with apostrophes and backslashes.
- Add an integration test for `cp /local/... /drive/...` using the available fake or fixture Drive applier. If live Google Drive is required, mark that verification as live-only and keep a hermetic test for plan construction and failure reporting.
- Run the relevant qfs test subset, expected to include `cargo test -p qfs-cmd`, `cargo test -p qfs-exec`, and the driver-gdrive test package if separate.
- Manually verify with a connected Drive account only if no fixture applier exists: upload a Markdown fixture, list `/drive/my`, read the uploaded blob metadata, and remove the test object afterward.

**Gate** — what must pass before approval:

- Regression tests prove the supported upload path and prevent false-positive `COMMITTED` output.
- The qfs Drive skill/docs state the supported local-file-to-Drive upload command and the same-Drive-only boundary for `drive.copy`, if that boundary remains.

## Considerations

- Preview without credential binding is useful, but commit must not reuse a pure preview path that skips the live Drive applier.
- Avoid broadening `CALL drive.copy` to mean cross-driver copy unless the procedure contract explicitly models a local byte source. A generic blob copy lowering may be the cleaner abstraction.
- The operator-facing failure should distinguish parser/string-literal errors from path resolution errors; `relative path 'a'` is misleading when the input was report prose that escaped a string literal.
- Keep docs and skill assets aligned so agents stop attempting fragile giant-literal uploads when the correct primitive is file-byte copy.

## Final Report

Development completed around the supported interactive `cp local -> drive` path. The production REPL
now injects the real world applier for `COMMIT`, and shell commits materialize pipeline/read sources
through the same commit-boundary materialization helper used by one-shot execution before handing
the plan to the live applier. The Drive cookbook and qfs Drive skill now steer agents toward
interactive `cp` for local file uploads instead of giant string literals.

### Discovered Insights

- **Insight**: The false-positive `COMMITTED` output came from the shell using the in-memory
  recorder path while the one-shot CLI used the binary's live apply registry.
  **Context**: Future interactive shell commit features should keep the production shell as a
  composition root that injects real I/O, while tests continue to omit that hook for hermetic plan
  assertions.
