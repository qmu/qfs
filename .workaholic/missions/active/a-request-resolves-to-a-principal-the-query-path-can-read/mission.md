---
type: Mission
title: A request resolves to a principal the query path can read
slug: a-request-resolves-to-a-principal-the-query-path-can-read
status: active
created_at: 2026-07-17T18:05:46+09:00
author: a@qmu.jp
assignee: a@qmu.jp
drive_authorized: true
tickets: []
stories: []
concerns: []
gate_type:
gate_target: the "who am I" answer on the path a query takes
gate_assert: North star, not a machine check — a request carrying a live session resolves to a named principal, a request without one resolves to an explicit not-signed-in answer, and the policy gate evaluates the resolved actor instead of anonymous. Verified per ticket, not by reading a page.
---

# A request resolves to a principal the query path can read

## Goal

**qfs can answer, for a given request, "who is asking" — on the path a query actually takes.**
Today it cannot, at any layer. The pieces to answer it all ship; nothing connects them to the
query path.

This mission threads a request-derived principal to the seam where queries are served, so the
answer becomes readable by the policy gate and by a consumer. It delivers **"who am I"**. It
deliberately does **not** deliver **"what may I administer"** — that is an open product decision
this mission does not close (see *The decision this mission does not take*).

### Why this is its own mission (developer ruling, 2026-07-17)

Ticket `20260717141600-declare-sessions-and-roles-as-principals.md` asked qfs to declare a
principal so a consumer could derive a root menu. It is **blocked and stays in `todo/`** — not
superseded by this mission, and not archived. Driving it surfaced two blockers, and the developer
ruled the second one **its own mission**:

- **Blocker 1 — "what may I administer" IS the open product decision.** Refused and escalated;
  unchanged by this mission, which does not touch it.
- **Blocker 2 — "who am I" is not reachable from a driver at all.** A core-trait change across
  every driver — the M2 seam. **That is this mission.**

The developer also ruled it is *not* part of the active mission
`support-create-agent-semantics-…`. That mission's principal is an **agent** (delegated
automation acting as itself); it treats the human/operator principal and the t57 machinery as
building blocks it composes, and never asks "who is the human at this request". The two share the
`DecisionContext` seam and whoever lands either will touch it — but they are different missions.
Note that mission's own changelog warns its acceptance items *"have never been re-litigated
against the source … Treat the items below as headings, not findings."*

### The plan-book background

The 正本 (canonical) background is the strategy repository, **`docs/plan.md`**, section
**「アクセス制御」** and the open question **「第一列は何から導かれるか」**. This mission is written
to be driven **without reading that document**, so the load-bearing background is restated here:

- **Principals are the model, and humans and AI are peers.** 「主体（principal）は人間と AI（ボット、
  API キー発行）を同格に扱い、AWS IAM のように **RBAC と PBAC を組み合わせる**」 — roles bundle
  subjects (RBAC), policies declare (subject/role, verb, path-pattern) triples as grants (PBAC).
- **The principal is what a consumer's first column is derived from.** 「ビューアの第一列に何が並ぶかは、
  **いまの主体が誰であるか**から導かれる — 誰でもないなら、並ぶのはサインインだけである。」 The
  signed-out state is a first-class answer, not an absence.
- **The resource unit is the path,** so one policy covers every face (screen, query, console, HTTP)
  and no per-face permission drift is structurally possible. **A trail is not a bypass**: resolution
  happens under the caller's principal.
- **The admin column is explicitly parked on this.** 「その列を誰に見せるかは主体から導かれるため、qfs が
  「誰が管理者か」を宣言できるようになるまでは開いている（開いた問い）。」 That is the decision this
  mission does not close.

**Two premises in the plan book are false as written**, corrected by measurement (2026-07-17) and
recorded here so this mission is not driven from them. `docs/plan.md`'s open question 「第一列は何から
導かれるか」 states 「**ロールのモデルが無く**」 and 「**セッションも未着**である」. Both are wrong:
**two** role models exist, and t46 sessions **shipped**. The real gap is narrower and different —
nothing supplies an actor to machinery that is already built. The plan book is the canonical
background for *intent*; this mission's *Measured starting state* is canonical for *fact*.

## Scope

**Done when** every acceptance item below is ticked: a request carrying a live session resolves to
a named principal on the path a query takes, a request without one resolves to an explicit
not-signed-in answer, the t57 policy gate evaluates that resolved actor instead of the anonymous
context, the answer is readable by a consumer through the one engine, and the whole loop is proved
by a real run.

