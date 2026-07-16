---
type: Concern
mission: declared-drivers-are-the-normal-way-to-add-a-service
tickets: [20260712005100-chatwork-declared-live-read-empty-columns.md]
origin_pr: 34
origin_pr_url: https://github.com/qmu/qfs/pull/34
origin_branch: work-20260712-015928
origin_commit: 497ff5e
created_at: 2026-07-12T02:45:19+09:00
last_seen: 2026-07-15T16:35:34+09:00
first_seen: 2026-07-12T02:45:19+09:00
concern_id: duplicate-declaration-rows-still-resolve-oldest
severity: low
status: active
resolved_by_pr: 
resolved_by_commit: 
---

# Duplicate declaration rows still resolve oldest-first outside the type lookup

## Description

Repeated `qfs run -f <driver>.qfs` installs append `sys_drivers` rows rather than replacing them. PR #34 switched only the `type` lookup to newest-wins (`types_from_conn` orders `id DESC`, so a re-install heals a stale type row). Duplicate `driver` and `view`/`map` rows from re-installs still resolve oldest-first in their own lookups: `assemble` seeds one `DeclaredDriver` per `driver` row (duplica… (`duplicate-declaration-rows-still-resolve-oldest.md`, origin `497ff5e`)

## How to Fix

Apply the same newest-wins ordering — or better, a replace-on-install semantic for same-name declaration rows — to the driver/view/map lookups in `declared_driver.rs::assemble` and the view template matching, so a re-install consistently heals every row kind.

