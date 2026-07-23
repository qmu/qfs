# The qfs blueprint

This is the **one living design document** of qfs: the current intended design of the whole
system, including parts not yet implemented. It is revised **in place** — git history is the only
history; superseded decisions are deleted, not archived. It absorbs the design role of RFD-0001
and the retired `docs/adr/0001–0009` (see [Retirement record](#retirement-record)).

Every section carries a status so intent is never passed off as fact:

- **implemented** — the binary does this today (tests hold it).
- **blueprint** — decided design, not yet built.
- **parked** — decided, deliberately waiting on a named external condition.

## 1. Vision — *implemented*

qfs ("cloud file system") is **one Rust binary** — CLI, daemon, and (parked) Workers artifact —
exposing every external service (mail, files, object stores, SQL databases, git, GitHub, Slack, …)
through **one uniform, filesystem-shaped, pipe-SQL language**.

It exists **for AI**: an agent learns one small grammar and one operating procedure —
**DESCRIBE → write a statement → PREVIEW → COMMIT** — instead of N SDKs. The same loop is
identical across every service and every face (CLI, shell, MCP, server).

## 2. Core model: paths are sets — *implemented*

Three faces of one engine:

1. **VFS** — every service mounts under a virtual root; **a path is a query that resolves to a
   set** (directories = sets, globs = queries, attribute predicates extend them).
2. **Pipe-SQL** — `FROM <path> |> op |> op …`; the query side (`WHERE SELECT JOIN UNION EXCEPT
   AGGREGATE …`) is pure.
3. **Effect plan** — write verbs never execute; they evaluate to a typed **Plan** (a DAG of
   effects). The only impure operation is the interpreter: `COMMIT : Plan → World → World`.

The path=set identity is the load-bearing foundation of everything below: capabilities gate
verbs per node, pushdown partitions a pipeline by source, and the [type system](#5-the-type-
system-types-are-sets--blueprint) is this same identity read as type theory.

## 3. The language — *implemented*

**Closed core, open registries.** The keyword set is frozen; a new backend adds **zero
keywords**. Everything new plugs into exactly one of three open namespaces: **paths** (mounts),
**functions/procedures** (`fn(...)`, `CALL driver.action(...)`), **codecs** (`DECODE`/`ENCODE`).

Frozen keywords — query/transform: `FROM WHERE SELECT EXTEND SET AGGREGATE GROUP BY ORDER BY
LIMIT DISTINCT JOIN UNION EXCEPT INTERSECT AS EXPAND`; effects: `INSERT INTO, UPSERT INTO,
UPDATE, REMOVE, VALUES, RETURNING, CALL`; codecs: `DECODE ENCODE`; plan: `PREVIEW COMMIT`;
definitions: `CREATE ENDPOINT|TRIGGER|JOB|VIEW|MATERIALIZED VIEW|WEBHOOK|POLICY`, `DO`, `EVERY`,
`ON`; operators: `|>`, comparison/predicate/logic (`==`, `<>`, `<`, `>`, `<=`, `>=`, `LIKE`,
`~`, `IN`, `ANY`, `BETWEEN`, `AND`, `OR`, `NOT`) and arithmetic (`+`, `-`, `*`, `/`). `==` is
equality; `=` is binding.

**The definition layer.** The `CREATE` family is not "server sugar" — it is the language's
second layer. Every statement is one of:

- **Data writes** — the universal CRUD verbs over paths. Creating a resource is `INSERT INTO`
  its collection (a draft → `/mail/drafts`; an object → `/s3/bucket/key`; a commit →
  `/git/repo/commits`). No per-driver create verb, ever.
- **Definitions** — `CREATE <Noun> …` declares structure whose existence changes the address
  space or the machine's behavior: `ENDPOINT TRIGGER JOB VIEW MATERIALIZED-VIEW WEBHOOK POLICY`
  (frozen), `CONNECTION` and `TABLE` (contextual idents), `TYPE` (§5), `DRIVER` and `MAP` (§13),
  and `TRANSFORM` (§15) are implemented contextual idents; `ACCOUNT` remains blueprint.
  Removal in this layer is `REMOVE <Noun> <path>` — one destructive verb everywhere. Every
  definition is **pure sugar desugaring to an effect-plan write over a registry path** (the
  server bindings → `/server/*`; connections → `/sys/paths`; tables → the `/sql/<conn>`
  catalog), so preview/commit, capability gating, and the appliers see one shape.
- **Irreducible domain actions** — namespaced `CALL driver.action(...)` procedures (`mail.send`,
  `github.merge`) for state transitions with no CRUD analog. Ergonomic aliases (`SEND`) are pure
  registry functions desugaring to `CALL` — never keywords.

**New nouns are contextual identifiers** (the `CONNECTION`/`TABLE` lesson): matched as bare
idents in position, adding no frozen keyword. Additive grammar = MINOR under the versioning
policy (§13).

**Function names are registry names, not keywords.** Core stdlib names are stored in canonical
lowercase and resolved case-insensitively (`upper`, `UPPER`, and `Upper` are the same builtin);
new function registries must assert no case-folding collisions. Operators do not get duplicate
function spellings: `LIKE` is the infix predicate form only, not a scalar `LIKE(...)` builtin.

**Purity invariant.** Every function — core or alias — has type `… → Plan`: it constructs
effects, never performs them. Two spellings, one plan: `INSERT INTO /b …` and `… |> INSERT INTO
/b` lower to the same `EffectStmt`.

## 4. Data model — *implemented*

Rows with typed columns (`ColumnType`: `Bool Int Float Decimal Text Bytes Timestamp Date Uuid
Struct(Schema) Array Json Unknown` — deliberately small; see §5 for why it does not grow).
Nested data: `Struct`/`Array` columns, `EXPAND` explodes collections, `a.b.c` navigates without
flattening. Codecs bridge blob↔relational (`json yaml toml csv markdown+frontmatter`), pure
`bytes↔rows`, applicable to any blob source. `@version` / `AS OF` is the uniform temporal
coordinate where a driver supports it.

## 5. The type system: the typed-path space — *vocabulary, checker, named/refined types, and the general `of` assertion implemented*

*(Re-founded 2026-07-09, mission `language-design-review-layering-principles-and-semantic-gaps`,
ticket 20260709104254 — this section is the foundation the mission's sibling rulings derive from.
It absorbs and supersedes the 2026-07-04 "types are sets" section (ticket 20260704124825) in
place: nothing is retracted, everything is re-founded on the typed-path space and extended with
the mission's rulings. Status is mixed and marked per rule — the `ColumnType`/`Schema` vocabulary,
the plan-time checker, named `CREATE TYPE` definitions, refinement predicates, name-referenced type
positions (`email email`, `OF customer`), row-bearing membership checks, and the general mid-pipe
`of <type>` assertion (both `of <name>` and `of (…)` inline forms — ticket 20260714154144) are all
**implemented**.)*

### 5.1 The typed-path space

§2's identity — *a path is a query that resolves to a set* — read as type theory, and now read
all the way: **a qfs path is a typed location, not a string address.** One path type governs
three operations at once:

1. **Navigation** (`cd` / `ls` — the shell face): a path's children are the elements of its set.
2. **Query** (the querying face): reading the path yields rows **in its type** — the rows of
   `/mail/inbox` are Gmail-shaped because that is the type of that location.
3. **Write-membership**: only type-conforming data may be placed at the path — a Gmail path
   admits only Gmail-shaped rows; a violating write is a structured refusal, never a mangled row.

**`Schema` is the type of a path.** The `qfs_types::Schema` a driver's pure `describe` returns
*is* the path's type — not documentation about it. The shell face and the query face are two
operations over **one typed namespace**; the shell adds no semantics (§9) precisely because `ls`
is a query and `cp` is a membership-checked write over the same types. This is the chapter's
spine, and every rule below is a consequence of it.

Verifiable form: for every describable path `p`, `describe p` returns the same `Schema` that
(a) types the rows a read of `p` yields, (b) gates what a write to `p` accepts, and (c) shapes
what `ls p` enumerates. One type, three faces — a driver whose three faces disagree is a
conformance bug (§6's drift check).

*Types are sets, kept in full:* a type is a set of values; `WHERE` is refinement; `EXCEPT` is the
difference check; membership is the consistency check. There is no second schema language (Prisma's
external file), no validator runtime (plgg's TS machinery), and `ColumnType` never grows a domain
atomic. The discipline borrowed from plgg maps exactly: nominal `Box` tag ↔ the path;
`cast`/`forProp` composition ↔ predicate conjunction over a schema; `decodeRow` ↔ membership at the
boundary.

### 5.2 One vocabulary, one grammar — *canonical scalar vocabulary implemented; nested CREATE column grammar remains*

**The vocabulary** is `ColumnType` and nothing else: the lowercase tokens
`bool int float decimal text bytes timestamp date uuid json` plus the two recursive forms
`array<TYPE>` and `struct<name:TYPE,…>` (`qfs_types::ColumnType::{type_token, parse}` — encoder
and decoder of one format). **The grammar** is the §5 type literal:
`( <col> <type> [PRIMARY KEY|UNIQUE|NOT NULL], … ) [WHERE <pred>]` — one production.

**Everywhere a type appears, it is spelled in this vocabulary and this grammar** — a checkable
rule, not a preference:

- `CREATE TABLE` / `CREATE TYPE` column lists (implemented — bare tokens today; the nested
  `array<>`/`struct<>` forms extend them to the full vocabulary);
- transform `INPUT`/`OUTPUT` clauses (implemented — already the full vocabulary);
- lambda parameter annotations (`(x: text) => …`) — implemented with the same scalar/recursive
  vocabulary plus `Resource`; the `string`/`i64`-family spellings are retired and rejected by the
  plan-time checker;
- DESCRIBE output and the §14 result envelope's `schema` tokens (implemented — `type_token()`);
- driver declarations, `/sys/drivers` stored bodies, and the §16 SoT emission (implemented —
  `ColumnType::parse` rehydrates).

**Retired spellings** (qfs is pre-release: hard breaks, no deprecation shims):

- The lambda-annotation zoo. `typeck::param_type` now accepts only the canonical column type grammar
  (`text`, `int`, `float`, `bytes`, `array<int>`, …) plus the one non-column word `Resource` (a
  first-class value that is deliberately not a column type — it stays CamelCase and stays out of
  `ColumnType`). `string`, `i64`, lowercase `resource`, and `Row` are structured
  `unknown_type_annotation` errors. `Row` is retired with nothing replacing it: an unannotated
  parameter is already the honest spelling of "late-bound".
- `"string"` as a `ColumnType::parse` alias for `text` (schema.rs) is gone. One token per type; the
  shared surface helper likewise treats old SQL/Rust aliases (`integer`, `varchar`, `jsonb`,
  `bytea`, …) as non-base names rather than silently normalizing them.
- An unrecognised annotation no longer degrades silently to late-bound: under one vocabulary it is
  a **plan-time structured error** naming the accepted tokens. (Conservative-never-false-reject was
  the right posture while the vocabulary was plural; once it is singular, an unknown token is a
  typo, not a dialect.)

**`unknown` is a state, not a type** — the ruling for `ColumnType::Unknown`, which is today a live
vocabulary word (`reduce` returns it; a path navigated into `json` resolves to it; the envelope
prints it). It stays, precisely bounded: `unknown` is legal **in inference and description output**
(a sparse source, a `json` descent, an ungiven annotation — honesty about what is not yet known)
and **illegal in declaration position** (a `CREATE TYPE` / `CREATE TRANSFORM INPUT/OUTPUT` /
annotation may not declare a column `unknown` — a declaration is a contract, and `unknown` is the
absence of one; declare-time structured error). `ColumnType::parse` keeps accepting the token so
*inferred* schemas round-trip through storage; the declaration validators reject it.

### 5.3 Relation types are first-class: a pipeline is a typed composition — *checker implemented for predicates; per-stage rules the formal contract*

A relation type is `Relation<S>` for a schema `S`. **Every pipe stage has a typing rule of the
shape `Relation<S> → Relation<S'>`**, where `S'` is computable at plan time from `S` and the
stage's own syntax — this is the formal content the stage admission test (the two-layer section)
inherits: a construct that cannot state its typing rule cannot be a stage.

The rules for the closed core (`PipeOp`, exhaustive — the governance lock and this table move
together):

| stage | typing rule `Relation<S> →` |
| --- | --- |
| `where p` | `Relation<S>` — `p : S ⊢ bool` (refinement; checked, implemented) |
| `select c₁,…` | `Relation<project(S, c₁…)>` |
| `extend n = e` | `Relation<S + n : ty(e)>` |
| `set n = e` | `Relation<S[n ↦ ty(e)]>` |
| `aggregate …` / `group by` | `Relation<group-keys + aggregate columns>` |
| `order by` / `limit n` / `distinct` / `as a` | `Relation<S>` (schema-preserving) |
| `join r on p` | `Relation<S ⋈ S_r>` (collision-qualified concat — `Schema::join`) |
| `union/except/intersect r` | `Relation<unify(S, S_r)>` |
| `expand f` | `Relation<expand(S, f)>` (`Schema::expand`) |
| `decode fmt` / `encode fmt` | `Relation<bytes-row> ↔ Relation<S_fmt>` (codec-declared) |
| `call d.a(…)` | effect seam — `Relation<S> → Plan` (per `ProcSig`) |
| `transform t` | `Relation<S_in(t)> →[model] Relation<S_out(t)>` — the declared `OUTPUT` schema, an effect-bearing stage (§15) |

Two consequences, both already load-bearing: **pushdown is type-preserving** (a pushed stage and
its local residual satisfy the same rule — the truthful-residual doctrine of §6 is a typing
statement), and **the expression layer types under the row schema** (`typeck::check_expr`,
implemented: predicate/comparison/builtin/lambda checking at plan time, before any effect node
exists — a type-failing plan can never be committed). The checker's lattice keeps exactly two
non-column types — `Fn` and `Resource` — because a relation column is never a closure; they live
in the checker, not in `qfs-types`, and the leaf crate stays a leaf.

The plan-time checker (decision T / t75) is hereby **repositioned as this chapter's enforcement
arm**, not a bolt-on: its scope grows exactly as fast as the rules in this table are wired, and its
conservative late-binding (`unknown`) is the honest gap meter — every `unknown` the checker carries
is a rule not yet enforced, driven toward zero, never papered over.

### 5.3a The stage admission test — *implemented as the governance rule*

qfs is a **two-layer language**: a closed, first-order relational stage algebra over typed paths,
and a total, pure, row-scoped expression layer where functions are values. Every stage is notation
for a relation combinator the planner can see through (`where p` reads as
`filter(rel, (row) => p)`, `select` as projection, `extend`/`set` as row maps, set operations as
set combinators). The stage layer stays closed because preview, pushdown, type checking, and the
effect gate all depend on seeing the whole relational transformation.

A construct may become a pipe stage only if at least one of these is true:

1. **Pushdown translation** — a backend may execute it natively, and the planner can still keep a
   truthful local residual when it cannot.
2. **Plan-time schema rewrite** — it changes, asserts, or names the relation type
   (`select`, `extend`, `set`, `aggregate`, `expand`, `decode`, `encode`, `transform`, `of`).
3. **Effect gating** — it constructs a plan node or an irreversible/model-calling seam that
   preview/commit must see (`insert`, `upsert`, `update`, `remove`, `call`, `transform`).
4. **Cardinality or ordering semantics** — it changes row count, identity, grouping, ordering, or
   set membership in a way that affects downstream planning (`where`, `join`, set ops, `distinct`,
   `limit`, `order by`).

Everything else belongs in the expression layer as a stdlib function/lambda or under a path as data.
The current inventory passes this test: the 39 frozen keywords either form syntax around the closed
algebra/plan gate, and all 22 `PipeOp` variants satisfy one of the criteria above (`of` — the 22nd,
ticket 20260714154144 — satisfies criterion 2: it asserts/names the relation type). A future stage
proposal must cite the criterion it satisfies; if it cannot, it is not a stage.

### 5.4 Named types are definitions — the refinement model — *implemented at declaration and row-bearing boundaries*

**An entity type is a named definition: an intensional relation** — a schema plus an optional
refining predicate — declared in the definition layer, **stored at a catalog path, referenced by
name**, describable but not enumerable. A table is an extensional relation constrained to
`table ⊆ type`.

```sql
CREATE TYPE email (value text NOT NULL) WHERE value LIKE '%@%'

CREATE TYPE customer (
  id int PRIMARY KEY,
  email email UNIQUE,
  joined timestamp NOT NULL
)

CREATE TABLE /sql/shop/customers OF customer
```

- **The type literal** is §5.2's one production. `CREATE TABLE …(cols)` (implemented) is the
  **anonymous** type literal; `CREATE TYPE <name> <literal>` names one; `CREATE TABLE <path> OF
  <name>` attaches storage to a named one. `TYPE`/`OF` are contextual idents — zero new keywords.
- **One name namespace.** A column's type is a base `ColumnType` token *or a declared type name* —
  `email email` above; base and refined types resolve in **one namespace** (a declared type may not
  shadow a base token; declare-time structured error). Names may be multi-segment where a catalog
  nests (`chatwork/message` — a qualified name, written without a leading slash; the `/type` mount
  prefixes it). "Narrow atomics" (email, uuid-shaped text, non-empty) are user-defined refined
  types — `ColumnType` never grows.
- **`/type` is the catalog, not the reference** — *mounted 2026-07-15*. `ls /type` is SHOW TYPES;
  `DESCRIBE /type/customer` teaches the shape; install/update/remove are ordinary previewed writes to
  the catalog (`/sys/drivers` rows, `kind='type'`), so the mount is **read-only** (SELECT only) and
  never mints a second write path to those rows. But the *reference* — a column type, an `OF`
  clause, an `INPUT OF` — is always the **name**. The pipe and the DDL never apply a path (§5.5).
  The two faces meet at the catalog's `name` column: `sys_drivers` stores a type's key in its path
  form (`/type/chatwork/message` — what `of` normalises a bare name into), but the listing renders
  the **reference name** (`chatwork/message`), because listing the stored path would print the one
  spelling the grammar rejects.

**The refinement model, precisely.** The optional `WHERE <pred>` is a **row-local, pure, total**
predicate over the declared columns — the one predicate language, nothing new:

- **Declare time — well-formedness, statically checked.** At `CREATE TYPE` the stored predicate is
  type-checked against the declared columns (`typeck::check_expr`): it must type to `bool`; an
  unknown column, a non-boolean result, an aggregate, a table-valued/source builtin (`READ`,
  `http.get`), a context builtin (`NOW`/`CURRENT_DATE`/`LAST_RUN`/`env`), or any reference to
  another path/relation is a **declare-time structured error** — a malformed refinement fails at
  CREATE, never at first write. (Purity is a checkable registry property, not a convention: the
  stdlib's builtin categories are the machine-readable line — scalar builtins are admissible,
  everything else is not.)
- **Use time — membership, contract-checked.** A refined type checks as **membership at the
  write/`OF` boundary** where rows are available: a literal `VALUES` write into an `OF` table is
  checked per row by the SQL apply facet before the row is committed; a declared view's `OF` shapes
  and membership-checks the delivered rows (§13); a transform's `OUTPUT` membership-checks the
  model's returned rows (§15). A **pipeline-sourced** SQL write is likewise membership-checked: the
  commit boundary materializes the source pipeline and embeds its rows into the effect's `args`
  channel (`materialize_pipeline_source`, capped at `MAX_MATERIALIZED_ROWS`), which the SQL apply
  facet checks per row — so a shell `cp <src> <OF-table>` (= `UPSERT INTO <table> <src>`) is
  contract-checked at commit, not merely at a literal `VALUES` write. (This retires the earlier
  "until that seam carries rows" caveat — the seam carries rows.) Evaluation is the pure expression
  evaluator over the candidate row — no I/O; a failing row is a structured error naming the column
  and the predicate.
- **Explicitly NOT refinement typing in the proof-carrying sense.** qfs does not statically prove
  `∀ rows: pred` over arbitrary pipelines, carries no solver, and discharges no obligations. A
  refined type is **contract-checked like a CHECK constraint** — enforced at the boundary where
  rows exist, well-formedness-checked where the definition is declared. (Verification-readiness
  survives as a representation constraint: qfs forms a Kripke structure — Worlds are database
  states, committed Plans the accessibility relation; type safety is □(table ⊆ type). A type is
  declarative data — schema + predicate as a stored, span-normalised AST — never opaque executable
  code, so a future model checker's door stays open. That door is the only promise.)
- Predicate scope grows only with the expression layer: arithmetic operators are in the row-local
  pure expression layer, so `WHERE abs(price - list_price) < 5.0`-shaped refinements come through
  that one path — the refinement language is never versioned separately. Arithmetic is deliberately
  monomorphic: `int +|-|* int -> int`, `float +|-|*|/ float -> float`, mixed numeric operands are a
  plan-time type error, and `int / int` is rejected until an explicit integer-division or cast rule
  exists. There is no implicit promotion. `%` remains out of the operator set. Backend pushdown for
  arithmetic predicates is a follow-up IR/driver feature; the current lowering boundary rejects
  arithmetic predicates structurally instead of pretending they are pushable.

**Consistency is membership; drift is set difference; redefinition, not migration** — all three
carried forward verbatim from the 2026-07-04 decision: reads decode by the same membership; the
introspected live catalog reconciles against the declared type and a table mutated outside qfs
surfaces the mismatch structurally in DESCRIBE; redefining a type previews a derived reconciliation
plan and anything destructive rides the irreversible gate.

### 5.5 The reference rule: paths are data, names are definitions — *ruling; type/view/transform grammar implemented*

**The stage-operand category rule.** A stage verb's operand has a category, decided by what flows:
it is either **inflowing data** — a path, because a path is a typed data-location
(`from /mail/inbox`, `join /sql/erp/products`, `union /path`) — or a **behavior-selector
parameter** — a bare name that selects a registered behavior (`decode json`, `call mail.send`,
`transform triage`). **A path in selector position is a category error**: data-location syntax
naming a function. The converse holds too: a bare name in data position is a `LET`-binding
reference, never a hidden path.

The Unix analogy is exact: definitions are *stored* at catalog paths (`/usr/bin/grep`) and *invoked*
by name (`grep`); the catalog is the shell face (`ls /transform`, `describe /type/customer`,
previewed install/remove writes, provenance records the catalog path), the name is the language
face — **the pipe never applies a path.**

Consequences, ruled here and applied by the sibling tickets:

- `|> transform <name>` — matches the shipped grammar (the T1 parser already takes a bare ident);
  blueprint §15's `/transform/…` stage and DDL examples are corrected to the name form.
- **Type references are name-ified**: `of customer` (not `of /type/customer`), a column type
  `email email` (not `email /type/email`), `create type customer` (not `create type
  /type/customer` — the `TYPE` noun implies the `/type` mount, exactly as shipped `CREATE TRANSFORM
  <name>` implies `/transform`). `INPUT OF message` in §15 likewise.
- **Selector resolution is registry-scoped**: the stage/clause word names the registry the bare
  name resolves in (`decode` → codecs, `call` → procedures, `transform` → transforms, a type
  position → the type namespace) — so one short name is unambiguous per position, and the
  namespaces never pool. `decode json` resolves in a *frozen* vocabulary; `transform triage` and
  `of customer` resolve in *mutable* catalogs — same rule, different registry, and a nested catalog
  qualifies the name (`chatwork/message`), never re-admits the path.
- **The `CREATE VIEW` dispatch is principled, not a pun**: a **path** name declares a readable data
  surface (a declared view — data lives there); a **bare** name declares a server binding (a
  definition). Path = data, name = definition, in the same statement.

### 5.6 `of <type>` — the use-site assertion — *implemented (ticket 20260714154144)*

`of <name>` is a **general, any-position, plan-time-checked type assertion** — the `create table …
of customer` vocabulary generalised, never a transform special case:

- In DDL: `create table <path> of <name>`, `create view <path> of <name> as …` (§13), `INPUT OF
  <name>` / `OUTPUT OF <name>` (§15) — attaches a named type as the contract.
- Mid-pipe: `… |> of customer |> …` (a named type) or `… |> of (priority text, reason text)` (an
  inline anonymous structural literal, the §5.2 column-list production) — asserts the relation's
  type at that point. The `transform triage |> of (…)` twin is exactly this stage following a
  transform: `of` is its own `PipeOp` (the 20th, §5.3a), never a transform-coupled suffix.
  **Checked at plan time** against the stage's computed schema (§5.3's rules make the schema known
  at every seam): a mismatch is a plan-time structured error (`of_assertion_failed`) naming the
  differing columns — the missing, the unexpected, and the type-mismatched. A named type is resolved
  from the plan-time declared-type registry (the `transform_defs` twin; the pure planner cannot read
  the System DB), so an unknown name is a structured `of_type_unresolved`. Where the asserted type
  carries a refinement, the structural half is plan-checked and the predicate half is membership at
  the next boundary rows exist (§5.4's honest split, restated at the use site): a bare mid-pipe read
  assertion enforces structure only and does not pretend a static proof over rows that do not yet
  exist. A column left `unknown` on either side (a late-bound `extend`/`set` column) is
  conservatively skipped — the honest gap meter, never a spurious failure.
- The assertion is **locality and readability with teeth**: a long pipeline states its contract
  where a human reads it, and the planner holds the author to it. It never coerces — `of` asserts,
  `select`/`extend` transform.

### 5.7 Rejected

An external schema file; CALL-based validation; new `ColumnType` atomics; a type registry beside
the path tree; migration tooling; blocking cross-driver enforcement in v1; **path-form references
to definitions** (`of /type/x`, `transform /transform/x` — the category error); **a second
type-spelling anywhere** (the annotation zoo, the `string` alias); **`unknown` in declaration
position**; solver-discharged refinement (out of scope — contract checking is the model); opaque
executable types (a type is declarative data, always). Noted as consequence, not commitment: a qfs
type can render a plgg validator (one entity definition serving both stacks).

### 5.8 Considered alternatives — the transform stage surface *(recorded verbatim from the mission, 2026-07-09; baseline is `|> transform <name>`, settled by §5.5)*

- **Bare-name application** `|> triage` — pipelines read as pure function composition and unify with
  future pipeline-valued lambdas (user-defined stages). **Deferred as the possible endgame**: only
  decidable after this chapter, because dropping the call-site word is sound only if effects ride
  the definition's type (`Relation<S> →[model] Relation<S'>`); also costs a stage-word/user-name
  namespace collision.
- **Expression-layer call** `|> extend p = triage(subject, body)` — **rejected permanently, reason
  recorded**: it would be the single hole in the expression layer's pure/total/row-scoped cage,
  undefine "describe/preview touch nothing" and expression totality, and break the one-seam rule.
  Expect this to be re-proposed; cite this verdict.
- **Inline anonymous transform** (`|> transform (subject text, body text) => (priority text, reason
  text) prompt '…'`) — the model-flavored twin of the language's named/inline lambda duality;
  plausible **future sugar** for throwaway exploration, blocked until provider/model/secret can come
  from session defaults (stored-only is correct for v1's auth/audit story).
- **Use-site contract annotation** `|> transform triage of (priority text, reason text)` — not an
  alternative but a readability/locality annotation, checked at plan time. Treated in §5.6 as the
  **general** use-site type-assertion rule, never a transform special case. Highest design leverage
  of the four.

### 5.9 Pipeline-valued lambdas — the sanctioned genericity axis — *blueprint*

Adopted as the direction for reusable, user-defined stages, but not yet shipped syntax. Genericity
belongs over **typed pipelines**, not by making predicates opaque. A future pipeline-valued lambda
has type `Relation<S> -> Relation<S'>`; its body is a closed read/transform algebra over the input
relation, preserving static typing, pushdown analysis, preview/commit purity, and the effect gate.

Slice plan:

1. **Relation-typed closure model** — represent a lambda body as either expression-valued or
   pipeline-valued, with the checker computing `Relation<S> -> Relation<S'>`.
2. **Grammar slice** — allow a `let`-bound lambda body to be a pipeline over its relation
   parameter; no new keyword and no effect stages inside the body.
3. **Application slice** — add explicit stage-position application of a relation-typed closure.
   The transform bare-name endgame and user-defined stage syntax must share one resolver, but bare
   `|> hot` is not admitted until the name-collision and effect-typing rules are settled.
4. **Planner slice** — inline/lower the body as typed relational algebra so existing pushdown and
   truthful-residual rules still apply.

This is a future implementation plan, not a current behavior claim.

## 6. Driver contract — *implemented; types re-founding: blueprint*

A driver declares — and the declaration is everything the engine and the AI need:

- **Namespace** (path tree) and per-node **archetype + schema** (powers DESCRIBE). Four
  archetypes: blob/namespace, relational/table, append/log, object-graph+workflow. One driver
  may expose several (git is three).
- **Capabilities** — which verbs each node supports; unsupported ops are rejected **at parse
  time** with a structured error.
- **Procedures** — the `CALL` registry (irreversibility declared per `ProcSig`).
- **Pushdown** — what the driver executes natively. The compiler keeps a **truthful residual**:
  a predicate that cannot be faithfully pushed is re-filtered locally — never wrong rows.
- **Prelude** — optional pure alias functions (`SEND`).
- **Relations** *(blueprint, with §5)* — declared edges as first-class DESCRIBE metadata beside
  the schema, so `describe` returns `(schema, relations)`: a `/sql` foreign key and a markdown
  collection's declared heading (§13b) are two instances of one **relation vocabulary**. A
  relation is what the address grammar traverses as a relation segment and what qfs-viewer draws
  as a clickable edge lowering to a JOIN (§14b) — a human click, an AI tool call, and a
  qfs-query stage are one move.

**Types re-found the schema surface** *(blueprint, with §5)*: a driver's output nodes declare
their shapes **as types in the one namespace**, so DESCRIBE reports type references, a
third-party driver's rows are typed by the same membership as a table, and one entity type can
serve a `/sql` table and a driver surface simultaneously. Third-party drivers register types
through the path registry (no fourth registry); declared-type-vs-delivered-rows conformance is
§5's drift reconciliation, honestly surfaced for drivers the binary does not compile.

Drivers themselves stop being compiled-only: **§13 lifts the driver definition into
definition-layer data**, so a new service is a declared script, not a Rust crate.

## 7. Runtime — *implemented*

Effect plans are typed DAGs with per-node `irreversible` flags and honest affected-count
estimates. **PREVIEW is the default** (pure, no I/O); `--commit` applies; **irreversible effects
additionally require `--commit-irreversible`** — `REMOVE` is inherently irreversible; `CALL`
irreversibility rides the procedure declaration. A single-source plan commits as one real ACID
transaction; cross-source plans are orchestrated best-effort with explicit partial-failure
recovery and a hash-chained audit ledger of every applied effect. `UPSERT` is the retry-safe
verb (at-least-once ingestion converges). Concurrency is bounded two-level (global / per-driver)
back-pressure; per-leg timeouts and bounded retries apply only to reversible legs.

**Write-source materialization at the commit boundary** *(implemented 2026-07-05, ticket
20260704164315)*: a write whose source is a pipeline/`FROM` query (`… |> upsert into <dst>`,
`INSERT … FROM`) materializes at **commit** time, above the interpreter — the exec layer
re-executes the query side through the read engine (already cross-driver) and embeds the produced
rows into the write effect's `args.rows`, the same channel `VALUES` writes use. The source `Read`
node is **consumed** there (dropped from the applied plan, its rows now in the write's args); drivers
never see it — so a single-file source `Read` is never dispatched as a directory scan (the `ENOTDIR`
that blocked every blob copy). A genuine driver read-*effect* (the REST `GET`-at-commit) is untouched
— only a pipeline source feeding a write is consumed. The interpreter/driver contract stays **payload-free** (`EffectOutput` = `{id, affected}`;
the audit ledger records metadata only, never payloads), and the interpreter never re-executes
query stages — no engine→runtime inversion. (§15's `|> transform` rides this same exec-layer
channel: its model-produced rows flow *above* the interpreter, and `EffectOutput` stays
`{id, affected}` — see §15's routing ruling.) A named payload cap refuses over-size materializations
with a structured error naming the in-driver remedy (`cp`, `CALL drive.copy`). Same-driver `Copy`
pushdown and streaming are **named parks** (optimizations, not correctness).

**The selector channel** *(designed 2026-07-14; fully implemented 2026-07-15 — increments 1 and 2,
tickets 20260713195008 + 20260714120000)*:
an effect node used to carry a single `args: RowBatch`, and the core collapsed `SET` and `WHERE` into
it — `setwhere_row_batch` drops a `WHERE` key that shares a `SET` column, so `SET name='X' WHERE
name='Y'` lost its selector and a driver could not tell "rename the matching child" from "rename the
container" (gdrive refused it loudly since v0.0.60; SQL — and Cloudflare D1, which mirrors the same
`split_update`/`build_key_where` path — paper over the shape by inferring the `WHERE` from
primary-key membership rather than the operator's clause). The design: give `EffectNode` a second
channel, **`selector: Option<RowBatch>`** (a one-row batch of `col == const` equalities), lowered from
the `WHERE` distinct from the SET/`VALUES` payload, so drivers read `selector` to resolve *which*
rows/nodes and `args` for *what* to write. Multi-match is a **per-driver policy, not a representational
rule**: node-resolving drivers (gdrive, object stores) refuse `AmbiguousTarget` on ≥2 matches
(refusing beats a wrong-node write, mirroring `resolve_node`); relational drivers (SQL) update the
whole matching set as SQL does. Equality-conjunction only (richer predicates stay the read-path
residual); the `#[non_exhaustive]` node may grow a richer selector later. The grammar is unchanged
(the AST already separates `SET`/`WHERE` — this is a plan-lowering + applier change).

*Increment 1 (landed)*: the `selector` field + its `with_selector` builder, the **additive** lowering
(the `WHERE` is populated onto `selector` while `args` stays exactly as before, so every existing
applier is untouched and no golden churns), the **gdrive** consumer (a name-path folder `UPDATE`
with a single `name` selector renames the matching child via the ambiguity-safe `resolve_node`/
`existing` path — refusing `AmbiguousTarget` on ≥2 — instead of the safe refusal; a non-`name` or
absent selector keeps refusing), and the selector's preview surfacing (` where <keys>`, key columns
only, secret-free).

*Increment 2 (landed 2026-07-15, ticket 20260714120000)*: the lowering is now **uniform** and the dual
convention is retired. `setwhere_row_batch` is SET-only, so **`args` is purely the payload** (what to
WRITE) and **`selector` is purely the match** (what to write it TO) — the one channel a filter travels
on. A `REMOVE` writes nothing, so its `args` is now genuinely **empty** and its match is wholly the
selector. Every applier that read a `WHERE` key out of `args` migrated to `EffectNode::selector_value`
/`selector_text`: SQL and CF D1 (**PK-inference retired for the match** — the operator's real `WHERE`
is honoured as written, so a non-key `WHERE status == 'stale'` is no longer silently ignored and a
same-column `SET id = 2 WHERE id = 1` is expressible; UPSERT `conflict_keys` stay PK-based, since
retry-safety is a separate concern from the match), CF KV, gmail, slack, transform, sys, and the
sql-catalog `DROP TABLE`. `Driver::plan_write` gained a `selector` parameter so "uniform for every
write verb" is true at the driver seam too. The floor is enforced at the source: the eval tests assert
a filtered `UPDATE`'s `args` carries the SET columns ONLY, and a `REMOVE`'s `args` is empty — if a
`WHERE` key ever reappears in `args`, they fail.

## 8. Authorization & accounts — *implemented*

**The multi-host account model** *(implemented)*: the CLI is a client of hosts; **local is an
implicit host** (OS login is its authentication; one `$HOME` = one operator). Remote hosts
(self-hosted or managed) authenticate with real sessions. The account layers have their own
verbs — `qfs init / host / app / account / connect` — and **selection state is abolished: the
mount carries `(host, driver, account)`**, so a statement's identity target is readable from the
statement. The vault key lives in guardian slots (passphrase today; keychain/agent/managed-KMS
as slots, not forks).

**The host-realm path canon** *(ruled 2026-07-16 by the owner; implemented 2026-07-17, mission
`claude-code-sessions-are-queryable-and-steerable-as-qfs-paths`)*: a host-scoped service is
addressed **`/hosts/<host>/<svc>/…`**, and the engine peels `/hosts/local` generally — any
service mount resolves under the local host's realm, never a per-driver special case. Today the
one host-scoped service is the machine's **Claude Code sessions**:
`/hosts/<host>/claude/sessions` (+ `…/sessions/<id>/instructions`), and **top-level `/claude/…`
is retired** — the t64 ticket had ruled `/hosts/<host>/claude/...` canonical in its own title
while the shipped code mounted bare `/claude`, and the contradiction was never reconciled until
this ruling; qfs is experimental, so the retirement is a hard break: the bare spelling fails
with a structured `retired_path` error naming the canonical form (one canonical address per
surface — a retired alias fails with a pointer, never a silent second path). A **non-local**
host fails closed as `remote_host_not_executable` — the cross-machine hop rides the t63 tunnel
and re-checks POLICY at the destination, a documented seam that is not wired (the same
fail-closed posture as `qfs connect --host <remote>`). Other realms (`/members`, `/projects`,
`/directories`) keep their existing behaviour — the peel deliberately widens nothing.

**The store boundary** *(re-drawn 2026-07-16, ticket 20260716143641)*: **one file holds secret
material; the other holds everything declarative, plus the ledger that observes it.** The Project
DB is the vault proper — `secret_store`, the key slots, rotation, E2E wraps. The System DB holds
the declarative configuration (`path_binding`, `connection_consent`, `sys_drivers`, policies,
settings) *beside* `audit_tail` and `sys_ddl_events`, so every config write — `CONNECT` /
`DISCONNECT`, account declare/remove, driver install — lands its row, its audit event, and its
replayable DDL event in **one** transaction. (Historically the two config tables lived in the
Project DB and their writes could only append a best-effort post-commit audit event — two WAL
files share no transaction; moving the tables dissolved that class instead of bridging it.)

**The subject model** *(blueprint framing over the shipped PBAC engine)*: a **principal** is any
actor a policy's `FOR <subject>` clause names — a human, or an AI (a bot, an issued API key) — and
the two are **the same kind of subject**, evaluated on one path through one policy engine; there is
no separate AI authorization path. (This is the mechanism under the product's "MCP server conscious
of its user": an agent reaches a resource by the same route and the same check as a human.) Because
the resource unit is the **path**, one policy binds every face at once — query, console, HTTP
endpoint (§10), and the viewer's trail (§14b) — so per-face permission drift cannot arise
structurally. Authorization is IAM-shaped: **PBAC** (the shipped path-scoped `CREATE POLICY … ALLOW
… FOR … AT …` grants below) combined with **RBAC** (roles bundle subjects). *RBAC as a grant stays
blueprint*: a role today is an invitation label, deliberately **not** a grant — which role, if any,
confers admin is an open product decision, so the who-axis wiring (carrying the request actor down
`ReadDriver::scan` to the policy's who-axis) and role-derived grants are a named seam, not shipped.
**Open** — the finer policy semantics: explicit-deny precedence, the evaluation point when a trail
crosses a derived reverse edge, and the permissions of the management paths themselves.

**Policy grants are path-aware** *(implemented 2026-07-04, ticket 20260704110923)*:

- The grammar is already sufficient: `CREATE POLICY … ALLOW <verbs> [ON <driver>]
  [FOR <subject>] [AT <path-glob>] [WHERE <cond>]` parses today (t57). **No language change.**
- The runtime `CapabilitySet` grant tuple carries an **optional path scope** —
  `(driver, verb, Option<PathScope>)` — matched by segment-glob against the effect target
  (`*` = one segment, trailing `**` = subtree), so DDL on `/sql/<conn>` and DML on
  `/sql/<conn>/<table>` are told apart though both are `(sql, INSERT)`. Unscoped grants keep
  matching any path (**additive**: no existing policy narrows silently). The scope means the same
  thing at both enforcement layers — the server policy engine (`ScopeGlob`) and the runtime
  re-check (`PathScope`) share the segment-glob semantics; a capability denial names the path.
- This makes ADR-0009 §6's matrix enforceable: *data-only* (DML on tables, no catalog writes),
  *read-only*, and *admin* are path-level grant sets. The policy layer and the irreversible
  gate remain **independent** controls — either alone refuses a destructive DDL, and a policy
  denial is never conflated with a missing `--commit-irreversible`.
- Scope: policy enforcement is the server's concern (handlers run under `CREATE POLICY`);
  one-shot CLI commits stay `allow_all` — the local operator holds every right (§8's degenerate
  case).

## 9. CLI & console — *implemented*

One-shot: `qfs run '<stmt>'` (preview by default), `qfs describe <path>`. Interactive: the
FTP-like shell (bare `qfs`) — `ls/cd/cat/cp/mv/rm` + raw pipe-SQL, every builtin desugaring to
the same closed-core statements and the same preview/commit pipeline (**the shell adds no
execution semantics**). The console covers database work with no mode: `CREATE CONNECTION`
creates+mounts a SQLite database, `CREATE TABLE`/`REMOVE TABLE` manage schema, reading
`/sql/<conn>` is SHOW TABLES.

**The shell face is the typed-path space's navigation/mutation face** (ruled 2026-07-09/07-14,
mission `language-design-review-layering-principles-and-semantic-gaps`; owner: adopt). It is a
REPL-layer desugar, never grammar — the verbs are recognised only at the line head, are NOT among
the 39 frozen keywords, and `cwd` is session state absolutized before parse (so the pure engine
stays stateless and absolute-path-only). Two verb classes: **pure navigation** (`ls`/`cd`/`cat`/
`describe` — touch nothing, build no plan) and **gated mutation** (`cp`/`mv`/`rm` — lower to
`insert`/`upsert`/`remove`/`update` and ride preview→commit like any write; a builtin can never
shortcut the gate). **Verb semantics derive from the path's ENTRY KIND** (its `describe` archetype):
`ls` over a blob namespace enumerates file rows (`name/size/is_dir/modified`), but over every other
kind the path's rows ARE its enumeration, so `ls` is the bare read — a relational table's rows, a
definition catalog's defs (`ls /transform` = SHOW TRANSFORMS, `ls /type` = SHOW TYPES), an append
log's tail, an object graph's entities *(entry-kind-typed `ls` implemented 2026-07-14, ticket
20260714182710)*. `cp` into an `OF`-typed table is membership-checked per row at commit (§5.4); a
data-row `cp` into a definition catalog is a category error (§5.5). The `/type` catalog mount and the
`describe` builtin are **implemented** *(2026-07-15, ticket 20260714182740)*: `/type` mounts as a
read-only catalog over the `kind='type'` declarations, so `ls /type` = SHOW TYPES is now true in the
binary, and `describe [path]` folds a node's contract in-session — the same report the one-shot `qfs
describe` renders, plus the one thing the one-shot form cannot do (it addresses the cwd, and resolves
a relative path against it).

The **enumerable-children `cd` gate is implemented** *(2026-07-15, ticket 20260714182720)*. A node is
enterable iff its **children are locations**, and that is a fact the DRIVER states per path
(`NodeDesc::navigable`), never a shell-side heuristic. It cannot be derived from the archetype: a
navigable catalog interior and a row leaf report the SAME one — `describe /transform` and `describe
/sys/drivers` are both `relational_table` — because the archetype says what a node's ROWS look like,
not whether its CHILDREN are locations. The two are orthogonal, so an `AppendLog` can be enterable
(the gmail label tree: `cd /mail` then `cd INBOX`, while the archetype correctly stays `AppendLog` —
`ls` is archetype-typed, so calling the root a blob namespace would break it). `navigable` defaults
from the archetype (blob/object-graph true, table/log false), preserving the old gate's behaviour
wherever a driver says nothing; `/transform`, `/type`, `/sql/<conn>` and the mail label tree opt in
per path. Entering a **row-set is still refused** — rows are values, not locations. (Two nodes remain
un-enterable for an unrelated reason: `/sys` and `/slack` do not describe their ROOTS at all, so a
`cd` there fails at describe; and `cd` into a blob FILE is still admitted, because a pure describe
cannot stat the path to tell a file from a directory — both are §5.1 driver-conformance follow-ups,
not gate bugs.)

The **per-entry-kind mutation ruling is implemented** *(2026-07-15, ticket 20260714182730)*, entirely
behind the shipped preview→commit gate — no new gate, no new plan-node type. **`cp` is keyed on the
DESTINATION's entry kind**: blob → `UPSERT` (idempotent, retry-safe — the recovery shape `mv` depends
on); every other kind → `INSERT`, because an idempotent "send" into an append log is a lie (`UPSERT`
claims a key it cannot match). That is what makes "`cp` ≡ membership-checked `insert into`" literally
true for an `OF`-typed destination — the shipped `materialize_pipeline_source` → args →
`check_table_membership` chain then polices every row at commit. **`mv` is same-kind-only**: blob→blob
copy→verify→delete, and every other combination is a structured refusal that NAMES the honest
spelling. The refusal is the point — `mv` on a mail path as copy+delete means **send a new message to
a third party and trash the original**, so it names `UPDATE … SET labels` instead; a row "move" names
`UPDATE`. **The two categories never pool** (§5.5): `NodeDesc::category` (`Data` | `Definition`) is,
like `navigable`, a fact the driver states, and copying data rows into `/type`/`/transform` is a
`category_error`, not a schema mismatch.

Two of that plan's def-catalog verbs proved **inexpressible and are refused instead of faked**: a
definition row CARRIES its own name, so `cp /transform/a /transform/b` would re-insert `a` rather than
clone it to `b`, and neither catalog offers an in-place rename (`/type` exposes no write verb at all —
a type is installed through `/sys/drivers`; `/transform` has no `UPDATE`). Both name the one spelling
that works: re-declare under the new name. `rm` needs no ruling — `REMOVE` is `REMOVE` for every kind,
and on a catalog it already IS the drop, irreversible-gated. `mkdir` stays deferred: only Drive has
real folder semantics, so a verb that is a category error on four of five entry kinds has not earned
the slot.

## 10. Server — *implemented (daemon); Workers host parked*

The server is a driver: endpoints, triggers, jobs, views, policies, webhooks are **data** under
`/server/…`; the `CREATE …` binding forms desugar to writes there. Bindings are "what causes a
plan to run": `ENDPOINT` (HTTP), `TRIGGER` (event), `JOB` (schedule), `VIEW`/`MATERIALIZED VIEW`,
`WEBHOOK` (ingest). The daemon host formalizes serving behind the `RuntimeHost` seam with an
fsync-safe durable store and a persistent audit ledger. Because bindings are data, the whole
configuration is **fetchable and reconcilable as one document**: `qfs dump` (secret-free JSONL
fetch of the system-DB state) and `qfs restore` (preview-by-default, insert-or-skip additive
replay) are implemented, and §16 promotes that pair into the declarative `qfs plan` / `qfs apply`
reconcile loop *(blueprint)*. The Cloudflare Workers host maps
`ENDPOINT`→fetch, `JOB`→Cron, `WEBHOOK`→Queues, watcher→Durable Object — **parked** until the
`worker` crate is buildable in the release pipeline (no wasm entrypoint ships today).

**Decision (2026-07-11) — in-server JOB firing, reversing t65.** t65 ruled "qfs is not a
scheduler": a `CREATE JOB … EVERY … DO …` row was a *saved named plan + cadence* that an EXTERNAL
scheduler (OS `cron` via `qfs job run`, Cloudflare Cron Triggers) invoked; the internal `qfs-cron`
daemon binding was retired (e188846). The owner has reversed that ("Changed mind, we need this"):
the qfs **daemon** now owns the "when" beside the "what" it already serves (endpoints, triggers,
webhooks). What t65 got right is kept: **no scheduler *library*, and no wall-clock in the pure
core** — the firing DECISION (`qfs_watchtower::cron`: `interval_secs` / `is_due` / `fire_due`) is
pure over an injected `now`, wasm-clean, and driven by an injected clock in tests; only the daemon
leaf reads `SystemTime::now()`, exactly like the watchtower bus. A firing reuses the shipped
watchtower chain verbatim — the injected `Committer` runs the policy gate (default-deny for a
policy-less job) and the `IrreversibleGuard` (`RunMode::Server` refuses an unattended REMOVE/CALL),
so scheduling **bypasses neither gate**. **Ruled semantics**: missed-fire = *skip* (a long outage
collapses to one catch-up fire, `last_run` jumps to `now` — no storm); overlap = *skip-if-running*;
timezone = *UTC only*; at-least-once + idempotency inherited from the committer + ledger. **Scope**:
daemon-only — the CF Workers host keeps platform Cron Triggers as its "when", so the reversal adds
no wasm/portability burden. The whole chain now ships: the pure sweeper + fire decision
(`qfs_watchtower::cron`, hermetic injected-clock tests), the daemon `tokio::time` interval that
drives it on a real clock with the LIVE committer (`crate::sweeper` in the binary — sweeps are
sequential, so overlap-skip holds structurally), the durable `last_run` high-water mark
(re-hydrated on every sweep, so a restart never re-fires early), and the relational
`/server/jobs/<name>/runs` read-back — a READ-ONLY telemetry collection deliberately outside the
closed `ServerNode` write coordinates (select-only capabilities; the history is capped and dies
with its job row). Firings also append to the audit ledger, so a denied run is visible both ways.
The remaining owner-attended live round only *verifies* this on a running daemon (`qfs serve`
firing a 1-minute JOB, then reading the runs collection back).

## 11. Build decisions — *implemented (locked, each behind a reversibility seam)*

One decision shape governed all of these: lean single binary, wasm-clean pure core, no heavy
vendor SDKs, owned DTOs at every boundary — measured, not assumed.

- **Parser: `winnow`**, confined to `qfs-parser`'s private grammar module; the public surface is
  an owned `ParseError` (byte span + expected set + machine code) and owned AST. Chosen over
  chumsky on token-level (vs char-level) expected-sets for the AI structured-error path.
- **Local combine engine: in-house `MiniEvaluator`** behind the `CombineEngine` trait; DuckDB
  rejected (C++ build, no `wasm32-unknown-unknown`). It runs only the cross-source residual —
  pushdown gives each backend the heavy lifting.
- **git access: in-house loose-object reader** (pure-Rust DEFLATE + SHA-1 + object framing)
  behind the `ObjectDb` seam; `gix` not taken. Correctness pinned by committed-real-git-bytes
  differential fixtures; pack-delta reading is a named park behind the same seam.
- **HTTP serving: in-house HTTP/1.1** over `tokio::net` behind vendor-free
  `HttpRequest`/`HttpResponse`; axum not taken. Loopback bind by default.
- **Host adapter: the `qfs-host` seam** (`RuntimeHost`/`DurableStore`, owned DTOs, wasm-clean);
  the daemon's `TokioHost` reuses the existing serve composition; the Workers host is parked
  (§10).
- **Test harness: in-house** canonical-JSON goldens (`QFS_BLESS=1` to re-bless, credential-shape
  scrub on every fixture), a seeded property corpus, and a no-socket `MockHttp` — insta/
  proptest/httptest not taken. Everything provable offline and on wasm's pure subset.
- **Docs are generated, never hand-edited**: `docs/{language,drivers,server}.md` render from the
  binary's own registries (`gen-docs --check` in CI); Agent Skills render from the cookbook
  articles (`gen-skills --check`), and every cookbook recipe is parse-checked against the shipped
  grammar (the ratchet). The `xtask` crate is the build tool; release builds are CI-only.

### Dependency posture — *the decision log*

Governing rule: **implement by default; take an external dependency only when a criterion is
*clearly* met, and record its Reason / Assessment / Monitoring / Exit** (Conservative Vendor
Dependence). The make/take calls above already discharge the "implement by default" half — parser,
combine engine, git reader, HTTP server, and test harness are all in-house. The load-bearing
*taken* set is logged below, grouped by the criterion that admits it. Structurally every one sits
behind a seam: the `driver-*` crates are the anti-corruption layers, the pipe-SQL language is the
domain vocabulary, and `tokio`/`futures`/`async-trait` are confined to `qfs-runtime` (enforced by
`qfs-plan`'s purity dep-closure test). Direct third-party deps: **~30 shipped** (48 shipped internal
crates + the build-only `xtask` + the throwaway `parser-spike`); the transitive weight lives almost
entirely under `reqwest` + the three DB drivers + `tokio`, so the real reduction lever is *feature
trimming those*, not shaving convenience crates.

**Re-measurement (2026-07-11, v0.0.54 — ticket 20260711121531→121533).** Snapshot at HEAD: **50
workspace members** (48 shipped `crates/*` + `xtask` + the `publish = false` `parser-spike`); the
`qfs` **binary's normal-edge tree is 334 crates**, the full-workspace default-feature resolution is
**466**. Direct third-party deps counted across the workspace = **31**, but that figure includes
`chumsky` — which lives ONLY in the throwaway `parser-spike` (comparison evidence, never shipped), so
the "chumsky rejected" call above stands; the real shipped set is **30**. **Growth attribution since
the ~29/47 baseline**: the +1 shipped internal crate is platform work (the M7 `qfs-tunnel` /
identity-session leaves), NOT the file-handling/transform mission — the mission's tickets
(file-handling, the reply/extraction/chain flows, `AUTH ACCOUNT`, in-server cron) added **no new
third-party crate and no new workspace crate** (in-server cron is a module inside `qfs-watchtower`;
the transform document-input leg reused the already-present `base64`). The one **previously-unlogged
shipped dep** the re-measure surfaced is **`uuid`** (below) — drift from the SQL epic, now recorded.
**Levers — status**: the named trim levers are **already executed** where safe — `reqwest` is
`default-features = false, features = ["json", "rustls-tls"]` (no native-TLS, no `blocking`),
`mysql` is `default-features = false, features = ["minimal"]`, and `postgres` carries an *explicit*
feature set (the rich-OID decode that pulls `uuid`); `rusqlite` needs `bundled` (a system-dep-free
artifact). So **removable-today ≈ 0** additional: the heavy roots are already pinned to their used
feature set, `tracing-subscriber`'s `env-filter` is a real `RUST_LOG` capability (kept), and
`async-trait` remains the monitored exit. No trim was executed this pass because none was available
without dropping a used capability — the honest ruling, recorded rather than forced.

**Re-measurement (2026-07-14, v0.0.62 — ticket 20260711121533).** Snapshot at HEAD:
**50 workspace members**, unchanged from v0.0.54 (48 shipped `crates/*` + `xtask` + the
`publish = false` `parser-spike`). Direct third-party deps across the workspace = **31** including
`chumsky` (throwaway `parser-spike` only), so the shipped set is **30** — *identical* to the
v0.0.54 count, crate-for-crate (the set is enumerated in the bullets below). Tree size, re-taken
with an explicit reproducible method — `cargo tree -p qfs --edges normal` on the host target with
default features, deduped by `(name, version)`: **binary 356 crates, full-workspace 363**. (These
supersede the v0.0.54 `334/466` pair, whose method was unstated and not reproducible; the *shipped
direct-dep count* is the method-independent figure and is directly comparable — flat.) **Growth
attribution since v0.0.54**: **zero new third-party crates and zero new workspace crates**. Every
mission delivery in the window — the file-handling capability rounds (Slack/Chatwork/Drive bytes,
Gmail→Drive transfer, the `att<N>` stable-index read), the transform chain/switch/PDF flows, the
`AUTH ACCOUNT` declared-driver work, and the in-server sweeper/scheduling fixes — reused
already-present crates (`base64`, the wire mount, `qfs-watchtower` modules); none added a dependency
edge, confirming the mission-scope-does-not-widen-the-tree attribution held all the way to v0.0.62.
**Levers — re-verified**: the heavy roots remain pinned to their used feature sets — `reqwest`
`default-features = false, features = ["json", "rustls-tls"]`, `mysql` `default-features = false,
features = ["minimal"]`, `postgres` an *explicit* OID set (`with-serde_json-1`, `with-time-0_3`,
`with-uuid-1` — `uuid`/`bytes` ride it), `rusqlite` `bundled`; `tracing-subscriber`'s `env-filter`
is still a live `RUST_LOG` capability (`EnvFilter::try_from_default_env` in `qfs-cmd`, kept). The
flat:span ratio has widened to **111 events : 3 spans**, so the opportunistic "slim
`tracing-subscriber` to a minimal formatter" lever is *more* available than before — but it is
**defer-with-reasoning**: a hand-rolled formatter would have to re-implement `RUST_LOG` env
filtering to avoid dropping a used capability, a net complexity add for no dependency removed
(`tracing-subscriber` stays via `tracing` regardless). `async-trait` (still 61 `dyn …Driver` sites)
remains the one **monitored exit** — adopt native `dyn` async-trait dispatch when it is ergonomic
in stable Rust; no change this pass. The transitive duplicate-version pairs a full `cargo tree -d`
surfaces (`base64` 0.21/0.22, `rand` 0.8/0.9/0.10, `getrandom` 0.2/0.3/0.4, `hashbrown` 0.15/0.17)
are pulled by *different* upstreams (crypto stack vs `reqwest`/`hyper`) and are not a lever this
workspace owns — they resolve only as those upstreams converge. **Net ruling (dated 2026-07-14):
removable-today ≈ 0 — no trim was available without dropping a used capability; every named lever is
either already executed (feature pins), a monitored exit (`async-trait`), an opportunistic
defer-with-reasoning (`tracing-subscriber` formatter), or upstream-owned (duplicate versions).**

- **Error-is-fatal / far-from-core** — `argon2`, `chacha20poly1305`, `p256`, `rand`, `zeroize`
  (crypto); `time` (TZ/calendar); `keyring` (OS keychain); `rustix` (safe syscalls under
  `unsafe_code = forbid`). **Reason**: getting these wrong is fatal, and equivalent in-house depth
  is unrealistic. **Assessment**: RustCrypto / rust-lang-adjacent, widely used, actively
  maintained, permissive licenses. **Monitoring**: RUSTSEC/Dependabot advisories. **Exit**:
  none intended — self-implementing here would *violate* the policy, not honour it.
- **Interop / protocol conformance** — `reqwest` (HTTP/TLS), `url` (WHATWG), `rusqlite` /
  `postgres` / `mysql` (the target databases themselves), `csv` (RFC 4180), `uuid` (Postgres
  `UUID`-column decode in `sql_backends.rs` — `try_get::<Option<uuid::Uuid>>`, a wire-format the
  `postgres` OID set already pulls). **Reason**: qfs's job is to speak these protocols/wire-formats
  accurately; self-implementation only adds compatibility risk. **Assessment**: reference-grade,
  high-reputation, actively maintained. **Monitoring**: advisories + upstream protocol changes.
  **Exit**: each is reachable behind its `driver-*` seam; the lever is *feature trimming* (TLS
  backend, `blocking`, unused DB features) — already exercised: `reqwest` no-default + rustls,
  `mysql` `minimal`, `postgres` an explicit OID set (`uuid` rides it).
- **Cost/time efficiency** — `serde`/`serde_json` (the Rust serialization std), `toml`, `clap`
  (confined to `qfs-cmd`), `tracing` (87 flat events vs 9 spans; subscriber only at the binary
  edges), `tokio`/`futures` (confined to `qfs-runtime`), `winnow` (private `qfs-parser` grammar;
  t02 spike). **Reason**: reimplementing these would be markedly poor cost/time; near-std tier.
  **Assessment**: Rust-team-adjacent or reference implementations, all actively maintained.
  **Monitoring**: advisories + release cadence. **Exit**: `clap` and `winnow` are single-crate
  confined (bounded rewrite if ever needed); `tracing-subscriber` is slimmable to a minimal
  formatter given the 87:9 flat:span ratio — an opportunistic, not urgent, reduction.
- **Ergonomics (parked, not reduced)** — `thiserror` (32 crates), `async-trait` (61 `dyn …Driver`
  sites), `base64` (110 sites), `bytes`, `rpassword`. **Reason**: convenience/boilerplate removal.
  **Assessment**: all std-tier reliability. **Exit / status**: realistic removable-today ≈ 0 —
  `async-trait` is a **monitored exit** (drop when native `dyn` async-trait dispatch is ergonomic);
  `bytes` arrives transitively via `hyper`/`reqwest` regardless; `rpassword` is really OS-compat
  (leans error-is-fatal); `thiserror`/`base64` are entrenched and correctness-adjacent. Revisit
  opportunistically, not now.

Retired/replaced: **`anyhow`** — removed from `[workspace.dependencies]` (declared, never opted in
by any member; a dead line). **`serde_yaml` → `serde_yaml_ng`** — dtolnay archived `serde_yaml` at
`0.9.34+deprecated` (2024), a live development-status/sustainability red flag; swapped to the
maintained community fork (API-compatible drop-in, confined to `qfs-codec`, invisible to the query
language). Docs toolchain (VitePress/npm) is a *separate* supply chain from the shipped binary and
carries its own advisories — monitored, out of scope for this binary log.

## 12. Versioning & distribution — *implemented*

The versioned surface is **the grammar + the registries** (paths/functions/codecs + their
declared shapes). Additive registry/grammar growth is MINOR; anything else is MAJOR; **every
shipped PR bumps the patch** and tags `v0.0.x`, keeping `qfs --version` in sync with the GitHub
Release (`install.sh` consumes the four native tarballs; Linux musl + macOS, both arches).

## 13. Self-hosting integrations: a driver is data — *implemented core, tiered breadth*

*(Decided 2026-07-04, ticket 20260704143743. The declaration surface (`CREATE DRIVER`/`TYPE`/
declared `VIEW`/`MAP`), `/sys/drivers` storage, declared-driver loading, host confinement, stored
view-body evaluation, template binding, `OF` shaping/membership, and provisioning participation are
implemented. The section remains tiered: compiled drivers stay until their script twins pass the
same conformance bar, and service-specific breadth such as Gmail multipart/push and GraphQL/
websocket shapes stays parked as named below.)*

**The problem.** Hundreds of thousands of web services should be connectable without an ad-hoc
Rust driver each. The flow: an LLM generates an integration **qfs script** — authoring happens
in the user's agent harness, outside qfs (the one seam where qfs itself calls a model is §15's
`transform` stage); executing the script stores the declarations in the system database;
connecting evaluates them against the live service.
The bar is **self-hosting**: Gmail, GitHub, and Google Drive must be expressible as scripts —
a service like Chatwork must need no Rust at all.

**The essential fact: the machinery already exists in three shipped pieces.** (1) The generic
REST driver already maps universal verbs onto HTTP internally (`SELECT→GET, INSERT→POST,
UPSERT→PUT, REMOVE→DELETE`), injects auth from a secret reference, follows cursor/Link-header
pagination bounded by `max_pages`, and decodes via codecs — its own docs state *"auth, headers,
base URL, and pagination are config, not grammar."* (2) The definition layer already stores
evaluable declarations as system-DB rows (`CREATE ENDPOINT/JOB/VIEW` — the exact
store-then-evaluate flow). (3) Endpoint routes already bind typed path/query/body parameters
into pre-parsed queries. **A driver definition is therefore a lift, not an invention: the
`RestApiConfig` struct becomes definition-layer data.** One genuinely new concept:
**parameterized definition nodes** — a declared path template whose typed `{param}` segments
bind into the declaration's body (the endpoint binding machinery pointed inward).

**The declaration surface** (nouns are contextual idents; zero new frozen keywords):

```sql
-- chatwork.qfs — an LLM-generated integration; installing it is an ordinary
-- preview/commit (the effects are system-DB rows); connecting evaluates it.

CREATE DRIVER chatwork
  AT 'https://api.chatwork.com/v2'
  AUTH HEADER 'x-chatworktoken'                -- scheme only; the value lives in the
                                               -- account layer (§8), never in a script
CREATE TYPE chatwork/message (       -- a qualified NAME (§5.5); stored at /type/chatwork/message
  message_id text PRIMARY KEY,
  body text NOT NULL,
  send_time timestamp
)

CREATE VIEW /chatwork/rooms AS
  /http/chatwork/rooms |> DECODE json

CREATE VIEW /chatwork/rooms/{room}/messages OF chatwork/message AS
  /http/chatwork/rooms/{room}/messages |> DECODE json

CREATE MAP INSERT /chatwork/rooms/{room}/messages AS
  INSERT INTO /http/chatwork/rooms/{room}/messages
    VALUES ({body: row.body})   -- a struct-literal expression over the bound row (`{…}` is the
                                -- struct constructor, `row.body` a dotted path); the driver
                                -- codec encodes the result onto the wire
```

- **One wire primitive.** Every declared driver's body addresses the wire mount (`/http/…`,
  today's `/rest` machinery re-founded), scoped by the driver's `AT` base URL. **Host
  confinement is a hard evaluator rule, not a policy default**: a declared driver's pipelines
  can only address its own declared host(s), so an LLM-generated script is *structurally
  unable* to read one service and exfiltrate to another. The wire mount's write surface is the
  ordinary effect plan (a POST is an `Insert` effect node the wire applier performs at COMMIT).
- **The auth descriptor** is declarative data — `AUTH NONE | BEARER | HEADER '<name>' |
  ACCOUNT '<provider>' | OAUTH2 (authorize '<url>' token '<url>' scopes '<…>')` — the language
  form of the shipped `AuthStrategy` plus OAuth2 riding the existing browser-consent machinery.
  It declares *how* the service authenticates; `qfs app add` / `qfs account add` / the vault hold
  the values (§8). **Scripts are credential-free by construction** — no clause can carry a secret
  value. `AUTH ACCOUNT '<provider>'` (ticket 20260711121534) extends the declared model past a
  per-driver API key to an **account REFERENCE**: the driver reuses an existing account provider's
  stored credential (`AUTH ACCOUNT 'github'`), resolved from the account/vault machinery at wire
  time (running an OAuth refresh where the provider needs one) — so an OAuth-style service is
  expressible as a declaration whose `/sys/drivers` row names only the provider, never a token. The
  wire coordinate is the shared provider account, injected at commit through the same
  `inject_auth` seam as `BEARER`; the connection's `CONNECT … ACCOUNT '<label>'` names which
  account. The API-key forms (`BEARER`/`HEADER`) parse unchanged — an additive registry growth
  (minor). Shipped proof asset: `github_account.qfs` (a read-only `/ghdecl` slice beside the
  compiled `/github`).
- **Reads are parameterized views over the wire** plus codecs; the `{param}` segments are
  typed and bound (the t32 machinery); DESCRIBE reports the parameterized family as one node.
  Pagination is a declared descriptor (`PAGINATE CURSOR (next 'next_cursor' param 'cursor'
  MAX 50)` / `PAGINATE LINK MAX 50` — the shipped `Pagination` enum, spoken) at driver level
  with per-view override; the engine drives the bounded follow loop. Predicates stay honest:
  everything not declared as a pushable query parameter is the local residual — correct first,
  fast by declaration later.
- **Writes and CALLs are declared mappings** — pure Plan rewrites from a universal verb (or a
  `CALL driver.action(...)` signature) on a declared node to a wire effect; the purity
  invariant holds (mappings construct plans; only the wire applier performs I/O at COMMIT).
  Irreversibility is declared per mapping and rides the standard gate.
- **Storage and the two-source registry.** Declarations desugar to `/sys/drivers` rows (the
  `/server` binding precedent); `CONNECT /chatwork TO chatwork` resolves against
  **compiled ∪ declared** drivers — on a name collision the compiled driver wins and the
  declared one is reported, never silently shadowed. Install / update / remove are ordinary
  previewed, policy-gated, audited writes; DESCRIBE stays pure (declarations are read from the
  local DB, never the network). And because a declared driver is `/sys/drivers` rows, it sits
  inside §16's provisioning source of truth: a declared integration is fetched, diffed, and
  converged by the same `qfs plan` / `qfs apply` reconcile loop as every other definition —
  installing, revising, or retiring an integration is an edit to the one config document.
- **Conformance is §5's drift check aimed outward.** A declared `OF` type is the contract for
  a service the binary never compiled: declared type vs delivered rows is the same
  set-difference reconciliation as a table's, honestly surfaced — and it is the acceptance
  test an LLM (and a user) runs after generating a script. §8's path-scoped policies and the
  audit ledger apply to declared drivers identically.
- **The self-hosting ratchet, honestly tiered.** Tier 1 (this design) = the dominant REST
  shape: JSON bodies, CRUD + list, cursor/Link pagination, header/bearer/OAuth2 auth, typed
  nodes, CALL mappings. Chatwork is fully tier 1; GitHub's read/PR surface and most GDrive
  metadata are; Gmail's metadata/labels are, while its batch endpoints, multipart uploads,
  MIME assembly, and push/watch channels are **named parks** (with GraphQL and websockets).
  Compiled drivers remain until their script twin passes the conformance suite; then they may
  be deleted — that ratchet, not rewrites, is the migration path.
- **Tier 2 — a declared view IS its stored query** *(decided 2026-07-05, after the first
  conversion — the Slack twin — surfaced five parity gaps that all shared one root: the tier-1
  evaluator fetched the wire natively and never evaluated the stored body)*. **Reading a
  declared mount means executing the view's stored pipeline through the real planner and
  engine**, with the driver's confined wire transport as the *only* resolvable source. That
  single rule, not per-quirk descriptors, is how service shapes are absorbed:
  - **Envelope unwrapping is `|> EXPAND <field>`** — an ordinary engine operator (already in
    the closed core), not an "envelope" descriptor. Any future shape quirk is handled by the
    pipe ops users already know (`WHERE`, `SELECT`, `EXTEND`, …), inside the body.
  - **The view path is the mount address, the body names the wire.** Because the body is
    evaluated, the declared path is decoupled from the service's endpoint naming (a dotted
    Slack method mounts at any path the author chooses).
  - **The `OF` type is enforced, not just reconciled**: after body evaluation the declared
    type shapes the rows (project to its columns, cast to its types), so the type is the
    delivered contract and conformance passes when the body is right.
  - **`{param}` segments bind by template match** against the requested path and substitute
    into the body's wire source (the parameterized-definition-node machinery, evaluated).
  - **A MAP body's `VALUES (<expr>)` is an expression over the bound row** (`row`, struct
    literals), evaluated per incoming row by the core expression evaluator and encoded by the
    driver codec — the declared row→wire-body mapping, still a pure plan rewrite.
  - **Transport honesty rides along**: the cursor descriptor's `next` accepts a dotted path
    (`response_metadata.next_cursor`), and the declared driver's HTTP client carries a
    redirect policy pinned to its confined hosts, so a 30x cannot leave the boundary the
    `send_one` guard enforces (closing the one known confinement gap).
  - **Confinement is checked at plan time too**: the body's only legal source is the driver's
    own `/http/<name>/…` — enforced structurally at install (load-time) *and* when the body
    plan is built (defense in depth); the body evaluator physically has no resolver for any
    other mount, so a declared view cannot read `/mail` and post it to its own host.
  - The acceptance bar sharpens accordingly: a script twin passes when its reads are
    **row-equivalent** to the compiled driver's on the same fixtures — the honest parity test
    tier 1 could record only as named gaps.
- **Rejected**: an embedded general-purpose plugin language (JS/WASM — offroading; the
  declarative surface *is* the plugin language, and declarations stay verifiable data, so §5's
  verification story covers drivers for free); OpenAPI or any manifest as the runtime format
  (an OpenAPI spec is *input the LLM reads* while writing a script, never what qfs evaluates);
  per-service Rust crates as the growth path (the compiled set shrinks toward primitives:
  wire, codecs, secrets, OAuth).

## 13b. The markdown collection path — *implemented (documents/links tables, full section context); relation-vocabulary typing blueprint*

*(Mission `markdown-trees-are-queryable-as-documents-and-links-tables`.)* A markdown tree is a
first-class qfs path. `CONNECT /markdown/<name> TO markdown AT '<dir>'` declares a root (an ordinary
`path_binding`, no secret), and the **read-only** `markdown` driver (`qfs-driver-markdown` — a pure
string scan with no I/O in the pure crate; the binary's read facet walks the declared tree) resolves
it to two relational tables, **read-through** so a query never sees a stale index — after files
change, the very next query reflects them:

- `/markdown/<name>/documents` — one row per file: `path` (root-relative, the canonical join id),
  `title` (frontmatter `title`, else the first ATX heading), `frontmatter` (the parsed YAML as one
  `json` value).
- `/markdown/<name>/links` — one row per inline link: `source_doc`, `section_path` (the **full
  nested ATX-heading path** at the link's line, top-level first — never collapsed to the nearest
  heading), `target`, `target_doc` (the normalized in-tree root-relative target, `null` for an
  external or root-escaping URL), `line`.

DESCRIBE is **pure** — the driver describes both tables credential-free for any tree name. The
shipped slice recognises frontmatter, ATX headings, and inline `[text](target)` links (setext
headings, reference-style links, and autolinks are documented non-goals of the minimal version).

**The design thesis — *the relation-vocabulary mission, blueprint*.** A heading is a field and the
links beneath it are that field's references — **a section is a field**. `section_path` is the
load-bearing column that makes this typeable, but the shipped slice only *extracts* it; typing it is
the next mission. The vocabulary of relations is a **closed set declared up front**; a link under a
heading outside the declared vocabulary is a typed-nothing edge and is **rejected with a diagnostic,
never silently admitted** — *don't guess; declare and reject*. Reverse edges (backlinks) fall out by
derivation without being declared, and become the derived-reverse-relation segments the address
grammar traverses (§14b). Once typed, the markdown document graph is a first-class relation source
(§6): the viewer, an AI agent querying qfs-query directly, the automation face, and cross-service
JOINs all reach one relation table, and editing a document rides the same DESCRIBE → PREVIEW →
COMMIT gate. The retired InsightBrowser indexer (heading = field, a declared relation vocabulary, a
frontmatter index) is subsumed here; its name is retired.

**Open — builtin driver, or one alias-registered set.** The collection ships as a **builtin
driver** with its collection condition and `documents`/`links` interpretation compiled in. The
alternative is a general **alias-registration** semantics — register any set over other paths
(`/local`, `/s3`, `/drive`, a `union`) as a named resource — of which `/markdown` would be one
instance; the same shape would then fall out for other file kinds (a photo's EXIF, CSV, PDF). No new
syntax is needed: the DDL already desugars a `CREATE <noun>` to an `INSERT` into a registry path
(§3), so a set definition is a pipeline, not a grammar addition. If alias registration lands,
`/markdown` is demoted from builtin to one example. The running mission assumes the builtin (and
`/markdown/<name>/{documents,links}` already reaches the engine); this question reopens the
assumption without blocking it. **Also open**: what the closed relation set contains
(`parent`/`concerns`/`references`/`supersedes` …) — it determines the UI that emerges — and how far
the *interpretation* is expressible in qfs-query itself: whether `decode markdown` yields the two
`documents`/`links` relations or one flat relation (the cross-document `links` edge appearing as a
stage output is the crux), whether the codec's `bytes↔rows` contract runs per row of a collected
set, and where the grammar (`decode md`, `expand`, `transform`) meets the registered codecs versus
where interpretation stays the driver's work.

## 14. The console face: one screen, loaded not embedded — *blueprint*

*(Decided 2026-07-04, ticket 20260704144737, revised same day by owner direction: qfs **has a
screen** — for local personal use and hosted use alike. The phpMyAdmin analogy: a monitoring +
administration console over qfs's own configuration and state, and Redash-shaped analytics in
the same screen. The boundary that survives from the first draft is not "no screen in qfs" but
**the screen is a client**.)*

**What the screen is.** One first-party face — the **qfs console** — with three surfaces over
one engine: **monitor** (connections, mounts, jobs, audit tail, materialized-view freshness),
**administer** (approval cards, policies, accounts — absorbing the current embedded dashboard,
which retires at parity), and **analyze** (Redash-shaped: saved queries, charts, dashboards).
It works against any host (§8): local `qfs serve` and a managed host are the same screen,
because **the console is a client of a host and local is just another host**.

**The layering rule (survives, checkable).** The grammar never grows presentation nouns
(`CREATE CHART`/`CREATE DASHBOARD` rejected — visualization is data about presentation, not
query semantics). The console speaks **public surfaces only** (the HTTP/MCP faces, the same
DESCRIBE → PREVIEW → COMMIT loop with the approval flow) — no privileged private API, so any
third party could build a rival console. Most of its backend is already shipped language
surface:

| console feature | qfs | status |
| -------------- | --- | ------ |
| saved query | `CREATE VIEW` (a named query at a path) | implemented |
| scheduled refresh | `CREATE MATERIALIZED VIEW` + `CREATE JOB` | implemented |
| parameterized query | endpoint typed param binding (t32); §13's `{param}` | implemented |
| share / publish as API | `CREATE ENDPOINT` + §8 policies | implemented |
| dashboards / viz configs | documents stored through qfs (durable-through-qfs rule) | convention |
| monitor state | `/sys/*` + `/server/*` read as ordinary paths | implemented |

**Durable-through-qfs storage.** Anything the user would miss — dashboard definitions, viz
configs, saved explorations — persists **through qfs** as documents at paths (the codec move
that makes `.workaholic/**/*.md` queryable). Ephemeral state (render caches, sessions) is
exempt: it must be regenerable, never load-bearing. The console keeps no database of its own —
if qfs were not good enough to back its own console, that is a qfs bug to fix.

**Loaded, not embedded — the delivery model.** The console is a **plgg plug-based SPA**
(Elm-architecture, the sibling stack) and is **not compiled into the binary**. The naive
alternative — the browser loading a bundle straight from a CDN — is rejected: this screen
operates a credential-holding control plane with operator authority, and a compromised delivery
edge would own every connected service. Instead, **fetch → verify → cache → self-serve**:

1. Each server release **pins its paired UI bundle**: a source URL + an integrity hash (bytes
   in the binary: a coordinate, not the UI).
2. On boot / first access, the server fetches the bundle, **verifies the hash**, and caches it
   locally.
3. The browser is served **only by the local qfs server** from that verified cache —
   same-origin CSP, no third-party origin at runtime, offline after first fetch, a tampered
   bundle refused at verification.
4. The source is overridable (`QFS_UI_URL`-style) for development against a live plgg dev
   server and for self-hosted mirrors — the default points at the official deploy.

*(Delivery machinery implemented 2026-07-05, ticket 20260704152640, in `crate::console`: the
`PairingCoordinate` pin, `resolve_delivery` (override wins over pin), `deliver` (fetch via an
injectable `BundleFetcher` seam → sha256 verify → atomic temp-then-rename cache), `serve` (cached
bytes + same-origin `CONSOLE_CSP`, or nothing), all hermetically tested with a mock fetcher — no
network. The pin is **unset today**: the plgg console has not published a paired bundle, so
`resolve_delivery` yields `None` and no console is served (honest, non-fatal). Stamping the real
URL + hash into `PINNED_BUNDLE` at release time, wiring the real HTTP `BundleFetcher`, and adding
the live `/console` route are the remaining steps — they land when the plgg bundle publishes.)*

**Version pairing is pinned by the server**, so server/client skew is structurally absent (the
server serves exactly the bundle it was released with). An independent-UI-release channel
(signature-verified, compatible-range) is a **named park**, as are third-party plugs loading
into the console (a second supply-chain surface; first-party bundle only until designed).

**The infra contracts qfs owes the console** *(the console is their first consumer)*:
1. **A stable, schema-carrying result envelope** *(implemented)* — rows + the typed schema (§5
   types when declared) + honest execution metadata (affected counts, truncation/limit flags).
   Absorbs the "JSON output shapes undocumented" gap; it is also the server↔console pairing
   contract, and the §5 plgg bridge lands here (a qfs schema rendering a plgg `cast` validator).
   Whether it joins the §12 versioned surface is decided once the first console consumes it.
2. **Freshness as data** *(implemented — readable surface)* — a materialized view's `last_run`
   is a nullable `Timestamp` column on the `/server/views` listing, read locally (DESCRIBE stays
   pure), `null` until refreshed (the "updated 5 minutes ago" primitive). The runtime *recording*
   of a view refresh's `last_run` rides the parked daemon materialize path (jobs already record it).
3. **Bounded result paging on endpoints** *(implemented)* — `?limit`/`?offset` query knobs sharing
   the envelope's `meta.{limit,offset,truncated}` vocabulary; a post-slice over the evaluated
   result, so it composes with a pushed-down `LIMIT` without double-truncation.

*Envelope + paging shape settled (2026-07-04, owner-approved; tickets 20260703150300 /
20260704152639):* `{"schema":[{"name","type"}…], "rows":[{col: value}…], "meta":{"row_count",
"truncated","limit","offset","affected"}}`. Rows stay **objects** (agent-native; the plgg `cast`
decodes them; one shape, no negotiated variants). `schema` is always present in column order —
§5 type tokens when known, `"unknown"` honestly otherwise. `meta` is honest execution fact
(`affected` non-null only when effects ran). Encodings are schema-discoverable: `timestamp` =
epoch-ms UTC (kept), `bytes` = **base64** (hard break from the byte-array rendering). One
serializer serves all three faces (`--json`, HTTP endpoint, MCP result payload); the
`{"error":…}` / `{"preview":…}` shapes are unchanged. Endpoint paging (contract 3) is
**`limit`/`offset`**, sharing exactly `meta`'s truncation vocabulary — no second pagination
dialect (cursor rejected: qfs sources cannot generally guarantee the stable sort key an honest
cursor requires).

**Rejected**: presentation nouns in the grammar; embedding the SPA in the binary; the browser
loading UI from a third-party origin at runtime; a side database for the console; a privileged
private API between the console and the engine.

## 14b. The address strip: qfs-viewer's second face — *implemented core; the column model is under reconsideration (2026-07-18)*

§14 is the **console** (monitor / administer / analyze). qfs-viewer is a **second first-party
face**, now a package in this repo (`packages/qfs-viewer/`): a knowledge browser that renders a
qfs address as a horizontal **strip of columns**. Both faces are clients of a host over the
public surfaces; neither is embedded in the binary.

**The address, and prefix closure — *implemented*.** The face rests on §2 (*a path is a query
that resolves to a set*). `GET /resolve/<address>` materialises an address as a container whose
children are the resolution of each prefix: **column `i` is the resolution of the address's
prefix `i`**, and any prefix of a valid address is itself valid (prefix closure). A trail — an
address spelled with row-selection and relation segments beyond bare containment — lowers
deterministically to a qfs-query prefix (the one-lowering rule). The row-selection segment `@`
shipped in v0.0.80: `/x/@A` lowers to `|> where <declared key> == A`, describe declares each
node's key columns, and effect positions refuse an unlowered selection (a red test proved
`REMOVE /db/users/@1` previously targeted the containment path `/db/users/1` silently).

**The reconsideration — *owner direction 2026-07-18, not yet settled*.** The first framing was
"one containment path segment = one column." The owner reconsiders: the column axis is closer to
the semantics as **the stages of a complete query pipeline, `where` included** — a column is a
pipeline stage, not merely a path segment. Concrete anchor: in the plggmatic reference exhibit,
the Clients / Projects **search-condition form is a `where` stage** rendered as a column. The full
design space this opened — linear-vs-structure, the intension/extension weld, actors vs viewports,
and the split primitive — is mapped in §14c *(open; nothing settled)*.

This is a generalisation of the shipped seam, not a rebuild — `@` already lowers to `where`, so
the lowering target is unchanged. What widens is the segment's expressive range: from "select one
row by its declared key" to "**any predicate**", with `@`-selection as the special case. It is
consistent with the address boundary (data-determining stages belong on the canonical address;
presentation state — sort display, column fold, highlight — does not): a search `where` is
data-determining, so it belongs on the trail. The earlier lean "filters escape to raw
qfs-query" was a **spelling cost, not a principle**.

**The open question is the spelling.** Composite-key selection `@2024,INV-003` (positional in
declared key order, percent-encoded) is settled. Spelling a rich predicate
(`where amount > 100 and status = 'open'`) as a single path segment — the notation that makes
"a search form is a column" real — is not. When settled, this section, the row-selection grammar,
and qfs-viewer's column construction are revised together.

**The closure principle — *`@`-selection and per-node keys shipped (v0.0.80); the enumerate root
plumbing is a follow-up*.** Everything a read shows has an address: **a row is a node.** Every
address answers three observations — **describe** (what is here: archetype, schema, keys, relations,
verbs), **enumerate** (what is directly beneath: the child addresses), and **read** (the contents).
The shell's `ls`, a viewer column, and a REST listing are one **enumerate**; a table's enumerate is
the projection of its rows onto `(address, label)`, sharing the read's source, order, and limit —
`cd /mail/INBOX` then `ls` lists message addresses, `cd @<id>` enters one. The space is closed under
the language's own operations: **a statement's result rows are again statement sources.**

**Keys are declared, not guessed.** A row's address derives from the row's identity, and the
identity columns (the key) are declared **by the driver in describe** — the viewer and the shell
never infer them: `relational_table` → the primary key, `append_log` → `id`, `blob_namespace` → the
entry name (the containing segment itself). A table that declares no key is **childless** — a valid
answer; not every table is a tree. A non-key identity column (`thread_id`, a sender) is reached not
by selection but by a **relation segment**.

**Relation segments and trails — *blueprint*.** A **trail** is an address spelled with segments
beyond bare containment; a trail is not a concept beside the path — **it is one path within the path
concept**, a wider grammar over one system. A containment-only spelling is the **canonical
address** — the anchor of identity, storage, and permission — while a trail also carries
**declared-relation** segments (`/…/@A/client`) and **derived reverse-edge** segments (a backlink,
`~projects`). Relations are the first-class DESCRIBE metadata of §6: a `/sql` foreign key and a
markdown collection's declared heading (§13b) are two instances of one **relation vocabulary**, so
the viewer draws each as a clickable edge and lowers a click to a JOIN. Link-typed relations
traverse identically: a markdown document web walks `…/references/@overview.md/~references` (forward
link, then backlink) exactly as a foreign key walks `…/@A/client/~projects`; a mutual reference or a
cycle is a valid trail. **Open**: the deterministic naming rule for derived reverse edges
(`~projects`); whether a type-agnostic edge (`linked`/`~linked`) is admitted at all (only as a union
view over typed edges, never a rehabilitation of untyped links); and the `/resolve` name itself.

**trail and walk, defined — *domain terms (2026-07-21)*.** The two words name a noun and a verb
over the same substance, and the viewer's design (§14c) rests on the pair.

- A **trail** is a NOUN — STATIC, a RESULT: **one written path within the path concept**. It is the
  canonical containment backbone (the containment-only *address*) plus the segments beyond bare
  containment — selection (`@A`), declared-relation (`/client`), derived reverse-edge (`~projects`).
  A trail is *where you have walked, recorded* — the trace, held still. The containment chain is a
  strict inclusion: **address/path (the canonical backbone) ⊆ trail (backbone + relation segments)**;
  a containment-only spelling is the degenerate trail.
- A **walk** is a VERB — DYNAMIC, an ACT: **extending a trail one step — one column — at a time.**
  **The walk produces the trail** — the trail is the walk's trace. A walk is **always linear**: it
  traverses exactly ONE trail, never a graph; the non-linearity of a DAG never appears in a walk
  (where a graph's structure is needed it lives *inside* a column, not across the walk — §14c). A
  walk = the act that builds and traverses a trail.
- **The sharpened, operational definition covers reads and writes in one sentence:** a walk is
  *"choose one of the steps the current trail admits, and extend."* For a **read**, the admitted
  steps are `describe`'s declared relations and keys (the next column drills rightward through a
  declared edge). For a **write**, the admitted step is the next input type, dependent on the values
  bound so far (filling a struct field-by-field is a walk whose effect fires only at the terminal
  column — §14c). One definition, both directions: extend the trail one column at a time.

**Resolve runs as the caller's principal.** Because many trails reach one resource,
`/resolve/<trail>` evaluates under the **caller's principal**, and RBAC binds the underlying paths
and relations, never the trail spelling — no chain of relation segments reads what the canonical
address cannot (§8). **A trail is not a policy loophole.**

**Presets.** One viewer changes face by **preset** — *Insight* over the markdown collection path
(§13b), *Form* over `/sql`, *Admin* over the management paths (the same paths §14's console
administers, resolved as ordinary qfs paths rather than a separate app). A preset is a
column-rendering choice over one engine, not a second product. The write-approval UX is not
invented: qfs's PREVIEW renders as the scene and COMMIT is the approval — rows highlighted in order,
approval requested last is the drawing of the data-plane safety model.

**The viewer's first column — *open*.** The strip's first column cannot be derived from the request
principal today: roles and sessions exist, but no seam carries the request's actor down the query
path (`ReadDriver::scan` takes no principal), so no caller passes an actor to a policy's who-axis.
Until that seam lands (an independent qfs mission — §8's named seam), the first column derives from
what qfs declares now: `/sys/paths` (`{path, driver, account}` — the connected query paths) and
`/sys/connections` (`{driver, connection}` — the admin view), two axes kept distinct. The *Admin*
preset column waits on the same seam — qfs deliberately has not ruled which role grants admin, so
the viewer must not invent that distinction.

*(This design corpus is authored **here** now: qfs's design — the path model, the address/trail,
access control, the qfs-viewer UI integration — lives in this blueprint, not in the qmu.app plan
book, per the owner's 2026-07-18 direction that qfs-related material bases itself in the qfs
repository. The remaining qfs-design sections — the closure/key/relation model and trails above, the
markdown collection path (§13b), and the subject model (§8) — are now migrated here; the plan book
keeps pointers plus the qmu.app-level product vision: the managed service, on-demand UI generation,
and plggmatic's rendering engine.)*

## 14c. The viewer, reconsidered — the design space *(rulings settled 2026-07-21; open items named below)*

§14b describes the *shipped* address strip, flags its column model as under reconsideration, and
**defines the domain terms *trail* and *walk*** — a trail is one recorded path (the canonical
address plus its relation segments); a walk is the act of extending a trail one column at a time.
This section reconsiders how the viewer renders a **walk over trails**, and uses those §14b terms
rather than paraphrasing them. The long design conversation of 2026-07-21 converged: the tensions
this map first posed as open are now **rulings**, recorded below, and only the genuinely-open items
remain on the consolidated list at the end, each naming the downstream mission that owns it.

**The rulings (settled 2026-07-21).** The long reconsideration converged. Four rulings fix the
viewer's shape; the tensions this map first posed as a choice are now dissolved, not chosen.

1. **The column-oriented layout is a display pattern for post-execution semantics — kept simple.**
   The strip does exactly one thing: *display the semantics after a query is exercised, in columns.*
   It is **not** an isomorphic re-encoding of qfs's query structure, and it must **not** be
   over-abstracted into a higher-abstracted container or a design-pattern abstraction — that framing
   was explicitly retracted by the owner. Column `i` remains the resolution of the trail's prefix `i`
   (§14b); the columns show the walk's trace, no more.

2. **Linear-vs-graph is dissolved by placement, not by choosing one.** The earlier tension — a linear
   strip cannot show a query's DAG (joins and unions fan *in*; one source can fan *out*) — does not
   force a choice between a strip and a graph view. The strip stays **linear across columns** (a walk
   is always linear — §14b); a DAG, the non-linear define-time structure (e.g. a React-Flow-like
   pipeline editor), lives **inside a single column**, not across columns. Non-linearity is confined
   to a column's interior while the column sequence stays a one-way linear walk. One row can hold a
   stored-procedure menu (enumerate) → a DAG-editor column (define; non-linear; wide) → a preview
   column → a result column (extension): getting into the query semantics and out to the exercised
   result, in the same single row.

3. **Definition (intension) and application (extension) are welded by `path = query = set`.** Unlike
   a filesystem path (a static address a *separate* query takes as an argument), a qfs path is a
   set-valued *expression* whose segments are operators: containment is select-from, `@A` lowers to
   `where <key> == A`, a relation segment is a join, `|>` stages are explicit operators. A path is
   therefore simultaneously a **query** (intension — `describe` gives schema, keys, relations) and
   **resolves to a set** (extension — `read` gives rows), and every prefix carries **both aspects of
   ONE object**, not two artifacts. A graph view foregrounds intension and a strip foregrounds
   extension, but both render one object at two aspects — the weld that lets the viewer be one
   substance rather than two paradigms. *Caveat (retained as open):* the unity is cleanest for reads;
   writes/effects reintroduce a distinct preview/commit aspect (§7), so the seam is not seamless at
   the write edge — where the intension/extension unity ends is on the open list below.

4. **100% parity between what the query language expresses and what the viewer configures is
   deliberately given up.** The viewer does not aim to configure everything the language can express.
   It is a **faithful representation of the subset it covers**, not a lossy projection of the whole;
   fidelity is the **content's** responsibility (e.g. the DAG inside a column — ruling 2), not the
   container's.

**Still open — the clean split primitive.** Merge exists everywhere as "a stage that takes another
stream as an argument" (join / union / zip). Fan-out — one flow feeding several downstream — has *no*
well-designed pipe syntax; `tee` and variable-binding are the crude substitutes. In a path/set
language both may reduce to **a named node plus references**: a merge references several named nodes
as inputs; a split is several nodes referencing one named node, so wires are the *rendering* of
references, not a new language primitive — consistent with "everything is a path." The clean
**split** primitive and its in-column DAG editor stay unsettled (see the open list below).

**Actors and viewports are two separate axes** (they were being mixed):
- *Who drives the surface.* A human via touch/mouse, or a browser-side realtime-API **AI agent that
  tool-calls** (WebMCP) to build the pipeline while the viewer co-renders pipeline *and* result. The
  AI is a co-operator of the same surface, not a separate modality; this implies **one operation
  vocabulary** exercised by both — the tools the agent calls are the primitives the columns expose —
  and it enables live human/AI handoff over one pipeline object (watch it build, take it over, hand
  it back). An open sub-question: is the human/AI relation *co-edit* of one live object, or
  *produce-then-review* (the agent yields a pipeline the human inspects before commit)? That choice
  sets how live and how shared the viewer state must be.
- *Which viewport renders.* A 420×640 phone vs a desktop — different *layouts* of the same graph.
  Voice is inherently sequential (an utterance the agent turns into tool calls); a phone favors
  focused, card-at-a-time navigation; only the desktop can show wires/lanes at once. Hence **one
  canonical graph, several projections**, echoing qfs's existing "one resource, many faces."

**The AI-letter — rulings 5–8 (settled 2026-07-21).** The AI-letter concept rides the **same column
UI** as the rest of the viewer; it introduces no new surface and no new mechanism. Its qfs mapping is
ruled:

5. **A letter is an envelope carrying context and its own interactivity.** The envelope encloses both
   the bounded context data and the interactivity that operates on it, and it rides the same column
   strip as any other trail — a letter is a walk over enclosed context, not a separate app.

6. **Inward confinement is the same principle as declared-driver host-confinement.** A letter's reach
   is confined *inward*: the recipient can reference and manipulate **only** the enclosed context,
   never reach back into the sender's live world. This is named explicitly as the **same confinement
   principle a declared driver applies to its host** (§ the declared-driver model) — applied here to
   the letter's data scope, not a new sandboxing mechanism.

7. **The only way out is a single typed egress.** A reply is a **typed `INSERT` into the sender's
   inbox** — one egress, typed, with no side channel. The letter's kind fixes the target reply type;
   nothing else leaves the envelope.

8. **Interactivity is derived from the type; form-filling is a walk.** The interactivity is derived
   from the type, not authored as a second attribute: an enum type → choice buttons, a struct type →
   a form, free text → conversation. The input **modality** is free (tap / form / voice / free
   conversation) but must land on the **fixed target type** — free input is distilled to the typed
   target and confirmed before egress ("enter freely, confirm typed, exit"). **Filling a form is a
   walk**: a struct input is a trail of per-field input columns, a partially-filled struct is a valid
   intermediate value (the prefix-closure analogue), and the **effect fires only at the terminal
   column** — no I/O until COMMIT. A **condition-split** — the next step depends on the value bound so
   far (reject ⇒ a reason column grows; approve ⇒ it does not) — branches the *path*, not the
   data-flow; it is a **declared, checkable** rule (not existential search), so it keeps every walk
   linear (this is the ruling that sharpens walk's operational definition — §14b). And **"who drives
   is not the design axis":** a human ultimately instructs either way, so the **same surface serves
   human and agent**, and the UI must stand on its own without AI.

**Developer acceptance.** Developers may not love linear pipes intrinsically; linear dominates
because good split+merge semantics are missing and text forces a linearization (CTEs and variables
are the workaround). The likely real acceptance driver is **round-trip fidelity to the query text** —
a developer trusts a visual pipe surface when it is a lossless projection of the query they could
have typed, droppable back to text at any moment. That same property serves the AI actor (which
writes text) and the human (who reads/edits columns) with a *single* artifact, which is itself an
argument for the one-substance reading.

**Multi-channel rendering candidates** (all open, none chosen): lanes/tracks (a timeline of
horizontal channels that combine at explicit merge columns); named-channel references (one strip
visible, other channels collapsed to expandable source chips — the "tree of named linear pipes");
nesting/fractal (a column that contains a sub-strip); parallel-lens (channels as simultaneous
renderings — rows / aggregate / chart / diff-over-`@ref` — of one prefix); focus+context. Each keeps
the single-channel case identical to today's strip and reveals structure only when a genuine second
input exists. The governing rule a ruling must set: **when is a second input an in-strip relation
segment (a declared relation) vs a new channel (an independent source)?** That boundary is the
simplicity governor.

**The delivery seam.** Whatever the viewer becomes must be renderable by the column-oriented UI
engine (plggmatic), which depends only on the `(declaration, rows)` protocol and is supplier-blind;
its parts (the strip container, the typed-table renderer, the preview/commit dialog, the connection
manager) are reusable components. So the qfs-side deliverable is fixed regardless of which UI reading
wins: **every path/trail must answer `describe` (schema, keys, relations), `enumerate` (child
addresses), `read` (rows), and `preview`/`commit`**, all through the one typed envelope (§14
contract 1). Driver configuration is anticipated to be authored through the *same* surface —
building `CREATE DRIVER`/`VIEW`/`MAP` as columns, previewed and committed into `/sys/drivers` — which
additionally requires qfs to **describe the addable-provider / declared-driver surface** (an
enumerate contract that does not yet exist).

**What this makes of the current missions.** The two active foundation missions are not features
but the qfs-side substrate the viewer consumes. The file-collection-as-a-declared-set mission
produces `(declaration, rows)` over collected file sets (the strip's content for local knowledge);
the declared-driver-DSL mission defines the authorable, describable vocabulary the visual surface
manipulates. The reconsideration surfaces further foundation items **not yet missioned**: the
enumerate-root plumbing (§14b), the request-principal seam that would let the *first column* derive
from the caller (the "empty home" root — a fresh, initially-empty personal namespace that fills as
one connects/declares, rather than the union-of-all-drivers root), a first-class **split** primitive,
and a **one-language** spelling in which a column action *is* a qfs statement, so authoring in the
viewer is authoring qfs itself.

**Consolidated open list — each item names the downstream mission that owns it.** The rulings above
settle the viewer's shape; what remains genuinely open is listed here, and **each open item names a
downstream mission that is named but deliberately NOT created by this recording mission** (creating
one would violate this mission's non-goals):

- **The ASK-grammar / predicate- and merge-column spellings** — how a rich `where` or a `join` is
  written as one address segment, and how a human-supplied value is inserted through a type-derived
  UI. These are **candidate spellings, explicitly unsettled**. → *the ASK-type-INTO-path grammar
  mission* (the predicate/merge-column spelling rides the same mission).
- **The `split` primitive + the in-column DAG editor** — the clean fan-out primitive (a named node
  plus references, wires as its rendering) and the non-linear define surface it lives inside (ruling
  2). → *the split-primitive-and-in-column-DAG-editor mission*.
- **The request-principal seam / "empty home" root** — the seam that lets the *first column* derive
  from the caller: a fresh, initially-empty personal namespace that fills as one connects/declares,
  rather than the union-of-all-drivers root. → *the request-principal-seam / empty-home-root mission*.
- **The enumerate-root plumbing** (§14b's own named follow-up) — the qfs-core seam that lets a walk
  drill rightward from the root. → *the enumerate-root-plumbing mission*.
- **The per-viewport projections** — one canonical graph rendered several ways (voice / phone /
  desktop), plus the still-open co-edit-vs-produce-then-review shape of the human/AI relation. → *the
  qfs-viewer minimal-walk implementation mission (scope "(い)")*.
- **The intension/extension write edge** — the caveat retained from ruling 3: the path = query = set
  unity is cleanest for reads, and writes/effects reintroduce a distinct preview/commit aspect (§7),
  so where the unity ends is not yet ruled. → *the qfs-viewer minimal-walk implementation mission
  (scope "(い)")*, where the write-edge preview/commit surface is realized.

Two further boundary questions stay open under the same downstream owners: the **in-strip-relation
vs new-channel boundary** (when a second input is a declared relation segment vs an independent
channel — the simplicity governor) and the **addable-provider `enumerate` contract** (describing the
declared-driver surface so driver configuration is authored through the same columns). None of the
named downstream missions is created here; this recording mission only maps them.

## 15. `transform` — the model-calling pipe stage — *implemented (grammar, execution, whole-tree routing, consent gate, three live providers); live-provider run owner-attended*

*(Decision W — decided 2026-07-08, ticket 20260708002100; **shipped 2026-07-09** (transform epic
T1–T4): the grammar (`transform <name>` — a bare ident), the `ModelProvider`/`TransformExecutor`
seams, whole-tree routing, the model-free PREVIEW, and the irreversible consent gate are in the
binary — a fail-closed `UnconfiguredProvider` stands in until a real provider authenticates, so
the single live model call is owner-gated, not autonomous. This section **reversed decision K**:
the absolute "qfs NEVER hosts or calls an LLM" — once carried as doc-comments in
`qfs-driver-claude`, `qfs-mcp`, and the binary's claude leaf — is superseded by the bounded thesis
below, and those comments now cite this section.)*

**The reversal, precisely bounded.** Decision K said the model always runs elsewhere — qfs never
hosts or calls one. Everything that absolute was actually protecting is kept in full; only the
blanket "never calls" falls:

- qfs still never **hosts** a model — no inference runtime, no weights, no embedded engine, ever.
- `/claude` is still a **pure path façade** over session metadata plus an append-log for steering
  a running agent; it has no inference dependency and calls no model API. That description stays
  accurate — it is just no longer the statement of an absolute.
- The pure/wasm engine still performs no I/O; DESCRIBE and PREVIEW still touch nothing.

The new thesis: **qfs MAY make an authenticated outbound model call, but only through the
`transform` seam** — a stage declared as data, activated by authentication, planned as an impure
effect performed by an injected async applier, and gated by PREVIEW/COMMIT like every other
effect. One seam, not a leaked capability: no other statement, driver, or code path calls a model.
This is **enforced**, not merely stated (ticket 20260709104300): a governance test proves a
model-call effect node originates only from `PipeOp::Transform` (a spread of non-transform
statements — reads, writes, `CALL`, codecs, DDL — carry none), and the `ModelProvider::call` seam
is **sealed to invoke** — every invocation must pass a crate-private `CallProof` witness minted only
by the `call_model` funnel, so a driver merely holding a `&dyn ModelProvider` cannot call a model
(a `compile_fail` doctest locks it). The trait stays open to *implement* (the live provider is a
binary-leaf concern), which is the exact property: open to implement, sealed to invoke.

**Why reverse at all.** The language moves rows between services but cannot map *meaning*:
classify a message, summarize a relation, extract structured rows from a blob. The options were
(a) keep the absolute and leave semantic mapping to the outer agent — every row round-trips
through the agent's context window, the N-SDK problem reborn as an N-roundtrip problem, and the
result never lands inside one previewable plan; (b) host a local model — rejected outright
(binary weight, wasm death, and the hosting half of decision K stays correct); (c) one declared,
authenticated, gated stage inside the pipeline. (c) is taken: the model call becomes an ordinary
effect the existing machinery already knows how to declare, preview, gate, apply, and audit.

**The grammar seam — a contextual identifier, not a keyword.** `transform` parses in pipe-stage
position as a **contextual ident** (the `CONNECTION`/`TABLE`/`DRIVER` lesson): the frozen keyword
set stays at **39** and the count-lock test does not move. The pipeline IR gains one stage (the
governance-locked `PipeOp` set grows by one variant, additive like the registry seams
`Decode`/`Encode`/`Call`); the definition noun `TRANSFORM` and its clause words (`INPUT OUTPUT
PROMPT PROVIDER MODEL EFFORT MAX`) are contextual idents like `AT`/`SECRET`. SemVer verdict:
**MINOR** (§12 — additive grammar/registry growth). The name is `transform`, **not `convert`**:
`convert` is the codec-chaining vocabulary (`|> DECODE json |> ENCODE yaml` — pure,
deterministic, bytes↔rows); `transform` names the model-mediated, schema-declared map. The two
must not share a word, because one is reversible and free and the other is neither.

**A transform is data (the §13 lifecycle: declare → store → activate).** A definition is
authored as a statement, stored as system-DB rows under `/transform/…` (the `/sys` registry
precedent — install/update/remove are ordinary previewed, policy-gated, audited writes;
`ls /transform` lists definitions; DESCRIBE reads locally, stays pure):

```sql
CREATE TRANSFORM triage              -- a NAME (§5.5); stored at /transform/triage
  INPUT  (id int NOT NULL, subject text NOT NULL, body text NOT NULL)
  OUTPUT (id int NOT NULL, priority text NOT NULL, reason text NOT NULL)
  PROMPT 'Assign a support priority (p1/p2/p3) to each message.'
  PROVIDER anthropic MODEL 'claude-sonnet-4-5' EFFORT low
  SECRET 'env:ANTHROPIC_API_KEY'
```

- **`INPUT`/`OUTPUT` reuse the §5 type literal** — the one `( <col> <type> … ) [WHERE <pred>]`
  production over the existing `ColumnType` vocabulary, or a named type (`INPUT OF message` — a
  name resolved in the type namespace, §5.5; the `/type` catalog stays the inspection surface).
  There is **no parallel schema language**: the declared schemas are ordinary
  `qfs_types::Schema` values, so membership, drift, and DESCRIBE all come for free.
- **`PROVIDER` names an implementation behind the `ModelProvider` seam** — an owned-DTO trait
  (request: instructions + declared output schema + payload; response: rows or a structured
  refusal). **Three live providers are wired** (shipped 2026-07-11) — `anthropic`, `openai`,
  `google` — dispatched by the `PROVIDER` column value; any other value fails closed as
  Unconfigured. A provider is swappable data, never a grammar surface. No vendor SDK crate
  (§11 posture — the confined `reqwest` transport speaks the wire):

  | `PROVIDER` | Endpoint | Auth header | JSON-output control | `MODEL` example |
  |---|---|---|---|---|
  | `anthropic` | `POST /v1/messages` (pinned `anthropic-version: 2023-06-01`) | `x-api-key` | `system` instruction | `claude-sonnet-5` |
  | `openai` | `POST /v1/chat/completions` | `Authorization: Bearer` | `response_format: json_object` | `gpt-5.4` |
  | `google` | `POST …/models/<model>:generateContent` | `x-goog-api-key` | `responseMimeType: application/json` | `gemini-2.5-flash` |

  Every auth header is in the single redaction authority (`qfs-http-core::SENSITIVE_HEADERS`), so
  no request `Debug`/log can carry the key; a missing key fails closed **pre-network**; a 429/5xx
  is retried with a bounded budget honoring `Retry-After`. The live text-generation round (one real
  call per provider with the owner's keys) is owner-attended, the mission's acceptance evidence.
- **`MODEL` and `EFFORT`** are the provider's coordinates (model id; the effort/reasoning knob),
  carried as data and echoed in every preview and audit record.
- **`SECRET` is a reference** (`env:…` / `vault:…`), resolved lazily at apply time by the §8
  vault machinery. Definitions are **credential-free by construction** — no clause can carry a
  secret value.
- An optional **`MAX <n>`** bounds rows/documents per commit (the `PAGINATE … MAX` bounded-loop
  discipline); exceeding it refuses with a structured error naming the bound.

**Activation is authentication — nothing new.** A declared transform is inert until its
provider's account authenticates through the existing `qfs app add` / `qfs account add` / vault
surface (§8): secrets stdin-only, guardian-slot key custody, no new credential path. The applier
resolves the secret reference only at apply time; DESCRIBE and PREVIEW never touch it.

**Semantics — three cardinality modes, derived from the declared input shape.** The stage's
output relation is **always exactly the declared `OUTPUT` schema** — downstream stages
(`ORDER BY`, `WHERE`, a terminal write) plan against it at parse/plan time, exactly as they
would against a table's. The mode is a **total function of the `INPUT` shape**, derived and
stored at CREATE time and reported by DESCRIBE — never inferred at run time:

1. **Schema-directed extraction** — `INPUT` is a single `bytes` column (the blob archetype: what
   a blob node delivers). Each input document produces **N** output rows; a relation of several
   blobs runs per document, outputs concatenated in input order. Blob/text → structured rows:
   `FROM /local/notes/meeting.md |> transform action-items |> WHERE owner == 'ty'`.
2. **Relation-wise** — `INPUT` is a single `array<struct S>` column: the engine packs the whole
   incoming relation (membership-checked against `S`) into that one value; the model returns a
   **new relation** in the `OUTPUT` shape — summaries, restructures; cardinality is free.
3. **Row-wise** — any other row shape. Input columns are matched by name against the incoming
   relation (a missing column is a plan-time type error); **each row maps to exactly one output
   row** (`SELECT`-like, cardinality preserved; a per-row failure is a structured error naming
   the row, never a silently dropped row).

The one shape that could read two ways — a single `text` column — is **row-wise by rule** (a
text *field* per row); extraction is reached by declaring the input as `bytes`, the archetype a
blob source actually delivers. An empty `INPUT` or `OUTPUT` is a declare-time structured error.
In every mode the declared input columns are matched **by name** against the incoming relation
and surplus incoming columns (a blob node's metadata beside its `bytes`) are ignored — one
matching rule, no per-mode variant. Consequence, not commitment: a relation whose own single
column is an `array<struct>` is relation-wise *by declaration*; to map such rows row-wise,
declare that column alongside a key column (any multi-column `INPUT` is row-wise).
In row-wise mode, an output column whose name and type match an input column is **engine-copied,
never model-echoed** (the `id` above): keys survive verbatim for downstream joins, and the model
is asked only for the genuinely new columns — those copied columns keep their upstream
`Provenance`. Every model-produced column carries `Provenance { driver: the definition's catalog
path (/transform/<name>), source_col: None }` — a model-made value never claims a backend origin, and the audit trail
names which definition made it.

**Plan shape — local, non-pushable, schema-transforming, impure.** No driver executes
`transform` natively; the planner always places it in the local segment (pushdown proceeds
normally upstream; everything downstream runs locally over the declared output schema). Unlike
the pass-through codecs, `transform` rewrites the schema at plan time — the type checker uses
the declaration, not inference. And unlike every query stage, it is an **effect**: a statement
containing `transform` is never a pure read — it evaluates to a Plan carrying a model-call
effect node. The pure/wasm engine only *plans* it; the call itself is performed by an **async
applier the binary injects** (the `driver-claude` template: pure declaration crate, runtime
applier, binary leaf). A build with no injected provider — wasm, or a binary without the feature
— fails closed: no applier, no commit. Hermetic tests inject a **mock `ModelProvider`** (the §11
`MockHttp` posture): every semantic, membership, and gating test runs offline.

**Routing — how a read reaches the gate** *(Decision W amended 2026-07-08, design review: the
original text said a transform statement "evaluates to a Plan" without specifying how the read
path gets there; this rules the mechanism)*. Statement classification walks the **whole tree**:
a `transform` stage *anywhere* — top-level, mid-pipe, in a subquery, a `JOIN` source, a set-op
branch, or a `LET` binding/body — classifies the statement as effect-bearing, so it routes
through the standard PREVIEW/COMMIT path instead of the direct read executor (the shipped
terminal-`CALL` reclassification, generalized from "last op" to "any stage"). The model call
itself is **exec-layer orchestration, not interpretation** — the same layer that performs §7's
commit-boundary materialization: at commit it runs the upstream segment through the read engine,
invokes the injected applier with those rows, membership-checks the returned rows against the
declared `OUTPUT` schema, and feeds them to the downstream local segment — or embeds them in a
terminal write's `args`, composing with §7's materialization as one ordered sequence (upstream
read → model call → membership check → write). The plan's model-call effect node is the
**consent and audit artifact**: the irreversible gate reads it and the ledger records its
`{id, affected}` plus token usage — but the row payload flows exec-side, *above* the
interpreter, so §7's payload-free interpreter contract (`EffectOutput = {id, affected}`) is
untouched and there is still no engine→runtime inversion. A **committed** statement whose
terminal statement is a query renders **rows + `meta.affected`** through the §14 envelope — a
committed semantic read returns its rows, never just a commit summary; PREVIEW renders the
effect plan only, never rows.

**Stored server bodies refuse `transform` — structurally, at definition-store time.** A
`CREATE VIEW|ENDPOINT|TRIGGER|JOB|WEBHOOK` body containing a `transform` stage is rejected when
the definition is stored (a structured error naming the stage), because the server's fire paths
have no `--commit-irreversible` channel: an unattended, per-request model spend would bypass the
consent model entirely. A declared-budget consent (a policy-attached spend bound that could lift
this) is a **named park**, not designed here.

Because a transform definition is system-DB rows, it sits inside **§16's provisioning source of
truth**: definitions are fetched, diffed, and converged by the same `qfs plan` / `qfs apply`
reconcile loop as every other definition — whichever implementation ticket ships second owns
that integration.

**Safety — irreversible by nature.** A model call spends tokens and quota and is
non-deterministic: it joins `REMOVE` as **inherently irreversible** (§7), so COMMIT requires the
explicit `--commit-irreversible` acknowledgement — always, in read position too. **PREVIEW calls
no model and fetches no source**: it surfaces the effect-plan only — definition path, provider,
model, effort, derived mode, and the honest count (§7 doctrine: exact for a literal source, an
upper bound when a `LIMIT` or `MAX` bounds the segment, honestly unknown otherwise — never a
fabricated price). At apply, the model's returned rows are **membership-checked against the
declared `OUTPUT` schema** (§5: the structured error names the failing column/predicate); a
refusal or non-conforming response fails the effect node and rides §7's partial-failure
recovery. The audit ledger stays **payload-free**: definition path, model, effort, counts, and
token usage — never prompts, rows, or payloads.

**Versioning & anti-drift.** MINOR (§12). Shipping regenerates `docs/{language,drivers,
server}.md` (`gen-docs --check` holds), regenerates the Agent Skills (`gen-skills --check`), and
bumps all four plugin version fields (a taught-surface change). Hermetic coverage rides the mock
provider; the live-provider end-to-end check is the implementation ticket's gate, not CI's.

**Rejected**: `convert` as the name (codec vocabulary — pure and reversible, which this is not);
a 40th frozen keyword; hosting or embedding a model; a free-form prompt-only stage with no
declared output schema (undeclared output cannot be membership-checked — the "types are sets"
contract is exactly what makes model output verifiable); calling the model during PREVIEW to
price exactly (preview is pure); a per-provider grammar surface (providers are data behind one
seam); pushing `transform` down to any driver; a second model-calling path anywhere else in the
binary; a `transform` stage inside a stored server binding body (no unattended spend — the
definition-store-time rejection above; a declared-budget consent is the named park).

## 16. Provisioning — the reconcile loop: `qfs plan` / `qfs apply` — *implemented*

*(Decision X — decided 2026-07-08, ticket 20260708004700. `qfs dump` and `qfs restore` shipped
first; the reconcile surface now ships too: `qfs plan` computes the desired-vs-current diff without
writing, and `qfs apply` recomputes and commits through the same gates. This section is the
implementation contract for that surface, extending §10 and §13 in place.)*

**The thesis.** qfs already made its whole configuration data: bindings are rows under
`/server/*` (§10), and declared drivers, defined-path connections, and account consents are rows
in the System DB under `/sys/*` (§13, §8 — the Project DB is the vault proper, holding only
secret material), and `DESCRIBE → write → PREVIEW → COMMIT` already *is* a fetch → desired →
plan → apply loop for one statement at a time. This design makes the loop **total**: an agent fetches
the whole current configuration as one editable "as code" document, edits it, and applies it
back as the **authoritative desired state** — the interpreter computes the desired-vs-current
diff and converges. Terraform's shape, with none of Terraform's apparatus: no state file, no
provider plugins, no second language — the config store *is* the state, the definition layer
*is* the language, and the effect plan *is* the diff.

**Reconcile semantics: drift is set difference, aimed at the machine's own config.** The
fetched document is the single source of truth. Per collection, the reconcile is exactly §5's
set algebra:

- **desired ∖ current → add** (`ServerWriteOp::Insert` / a system-DB insert);
- **current ∖ desired → destroy** (`ServerWriteOp::Remove` / a system-DB delete) — a row absent
  from the desired document is removed, full stop;
- **key-matched but canonically unequal → change** (`ServerWriteOp::Update` / a system-DB
  update).

This is the qfs generalization of §5's **redefinition, not migration**: there is no migration
subsystem, no deprecation period, no three-way merge — the desired document redefines the
configuration and the machine converges to it. Destroy is inherently irreversible and rides the
existing §7 gate unchanged. The shipped `restore` (insert-or-skip for drivers and policies,
upsert-overwrite for settings, billing, and path bindings — it never **removes**) remains what
it is — an additive backup import — but the reconcile loop, not `restore`, is the operating
model: authoritative desired state closes the remove gap `restore` deliberately left open and
makes its update behavior an explicit, previewed *change* instead of an incidental overwrite.

**The source-of-truth artifact is a canonical `.qfs` script.** The fetch emits the whole
configuration as a normalized list of definition-layer statements — `CREATE
ENDPOINT|TRIGGER|JOB|VIEW|MATERIALIZED VIEW|POLICY|WEBHOOK`, `CREATE CONNECTION`, `CREATE
DRIVER` (§13), `CREATE TRANSFORM` (§15), plus the relevant `/sys` settings and path bindings — in
a fixed collection order with a fixed per-collection sort, so two fetches of the same state are
byte-identical.
Two shipped facts make the round-trip exact:

- **CREATE ≡ INSERT** (§3): every emitted statement desugars to the same registry-path write
  the current rows came from, so executing the script reproduces the state.
- **`StatementSpec.canonical()`**: body-bearing rows store a span-normalised parsed AST whose
  serialized form is deterministic. Drift equality is decided on **canonical specs, never
  source text** — reformatting a body, re-wrapping lines, or renaming nothing reads as **zero
  drift**. Cosmetic difference is not difference.

The JSONL dump remains the machine/backup form; the `.qfs` script is the authoritative,
agent-editable SoT. It is **commit-safe by construction**: secrets appear only as references
(`env:…` / `vault:…` — the §13 rule that no clause can carry a secret value; the emitter shares
`dump`'s credential boundary). **Secret-shaped settings are excluded, not redacted** *(Decision
X amended 2026-07-08, design review)*: a `sys_settings` row whose key matches the secretish
predicate (the rule `dump`'s redaction already encodes) stays **out of the SoT and out of the
reconcile universe entirely** — never emitted, never diffed, never destroyed by absence; it is
managed only through the direct setting write path. A redacted placeholder in the SoT would
either read as permanent drift or apply the literal placeholder over the live value — which is
exactly the flaw the shipped JSONL round-trip carries today (`dump` redacts to `<redacted>`,
`restore` writes that literal back); the implementation fixes `restore` to skip redacted
secretish values as part of this work. The audit chain (`sys_ddl_events`) and billing rows are
provenance and entitlement, not configuration — readable via `dump --include-events`, **excluded
from the editable SoT — and exclusion is total**: an excluded collection is also outside the
diff, so authoritative destroy can never touch it (billing rows absent from the document plan
nothing).

**The surface: `qfs plan` / `qfs apply`.** A short top-level verb pair (the `qfs auth`
convention), built on the dump/restore machinery and the pure `qfs-plan` substrate:

- `qfs dump` gains the canonical-script format — the fetch that produces the SoT document.
- `qfs plan <file.qfs>` — **pure**: reads the document, reads live current state, computes the
  reconcile diff, renders it via `preview()`. Touches nothing, always safe. Its exit code
  distinguishes "no changes" from "changes pending", so an agent gates on it without parsing
  the rendering.
- `qfs apply <file.qfs>` — recomputes the same diff against live current state and commits it.

These are CLI verbs, not grammar: **no new frozen keyword**, the keyword-count lock does not
move, and the SemVer verdict is **MINOR** (§12 — additive surface).

**Unified fetch across both stores.** One document, two sources unioned: `Runtime::snapshot()`
yields the `ServerState` collections (endpoints, triggers, jobs, views, policies, webhooks);
the system/project-DB read that `dump` already performs yields declared drivers, settings,
sys-policies, and path bindings (connections). The document carries a **generation stamp** —
the migration counts plus the `sys_ddl_events` chain head `(seq, hash)`, exactly what the JSONL
header records today — as a header pragma. The stamp makes a stale base *detectable*: when the
live chain head no longer matches the document's stamp, `plan` flags **base moved**
(old head → new head) in the rendered plan, so an agent sees that it is about to overwrite
configuration that changed since its fetch — and **`qfs apply` refuses on a moved base** unless
the explicit `--allow-stale-base` override is passed *(Decision X amended 2026-07-08, design
review)*: without the refusal, a stale document whose plan happens to contain no destroy would
silently revert concurrent non-destructive changes with no gate at all (the lost-update window).
The cheap correct loop is re-fetch, not override. Correctness still never depends on the stamp:
the diff is always recomputed against live current state at plan time *and again* at apply time
— a plan rendering is advice, never an input to `apply`; the stamp check is a **consent gate**,
independent of the irreversible ack and the policy gate (three controls, any one refuses alone).

**The process boundary — reconcile is a client of the host (§8)** *(Decision X amended
2026-07-08, design review: the original text cited `Runtime::snapshot()` without saying how a
CLI process reaches it — `ServerState` is an in-memory registry inside the running daemon, and
`snapshot()`/`reconcile_all()` exist only in that process)*. The transport is ruled, not
assumed: `qfs dump` (canonical form), `qfs plan`, and `qfs apply` address a **host** (§8 —
local is the implicit host, meaning the local daemon at its loopback bind). The system-DB half
is read and written directly, as `dump`/`restore` do today; the `/server` half is fetched from
and applied **through the running daemon's public statement face** — reading `/server/*` and
submitting the batch's `ServerConfigWrite` nodes as ordinary statements, no privileged config
API (§14's layering rule). That transport is exactly why convergence works: the `/server`
commit executes **inside the daemon process**, so `reconcile_all()` runs where the bindings
live — routes mount and unmount, watchers attach and detach — with no `ServerState` IPC and no
second store. No running daemon ⇒ no live `/server` collections: `plan`/`apply` then cover the
system-DB half and render the `/server` half as **host not serving** — a structured, honest
refusal; an unreachable daemon is never read as an empty current state (which would plan the
whole document as adds). And an applied `/server` reconcile must survive a restart: after every
committed `/server` batch the daemon **re-emits its post-commit `ServerState` as the canonical
document at its boot config path** (atomic temp-then-rename through the durable-store seam) —
the boot file is the at-rest form of the state it replays, closing the pre-existing gap where a
hot reconfigure died with the process, and making "the config store *is* the state" literal.

**The face, named** *(Decision X further amended 2026-07-08, after the increment-4
investigation established what the daemon actually serves)*. "The public statement face" is
not a face to be designed — it is the one that already exists: the daemon's statement bridge
(`POST /api/describe` / `/api/run` / `/api/commit`), which drives the **same single
`McpEngine` statement path** as `POST /mcp` (t47/t51/t52 — one executor, three clients: MCP,
the dashboard, and now the reconcile CLI). No new endpoint is minted and no private RPC exists
to mint (§14's layering rule holds; any third party could build a rival `plan`/`apply`). What
was missing is not an API but **two wiring legs**, and both are completions of §10's
server-is-a-driver thesis, not new surface:

- **The read leg**: the introspective `/server` driver mounts into the **serve composition's**
  engine (a read facet over the live `ServerState` snapshot — pure, credential-free, DESCRIBE
  unchanged), so `/server/endpoints` resolves through the statement bridge like any path. The
  CLI's offline engine deliberately never mounts it: an unrouted `/server` read outside a
  daemon is what keeps the host-not-serving refusal honest.
- **The write leg**: the daemon's one commit path routes `ServerConfigWrite` effects into the
  **live** `ServerConfigApplier` — the same lock the boot replay mutates, so §10's "no
  privileged config loader" now covers network commits too — then `reconcile_all()`, the
  audit entry, and the boot-config re-emission. (Today every injected committer — MCP,
  dashboard, watchtower — clones a throwaway registry, which is precisely why a network
  `ServerConfigWrite` could not converge; the routing fix is the whole job.)

**Who may drive it (§8)** — three independent controls, all pre-existing, none minted here:
(1) the **face gate** — bearer validation via the booted OAuth AS, exactly as `/mcp` is gated
today; without AS material the documented loopback-trust dev posture applies, **and a
non-loopback bind without bearer material refuses the commit bridge fail-closed** (the one
hardening this amendment adds); (2) the **default-deny policy engine** — a `/server` write
commits only under a policy that explicitly grants the verb on the `server` driver
(path-scoped `/server/**`); the bridge's commit gate resolves the **live `/server/policies`
row named `api`** (the cookbook's taught convention — no new name, no env knob; absent that
row the gate stays the empty default-deny it always was, and a broad `ALLOW ALL` still does
not grant the irreversible REMOVE); (3) the **irreversible ack** — destroys carry the same
explicit ack the approval-card commit already requires. The `/server` half applies
statement-by-statement in plan order — the boot-replay shape — so a partial failure is
per-statement and a re-plan converges (idempotency is the recovery, §7). With both legs wired,
the reachable-daemon `ServerFaceNotWired` refusal retires; `HostNotServing` remains the
no-daemon truth.

**The diff.** Collections are keyed by their natural identities — a binding by its path/name
(`ServerNode` collection + name), a declared driver by `(kind, name)`, a policy by name
**within its store** (`/server/policies` and the system-DB `sys_policies` are two collections
with two key spaces — the emitter and the differ never conflate them), a setting by key, a path
binding by path. Equality within a matched key is the canonical-spec compare above (byte
equality of `canonical()` for body-bearing rows; normalized field equality for scalar rows).
Equality — and the emitted document itself — cover the **config projection only**: runtime
fields (`ViewDef.last_run`, `cache_json`, `JobDef.last_run`) are execution state, not
configuration — excluded from the canonical form and from drift, and an `Update` **preserves**
them (a reconcile never resets freshness or a materialized cache; the round-trip ratchet holds
precisely because a refresh between fetch and apply is not drift). The result is **one batch `Plan`**: `EffectKind::ServerConfigWrite` nodes
carrying `ServerWriteOp::{Insert, Update, Remove}` for the server side, the existing `/sys`
effect nodes for the system-DB side. `preview()` renders it as the terraform-style summary —
**"Plan: N to add, M to change, K to destroy"** — with `Preview.irreversible` naming every
destroy node individually. An unchanged desired state produces an **empty plan** (`is_pure`):
"No changes." — the idempotency contract, and the round-trip test (fetch → apply immediately →
empty plan) is the ratchet that keeps the emitter and the differ honest.

**The apply.** The batch plan drives through the existing `commit()` walk — which takes **one**
`PlanApplier`, so a thin **dispatching applier** routes each node to the existing machinery
(`ServerConfigApplier` for `/server` nodes, the `SystemDbBackend` path for `/sys` — a router,
not a third applier; `ServerConfigApplier` already refuses foreign nodes, so mis-routing fails
loud) — then `reconcile_all()` converges the live causes from the new snapshot: routes mount and
unmount, schedules start and stop, watchers attach and detach. Removing a binding still
referenced by a live cause is not a special case — tearing down its route/schedule/watch is the
same `Binding::reconcile` that set it up; the destroy in the plan is the consent, the reconcile
is the mechanism. Every applied effect lands in the hash-chained `sys_ddl_events` WORM tail —
a reconcile's provenance is ordinary DDL provenance, one committed batch in the one ledger.

**Cross-store honesty.** `/server` state and the system DB are two stores, so a whole-config
apply is §7's cross-source case: **best-effort orchestration with explicit partial-failure
recovery, never silent atomicity**. `CommitReport.applied` lists exactly what landed;
a failed node stops its dependents (recorded skipped) and the report names the frontier.
Recovery is structural, not procedural: because the diff is always recomputed from live current
state, re-running `qfs plan` after a partial apply yields exactly the remainder — the reconcile
loop is its own recovery tool, and re-applying converges (the `UPSERT`-shaped idempotency of §7,
expressed at the config level).

**Safety.** `qfs plan` never writes — it is `PREVIEW` wearing a provisioning coat. `qfs apply`
whose plan contains **any destroy** requires the explicit `--commit-irreversible`
acknowledgement (§7: `REMOVE` is inherently irreversible), and a policy denial (§8 —
`CREATE POLICY` gates apply to reconcile writes identically) is **never conflated** with a
missing ack: the two controls stay independent, either alone refuses. Secrets never enter the
loop at all — the document carries references, the vault resolves them only where the live
definitions already do.

**Versioning & anti-drift.** MINOR (§12). Shipping regenerates
`docs/{language,drivers,server}.md` (`gen-docs --check` holds), regenerates the Agent Skills
(`gen-skills --check`), and bumps all four plugin version fields (a taught-surface change). Any
change to the config schema ships a **new** migration — a shipped migration body is never
edited (`check-migrations`). All reconcile tests are hermetic: round-trip idempotency
(fetch → apply → empty plan), add/change/destroy coverage, cosmetic-formatting-is-not-drift,
stale-base refusal (+ `--allow-stale-base` override), secretish-settings exclusion (never
emitted, never diffed, never destroyed), runtime-field preservation on `Update`,
excluded-collections-never-destroyed, and partial-failure re-plan all run offline against
temp-home stores; the live-daemon reconcile check is the implementation ticket's gate, not CI's.

**Rejected**: insert-or-skip as the end state (`restore` stays as the additive backup import,
but an operating model that can never remove or update cannot reconcile); JSONL as the SoT (the
machine form survives for backup, but agents edit statements, not row dumps); imperative
per-binding `CREATE`/`REMOVE` choreography as the agent workflow (the loop exists precisely so
an agent edits one document instead of sequencing N statements against a moving target); a
Terraform-style state file (the config store *is* the state — a third artifact would be one
more thing to drift); pretending cross-store atomicity (§7's honesty rule); a new frozen
keyword for `plan`/`apply` (CLI verbs, grammar untouched); reading an unreachable daemon as an
empty current state (it would plan the whole document as adds — refuse instead); redacted
placeholders in the SoT (exclusion, not redaction — a placeholder is drift or a clobber);
a privileged config API for the reconcile transport (the daemon's public statement face is the
only face, §14's layering rule).

## 17. Command-execution assurance — *implemented (enforced lock)*

qfs runs external services by speaking their protocols, not by shelling out — so "there is no
command-execution risk" is a load-bearing security property. As with §15's one-seam model-call
lock, this section turns that from an audit claim into a **mechanically enforced invariant**
(ticket 20260711121536). Two properties, one lock each.

**The spawn inventory.** The whole shipped runtime spawns exactly one external program family —
`git` — from a small, fixed set of sites, plus a desktop opener. `crates/cmd/tests/exec_inventory.rs`
scans every `crates/*/src` and `xtask/src` source file (test harnesses excluded) for
`Command::new(...)` and asserts the set of `(file, spawned-program)` pairs matches an exact
allowlist:

| site | program | why |
| ---- | ------- | --- |
| `driver-git/src/applier.rs` (×2) | `git` | the COMMIT applier — `hash-object -w --stdin`, atomic `update-ref` CAS, `rev-parse` |
| `qfs/src/git.rs` (×2) | `git` | the `/git` read facet — `cat-file` / `show-ref` repo introspection |
| `qfs/src/migration_guard.rs` (×2) | `git` | release-tag / shipped-migration introspection (dev tooling) |
| `qfs/src/tty.rs` (×1) | `OPENER` | the desktop opener (`open`/`xdg-open`), `Stdio::null` |
| `xtask/src/main.rs` (×2) | `cmd`/`program` | **build-only** release tooling (`publish=false`, never shipped) |

A new `Command::new` — from a future driver, a transform executor, a declared-driver evaluator, a
codec — trips the lock immediately and demands a deliberate allowlist edit plus a matching edit to
this section, in the same PR.

The inventory is deliberately scoped to the **shipped runtime** — files under a `crates/*/src`
segment plus `xtask/src`. **Build scripts are out of scope by construction**: `crates/qfs/build.rs`
runs a best-effort `git rev-parse` at *build* time to stamp `QFS_GIT_SHA`, but a build script is not
under `/src/`, never runs in the shipped binary, and cannot be reached by query text or fetched data
— it is not part of the runtime attack surface the lock guards. The claim is "no path from data to
process execution *in the running system*", not "no `Command::new` anywhere in the repository."

**No path from data to a shell.** `no_shell_string_execution_anywhere_in_production_source` asserts
there is no `sh -c` / `bash -c` / `Command::new("sh"|"bash"|"cmd"|"powershell")` anywhere in
production source. There is no shell-string interpolation and no program name derived from query
text or fetched bytes; every argv is built from fixed literals + validated tokens, passed as
distinct argv elements (never shell-joined), so spaces and quotes cannot split into extra arguments.

**Argument hygiene at the git sites.** The one injection class that survives argv-element passing is
**git option injection** — a positional value beginning with `-` read as a flag (`--upload-pack=…`,
`-c core.sshCommand=…`). Two structural defenses, pinned by hermetic tests in `driver-git`:

- The only query-derived positional is a **ref name**, which routes through `qualify_ref` — it
  prefixes `refs/heads/` (or requires a literal `refs/`/`HEAD`), so a value can never present to
  `git` as a leading-`-` flag (`qualify_ref_neutralizes_option_injection_in_branch_names`).
- Every **oid** routes through `Oid::parse`, which admits only 40 hex chars; a flag-shaped string
  is rejected before it can reach `cat-file`/`update-ref` (`oid_parse_rejects_flag_shaped_and_non_hex_strings`).

**Data-path argument.** Transform output, declared-driver rows, and codec-decoded content reach
none of the allowlisted sites: those sites take only fixed literals, a config-supplied repo path,
`Oid`-validated oids, and `qualify_ref`-qualified names — none of which is a program name or an
unsanitized argument. There is no seam by which fetched or query-derived bytes become a spawned
program or an unescaped argument, and the lock keeps it that way.

**Standing rule.** A new process-spawn site is a deliberate security decision: it requires a
blueprint edit to this section, an allowlist edit in `exec_inventory.rs`, argument-hygiene coverage,
and confirmation that the program is fixed and no argument derives from query text or fetched bytes
— all in the same PR.

## 18. Switch routing — the model picks the branch

*(Ticket 20260711121532; extends decision W (§15), rides the pipeline-valued-lambdas adoption and
the stage admission test. Status: **implemented** (v0.0.56) — grammar (`switch`/`else` contextual
idents, the bounded arm-boundary scan), the eval-side arm-union plan, the resolve-stage arm gates
(write capability + CALL procedure), the commit-boundary routing (one materialization → partition →
embed/prune → one apply), `PipeOp` 19→20, and the hermetic parse/eval/e2e suites. The scope cuts
this first slice makes are recorded at the end of this section.)*

**A. The disciplined shape.** "Let the AI choose the tool" decomposes into two constructs qfs
already governs, plus one new pure stage. A `transform` whose `OUTPUT` carries a **closed enum-like
column** — a refined type, e.g. `CREATE TYPE route (value text NOT NULL) WHERE value IN
('urgent','archive','other')` — produces the *choice* inside the one model-call seam; membership-
checking at apply (§15) already guarantees the model's answer lies in the declared value set. A new
**`switch` stage** then routes rows to one of several **declared sub-pipeline arms** by that column's
value. The switch itself calls no model and performs no effect of its own: it is a pure, planable
partition-and-dispatch over alternatives that are all lexically in the statement. The model chooses
**among** pre-declared arms; it can never invent an effect, because every effect node in every arm
exists in the plan before the model is ever called. The one-seam lock (§15) holds untouched: a
model-call effect node originates only from `PipeOp::Transform`; `PipeOp::Switch` originates none.

**B. Surface form.**

```
|> switch <col> { '<label>' => <pipeline>, …, else => <pipeline> }
```

`switch` and `else` are **contextual identifiers** in stage position (the `transform` lesson): the
frozen keyword set does not move. `{`/`}` are the existing brace tokens; `=>` is the existing arrow.
Each arm's right-hand side is a **pipeline-valued lambda** written as a bare pipeline continuation
over the arm's routed partition (`'urgent' => <pipeline>` is notation for
`(rows) => rows |> <pipeline>`). Examples:

```sql
FROM /mail/inbox |> select id, subject, body
  |> transform triage        -- OUTPUT (…, route route NOT NULL): the model's choice
  |> switch route {
       'urgent'  => select subject |> INSERT INTO /slack/ops-alerts,
       'archive' => select id |> CALL mail.relabel(label=>'archived'),
       else      => select id, subject |> INSERT INTO /mail/drafts
     }
```

```sql
FROM /drive/Inbox/contracts |> transform classify-doc
  |> switch kind { 'invoice' => INSERT INTO /sql/books/invoices,
                   else      => INSERT INTO /drive/Inbox/unsorted }
```

**C. Semantics — ruled.** (1) **Per-row routing, batched per arm.** Routing is per-row (whole-
relation routing is the degenerate case: a relation-wise transform emitting one row). At commit the
input relation is **partitioned** by the discriminant, then each arm's pipeline runs **once over its
partition, arms in declaration order** — set-oriented, deterministic, effects batch per arm; there is
no per-row interleaving (interleaving would make the commit envelope's ordering depend on model-
produced row order — rejected). (2) **Exhaustiveness is a plan-time type check, surfaced at
PREVIEW.** Over a **closed refined type** (a finite value enumeration) the arm labels plus optional
`else` must cover the value set; over an **open type** (plain `text`), `else` is **mandatory** — a
non-exhaustive switch fails at PREVIEW with a structured error naming the uncovered values. No
runtime hole exists: a closed discriminant is membership-checked at the transform seam, an open one
falls to `else`. (3) **Commit envelope.** PREVIEW is model-free, so the taken arm is unknowable;
therefore **every arm's effects are previewed and the union is the statement's declared effect
set**. Per-arm counts follow the honest-count doctrine: an upper bound, never a fabricated split. At
commit an arm with an empty partition **does not fire** (`affected: 0`) — untaken arms were consented
to but spend nothing. If any arm carries an irreversible effect, the union does. (4) **Typing
rule**: all arms effect-terminal (`Relation<Sᵢ> → Plan`) or all pure with a unifiable output; mixing
is a plan-time error. One reconciliation stated deliberately: the pipeline-valued-lambda rule forbids
effects inside *`let`-bound* lambdas (reusable, opaque at use site); switch arms **may** carry effect
stages because they are inline in the statement — the gate sees every arm whole. The prohibition is
on *hidden* effects, and nothing here is hidden.

**D. Stage admission.** `switch` passes on the **effect-gating** criterion (it constructs branch plan
nodes PREVIEW/COMMIT must see as one envelope) and the **cardinality/routing** criterion (it
partitions rows and determines which downstream alternative sees them). It states its typing rule,
declares its full effect set up front, adds **no** model-call path, and the model's choice can only
*narrow* execution within the declared union, never widen it. Forced-local like `transform`; no
pushdown.

**E. Governance.** Two facts move deliberately, in one change: `PipeOp` **19 → 20**
(`PipeOp::Switch`), keyword freeze **unmoved** (contextual idents). SemVer: **MINOR** (additive
grammar) — so all four plugin `version` fields bump and gen-docs/gen-skills regenerate (taught
surface). Defense-in-depth: each arm's effect nodes are individually policy-gated at plan time —
routing composes existing gates, it does not merge them. Access control: arms touch only paths the
policy layer grants to the statement's principal; a model-produced label selects among pre-authorized
plans and can never escalate to an ungranted path.

**F. Alternatives rejected.** (1) **CASE-expression reuse** — a `CASE` picking effects would put
effects in the pure/total/row-scoped expression layer, the exact hole rejected permanently for
expression-layer transform calls; a `CASE` may *compute* a discriminant but cannot route to
sub-pipelines. (2) **Per-arm separate statements** (the agent reads the transform's output, issues a
follow-up per label) — the N-roundtrip problem §15 exists to kill; the effects never land in one
previewable envelope and the union is never declared. (3) **The `switch` stage — adopted**: the only
shape where the choice is model-made yet the full effect set is operator-consented before any model
runs.

**Owner calls ratified** at implementation (v0.0.56, owner-directed drive): (1) per-row routing
batched per-arm in declaration order, no interleaving; (2) switch arms may carry effects despite
the pure-lambda rule, reconciled as "the ban is on *hidden* effects — inline arms are
gate-visible"; (3) exhaustiveness split by discriminant type (closed refined enum: label coverage;
open type: mandatory `else`), failing at PREVIEW; (4) untaken arms previewed-but-not-fired with
`affected: 0`; (5) `switch`/`else` as contextual idents (keyword freeze unmoved), `PipeOp` 19→20,
MINOR.

**First-slice scope cuts** (each a deliberate, recorded deferral — the surface above is the ruled
end state):

1. **`else` is mandatory for every switch** — the closed-refined-enum half of call (3)
   (label-coverage exhaustiveness allowing `else`-less switches) needs the transform OUTPUT
   schema to *carry* its refined value set (`ResolvedTransform` holds plain `ColumnType`s today);
   until refinement-carrying schemas land, the open-type rule applies universally, which is the
   fail-closed side of the split. A missing/duplicate/non-trailing `else` is the structured
   `switch_shape` error.
2. **All-pure switch is deferred** — every arm must be effect-terminal (`INSERT`/`UPSERT INTO`
   write or effect `CALL`); a pure arm is the structured `switch_arm_not_effect` refusal. The
   all-pure form (arms with a unifiable relation output, the engine-side `CombineOp` route) adds a
   read-plan lowering this slice does not need for the mission capability (route-to-effect).
3. **Arm vocabulary is row-local** — `where`/`select`/`extend`/`set`/`aggregate`/`group by`/
   `order by`/`limit`/`distinct`/`as` over the routed partition; `join`/set-ops/`expand`/codecs/
   `transform`/nested `switch` inside an arm are the structured `switch_arm_op_unsupported`
   refusal (a second model call inside an arm would also break the one-materialization spend
   contract).
4. **`UPDATE`/`REMOVE` are not arm terminals** — both are self-contained (`SET`/`WHERE`) rather
   than partition-consuming; route to them via a terminal `CALL` or lift them to their own
   statement.
5. **The committed summary lists fired arms only** — an untaken arm is consented at PREVIEW and
   pruned at commit (its effect node never reaches the applier), so the commit report shows
   exactly what ran; the `affected: 0` reading of call (4) is realized as prune-not-fire, not as
   a zero-count applier invocation (an applier handed an empty batch could still act — e.g. a
   CALL resolving its target live — so it is never handed one).
6. **Unmatched and non-text discriminant values fall to `else`** at commit (the membership check
   at the transform seam already pins a *closed* discriminant to its declared set; the open-type
   fallback is total by construction).

## 19. Agents — a principal, not a process — *blueprint (rulings settled 2026-07-18; grammar/subject/functions/cadence being built)*

*(Owner rulings, 2026-07-18. An **agent** is a new **user principal** the language can create:
`CREATE AGENT <name>` grants a non-human actor a scoped reach, named query functions, and an
optional launch cadence. The decisive framing — from which the five axis rulings follow — is that
an agent is a **principal, not a process**: it is an identity that owns grants and saved plans, NOT
a new runtime, scheduler, or effect path. Everything below reuses a shipped seam; a new backend, a
new daemon, a second vault, or a new execution semantics are each **rejected**, recorded as such.
This chapter is the contract the mission's remaining tickets implement against — the seam names are
written exactly as those tickets cite them so implementers land on the same files.)*

**A. Row home — an agent is `/server/agents` binding rows.** *(ruled)* An agent's declared shape
lands as `ServerBindingDdl` rows on the `/server/agents` surface, read back beside `/server/jobs`,
exactly as every other server binding is data under `/server/...` (§10). It constrains
[`core/src/ddl/server.rs`](../packages/qfs/crates/core/src/ddl/server.rs) (a new `AgentDecl` beside
`EndpointDecl`/`JobDecl`/`ViewDecl`, a new `ServerNode::Agents`, its credential-free schema, and the
same `binding_config_row` → `config_row_batch` → `desugar_to_insert` desugar the other bindings
ride). **Reason:** a binding row is already the closed-core "declared as data" home — the §16
provision dump/restore loop round-trips it, `REMOVE` drops it through the standard gate, and a new
backend adds zero variants. **Rejected — a durable `/sys` identity now:** minting an agent as a
first-class `/sys` principal (a durable, cross-host identity) is more than this mission needs and
would couple the agent to the identity crate. Recorded instead as the **FUTURE federation seam**:
promotion of an agent binding to a durable `/sys` principal, when cross-host agent identity is
scoped.

**B. Subject — a new `Subject::Agent` variant.** *(ruled)* The agent is a **first-class policy
subject**, via a new `Subject::Agent(String)` plus a `DecisionContext::for_agent` constructor, NOT a
reused user/role. It constrains
[`server/src/policy/model.rs`](../packages/qfs/crates/server/src/policy/model.rs) (the `Subject`
enum, its `label`/`from_label` round-trip as `agent:<name>`, and `satisfies_subject`) and
[`server/src/policy/context.rs`](../packages/qfs/crates/server/src/policy/context.rs) (the resolved
`DecisionContext` carrying the acting agent). `FOR agent <name>` routes through the existing
[`ast.rs` FOR clause](../packages/qfs/crates/parser/src/ast.rs) (`PolicySubjectAst`), adding no new
grammar beyond one more contextual `agent` word beside `user`/`role`/`group` — no frozen keyword.
**Reason:** the agent must be legible in the audit identity and the default-deny reasoning as *its
own* actor; a distinct subject keeps a `deny_reason` that names the agent, and keeps default-deny
honest — an agent with no matching rule is denied even on a path the operator context reaches.
**Rejected — model the agent as a service user:** folding the agent onto `Subject::User` would blur
the audit identity (an operator and its agent would be indistinguishable in the ledger) and weaken
the default-deny reasoning (an agent could silently inherit an operator's `FOR user` grant). The
pure enforcer `evaluate_with_context`
([`server/src/policy/enforce.rs`](../packages/qfs/crates/server/src/policy/enforce.rs)) stays **pure
— no I/O added**; the agent context is resolved up front and frozen, like every other actor.

**C. Query functions — saved-plan registry rows, a gated statement.** *(ruled)* An agent function is
a **named saved plan** — the `JobDecl` `DO <plan>` body shape **without a cadence** — stored as a row
on the agent's `/server/agents` surface, readable exactly like a `/server/jobs` row and
credential-free. Invocation (`qfs agent run <agent> <fn>` or equivalent) builds via
`qfs_exec::build_plan`, **previews by default**, and commits only through the same policy +
`IrreversibleGuard` chain the sweeper's `LiveCronCommitter` uses — evaluated under the **agent's**
`DecisionContext`, never the operator's. **Reason:** a function is a *gated statement*, so it cannot
shortcut the preview/commit gate; it desugars to the shipped pipeline and adds no execution
semantics. The **§5.9 pure-lambda effects ban STANDS**: a function is a named gated statement, not a
lambda — if functions were pure lambdas they would escape the preview/commit gate, which is exactly
why that shape is **rejected**.

**D. Fire chain — `DecisionContext` threaded to the `qfs_watchtower::Committer` seam.** *(ruled)* An
agent with a launch cadence enters the **same sweep** as `/server/jobs` — no new scheduler. The pure
`qfs_watchtower::cron::fire_due` decision stays pure; the gated fire runs inside the injected
[`Committer`](../packages/qfs/crates/watchtower/src/commit.rs), and the fire path is
[`qfs/src/sweeper.rs`](../packages/qfs/crates/qfs/src/sweeper.rs)'s `LiveCronCommitter` +
`watchtower::Committer`. The `Committer` seam is **extended to carry the firing principal** so the
policy gate evaluates the agent as subject (axis B) rather than the operator. Because the `Committer`
trait lives in the wasm-clean pure core (which must not depend on `qfs-server`, per the dep-direction
guard), the seam carries the principal as an **owned, vendor-free descriptor** (the agent name); the
native committer constructs the `DecisionContext::for_agent` at the gate. Runs append to the agent's
own run history through `job_runs_schema`
([`core/src/ddl/server.rs`](../packages/qfs/crates/core/src/ddl/server.rs)), carrying the firing
principal. **Reason:** reusing `fire_due` + `LiveCronCommitter` means the durable `last_run`,
missed-fire collapse, and skip-if-running semantics (§10) hold for an agent for free, and the
decision/committer split stays intact. **Rejected — a forked agent scheduler:** a second scheduler
would duplicate the ruled cron semantics and risk them diverging.

**The ruled irreversible property (recorded verbatim):** `IrreversibleGuard` with `RunMode::Server` +
`Ack::Absent` → **an agent can NEVER fire an irreversible plan unattended.** A scheduled agent fire
is unattended (`RunMode::Server`) with no ack path on a timer (`Ack::Absent`), so an irreversible
`REMOVE` / declared-irreversible `CALL` is refused fail-closed — this is a **ruled property**,
asserted by its own test, not an incidental behaviour of the guard being exercised elsewhere.

**E. Secret posture — policy-subject only, daemon-mediated.** *(ruled)* There is **no second vault**.
An agent's reach is **exactly its `ALLOW … AT` grants evaluated against its subject**, at the §8
store boundary — the same boundary every actor's reach is enforced at. `DESCRIBE /server/agents` and
reading an agent's function surface are **credential-free from the start**. **Reason:** the agent is
a *policy subject* (axis B), so its authorization is already fully expressed by its grants — a
separate credential store would be redundant surface and a second thing to leak. **Rejected — a
per-agent credential store:** recorded instead as the **FUTURE seam**: per-agent credential handles
(an agent holding its own resolvable secret refs), if and when an agent must act with a credential
distinct from the daemon's.

**Out-of-scope, recorded as named FUTURE seams (not omissions):**

- **Federation** — promoting an agent binding to a durable `/sys` principal (axis A), for cross-host
  agent identity.
- **Delegation chains** — an agent granting a subset of its reach to another agent; today an agent's
  reach is exactly its own `ALLOW … AT` grants, with no re-delegation.
- **Per-agent credential handles** — an agent resolving its own secret refs (axis E), distinct from
  the daemon's mediation.

## Retirement record

This blueprint absorbed and retired the numbered design pile on 2026-07-04 (owner directive:
hold one comprehensive design snapshot; scrap-and-build history lives in git, not in documents):

- `docs/adr/0001…0007` → §11; `docs/adr/0008` → §8; `docs/adr/0009` (rev. 2) → §3, §5, §8, §9.
  The files are deleted; git history holds them; citations in old tickets/commits resolve there.
- `RFD-0001` (`.workaholic/RFDs/`) is **superseded by this blueprint** for design content. The
  file remains temporarily as a frozen citation anchor — crate doc-comments cite its §numbers —
  until the citation sweep replaces them with blueprint anchors; then it is deleted too.
- **No new numbered design documents.** Design lands here, revised in place.
