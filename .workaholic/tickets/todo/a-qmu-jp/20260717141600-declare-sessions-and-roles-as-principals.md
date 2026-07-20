---
created_at: 2026-07-17T14:16:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category:
depends_on:
mission:
---

# Declare sessions and roles as principals, so a consumer can derive a root menu

## Overview

Developer direction (design discussion, 2026-07-17), for qfs-viewer's **root column**:

- **Not signed in** → the menu is **sign-in only**.
- **After sign-in** → a **qfs-admin menu** and a **user menu** sit **side by side**.

They were explicit that these are **two different axes**:

- **The query paths** — actually connecting to GitHub and *querying* it (`/github`, `/mail`, …).
- **The admin view** — the **driver catalog** plus **which connections exist per driver**.

Both axes already have shipped surfaces (verified below). What is missing is the **principal** to
derive them from: qfs cannot today answer, for a given request, *"who am I"* and *"what may I
administer"*. This ticket scopes that answer.

## Verified against source (2026-07-17, this session)

Several of the assumptions this ticket started from were **wrong**; the corrections are the point.

### Sessions ARE landed — t46 shipped

- `packages/qfs/crates/session/` is `qfs-session`, *"the session domain core (roadmap **M1 / t46**):
  server-side sessions for the local web / dashboard face, built ON the t45 identity store"*
  (`session/src/lib.rs:1-2`). It ships `Session`/`SessionId` + expiry, the opaque `SessionToken`
  (hashed at rest), the `SessionStore` trait (`create`/`lookup`/`rotate`/`revoke`), cookie
  format/parse, and `authenticate(cookie_header, store) -> Result<Option<UserId>>`
  (`session/src/auth.rs:28-39`).
- **What t46 is** (the repo does record it): `t46` = *session handling*, part of **M1 — Identity
  (t45, t46)** (`.workaholic/stories/work-20260628-000332.md:27`). Its ticket
  `.workaholic/tickets/archive/work-20260628-000332/20260626100400-t46-session-handling.md` is
  **archived with `commit_hash: 651050a`** — shipped. Note the t-numbers are a historical ticket
  series; the current `docs/roadmap.md` no longer carries them (a `grep -n "t46" docs/roadmap.md`
  returns nothing), so code comments citing "roadmap §3.4" point at a document that has since been
  rewritten.
- **But the session is deliberately INERT.** `session/src/lib.rs:12-16`: *"This is
  **AUTHENTICATION STATE ONLY**. A session proves who you are; it grants nothing by itself this
  milestone. Authorization (policy / OAuth) is **M2** — until it lands, an attached session is
  **inert**: no data path may silently trust it."* And `auth.rs:8-10`: *"**It grants NOTHING.** …
  The result is deliberately inert."*
