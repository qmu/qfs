# Design brief — the shell face of the typed-path space

*(Mission `language-design-review-layering-principles-and-semantic-gaps`, scope item "The shell
face of the typed-path space", owner-raised 2026-07-09. Deliverable: adopt-with-plan or
defer-with-reasoning. All file references are to the workspace under `packages/qfs/`.)*

---

## 1. State — what exists today

The single most important fact for this decision: **the shell face is not a proposal; most of it
shipped in ticket t28 and is running code.** The decision is therefore not "should qfs have a
shell" but "what are the ruled semantics of the shipped verbs over the *typed*-path space —
definition catalogs, relational tables, append logs — and which gaps are principled vs. bugs."

### 1.1 The interactive REPL exists, exactly in the leaning's shape

Bare `qfs` runs `run_interactive_shell` (`crates/qfs/src/shell.rs:106`). The binary owns only the
line reader, history file, prompt, and rendering; **all shell logic lives in `qfs_exec::shell`**
(`crates/exec/src/shell/{desugar,session,path,complete}.rs`), by the topology guard stated in that
module's header. The architecture the mission's leaning asks for — *thin REPL layer, desugar to
core statements, session cwd, zero new keywords, stateless pure engine* — is the architecture
that shipped:

- **Verbs are line-head lowercase idents, not keywords.** `Builtin::from_head`
  (`crates/exec/src/shell/desugar.rs:44`) recognises `ls cd pwd cat cp mv rm` only as the first
  token of a REPL line, case-sensitively lowercase; `LS x` at the prompt parses as raw pipe-SQL.
  The frozen keyword set (`crates/lang/src/keywords.rs:206`, the committed fixture) is **39
  entries and contains none of the shell verbs** — verified by count; `of`/`type`/`transform`/
  `table` are likewise contextual idents. Constraint 1 holds with zero work.
- **Desugaring is to source text, not AST** — deliberately, so a builtin produces byte-for-byte
  the same `Statement` (hence the same `Plan`) as the equivalent typed statement (desugar.rs
  header). The shipped table:

  | verb | lowers to | gate |
  |---|---|---|
  | `ls [p]` | `<abs> \|> SELECT name, size, is_dir, modified` | pure read |
  | `cat p` | bare `<abs>` (a read) | pure read |
  | `cp s d` | `UPSERT INTO <d> <s>` | preview→COMMIT |
  | `mv s d` | `UPSERT INTO <d> <s>` **then** `REMOVE <s>` (copy→verify→delete) | preview→COMMIT, both legs previewed |
  | `rm p…` | one `REMOVE <p>` per arg | preview→COMMIT |
  | `cd` / `pwd` | **no statement form** — pure session state (desugar rejects them; the session handles them) | none |

- **cwd is REPL session state and nothing else.** `VfsPath` + `resolve`
  (`crates/exec/src/shell/path.rs`) is pure lexical resolution: relative joins the cwd, `..` pops
  clamped at the mount root (never crosses a driver), `~`/`/` anchor at the mount root, an
  absolute `/other/x` crosses drivers freely. Every statement reaching the engine is absolute.
  The one-shot path is the mirror-image proof: `addressing::validate`
  (`crates/exec/src/addressing.rs`) *rejects* relative paths pre-parse in `qfs run` because "in
  one-shot mode there is no cwd". Constraint 2 holds structurally.
- **Mutation inherits the gate.** `Session::eval_statements`
  (`crates/exec/src/shell/session.rs:139`) funnels every line — builtin or raw — through the same
  `parse → build_plan → plan_preview / apply_via` pipeline as `qfs run`
  (`crates/exec/src/lib.rs:166` `run_oneshot`, `:273` `preview_or_commit`). The REPL previews by
  default and applies only on a typed bare `COMMIT` confirming the pending line
  (`crates/qfs/src/shell.rs:693-728`); one-shot requires `--commit` and the
  `IrreversibleGuard`/`SafetyMode` floor. A builtin structurally cannot shortcut the gate.
  Constraint 3 holds.
