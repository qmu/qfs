# Coding Review — Architect — t20 (Gmail driver)

- Reviewer: Architect (Neutral / structural)
- Artifacts reviewed: `crates/driver-gmail/src/{lib,path,schema,query,mime,client,effect,applier,error}.rs`, `crates/driver-gmail/Cargo.toml`, `crates/driver-gmail/src/tests.rs`, `crates/cmd/tests/dep_direction.rs`, `ARCHITECTURE.md`
- Commit: `7162519` on `work-20260622-230954`
- QA domain: analytical review only (no cargo/test execution)

## Decision

**Request revision** — one real pushdown-correctness defect with a small, local fix
(`query.rs`: lossy `Eq`/`LIKE` terms are pushed to the Gmail `q=` while the residual is
dropped to `None`, so the engine never re-checks them; Gmail's `from:`/`to:`/`subject:`
matching is looser than SQL `=`, so over-fetched rows are returned as if exact). This is
exactly the "returns wrong rows by pushing a partially-translated predicate" class. Token
safety, MIME, multi-account, and the spine are all sound — the rest of the driver is strong.

## The defect — semantic-narrowing pushdown drops its residual (`query.rs`)

`lower` / `lower_cmp` push a Gmail search term **and return `None` residual** for:

- `from = 'v'` → `from:v` (lines 99–101, residual dropped at line 77)
- `to   = 'v'` → `to:v`
- `subject = 'v'` → `subject:v`
- `LIKE` on `from`/`to`/`subject` → same (lines 81–86)

The module doc-comment asserts the safety invariant explicitly (lines 38–42): *"a term is
emitted only when Gmail can express it **exactly**, so a residual is always re-checked
locally and the result set is never wrong."* That invariant is **violated** for these arms,
because Gmail's field operators are not exact equality on the header value:

- `from:x@y` is an **address/substring** match, while SQL `from = 'x@y'` is **exact string
  equality** on the `From` header (typically `"Alice <x@y>"`). Two divergences, both
  unfiltered once the residual is `None`:
  - **Over-fetch**: `from:bob` also matches `bob@other.com`; those rows are returned though
    `from = 'bob'` is false for them.
  - **Under-/mis-fetch vs. exact**: a row whose `From` header is `"Alice <x@y>"` is returned
    by `from:x@y` but is *not* equal to `'x@y'` under `=`; with the residual dropped it is
    emitted anyway.

So `FROM /mail/INBOX |> WHERE from = 'x@y'` can return rows that do not satisfy the
predicate. `test where_lowers_to_gmail_query_with_residual_kept_local` asserts
`residual.is_none()` for the all-`Eq` conjunction — it pins the **buggy** behaviour as
intended, which is why this slipped through. The `subject = 'hello'` substring case is the
clearest miss (`subject:hello` matches `"hello world"`).

A second, narrower instance: `date > <ms>` → `after:<ms/1000>` (lines 109–110) truncates to
second granularity (and Gmail `after:`/`before:` are date-granular in practice), so the bound
is approximate; with the residual dropped, boundary rows leak. Lossy in the same way.

### Fix (small, local, no new dependency)

For the lossy operators, **push the Gmail term as a pre-filter AND keep the predicate as
residual** so the engine re-checks it locally — push for cheapness, residual for correctness:

```rust
// from/to/subject Eq and LIKE, and the date bounds: push the q= term for the backend
// pre-filter, but return Some(p.clone()) so the engine re-applies the exact predicate.
(f @ ("from" | "to" | "subject"), CmpOp::Eq, Literal::Text(v)) => { terms.push(...); /* still residual */ }
```

Concretely, change those arms (and the `LIKE` arm, and the `date` bound arms) to push the
term into `terms` but return `Some(p.clone())` rather than `None`. Genuinely exact mappings
keep returning `None`: `label = 'INBOX'` → `label:INBOX` (exact label-id), `is_unread = b`
→ `is:unread`/`is:read` (exact), and `CmpOp::Match` (a regex match maps to the same loose
operator — `Match` already accepts looseness, so it may stay `None` only if `Match`'s
contract is "backend-defined substring"; otherwise treat it like `Eq`). Then flip the test
expectation: the all-`Eq` conjunction should now report `label:... from:... subject:...
is:unread` as the pushed query **with a residual that re-checks `from`/`subject`** (only the
exact `label`/`is_unread` terms drop out). This preserves the over-fetch-then-filter
contract the doc-comment already promises.

### Coherence note (not blocking, surfaced for the lead)

I could not find a residual-application stage in `cfs-runtime`/`cfs-plan` (`grep residual`
is empty there). So even a *correctly reported* residual may not yet be filtered locally at
this layer. That is a wiring gap to confirm separately (t10 SELECT path), but it does not
change the driver-level obligation: the driver must report the residual **truthfully**.
Reporting `None` for a lossy push is the structural lie regardless of whether the consumer
filters today — and the moment the consumer does filter, a truthful residual is correct for
free. The fix is therefore right independent of the engine state.

## What is sound (the strong majority)

**Reuse of the locked t19 seams — clean.** `GoogleApiGmailClient` wraps `Arc<GoogleApiClient>`
and calls `self.api.send(req)` (client.rs 118–119); bearer injection + refresh-on-401 are
inherited, not reimplemented. Requests are `cfs_http_core::HttpRequest` with **no**
`Authorization` header set by this crate — the auth base injects it. `Cargo.toml` carries
`cfs-google-auth` + `cfs-http-core` and **no `reqwest`**, so HTTP/redaction are single-sourced
and reqwest stays confined to `cfs-driver-http`. `dep_direction.rs` appends `cfs-driver-gmail`
to the runtime-consumer allowlist (line 326) and the generic leaf check (b) proves nothing
depends back onto it — gmail is a genuine runtime leaf; tokio cannot transit back into the
spine. The append composes with the generic rule exactly as the test's own comment promises.

