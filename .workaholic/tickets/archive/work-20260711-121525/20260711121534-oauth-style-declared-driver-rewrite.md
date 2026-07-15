---
created_at: 2026-07-11T12:15:34+09:00
author: a@qmu.jp
type: enhancement
layer: [Infrastructure, Domain]
effort:
commit_hash:
category:
depends_on:
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
---

# OAuth-style API as a declared driver — extend the declared model past API keys

## Overview

Prove the mission's second driver-rewrite variant: a service whose auth is **OAuth (bearer token
with refresh)**, expressed as a declared qfs-query driver. API-key-style is proven (Cloudflare
shipped; Chatwork extends it); the OAuth variant is the open question because the declared
`AUTH` clause today models a static header/token, while OAuth needs token acquisition/refresh —
which qfs already owns *elsewhere* (the compiled Google/Slack account flows and the vault). The
disciplined shape: the declared driver names an **account reference** (`AUTH ACCOUNT google`
style — exact form to rule), and the evaluator injects the live bearer token from the existing
account/vault machinery at wire time; the declaration itself stays credential-free. Prove it by
rewriting one real OAuth surface already served compiled (candidate: a read-only slice of GitHub
or Google Calendar-class API — pick the smallest real one) as a declaration, side-by-side with
the compiled driver.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout (applies to all code work)
- `workaholic:implementation` / `policies/coding-standards.md` — style conventions (applies to all code work)
- `workaholic:design` / `policies/vendor-neutrality.md` — the declared model is the anti-corruption boundary; OAuth support must be a generic AUTH capability, not per-service code
- `workaholic:safety` / `policies/standard.md` — tokens stay in the vault/account store; the declaration carries only the account reference
- `workaholic:design` / `policies/access-control.md` — the account reference scopes what the declared driver may reach; no ambient credential pickup

## Key Files

- `packages/qfs/crates/qfs/src/declared_driver.rs` - AUTH handling in DeclaredDriver → RestApiConfig (where the account-reference form lands)
- `packages/qfs/crates/qfs/src/declared_eval.rs` - wire-time evaluation; token injection point
- `packages/qfs/crates/qfs/src/sys.rs` - accounts/vault surface the token comes from
- `packages/qfs/crates/skill/assets/examples/cloudflare.qfs` - the API-key precedent whose shape the OAuth asset mirrors

## Related History

- [20260704145138-driver-conformance-and-first-conversion.md](.workaholic/tickets/archive/work-20260705-032203/20260704145138-driver-conformance-and-first-conversion.md) - the conversion ratchet (Slack bearer was the first proof)
- [20260708023259-cloudflare-declared-driver-query-based.md](.workaholic/tickets/archive/work-20260707-181519/20260708023259-cloudflare-declared-driver-query-based.md) - shipped API-key declared driver

## Implementation Steps

1. **Rule the AUTH form first**: extend the declared-driver grammar/blueprint with an account-referencing auth clause (`AUTH ACCOUNT <provider>` or similar), semantics = evaluator resolves the account's live bearer token (running refresh via the existing compiled account machinery) at wire time. Record considered alternatives.
2. Implement the evaluator support: token resolution injected where header auth is applied today; a missing/expired account fails closed with a structured error, never a silent unauthenticated call.
3. Author the proof asset: one real OAuth service slice (smallest real candidate served by an existing account type) as CREATE DRIVER/TYPE/VIEW declarations, host-confined.
4. Hermetic tests: parse + install + DESCRIBE credential-free; wire-time mock asserting the bearer header is injected from the account store and never stored in /sys/drivers rows.
5. Cookbook + docs + gen-skills; plugin version bump if taught surface changes.

## Quality Gate

**Acceptance criteria**

- The OAuth declared asset installs and DESCRIBEs credential-free; wire-time calls carry the account's bearer token (mock-asserted).
- /sys/drivers rows and dumps contain no token material.
- A missing account fails closed with a structured error naming the account reference.

**Verification method**

- `cargo test --workspace` green (grammar, evaluator, secret-free assertions); `gen-docs --check` / `gen-skills --check` clean.

**Gate**

- Hermetic suite green is the /drive approval gate; the live round (real OAuth account, real read through the declared driver) runs owner-attended and is recorded on this ticket.

## Considerations

- Token refresh mid-statement: rule whether the evaluator refreshes eagerly per statement or retries once on 401 (`packages/qfs/crates/qfs/src/declared_eval.rs`)
- Keep the AUTH clause additive (registry growth = minor) — the API-key form must keep parsing unchanged (`docs/blueprint.md` versioning rule)

## Live Round Evidence

### Round 8 — declared /ghdecl read via AUTH ACCOUNT (2026-07-13, owner-attended, PASSED)

- **Binary:** qfs 0.0.59 (c30fa0a). The gh CLI's existing token was stored as the qfs account
  (`gh auth token | qfs account add github gh`), all 5 statements of the shipped
  `github_account.qfs` installed (driver + 2 types + 2 views, each one previewed `/sys/drivers`
  INSERT), and the mount connected with `qfs connect /ghdecl --driver ghdecl --account gh`.
- **The proof:** `/ghdecl/user/repos |> select name, full_name, private |> limit 5` returned
  5 typed rows of the token's real repositories — **private repos included**, so the
  `AUTH ACCOUNT 'github'` bearer injection worked at wire time (the declaration itself carries
  only the provider name; the token stayed in the vault) — and GitHub accepted the request, which
  live-proves the PR #35 `User-Agent: qfs/<version>` default header (GitHub refuses agent-less
  requests).
- **Parameterized view:** `/ghdecl/repos/qmu/qfs/pulls` resolved its `{owner}/{repo}` path
  parameters and returned an empty set (no open PRs at the time — correct).
- **Doc drift noted:** the asset header's install hint `qfs connect /ghdecl TO ghdecl ACCOUNT
  '<label>'` is not the CLI's actual syntax (`--driver/--account` flags); the in-language CONNECT
  form is what the TO/ACCOUNT shape belongs to. Worth a one-line asset comment fix on the next
  touch.
- **Ticks acceptance:** OAuth-style declared driver proven end-to-end (read leg) — the mission's
  "Drivers rewritten as qfs query declarations: an OAuth-style API" item.