- **`cd` is already capability-gated** — `namespace_check` (session.rs:226) resolves the target's
  driver and admits only `Archetype::BlobNamespace | ObjectGraphWorkflow`; anything else is a
  structured `not_a_namespace` capability error.
- **Tab-completion is a pure API** (`crates/exec/src/shell/complete.rs`): builtin names, mounts,
  and path segments via a per-parent `ls` under a 750 ms timeout with a per-prompt cache. It is
  not yet bound to inline TAB (minimal std line reader; rustyline was unavailable offline —
  recorded decision in `crates/qfs/src/shell.rs:21`).
- **Mount parity with one-shot**: the shell mounts every CONNECTed cloud/sql/git/declared surface
  plus `/sys` and `/transform` via the shared `register_cloud_and_sys_mounts`
  (`crates/qfs/src/shell.rs:234`), so `cp /local/x /drive/y` resolves and plans (regression
  ticket 20260707181404 closed exactly this).

### 1.2 The catalog-as-relation half — partially realized

- `/transform` is a real mount (`crates/driver-transform/`): `ls /transform` / `SELECT` lists
  definitions, `DESCRIBE /transform/<name>` reports schemas + derived mode, `INSERT INTO
  /transform` creates, `REMOVE /transform/<name>` drops behind the irreversible gate. This is the
  full "definition catalog under shell verbs" story, shipped for one catalog.
- `/sys/drivers` is queryable and is where declared types live (`kind='type'` rows named
  `/type/<name>` — `crates/qfs/src/declared_driver.rs:516`).
- **`/type` is NOT a mount today.** No driver registers it; `ls /type` does not resolve. The
  blueprint's "`ls /type` is SHOW TYPES" (§5.4, docs/blueprint.md:274) and the mission's
  "already realize" phrasing are ahead of the binary: the *reference* side of §5.5 shipped
  (`type_name` in `crates/parser/src/grammar.rs:2173` canonicalizes bare names to `/type/<segs>`
  and rejects path-form references), but the *catalog face* — the path you `ls` — exists only for
  `/transform`.

### 1.3 The membership machinery a policed `cp` needs — shipped

- `check_membership` (`crates/core/src/membership.rs:60`): pure per-row refinement evaluation
  under a capability-denied context; structured `MembershipError` naming predicate + columns,
  never row data.
- The SQL apply facet enforces it at commit: `SqlContractApplyDriver` (`crates/qfs/src/commit.rs:275`
  → `sql_contracts.rs:148 check_table_membership`) checks **`effect.args.rows`** — and
  `materialize_pipeline_source` (`crates/exec/src/lib.rs:410`) executes a pipeline-sourced write's
  source at the commit boundary and **embeds the materialized rows into that same `args` channel**
  (`consume_source_into_write`), capped at `MAX_MATERIALIZED_ROWS`. Consequence, verified in code:
  **a `cp` (= `UPSERT INTO <table-of-T> <src>`) whose destination is an `OF`-typed table IS
  membership-checked per row at commit, today** — the blueprint §5.4 caveat "until that seam
  carries rows" is stale; the seam carries rows. Declared views likewise membership-check
  delivered rows (`crates/exec/src/declared.rs:489`).
- Mid-pipe `of` is the 22nd `PipeOp`, plan-time structural check with
  `of_assertion_failed`/`of_type_unresolved` (`crates/core/src/eval.rs:911,926`), and a query
  carrying `of` is routed through the evaluator so the check runs against the addressed path's
  schema (`run_oneshot_inner`, exec/lib.rs).

### 1.4 The two real defects the typed-path reading exposes

These are where the shipped shell face *violates* §5.1's "one type, three faces" and motivate the
ruling:

