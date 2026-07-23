---
type: Mission
title: plggmatic screen-structure model semantics
slug: plggmatic-screen-structure-model-semantics
status: active
created_at: 2026-07-09T11:55:55+09:00
author: a@qmu.jp
tickets: [20260711205500-settle-engine-home-and-resume-design-plan.md, 20260712004100-absorb-the-declare-schedule-engine-into-plggmatic.md, 20260712004200-settle-manifest-lowering-and-view-params-model-semantics.md, 20260712013000-refound-scheduler-on-view-typed-params-core.md, 20260712013100-derive-typed-fields-and-query-semantics-from-manifest.md, 20260712020000-implement-the-fieldvalue-union-and-reference-jump.md, 20260712021000-implement-declared-typed-query-fields.md, 20260712111200-add-a-board-level-for-dashboard-screens.md, 20260712141100-freeze-the-dsl-v1-core-on-the-plgg-ir-family.md, 20260712141200-specify-the-capability-binding-contract.md, 20260712141300-design-the-webmcp-adapter-boundary.md, 20260712141400-prototype-the-pausable-interpreter-settle-loop-handshake.md, 20260713110000-implement-the-flow-static-layer.md]
stories: []
concerns: []
---

> Imported from qmu/plggmatic on 2026-07-16 by HQ triage (strategy mission qfs-viewer-mvp-headquarters); original path .workaholic/missions/active/plggmatic-screen-structure-model-semantics/mission.md

# plggmatic screen-structure model semantics

## Goal

plggmatic is being repositioned (and extracted toward its own repository) as a
**UI design system generatable by an LLM through a condensed semantic DSL**,
whose user interface is **WebMCP-native**: at every settled scene, a browser
agent can discover the currently-legal operations as MCP tools, and the tool
surface changes incrementally as the user (or agent) drills through the UI.
The same DSL that declares the UI schema must also be expressive enough to
*manipulate* the generated UI — a whole flow ("open Clients → choose Search →
set conditions → read results") written as one condensed script, executed by
an embedded runtime.

Before any of that can be specified, the **semantics of the model that
defines screen structure** must be settled: what the declaration vocabulary
can express, what its runtime state means, and where the vocabulary must grow.
This mission records those design discussions and their decisions, and tracks
the tickets that realize them.

### Where the design stands (discussion of 2026-07-09)

The current engine (`packages/plgg-ui/src/Declare` + `Schedule`, re-exported
by the `plggmatic` facade) is a three-tier structure, one pure function
connecting each tier to the next:

1. **`Declaration` — the data model as static vocabulary.**
   `Declaration { title, menu, collections }`; `MenuEntry` names root
   collections; each `Collection { id, title, source, child, query, actions }`
   points to at most one `child`, so the navigable structure is a *forest of
   linear drill chains* — the flow graph is implicit, there is no screen or
   route type. `Row { id, label, fields }` is the erasure seam: a typed `T`
   never survives past the collection's `toRow`. `Source` is a three-variant
   seam (`Sync` / `Async` / `Dynamic`); sources read against a `Path` of
   ancestor selection ids. `Action { id, label, verb, confirm, run }` carries
   closed `Verb` (`create|update|delete`) and `Confirm`
   (`Immediate | Confirm{prompt, destructive}`) semantics as data.

2. **`Model` + `SchedulerMsg` — the user's behavior.**
   The whole session is six fields:
   `{ base, root, path, query, slots, pending }` — ids only, never domain
   objects, never derived facts ("which screen is focused" = `path.length`).
   Load state is a closed `Slot` union (`Idle|Loading|Loaded|Failed`).
   Everything a user can do is the closed `SchedulerMsg` union (`OpenMenu`,
   `Select{level,id}`, `QueryInput`, `RequestAction`, `Confirm/CancelAction`,
   `UrlChanged`, `Loaded/Failed`). `schedule(declaration)` derives a pure TEA
   program; effects return as `Cmd` data. A session is a fold over a Msg
   list; the Model↔URL codec is total both ways, so every reachable state is
   a shareable address.

3. **`Scene` — the observed behavior.**
   `scene: Model → Scene { title, levels, confirm }` with
   `Level = MenuLevel | ListLevel | DetailLevel`; a Level is *depth*, not a
   column or screen (mode-agnostic, D10). Renderers are folds over this
   closed union.

Key conclusions reached so far:

- **The automation vocabulary already exists**: the example flow maps 1:1
  onto `SchedulerMsg`. A DSL script is forms evaluating to Msgs interleaved
  with reads of the settled Scene — no new automation model is needed.
- **WebMCP is a third renderer**: the tool manifest is a fold over `Scene`
  (legal Msgs → tools, input schemas derived from the Declaration), diffed
  and re-registered per model change, so the MCP surface can never drift
  from the visible UI. One meta-tool (`run_flow`) accepts DSL text for
  coarse-grained agent operation.
- **The DSL should be an EDN/Clojure-shaped Lisp with plgg's discipline**:
  no nil (Option/Result literals in the core), no exceptions, exhaustive
  `match`, no ambient I/O (capability-based effects), fuel-metered total
  evaluation, no user macros in v1. Reader built on `plgg-parser`;
  single-digit-KB embeddable runtime is realistic.
- **Recommended execution semantics**: a pausable small-step interpreter
  that yields effects to the scheduler (TEA-native); a paused script is a
  serializable value. To be prototyped first.
- **The serialization gap is exactly four function-valued leaves**
  (`Source.Sync`, `Source.Async`, `Action.run`, `toRow`); everything else in
  the system is inert data. The DSL names capabilities the host binds;
  `toRow` becomes a keyword-projection map in the common case.
- **Known expressiveness limits** of the current vocabulary (the pressure
  points an LLM-generated CMS will hit first): (1) linear drill only —
  `child` is a single optional pointer, no branching or cross-links;
  (2) no form/payload model — `Action.run` takes only an optional target id;
  (3) flat presentation — `Field` is label/value strings, no typed fields or
  widgets; (4) fixed query semantics — one substring predicate over
  `Row.label`. Each is an extension to `Declaration`, not the scheduler.

### Repository landscape (recorded at migration, 2026-07-11)

This mission was created in the `qmu/plgg` monorepo on 2026-07-09 and migrated
here on 2026-07-11, since this repository is where plggmatic's design-system
and DSL/WebMCP work is driven. Facts a resuming session needs:

- **The engine source no longer lives at `packages/plgg-ui`.** On 2026-07-10
  the plgg monorepo retired the `plgg-ui` package boundary (`plgg` commit
  `807a422c`) and moved the UI runtime — the `Declare` + `Schedule` tiers this
  mission describes — into `packages/plgg-cms/src/ui/` there. The npm artifact
  `plgg-ui@0.1.1` (published 2026-07-09) is a frozen snapshot of the
  pre-retirement source; this repo's `packages/plggmatic` facade re-exports it.
  **Settling the engine's development home (re-home into this repo vs. keep in
  plgg-cms and republish) is now the first blocking decision** — every
  acceptance item below lands code or specs against that engine.
