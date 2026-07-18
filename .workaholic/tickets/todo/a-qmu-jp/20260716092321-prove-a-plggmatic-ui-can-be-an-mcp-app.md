---
created_at: 2026-07-16T09:23:21+09:00
author: a@qmu.jp
type: enhancement
layer: [UX, Domain, Infrastructure]
effort: 4h
commit_hash:
category: Added
depends_on:
mission: qfs-viewer-mvp
---

> Imported from qmu/plggmatic on 2026-07-16 by HQ triage (strategy mission qfs-viewer-mvp-headquarters); original path .workaholic/tickets/todo/a-qmu-jp/20260716092321-prove-a-plggmatic-ui-can-be-an-mcp-app.md

# labs PoC: prove a plggmatic UI can BE an MCP App (UI resource + server)

## Overview

Make a plggmatic-declared interface consumable as an **MCP App** — the
extension (`io.modelcontextprotocol/ui`, spec `2026-01-26`, Stable) in
which an MCP **server** exposes a `ui://` HTML resource that a **host**
(Claude, ChatGPT, VS Code Copilot) renders in a sandboxed iframe and
talks to over JSON-RPC on `postMessage`.

This is the **third** agent-facing surface over the same Scene, beside
the shipped WebMCP skin and plain MCP — all three live at once, each
for a different consumer. It exists for a named one: Insight Browser's
horizontal documentation browser, embedded in a vendor's chat.

This is a **labs PoC**, not the implementation. The developer chose it
deliberately (2026-07-16): the MCP Apps extension is Stable, but the
base MCP spec `2026-07-28` is still a release candidate, and
`workaholic:planning` / `verify-before-building` says to prove the
uncertain part with real components in `labs/` before implementation
piles up on vague premises. The implementation ticket is written FROM
this PoC's findings, not before them.

## Three surfaces, live at once — not a choice between them

The service offers **MCP, WebMCP and MCP Apps simultaneously**, over one
Scene (developer, 2026-07-16). They are not alternatives and point 8 did
not rule the others out; each reaches a different consumer:

| surface | who ships the UI | who calls the tools | reaches |
|---|---|---|---|
| **MCP** (plain server) | no UI | any agent, over a transport | non-browser agents |
| **WebMCP** (shipped, point 8) | the page IS the UI | a browser agent calls tools the PAGE registered | an agent on the real page (Chrome nightly) |
| **MCP Apps** (this ticket) | the SERVER ships the UI as a resource | the UI calls `tools/call` on the SERVER | a first-party vendor's chat |

The topologies really do differ, and one consequence is load-bearing:
**MCP Apps' UI-side channel is for *calling* tools, not *exposing* them.**
So "point the existing catalog at MCP Apps" is not something the spec
offers — a third `ModelContext` skin over postMessage would be a
category error. For any tool to reach the model here, something must
**serve** `tools/list`, which is exactly the transport
`the-http-mcp-transport-is-deferred` defers. That is why the developer
chose **UI resource + server** (both), and why this PoC is that
concern's trigger.

None of which retires the WebMCP skin: it keeps serving the agent on the
real page, where the catalog IS the right surface. This adds a third
consumer to the same fold.

## The consumer this is actually for

**Insight Browser** (a sibling repository, `../qfs-viewer`) is a
Markdown documentation service that will adopt plggmatic. Its intended
shape is the reason this ticket exists:

- an MCP server answering with Markdown file contents — search / RAG /
  document tools;
- **the same server exposing an MCP App**, so a first-party AI vendor's
  user sees a **horizontally-oriented documentation browser embedded
  directly in their chat history**, documents laid out across the strip;
- WebMCP too, for an agent on the real page.

**Read that twice before touching question 1 below.** The unbounded
horizontal column strip meeting the host's bounded box is not a
collision to be survived — the horizontal browser in the chat is the
PRODUCT. Do not "solve" it by making the strip vertical or by concluding
the reference UI does not belong in a host frame.

