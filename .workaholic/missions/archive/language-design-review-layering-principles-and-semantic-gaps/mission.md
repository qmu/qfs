---
type: Mission
title: Language design review: layering principles and semantic gaps
slug: language-design-review-layering-principles-and-semantic-gaps
status: achieved
created_at: 2026-07-09T05:12:09+09:00
author: a@qmu.jp
tickets: []
stories: []
concerns: []
---

# Language design review: layering principles and semantic gaps

## Goal

qfs's language has grown by per-feature decisions (39 frozen keywords, PipeOp governance lock,
decision H "functions are values", §15 `transform`) but the **principle that decides what becomes
a pipe stage vs a stdlib function vs stored data has never been written down**. The owner raised
this during the TRANSFORM epic (2026-07-09 design discussion): the language claims a
function-pipeline / higher-order identity while adding special forms, and the discomfort traces to
the principle being implicit, not to the design being wrong.

The discussion established the reading this mission encodes: qfs is a **two-layer language** —
a closed, first-order relational **stage algebra** (planable, pushdown-capable, effect-gated) over
a **total, pure, row-scoped expression layer** where functions are values. Every stage is notation
for an implicit-lambda combinator application (`where p ≡ filter(rel, (row) => p)`); the stage
layer stays closed because the planner and the preview/commit gate must *see through* every stage.
The sanctioned axis for more genericity is **abstraction over pipelines** (pipeline-valued
lambdas), never opacity of predicates.

**Reframing (2026-07-09, owner: "shell + querying integration"): the type system is the typed-path
space itself.** A qfs path is not a string address but a **typed location** — its type governs
three operations at once: navigation (`cd`/`ls` — the shell face), query (reading the path yields
rows in its type — the querying face), and write-membership (only type-conforming data can be
placed there, e.g. a Gmail path admits only Gmail-shaped rows). The `Schema` **is** the type of a
path; the shell face and the query face are two operations over one typed namespace. This is the
owner's meaning of "type system" and the mission's foundation. It also decides the transform
operand: after a stage verb the operand is either *inflowing data* (a path — `join /sql/...`) or a
*behavior-selector parameter* (a bare token — `decode json`, `call mail.send`); `transform` is in
the selector family, so `|> transform triage` (bare name), never a path — a path there is a
category error (data-location syntax naming a function). Definitions live at typed catalog paths
(`ls /transform`, the shell face) and are invoked by name (the stage face); the two never conflate.
transform legitimately earns stage-hood because its input is the inflowing relation, which no path
coordinate can address.

**Root finding (2026-07-09, owner: "types are too much an afterthought"): the type system is
retrofitted, and the gaps below are its symptoms.** The sharpest single proof: the blueprint's own
refinement-type example `CREATE TYPE /type/email (value text NOT NULL) WHERE value LIKE '%@%'`
(§105) has **no `WHERE` clause in the parser** — `create_type_stmt` (`grammar.rs`) reads only the
column list, so the refinement predicate is specced and unbuilt. More evidence: one type vocabulary
is spelled three ways — column types `text`/`int`/`bytes` (`ColumnType::parse`), lambda annotations
`string`/`i64`/`Row` (`TypeAnn`, parse-and-retain, unenforced until t75), and the transform DDL
holding raw strings rehydrated later by `ColumnType::parse` (`grammar.rs:1305,1458`); `reduce`
returns `Unknown` and `ColumnType::Unknown` is a vocabulary word; the blueprint conceives types as
path-addressed data ("a named, path-addressed, intensional relation"), colliding with the
reference principle in gap 6; and spec/implementation drift already exists — blueprint §15 writes
`CREATE TRANSFORM /transform/triage` while the shipped T1 parser takes a bare ident name
(`grammar.rs:1330`). The fix is foundational, not a sweep: a blueprint **type-system chapter** —
one vocabulary and one grammar used everywhere (DDL, lambda annotations, DESCRIBE, driver
declarations); relation types first-class, so a pipeline is typed composition and every stage has
a typing rule (this is the formal content of the stage admission test); named types are
*definitions* (name-referenced, refinement predicates checked at seams); `/type` and `/transform`
remain catalog *inspection* surfaces; t75 becomes the enforcement of that chapter, not a bolt-on.

