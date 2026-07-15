---
created_at: 2026-07-06T12:06:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort: 2h
commit_hash: 67afe98
category: Added
depends_on: []
---

# Filter-based REMOVE on /sys/accounts (WHERE account == '<email>') for Google-email accounts

## What's wanted

A Google account whose label is an email cannot be removed by a `REMOVE /sys/accounts/google/<email>`
path, because `@` is a path version coordinate — the lexer binds `a@qmu.jp` as segment `a` +
version `qmu.jp` and `render_path` drops the version, so the applier receives the truncated
`.../google/a`. Support a filter-based remove: `REMOVE /sys/accounts WHERE account == '<email>'`,
where the email rides safely inside a string literal. (Path-safe labels like `github/work` already
remove cleanly; `rotate`/`revoke` stay CLI-only by rule since they need a new secret value.)

## Current state (verified this session)

- `/sys` removes are path-addressed; `EffectNode` carries no filter predicate. The `@`-drop is in
  `crates/core/src/eval.rs:1058` (`render_path`).

## Implemented (simpler than the sketch: no EffectNode / eval change)

The sketch feared `EffectNode` had to grow a filter. It does not: a `REMOVE … WHERE col == const`
already lowers each equality into the effect's single-row payload via `setwhere_row_batch`, so
`REMOVE /sys/accounts WHERE account == 'a@qmu.jp'` reaches the applier with `account = 'a@qmu.jp'`
in `node.args` — the email intact (a string literal, never a path segment). The whole change is
applier-side:

1. `crates/driver-sys/src/applier.rs` — the `(Remove, Accounts)` branch: path-addressed
   (`/sys/accounts/<p>/<a>`) as before, else filter-addressed — read `account` (required) and
   `provider` (optional) from `node.args`.
2. Provider resolution when `provider` is omitted: `backend.scan(SysNode::Accounts)` (its collapsed
   view keys a Google account by its email) → the single matching provider; zero / ambiguous
   matches are honest, secret-free `MalformedEffect` rejections (never a wrong-row delete).
3. Reuses the existing `backend.remove_account(provider, account)` (which dispatches google →
   `remove_google`), so **no `SysBackend` trait change**. Capability already grants `Remove` on the
   `Accounts` node, so the bare `/sys/accounts` REMOVE passes the parse-time gate.

## Key files

- `crates/driver-sys/src/applier.rs` (the whole change + tests). Unchanged but relied on:
  `crates/core/src/eval.rs::setwhere_row_batch` (already carries the WHERE row),
  `crates/qfs/src/account.rs::remove_account` (google dispatch).

## Considerations

- This is edge 2 of concern 22 — a scoped evaluator/applier extension, not a one-liner.
- Edge 1 (SECRET reference resolution for accounts) is deliberately NOT ticketed: it needs an owner
  design decision first (no bind-time `env:`/`vault:` resolution for accounts exists today).
- Source concern: `.workaholic/concerns/22-create-account-secret-clause-and-google-email-remove.md`.