**Boundary:** this ticket is plggmatic's work only. Insight Browser is a
different repository; nothing here may write to it, and the integration
is its own repo's work (`/request` is the sanctioned route across that
line). Naming it here is fine — `qmu/plggmatic` is a PRIVATE repo and
Insight Browser is a first-party sibling, not client context.

## Where this lands (all of it already exists)

- `packages/plggmatic/src/Catalog/usecase/catalog.ts` — `catalogOf(scene)`,
  the pure fold from the settled Scene to `ReadonlyArray<Tool>`. **Do not
  touch it.** The whole claim under test is that it is transport-neutral.
- `packages/plggmatic/src/Catalog/model/tool.ts` L12-27 — states that
  JSON-Schema serialization is **the adapter's job**, derived from this
  data. That is the hook `tools/list` needs; it has never been built.
- `packages/plggmatic/src/Catalog/usecase/adapter.ts` — the WebMCP skin.
  Its docstring already calls itself sacrificial: "meant to be rewritten
  without touching the fold". `detectModelContext` (L214-240) is the
  in-repo reference for narrowing an untrusted host object **without an
  escape hatch** — copy that idiom for postMessage payloads.
- `packages/plggmatic/src/Catalog/usecase/runFlow.ts` — `RunOutcome` is
  already `Rejected | Stalled | Ran` as VALUES, never throws. That is
  already tool-result shaped.
- `packages/plggmatic-example/bundle.config.ts` — `target: "app"` inlines
  plggmatic + the plgg family into one self-contained ESM bundle. The
  right primitive for a `ui://` resource.
- `packages/plggmatic-example/src/stamp.ts` L26-34 — copies HTML that
  references an **external** `./demo1.js`. A `ui://` resource must be ONE
  self-contained file, so this step does not exist yet.

**Load-bearing fact the PoC starts from: the WebMCP adapter is dead
code.** `catalogOf`, `syncCatalog`, `detectModelContext`, `invokeWith`,
`flowHost` and `runFlow` are exported from the facade and called
**nowhere** outside `Catalog/` and its own specs. No demo entry wires
them. This PoC is therefore the FIRST real consumer of a seam built and
tested in anticipation of exactly this — expect to find that the seam
was never exercised end-to-end, not that it is broken.

## The contract to build against (verified from the spec, 2026-07-16)

- Extension id `io.modelcontextprotocol/ui`; host declares it in
  `initialize.capabilities.extensions` with
  `mimeTypes: ["text/html;profile=mcp-app"]`.
- Resource `uri: "ui://…"`, `mimeType: "text/html;profile=mcp-app"`,
  delivered by `resources/read` as `text` or base64 `blob`.
- A tool links its UI via `_meta.ui.resourceUri` (+ `visibility`).
- Handshake: UI sends `ui/initialize` → host returns
  `McpUiInitializeResult` (`hostCapabilities`, `hostContext`, `hostInfo`)
  → UI sends `ui/notifications/initialized`.
- UI may call `tools/call`, `resources/read`, `ui/open-link`,
  `ui/message`, `ui/request-display-mode`, `ui/update-model-context`.
- Host notifies `ui/notifications/tool-input` (once),
  `…/tool-input-partial` (0+), `…/tool-result`, `…/tool-cancelled`,
  `…/host-context-changed`, `ui/resource-teardown`.
- Sizing: `HostContext.containerDimensions` is `{height}|{maxHeight}` ×
  `{width}|{maxWidth}`; the UI sends `ui/notifications/size-changed`.
- Sandbox: iframe with at least `allow-scripts allow-same-origin`; CSP
  built by the host from `_meta.ui.csp`
  (`connectDomains`/`resourceDomains`/`frameDomains`/`baseUriDomains`),
  restrictive default if omitted.
- SDKs (Apache 2.0): `@modelcontextprotocol/ext-apps` (app side),
  `…/app-bridge` (host side), `…/server` (server helpers).

## Policies