The concrete gaps originally verified against the code on 2026-07-09, each to be resolved as a
consequence of the type-system chapter:

1. **Original finding: no arithmetic operators.** At mission creation, `Op` was comparison/logical/
   LIKE/regex only; `+ - * /` did not exist (`*` lexed only as projection star).
   `extend total = price * qty` was unwritable while
   `SUM`/`DATE_DIFF`/`ABS` exist — the gap real transform+join queries hit first.
2. **Original finding: stdlib name-resolution split from keyword policy.** Keywords were
   lowercase-canonical,
   case-insensitively recognized (decision S/t74); builtins are a **case-sensitive** exact
   `HashMap` lookup (`registry.rs::builtin`) with a mixed convention — `UPPER`/`COUNT` uppercase,
   `map`/`filter`/`reduce`/`env`/`http.get` lowercase. `upper(x)` returned `unknown_function`.
   In-tree docs already trip on this (`core/src/lambda.rs` module doc writes `concat(x, suffix)`,
   which does not resolve).
3. **Original finding: `LIKE` was spelled twice** — a frozen operator (`expr LIKE pat`,
   `Op::Like`) *and* a registered scalar builtin. One meaning, two grammars; anti-compression and a
   precedent that erodes the operator freeze.
4. **Original finding: doc drift on `Op::Eq`** — `parser/src/ast.rs` documented `Op::Eq` as
   `` `=` `` while decision O reserves `=` for binding and `Token::EqEq` (`==`) is what maps to `Op::Eq`
   (`grammar.rs:380`).
5. **`transform` must stay the only opaque-function seam** — §15's boundary (one declared,
   authenticated, gated model-call stage; no second entry point ever) is currently prose in the
   blueprint, not a locked test.
6. **Reference conventions are unprincipled across the surface** (owner-raised 2026-07-09: a
   source/join path *contributes rows* while `transform`'s path names an *applied definition* —
   a cognitive pun). Inventory shows five reference styles: relational paths (rows), definition
   paths (`/transform/…`, `/type/…`, declared `CREATE VIEW /path`), dotted procedures
   (`call mail.send`), bare format tokens (`decode json`), and bare server-binding names
   (`create endpoint <name>`, `job <name>`). Worst confusable: `CREATE VIEW` dispatches
   declared-view vs server-binding **by reference style alone** (path vs bare ident,
   `parser/src/tests.rs:877`).
   **Ruling direction (owner, 2026-07-09, superseding the earlier "lifecycled ⇒ path" candidate):
   paths = data (rows you can read); names = definitions**, resolved by the stage/clause word that
   reads them (`decode` → codecs, `call` → procedures, `transform` → transform definitions) — the
   Unix storage-vs-invocation split (`/usr/bin/grep` vs `grep`). Definitions stay stored and
   inspectable at catalog paths (`ls /transform`, `describe /transform/triage`, previewed
   install/remove writes, provenance records the catalog path); the pipe never applies a path.
   Under this rule the `CREATE VIEW` pun becomes principled (path = readable data surface,
   name = server binding), the transform stage reads `|> transform triage` (the shipped T1 DDL
   already takes a bare name), and the earlier stage-word rename question (`via`/`apply`/`infer`,
   assessed 2026-07-09) is **moot** — `transform triage` is verb+object with no stutter. Open
   sub-question: `OF /type/…` references in declarations (resolved by the type-system chapter).
   At the time, the transform stage had not reached its execution slice, so this had to be ruled
   before T2.

**Current status (2026-07-10).** The original gap list is now mostly historical. Arithmetic
operators shipped; stdlib names are canonicalized and case-insensitive; `LIKE` is operator-only;
the `Op::Eq` doc drift is fixed; the transform one-seam lock is enforced; reference conventions
are implemented for `transform <name>`, `CREATE TYPE <name>`, declared-view `OF <name>`, and
column type positions such as `email email`; lambda annotations now enforce the canonical
type-literal grammar and reject the old alias zoo. The remaining semantic cleanup is narrower:
implementing the general mid-pipe `of <type>` assertion and deciding the shell-face verb
semantics.