- The `SoleUser` doc comment (`identity/src/model.rs:111-112`) reads *"…for a session-less `whoami`
  (sessions land in t46)"* — **stale in tense**: t46 has landed. What remains true is narrower: the
  **CLI** `whoami` is session-less *by design* (`cmd/src/lib.rs:255`: *"the AUTHENTICATION surface
  — local sign-up + a session-less `whoami`"*), resolving `SoleUser::{None, One, Many}` and
  answering only when the deployment has exactly one user.

### A role model EXISTS — in fact, two

1. **`identity::Role`** (`identity/src/invite.rs:141-149`): `Owner | Admin | Member`, a coarse label
   on a `Membership` (M5 / t55). Default `Member`; an unknown stored value decodes to `Member` —
   *"fail toward least privilege"* (`:162-170`). Its doc (`:135-139`) flags it as an **OPEN PRODUCT
   DECISION**: *"the role taxonomy (super-admin / project-admin / member) overlaps the t53 admin
   split the roadmap §3.4 info box leaves open. This is a **label** on the membership for a LATER
   ACL (`POLICY`, t57), **NOT** an authorization grant — identity ≠ authorization (§4.1)."*
2. **The policy who-axis (t57)**: `server/src/policy/model.rs:296-307` defines
   `Subject::{Anyone, User(String), Role(String), Group(String)}` — with `Role` documented as
   *"a coarse role label (t55 `Role`: owner/admin/member …), resolved against the actor's
   (inheritance-expanded) role set in the decision context."* `server/src/policy/context.rs:24-38`
   defines `DecisionContext { user, roles, groups, memberships }`, and `satisfies_subject`
   (`:89-95`) matches `Subject::Role(r) => self.roles.contains(r)`. `RoleGraph` expands inheritance.

So the claim *"no role model"* is **false**, and the `model.rs:39` evidence for it does not say what
it appears to say: the full sentence (`identity/src/model.rs:38-40`) is *"`active` is the sign-up
default; `disabled` is reserved for a future admin / off-boarding path (not reachable in t45)."* —
`"admin /"` is a **line-wrap artifact** of "admin / off-boarding path", i.e. *administrative or
off-boarding*. It is **not** evidence of an anticipated admin `/` path. The conclusion "admin is
anticipated but not built" still holds — but the real evidence is elsewhere (below).

### The ACTUAL gap: nothing connects a session to a principal a consumer can read

This is what the ticket exists for.

- **No session → `DecisionContext` bridge exists.** `DecisionContext::for_user` appears in the
  binary only at `qfs/src/shared_connection.rs:167` (the shared-connection use gate) and in tests
  (`qfs/src/directory.rs:116,132,157`). **Nothing calls `authenticate()` and builds a
  `DecisionContext` from the resulting `UserId`.** The gate's default is
  `DecisionContext::anonymous()` — *"no user, no roles, no groups, no memberships … fail closed"*
  (`policy/context.rs:11-15,41-47`).
- **`/sys` is wired loopback / local-CLI super-admin only.** `qfs/src/sys.rs:24-27`: *"Until the
  super-admin vs. project-admin split is settled (roadmap §3.4), the binary wires this loopback /
  local-CLI super-admin only — the split is recorded as an open decision rather than baked into a
  model."* Its audit actor is the **constant** `ACTOR_CLI = "cli"` (`sys.rs:45-47`), whose doc says:
  *"a request-derived identity replaces this once **the super-admin session model lands**."*
- **`/sys/users` carries no role.** `driver-sys/src/schema.rs:151-157`: columns are `id`,
  `primary_email`, `status`, `created_at`. There is **no `/sys/memberships` node**, and `SysNode` is
  *"a **closed set**; a new admin view adds a variant here, never a side-channel API (the one-engine
  constraint)"* (`driver-sys/src/schema.rs:18-20`).
- **Both of the developer's axes already have surfaces** — which is exactly why the principal is
  the missing piece, and confirms the axes are genuinely distinct:
  - *Admin view* → `/sys/drivers` (the declared-driver registry) + `/sys/connections` (the
    connection registry: driver + connection label + created_at, **names/metadata only, never
    secret material** — `driver-sys/src/schema.rs:41-46`).
  - *Query paths* → the ordinary driver paths (`/github`, `/mail`, …).

**Net:** a consumer has no way to ask *"who am I"* or *"what may I administer"*. The pieces
(`authenticate`, `Role`, `Subject::Role`, `DecisionContext`) all exist and are **unconnected**.

## Scope

**What qfs must answer for a consumer (like qfs-viewer) to derive a root menu from the principal**,
at minimum:

1. **"Who am I"** — a request-derived principal from a **session** (t46 has shipped the mechanism;
   nothing exposes its answer). Including the honest negative: *not signed in* is a first-class
   answer, and it is what makes the sign-in-only menu derivable rather than guessed.
2. **"What may I administer"** — a **role**/administer-capability answer, distinguishing the
   qfs-admin menu from the user menu. Both role models above are candidates; **which one is
   authoritative is an open product decision the source itself flags** (t55 taxonomy vs. the t53
   admin split). This ticket must NOT bake that decision silently — it surfaces the choice.

**Explicitly in scope:** ruling how the answer is *shaped* and *reached*, on this repo's own
conventions — `/sys` is a **closed set** and *"a new admin view adds a variant here, never a
side-channel API (the one-engine constraint)"*, so a principal read-back is a `SysNode` variant or
an existing-node column, never a bespoke endpoint.

**Out of scope:**

- Building qfs-viewer's menu. This ticket delivers the *answer*; the consumer renders it.
- Un-inerting the session as an authorization grant across every data path (that is M2 / the
  policy work). "Refuse the undeclared" for the whole engine is a much larger change than
  "declare the principal".
- Settling the super-admin vs. project-admin taxonomy by fiat. Surface the open decision
  (`invite.rs:135-139`, `sys.rs:24-27`) and get it ruled; do not pick one in passing.

## Ordering consequence (why this blocks)

