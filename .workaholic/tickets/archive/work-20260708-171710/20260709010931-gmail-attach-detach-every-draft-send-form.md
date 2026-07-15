---
created_at: 2026-07-09T01:09:31+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, Infrastructure]
effort:
commit_hash: 41e0ce4
category: Added
depends_on: [20260709010930-gmail-thread-reply-support.md]
mission:
---

# Attach and detach files freely across every Gmail draft/send/reply form

## Overview

Attaching files works today **only** on `INSERT`/`UPSERT INTO /mail/drafts` (the `attachments`
`Array(Struct{filename, mime, bytes})` column), and there is **no explicit "detach."** The owner's
requirement is that a user can **attach and detach files freely in any case** — every form that
creates or sends mail. This ticket closes the gaps so attachments are uniform across:

- new draft (`INSERT`), updated draft (`UPSERT`) — already work;
- **create-then-send** via `call mail.send(to => …, subject => …, body => …)` — **cannot carry
  attachments today** (the `send` proc declares only `to/subject/body`);
- the **reply draft/send** form added by the sibling ticket (`depends_on`);
- **detach** — removing an attachment from a draft.

**Verified state (HEAD `2131a57`):**

- Attach: `effect.rs::attachments_col` (`:343`) decodes the `attachments` `Array(Struct{filename,
  mime, bytes})` column into `MailDraft.attachments`; `mime.rs::build_mime` (`:71-82`) emits one
  `multipart/mixed` base64 part per attachment. This fires for `draft_from_row` (INSERT/UPSERT) and
  for the `mail.send` create-then-send row — but the **`send` proc params** (`lib.rs:129-134`) are
  `to/subject/body` only, so a `call mail.send(...)` cannot pass `attachments`.
- Detach: the only expression today is **UPSERT full-replace** — `upsert_draft` (`client.rs:352`)
  PUTs a fresh `raw` MIME built solely from the row's current `attachments`, so any attachment
  omitted from the UPSERT row is dropped and any new one added — the draft's attachment set is
  wholesale whatever the latest UPSERT row says. This is undocumented and untested as "detach."
- Read/write shape asymmetry: an attachment **read** yields `{filename, mime, size}` (+
  `attachment_id`; bytes fetched on demand via `/mail/<label>/<msg>/<att>` → a `content` `Bytes`
  column), while a **write** consumes `{filename, mime, bytes}`. So a read→write attachment copy is
  **not** a symmetric round-trip — re-attaching a received attachment must source its **bytes** from
  the byte-read node, not from the listing row.

**Settled design decision (owner, 2026-07-08 — do not re-litigate):** **detach = explicit UPSERT
full-replace.** No new `DETACH` keyword (blueprint: new nouns are contextual idents, zero new
keywords). The draft's attachment set is exactly the latest UPSERT row's `attachments` array;
omitting an attachment detaches it, omitting all detaches everything. This ticket **makes that
semantics explicit, documented, and tested** — it does NOT add per-attachment identity/removal.

## Policies

- `workaholic:implementation` / `policies/type-driven-design.md` — the attachment set is typed
  (`Vec<Attachment>` with `{filename, mime, bytes}`); the read-vs-write shape difference is made
  explicit, not smuggled through a catch-all.
- `workaholic:implementation` / `policies/coding-standards.md` — add the `attachments` proc param
  and reply-form wiring without stringly plumbing; clippy/fmt clean.
- `workaholic:implementation` / `policies/test.md` — hermetic mock coverage: attach on every form,
  detach-by-omission, detach-then-reattach, zero/one/many attachments, and a byte-read → re-attach
  round-trip. No live Gmail.
- `workaholic:implementation` / `policies/objective-documentation.md` — the cookbook must teach
  attach AND detach truthfully (parse-checked + e2e), including that detach is UPSERT full-replace
  and that re-attaching a received file sources its bytes from the attachment byte-read.
- `workaholic:design` / `workaholic:safety` — attaching/detaching a **draft** stays reversible;
  only `mail.send` is irreversible; no attachment bytes leak into logs/errors.

## Key Files

Verified anchors at HEAD `2131a57` (2026-07-09):

- `crates/driver-gmail/src/lib.rs:129-135` — the `send` `ProcSig`: add an `attachments` param (typed
  as the `Array(Struct{filename,mime,bytes})` the column decoder already reads) so
  `call mail.send(..., attachments => [...])` and the reply form can attach. Keep `plan_call`
  (`:311`) consistent (an attachment-only send still needs a draft/recipient).
- `crates/driver-gmail/src/effect.rs:295,343` — `draft_from_row`/`attachments_col`: already decode
  `attachments`; confirm the create-then-send and reply rows route through it (add the param → column
  bridge if a proc arg needs mapping to the `attachments` column name).
- `crates/driver-gmail/src/client.rs:352` — `upsert_draft` full-replace: the detach mechanism; keep
  it authoritative (the new `raw` fully replaces the stored draft, attachments and all).
- `crates/driver-gmail/src/mime.rs:71-82` — the multipart builder (attach); no change expected, but
  a zero-attachment draft must produce a valid single-part message (assert).
- `crates/driver-gmail/src/read.rs` + `schema.rs` (`AttachmentMeta {filename,mime,size,
  attachment_id}` vs `Attachment {filename,mime,bytes}`) — document/reconcile the read-vs-write
  shape: re-attach sources bytes from `/mail/<label>/<msg>/<att>` (`content` Bytes), not the listing.