**Recorded design space — the transform stage surface** (owner walkthrough, 2026-07-09; baseline
is `|> transform <name>`, settled above). Four alternatives, each with a standing verdict, to be
carried into the type-system chapter's considered-alternatives section:

- **Bare-name application** `|> triage` — pipelines read as pure function composition and unify
  with future pipeline-valued lambdas (user-defined stages). Deferred as the possible
  **endgame**: only decidable after the type chapter, because dropping the call-site word is
  sound only if effects ride the definition's type (`Relation<S> →[model] Relation<S'>`); also
  costs a stage-word/user-name namespace collision.
- **Expression-layer call** `|> extend p = triage(subject, body)` — **rejected permanently,
  reason recorded**: it would be the single hole in the expression layer's pure/total/row-scoped
  cage, undefine "describe/preview touch nothing" and expression totality, and break the
  one-seam rule. Expect this to be re-proposed; cite this verdict.
- **Inline anonymous transform** (`|> transform (subject text, body text) => (priority text,
  reason text) prompt '…'`) — the model-flavored twin of the language's named/inline lambda
  duality; plausible **future sugar** for throwaway exploration, blocked until provider/model/
  secret can come from session defaults (stored-only is correct for v1's auth/audit story).
- **Use-site contract annotation** `|> transform triage of (priority text, reason text)` — not an
  alternative but a readability/locality annotation, checked at plan time. To be treated in the
  type chapter as a **general** use-site type-assertion rule (`of <type>` insertable mid-pipe on
  any stage, the `create table … of` vocabulary generalized), never a transform special case.
  Highest design leverage of the four; owner flagged it as the one to work through first.

## Scope

**In scope**

- **The type-system chapter in the blueprint — the mission's first ticket, which the rest
  derive from**: the **typed-path space** (a path's type governs navigation, query, and
  write-membership as one — the shell/querying unification); one type vocabulary and grammar
  everywhere (the `string`/`i64` annotation spellings are now retired); first-class relation types with a
  typing rule per stage; named types as name-referenced definitions carrying **refinement
  predicates** (implement the specced-but-unbuilt `CREATE TYPE … WHERE <pred>` — parse, store,
  declare-time well-formedness check via t75's checker, and eval-time membership enforcement at the
  write/`OF` boundary reusing the pure predicate evaluator; row-local pure predicates only, no
  proof-carrying/solver scope); the stage-operand category rule (data-path vs selector-name);
  catalog-vs-reference split; and t75 positioned as its enforcement.
- A blueprint section stating the **stage admission test**: a construct may become a pipe stage
  only if the planner or the gate must see through it — (a) pushdown-translatable, (b) plan-time
  schema rewrite, (c) effect gating, (d) cardinality/ordering semantics. Everything else is a
  stdlib function (expression layer) or data under a path (definitions/registries). Record that
  all 39 keywords + every current PipeOp pass the test; future stage proposals cite it.
- Correcting the self-description in docs: qfs is a closed relational pipe algebra + a total pure
  expression language with functions-as-values + declared effect seams — with the desugaring
  equivalence table (`where p ≡ filter(rel, (row) => p)`) so special forms read as notation, not
  exception.
- A deliberate decision (either way) on arithmetic operators, including precedence and the
  operator-freeze test change if adopted.
- One naming/resolution policy for stdlib functions (canonical case + recognition rule) aligned
  with the keyword policy, plus resolving the `LIKE` operator/function duplication and the
  `Op::Eq` doc drift.
- A decision on **pipeline-valued lambdas** (`let hot = (rel) => rel |> where …`) as the
  sanctioned genericity axis — adopt with a slice plan, or defer with the reasoning recorded.
- A lock (test or governance note) that `transform` remains the sole model-call seam.
- **The shell face of the typed-path space** (owner-raised 2026-07-09, "can we exercise cd/ls/cp on
  type/transform/table paths?"). Decision + design note: a uniform shell verb set over typed paths,
  split into **pure navigation** (`ls`/`cd`/`cat`/`describe` — touch nothing) and **gated mutation**
  (`cp`/`mv`/`rm`/`mkdir` — effects that route through preview/commit like any write). Semantics
  derive from the path's entry kind: a **data path** (table, mail, drive) has typed rows — `ls` ≡
  `from`, `cp` ≡ a membership-checked `insert into` (the type polices the copy; cross-layer or
  wrong-type `cp` is a structured refusal), `rm` ≡ `remove`; a **definition catalog** (`/type`,
  `/transform`) has definition files — `ls` lists, `cat`/`describe` reads the def, `cp`/`mv`/`rm`
  clone/rename/drop the file (no membership check), and a def path is never a data-`cp` target.
  `ls /type` (= SHOW TYPES) and the queryable `/sys/drivers` catalog already realize the "`ls` ≡
  query the catalog relation" half. **Leaning: implement as a thin REPL/shell layer that desugars
  `cp`/`ls`/`rm`/`mv` to `insert`/`from`/`remove`/`update` and adds session `cwd` — NOT new frozen
  keywords** (the core stays 39 keywords + absolute-path-only; `cd`/cwd is REPL session state that
  absolutizes relative paths, so every statement still lowers to absolute paths and the pure engine
  stays stateless). Bundles the earlier `cwd`/shell-relative-path idea (data-path ergonomics) with
  the verb set.

**Out of scope**

- Additional live-provider/provider-specific TRANSFORM work beyond the shipped T1–T4 seam.
- Whole-program / let-polymorphic inference and `reduce`'s late-bound `Unknown` return tightening;
  the existing plan-time checker enforces the canonical annotation vocabulary, but this mission does
  not turn it into a solver.
- Any relaxation of purity/totality in the expression layer (no recursion, no statements in
  lambda bodies, no I/O) — those invariants are the point, not a gap.

## Acceptance

- [x] Blueprint type-system chapter written: one vocabulary/grammar, per-stage typing rules,
      named types as definitions, catalog-vs-reference split, t75 as enforcement; includes the
      use-site `of <type>` assertion ruling and the transform-surface considered-alternatives
      record (#20260709104254-blueprint-type-system-chapter.md)
- [x] Blueprint carries the stage admission test + two-layer self-description, and the existing
      keyword/PipeOp inventory is recorded as passing it (#20260709104255-two-layer-model-stage-admission-test.md)
- [x] docs/language.md (via gen-docs source) states the two-layer model and the stage↔combinator
      equivalence table (#20260709104255-two-layer-model-stage-admission-test.md)
- [x] Arithmetic operators: decided, and if adopted, shipped with precedence + operator-freeze
      test update; if rejected, the reasoning is in the blueprint (#20260709104257-arithmetic-operators.md)
- [x] Stdlib naming/resolution policy decided and enforced by a test; `LIKE` double-spelling
      resolved; `Op::Eq` doc comment fixed (#20260709104258-stdlib-naming-resolution-like-eq.md)
- [x] Pipeline-valued lambdas: decision recorded (adopt-with-plan or defer-with-reasoning)
      (#20260709104259-pipeline-valued-lambdas-decision.md)
- [x] A test or governance lock asserts `transform` is the only model-call seam
      (#20260709104300-transform-one-seam-lock.md)
- [x] Canonical type vocabulary enforced for lambda annotations and base type parsing: retired
      spellings such as `string`, `i64`, `integer`, `varchar`, `jsonb`, and lowercase `resource`
      no longer silently canonicalize
- [x] Reference-convention principle ruled and recorded (direction: paths = data, names =
      definitions resolved by the stage/clause word); `CREATE VIEW` pun judged against it;
      **type references name-ified** (`of /type/customer` → `of customer`, column `email
      /type/email` → `email: email`, `create type /type/customer` → `create type customer`; `/type`
      stays the catalog/shell face; base and refined types unify into one name namespace, fixing the
      current bare-`int`-vs-path-`/type/email` split); transform stage surface fixed as
      `|> transform <name>` (blueprint §15 path examples corrected to match the shipped name-taking
      DDL)
      (#20260709104256-reference-convention-transform-surface.md)
- [x] General mid-pipe `of <type>` assertion implemented for arbitrary relation stages, not only
      declared views / row-bearing write boundaries
      (#20260714154144-general-of-type-assertion.md)
- [x] Shell face decided: pure-navigation vs gated-mutation verb split, data-vs-definition
      semantics, `cp` membership-policing, and REPL-layer `cwd`/desugaring (no new frozen keywords)
      — recorded as a design note (shell-face-design-brief.md) and ruled **adopt** by the owner
      (2026-07-14); the completion work is sliced into three tickets
      (#20260714182710 ls/cat/describe typed + `/type` mount + §9 record;
      #20260714182720 `cd` enumerable-children gate;
      #20260714182730 mutation verbs per entry kind)

## Changelog

- 2026-07-09 — Mission created from the owner's language-design discussion during the TRANSFORM
  epic (T1 shipped); five gaps verified against the code and recorded.
- 2026-07-09 — Gap 6 added (owner): reference-convention inconsistency — relational paths vs
  `transform`'s applied-definition path, the five-style inventory, and the `CREATE VIEW` dispatch
  pun; timing note that T2 makes this urgent.
- 2026-07-09 — Stage-word rename option recorded (owner asked whether renaming `transform`
  improves the dissonance): rename the stage verb only, noun/mount unchanged; candidates
  `via` (leaning) / `apply` (rejected-leaning: collides with `qfs apply` CLI + blueprint
  applier vocabulary) / `infer`; acceptance item added with the before-T2 deadline.
- 2026-07-09 — Principle flipped after owner's "the problem is that it's a path": paths = data,
  names = definitions (stage word selects the registry); transform stage reads
  `|> transform triage`; stage-word rename mooted; the two before-T2 acceptance items merged.
- 2026-07-09 — Root reframing (owner: "types are too much an afterthought"): type system
  identified as the root, gaps re-read as its symptoms; blueprint type-system chapter added as
  the mission's first ticket; evidence recorded (three type spellings, `Unknown` in the
  vocabulary, §15 path-vs-name drift against the shipped T1 DDL).
- 2026-07-09 — Transform-surface design space recorded (four alternatives with verdicts:
  bare-name endgame deferred to the type chapter, expression-layer call rejected permanently,
  inline anonymous form as future sugar, use-site `of` assertion generalized into the type
  chapter); type-chapter acceptance item extended accordingly.
- 2026-07-09 — Acceptance ticketed (owner: "ticket all"): seven tickets under
  `todo/a-qmu-jp/`, root = `20260709104254-blueprint-type-system-chapter.md`; the other six
  `depend_on` it (reference-convention + two-layer/admission-test + arithmetic + stdlib-naming +
  pipeline-lambdas; one-seam-lock depends on reference-convention). Reference-convention carries
  the before-T2 sequencing flag; arithmetic is decide-then-implement-in-one; one-seam-lock uses
  plan-check + visibility.
- 2026-07-09 — Typed-path-space reframing folded in (owner: "shell + querying integration"): type
  system = the typed-path space (navigation/query/write-membership from one path type); transform
  operand settled by the stage-operand category rule (data-path vs selector-name), so
  `|> transform triage`; concrete finding recorded — `CREATE TYPE … WHERE <pred>` refinement is
  specced (§105) but unparsed. Type-chapter ticket scope broadened to the typed-path space + a
  refinement-predicate implementation slice (parse/store/declare-time check/eval-time membership,
  row-local pure predicates only). reference-convention ticket regrounded on the operand-category
  rule.
- 2026-07-09 — Type references name-ified (owner walked the full "reduce paths" inventory): applying
  the operand-category rule, only type references + `create type` actually change (transform already
  ships as a name; server bindings endpoint/job/… were already names — vindicated, not outliers).
  Cognitive synthesis recorded: paths where location is information (data), names where the path was
  a definition's storage address; data-path shortening is a separate ergonomic (shell `cwd`), not a
  semantic change. reference-convention acceptance extended with the type-reference name-ification.
- 2026-07-09 — Shell face added (owner: "add to mission"): uniform shell verbs over typed paths,
  pure-navigation (`ls`/`cd`/`cat`) vs gated-mutation (`cp`/`mv`/`rm`), data-vs-definition
  semantics, `cp` as membership-checked `insert` (type polices the copy), leaning to a REPL-layer
  `cwd`/desugaring with no new frozen keywords. New scope + acceptance item (ticket TBD).
- 2026-07-09 — **Root ticket landed** (20260709104254): blueprint §5 rewritten as the typed-path
  space chapter (§5.1–§5.8 — typed-path space spine, stage-operand category rule, one vocabulary,
  first-class relation types, refinement model, general `of` assertion, transform considered-
  alternatives), Fable-drafted and owner-approved (J1–J4). Refinement slice shipped: `CREATE TYPE
  … WHERE <pred>` parses and stores `{columns, where}`; declare-time well-formedness (non-bool /
  impure builtin / unknown-or-`unknown`-typed column rejected at CREATE); pure `check_membership`
  enforced per row at the declared-view `OF` boundary. §15/§13/line-100 name-reference examples
  corrected; §15 stale "nothing implemented" note fixed; `TypeAnn` doc-comments re-pointed at the
  chapter. Deferred (recorded, not silent): `create table … of <name>` grammar + the full cookbook
  write-recipe ride the reference-convention sibling; the cookbook carries a declare-only refined-
  type recipe (parse-verified) meanwhile.
- 2026-07-09 — **One-seam lock landed** (20260709104300): §15's "transform is the only model-call
  seam" enforced two ways. Governance test (`eval::tests::transform_is_the_only_model_call_seam`):
  a spread of non-transform statements (read/write/CALL/codec/DDL) carry no model-call effect node,
  a `transform`-bearing statement carries exactly the seam. Visibility seal: `ModelProvider::call`
  now requires a crate-private `CallProof` witness minted only by the `call_model` funnel — a driver
  holding a `&dyn ModelProvider` cannot invoke a model (compile_fail doctest locks it); the trait
  stays open to implement (live provider is a binary-leaf concern) = open-to-implement / sealed-to-
  invoke. Residue recorded: singling out the binary applier as the sole *caller* is architectural,
  not Rust-type-enforced (would drag rusqlite/vault into the tokio-free driver crate). §15 cross-
  references the lock.
- 2026-07-09 — **Name-resolution ruling recorded** (owner design session, approved "ok go"): no
  module system. (1) reference = registry-relative name (bare when flat, qualified when nested —
  §5.5 as landed), ambiguity a structured error, no base-token shadowing; (2) store-time
  canonicalization — persisted artifacts (views/jobs/endpoints, provenance) carry the resolved
  absolute catalog path, short names are interactive-only; (3) `import`/`open`
  considered-and-rejected (breaks query self-containment; second skeuomorph) with the recorded
  escape hatch being a $PATH-style session search path over catalog prefixes, never a module
  language. Recorded into the live resolver ticket
  (#20260709140000-column-type-refined-name-resolution.md).
- 2026-07-09 — **Reference-convention #3 landed as (A)+(B)** (20260709104256): the mechanical locks
  + name-form canonicalization shipped; the column-type resolution seam split to (C)
  (20260709140000). Delivered: a `type_name` qualified-name parser (`email`, `chatwork/message` →
  canonical `/type/<name>`) with the legacy `/type/…` path form rejected at a reference site;
  `create type <name>` and declared-view `OF <name>` name-ified; parser lock tests
  (`transform triage` accepted / `transform /path` rejected; `create type /type/…` rejected); the
  `CREATE VIEW` path-vs-bare dispatch documented as principled; `TransformRef` doc-comment ruled.
  Every `/type/…` reference site converted in lockstep (core/exec/parser tests, slack.qfs +
  cloudflare.qfs assets, databases/cloudflare cookbook + regenerated skills, chatwork-benchmark
  guide). Finding: `CREATE TRANSFORM INPUT/OUTPUT` take only inline column literals (no named-type
  path to name-ify). Plugin bumped 0.7.0→0.8.0 (taught-surface break: path-form `CREATE TYPE`
  retired), qfs 0.0.38→0.0.39. The reference-convention acceptance box stayed open until (C)
  landed the column-type-position name form (`email email`).
- 2026-07-10 — Documentation truth-maintenance pass: the archived acceptance tickets were reconciled
  back into this mission. Stage admission, generated language docs, arithmetic, stdlib naming,
  pipeline-valued lambda decision, one-seam lock, and reference convention are now marked complete.
  Remaining open mission work is the shell-face decision plus the general mid-pipe `of <type>`
  assertion.
- 2026-07-10 — Canonical type vocabulary cleanup landed: lambda annotations now parse recursive
  `array<…>` / `struct<…>` forms, reject retired aliases as `unknown_type_annotation`, preserve only
  CamelCase `Resource`, and shared base-type parsing no longer normalizes old SQL/Rust aliases.
- 2026-07-11 — story reported — work-20260709-023822.md
- 2026-07-11 — concern deferred (stuck) — 32-carried-from-pr-11-cf-live.md
- 2026-07-11 — concern deferred (stuck) — 32-carried-from-pr-11-cloud-reads.md
- 2026-07-11 — concern deferred (stuck) — 32-carried-from-pr-11-extend-on.md
- 2026-07-11 — concern deferred (stuck) — 32-carried-from-pr-11-local-write.md
- 2026-07-11 — concern deferred (stuck) — 32-carried-from-pr-11-postgres-mysql.md
- 2026-07-11 — concern deferred (stuck) — 32-carried-from-pr-18-170000-quality.md
- 2026-07-11 — concern deferred (stuck) — 32-carried-from-pr-18-console-bundle.md
- 2026-07-11 — concern deferred (stuck) — 32-carried-from-pr-22-create-account.md
- 2026-07-11 — concern deferred (stuck) — 32-carried-from-pr-25-live-google.md
- 2026-07-11 — concern deferred (stuck) — 32-carried-from-pr-25-live-only.md
- 2026-07-11 — concern deferred (stuck) — 32-carried-from-pr-25-project-db.md
- 2026-07-11 — concern deferred (stuck) — 32-carried-from-pr-26-live-provider.md
- 2026-07-11 — concern deferred (stuck) — 32-carried-from-pr-26-local-rust.md
- 2026-07-11 — concern deferred (stuck) — 32-carried-from-pr-30-the-api.md
- 2026-07-11 — concern deferred (stuck) — 32-carried-from-pr-30-bearer-gated.md
- 2026-07-11 — concern deferred (stuck) — 32-carried-from-pr-30-the-config.md
- 2026-07-11 — concern deferred (stuck) — 32-artifacts-repo-token-is-sealed-but.md
- 2026-07-11 — concern deferred (stuck) — 32-qfs-runtime-span-buffer-test-flakes.md
- 2026-07-14 — **General mid-pipe `of <type>` assertion landed** (20260714154144): `of` is now the
  20th `PipeOp` — a general, any-position, plan-time type ASSERTION (named `of customer` or inline
  `of (priority text, reason text)`), never coercing, path-form rejected (§5.7). The structural check
  runs in the evaluator's schema fold against the **addressed-path** schema (`check_of_assertion`),
  with named types resolved from a new `MountRegistry::declared_types()` registry (the `transform_defs`
  twin, wired from the `/sys/drivers` `kind='type'` rows); refinement rides §5.4's honest split to the
  next row boundary. A read carrying an `of` is reclassified through the evaluator (exec `contains_of`
  → `build_plan`) so the check fires on the pure-read path too — the fix after driving the binary
  revealed the pushdown lowering only sees the driver ROOT schema. Structured errors:
  `of_assertion_failed` (missing/unexpected/mismatched columns), `of_type_unresolved`. Blueprint §5.6
  flipped to *implemented*, §5.3a to *20 variants*. qfs 0.0.64→0.0.65, plugin 0.11.4→0.11.5. Only the
  shell-face decision now remains open.
- 2026-07-14 — **Shell-face decided: adopt** (owner ruling). Fable design brief
  (shell-face-design-brief.md) — code-grounded — found the shell face is not a proposal but mostly
  **already shipped in ticket t28** (interactive REPL, verbs as line-head idents outside the 39
  keywords, cwd as session state absolutized before parse, desugar-to-core, gate inheritance,
  membership-policed `cp` via the materialized-rows seam). Two real defects vs §5.1 remain (`ls` is
  blob-typed and fails on non-blob paths; the `cd` gate is archetype-coarse) plus `/type` is
  unmounted. Owner ruled adopt; completion sliced into three tickets (182710 navigation/`ls`-typed +
  `/type` mount + §9 blueprint record; 182720 `cd` enumerable-children gate; 182730 mutation verbs
  per entry kind). All five hard constraints (39-keyword freeze, stateless absolute-path engine,
  preview/commit gate, paths=data/names=definitions, no back-compat) verified against the code. With
  both remaining acceptance items now closed, the mission's open design questions are exhausted; the
  three shell-face slices are implementation follow-ups.
- 2026-07-14 — ticket archived — 20260714154144-general-of-type-assertion.md
- 2026-07-14 — ticket archived — 20260714182710-shell-face-slice1-ls-cat-describe-typed.md
- 2026-07-15 — ticket archived — 20260714182740-shell-face-type-mount-and-describe-builtin.md
- 2026-07-15 — ticket archived — 20260714182720-shell-face-slice2-cd-gate-enumerable-children.md
- 2026-07-15 — ticket archived — 20260714182730-shell-face-slice3-mutation-verbs-per-kind.md
- 2026-07-15 — ticket archived — 20260714220213-resume-shell-face-slices-and-report.md
- 2026-07-15 — story reported — work-20260714-111817.md
- 2026-07-15 — concern deferred (stuck) — carried-create-account-ships-the-core.md
- 2026-07-15 — concern deferred (stuck) — the-interactive-shell-s-local-reads.md
- 2026-07-15 — concern deferred (stuck) — the-branch-safety-scanner-false-positives.md
- 2026-07-15 — concern deferred (stuck) — sys-and-slack-do-not-describe.md
- 2026-07-15 — concern deferred (stuck) — cd-into-a-blob-file-is.md
- 2026-07-15 — concern deferred (stuck) — definition-catalog-cp-clone-and-mv.md
- 2026-07-15 — concern deferred (stuck) — the-type-catalog-and-the-type.md
- 2026-07-15 — concern deferred (stuck) — the-carried-create-account-ships-the.md
- 2026-07-15 — **Mission achieved and archived** by the missions/tickets reframing (owner-approved).
  All 11 acceptance items are ticked and the 2026-07-14 entry above already recorded that the open
  design questions were exhausted; the three shell-face slices that remained were implementation
  follow-ups and have since shipped and archived. Nothing here was cut short.

  **The nine concerns that hung off this mission were re-homed BEFORE this archive, deliberately.**
  This mission was framed as an *activity* ("design review"), and an activity ends while its residue
  does not — which is exactly how the earlier `qfs-capability-tryout-…` mission archived `achieved`
  while its goal #2 was unfinished, orphaning seven concerns that no one owned for four days. Six of
  the nine (`cd`-into-blob, `/sys`+`/slack` root describe, definition-catalog `cp`/`mv`, `/type`
  catalog-vs-resolver key, shell `/local` cwd-vs-root, plus the span-buffer flake) moved to the
  **mission-free backlog** — a state the reframing makes legitimate, so residue no longer needs a
  fictional parent to stay alive. `the-carried-create-account-ships-the` was resolved outright (its
  recorded fix was performed and verified against v0.0.71). `artifacts-repo-token…` and the
  branch-safety scanner false-positive (a cross-repo fix) are mission-free too.

  Successor framing: missions are now **standing product properties**, not episodes — see
  `declared-drivers-are-the-normal-way-to-add-a-service`, which adopts the tryout mission's
  unfinished goal #2.
