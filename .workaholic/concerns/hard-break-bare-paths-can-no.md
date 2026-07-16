---
type: Concern
concern_id: hard-break-bare-paths-can-no
mission: [declared-drivers-are-the-normal-way-to-add-a-service]
tickets: [20260715190000-resume-development-in-the-new-public-repo.md, 20260716005029-unify-the-qfs-statement-splitter.md, 20260716120200-reinstall-replaces-a-declaration.md]
origin_pr: 1
origin_pr_url: https://github.com/qmu/qfs/pull/1
origin_branch: work-20260715-205333
origin_commit: ddb419e
created_at: 2026-07-16T15:16:32+09:00
first_seen: 2026-07-16T15:16:32+09:00
last_seen: 2026-07-16T15:16:32+09:00
severity: moderate
status: active
resolved_by_pr:
resolved_by_commit:
---

# Hard break: bare paths can no longer carry a literal semicolon

## Description

Commit [0afaf2b](https://github.com/qmu/qfs/commit/0afaf2b) added `;` to the lexer's path-delimiter set in `lex.rs` to fix the splitter's root cause. A bare path that previously absorbed a `;` now ends at it, consistent with `#` and `,` already in the set. Deliberate, versioned hard break (crate 0.0.72, plugin 0.11.9); the prior behavior was a silent shipped bug

## How to Fix

Any `.qfs` file that relies on a literal `;` inside an unquoted path must quote the locator; nothing else to do — the break is intended
