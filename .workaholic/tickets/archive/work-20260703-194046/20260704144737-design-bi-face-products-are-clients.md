---
created_at: 2026-07-04T14:47:37+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, UX]
effort: 2h
commit_hash: 4cb8102
category: Changed
depends_on:
---

# Design blueprint section: the BI face — products are clients, definitions are already language

## Overview

Design (as a blueprint section; small — this is a layering rule, not a subsystem) where
Redash-shaped capability lives: saved queries over many sources, scheduled refresh, shared
results, visualization, dashboards. The owner's question (2026-07-04): should the Redash-like
screen mechanism live in **qfs itself** or in the **managed service** — leaning toward qfs
staying pure query-execution infrastructure, suspecting that merging Redash-like features into
qfs breaks separation of concerns.

**The design position to record (assessed this session; the section argues it):** the instinct
is right, but the separation line runs deeper than "qfs vs managed service" — it runs between
**query semantics (language) and presentation (product)**. Decomposing Redash shows most of its
*backend* is already qfs language surface, shipped:

| Redash feature | qfs | status |
| -------------- | --- | ------ |
| saved query | `CREATE VIEW` (a named query at a path) | frozen keyword, shipped |
| scheduled refresh | `CREATE MATERIALIZED VIEW` + `CREATE JOB` | shipped |
| parameterized query (`{{param}}`) | endpoint typed param binding (t32); §13's `{param}` | shipped |
| share / publish as API | `CREATE ENDPOINT` + §8 policies | shipped |
| viz config / dashboard definition | **data** (documents stored through qfs) | convention to design |
| the screen (explore, render, collaborate) | a **product face** — a client of qfs | out of core |

So "build query saving/reuse on qfs" needs no new design — it exists. What needs deciding is
the **layering rule** and the few **infra contracts** a BI face consumes.

**Decision points the section must settle:**

1. **The layering rule.** qfs core never grows presentation nouns — `CREATE CHART` /
   `CREATE DASHBOARD` are rejected (visualization is data *about presentation*, not query
   semantics; adding them would grow the grammar for a concern the language does not execute).
   The binary's embedded dashboard stays operator-minimal (approval cards, connections) and
   never becomes a BI tool.
2. **Dogfooding storage.** A BI product stores its dashboard/viz definitions **as qfs data** —
   documents at paths (markdown+frontmatter or JSON via the codec registry, the `.workaholic/`
   pattern) — so the product is a **pure client with zero backend of its own**: queries are
   views, refresh is jobs, sharing is endpoints, persistence is qfs paths. Decide the
   recommended convention (a path layout + document shape), explicitly as convention, not
   grammar.
3. **Where the product lives.** The BI face ships as a separate client (managed service
   feature, or a separate OSS app — either way *outside* the qfs binary); multi-tenant/org/
   collaboration stay internal to the managed host per §8's host model. Record the strategic
   consequence: the BI face is the managed service's own proof that qfs suffices as
   infrastructure — built as a customer of qfs, never a fork.
