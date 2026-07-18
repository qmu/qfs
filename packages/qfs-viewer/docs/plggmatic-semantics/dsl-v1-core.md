> Imported from qmu/plggmatic on 2026-07-16 by HQ triage (strategy mission qfs-viewer-mvp-headquarters); original path .workaholic/missions/active/plggmatic-screen-structure-model-semantics/dsl-v1-core.md

# plggmatic flow DSL — v1 core specification (FROZEN 2026-07-13)

Mission discussion point 6. This document freezes the v1 core of the
flow DSL: placement, forms, literals, host functions, and fuel
semantics. Changes to anything marked CLOSED follow the same growth
discipline as the engine's `Level` union: grown deliberately, one case
at a time, on demonstrated need — never widened.

Sources: the mission changelog entries of 2026-07-12/13 (points 1–6),
`../plgg/.workaholic/missions/build-the-plgg-ir-package-family/design.md`
(§4/§5 language boundaries, §24 dialect composition, §37 consumer
independence), and the published plgg-ir packages (0.0.1).

## 1. Placement and layering

The flow DSL splits over an existing boundary: **plgg-ir owns
everything static, plggmatic owns execution.** plgg-ir deliberately
ships no evaluator (design.md §5, §36.2); the runtime below is new,
plggmatic-owned surface.

The pipeline a flow travels:

| stage | responsibility | owner |
|---|---|---|
| read (text → `Sexp`) | plgg-ir-syntax | published |
| check (forms, types, names) | plgg-ir-language framework | published |
| check vocabulary | **flow dialect** (`Flow/model`) | this repo |
| normalize (canonical form) | plgg-ir-language | published |
| execute (fuel, pause, Msg) | **interpreter** (`Flow/usecase`) | this repo |

Both plggmatic-owned pieces live together as a fourth tier:

```
packages/plggmatic/src/Flow/
├─ model/
│   ├─ forms.ts    flow dialect: FormDef registrations
│   ├─ script.ts   FlowScript — the checked, normalized, runnable shape
│   └─ run.ts      RunState = Running | Paused{k, fuel} | Done | Failed
└─ usecase/
    ├─ dialect.ts  compose(core, manifest dialect, flow dialect)
    ├─ read.ts     text → Result<FlowScript, diagnostics>
    └─ step.ts     the small-step evaluator
```

`plgg-ir-syntax` / `plgg-ir-language` / `plgg-ir-manifest` become core
dependencies of `packages/plggmatic` (first-party foundation chain;
the foundation-only rule bars only app-layer packages). Grounds: Flow
folds the same closed `Msg`/`Scene` unions the renderers fold — a
third-renderer-like consumer — so living in-package keeps it under the
same exhaustive-`match` forcing that migrated the renderers when
`BoardLevel` landed. Non-Flow consumers are isolated by tree-shaking.
A separate package was rejected (two-package publish coordination on
every `Msg`/`Scene` growth; delayed exhaustiveness forcing).

The manifest dialect and the flow dialect stay SEPARATE dialects —
different subjects (structure vs procedure), different owners (plgg
vs plggmatic) — merged into one checked language by `compose()`.
Cross-dialect references (a flow naming a manifest-declared choice)
resolve through that composition. There is no "plggmatic manifest":
one Domain Manifest, many consumers; purpose-specific needs are added
as dialects, never as purpose-specific manifests.

## 2. Special forms (CLOSED — eight)

| form | semantics |
|---|---|
| `(flow <name> <body>…)` | one flow: the unit `run_flow` accepts; body is a `do` |
| `(do <expr>…)` | sequential evaluation; value of the last expr |
| `(dispatch <msg-form>)` | evaluate to a `SchedulerMsg`, YIELD it, park as `Paused`; resumes after the scheduler settles |
| `(scene-… …)` | read family over the settled `Scene` (rows, fields, title); the only observation口 |
| `(let [<name> <expr>…] <body>)` | lexical binding |
| `(match <expr> <case>…)` | plgg-style exhaustive fold; the ONLY branching (no `if`) — non-exhaustive cases are a static error |
| `(ok e)` `(err e)` `(some e)` `(none)` | Option/Result constructors (see §3) |
| host application `(f a…)` | pure computation only (§4) |

Excluded from v1, with the recorded costs:

- **`fn` (user functions)** — host functions substitute; repetition in
  LLM-generated flows is tolerated. Consequence codified in §4:
  higher-order positions take keywords.
- **loops / recursion** — kills the hardest property (serializable
  continuations + fuel) for v1; the v2 candidate is bounded
  `for-each` with a mandatory limit.
- **macros** — would open the vocabulary and break static
  exhaustiveness; excluded on principle, not deferred.

`dispatch` is the only world-touching form. Everything effectful
beyond it is a capability (point 7) — never a form, never a host
function.