The hard copies this ticket answers to. Read these before writing code
— several of them constrain the OBVIOUS implementation out of existence
(notably `vendor-neutrality` + the foundation-deps rule, which together
bar the natural move of adding an MCP SDK to the engine).

**企画 / `workaholic:planning`**

- `verify-before-building` — **why this ticket is a PoC at all.** "We
  place a labs directory alongside docs … and build PoC for the
  technical challenges we want to verify using real components." Names
  the failure mode directly: "In development where AI writes most of the
  implementation, implementation piles up quickly and in large
  quantities even on vague premises."
- `ai-native-future` — **the decisive constraint on how much to build.**
  "Do not commit to uncertain formats … For parts we cannot see through,
  we do not fix them to one form, but plan them with a wide margin for
  human observation and intervention, in a state where they can be
  rebuilt later." Also mandates the human-in-the-loop path as a planning
  requirement, not a later feature — which is question 4's basis.
- `accessibility-first` — the agent-reachability policy. "Design UI
  considering AI interfaces as well … we proceed with defining
  read/write tool definitions compatible with WebMCP." Tool names use
  domain vocabulary; tool definitions extend rich typing to the UI
  boundary; reachability is verified, not assumed. WCAG 2.2 AA is the
  floor for anything the host frame renders.
- `terminology` — tool names crossing to the host are domain vocabulary
  readable by AI, developers and business alike. The catalog already
  honours this (`open_menu`, `select`, `filter`, `action`, `jump`,
  `run_flow`); the JSON-Schema serializer must not mangle it.

**設計 / `workaholic:design`**

- `vendor-neutrality` — **the primary gate.** Adopting the MCP Apps spec
  is a new external dependency: "make our own implementation the first
  choice … Only when our own implementation can be clearly judged as
  suboptimal do we rely on external libraries." The applicable basis is
  *Interoperability / protocol compliance*. Requires the four-point log
  entry (see Considerations). "Where to draw the anti-corruption layer
  is based on whether that dependency might be replaced in the future" —
  an RC-stage spec is a high-probability target.
- `defense-in-depth` — **the security core.** The sandboxed host frame
  is a new boundary: "make the most restrictive default the starting
  point … only the paths that need to be opened are made explicit …
  Relaxations are recorded in the PR or ADR that makes them — why, for
  what scope, until when."
- `access-control` — "Define the authorization layer once … Do not
  replicate the check in service functions as a secondary defense."
  `Action.authorize` + permits already exist and already gate the
  catalog; no second MCP-specific check.
- `data-sovereignty` — data minimization at the frame boundary: only the
  descriptors and Scene projection the host actually needs cross into
  it. "Data that is never collected cannot be leaked or misused."
- `sacrificial-architecture` — the host skin is a discardable unit. The
  existing adapter cites this by name: "meant to be rewritten without
  touching the fold".
- `modeless-design` — the tool surface stays composable and mode-free so
  the host agent reaches the whole operation space: "If entry points are
  not constrained by mode, AI can resume midway, proceed multiple
  operations in parallel, or assemble them in an unanticipated order."

**実装 / `workaholic:implementation`**

- `coding-standards` — **binds hardest here.** "`any` — Disables type
  checking at that point. Receive at boundaries with `unknown` and pass
  through a validation function." Every postMessage payload is untrusted
  `unknown`. This is the same rule `CLAUDE.md` states as THE MOST
  IMPORTANT RULE. `detectModelContext` is the in-repo reference for
  doing it without an escape hatch.
- `anti-corruption-structure` — host/JSON-RPC types must not propagate
  into the Catalog fold or the Scene/Msg unions. "Keep it as a thin
  wrapper with no domain logic; limit responsibility to translation and
  delegation."
- `domain-layer-separation` — "HTTP routers, CLIs, queue workers, and
  other entry points are not placed in the package expressing the
  domain. Instead they are placed outside as thin shells." The MCP
  server is an entry point; it lives in `labs/`.