4. **The infra contracts to design now** (the real work this ticket surfaces):
   (a) a **stable, schema-carrying result envelope** — the JSON shape a chart renders from
   (rows + schema + affected/truncation flags); connects to ticket 20260703150300 ("JSON
   output shapes undocumented") which becomes part of this contract;
   (b) **freshness metadata as data** — a materialized view's `last_run`/staleness readable
   through the language (the "updated 5m ago" primitive);
   (c) **result paging/limits on endpoints** so a dashboard widget can render bounded slices.
5. **Rejections (record them).** `CREATE CHART`/`CREATE DASHBOARD` keywords; a BI UI embedded
   in the qfs binary; a separate metadata database for the BI product (it must store through
   qfs — if qfs is not good enough to be its own product's backend, that is a qfs bug to fix,
   not a reason for a side database).

**Boundary:** design only — a blueprint section (in place, status-marked) deciding the layering
rule and naming the infra contracts; the contracts themselves (result envelope, freshness
surface) become their own implementation tickets cut from the accepted section.

## Discussion

### Revision 1 - 2026-07-04T15:20:00+09:00

**User feedback** (summarized from Japanese): Change the thinking substantially — assume qfs
**has a screen**, for local personal use and hosted server use alike: a phpMyAdmin-like
monitoring/admin console showing qfs configuration and state, and Redash-like analytics in that
same screen is welcome too. The original vision already included an SPA from the start and
"usable as a CLI client AND bootable as a server operated via the screen". Implement the screen
with the **plgg plug-based SPA stack**, integrated with the Rust qfs server. The SPA is **not
embedded in the binary**: when the localhost server starts, a deployed plug-based SPA bundle is
loaded from somewhere; server and plug-client versions must pair, but the un-embedded
composition is desired.

**Ticket updates**: The "managed-only" shipping answer and the "no UI beyond operator-minimal in
qfs" invariant are superseded. §14 is rewritten as **the console face**: one first-party screen
(monitoring + administration + Redash-shaped analytics) that is still *a client over public
surfaces only* (that layering survives), delivered un-embedded via a **fetch → verify → cache →
self-serve** model: the server release pins its paired UI bundle version + integrity hash
(bytes in the binary: a URL + hash, not the UI), downloads and verifies on boot/first access,
serves it from local cache to the browser (same-origin CSP, offline after first fetch), with a
dev/self-host source override. Version pairing is pinned by the server, so skew is structurally
absent; an independent-UI-release "signed channel" is a named park. The existing embedded
approval-cards dashboard retires into the console at parity. Durable-through-qfs storage stays
(ephemeral caches exempt); the three infra contracts stay and gain a first-party consumer.

**Direction change**: from "the BI face ships outside qfs (managed)" to "qfs ships one
first-party console face for every host, loaded not embedded" — the boundary that survives is
public-surfaces-only, not where the screen lives.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional layout (applies to all code work)
- `workaholic:implementation` / `policies/coding-standards.md` — style conventions (applies to all code work)
- `workaholic:design` / `policies/modeless-design.md` — the BI face is composition over the same statements, never a mode; everything it does stays reachable one-shot
- `workaholic:design` / `policies/modular-monolith-first.md` — the split is a **client**, not a service mesh: qfs stays one binary, the BI face is a separate deployable client
- `workaholic:design` / `policies/vendor-neutrality.md` — the BI face consumes public qfs surfaces (endpoints/MCP), so any third party could build the same product; no privileged private API
- `workaholic:planning` / `policies/it-investment-evaluation.md` — the managed-service framing: the BI face is product investment on top of infra, evaluated as such
- `workaholic:implementation` / `policies/objective-documentation.md` — the section records what ships vs what is product intent; no capability claims for the unbuilt face

## Key Files

- `docs/blueprint.md` - §3 (definition layer: VIEW/JOB/ENDPOINT), §8 (hosts; managed-internal concerns), §10 (server bindings), §13 (params/conformance) — the chapters this section composes
- `packages/qfs/crates/http/src/params.rs` - t32 typed param binding — parameterized saved queries already work
- `packages/qfs/crates/qfs/src/dashboard.rs` - the embedded operator dashboard whose scope boundary this section fixes
- `.workaholic/tickets/todo/a-qmu-jp/20260703150300-agent-facing-doc-gaps.md` - the JSON output-shape gap that becomes the result-envelope contract

## Related History

- [20260704143743-design-self-hosting-integrations.md](.workaholic/tickets/todo/a-qmu-jp/20260704143743-design-self-hosting-integrations.md) - §13 (pending approval): the same pattern — capability as data over existing machinery
- [20260704124825-design-entity-type-system.md](.workaholic/tickets/archive/work-20260703-194046/20260704124825-design-entity-type-system.md) - types-are-sets: the schema the result envelope carries
- ADR 0008 (absorbed into blueprint §8) - hosts model; multi-tenant/org stay managed-internal

## Implementation Steps

1. Verify the mapping table against the shipped surface (VIEW/MATERIALIZED VIEW/JOB/ENDPOINT
   semantics, t32 params, the current `-json` output shape and dashboard scope) so the section
   claims only what runs.
2. Draft the blueprint section (likely inside or adjacent to §9 CLI & console / §10 Server as
   "faces are clients" — placement decided while drafting; the document count stays one)
   settling decisions 1–5.
3. Specify the result-envelope contract sketch (rows + schema + truncation/affected metadata)
   precisely enough that ticket 20260703150300 can implement against it, and the freshness
   surface (where `last_run` reads from).
4. Cut the implementation tickets (result envelope; freshness metadata; endpoint paging) from
   the accepted section.

## Quality Gate

**Acceptance criteria:**

- The blueprint gains the section (in place, status-marked; document count unchanged), settling
  decisions 1–5 with the mapping table evidenced against shipped surface and a substantive
  rejections list.
- The layering rule is stated as a checkable invariant: no presentation noun in the grammar, no
  BI UI in the binary, no side database for a qfs-based BI product.
- The result-envelope and freshness contracts are specified to implementable precision, each
  with its follow-up ticket named.
- `cargo test --workspace` remains green (no product code changes).

**Verification method:**

- `cd packages/qfs && cargo test --workspace`; `gen-docs --check`; `gen-skills --check` (all
  unchanged/green); cross-read against blueprint §3/§8/§10/§13.

**Gate:**

- Decisions settled, workspace green, owner approves the section content at `/drive`.
  Owner-taste-heavy product/infrastructure boundary — **never auto-approve in night mode**.

## Considerations

- The strongest argument to record: Redash needs its own backend because SQL engines are not
  filesystems; qfs IS one, so the BI product's entire persistence is qfs paths — dogfooding as
  architecture proof, not slogan
- The embedded dashboard already exists (approval cards); the section must draw its scope line
  without breaking the shipped operator surface (`packages/qfs/crates/qfs/src/dashboard.rs`)
- Result-envelope stability becomes a versioned-surface question (§12): decide whether the JSON
  envelope joins the SemVer surface
- qfs is experimental: the envelope may hard-break while it settles; no compat machinery

## Final Report

Development completed through one owner-directed revision. First draft (blueprint §14 v1,
`19a41d7`) placed the BI face outside qfs entirely; owner feedback redirected: qfs **has a
screen** for local and hosted use alike (phpMyAdmin-analog monitoring + admin + Redash-shaped
analytics), built on the plgg plug-based SPA stack, loaded at runtime rather than embedded.
The revised §14 (`6005ad6`) keeps the surviving boundary — the console is a client over public
surfaces only, no presentation nouns in the grammar, durable-through-qfs storage — and adds the
delivery design: fetch → verify → cache → self-serve with a release-pinned bundle coordinate,
same-origin CSP, offline-after-fetch, an explicit dev/self-host override, and structurally
absent version skew. Owner approved; implementation cut as: the result envelope absorbed into
ticket 20260703150300, freshness+paging (20260704152639), and console bundle delivery
(20260704152640).

### Discovered Insights

- **Insight**: The hosts model (§8, ADR 0008) absorbed the redirect with zero friction — "the
  console is a client of a host, and local is just another host" resolves local-vs-managed
  screen placement without a fork. **Context**: Design changes that look large can land small
  when the host abstraction already carries them; check §8 first for any "where does X live"
  question.
- **Insight**: For a control plane holding live credentials, browser-loads-from-CDN is the
  wrong default; fetch-verify-cache-self-serve gives un-embedded delivery AND a same-origin
  security posture, and the server-pinned pairing eliminates version skew as a class.
  **Context**: Reuse this delivery pattern for any future runtime-loaded asset (e.g. console
  plugs, §14's park).