**Amendment (2026-07-13, from the point-9 prototype):** in v1,
`dispatch` appears only as a DIRECT STEP of a flow body — never nested
inside an expression. Pure expressions therefore never pause, and the
paused continuation is trivially data (a step index + bindings +
remaining fuel), which is what made property §5.4 provable without a
defunctionalization machine. All three §6 worked flows already have
this shape; lifting the restriction is a v2 question that rides with
bounded iteration.

## 3. Literals

No reader extension. The reader is plgg-ir-syntax exactly as
published — its grammar and round-trip guarantee are a shared promise
across all manifest consumers, and only the syntax layer knows the
grammar. `ok`/`err`/`some`/`none` are ordinary constructor forms;
base literals (numbers, strings, keywords, vectors) are what
plgg-ir-syntax already reads.

## 4. Host vocabulary (CLOSED — ~31 names, six groups)

Arithmetic and comparison register in the plgg-ir-language OPERATOR
registry (typed signatures); the rest are pure host functions.

| group | names |
|---|---|
| list | `map` `filter-by` `count` `first` `last` `nth` `take` `sort-by` `reverse` `empty?` |
| access | `get` (keyword lookup → Option) `contains?` |
| Option/Result | `get-or` `some?` `none?` `ok?` `err?` |
| string | `str` `includes?` `starts-with?` `lower` `trim` |
| number | `+` `-` `*` `sum` `min` `max` |
| compare/logic | `=` `<` `<=` `and` `or` `not` |

Rules binding every entry:

- **Total.** Partiality is an Option/Result value (`first` of an empty
  list is `(none)`), never a throw.
- **Typed over SemType.** `sum` demands `(list number)`; feeding it
  `(map :label …)` fails BEFORE execution.
- **Keyword projection.** With no `fn`, higher-order positions take
  KEYWORDS as field projections — `(map :label rows)`,
  `(sort-by :since rows)` — the same shorthand idea point 7 gives
  `toRow`. Predicates are fixed to `filter-by` equality
  (`(filter-by :status "Active" rows)`); arbitrary predicate
  EXPRESSIONS belong to the manifest/source-side predicate language
  (v2, per the point-4 boundary).
- **Pure.** No I/O, no clock, no randomness — determinism is what
  makes fuel and continuation-serialization meaningful.

## 5. Fuel semantics

1. **Unit**: 1 fuel = 1 reduction step of `Flow/usecase/step.ts`.
   Deterministic: same script + same Scene consumes the same fuel on
   any machine (required by the serializable-continuation property).
   Never wall-clock.
2. **Budget**: passed by the caller — `run(script, fuel?)` from the
   settle-loop harness or the `run_flow` meta-tool. Default 10,000
   reductions (Demo 1 flows measure in the hundreds); a hard cap above
   which the request is a declaration error.
3. **Exhaustion**: `Failed{fuel-exhausted}` carrying the source range
   of the reduction that starved — a value the caller folds with
   `match`, never a throw.
4. **Pause**: fuel counts only the script's own computation. Zero
   consumption while `Paused` on a `dispatch`; the remaining budget
   serializes inside the `Paused` value with the continuation.
   Real-world slowness (settle time, user time) is the harness's
   timeout concern.

The `Paused` continuation is DATA (defunctionalized), not a closure —
the property point 9's prototype exists to prove (serialize → resume
→ identical trace).

## 6. Worked examples (Demo 1)

Search and read (the canonical flow of the 2026-07-09 discussion):

```clojure
(flow find-active-beacon
  (dispatch (open-menu clients))
  (dispatch (query-input "beacon"))
  (dispatch (query-choice cstatus "Active"))
  (let [rows (scene-rows clients)]
    (match rows
      []  (err :not-found)
      _   (ok (map :label rows)))))
```

Read a dashboard tile's headline:

```clojure
(flow dashboard-active-projects
  (dispatch (open-menu dashboard))
  (match (first (scene-rows dashboard))
    (some tile) (ok (get :label tile))
    (none)      (err :empty-board)))
```

Sum unbilled hours across timesheet rows:

```clojure
(flow unbilled-total
  (dispatch (open-menu timesheets))
  (ok (sum (map :hours (scene-rows timesheets)))))
```

All three stay inside the eight forms and the §4 vocabulary; the
implementation ticket carries them into spec tests.

## 7. Interfaces to the open points

- **Point 7 (capabilities)**: capability references are DSL names the
  host binds; their syntax builds on this core but is out of scope
  here. Host functions never overlap capabilities: pure vs effectful.
- **Point 8 (WebMCP)**: `run_flow` accepts flow text, runs it through
  this pipeline, and is bounded by §5's fuel contract.
- **Point 9 (prototype)**: proves `dispatch` pause/settle/resume,
  continuation serialization, and fuel totality against the real
  scheduler; any semantics this forces back into the core amend THIS
  document (recorded in the mission changelog).