- **Related missions in `qmu/plgg`:**
  - `build-the-plgg-ir-package-family` — a planned `plgg-ir-syntax` /
    `plgg-ir-language` / `plgg-ir-manifest` toolchain for restricted, typed,
    statically verified S-expression IRs. Discussion point 6 below (DSL v1
    core freeze) should evaluate building the DSL reader/checker on that
    family instead of a parallel plgg-parser stack; the division of labor is
    plgg-ir = generic language toolchain (in plgg), this mission = plggmatic's
    dialect and runtime semantics.
  - `plggmatic-ai-native-ui-toward-a-dsl` — the north-star concept mission;
    its two remaining acceptance items ("DSL distilled", "WebMCP-operable
    generated UI") are satisfied by this mission's outcomes and will be
    checked off from there when this mission delivers them.

### Decisions (2026-07-12)

- **Engine home settled — re-homed into this repo, absorbed into
  `packages/plggmatic`.** The engine depends only on the foundation libraries
  (`plgg`, `plgg-view`); plgg-cms is explicitly unrelated — plggmatic carries
  no publish/coordination obligation toward it. The frozen `plgg-ui@0.1.1`
  snapshot (whose surface the facade already re-exports byte-for-byte) is the
  source of record for the code move; `../plgg/packages/plgg-cms/src/ui`
  stays reference-only (its extraction cost was verified low: 113 TS files,
  zero imports from outside `ui/`). Extraction ticket:
  20260712004100-absorb-the-declare-schedule-engine-into-plggmatic.md.
- **Flow-graph expressiveness (point 1) — answered by re-founding on
  plgg-ir.** plggmatic processes a canonical **plgg-ir Domain Manifest** on
  demand to build the UI (see `build-the-plgg-ir-package-family` in qmu/plgg,
  design.md §7/§11/§12/§15/§37). The flow graph is the manifest's
  view/navigation graph — `(navigate (to <view>) (with (<typed params>)))`
  over views whose branching arises from entity `relation`s — an arbitrary
  directed graph of semantic views, not a grown `child` pointer. Screen state
  is `(view, typed parameter bindings)`, not a walk, so the URL codec derives
  from it and is canonical (one state = one URL) **by construction**; the
  current linear drill chain survives only as a special case the lowering can
  target. Consequences rippling into the other points: form/payload (2) =
  manifest `action` typed input; typed fields (3) = manifest field types;
  query (4) = manifest query/projection scope; DSL v1 (6) = plgg-ir-manifest
  as the core, plggmatic adding a UI dialect via dialect composition;
  capability binding (7) collapses toward one persistence/effect adapter;
  WebMCP (8) = fold over the settled view's legal navigations + actions. The
  **lowering direction** (compile the manifest onto an extended Declaration
  vocabulary vs re-found the scheduler on view/params semantics) is the next
  decision:
  20260712004200-settle-manifest-lowering-and-view-params-model-semantics.md.

- **Lowering direction settled (2026-07-12) — re-found the scheduler on
  (view, typed params); the legacy vocabulary becomes a lowering.** The
  Schedule tier's state is rebuilt as `Model = {view, params, slots,
  pending}` with `Navigate{to, with}` as the core message (`OpenMenu` /
  `Select` survive as special cases); the URL codec derives from
  (view, params) and is canonical by construction. The three-tier shape
  (declaration → schedule → Scene) survives; renderers largely carry over
  (the drill chain degenerates to one derivable pattern). Inversion instead
  of compilation-onto: the existing `Declaration` vocabulary is re-expressed
  as a lowering INTO the new core (collection → list view + detail view,
  `child` → navigation), so current consumers keep compiling and the plgg-ir
  manifest lowers onto the same core — no double implementation, no
  impedance mismatch. Rebuild ticket:
  20260712013000-refound-scheduler-on-view-typed-params-core.md.
- **Form/payload semantics settled (point 2, 2026-07-12) — manifest typed
  input is the single source.** An action's `(input (field … (type …)
  (validate …)))` lowers to a `FormSpec` (data); the Model holds per-pending-
  action `drafts` (field name → SoftStr) and `FormErrors`; `update` stays
  pure — input Msgs write drafts, submit runs a caster fold derived from the
  manifest types (`Ok(payload)` → effect Cmd, `Err` → FormErrors). Scene
  projects a form level whose control kinds derive from field types (feeds
  point 3), and the WebMCP tool input schema derives from the same typed
  input spec, so the UI and agent surfaces cannot drift. The existing Form
  tier (`FieldSpec`/`parseForm`/`formView`) is the seed: `FieldSpec` becomes
  derivable from manifest input fields.

### What needs to be discussed

The open design questions this mission exists to settle, roughly in order:

1. **Flow-graph expressiveness** — should `child` grow into branching
   children / cross-chain links, and what do `Path`, the URL codec, and
   `Select{level,id}` mean once the drill graph is no longer linear?
2. **Form/payload semantics** — a declarative input model for actions
   (create/update payloads) that stays data, keeps `update` pure, and folds
   into Scene/WebMCP tool schemas the same way rows do.
3. **Typed fields / richer `Row`** — whether `Field` grows a closed value
   union (text, number, date, reference, media…) and what renderers and
   tool schemas derive from it.
4. **Query semantics** — whether Query stays one substring predicate or
   becomes a declared, serializable predicate language (and its relation to
   the DSL).
5. **Scene/Level semantics beyond drill depth** — does the mode-agnostic
   Level stack suffice for the screen structures a design system must
   express (dashboards, side-by-side comparisons, wizards), or does the
   projection vocabulary need new closed variants?
6. **DSL v1 core freeze** — special forms, literals (Option/Result), host
   function set (~30), reader grammar on plgg-parser, fuel semantics.
7. **Capability-binding contract** — how the host names and binds the four
   function seams (`(source :db/clients)`-style references), including the
   keyword-projection shorthand for `toRow`.
8. **WebMCP adapter boundary** — engine-owned `Tool` type with thin
   adapters to `navigator.modelContext` (moving proposal) and to plain MCP
   over HTTP (reusing `plgg-mcp`), so non-browser agents get the identical
   surface.
9. **Execution-model prototype** — prove the pausable small-step
   interpreter ↔ settle-loop handshake (dispatch Msg → settle → resume with
   Scene) before freezing flow semantics.

## Scope

**In scope:** design discussions and decisions on the declaration/model/scene
semantics listed above; DSL language specification (v1 core); the WebMCP
tool-manifest projection design; prototype tickets that validate the
execution model; extensions to `Declaration` (flow graph, forms, typed
fields, query) driven by those decisions.

**Out of scope:** the plggpress → plgg-cms package spread and the plggmatic
repository extraction mechanics (handled by the `plggmatic-extraction-cut`
trip and its tickets); production hardening of any WebMCP browser API
integration while the proposal is still moving; visual theme/design-token
work in `plgg-ui/Style`.

**Done when:** each numbered discussion point above has a recorded decision
(in this mission's changelog and/or a design ticket), and the acceptance
checklist below is fully checked.

## Acceptance

- [x] Engine development home settled — where the `Declare`/`Schedule` source
      lives going forward and how this repo consumes it
      (#20260711205500-settle-engine-home-and-resume-design-plan.md)
- [x] Flow-graph expressiveness decision recorded — branching children /
      cross-links, and the resulting `Path`/URL/`Select` semantics
      (#20260711205500-settle-engine-home-and-resume-design-plan.md)
- [x] Form/payload semantics for actions designed as declaration data
      (#20260712004200-settle-manifest-lowering-and-view-params-model-semantics.md)
- [x] Typed-field / `Row` enrichment decision recorded
      (#20260712013100-derive-typed-fields-and-query-semantics-from-manifest.md)
- [x] Query-semantics decision recorded — fixed predicate vs declared
      predicate language
      (#20260712013100-derive-typed-fields-and-query-semantics-from-manifest.md)
- [x] Scene/Level vocabulary reviewed against target screen structures;
      extension decision recorded
      (#20260712111200-add-a-board-level-for-dashboard-screens.md)
- [x] DSL v1 core specification frozen — forms, literals, host functions,
      reader grammar, fuel semantics
      (#20260712141100-freeze-the-dsl-v1-core-on-the-plgg-ir-family.md)
- [x] Capability-binding contract specified for the four function seams
      (#20260712141200-specify-the-capability-binding-contract.md)
- [x] WebMCP adapter boundary designed — engine `Tool` type + WebMCP/MCP
      adapters
      (#20260712141300-design-the-webmcp-adapter-boundary.md)
- [x] Pausable-interpreter ↔ settle-loop handshake proven by a runnable
      prototype
      (#20260712141400-prototype-the-pausable-interpreter-settle-loop-handshake.md)

## Changelog

- 2026-07-09 — Mission created from the plggmatic DSL/WebMCP design
  discussion: current three-tier model (Declaration/Model+Msg/Scene)
  documented, key conclusions recorded (Msgs as automation vocabulary,
  WebMCP as a third renderer, EDN-shaped fuel-metered Lisp, four function
  seams as the serialization gap), and the nine open discussion points
  registered.
- 2026-07-11 — mission migrated from qmu/plgg; repository landscape and engine-home blocker recorded; resumption ticket filed — 20260711205500-settle-engine-home-and-resume-design-plan.md
- 2026-07-12 — engine home settled: absorbed into `packages/plggmatic` in this repo, foundation deps only (plgg/plgg-view), plgg-cms unrelated; extraction ticket filed — 20260712004100-absorb-the-declare-schedule-engine-into-plggmatic.md
- 2026-07-12 — flow-graph decision (point 1): mission re-founded on plgg-ir — plggmatic interprets a canonical Domain Manifest on demand, flow graph = the manifest view/navigation graph, screen state = (view, typed params), URL codec canonical by construction; lowering-direction ticket filed — 20260712004200-settle-manifest-lowering-and-view-params-model-semantics.md
- 2026-07-12 — engine absorption landed (commits 4855037 + e1173bd): 111 engine files absorbed into packages/plggmatic from the plgg-ui@0.1.1 snapshot (recovered at plgg 807a422c^, zero functional drift vs plgg-cms/src/ui), plgg-ui dependency retired, dist surface machine-verified identical (root 83 / style 117 exports) — 20260712004100-absorb-the-declare-schedule-engine-into-plggmatic.md
- 2026-07-12 — lowering direction settled: scheduler re-founded on (view, typed params) with Navigate{to, with} as the core Msg; legacy Declaration vocabulary re-expressed as a lowering into the new core (inversion, not compilation-onto); rebuild ticket filed — 20260712013000-refound-scheduler-on-view-typed-params-core.md
- 2026-07-12 — form/payload decision (point 2): manifest typed input as single source — action input lowers to FormSpec data, Model holds drafts + FormErrors, update stays pure via caster fold, Scene form level and WebMCP tool input schemas derive from the same spec; typed-fields/query design ticket filed — 20260712013100-derive-typed-fields-and-query-semantics-from-manifest.md
- 2026-07-12 — navigation core landed (commit 012c7e0): View vocabulary + Navigate{to, with} core Msg, one goto funnel for every navigation message, focusedView/sliceOf as the total legacy bridge, scene re-expressed over the derived view; surface additive 83 → 93. Refinement over the ticket's literal target shape: the focused view is DERIVED, never stored (the engine's own no-derived-facts tenet), so the stored Model keeps the legacy slice encoding until the manifest dialect generalizes the params — 20260712013000-refound-scheduler-on-view-typed-params-core.md
- 2026-07-12 — typed-field decision (point 3): `Field.value` grows a CLOSED `FieldValue` union — Text / Num{value, unit} / Flag / Moment / Reference{binding, label} / Media — derived from manifest field types (string/number/money/boolean/date/relation/media); `Reference` carries a `Binding` and its activation dispatches `Navigate` to the target's canonical address (the flow-graph cross-link jump realized at the field seam); legacy `field(label, string)` lowers to Text so existing declarations keep working; renderers fold the union exhaustively and WebMCP field schemas derive from the same types. Implementation ticket filed — 20260712020000-implement-the-fieldvalue-union-and-reference-jump.md. Query semantics (point 4) remains open on the same design ticket.
- 2026-07-12 — query decision (point 4): a list view's query becomes a DECLARED set of typed query fields (e.g. keyword: Text, status: choice) whose state is part of the view's typed params and serializes into the URL (shareable, canonical); client-side evaluation is limited to a closed pair — substring for text, equality for choices — while predicate EXPRESSIONS (ranges, boolean composition) belong to the manifest/DSL (point 6) and execute at the source; the current label-substring query survives as the degenerate one-keyword-field declaration. Implementation ticket filed — 20260712021000-implement-declared-typed-query-fields.md
- 2026-07-12 — typed fields landed (commit 188020a): closed FieldValue union on Row with the field(label, string) Text lowering, scene-resolved Reference hrefs (jump to the canonical address), exhaustive renderer fold with class hooks, surface additive 93 → 108; Demo 1's Project.Client is a live Reference — 20260712020000-implement-the-fieldvalue-union-and-reference-jump.md
- 2026-07-12 — declared typed query fields landed (commit f56bbce): QueryChoice declarations with the keyword-only lowering, chosen values in the model and the URL (canonical, junk-dropping, deep-linkable), closed client-side evaluation (substring + equality), controlled selects in the shared query part; Demo 1 declares status choices; surface additive 108 → 112 — 20260712021000-implement-declared-typed-query-fields.md
- 2026-07-12 — Scene/Level decision (point 5): the `Level` union stays CLOSED and grows deliberate variants as target screens demand them (renderers are forced by the exhaustive fold), rejecting a layout-data generalization for now — that generalization is deferred to the manifest layout semantics and rides with the lowering work (points 6+). First variant: a board (tile) level for dashboard-shaped screens, grounded in Demo 1's Dashboard, with tiles carrying an optional jump href (reusing the Reference/Navigate machinery). Implementation ticket filed — 20260712111200-add-a-board-level-for-dashboard-screens.md
- 2026-07-12 — ticket archived — 20260712111200-add-a-board-level-for-dashboard-screens.md
- 2026-07-13 — point-8 decision COMPLETE (transports + run_flow, 3 of 3 topics): v1 ships ONE thin transport — the in-browser WebMCP adapter over `navigator.modelContext` — kept deliberately thin and disposable while the proposal keeps moving; the plain-MCP-over-HTTP transport for non-browser agents is DEFERRED (no plgg-mcp package exists — the old "reuse plgg-mcp" note was stale), with the boundary already guaranteed: the Tool catalog is transport-neutral pure data, so an HTTP skin is additive later and its server/auth/actor questions belong to a hosting application, not the engine. `run_flow` is a standing catalog entry composing only decided machinery: read → static check (plgg-ir) → fuel-bounded execution (point 9), results and positioned failures returned as values. Destructive confirmations are NOT bypassed for agents: a flow containing a delete must dispatch the confirm step explicitly (the confirm is itself a catalog operation) — the agent walks the same road a human does, no auto-confirm side door. Acceptance point 8 ticked — ALL 12 mission discussion points now have recorded decisions — 20260712141300-design-the-webmcp-adapter-boundary.md
- 2026-07-13 — point-8 partial decision (catalog lifecycle, 2 of 3 topics): the tool catalog rebuilds only at SETTLED scenes (loading collections contribute no tools until loaded — an agent must not read "no rows" from a still-loading list, matching the human's disabled loading state); the fold produces the FULL catalog every settle and the outer adapter diffs by tool name, re-registering only changed entries (the virtual-DOM division of labor: pure full projection inside, minimal mutation outside); the catalog fold lives beside the HTML renderers as the third renderer, subscribed to the same model changes — 20260712141300-design-the-webmcp-adapter-boundary.md
- 2026-07-13 — point-8 partial decision (Tool shape + selection args, 1 of 3 topics): the engine owns a `Tool` value — name, description, an input-schema as engine DATA (JSON-Schema serialization is the outer adapter's job, exactly as renderers emit Html values not strings), and the lowering of validated args onto a `SchedulerMsg`. Tools derive from the settled Scene the way HTML does (menu entries → open_menu, declared query fields → filter tools, rows → select, create actions → typed-input tools, plus the standing run_flow), argument vocabularies reuse the existing typed sources (point-4 query fields, point-2 manifest input, point-7 legality gate — no new schema language). Row-selection arguments are an ENUM of the currently visible row ids, capped at 50 (beyond the cap the tool withholds choices and directs the agent to filter first — never a free-string fallback, one behavior only): a hallucinated id is structurally impossible, the tool doubles as discovery, and the agent obeys the same visibility constraint a human does — the mission's UI/agent-parity promise implemented literally — 20260712141300-design-the-webmcp-adapter-boundary.md
- 2026-07-13 — point-7 decision COMPLETE (failure + authorization, 3 of 3 topics; delegated by the developer): every failure is a value on the existing machinery — bind-time (unknown adapter name) fails at startup with a positioned diagnostic; a runtime `read` Err lands in the collection's `Failed` slot exactly as async reads do today; an `apply` Err surfaces through the action result path (and, once point-2 forms land, validation-shaped errors become FormErrors); nothing throws, timeouts are the adapter's own concern. Authorization: the host supplies the ACTOR as data at program start; the ENGINE evaluates the manifest's declared `authorize` policies over (actor, subject) BEFORE dispatching `apply` — an unauthorized action never reaches the adapter and the Scene can project legality (hide/disable, and the same fold feeds WebMCP tool visibility in point 8); adapters MAY re-enforce at the data layer (defense-in-depth encouraged, not required). Deny-by-default is inherited from the manifest's static checks. Acceptance point 7 ticked; implementation follow-up filed — 20260712141200-specify-the-capability-binding-contract.md
- 2026-07-13 — point-7 partial decision (naming, 2 of 3 topics): the declaration names adapters only when there are several — a host registering ONE adapter makes it the default and declarations write no `source` at all (the common single-backend case has zero connection vocabulary for the LLM to get wrong); multiple backends register named adapters (`:crm`, `:billing`) and only non-default collections name theirs. Bindings are reconciled at STARTUP: an unregistered name is a positioned declaration-time error, an unused registration a warning. One read always completes within ONE adapter — a view's include/projection touching an entity owned by a different adapter is a static error in v1; cross-backend federation is the host's business inside a single adapter's read. `Reference` jumps stay free (id + label only; the read runs on the target's own adapter after the jump) — 20260712141200-specify-the-capability-binding-contract.md
- 2026-07-13 — point-7 partial decision (adapter shape, 1 of 3 topics): capability binding collapses the four function seams into ONE host adapter carrying two verbs — `read` (a view's query-scoped read; the Sync/Async split is an implementation detail the declaration never names) and `apply` (execute a statically checked manifest effect) — plus `toRow` leaving the capability surface entirely (it becomes a keyword-projection data map, point-6 convention). Separate read/write capability names were rejected: per-口 permissioning belongs to the manifest's `authorize` declarations, not the binding registry — 20260712141200-specify-the-capability-binding-contract.md
- 2026-07-13 — point-9 PROVEN: the pausable interpreter ↔ settle-loop handshake runs against the real derived scheduler in `packages/plggmatic/src/Flow/` (model: prototype script/value unions + run-state; usecase: the fuel-metered small-step evaluator) with 11 spec tests asserting all five properties — pause-at-dispatch yielding the Msg as a value, dispatch→settle→resume over the settled Scene to a final value, JSON-revived pauses resuming identically, and fuel totality (exhaustion as `Failed`, 1 fuel = 1 reduction, zero burn while parked, monotone across pauses). One clarification fed back into `dsl-v1-core.md` §2: v1 `dispatch` appears only as a direct flow-body step, which makes the continuation trivially serializable data. Engineering note: direct self-reference in a `Box<…, Union>` type argument is an illegal circular alias — recursive positions wrap in object literals (the plgg-ir `Sexp` convention). plgg-ir deps NOT yet added (the prototype stubs the checked IR, per its ticket); they arrive with the static-layer ticket — 20260712141400-prototype-the-pausable-interpreter-settle-loop-handshake.md
- 2026-07-13 — point-6 FROZEN: the v1 core specification is written to `dsl-v1-core.md` in this mission directory — placement (Flow tier + plgg-ir core deps), the closed eight-form set, no-reader-extension literals, the ~31-name host vocabulary with keyword projection, and deterministic fuel — plus three worked Demo 1 flows and the interfaces to points 7/8/9. Acceptance point 6 checked; the Flow static-layer implementation ticket is filed (interpreter core is point 9's existing prototype ticket) — 20260712141100-freeze-the-dsl-v1-core-on-the-plgg-ir-family.md
- 2026-07-13 — point-6 partial decision (fuel, 5 of 5 freeze topics): 1 fuel = 1 interpreter reduction step (deterministic — same script + same Scene consumes the same fuel on any machine, which the serializable-continuation property requires; never wall-clock). The budget is passed by the caller (`run(script, fuel?)` — settle-loop harness or the future run_flow meta-tool) with a spec-set default (~10,000; Demo 1 flows are hundreds) and a hard cap; exhaustion returns `Failed{fuel-exhausted}` with the source range as a diagnostic — a value folded by match, never a throw. Fuel counts ONLY the script's own computation: zero consumption while Paused on a dispatch (settle time / user time), and the remaining budget serializes inside the Paused value with the continuation; real-world slowness is the harness's timeout concern, not fuel's — 20260712141100-freeze-the-dsl-v1-core-on-the-plgg-ir-family.md
- 2026-07-13 — point-6 partial decision (host functions, 4 of 5 freeze topics): the flow-visible vocabulary is ~31 names in six groups — list (map, filter-by, count, first, last, nth, take, sort-by, reverse, empty?), access (get→Option, contains?), Option/Result folds (get-or, some?, none?, ok?, err?), string (str, includes?, starts-with?, lower, trim), number (+, -, *, sum, min, max), comparison/logic (=, <, <=, and, or, not; arithmetic+comparison live in the plgg-ir-language OPERATOR registry, the rest are pure host functions). All pure and TOTAL (partial results are Option/Result values, never throws), typed over SemType so misuse fails before execution. The no-fn consequence is codified: higher-order positions take KEYWORDS as projections (keyword-projection, same idea as toRow's shorthand) and predicates are fixed to filter-by equality — arbitrary predicate expressions stay in the manifest/source-side predicate language (v2, per the point-4 boundary). Effectful anything is NOT a host function: capabilities (point 7) are a separate registry; dispatch stays the only world-touching口. Additions follow the closed-set discipline, one at a time on demonstrated need — 20260712141100-freeze-the-dsl-v1-core-on-the-plgg-ir-family.md
- 2026-07-13 — point-6 partial decision (literals, 3 of 5 freeze topics): NO reader extension — `ok`/`err`/`some`/`none` are ordinary constructor FORMS in the closed set, not lexical literal syntax. The reader stays plgg-ir-syntax exactly as published (its S-expression grammar and round-trip guarantee are a shared promise across all manifest consumers; the layering "only the syntax layer knows the grammar" is preserved). Type checking and match exhaustiveness over Option/Result work through the form registry; base literals (numbers, strings, keywords, vectors) are whatever plgg-ir-syntax already reads — 20260712141100-freeze-the-dsl-v1-core-on-the-plgg-ir-family.md
- 2026-07-13 — point-6 partial decision (form set, 2 of 5 freeze topics): v1 special forms are a CLOSED set of eight — `flow`, `do`, `dispatch` (yield a Msg + pause until settle), the `scene-*` read family over the settled Scene, `let`, exhaustive `match` (no `if`), the `ok`/`err`/`some`/`none` constructors, and pure host-function application. Deliberately excluded from v1: user-defined `fn` (host functions substitute; repetition tolerated in LLM-generated flows), loops/recursion (kills the hard part of serializable continuations + fuel; v2 candidate is bounded `for-each` with a mandatory limit), and macros (would open the vocabulary and break static exhaustiveness). Same growth discipline as the Level union: closed, grown deliberately on demonstrated need — 20260712141100-freeze-the-dsl-v1-core-on-the-plgg-ir-family.md
- 2026-07-13 — point-6 partial decision (placement, 1 of 5 freeze topics): the flow dialect (static vocabulary registered into plgg-ir-language) and the fuel-metered pausable interpreter live together as a FOURTH TIER `packages/plggmatic/src/Flow/` (`model/` = forms/script/run-state, `usecase/` = dialect composition, reader entry, small-step evaluator), with plgg-ir-syntax/language/manifest added as plggmatic core dependencies (first-party foundation chain; allowed — the foundation-only rule bars only app-layer packages like plggpress/plgg-cms). Grounds: Flow folds the same closed `Msg`/`Scene` unions the renderers fold (a third-renderer-like consumer), so the in-package exhaustive-match forcing keeps it in lockstep; tree-shaking already gives non-Flow consumers isolation; a tier directory + three dep lines is the sacrificial unit. A separate plggmatic-flow package was rejected (two-package publish coordination for every Msg/Scene growth, delayed exhaustiveness forcing) — 20260712141100-freeze-the-dsl-v1-core-on-the-plgg-ir-family.md
- 2026-07-12 — remaining discussion points 6–9 ticketed, grounded in a fresh survey of the now-merged-and-published plgg-ir family (syntax/language/manifest at 0.0.1; PR #65): the family deliberately ships NO runtime (no evaluator, fuel, host functions, capability binding), so points 6/7/9 define new plggmatic-owned surface on top of the checked IR; the point-8 "reusing plgg-mcp" assumption is stale (no such package exists — the plain-MCP transport is an open decision). Tickets: 20260712141100-freeze-the-dsl-v1-core-on-the-plgg-ir-family.md, 20260712141200-specify-the-capability-binding-contract.md, 20260712141300-design-the-webmcp-adapter-boundary.md, 20260712141400-prototype-the-pausable-interpreter-settle-loop-handshake.md
- 2026-07-13 — ticket archived — 20260712141100-freeze-the-dsl-v1-core-on-the-plgg-ir-family.md
- 2026-07-13 — ticket archived — 20260712141400-prototype-the-pausable-interpreter-settle-loop-handshake.md
- 2026-07-13 — ticket archived — 20260712141200-specify-the-capability-binding-contract.md
- 2026-07-13 — ticket archived — 20260712141300-design-the-webmcp-adapter-boundary.md
- 2026-07-13 — story reported — work-20260712-111148.md
- 2026-07-13 — concern deferred (stuck) — 2-same-day-first-party-publishes-are.md
- 2026-07-13 — concern deferred (stuck) — 2-the-dist-smoke-gate-skips-on.md
- 2026-07-13 — concern deferred (stuck) — 2-plgg-ir-0-0-1-carries.md
- 2026-07-13 — concern deferred (stuck) — 2-the-interpreter-prototype-stubs-the-checked.md
- 2026-07-13 — concern deferred (stuck) — 2-the-http-mcp-transport-is-deferred.md
- 2026-07-13 — ticket archived — 20260713110000-implement-the-flow-static-layer.md
- 2026-07-14 — concern resolved (unstuck) — the-interpreter-prototype-stubs-the-checked.md
- 2026-07-14 — story reported — work-20260713-184234.md
- 2026-07-14 — concern deferred (stuck) — the-reader-draft-s-mechanical-fix.md
- 2026-07-14 — ticket archived — 20260713140000-implement-the-host-adapter-and-keyword-projection.md
- 2026-07-14 — story reported — work-20260714-003038.md
- 2026-07-14 — concern deferred (stuck) — authorize-absence-is-permitted-pending-manifest.md
- 2026-07-14 — concern deferred (stuck) — keyword-projection-covers-the-scalar-kinds.md
- 2026-07-14 — concern deferred (stuck) — the-apply-verb-is-a-hand.md
- 2026-07-14 — ticket archived — 20260713150000-implement-the-tool-catalog-and-webmcp-adapter.md
- 2026-07-14 — ticket archived — 20260713104000-write-the-plgg-ir-and-flow-architecture-guide.md
- 2026-07-16 — successor mission opened — the semantics this mission
  settled are now the input to `pragmatic-on-demand-system-generation`,
  which carries the manifest lowering (the one link that still stops an
  LLM-written declaration from becoming a running UI), the MCP Apps
  surface, and the `(declaration, rows)` I/O interface. Acceptance here
  stands at 10/10; what remains is not this mission's scope —
  ../pragmatic-on-demand-system-generation/mission.md
