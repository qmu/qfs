---
created_at: 2026-07-04T14:37:43+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, Infrastructure]
effort: 2h
commit_hash: 944b154
category: Changed
depends_on:
---

# Design blueprint chapter: self-hosting integrations — a driver is data

## Overview

Design (as a blueprint chapter; blueprint-first, implementation tickets cut afterward) the
**generic API integration capability**: hundreds of thousands of web services connectable to qfs
without an ad-hoc Rust driver each. The owner's flow (2026-07-04): an LLM generates an
integration **qfs script**; executing it stores the declarations in the system SQLite database;
when a user connects that source, qfs evaluates the stored declarations to reach the service.
The bar is **self-hosting**: the syntax and semantics must be powerful enough to express Gmail,
GitHub, or Google Drive as scripts — Chatwork must need no Rust at all.

**The essential observation (less is better — almost everything already exists):**

> The binary already contains a generic REST execution engine and already stores evaluable
> declarations in the system DB. What is missing is only the bridge: **lifting the driver
> definition from a Rust struct into definition-layer data.**

Concretely, three shipped facts collapse the problem:

1. **`qfs-driver-http` (t18) is the wire engine.** It already maps the universal verbs onto HTTP
   internally (`SELECT→GET, INSERT→POST, UPSERT→PUT, REMOVE→DELETE` — no HTTP-verb keywords),
   injects auth from a `Secret`, follows pagination, classifies status→error, and decodes via
   codecs. Its own docs state the thesis: *"auth, headers, base URL, and pagination are
   **config** (`RestApiConfig`), not grammar."* The GitHub and Slack drivers are already just
   `RestApiConfig` suppliers over this seam — self-hosting is half-proven in-tree.
2. **The definition layer already stores evaluable declarations as data.** `CREATE
   ENDPOINT/TRIGGER/JOB/VIEW` desugar to system-DB rows the server later evaluates — exactly the
   owner's store-then-evaluate flow. A driver definition is more of the same, not a new
   mechanism.
3. **Typed parameter binding exists (t32).** Endpoints bind route/query/body params as typed
   `Value`s into a pre-parsed query. The same machinery, pointed inward, binds a driver node's
   path parameters into its wire pipeline.