1. **`ls` is blob-typed, not path-typed.** The desugar hardcodes
   `SELECT name, size, is_dir, modified` — the blob-listing projection. `Schema::project` errors
   on an unknown column (`crates/types/src/schema.rs:244-250`), and gmail describes `MailMessage`
   columns, `/transform` describes definition columns — so **`ls /mail/inbox` and `ls /transform`
   fail with `unknown column` today.** `ls` currently contradicts §5.1's own words ("`ls` is a
   query … shapes what `ls p` enumerates"): the projection should come from the path's type, not
   from a frozen guess.
2. **The `cd` gate is archetype-coarse.** `/sql/<conn>` (the table catalog), `/transform`, `/sys`
   all describe as `RelationalTable`, and gmail describes `AppendLog` even at the root/label
   level (`crates/driver-gmail/src/lib.rs:278`) — so `cd /sql/erp`, `cd /transform`, `cd /mail`
   are all refused as `not_a_namespace`, while their `ls` (once fixed) is perfectly meaningful.
   The gate conflates "is a namespace whose elements are locations" with two specific archetypes.

Also absent: `mkdir` (no builtin; Drive folder creation exists as `INSERT` carrying the folder
MIME — `crates/driver-gdrive/src/effect.rs:747`), and `describe` as a *shell* builtin (`qfs
describe <path>` is a one-shot CLI mode, `run_describe`, exec/lib.rs:921 — inside the REPL you
cannot describe without leaving).

---

## 2. The design space

### Option A — ratify and complete the thin desugar layer *(the owner's leaning, stated precisely)*

The shell face is a REPL-layer construct: a closed verb set recognised only at the line head,
each verb desugaring to closed-core statements over absolutized paths, semantics derived from the
destination path's **entry kind** (its describe — archetype + schema + capabilities), pure
navigation (`ls cd pwd cat describe`) building no plan, gated mutation (`cp mv rm [mkdir]`)
lowering to `insert/upsert/remove/update` and riding preview→commit. Zero new keywords; the pure
engine never sees a cwd.

