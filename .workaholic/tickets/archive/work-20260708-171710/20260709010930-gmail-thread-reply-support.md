---
created_at: 2026-07-09T01:09:30+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, Infrastructure]
effort:
commit_hash: 197c851
category: Added
depends_on:
mission:
---

# Add Gmail thread-reply support: draft/send a reply that lands in an existing thread

## Overview

qfs can create, update, and send **standalone** mail, but it cannot **reply into an existing
thread**. Two capabilities are missing (found in a 2026-07-08 review of the Gmail surface):

- **Add a draft under an existing received email** — a *reply draft* that belongs to the parent's
  thread (capability 3).
- **Send a reply into the thread** — the sent message threads under the parent, not a new standalone
  message (capability 5).

**Root cause (verified in source):** the compose DTO `MailDraft`
(`crates/driver-gmail/src/schema.rs:68`) carries only `id/to/cc/subject/body/attachments` — **no
thread linkage**; the MIME builder (`crates/driver-gmail/src/mime.rs:29`, header block `:37-45`)
emits To/Cc/Subject/MIME-Version but **no `In-Reply-To`/`References`**; and
`create_draft`/`upsert_draft`/`send_draft` (`crates/driver-gmail/src/client.rs:338/352/374`) POST
`{message:{raw}}` / `{id}` with **no `threadId`**. So every draft/send produces a new standalone
message. The read side already has the raw material — `MailMessage.thread_id` (`schema.rs:22`) is
populated and surfaced as a column — but the parent's RFC 5322 `Message-Id` header is **not**
extracted (`decode_message` reads only From/Subject, `client.rs:494-495`), so `References` cannot be
sourced from a message read yet.

**Settled design decisions (owner, 2026-07-08 — do not re-litigate):**

1. **A reply is addressed at the parent message node** (`id:<msg>` or `/mail/<label>/<msg>`), NOT via
   a `thread_id` column on a `/mail/drafts` write. The parent is the source of the pipeline; the
   applier resolves the thread from it.
2. **Full threading fidelity:** set the Gmail `message.threadId` **and** emit `In-Reply-To` +
   `References` headers sourced from the parent's `Message-Id` (so every mail client threads it, not
   only Gmail's server-side view).
3. **Reversible reply draft, reuse the existing send.** The new operation creates a **reversible**
   reply *draft* in the parent's thread; **sending it reuses the just-shipped
   `/mail/drafts/<id> |> call mail.send`** (commit `2131a57`), and Gmail's `drafts.send` keeps a
   draft in its thread — so capability 5 falls out of capability 3 + the existing send, with no
   second threaded-send path to maintain.

So the surface is **one new reversible procedure** addressed at the parent message:

```
id:<msg> |> call mail.reply(body => '…' [, to => …, cc => …, subject => …])
```