**Out of scope — do not do these in passing:**

- **"What may I administer."** The open product decision. This mission does not answer it, and must
  not answer it as a side effect of threading the principal. See below.
- **Making `identity::Role` a grant.** Ruled by the developer, 2026-07-17: **`Role` remains NOT a
  grant.** See *The invariant this mission must not break*.
- **Settling the super-admin vs. project-admin split** (`qfs/src/sys.rs:24-27`, roadmap §3.4).
  Flagged open in the source; surface it, do not pick one.
- **Building qfs-viewer's root menu.** This mission delivers the answer; the consumer renders it.
- **Un-inerting the session as an authorization grant across every data path.** "Refuse the
  undeclared" for the whole engine is M2's full scope; declaring the principal is this mission.
- **Refusing unauthenticated requests** (t50 for MCP, t51 for the SPA) — deferred by the session
  crate's own docs and not claimed here.
- **Multi-tenant / network identity federation**, and agent principals (the sibling
  `support-create-agent-semantics-…` mission owns delegated automation).

## Experience

What must be true when this mission is achieved, stated as behavior that can be observed:

1. **"Who am I" is answerable for a request, and its negative is first-class.** A request carrying
   a live session resolves to a named principal. A request with no session resolves to an explicit
   **not-signed-in** answer — **not** an error, and **not** a silent fallback to the sole user. The
   signed-out answer is what makes a consumer's sign-in-only state derivable rather than guessed.
2. **The answer is reachable on the path a query takes** — not only on a face that happens to have
   a session already. The seam that serves a scan can see who is asking; a driver does not have to
   invent its own way to find out, and no per-face divergence appears.
3. **The policy gate evaluates a real actor.** A t57-narrowed rule (`FOR <subject>`) contributes
   something when a principal is present, instead of contributing nothing because the actor is
   always anonymous. The machinery is already built and correct; what changes is that it receives
   an input.
4. **Fail-closed is preserved, and provably so.** No session ⇒ anonymous ⇒ no user, no roles, no
   groups, no memberships ⇒ default-deny still holds. Nothing widens because a principal was
   threaded. A test would fail if the default ever widened.
5. **The answer is machine-readable.** A consumer can read the principal as data through the one
   engine, without parsing prose and without a bespoke endpoint. `/sys` is a **closed set** by
   construction — *"a new admin view adds a variant here, never a side-channel API (the one-engine
   constraint)"* (`driver-sys/src/schema.rs:18-20`).
6. **The answer carries no credential material.** The `/sys/connections` redaction contract
   (names/metadata only, never secret material — `driver-sys/src/schema.rs:41-46`) is the standard
   to match.
7. **A consumer no longer guesses the signed-in/signed-out distinction.** It reads it. Whether it
   can distinguish *admin* from *user* is **out of scope** and stays open.

**How the answer is shaped and reached is not prescribed here.** The acceptance below describes the
demanded structure; the design is the mission's to rule, per ticket, against the source — not this
document's to dictate.

## The invariant this mission must not break

**`identity::Role` is NOT a grant, and this mission does not convert it into one** (developer
ruling, 2026-07-17).

`identity::Role` (`identity/src/invite.rs:141-149`) is `Owner | Admin | Member` — a coarse **label**
on a `Membership`, applied when an invite is redeemed (t55 / M5). Its own doc (`:135-139`) flags it
as an **OPEN PRODUCT DECISION** and states: *"This is a **label** on the membership for a LATER ACL
(`POLICY`, t57), **NOT** an authorization grant — identity ≠ authorization (§4.1)."* `Role::Admin`
is documented *"reserved for the t53/t57 admin split; **not privileged yet**"*. `qfs invite --help`
says the same to the operator: *"MEMBERSHIP, not authorization (§4.1): redeeming makes someone a
*member*, never grants a capability (the ACL is t57)."*

The failure mode to avoid is quiet: threading a principal that carries a role set, and letting some
consumer or gate treat `Role::Admin` as "is an admin", settles the t55-vs-t53 taxonomy **by fiat
while implementing something else** — which is exactly what ticket `20260717141600`'s scope
forbids, and why it was escalated rather than driven.