**Therefore: a user-space driver is a set of definition-layer declarations — parameterized,
typed views and write/call mappings over the wire primitive, plus a declarative auth descriptor
— stored as rows, evaluated at connect time.** One genuinely new concept: **parameterized
definition nodes** (a path template whose typed parameters bind into the declaration's body).
Everything else is composition: codecs decode the wire, the type system (blueprint §5/§6) types
and conformance-checks the outputs, ADR-0008's app/account/connection layers hold credentials,
preview/commit gates installation, and §8's path-scoped policies confine it.

Sketch (final shape is the chapter's to decide; `DRIVER`/`OF`/`{param}` details open; zero new
frozen keywords — nouns are contextual idents):

```sql
-- chatwork.qfs — an LLM-generated integration script; installing it is an ordinary
-- preview/commit (the effects are system-DB rows), and connecting evaluates it.

CREATE DRIVER chatwork
  AT 'https://api.chatwork.com/v2'
  AUTH HEADER 'x-chatworktoken'          -- value = the account's secret, by reference only

CREATE TYPE /type/chatwork/room (room_id int PRIMARY KEY, name text NOT NULL, ...)

CREATE VIEW /chatwork/rooms OF /type/chatwork/room AS
  /http/chatwork/rooms |> DECODE json

CREATE VIEW /chatwork/rooms/{room}/messages AS
  /http/chatwork/rooms/{room}/messages |> DECODE json

-- a write mapping: the universal verb on the node lowers to a wire effect (pure rewrite;
-- the /http applier performs it at COMMIT)
CREATE MAP INSERT /chatwork/rooms/{room}/messages AS
  INSERT INTO /http/chatwork/rooms/{room}/messages VALUES (ENCODE json)
```

**Decision points the chapter must settle** (each with rationale and rejected alternatives):

1. **The wire primitive.** One primitive mount is the target of every declared driver
   (today's `/rest`+`RestApiConfig` machinery, re-founded; naming/addressing to decide —
   `/http/<driver>/<path>` scoped by the declaration's `AT` base URL). Its effect surface
   (POST/PUT/DELETE as ordinary effect nodes) and its **confinement rule**: a declared driver's
   pipelines may only address its own declared host(s) — the structural anti-exfiltration
   guarantee (a Gmail-reading script cannot also post to attacker.com).
2. **The auth descriptor.** Declarative schemes as declaration data — `HEADER <name>` /
   `BEARER` / `BASIC` / `OAUTH2 (authorize '<url>' token '<url>' scopes '…')` — riding the
   existing ADR-0008 layers unchanged: the descriptor says *how* the service authenticates;
   `qfs app add` / `qfs account add` / the vault hold *the values* (secrets stay references,
   never in a script — an LLM-generated script is credential-free by construction).
3. **Parameterized nodes.** `{param}` template segments in a declared path, typed, bound into
   the body (the t32 binding machinery pointed inward); what DESCRIBE reports for a
   parameterized family; how a bound read pushes remaining predicates (truthful residual
   unchanged).
4. **Read mappings.** A node is a (parameterized) VIEW over the wire primitive plus codecs
   (`DECODE json |> EXPAND …`); pagination as a small declared descriptor (cursor param +
   next-page field — the engine loops, declarations stay data); rate-limit/retry policy stays
   engine-side.
5. **Write and CALL mappings.** Universal verbs and `CALL driver.action(...)` lower through
   declared mappings to wire effects — a pure Plan rewrite (the purity invariant holds: mappings
   construct plans; only the wire applier performs I/O at COMMIT). Irreversibility is declared
   per mapping (`mail.send`-style gating for e.g. Chatwork message posting if so declared).
6. **Storage, lifecycle, and the two-source registry.** Declarations desugar to system-DB rows
   (the `/server` binding precedent); `CONNECT /chatwork TO chatwork` resolves the driver name
   against **compiled drivers ∪ declared drivers**; install/update/remove are ordinary
   effect-plan writes (previewed, policy-gated, audited). Where rows live (`/sys/drivers`?) and
   how DESCRIBE lists declared drivers.
7. **Conformance and safety.** Declared types over outputs make blueprint §5/§6's
   drift-reconciliation the *conformance test* for a service the binary never compiled
   (declared type vs delivered rows, honestly surfaced); §8 path-scoped policies apply to
   declared drivers identically; the audit ledger records their effects identically; host
   confinement per decision 1.
8. **The self-hosting ratchet.** Gmail/GitHub/GDrive are the acceptance benchmarks, honestly
   tiered: tier 1 (this design) covers the dominant REST shape — JSON, CRUD, cursor pagination,
   header/bearer/OAuth2 auth (Chatwork is fully tier 1; GitHub reads/PRs and most of GDrive
   metadata likely are); named parks for what tier 1 does not cover (batch endpoints, multipart
   uploads, push/watch channels, websockets — Gmail send/attachments partially). Compiled
   drivers remain until a script proves parity for their surface; the ratchet is "a compiled
   driver may be deleted when its script twin passes the conformance suite."
9. **Rejections (record them).** A general-purpose embedded scripting language (JS/WASM
   plugins) — offroading; qfs's declarative surface IS the plugin language, and declarations
   stay verifiable data (the §5 verification story extends to drivers for free). An external
   manifest format (OpenAPI/Prisma-style) as the runtime representation — OpenAPI is an *input
   the LLM reads* when generating a script, never what qfs evaluates. Per-service Rust crates
   as the growth path — the compiled set becomes reference implementations and primitives only.

**The LLM flow, restated on this design:** the integration script is ordinary qfs (one small
grammar the agent already knows); DESCRIBE of the wire primitive + codecs lets the agent iterate
interactively; installing is preview/commit like any write; the conformance check (decision 7)
tells the agent — and the user — whether the generated declarations match the live service.

**Boundary:** design only — this ticket produces the blueprint chapter (revised in place, with
implemented/blueprint status per the blueprint's discipline) and the implementation tickets cut
from it. No grammar, driver, or storage code changes under this ticket.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional layout (applies to all code work)
- `workaholic:implementation` / `policies/coding-standards.md` — style conventions (applies to all code work)
- `workaholic:planning` / `policies/modeling-centric-design.md` — driver/auth/node/mapping modeled as one shared model before code
- `workaholic:planning` / `policies/ai-native-future.md` — the generator and first consumer of integration scripts is an AI agent; the flow must stay observable and interruptible (preview/commit on install)
- `workaholic:design` / `policies/vendor-neutrality.md` — no external manifest format at runtime; the integration boundary stays thin (one wire primitive)
- `workaholic:design` / `policies/defense-in-depth.md` — host confinement, policy scopes, and the irreversible gate are independent layers over declared drivers
- `workaholic:design` / `policies/access-control.md` — the auth descriptor declares mechanisms; grants stay proportionate and separate
- `workaholic:implementation` / `policies/type-driven-design.md` — declared node types are the conformance contract for services the compiler never sees
- `workaholic:implementation` / `policies/functional-programming.md` — mappings are pure plan rewrites; declarations are data; no embedded imperative plugin code
- `workaholic:implementation` / `policies/objective-documentation.md` — the chapter marks tier-1 coverage vs named parks honestly; benchmarks are stated as targets, not capabilities

## Key Files

- `packages/qfs/crates/driver-http/src/lib.rs` - the shipped wire engine: `RestApiConfig`, verb→method mapping, auth injection, pagination following — the thing being lifted to language data
- `packages/qfs/crates/http/src/params.rs` - t32 typed param binding (route→Values) — the parameterized-node machinery pointed inward
- `packages/qfs/crates/parser/src/grammar.rs` - the definition layer (`server_ddl`, contextual-ident nouns, `AT`/`SECRET` clause precedents) a `CREATE DRIVER`/`CREATE MAP` family joins
- `packages/qfs/crates/core/src/ddl/connections.rs` - CREATE CONNECTION declarations — the auth/locator declaration precedent
- `packages/qfs/crates/qfs/src/connections_config.rs` + `crates/qfs/src/sys.rs` - where declarations persist and the `/sys/*` administration surface the driver rows join
- `packages/qfs/crates/driver-github/src/` + `crates/driver-slack/src/` - the in-tree proof that services are RestApiConfig suppliers — the first self-hosting conversion candidates
- `docs/blueprint.md` - §3 definition layer, §5 type system, §6 driver contract, §8 authorization — the chapters this one composes

## Related History

The definition layer has absorbed one noun per round this branch: TABLE (catalog writes), TYPE
(blueprint §5), and now DRIVER/MAP — each time discovering the execution machinery already
existed and only the declaration surface was missing.

- [20260704124825-design-entity-type-system.md](.workaholic/tickets/archive/work-20260703-194046/20260704124825-design-entity-type-system.md) - types-are-sets: the conformance machinery declared drivers inherit
- [20260704110923-path-aware-capability-ddl-authorization.md](.workaholic/tickets/todo/a-qmu-jp/20260704110923-path-aware-capability-ddl-authorization.md) - path-scoped grants: the policy layer that confines declared drivers
- [20260630004110-design-connection-declaration-grammar.md](.workaholic/tickets/archive/work-20260629-110121/20260630004110-design-connection-declaration-grammar.md) - the declaration-grammar precedent (AT/SECRET clauses)

## Implementation Steps

1. Read the shipped wire engine (`driver-http`) and enumerate exactly what `RestApiConfig`
   expresses today (auth schemes, pagination, headers, per-resource verbs) — the tier-1 floor is
   what already executes; the chapter must not promise below-floor.
2. Read the GitHub/Slack `RestApiConfig` suppliers and write the gap list between "expressible
   as config today" and "needed for their full surface" — this becomes the tier-1/parks split
   (decision 8) with evidence, not guesses.
3. Draft the blueprint chapter settling decisions 1–9, biased by less-is-better: every candidate
   addition must first fail to be expressible as composition of the existing registries,
   definition layer, type system, and account layers.
4. Write the Chatwork script end-to-end as the running example, and sketch the Gmail/GitHub
   fragments that prove tier-1 coverage plus the honest parks.
5. Specify install/evaluate semantics precisely: desugar targets, connect-time resolution order
   (compiled ∪ declared), conflict rules, and the confinement/conformance checks' error shapes.
6. Cut implementation tickets from the accepted chapter (grammar nouns; declaration storage;
   wire-primitive effect surface; the evaluator; the conformance harness; the first compiled→
   script conversion as the ratchet's proof).

## Quality Gate

**Acceptance criteria:**

- The blueprint gains the self-hosting-integrations chapter (in place, status-marked), settling
  decisions 1–9 with rationale and a substantive rejections section; the document count does not
  grow (one blueprint, revised).
- The Chatwork example is complete enough that a reader could hand it to an LLM as the target
  shape; every statement in it either parses on the shipped grammar or is marked as a proposed
  additive form citing the contextual-ident / zero-new-frozen-keyword rule.
- The tier-1 floor is evidenced from the shipped `RestApiConfig` (step 1), and the Gmail/GitHub
  parks are named specifically (no "powerful enough" hand-waving).
- Purity, confinement, conformance, and credential-freedom (no secret ever appears in a script)
  are each stated as checkable invariants.
- `cargo test --workspace` remains green (no product code changes).

**Verification method:**

- `cd packages/qfs && cargo test --workspace`; `gen-docs --check`; `gen-skills --check` (all
  unchanged/green); parse-check the example's currently-valid statements; cross-read against
  blueprint §3/§5/§6/§8 for composition consistency.

**Gate:**

- Decisions 1–9 settled, the less-is-better bias visible, workspace green, and the owner
  approves the chapter content at `/drive`. Owner-taste-heavy language design —
  **never auto-approve in night mode**.

## Considerations

- The keyword freeze binds: `DRIVER`/`MAP`/`{param}` syntax must be contextual idents and
  additive clause grammar; `AUTH`/`HEADER`/`BEARER`/`OAUTH2` follow the `AT`/`SECRET` clause
  precedent (`packages/qfs/crates/parser/src/grammar.rs`)
- Host confinement is the load-bearing security property: an LLM-generated script must be
  *structurally unable* to exfiltrate across services — decide it as a hard evaluator rule, not
  a policy default (`packages/qfs/crates/driver-http/src/lib.rs`)
- Connect-time evaluation cost: declared drivers are parsed/planned per process start or cached;
  the system-DB read must not put a network dependency into DESCRIBE (describe stays pure)
- Name collisions between compiled and declared drivers need one deterministic rule (compiled
  wins? declared shadows with a warning? reject?) — decide once
- OAuth2 flows for arbitrary services reuse the existing browser-consent machinery; services
  with nonstandard flows are a named park, not a driver-specific code path
- qfs is experimental: the declaration storage format may hard-break freely; no compat machinery

## Final Report

Development completed as planned. Delivered blueprint §13 "Self-hosting integrations: a driver
is data" (commit `b0cfd21`) settling all nine decision points — the declaration surface
(CREATE DRIVER / parameterized typed views / CREATE MAP / PAGINATE / credential-free AUTH
descriptor), the wire primitive with host confinement as a hard evaluator rule, the /sys/drivers
two-source registry with compiled-wins collision handling, conformance as §5's drift check aimed
outward, the honestly tiered self-hosting ratchet, and the recorded rejections. Owner approved;
implementation was cut as three dependency-ordered tickets (20260704145136 surface →
20260704145137 evaluator → 20260704145138 conformance + Slack twin, commit `c391937`).

### Discovered Insights

- **Insight**: `driver-http`'s own docs already state the whole thesis — "auth, headers, base
  URL, and pagination are config, not grammar" — and `RestApiConfig` is a serde-ready pure DTO.
  **Context**: The design reduced to lifting an existing struct into definition-layer
  declarations; the tier-1 floor is exactly what that struct already executes, so the chapter's
  capability claims are evidence-backed, not aspirational.
- **Insight**: GitHub/Slack do NOT actually consume RestApiConfig today (they have bespoke
  schema/procs/pushdown modules) even though driver-http's docs describe them as config
  suppliers. **Context**: The gap between "described as config-shaped" and "actually bespoke"
  is precisely the parity gap the conversion ticket (20260704145138) must measure — Slack twin
  first; the doc claim should not be taken at face value.