It builds a draft in the parent's thread — reversible (a draft, exactly like an `INSERT INTO
/mail/drafts`, held until a separate send). `to` defaults to the parent's `From`, `subject` to
`Re: <parent subject>`, unless overridden. Sending the resulting draft is the existing
`/mail/drafts/<reply-draft-id> |> call mail.send`, which now threads because the draft carries
`threadId`.

## Policies

- `workaholic:implementation` / `policies/type-driven-design.md` — the thread linkage is typed data
  on the draft (an `Option<ThreadRef>` / owned struct carrying `thread_id` + parent `message_id`),
  never a bare string; a new-message vs reply-in-thread distinction is a sum type so an unhandled
  case is a compile error, not an empty `threadId`.
- `workaholic:implementation` / `policies/domain-layer-separation.md` — the reply semantics live in
  the driver's domain layer; the Gmail `threadId`/header JSON stays behind the `client`
  anti-corruption boundary (no google type crosses).
- `workaholic:implementation` / `policies/coding-standards.md` — no stringly thread refs, no
  `unwrap` on the optional `Message-Id`; clippy-clean under `--workspace --all-targets -D warnings`.
- `workaholic:implementation` / `policies/test.md` — hermetic `MockGmailClient` coverage asserting
  the outgoing draft/create JSON carries `threadId` and the MIME carries `In-Reply-To`/`References`;
  this dev host has LIVE Gmail — never probe real mail in a test (`.claude` memory:
  qfs-env-has-live-cloud-accounts).
- `workaholic:design` / `workaholic:safety` — `mail.send` stays irreversible; the new `mail.reply`
  **creates a reversible draft** and PREVIEW performs no I/O; `DESCRIBE` honestly reports the new
  procedure and which node it is addressed at.
- `workaholic:implementation` / `policies/objective-documentation.md` — the Gmail cookbook must
  teach the reply form truthfully (parse-checked AND driven end-to-end against the mock).

## Key Files

Verified anchors at HEAD `2131a57` (2026-07-09):

- `crates/driver-gmail/src/schema.rs:68` — `MailDraft`: add the thread linkage (`thread_id:
  Option<String>` + the parent `Message-Id` for `In-Reply-To`/`References`; consider a small owned
  `ReplyContext`). `Default` (`:66`) and `for_test` (`:90`) must keep compiling.
- `crates/driver-gmail/src/mime.rs:29,37-45` — `build_mime`: emit `In-Reply-To:` + `References:`
  from the new fields, after the To/Cc/Subject block, so both the simple and multipart branches
  inherit them (Gmail supplies `Message-Id` itself; threading needs `References` + the API
  `threadId`, not a self-generated `Message-Id`).
- `crates/driver-gmail/src/client.rs:338,352` — `create_draft`/`upsert_draft`: send `threadId`
  inside the `message` object (`{message:{raw, threadId}}`). Widen the trait signatures
  (`:110/:117`), the `GoogleApiGmailClient` impls, the `MockGmailClient` impls, and the
  `CreateDraft`/`UpsertDraft` `RecordedCall` variants (`:630/:635`) so tests can assert `threadId`.
- `crates/driver-gmail/src/client.rs:447,494-495` — `decode_message`: extract the `Message-Id`
  header (today only From/Subject are read) so a reply can source `References` from the parent.
- `crates/driver-gmail/src/effect.rs:216,295` — `decode_call`/`draft_from_row`: add the `mail.reply`
  decode arm (parent addressed via `MailPath::Message`); build the `GmailEffect` carrying the reply
  context (reversible — a draft-create, not a send).
- `crates/driver-gmail/src/applier.rs` — the apply leg: for `mail.reply`, `get_message(parent)` →
  `thread_id` + `Message-Id` + default `to`/`subject`, build the draft with `threadId` + headers,
  `create_draft` (reversible). The read-at-COMMIT is fine (COMMIT is the impure seam); PREVIEW never
  reaches the applier.
- `crates/driver-gmail/src/lib.rs:125-135,204` — declare the `reply` `ProcSig` (params `body`
  required; `to`/`cc`/`subject` optional; **reversible**, `requires_scopes` compose); `caps_for` /
  `procedures` are the anchors. Keep `mail.send` unchanged (it already threads a draft that carries
  `threadId`).
- `crates/driver-gmail/src/path.rs:36` — `MailPath::Message { id }` is the reply's parent address;
  no new variant needed (the column-based alternative was rejected).
- `crates/driver-gmail/src/tests.rs` — new tests: a `mail.reply` decode + apply (mock records a
  create carrying `threadId`), a MIME `In-Reply-To`/`References` golden (mirror `:703`), and a
  reply-draft → `mail.send` round-trip proving the send preserves the thread.
- `docs/cookbook/gmail.md` — a reply recipe (draft the reply, then send it), parse-checked +
  e2e-tested; regenerate the skill.

## Related History

- [20260708233730-fix-mail-send-existing-draft-by-id.md](.workaholic/tickets/archive/work-20260708-171710/20260708233730-fix-mail-send-existing-draft-by-id.md) — commit `2131a57`: the send-by-id fix this builds ON (`/mail/drafts/<id> |> call mail.send`, `MailPath::Draft`, `list_drafts`/`get_draft`, `Driver::plan_call`). Sending the reply draft reuses this.
- [20260701192439-query-array-struct-bytes-literals-gmail-draft-attachments.md](.workaholic/tickets/archive/work-20260629-110121/20260701192439-query-array-struct-bytes-literals-gmail-draft-attachments.md) — the t92 draft/attachment foundation (Array/Struct/Bytes literals) this reply draft reuses for attachments.
- [20260703150100-mail-drafts-write-parity.md](.workaholic/tickets/archive/work-20260703-194046/20260703150100-mail-drafts-write-parity.md) — the `/mail/drafts` INSERT/UPSERT write shape a reply draft extends with thread linkage.
- `docs/blueprint.md:66` — §"Irreducible domain actions": `mail.send`/`SEND` are the CALL/alias shape; the reply stays within it (no new keyword — `reply` is a contextual proc ident).

## Implementation Steps

1. **Message-Id read:** extend `decode_message` (`client.rs`) to capture the parent's `Message-Id`
   header; surface it where the applier can read it (a column or the `MailMessage` DTO). Add a decode
   unit test.
2. **Thread linkage on the draft:** add the typed thread/reply context to `MailDraft`
   (`thread_id` + parent `Message-Id`/references); keep `Default`/`for_test` compiling.
3. **MIME headers:** `build_mime` emits `In-Reply-To`/`References` from the context; MIME golden test.
4. **Client `threadId`:** widen `create_draft`/`upsert_draft` to send `threadId` in the `message`
   object; extend the real + mock impls and the `RecordedCall` variants.
5. **`mail.reply` proc:** declare the reversible `reply` `ProcSig` (`lib.rs`); decode it in
   `effect.rs` (parent addressed via `MailPath::Message`); the applier resolves the thread from the
   parent (`get_message`) and creates the draft (reversible) with `threadId` + headers, defaulting
   `to`=parent `From`, `subject`=`Re: <subject>` unless overridden.
6. **Send preserves the thread:** confirm `/mail/drafts/<reply-draft-id> |> call mail.send` sends the
   reply in-thread (drafts.send keeps `threadId`); add the round-trip test.
7. **Docs + skills + versions:** add the reply cookbook recipe (draft → send), regenerate
   `docs/drivers.md`/skills, bump the qfs patch and the four plugin version fields (new taught
   surface — see Quality Gate).

## Quality Gate

**Acceptance criteria:**

- `id:<msg> |> call mail.reply(body => '…')` creates a **reversible** draft; PREVIEW performs
  **zero** Gmail API calls (mock asserts empty), and COMMIT records a `create_draft` whose request
  carries the parent's `threadId` (asserted on the `RecordedCall`).
- The created draft's MIME carries `In-Reply-To` and `References` referencing the parent's
  `Message-Id` (MIME golden), and `to`/`subject` default to the parent's `From` / `Re: <subject>`
  unless overridden (asserted).
- Sending the reply draft via the existing `/mail/drafts/<id> |> call mail.send` records a
  `drafts.send` of THAT draft id and the thread is preserved (the draft carried `threadId`) — proven
  hermetically end-to-end (query string → plan → mock applier), not a hand-built node.
- `decode_message` extracts the parent `Message-Id` (unit test); a reply to a message with no
  resolvable thread/`Message-Id` fails closed with an actionable, secret-free error (no panic).
- `mail.reply` is **reversible** (a draft), `mail.send` stays **irreversible**; `DESCRIBE` reports
  the new procedure honestly.
- No google type crosses the `client` boundary; no token/secret in any output or error.

**Verification method:**

- `cargo test -p qfs-driver-gmail` (decode + apply + MIME golden + reply→send round-trip);
  a query-string e2e where the crate boundary allows (else split across core lowering +
  driver decode/apply, as the send-by-id fix did — document why).
- `cargo test -p qfs-test --test cookbook_skills` (reply recipe parses) + the e2e test (it runs).
- `cargo run -p xtask -- gen-docs --check` / `gen-skills --check` / `check-migrations`;
  `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all --check` (never piped).
- Manual live smoke (owner, out of band, one-shot, only with explicit approval): reply to a
  throwaway thread and confirm it threads; this host has LIVE Gmail — never in CI, never without ask.

**Gate:** all hermetic suites + four ratchets green; a reply drafts into the thread and sends in the
thread against the mock; PREVIEW-zero-calls and fail-closed-no-thread proven; no secret in any output.

## Considerations

- **Split sibling:** free attach/detach across every draft/send form (including this reply form) is
  the sibling ticket `20260709010931-gmail-attach-detach-every-draft-send-form.md`, which
  `depends_on` this one so it can cover the reply form.
- Experimental / no backward compat — pick definitive shapes; no shims. `reply` is a contextual
  proc ident (zero new keywords, blueprint policy).
- Reuse, don't reinvent: the reply draft is a normal reversible draft plus thread linkage; sending
  reuses the shipped `/mail/drafts/<id> |> call mail.send`. Do not add a second threaded-send path.
- Plugin re-version: a new taught procedure (`mail.reply`) is additive → **patch** the four plugin
  fields (minor only if a previously-taught form is hard-broken); qfs patch per shipped PR.
- Commit via `workaholic:commit` `commit.sh` with explicit file args; never `git add -A`
  (shared-tree concurrent sessions). `cargo fmt --check` never piped through head/tail.