`Role::decode` (`invite.rs:162-170`) already establishes the repo's stance: an unrecognised role
decodes to `Member` — *"fail toward least privilege"*, *"a plain member, never silently an admin"*.
A principal that inverted that default would be worse than no principal at all.

## The decision this mission does not take

**"What may I administer" is an OPEN PRODUCT DECISION (t55), and this mission does not close it.**

The source flags it in two independent places:

- `identity/src/invite.rs:135-139` — the role taxonomy (super-admin / project-admin / member)
  overlaps the t53 admin split; recorded as open, *"not baked in"*.
- `qfs/src/sys.rs:24-27` — *"Until the super-admin vs. project-admin split is settled (roadmap
  §3.4), the binary wires this loopback / local-CLI super-admin only — the split is recorded as an
  open decision rather than baked into a model."*

What the developer must rule before any mission can answer "what may I administer":

1. **Which role model is authoritative** — `identity::Role` (t55) or the t57 `Subject::Role` set
   resolved through `DecisionContext`?
2. **Does a role grant anything?** Today `Role` is explicitly not a grant and `Admin` is not
   privileged. An admin view needs *some* principal to be an admin. Which one, and by what rule?
3. **Super-admin vs. project-admin** — decides whether "administer" is one capability or a scoped
   set.

**This mission proceeds without those rulings, and that is deliberate.** "Who am I" is separable
from "what may I administer": the first is answerable today (a session either resolves to a user or
it does not), the second is not. A mission that waited for the ruling would deliver neither.

**The honest consequence, stated up front:** when this mission is achieved, a consumer can derive
**signed-out vs. signed-in**. It still **cannot** derive **admin vs. user**. That is not a shortfall
of this mission — it is the open decision, and this mission is what makes the ruling implementable
once it is made.

## Measured starting state (2026-07-17, binary `qfs 0.0.78`, tree at `91cde7d`)

Verified by execution and by reading the tree. A later session should re-check these before cutting
each ticket — line references drift, and this section's own predecessor needed three corrections.

### The machinery is complete. The input never arrives.

- **The t57 policy who-axis is built and correct.** `server/src/policy/model.rs:296-307` defines
  `Subject::{Anyone, User(String), Role(String), Group(String)}`; `server/src/policy/context.rs:25-38`
  defines `DecisionContext { user, roles, groups, memberships }`; `RoleGraph`
  (`server/src/policy/model.rs:537-540`) expands role inheritance; `satisfies_subject`
  (`context.rs:89-95`) matches `Subject::Role(r)` against the actor's expanded role set.
- **No caller ever supplies it an actor.** `server/src/policy/enforce.rs:177-179` —
  `pub fn evaluate(policy, plan)` is the back-compat entry and is
  `evaluate_with_context(policy, plan, &DecisionContext::anonymous())`. Its own doc states the
  consequence: *"a t57-narrowed rule contributes nothing **until a real actor is resolved** (fail
  closed)."*
- **The HTTP path resolves no actor.** `http/src/policy.rs:76` calls that back-compat `evaluate()`.

### "Who am I" is not reachable from a driver

- **The read seam carries no request.** `exec/src/read.rs:48` —
  `async fn scan(&self, scan: &ScanNode) -> Result<RowBatch, CfsError>;` — no request, no cookie,
  no principal. This is a **core trait every driver implements**; that is what makes this the M2
  seam and not a wire.
- **Engine and registry are process-wide.** `http/src/handler.rs:35-45` — `EndpointCtx` holds
  `engine: Arc<Engine>` (`:37`) and `reads: Arc<ReadRegistry>` (`:39`), built once at boot and
  shared across requests. There is no per-request driver instance a principal could be bound into.

### Sessions shipped, are exercised in exactly one face, and are deliberately inert

- **t46 shipped.** `crates/session/` is `qfs-session`: `Session`/`SessionId` + expiry, the opaque
  `SessionToken` (hashed at rest), the `SessionStore` trait (`create`/`lookup`/`rotate`/`revoke`),
  cookie format/parse, and `authenticate(cookie_header, store) -> Result<Option<UserId>>`
  (`session/src/auth.rs`).
- **It is called — but never on the query path.** `authenticate()` is called at
  `qfs/src/oauth.rs:232` and `:262` (the OAuth authorization-server / consent face) and nowhere else
  outside tests. Nothing builds a `DecisionContext` from its result. Outside tests, `DecisionContext`
  is constructed only at `qfs/src/shared_connection.rs:167` (from an `actor` string, **not** a
  session) and via `DecisionContext::anonymous()` at `enforce.rs:178`.