- `type-driven-design` — tool inputs and host messages are closed
  discriminated unions folded with exhaustive `match`.
- `functional-programming` — pure data transforms with a single impure
  edge, the shape the current adapter already takes.

**運用 / `workaholic:operation`**

- `ci-cd` — "Consolidate build, type checking, tests, lint … into a
  single inspection command … Keep the inspection logic in repository
  scripts." `check-all.sh` is that command and must stay green. It is
  also the policy this ticket takes a **recorded exception** to: the
  PoC's verdict is a manual real-host round trip, because "Ground the
  decision to ship not in the fact that the process turned green but in
  the fact that production actually responds as expected" — and for an
  MCP App, the real host IS production. See Quality Gate.

**House rules (`CLAUDE.md` + established practice)**

- No escape hatches (`as` / `any` / `ts-ignore`) — CLAUDE.md's stated
  most important rule.
- Type-driven design, Option/Result, exhaustive `match`.
- Prettier, `printWidth: 50`, per-package — don't hand-pack.
- plgg family from npm as `^version`, not a sibling checkout.
- **foundation-deps** (mission.md:130, dsl-v1-core.md:46-47) and
  **additive-only facade** (enforced in story gates, `facade.spec.ts`) —
  neither is in `CLAUDE.md`; both bind. See Considerations.

## Implementation Steps

1. **`labs/mcp-app/` — a new labs unit** (per `verify-before-building`;
   `labs/` sits alongside `docs/`). It is NOT part of the published
   packages and NOT in `check-all.sh` (see Quality Gate).
2. **Emit one self-contained HTML resource.** Bundle demo1 with
   `target: "app"` and inline the JS into the HTML (today's `stamp.ts`
   only copies a shell referencing an external `.js`). Do not generalise
   the build yet — one file, by whatever means, is enough to learn.
3. **Stand up a minimal MCP server** exposing (a) the `ui://` resource
   and (b) `tools/list` derived from `catalogOf(scene)` via a new
   `ToolInput → JSON-Schema` serializer. **The server lives in `labs/`,
   never in `packages/plggmatic`** — see Considerations.
4. **Bridge the UI side**: a `ModelContext`-shaped implementation that
   speaks JSON-RPC over postMessage, plus the `ui/initialize` handshake.
   Narrow every inbound payload with `isObjLike`/`hasProp`/`isFunc` into
   an `Option`, exactly as `detectModelContext` does. **No `as`, no
   `any`.**
5. **Load it in a real host and drive it.** Record what happens.
6. **Write the findings up** — the four questions below are the
   deliverable. Code that answers them is a means; a findings document
   in `labs/mcp-app/` (or the branch story) is the artifact the
   implementation ticket is written from.

## Quality Gate

**Success is judged by a REAL HOST, not by a harness.** The developer
chose this over an automated host-side harness, and over both
(2026-07-16). Rationale: a harness asserts conformance to *our reading*
of the spec and goes green against our own misunderstanding — which is
precisely how `the-webmcp-payload-shape-is-nominal` happened (the
descriptors are forwarded positionally to a moving proposal, and the
only tested guarantee is the inert path; it has never been verified
against a real host). Do not repeat that.

**The gate:** in a real MCP host (Claude Desktop or equivalent), the
plggmatic UI renders, and a **full round trip completes**: UI →
`tools/call` → server → `ui/notifications/tool-result` → the UI settles
to a new Scene. Recorded with screenshots and the JSON-RPC log, in the
findings doc and the commit message.

**This gate is MANUAL and does NOT join `scripts/check-all.sh`.** That
is a deliberate exception, not an oversight: `check-all.sh` must stay
reproducible on any machine, and a real host is neither installed nor
scriptable there. Anything in `labs/` that CAN be checked mechanically
(the JSON-Schema serializer, the payload narrowing) should carry unit
tests, but the PoC's verdict is the manual round trip. `check-all.sh`
must still pass — this work must not break it.

**The four questions the PoC must answer** (all four chosen by the
developer; each is "answered" only with evidence from the real host,
not an opinion):

