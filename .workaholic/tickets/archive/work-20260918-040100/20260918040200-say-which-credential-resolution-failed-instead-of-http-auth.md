---
created_at: 2026-09-18T04:02:00+09:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
feedback:
merge_policy:
verification_handoff: 
---

# Say which credential resolution failed instead of `http_auth`

## Overview

A declared-driver read whose credential cannot be resolved reports the caller's path as invalid
and names no cause. Measured live on 2026-09-18 with the encrypted credential store locked:

```
$ qfs run "/slack/team/channels |> where name == 'dev-qfs' |> select id, name"
{"error":{"code":"invalid_path","kind":"usage",
 "message":"invalid path \"/rest/slack/conversations.list\": http_auth"}}

$ qfs run "/mail/labels |> limit 1"
{"error":{"code":"invalid_path","kind":"usage",
 "message":"invalid path \"/mail/labels\": the encrypted credential store is locked — export
  QFS_PASSPHRASE (`read -rs QFS_PASSPHRASE && export QFS_PASSPHRASE`) or run on a terminal so
  qfs can prompt, then retry"}}
```

One locked vault, two surfaces, two different answers. The compiled cloud mount says what to do;
the declared mount says `http_auth` against a `/rest/…` path the caller never typed. Three
defects are stacked:

1. **The cause is destroyed.** `HttpError::Auth` carries the store's own secret-free code —
   `secret_locked`, `secret_not_found`, `secret_revoked`, `secret_backend` — and
   `read_http_error` replaces all four with the single label `http_auth`. Locked vault, absent
   account and revoked credential are three different operator actions and they arrive
   indistinguishable.
2. **The category is wrong.** `CfsError::InvalidPath` is *a virtual path was malformed at the
   driver boundary*; a credential that did not resolve is not that. It renders `kind: usage`,
   exit 2, so an agent's recovery branch rewrites its query — the one recovery that cannot work.
   `ErrorKind::Auth` and `ExitCode::Auth` (6) already exist for exactly this and no `CfsError`
   arm reaches them.
3. **The locked compiled mount is miscategorised too.** Its message is actionable but it is also
   returned as `InvalidPath` / `kind: usage` / exit 2, so a locked store is exit 2 on every
   surface qfs has.

This is the same class as `20260918022538` (that one restores the *evaluator*'s discarded error;
this one restores the *credential resolver*'s) and it is a separate seam: `read_http_error` and
the lazy cloud bind, not `run_body_ops`.

## Policies

- `workaholic:implementation` / `policies/error-handling.md` — a failure must say what failed;
  errors are structured for machine consumption (blueprint §6: parseable by an AI, not prose)
- `workaholic:implementation` / `policies/coding-standards.md` — style and structure conventions
- `workaholic:safety` — no credential value may reach an error string, a log line or a test

## Key Files

- `packages/qfs/crates/qfs/src/declared_driver.rs` — `read_http_error` (line 1616): the mapper
  that collapses every `HttpError` but `Application` into `InvalidPath { reason: code }`.
- `packages/qfs/crates/driver-http/src/applier.rs` — `inject_auth` (line 219): where the store's
  `SecretError::code()` becomes `HttpError::Auth { code }`. The detail exists; it is thrown away
  downstream, not here.
- `packages/qfs/crates/secrets/src/store.rs` — `SecretError::code` (line 87): the closed
  vocabulary the hint is chosen from.
- `packages/qfs/crates/driver/src/error.rs` — `CfsError`, `#[non_exhaustive]`. New arm goes here.
- `packages/qfs/crates/exec/src/error.rs` — `ExecError::from_qfs`: the `kind` / exit-code map.
  `ErrorKind::Auth` → `ExitCode::Auth` (6) is already defined and unreachable from `CfsError`.
- `packages/qfs/crates/qfs/src/shell.rs` — `LOCKED_STORE_HINT` (line 511) and the
  `LazyCloudReadDriver` bind failure (line 575) that returns it as `InvalidPath`.

## Implementation Steps

1. Add `CfsError::Auth { path: String, code: &'static str, hint: &'static str }` with code
   `auth_unresolved`: `path` is the path the CALLER addressed, `code` the store's own secret-free
   code, `hint` the action that clears it. No credential, no header, no URL with a token.
2. Map it in `ExecError::from_qfs` to `ErrorKind::Auth` (exit 6), attaching `path` and carrying
   `code` in `detail`.
3. Route `read_http_error`'s `HttpError::Auth` arm through it, choosing the hint from the store
   code: `secret_locked` → the existing `LOCKED_STORE_HINT`; `secret_not_found` → authorize the
   account and re-connect the mount; `secret_revoked` → rotate the account; anything else → a
   generic "credential could not be resolved" that still carries the code.
4. Route the `LazyCloudReadDriver` locked-store branch through it too, so a locked vault is exit 6
   on the compiled mounts as well. The *connect*-hint branch stays `InvalidPath` → capability
   (exit 3): an unconnected account is a capability denial, not an auth failure, and the e2e
   goldens pin it.
5. Bump the patch in `packages/qfs/crates/qfs/Cargo.toml`.

## Quality Gate

**Acceptance criteria**

- A declared-driver read with a locked store reports the locked-store hint and its store code,
  not `http_auth`, and not `code: invalid_path` / `kind: usage`.
- The four store codes produce four distinguishable, actionable messages.
- A locked store exits 6 (`auth`) on both the declared and the compiled read path; an
  unconnected account still exits 3 (`capability`).
- No credential material appears in any string added by this change.

**Verification method**

- `cd packages/qfs && cargo test --workspace`
- `cd packages/qfs && env -u XDG_CONFIG_HOME cargo test -p qfs --lib -- --test-threads=1`
- `cd packages/qfs && cargo clippy --workspace --all-targets -- -D warnings`
- `cd packages/qfs && cargo fmt --all --check`
- Live: with the store locked, `qfs run "/slack/<ws>/channels |> limit 1"` names the lock.

## Considerations

- **Why a new arm rather than a better string.** `InvalidPath.reason` is `&'static str` and
  cannot carry both the code and the hint, and any string still renders as a path error. qfs is
  experimental and takes hard breaks, so no shim for the old `http_auth` reason is wanted.
- **Overlap with `20260918022538`.** That ticket adds `CfsError::ViewBodyEval` in the same enum
  and the same `from_qfs` match. Expect a textual conflict in two files and nothing semantic:
  the arms are disjoint and neither changes the other's classification.
- **Do not widen `HttpError::Auth`.** It already carries the right code. The loss is entirely in
  the mapper.