- **It is deliberately inert.** `session/src/lib.rs:12-16`: *"This is **AUTHENTICATION STATE
  ONLY**. A session proves who you are; it grants nothing by itself this milestone."*
  `session/src/auth.rs:8-10`: *"**It grants NOTHING.** … The result is deliberately inert."*

### How a principal comes to exist today — precisely

The claim *"qfs has no sign-in"* is **true of the CLI only**, and the precision matters — the
mechanism this mission needs already exists and is exercised:

- **CLI: no sign-in, by design.** `qfs identity` exposes only `whoami` (verified: `qfs identity
  --help`). `signup` is **retired** — `qfs identity signup` is a hard parse error (verified:
  EXIT=2; `cmd/src/lib.rs:2481` — *"the signup verb is RETIRED (ADR 0008 — `qfs init` replaced
  it)"*). `qfs init --help`: *"one operator per OS user, **no password** (your OS login is the
  authentication; the email is an accountability label)"*.
- **A password-bearing user is created by invite redeem.** `qfs invite redeem <token> <email>`,
  password from STDIN (t55 / M5) — creating the local user **and** the `Membership` that carries
  the `identity::Role` label.
- **A full password sign-in + session mint DOES exist — in the OAuth face.** `qfs/src/oauth.rs`
  renders a sign-in form (`:230`), authenticates an existing session or a fresh password sign-in
  (`:260-262`), verifies against the t45 identity store and mints a session (`:460-480`).

**So the gap is the seam, not the mechanism.** qfs can already turn a cookie into a `UserId`, and
does — in exactly one face, which is not the query path.

### Two defects on the identity read-back, recorded here (not fixed by this mission's premise)

Both are the **same silent-wrong-answer family** as the three defect tickets filed alongside this
mission (`20260717180100` / `20260717180200` / `20260717180300`). They sit on the surface this
mission extends, so a ticket that touches that surface should close them rather than build on them:

1. **`qfs identity whoami --json` accepts `--json` and silently ignores it**, emitting prose:
   ```
   $ qfs identity whoami --json
   a@qmu.jp (user 1)
   EXIT=0
   $ qfs --json identity whoami
   a@qmu.jp (user 1)
   EXIT=0
   ```
   The flag is declared (`qfs identity --help` lists `--json`: *"Emit machine-readable JSON instead
   of human output (blueprint §6/§9)"*), accepted at exit 0, and has no effect on either spelling.
   This matters directly to **Experience 5**: the principal answer must be machine-readable, and the
   nearest existing identity read-back silently is not.
2. **`qfs identity --help` asserts three things that are false** (`cmd/src/lib.rs:702-706`, the
   clap doc that renders verbatim into the user-visible help): *"sign up (email + password)"* and
   *"there is local sign-up"* — signup is **retired**; and *"sessions land in t46"* — t46 has
   **shipped**. Commit `91cde7d` corrected two other instances of the stale t46 claim
   (`identity/src/model.rs`, `cmd/src/lib.rs:255`) but did not reach `:705`, which is the
   **user-visible** one.

## Acceptance

Each item is a criterion, not a design. The ticket that satisfies it is cut against the source at
`/ticket` time — **re-verify the *Measured starting state* first; do not paraphrase it into a
ticket.**

- [ ] **"Who am I" is answerable for a request, and its negative is first-class.** A request
      carrying a live session resolves to a named principal; a request with no session resolves to
      an explicit not-signed-in answer — not an error, not a silent fallback to the sole user.
      Demonstrated by a real run with raw exit codes. (#20260719101202-thread-the-request-principal-to-the-scan-seam.md)
- [ ] **The principal is reachable on the path a query takes.** The seam that serves a scan can see
      who is asking, without each driver inventing its own route to the answer and without a
      per-face divergence. The shape of the seam is the ticket's to rule against the source.
      (#20260719101202-thread-the-request-principal-to-the-scan-seam.md)
- [ ] **The policy gate evaluates the resolved actor, not `anonymous()`.** A t57-narrowed
      (`FOR <subject>`) rule contributes when a principal is present. Proved both directions: the
      rule bites with a principal, and the same rule contributes nothing without one.
      (#20260719101202-thread-the-request-principal-to-the-scan-seam.md)
- [ ] **Fail-closed is preserved, pinned by a test that would fail if the default widened.** No
      session ⇒ no user, no roles, no groups, no memberships ⇒ default-deny holds. Threading a
      principal widens nothing. (#20260719101202-thread-the-request-principal-to-the-scan-seam.md)
- [ ] **The answer is readable as data through the one engine, credential-free.** Reached on the
      closed-set convention — never a bespoke side-channel endpoint — and carrying no credential
      column, matching the `/sys/connections` redaction contract. Verified by a `DESCRIBE` run and a
      structural test. (#20260719101202-thread-the-request-principal-to-the-scan-seam.md)
- [ ] **`Role` is still not a grant, and the open decision is still open.** The mission's outcome
      states plainly that `identity::Role` was not converted into an authorization grant, that
      `Role::Admin` is still not privileged, and that the t55-vs-t53 taxonomy and the
      super-admin/project-admin split remain unruled. The flags at `invite.rs:135-139` and
      `sys.rs:24-27` still stand, or are updated only to record a ruling the developer actually
      made. (#20260719101203-role-stays-not-a-grant-and-the-open-decision-stays-open.md)
- [ ] **The identity read-back tells the truth.** `whoami --json` emits machine-readable JSON or
      rejects the flag — it does not accept it and emit prose; and `qfs identity --help`
      (`cmd/src/lib.rs:702-706`) no longer asserts a retired sign-up or a pending t46.
      (#20260719101201-identity-read-back-tells-the-truth.md)
- [ ] **One live round, developer-attended.** A real request with a session and a real request
      without one, end to end, each resolving to its answer through the shipped path — output and
      exit codes pasted. (#20260719101204-one-live-round-developer-attended.md)

## Changelog

- 2026-07-17 — mission created by HQ (strategy repo) from the developer's ruling that the
  request-principal seam is its own mission, independent of ticket
  `20260717141600-declare-sessions-and-roles-as-principals.md` (which stays in `todo/`, blocked).
  `assignee` left **unclaimed**. Goal/Scope/Experience/Acceptance drafted against measured source
  (binary `qfs 0.0.78`, tree at `91cde7d`), not against a summary — mission.md
- 2026-07-17 — Recorded the developer's ruling that **`Role` remains NOT a grant**, and that the
  role classification stays an OPEN PRODUCT DECISION (t55) this mission does not close — mission.md
- 2026-07-17 — Corrected two premises of the strategy plan book's open question
  「第一列は何から導かれるか」 (`docs/plan.md`): 「ロールのモデルが無く」 and 「セッションも未着」 are
  both false — two role models exist and t46 shipped. The gap is that nothing supplies an actor to
  built machinery — mission.md
- 2026-07-17 — Corrected the framing *"qfs has no sign-in"*: true of the **CLI** only. A full
  password sign-in + session mint ships in the OAuth face (`oauth.rs:230,260-262,460-480`), and
  `authenticate()` is called there (`:232,:262`) — never on the query path. The gap is the seam,
  not the mechanism — mission.md
- 2026-07-17 — Recorded two identity read-back defects found by execution (`whoami --json` accepts
  and ignores the flag; `identity --help` asserts a retired sign-up and a pending t46 at
  `cmd/src/lib.rs:702-706`, an instance commit `91cde7d`'s sweep did not reach) — mission.md
- 2026-07-19 — CLAIMED (`assignee: a@qmu.jp`, `drive_authorized: true`) and REPLANNED by a
  `/monitor` drive leaf. Re-verified the Measured starting state against the tree at `cab0197`
  (binary `qfs 0.0.81`): the load-bearing line refs still hold (`exec/src/read.rs:48` scan trait;
  `enforce.rs` `evaluate` → `DecisionContext::anonymous`; `context.rs` DecisionContext;
  `driver-sys/src/schema.rs` closed-set + redaction; `qfs/src/identity.rs` prose whoami;
  `cmd/src/lib.rs:702-706` stale help). Emitted the full 8-item ticket set (4 files): identity
  read-back (item 7); the scan-seam principal thread + `/sys/whoami` + policy-gate wiring
  (items 1-5); Role-stays-not-a-grant outcome (item 6); developer-attended live round (item 8) —
  mission.md
- 2026-07-19 — ticket archived — 20260719101201-identity-read-back-tells-the-truth.md
