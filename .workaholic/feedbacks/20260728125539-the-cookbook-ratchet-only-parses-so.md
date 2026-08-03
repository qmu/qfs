---
type: Feedback
title: The cookbook ratchet only parses, so it cannot catch a fabricated column
kind: concern
source: development
created_at: 2026-07-28T12:55:39+09:00
author: a@qmu.jp
supersedes:
severity: moderate
concern_id: the-cookbook-ratchet-only-parses-so
owner: a@qmu.jp
mission: [a-where-predicate-is-honored-or-refused-never-dropped]
tickets: [20260717180100-where-on-an-unknown-column-returns-zero-rows-at-exit-0.md, 20260717180200-expand-silently-no-ops-on-json-and-unknown-columns.md, 20260717180300-codec-source-error-names-the-pre-decode-columns.md, 20260723020055-gdrive-where-pushdown-silent-drop.md]
origin_pr: 26
origin_pr_url: https://github.com/qmu/qfs/pull/26
origin_branch: work-20260724-011029
origin_commit: ee5af0f
last_seen: 2026-07-28T12:55:39+09:00
---

# The cookbook ratchet only parses, so it cannot catch a fabricated column

## Description

`crates/test/tests/cookbook_skills.rs` asserts every cookbook recipe **parses** and nothing more, which is exactly why six `/github` recipes naming columns the driver does not carry shipped green — and why this branch's law 1 turned them into exit-2 refusals in an article mirrored verbatim into the installed skill (see [c8ae248](https://github.com/qmu/qfs/commit/c8ae248) in `docs/cookbook/github.md`). The ratchet's promise is that a skill can never teach a statement the binary rejects; today it only guarantees the statement is well-formed.

## How to Fix

Raise the ratchet from parse-only to a typecheck against the compiled describe registry, with an explicit skip report so a silent skip cannot read as green. Ticketed at `20260725143000-cookbook-ratchet-only-parses-it-must-typecheck.md`.
