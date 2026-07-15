---
type: Concern
concern_id: the-branch-safety-scanner-false-positives
mission:
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

# The branch-safety scanner false-positives on Rust `Token::Variant`, hard-blocking `/ship`

## Description

`scan-branch-safety.sh` returns `block` with 7 `secret`/`credential` findings on this branch, all false positives — five are **doc comments** (`/// \`Token::Path\` there fails`). The cause is in the workaholic plugin's `skills/release-scan/scripts/lib/secret-patterns.sh`: `_SP_KEY` (line 46) matches the bare word `token`, then `[:=]` binds to the *first* colon of Rust's `::` and `[^[:space:]]{6,}` swallows the variant name, so every `Token::Variant` in a parser reads as `token=<secret>`. Reproduced in isolation; the rule still matches real assignments, so it is a precision bug, not a broken rule (flagged lines rode in on [ba06534](https://github.com/qmu/qfs/commit/ba06534) in `packages/qfs/crates/parser/`).

## How to Fix

In the **workaholic plugin repo** (not qfs), subtract the scope-resolution operator in pass 2 — e.g. exclude `"${_SP_KEY}[[:space:]]*::"` — alongside the existing reference-shaped exclusions. Since the `secret` tier is non-overridable by design in `gate-decision.sh`, `/ship` will hard-block this branch until that lands.