What it buys: §5.1 becomes checkable end-to-end ("for every describable path `p` … (c) shapes
what `ls p` enumerates" — currently false for non-blob paths); the definition catalogs get the
full Unix story the §5.5 analogy promises (defs stored at catalog paths, `ls`ed and `rm`ed there,
invoked by name); agents and operators get one navigation idiom over every service.
What it costs: per-entry-kind desugar rules must be *ruled*, not improvised — that is §3 below.
The implementation cost is small because the machinery (describe registry, capability verbs
`Verb::Ls/Cp/Mv/Rm` already in `crates/driver/src/lib.rs:78`, membership at apply) exists.

### Option B — no cwd, always-absolute (retire `cd`/`pwd`, keep the verbs)

The purist reading: cwd is hidden state, and hidden state in an agent-facing tool invites
mis-addressed writes (`rm x` meaning different things depending on an earlier `cd`).
What it buys: every line is self-contained and auditable in isolation; the REPL history is a
replayable script.
What it costs: it is refuted by working, tested code — cwd is *session* state that absolutizes
before parse, the engine is provably cwd-free, and the preview echoes the absolute path before
any commit, which is the actual mis-addressing defense. Retiring `cd` buys purity the
architecture already has. **Reject.** (The auditability point survives as a rule: the *preview
must always render the absolute path* — it does, via desugar-to-source.)

### Option C — a richer shell model (flags, globs, shell-only semantics, new keywords)

`ls -la`, `rm -rf`, glob expansion in the shell layer, a `find` verb, etc.
What it buys: familiarity.
What it costs: it breaks "the shell adds no execution semantics" (blueprint §9) — the moment `ls`
has behavior that is not a projection of a core statement, preview/pushdown/typing no longer see
the whole truth, and the 39-keyword core would eventually be pressured to absorb shell notions.
Set-shaped needs (`-r`, globs) belong to the *path/set* semantics the core already has (a path is
a set; `rm <folder>` is already set-wide and the destructive-set gate already exists).
**Reject permanently; cite this verdict when `-rf` is re-proposed.**

### Option D — promote the verbs into the language (an `LS` statement, `CP` statement)

What it buys: one surface instead of two.
What it costs: fails the §5.3a stage admission test — `ls` states no typing rule that `select`
does not already state; `cp` constructs no plan node that `upsert into` does not already
construct. They would be pure synonyms, and the core's discipline is that synonyms live in the
sugar layer. **Reject** — this is what §5.3a is for.

### The genuine sub-decisions inside Option A

Adopting A does not settle the design; these do:

1. **What does `ls` desugar to, per entry kind?** (the projection bug)
2. **What may `cd` enter?** (the gate predicate)
3. **What is `mv`, per entry kind?** (rename vs copy+delete vs category error — the sharpest one)
4. **Are definition catalogs write-targets for the verbs (`cp` = clone, `mv` = rename, `rm` =
   drop), and is a def path banned as a *data*-`cp` destination?**
5. **Does `mkdir` earn inclusion?**
6. **Is `cp`'s lowering `UPSERT` (shipped) or `INSERT` (the mission text), per destination kind?**

---

## 3. The hard cases — entry kind × verb

Entry kinds, from the code (the `Archetype` enum, `crates/driver/src/lib.rs:60`, refined by what
drivers actually describe): **blob namespace** (`/local`, `/s3`, `/r2`, Drive folders, git tree
`@ref` — `blob_listing_schema`), **relational table** (`/sql/<conn>/<table>`, git
commits/refs/changes), **catalog of definitions** (`/transform`, `/type` (unmounted), `/sys/*`,
`/server/*`, and the *interior* node `/sql/<conn>` — all describing `RelationalTable` today but
set-of-locations in nature), **append log** (`/mail/*`, `/slack/*`, git reflog, `/sys/audit`),
**object graph** (`/github` — CRUD + CALL).

Legend: ✓ clean desugar · ✗ category error (structured refusal) · △ ruled special case.

| | blob namespace | relational table | definition catalog | append log | object graph |
|---|---|---|---|---|---|
| **`ls`** | ✓ listing projection (shipped) | ✓ **bare read** — the rows *are* the enumeration (today: broken projection) | ✓ bare read = SHOW TYPES / SHOW TABLES (needs `/type` mount) | ✓ bare read (tail) (today: broken) | ✓ bare read (today: broken) |
| **`cd`** | ✓ (shipped) | ✗ rows are values, not locations — keep refusing | ✓ its children are definitions/tables = named locations (today: wrongly refused) | △ label/channel *trees* are navigable (`/mail`, `/slack`); a message set is not | ✓ (shipped) |
| **`cat`** | ✓ bare read of the blob (shipped) | ✓ = `ls` (degenerate; fine) | ✓ read the definition row | ✓ read | ✓ read |
| **`describe`** | ✓ pure, from the registry — needs a REPL builtin (CLI-only today) | ✓ | ✓ (this is the def's contract) | ✓ | ✓ |
| **`cp` (as dst)** | ✓ `UPSERT INTO` — idempotent blob write (shipped) | ✓ `INSERT INTO`, **membership-checked** per row at commit via the materialized-rows→`args`→apply-facet chain (works today) | △ *def-clone only from a def of the same catalog*; a **data row → def catalog is ✗** (§5.5: the two categories never pool) | △ `INSERT` = **append is a send/post** — legal but the preview must say what it is | △ `INSERT` where capabilities allow (create issue); else ✗ |
| **`mv`** | △ per driver: copy→verify→delete (shipped) or native rename where `Verb::Update/Mv` caps exist (Drive rename is `UPDATE`) | ✗ a row has no name; "move a row" is `UPDATE`/`REMOVE+INSERT` spelled honestly, not `mv`. (Table-level rename = future DDL, not `mv`) | ✓ rename-a-definition: rewrite the catalog row's name — one previewed catalog write | ✗ **the trap**: copy+delete on mail = *send a new message and trash the original*. Relabel is `UPDATE labels`. `mv` must refuse here | ✗ no rename semantics |
| **`rm`** | ✓ `REMOVE` (shipped; set-wide gate exists) | ✓ `REMOVE <path> [where …]` — the predicate form is raw pipe-SQL already; `rm` need not grow a flag | ✓ drop the def — `REMOVE /transform/<name>` ships today, irreversible-gated | △ = trash/detach where the driver grants `Remove`; irreversible gate | △ = close/delete via caps; merge stays `CALL` |
| **`mkdir`** | △ Drive: `INSERT` folder-MIME (exists as raw form); local: implicit; S3/R2: prefixes are virtual — ✗ | ✗ a table needs a schema; `CREATE TABLE` is the constructor | ✗ a def needs a body; `CREATE TYPE` is the constructor | ✗ | ✗ |

Readings off the matrix:

- **The two-way split in the mission (data path vs definition catalog) is almost clean but not
  binary.** The append log is a genuine third case for mutation: `cp` into it is an *outbound
  communication*, `mv` on it is a semantic trap, `rm` is often irreversible. The matrix stays
  coherent because every △/✗ is decided by machinery that already exists — per-node
  `Capabilities` (the closed `Verb` set) plus the archetype — not by new theory. The verb set
  does not leak; it *refuses*, structurally, which is the qfs way.
- **`mv` is the only verb without a uniform meaning.** Blob rename, def rename, and copy+delete
  are three different lowerings; on rows and logs it has none. Today's unconditional copy+delete
  desugar is wrong beyond blobs (the mail case). The rule that fixes it: **`mv` requires src and
  dst to be same-entry-kind, and lowers per kind** — blob→blob = copy+delete (or the driver's
  native rename when capabilities offer it), def→def same-catalog = rename write, everything else
  a structured refusal naming the honest spelling (`UPDATE` for relabel).
- **`cp`'s `UPSERT` vs `INSERT`**: the shipped `UPSERT` is correct for blob destinations
  (idempotent, retry-safe — the desugar comment records why) and wrong for append logs (an
  idempotent "send" is a lie) and merely tolerable for tables. Key the lowering on the
  destination's entry kind (known at desugar time from describe): blob → `UPSERT INTO`,
  table/log/graph → `INSERT INTO`. This also makes the mission's "`cp` ≡ membership-checked
  `insert into`" literally true where types police.
- **`cwd` × coordinates**: `resolve` treats `@ref` as segment text, so `cd /git/repo@v1.2/src`
  composes lexically and enters (git trees describe `BlobNamespace`); a write under that cwd
  lowers to an absolute path the git driver refuses (writes are commits only) — correct,
  structured. `{param}` template segments exist only in declared-driver *definitions*; the shell
  only ever addresses concrete paths. No new rules needed — worth one test each, not design.
- **Cross-service `cp` refusal mechanism exists**: schema mismatch surfaces at plan time (column
  resolution — the gmail attachment/Drive-upload column alignment in
  `crates/driver-gmail/src/lib.rs:266` shows the deliberate happy path) and per-row membership at
  commit for refined destinations. The one rule to *add* is plan-time, cheap, and categorical: a
  destination inside a definition catalog admits only definition-shaped writes from the same
  catalog kind (data→def is `category_error`, the §5.5 line).

---

## 4. Recommendation — adopt, with a three-slice plan

**Adopt.** Ratify Option A as the ruled architecture and record it in the blueprint (a §9
re-founding: "the shell face is a REPL-layer desugar over the typed-path space; verb semantics
derive from the path's entry kind; pure navigation builds no plan; gated mutation lowers to the
closed core and rides preview→commit; zero new keywords; cwd is session state absolutized before
parse"). This is not speculative: four-fifths of it is shipped and tested, all five hard
constraints verify against the code today, and the remaining work is closing the two places where
the shipped shell *contradicts* §5.1 plus mounting `/type`. Deferring would leave `ls` broken on
every non-blob path and the mission's "already realize" claim untrue — the cheapest possible
adoption against a real defect list.

**Slice 1 — `ls`/`cat`/`describe` become path-typed (pure navigation; no gate involvement).**
Desugar `ls p` by entry kind: blob namespace keeps the `name, size, is_dir, modified` projection;
every other kind lowers to the bare read `p` (the path's rows are its enumeration — §5.1 clause
(c) becomes true). The desugar already resolves the path; give it the describe registry (the
`Session` has `engine.mounts`) so the projection choice is a lookup, not a guess. Mount `/type`
as a read-only catalog mirroring `/transform`'s split (pure describe facet; System-DB-injected
read facet over `sys_drivers kind='type'`) so `ls /type` = SHOW TYPES ships. Add `describe` as a
pure REPL builtin reusing `run_describe`'s machinery. Acceptance: `ls /mail/inbox`,
`ls /transform`, `ls /type`, `describe /type/<name>` all answer inside one session.

**Slice 2 — the `cd` gate becomes "enumerable-children", not "two archetypes".**
Replace `namespace_check`'s archetype pair with a predicate the driver already answers:
a node is enterable iff its children are *locations* (blob namespaces, object graphs, catalog
interiors — `/sql/<conn>`, `/transform`, `/type`, `/sys`, `/server`, the mail/slack label/channel
trees), and a row-bearing leaf is not. Concretely: key on the node's capability to `Ls` a
child-of-locations relation, or introduce a `NodeDesc` boolean the describe facet states — either
way it is data the pure registry serves, and drivers whose interior nodes misdescribe (gmail's
root as `AppendLog`) get a describe fix, which is a driver-conformance correction §5.1 already
demands. Keep refusing `cd` into row-sets.

**Slice 3 — the mutation verbs get their per-kind ruling (behind the existing gate).**
(a) `cp` lowering keyed on destination kind — `UPSERT INTO` for blob, `INSERT INTO` otherwise;
membership continues to police refined destinations through the shipped materialize→args→apply
chain (no new machinery; add the cross-service golden test).
(b) `mv` same-kind rule: blob→blob as today (native rename where caps allow), def→def rename as
one catalog write, all else structured refusal naming the honest spelling.
(c) def-catalog verbs: `cp`/`mv`/`rm` on `/type`/`/transform` as clone/rename/drop —
ordinary previewed catalog writes (`rm /transform/<name>` already ships; clone/rename are small),
plus the categorical plan-time refusal of data→def `cp`.
(d) `mkdir`: **defer**. Only Drive has real folder semantics and its `INSERT` folder-MIME form
exists; S3/R2 prefixes and tables/defs make `mkdir` a category error almost everywhere. A verb
that is a refusal on four of five entry kinds does not earn the slot yet.

Also in slice 3's ticket, one stale-text fix: blueprint §5.4's "until that seam carries rows"
caveat about pipeline-sourced membership — the seam carries rows now
(`materialize_pipeline_source` → `check_table_membership`); the blueprint should say so.

**Open sub-questions the owner must still rule (each one line, decidable at ticket time):**

1. `mv` on an append log: flat refusal (recommended — relabel is `UPDATE`, and `mv`-as-send is a
   trap), or allow with an explicit two-leg preview? Recommend refusal; the preview-both-legs
   escape hatch is typing the two statements yourself.
2. `cd` into the mail label tree: worth the gmail describe correction in slice 2, or park mail as
   non-navigable? Recommend the correction (it is a §5.1 conformance fix regardless of the shell).
3. `cp` def-clone across catalogs (`/type` → `/transform`): pure refusal (recommended) — there is
   no meaningful cast between definition kinds.
4. Whether the REPL should grow inline TAB (rustyline) when the dependency is available — an
   ergonomics call, orthogonal to this ruling; the `Completer` API is done either way.

One decision, three slices, no new keywords, no engine state, no new gate — the shell face
becomes exactly what §5.1 already claims it is.