1. **HOW does the unbounded column strip live in a bounded box?**
   plggmatic's UI grows horizontally without limit (the recursion trail:
   client → projects → project → client → …, measured at 9 columns and
   still going). MCP Apps hands it `containerDimensions` with
   `maxHeight` / `maxWidth` and expects `ui/notifications/size-changed`.
   **Answer this first — and note the question is HOW, not WHETHER.**
   A horizontal documentation browser inside the chat is the product
   Insight Browser is being built to ship; "it doesn't fit, make it
   vertical" is not an available answer. What must be established: which
   of `{height}` vs `{maxHeight}` and `{width}` vs `{maxWidth}` real
   hosts actually send; whether the host scrolls the frame or expects
   the app to; whether the strip's own horizontal scroll survives inside
   it; and what `size-changed` must report as the trail grows. If a real
   host makes the strip genuinely unusable, that is a finding about the
   HOST worth escalating — not a licence to reshape the reference app.
2. **Does the URL codec survive with no address bar?** plggmatic holds
   that the URL is the single source of truth: every view stage is
   reconstructable from it, `toUrl` reflects it back, and
   `open_menu`/`jump` reconstruct a `Url` via `hrefToUrl`
   (`catalog.ts` L85-90). A sandboxed iframe has no address bar and no
   history to speak of. Does the model hold, degrade, or break?
3. **What is actually IN the server's `tools/list`?** Left open
   deliberately — the developer put this to the PoC rather than deciding
   it up front (2026-07-16). The ticket was first drafted assuming "the
   Tool catalog becomes `tools/list`", and that assumption does not
   survive contact with the use case: the plggmatic app in the iframe is
   a LIVE app that navigates itself by clicks and its own URL state. It
   does not need `open_menu`/`select`/`jump` exposed as MCP tools to do
   that — those exist so an AGENT can drive the UI, which is WebMCP's
   job on the real page. Meanwhile Insight Browser's server tools are
   `search_docs`/`get_doc` over Markdown. Three candidates; the real host
   decides which is workable:

   - **(a) Domain tools + UI state reporting** — `tools/list` is the
     domain's; the UI reports what is on screen via
     `ui/update-model-context` so the model knows the view but steers it
     only by calling domain tools again. Spec-idiomatic; keeps
     URL-as-truth intact; the model cannot say "open the third result".
   - **(b) Domain tools only** — no UI state crosses back. Simplest; the
     chat and the embedded browser stay loosely coupled.
   - **(c) Domain tools + the catalog** — the model can drive the
     embedded browser directly. Richest, and the one that could force
     real engine change: driving UI state from the server fights
     URL-as-truth, since that state lives only in the client's URL
     today.

   Whichever it lands on, the standing claim still under test is that
   the Tool catalog is transport-neutral pure DATA — `catalogOf` should
   not need to change. If it does, that claim is wrong and the
   architecture note in `tool.ts` L12-27 needs correcting: say so
   plainly rather than quietly editing the fold.
4. **Does the confirm survive the host boundary?** The mission's
   recorded decision is that destructive actions are NOT auto-confirmed
   for agents — `runFlow` parks and returns `Stalled` with a
   `ConfirmPrompt`, and the caller must dispatch the confirm explicitly.
   Show what `Stalled` looks like through a host, and that no side door
   opened.

## Considerations