Without a declared principal surface, **a consumer would have to GUESS the admin/user
distinction** — inferring "this user is an admin" from side-evidence (sole-user-ness, whether a
`/sys` read happens to succeed, the shape of a connection list). That directly violates the
project's **「推測するな、宣言して拒否せよ」 — "declare, don't guess; refuse the undeclared"** —
the principle the sibling mission
`markdown-trees-are-queryable-as-documents-and-links-tables` names as load-bearing (its mission.md,
「Sections are the future relation carrier」). A guessed principal is worse than an absent one: it
is a **security-shaped guess**, and `identity::Role::decode` already establishes the repo's stance
that an unrecognised role must be *"a plain member, never silently an admin"*
(`invite.rs:162-170`). **A consumer that guesses inverts that default.**

So: the principal must be declared **before** a consumer's root menu is built against it.

## Mission relation — NOT owned by the agent mission

Checked, as asked: the active mission
`support-create-agent-semantics-that-introduce-a-new-user-principal-with-query-functions-scheduled-launch-and-access-control-to-resources`
does **not** own this. Read directly, its principal is an **AGENT** — delegated automation:

- Its goal: *"`CREATE AGENT <name> …` introduces a NEW PRINCIPAL — an identity distinct from the
  operator who created it"*, and its access-control acceptance item is *"The policy gate evaluates
  the **AGENT** as subject"*.
- It treats the human/operator principal and the t57 machinery as **existing building blocks it
  composes**: *"The t57 policy axes (`FOR <subject>`, `AT <scope>`, `member_of`, `DecisionContext`)
  **already model subjects**"*. It never asks "who is the human at this request" nor "what may a
  consumer render".
- Its scope explicitly parks identity federation: *"Multi-tenant / network identity federation
  (OIDC for agents, cross-daemon agent identity) — the principal is daemon-local this mission."*

**Verdict:** this ticket is **adjacent** to that mission — they share the `DecisionContext` seam,
and whoever lands either will touch it — but it is **not one of that mission's six acceptance
items**, and scoping it as an advance on that mission would misreport what that mission is for.
`mission:` is therefore left **empty**. Whether this becomes a new acceptance item there, or its
own mission, is a developer decision this ticket surfaces rather than takes.

Worth carrying to that decision: that mission's own changelog (2026-07-16) warns its acceptance
items *"have never been re-litigated against the source … Treat the items below as headings, not
findings."* This ticket's corrections above are a live example of why.

## Key Files

- `packages/qfs/crates/session/src/{lib,auth,model,store}.rs` — the shipped, inert session (t46).
- `packages/qfs/crates/identity/src/invite.rs:135-170` — `Role`, and the flagged open taxonomy.
- `packages/qfs/crates/identity/src/model.rs` — `User`/`UserStatus`/`SoleUser` (+ the stale t46 doc).
- `packages/qfs/crates/server/src/policy/{context,model}.rs` — `DecisionContext`, `Subject::Role`,
  `RoleGraph`; the seam nothing populates from a session.
- `packages/qfs/crates/qfs/src/sys.rs:24-27,45-47` — the loopback super-admin wiring + `ACTOR_CLI`.
- `packages/qfs/crates/driver-sys/src/schema.rs` — `SysNode` (closed set), `/sys/users` columns,
  `/sys/connections` and `/sys/drivers` (the admin-view axis).
- `packages/qfs/crates/qfs/src/identity.rs:73-95` — the session-less `whoami` as it stands.

## Policies

- `workaholic:design` — "declare, don't guess; refuse the undeclared". A consumer must READ the
  principal, never infer it; this is the ticket's whole reason to exist.
- `workaholic:safety` — a principal answer is a security surface. It must fail closed (anonymous =
  no roles), never widen a grant, and never carry credential material — the `/sys/connections`
  redaction contract (names/metadata only) is the standard to match.
- `workaholic:implementation` — `/sys` is a closed set by construction; a new admin view is a
  `SysNode` variant, never a side-channel API (the one-engine constraint).
- `workaholic:planning` — the super-admin vs. project-admin split is a flagged OPEN PRODUCT
  DECISION; surface it for a ruling, do not bake it in while implementing something else.

## Quality Gate

Verify with **raw exit codes** (`echo "EXIT=$?"` directly after the command; never `cmd | tail`,
which masks failure).

1. **"Who am I" is answerable from a session, and its negative is first-class.** A request carrying
   a live session resolves to a principal; a request with no session resolves to an explicit
   *not-signed-in* answer — **not** an error, and **not** a silent fallback to the sole user.
   Demonstrated by a real run, output and exit code pasted.
2. **"What may I administer" is answerable, and fails closed.** The answer distinguishes the
   qfs-admin capability from an ordinary user. An unauthenticated or role-less principal answers
   *"nothing"* — pinned by a test that would fail if the default ever widened to admin.