**Token safety — preserved by construction.** No token type appears in this crate; the bearer
lives behind `cfs_secrets::Secret` upstream. `From<AuthError>` keeps only the stable `code` +
`reauthorize` flag (error.rs 123–132); every `GmailError` arm carries a path / verb / status /
fixed reason, never a body or header. `errors_are_secret_free` asserts no `Bearer`/`ya29` text.
The `RecordedCall` mock seam is secret-free by construction (no token ever enters it).

**Contract fit — capabilities and irreversibility are right.** Path-keyed caps (lib.rs
141–155) match the ticket: drafts = Insert|Upsert|Select|Remove, label = Select|Update|Remove,
message = Select|Remove, thread = Remove. REMOVE = trash (message/thread), and **permanent
delete is simply absent from the trait** (`messages.delete` is never a method) — the right
safety default: blast radius is bounded by what the API surface can express, not just by a
runtime check. `mail.send` is `irreversible(true)` + `requires_scopes([compose])`; the `SEND`
prelude desugars to `mail.send`. The send recovery path (applier.rs 73–87) creates a draft
then sends by id, so a mid-send crash leaves a recoverable draft — the RFD §6 de-dupe story
is honoured, and `PREVIEW` is proven to touch the client zero times.

**N+1 shape — correct.** `search_message_ids` returns ids only (`MessageIdPage`); the per-id
`get_message` is a separate call. The detail fetch is therefore expressible as independent
leaves for the t10 interpreter to batch, not an inline loop — the RFD §6 example. The seam is
the right shape; the actual leaf-emission lives in the t10 SELECT path (out of this crate).

**MIME builder — pure, deterministic, correct.** CRLF throughout, RFC 2047 base64 subject only
when non-ASCII, `multipart/mixed` with a fixed boundary, per-attachment base64 + correct
`Content-Disposition`, base64url for the Gmail `raw` field. The self-written base64 core
(mime.rs 151–172) is correct: the `chunks(3)` + `get(1/2).unwrap_or(&0)` handles 1- and
2-byte tails with the right `=` padding, no off-by-one; `wrap76` slices ASCII base64 safely.
Golden test covers non-ASCII subject + two attachments + base64url alphabet. (Minor, non-
blocking: a very long non-ASCII subject becomes one unwrapped `=?UTF-8?B?...?=` exceeding the
RFC 2047 75-char encoded-word cap — fine for delivery, worth a follow-up.)

**Multi-account — genuine isolation.** One `GoogleApiClient` per account (account bound at
client construction); the driver is account-agnostic, holding only `Arc<dyn GmailClient>`.
`multi_account_selects_independent_clients` proves an op on driver A records only on mock A
and leaves mock B untouched. Isolation is structural (separate client instances), not a
runtime filter — the right design.

**No vendor leak — enforced.** Gmail JSON is decoded to owned DTOs at the client boundary
(`decode_message`/`decode_attachments`); the `Driver` surface and the `Plan` carry zero google
types, behind the mockable `GmailClient`. `internalDate` → epoch-ms `date` matches the schema's
`Timestamp` contract.

## Honesty of the parks — accurate

- **Attachment bytes**: `MailMessage.attachments` carries metadata only; bytes "fetched on
  demand" (schema.rs 13–15). Honest — but the on-demand fetch path itself is **not implemented**
  in this crate (no `get_attachment` on the `GmailClient` trait, despite implementation-step 2
  listing it and the `Attachment {bytes}` read path being described). The `MailPath::Attachment`
  parse + `Select` capability exist with no client method behind them. This is a real,
  acceptable park for t20, but it should be named explicitly as deferred rather than implied
  present — recommend a one-line note in the crate doc / ARCHITECTURE row.
- **historyId / @version sync**: `VersionSupport::None` with an inline pointer to the E7
  trigger sibling (lib.rs 185–188). Honest and correctly scoped.
- **Live smoke test**: the ticket's env-gated create→send→trash smoke test is **not present**.
  Acceptance criterion list (ticket line 169) calls for one opt-in test; the suite is mock-only.
  Defensible for an analytical/internal pass (Planner's E2E domain), but it is a missing
  acceptance item that should be tracked, not silently dropped.

## ARCHITECTURE.md — missing the `/mail` crate row

ARCHITECTURE.md documents t19 (`google-auth`) but has **no `cfs-driver-gmail` / `/mail` row**
in the crate table — the new crate is undocumented in the structural map. Add a row mirroring
the `google-auth` entry (mount `/mail`, Append/log archetype, path-keyed caps, runtime leaf,
least-privilege scopes, no-vendor-leak) so the structural ledger stays in sync with the spine.
Non-blocking but should land with the revision.

## Summary

Token safety, capability/irreversibility contract, MIME, multi-account isolation, no-vendor-
leak, and the runtime-leaf spine are all correct and well-tested. The single blocking issue is
the `query.rs` residual-dropping on lossy `from`/`to`/`subject` `Eq`/`LIKE` (and the `date`
bounds): it can return rows that violate the WHERE predicate, contradicting the module's own
stated over-fetch-then-filter invariant. The fix is local — keep those predicates as residual
while still pushing the `q=` pre-filter — and one test expectation flips with it. Address that,
add the attachment-bytes / smoke-test parks explicitly and the ARCHITECTURE row, and this is an
approve.