- **`packages/plggmatic` MUST NOT gain an MCP dependency.** The
  foundation-deps rule (mission.md:130, dsl-v1-core.md:46-47) allows the
  engine only the first-party foundation chain (`plgg`, `plgg-view`,
  `plgg-ir-*`); it bars app-layer packages. The mission's own point-8
  decision says the server/auth/actor questions "belong to a hosting
  application, not the engine". The obvious move — `npm i
  @modelcontextprotocol/ext-apps` inside the engine — is exactly what is
  barred. The server and the host binding live in `labs/`; the engine's
  contribution is the `Tool` catalog it already has.
- **This rule is NOT in `CLAUDE.md`.** Neither is the additive-only
  facade rule. Both are enforced in mission docs and story gates, so an
  agent reading only `CLAUDE.md` would miss them. Worth a `CLAUDE.md`
  amendment — out of scope here, but do not let that silence bite this
  ticket.
- **A dependency-decisions entry is required** before any
  `@modelcontextprotocol/*` package is added, even in `labs/`
  (`workaholic:design` / `vendor-neutrality`). Follow
  `docs/dependency-decisions.md`'s existing four-point format: Date /
  **Reason** (name the criterion explicitly — here it is
  *Interoperability / protocol compliance*: "domains where we want to
  ride OSS for the protocol layer needed to communicate accurately with
  other services; self-implementation only increases compatibility
  risk") / **Assessment** (License: Apache 2.0; Reputation; Development
  status; Sustainability) / **Monitoring plan** (the specific structural
  signal: a breaking change to the postMessage/JSON-RPC message shape or
  the resource contract) / **Exit strategy** (name the anti-corruption
  seam: the `Tool` catalog — the fold does not change on a host swap,
  only the adapter does).
- **The sandbox is a new boundary and starts closed**
  (`workaholic:design` / `defense-in-depth`): postMessage origin
  allow-listing is explicit-open, never permissive-default; every
  relaxation is recorded in the PR. Declare the narrowest
  `_meta.ui.csp` that works and say why each domain is there. Data
  minimization applies to what crosses into the frame
  (`data-sovereignty`).
- **Reuse the ONE authorization layer** (`workaholic:design` /
  `access-control`): `Action.authorize` + permits already project into
  the Scene as action legality, and `catalogOf` already withholds
  illegal action tools. Do not add a second MCP-specific check — a
  single authoritative check is what makes this auditable.
- **Do not fix the shape** (`workaholic:planning` / `ai-native-future`):
  "for parts we cannot see through, we do not fix them to one form, but
  plan them with a wide margin for human observation and intervention,
  in a state where they can be rebuilt later." The base spec is an RC
  until 2026-07-28. Keep the skin sacrificial and disposable, like the
  WebMCP one.
- **`dependency/` vs `usecase/` is an unresolved inconsistency.**
  `workaholic:implementation` / `anti-corruption-structure` assigns
  external containment to a `dependency/` layer, but the existing WebMCP
  adapter sits in `Catalog/usecase/`. If any engine-side code lands,
  follow the precedent or justify moving it — do not silently pick one.
- **Related concerns.** `the-http-mcp-transport-is-deferred` (active,
  low) is the closest prior art and this PoC is its trigger: decide at
  the end whether it resolves, supersedes, or stays deferred — a
  judgement for the findings, not an assumption for the start.
  `the-webmcp-payload-shape-is-nominal` (active, low) is the precedent
  for how a sacrificial skin is recorded.
  `run-flow-schema-observes-sync-sources-only` (active) is a live limit
  of the very meta-tool this exposes: Async/Adapter/Dynamic collections
  contribute no field types, so numeric flow scripts fail the static
  check. Expect it to show up in the PoC.
- **A PoC is allowed to report bad news — but not to abandon the
  product.** An honest "this does not work, and here is exactly where it
  breaks" is a real deliverable and beats a forced integration. What is
  NOT open: concluding that the horizontal strip does not belong in a
  host frame. That strip in the chat is what Insight Browser is being
  built to ship, so a host that cannot carry it is a finding about the
  host — escalate it, do not resolve it by reshaping the reference app.
  (An earlier draft of this ticket had that backwards and would have
  authorised exactly the wrong retreat.)
- **The three surfaces must keep agreeing.** The point of folding tools
  from the settled Scene is that the UI and the tools cannot drift.
  With MCP, WebMCP and MCP Apps all live over one Scene, that property
  is now load-bearing in three places at once — anything learned here
  that would make one surface disagree with another is a finding, not
  an implementation detail.
