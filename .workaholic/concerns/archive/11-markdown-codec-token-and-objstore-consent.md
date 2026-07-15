---
origin_pr: 11
origin_pr_url: https://github.com/qmu/qfs/pull/11
origin_branch: work-20260629-110121
origin_commit: 3c6f995
created_at: 2026-07-02T01:21:00+09:00
last_seen: 2026-07-02T01:21:00+09:00
first_seen: 2026-07-02T01:21:00+09:00
concern_id: markdown-codec-token-and-objstore-consent
severity: low
status: resolved
resolved_by_pr: 12
resolved_by_commit: 0945382
---

# Markdown codec token and objstore consent-gate reconciliation

## Description

The markdown codec now resolves as `md` ([69fd0c8]); separately, the `CLOUD_DRIVERS` consent set lists `objstore` while the driver ids are `s3`/`r2`, so the bind gate is effectively off for s3/r2 ([cf08355]) — worth reconciling so the consent gate matches the real driver ids.

## How to Fix

Align the `CLOUD_DRIVERS` consent set with the actual `s3`/`r2` driver ids so the bind gate governs object-storage reads consistently.

## Resolution (verified 2026-07-06, HEAD 61f696c)

Both halves resolved and no longer reproduce:

- **Consent gate** — commit `0945382` (PR #12) rewired every objstore bind/read/apply site to key on
  a single `OBJSTORE_PROVIDER = "objstore"` constant (`crates/qfs/src/commit.rs:483,499-501`), which
  **is** a member of `CLOUD_DRIVERS` (`crates/secrets/src/consent.rs:35`). `bind_gate` now enforces
  sign-in + consent for both `/s3` and `/r2`; the gate is no longer keyed on the mount-kind strings
  `s3`/`r2` that were absent from the set.
- **Markdown codec** — resolves as `md` (`crates/codec/src/codecs/markdown.rs:32`), asserted by the
  hermetic `codec_registry_with_builtins_resolves_all_six` test.

Nothing left to do; archived.