3. **The taxonomy decision is recorded, not assumed.** The ticket's outcome states which role model
   is authoritative for this answer (`identity::Role` vs. the t57 `Subject::Role` set) **with the
   developer's ruling**, and updates the OPEN PRODUCT DECISION flags at `invite.rs:135-139` and
   `sys.rs:24-27` to match — or records explicitly that they stand unresolved and why.
4. **The surface rides the closed-set convention.** Any new read-back is a `SysNode` variant or a
   column on an existing node, reachable through the one engine — verified by a `DESCRIBE` run —
   and NOT a bespoke endpoint. Its schema carries **no credential column**, matching the
   `/sys/connections` redaction contract; pinned by a structural test.
5. **The stale doc is corrected.** `identity/src/model.rs:111-112` ("sessions land in t46") no
   longer reads as future tense, since t46 shipped at `651050a`. Any other doc comment this
   ticket's findings falsify is corrected in the same PR.
6. **The consumer no longer needs to guess.** State, concretely, how qfs-viewer derives all three
   root-column states (signed-out → sign-in only; signed-in user menu; signed-in admin menu) from
   the declared answers **alone**, naming the exact query/paths — with the admin view resolved from
   `/sys/drivers` + `/sys/connections`, and NOT conflated with the query-path axis.
7. **Workspace gates green, raw exit codes shown**: `cargo test --workspace`, `cargo clippy
   --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`, and
   `cargo run -p xtask -- gen-docs --check` if any describable surface changed. Patch version
   bumped per CLAUDE.md if this reaches a PR.

---

## STATUS: BLOCKED — stopped and escalated 2026-07-17 (branch `work-20260717-160001`)

**This ticket is NOT implemented, and is deliberately left in `todo/`.** Its central deliverable —
the principal — is blocked on a question this ticket itself says must not be answered in passing.
Only Gate 5 (the stale doc) was delivered; it is the one gate that needs no ruling.

### Blocker 1 — "What may I administer" IS the open product decision (refused)

Gate 2 asks for an answer that *"distinguishes the qfs-admin capability from an ordinary user"*, and
Gate 3 asks to record which role model is authoritative **"with the developer's ruling"**. There is
no ruling to record, and **deriving one is the decision itself**:

- `identity::Role` (`invite.rs:135-149`) is flagged verbatim as an **"OPEN PRODUCT DECISION
  (flagged, t55 — not baked in)"**, is *"a **label** on the membership for a LATER ACL (`POLICY`,
  t57), **NOT** an authorization grant — identity ≠ authorization (§4.1)"*, and `Role::Admin` is
  documented *"reserved for the t53/t57 admin split; **not privileged yet**"*.
- `qfs/src/sys.rs:24-27` independently records the same gap: *"Until the super-admin vs.
  project-admin split is settled (roadmap §3.4), the binary wires this loopback / local-CLI
  super-admin only — the split is recorded as an open decision rather than baked into a model."*

Any implementation of "what may I administer" must say **which principals are admins**. Ruling that
`Role::Admin`/`Owner` ⇒ the qfs-admin menu would convert a label qfs explicitly calls *not a grant*
into a grant, and would settle the t55-vs-t53 taxonomy **by fiat while implementing something
else** — precisely what this ticket's own Scope and Policies forbid. **Refused; escalated.**

Note this is not a gate that can be met halfway: a principal that always answers *"administers:
nothing"* is fail-closed and honest, but it makes the signed-in **admin menu underivable**, so
Gate 6 could not be met either. The shape of the answer is not separable from the ruling.

### Blocker 2 — "Who am I" is not reachable from a `SysNode` today (architectural, not a decision)

This is a **new finding**; the ticket assumed the pieces were merely "unconnected". They are, but the
connector is a core seam, not a wire:

- Gate 4 (and the repo's one-engine constraint) requires the read-back to be a `SysNode` variant read
  **through the engine**. The engine's read seam is
  `ReadDriver::scan(&self, scan: &ScanNode)` (`exec/src/read.rs:48`) — it carries **no request, no
  cookie, no principal**. A `/sys/principal` node physically cannot see who is asking.
- The engine and read registry are **process-wide**, built once at boot and shared across requests:
  `handler.rs:37-39` holds `engine: Arc<Engine>` + `reads: Arc<ReadRegistry>`. There is no
  per-request driver instance a principal could be bound into.
- **The HTTP request path resolves no actor at all**, confirming the gap is not merely at the driver
  seam: `http/src/policy.rs:76` calls `qfs_server::evaluate(&resolved, plan)` — the **back-compat**
  entry point, which is *"equivalent to `evaluate_with_context` with `DecisionContext::anonymous`"*
  (`server/src/policy/enforce.rs:172-179`). Its own doc states the consequence: *"a t57-narrowed rule
  contributes nothing **until a real actor is resolved** (fail closed)."* The t57 who-axis is built
  and correct; **no caller ever supplies it an actor.**
- So Gate 1 (*"A request carrying a live session resolves to a principal"*) requires threading a
  request-derived identity through a **core trait implemented by every driver** — the same seam M2 /
  the policy work needs. That is a foundational change, not the enhancement this ticket is typed as.
- **And its design is downstream of Blocker 1**: what the principal *carries* (which role set, whose
  taxonomy) is exactly the unresolved question. Building the thread first would bake the answer into
  a trait signature.

### Corrections to this ticket's own "Verified against source"

Verified in the tree at `f895c4a`; the ticket's corrections needed corrections.

1. **"Nothing calls `authenticate()`"** — **imprecise**. `authenticate()` **is** called, at
   `qfs/src/oauth.rs:232` and `:262` (the OAuth authorization-server / consent face). The true, and
   narrower, statement: *nothing calls it on the **query** path, and nothing builds a
   `DecisionContext` from its result.* Confirmed: outside tests, `DecisionContext` is constructed
   only at `qfs/src/shared_connection.rs:167` (from an `actor` string, **not** a session) and via
   `DecisionContext::anonymous()` at `server/src/policy/enforce.rs:178`.
2. **"archived with `commit_hash: 651050a` — shipped"** — the ticket record says so, but **that hash
   does not resolve in this repository**:
   ```
   $ git rev-parse --verify 651050a^{commit}   → fatal: Needed a single revision   EXIT=128
   ```
   The repo was **republished fresh** (`f9387de`, *"Publish qfs as a fresh repository"*), which is
   the commit that introduces `crates/session/src/lib.rs` and the t46 ticket file alike. **t46 did
   land** — `qfs-session` ships — but `651050a` is a pre-publication artifact. It must not be cited
   as a live hash in shipped source (Gate 5 originally asked for exactly that citation).
3. **The `Role`/`SoleUser` readings hold** as written, including that `model.rs:38-40`'s `"admin /"`
   is a line-wrap artifact of *"admin / off-boarding path"*.

### What WAS delivered — Gate 5 only (no ruling required)

Two doc comments asserted a **shipped** milestone in the future tense. Both corrected:

- `identity/src/model.rs:111-112` — `SoleUser`'s *"(sessions land in t46)"*. Rewritten: t46 has
  shipped; the CLI `whoami` is session-less **by design** (the OS login is the authentication there,
  ADR 0008), not pending a milestone. The `Many` variant's *"(no session yet)"* corrected likewise.
- `cmd/src/lib.rs:255` — **a second instance the ticket did not list**: *"sessions land in t46"* in
  `IdentityAction`'s doc. Same correction. (Gate 5: *"Any other doc comment this ticket's findings
  falsify is corrected in the same PR."*)

Neither touches behavior; no describable surface changed. **No `SysNode` variant was added, no
principal was declared, and no role model was made authoritative.**

### What the developer must rule before this ticket can proceed

1. **Which role model is authoritative** for "what may I administer" — `identity::Role` (t55) or the
   t57 `Subject::Role` set resolved through `DecisionContext`? The source flags these as
   overlapping and unsettled (`invite.rs:135-139`).
2. **Does a role grant anything?** Today `Role` is explicitly *not* a grant and `Admin` is *not
   privileged*. The admin menu needs *some* principal to be an admin. Which one, and by what rule?
3. **Super-admin vs. project-admin** (`sys.rs:24-27`, roadmap §3.4) — still open, and it decides
   whether "administer" is one capability or a scoped set.
4. **Is the request-principal thread (Blocker 2) in scope here, or its own ticket/mission?** It is a
   core-trait change across every driver and is adjacent to M2 and to the active agent-principal
   mission (which this ticket correctly declines to claim).

Until (1)–(3) are ruled, a consumer's admin menu cannot be derived from a declared answer — and per
this ticket's own *Ordering consequence*, it must not be **guessed**. The honest current state is
that qfs-viewer's root column can derive **signed-out vs. signed-in** only once Blocker 2 is built,
and cannot derive **admin vs. user** at all.

### Gate

`cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test
--workspace` — raw exit codes recorded in the commit. Patch bumped to `0.0.78` (shipped source
changed). `gen-docs --check` unaffected: no describable surface changed.