- `crates/driver-gmail/src/tests.rs:505` — the existing attachment decode test; extend for send/reply
  attach, detach-by-omission, and the byte-read → re-attach round-trip.
- `docs/cookbook/gmail.md` + `docs/cookbook/cross-service.md:52,205` — the attach recipes; add a
  detach recipe and a "re-attach a received file" recipe; keep the Drive→attach cross-service recipe
  working.

## Related History

- [20260709010930-gmail-thread-reply-support.md](.workaholic/tickets/todo/a-qmu-jp/20260709010930-gmail-thread-reply-support.md) — the sibling this `depends_on`; the reply form is one of the "every form" this ticket must cover for attach/detach.
- [20260701192439-query-array-struct-bytes-literals-gmail-draft-attachments.md](.workaholic/tickets/archive/work-20260629-110121/20260701192439-query-array-struct-bytes-literals-gmail-draft-attachments.md) — t92: the `attachments` `Array(Struct{filename,mime,bytes})` column + Array/Struct/Bytes literals this extends to every form.
- [20260701192440-cross-service-drive-to-gmail-attach-and-send.md](.workaholic/tickets/archive/work-20260629-110121/20260701192440-cross-service-drive-to-gmail-attach-and-send.md) — Drive-download → Gmail attach-and-send; must stay compatible, and is the model for byte-sourced re-attach.
- [20260701192441-gmail-attachment-byte-read.md](.workaholic/tickets/archive/work-20260629-110121/20260701192441-gmail-attachment-byte-read.md) — `get_attachment` byte-read (`/mail/<label>/<msg>/<att>` → `content`), the bytes source for re-attaching a received file.

## Implementation Steps

1. **Attach on send/reply:** add the `attachments` param to the `send` `ProcSig` (and the reply
   proc from the sibling), bridging it to the `attachments` column the decoder already reads, so
   `call mail.send(..., attachments => [...])` and a reply attach files. Confirm INSERT/UPSERT
   already attach (regression test).
2. **Detach = UPSERT full-replace, made explicit:** document and test that an UPSERT row's
   `attachments` array is the draft's full set — omit to detach, omit all to clear. No new keyword,
   no per-attachment API.
3. **Re-attach a received file:** prove a byte-read (`/mail/<label>/<msg>/<att>` → `content` Bytes)
   can feed the `attachments` `bytes` field of a new draft/reply (the read `{filename,mime,size}` vs
   write `{filename,mime,bytes}` asymmetry resolved by sourcing bytes from the byte-read). Document
   the asymmetry in the cookbook.
4. **Cookbook + skills + versions:** add attach-on-send, detach, and re-attach recipes (parse-checked
   + e2e); regenerate skills; bump the qfs patch and the four plugin version fields.

## Quality Gate

**Acceptance criteria:**

- Attach works on **every** form, proven hermetically (mock records the outgoing `raw` carrying the
  attachment parts): `INSERT`, `UPSERT`, `call mail.send(..., attachments => [...])`, and the reply
  form (draft + send).
- **Detach**: an `UPSERT` row that omits an attachment drops it, and one with an empty/absent
  `attachments` clears all — asserted against the mock (the new `raw` has no attachment part).
- **Detach-then-reattach** round-trips: after detaching, a later `UPSERT` re-adds an attachment and
  the mock records it.
- **Re-attach a received file**: a byte-read (`content` Bytes) fed into a draft's `attachments`
  bytes produces an outgoing message carrying that file (hermetic; bytes sourced from the read, not
  the listing) — proving the read/write shape asymmetry is handled and documented.
- Attaching/detaching a **draft** stays reversible; PREVIEW performs no send; no attachment bytes
  appear in any log/error.
- Cookbook attach + detach + re-attach recipes are true against the binary (parse-checked AND
  e2e-tested).

**Verification method:**

- `cargo test -p qfs-driver-gmail` (attach-every-form, detach-by-omission, detach→reattach,
  byte-read→re-attach); `cargo test -p qfs-test --test cookbook_skills` + the e2e tests.
- `cargo run -p xtask -- gen-docs --check` / `gen-skills --check`; `cargo clippy --workspace
  --all-targets -- -D warnings`; `cargo fmt --all --check` (never piped).
- Manual live smoke (owner, out of band, only with explicit approval): attach a file, send, detach
  via re-upsert, confirm; LIVE Gmail on this host — never in CI, never without ask.

**Gate:** all hermetic suites + ratchets green; attach proven on every form; detach and reattach
proven against the mock; a received file re-attaches from its byte-read; no bytes leak.

## Considerations

- **Depends on** the thread-reply sibling (`20260709010930`) so "every form" genuinely includes the
  reply form; attach/detach for INSERT/UPSERT/send is otherwise independent.
- Experimental / no backward compat — pick definitive shapes; **detach is UPSERT full-replace**, not
  a new keyword or per-attachment identity (explicitly out of scope; revisit only if a stable
  attachment id is later needed).
- Read vs write attachment shape differs by design (listing carries metadata only; bytes are fetched
  on demand); do not force a symmetric struct — source bytes from the byte-read for re-attach.
- Plugin re-version: adding the `attachments` proc param + detach recipe is additive taught surface →
  **patch** the four plugin fields; qfs patch per shipped PR.
- Commit via `workaholic:commit` `commit.sh` with explicit file args; never `git add -A`.
