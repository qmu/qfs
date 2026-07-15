---
type: Concern
concern_id: the-interactive-shell-s-local-reads
mission: language-design-review-layering-principles-and-semantic-gaps
tickets: [20260713195008-effect-selector-channel-folder-rename.md, 20260714120000-effect-selector-uniform-migration.md, 20260714154144-general-of-type-assertion.md, 20260714182710-shell-face-slice1-ls-cat-describe-typed.md, 20260714182720-shell-face-slice2-cd-gate-enumerable-children.md, 20260714182730-shell-face-slice3-mutation-verbs-per-kind.md, 20260714182740-shell-face-type-mount-and-describe-builtin.md, 20260714220213-resume-shell-face-slices-and-report.md]
origin_pr: 41
origin_pr_url: https://github.com/qmu/qfs/pull/41
origin_branch: work-20260714-111817
origin_commit: 7752cb3
created_at: 2026-07-15T16:35:34+09:00
first_seen: 2026-07-15T16:35:34+09:00
last_seen: 2026-07-15T16:35:34+09:00
severity: moderate
status: active
resolved_by_pr:
resolved_by_commit:
---

# The interactive shell's `/local` reads from the cwd but writes to the filesystem root

## Description

`shell.rs` roots the REPL's `/local` READ mount at `current_dir()`, while `commit.rs:246` roots the apply driver at `/`. A REPL `mv a.md b.md` therefore previews against `$CWD/a.md` and then commits into `/b.md` — observed as `PermissionDenied`, but it would **succeed as root**. Pre-existing and untouched by this branch (the blob→blob lowering is byte-identical), found only by driving a real COMMIT rather than trusting the preview (see [fc99572](https://github.com/qmu/qfs/commit/fc99572) in `packages/qfs/crates/qfs/src/shell.rs` and `commit.rs`).

## How to Fix

Root the commit-side local applier at the same directory the shell's read mount uses (or root both at `/` and make the shell's cwd purely a prompt-level convenience). It means no `cp`/`mv` COMMIT in the REPL has ever worked as the operator reads it, so it deserves its own ticket rather than a drive-by fix.
