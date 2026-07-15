---
created_at: 2026-07-08T00:21:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash: e674f2b
category: Added
depends_on: []
---

# Design brief: the `transform` pipe predicate — qfs's first model-calling stage

## Overview

Write the **design brief** (a full prose brief, authored with **Fable**) that defines a new
pipeline predicate — working name **`transform`** — a `|>` stage that maps its input set through
an authenticated LLM into a **declared output schema**, and land it as a new **`docs/blueprint.md`**
section (status: `blueprint`) with a decision id.

This predicate is *not* a routine stage addition. It collides with two load-bearing invariants that
the brief must resolve explicitly and definitively (this is experimental / pre-release qfs — there
is no backward-compat or migration surface; make the definitive call, no risk framing):

1. **Decision K reversal.** `crates/driver-claude/src/lib.rs:11-15` and `docs/blueprint.md` currently
   state *"qfs NEVER hosts or calls an LLM"* — the existing `/claude` driver is a pure path façade
   over session metadata that calls no model API. This feature reverses that: qfs now makes an
   **authenticated outbound model call**. The brief replaces the Decision K text and states the new
   thesis: qfs may invoke a model, but only through the `transform` seam, behind the pure-driver +
   injected-impure-applier boundary, gated by PREVIEW/COMMIT.

2. **Closed-core keyword freeze.** The grammar keyword set is frozen (39 keywords, count-locked by
   `crates/lang/src/keywords.rs`). `transform` must **not** become a new frozen keyword (that would
   be a MAJOR grammar break contradicting the "the language never grows new keywords / three seams"
   doctrine in `crates/qfs/src/docs.rs:92-102`). It is a **contextual-identifier predicate** (parsed
   in clause position like `CONNECTION`/`DRIVER`), registered through the open registries → **MINOR**
   under the SemVer policy. The name `transform` (rather than `convert`) is deliberate: `convert` is
   already the informal vocabulary for codec chaining (`|> decode json |> encode yaml`,
   `docs/roadmap.md:37`), so a distinct verb avoids overloading that word.

### The shape the brief must specify

- **Declaration (step 1).** A `transform` *definition* is declared and stored as data (follow the
  blueprint chapter *"self-hosting integrations — a driver is data"*: declaration authored as qfs
  script, stored in system SQLite, activated on connect/auth). A definition names:
  - **input and output schema** — reuse the entity type system (*"types are sets"* blueprint chapter
    + `crates/types/src/schema.rs` `ColumnType`); do **not** invent a parallel schema language.
  - **LLM API / Agent SDK** — the provider, partitioned behind a `ModelProvider`-style driver seam
    (vendor-neutrality: a provider must be swappable).
  - **model** and **effort**.
- **Activation (step 2).** Activated by **authentication**, reusing the existing account/secret/vault
  machinery (`crates/qfs/src/account.rs`, `crates/core/src/ddl/connections.rs` `SECRET 'env:…'`
  by-reference; secrets stdin-only, never argv/inline). No new credential path.
- **Use (step 3).** `/source |> transform <def> |> order by …` — a contextual-ident pipe stage
  referencing a declared definition.
- **Definitive semantics (step 4).** The predicate covers **all three cardinality modes**, selected
  by the shape of the declared input/output schema:
  - **row-wise** — each input row → one output row (cardinality preserved, `select`-like);
  - **relation-wise** — the whole input relation → a new relation (summary / restructure, cardinality
    may change);
  - **schema-directed extraction** — a blob/text input → N structured rows matching the output schema.
  The brief must define, for each mode, the archetype it consumes/produces, its output `Schema` and
  `Provenance`, and how the mode is disambiguated from the declared schemas.
- **Planning & purity.** `transform` is inherently **local / non-pushable** (never pushed to a source
  driver) and **schema-transforming** (unlike the pass-through `decode`/`encode`). The model call is
  an **impure effect**: it does not run in the pure/synchronous/wasm engine — it is planned as an
  effect node and applied by an async runtime applier the binary injects (the `driver-claude`
  pure-driver + injected-applier template).
- **Safety.** `transform` spends tokens/quota and is non-deterministic → treat it as **irreversible**:
  its COMMIT requires the explicit irreversible ack (like `mail.send`); **PREVIEW calls no model** and
  surfaces only the effect-plan (estimated cost / model / effort / rows).
- **Versioning & anti-drift.** State the SemVer verdict (MINOR — new registry entry, no grammar
  change), and that shipping will require regenerating `docs/{language,drivers,server}.md` via
  `gen-docs`, regenerating skills, and bumping the four plugin version fields (taught-surface change).

## Key files (to read while writing the brief)

- `docs/blueprint.md` — the one living design doc; §3 closed core, §7 preview/commit + irreversibility,
  and the LLM-is-always-external references (~328-448) that this brief revises. **Revise in place** —
  delete superseded text, do not add a numbered ADR.
- `crates/driver-claude/src/lib.rs:11-15` + `crates/driver-claude/src/schema.rs` — the Decision K text
  and the pure-driver + injected-applier / no-credential-column template.
- `crates/lang/src/keywords.rs` — the frozen keyword set + count-lock test (why `transform` is a
  contextual ident, not a keyword).
- `crates/parser/src/ast.rs:169-214` (`PipeOp`) + `crates/parser/src/grammar.rs` (contextual-ident
  clause parsing for `CONNECTION`/`DRIVER`) — the seam `transform` follows.
- `crates/core/src/ddl/connections.rs` + `crates/core/src/ddl/server.rs` — the declare→typed-binding
  (`SECRET` by reference) model the definition reuses.
- `crates/types/src/schema.rs` — `ColumnType`/`Schema`/`Provenance`, the input/output schema vocabulary.
- README SemVer policy — grammar + registries = the versioned surface (MINOR/MAJOR framing).

## Related history

- Archived blueprint chapter *"self-hosting integrations — a driver is data"* — the declare/store/
  activate lifecycle this predicate reuses.
- Archived blueprint chapter *"types are sets — the entity type system"* — the schema half.
- t64 `/hosts/<host>/claude/...` driver — the driver-contract precedent (pure introspective describe +
  impure applier).
- t72 pipe-stage write grammar — the `pipe_op` grammar seam a new stage extends.

## Implementation steps (this ticket produces a design doc, not code)

1. Read the key files above; confirm the current Decision K wording and the exact keyword-freeze
   mechanism.
2. Draft the brief in the standard state / options / trade-offs / recommendation form, resolving each
   of the four axes already decided (semantics = all three modes; seam = contextual-ident `transform`,
   MINOR; Decision K = reversed; safety = irreversible gate; test boundary = injected mock provider).
3. Land it as a new `docs/blueprint.md` section with a decision id and `status: blueprint`; **rewrite**
   the Decision K passage and the "LLM is always external" references in place.
4. Enumerate the follow-on implementation surface precisely enough that the implementation ticket
   (`depends_on` this one) can be driven without re-deciding anything.

## Considerations

- Author with **Fable** — this is genuine design judgment, not mechanical work.
- Keep it definitive: no compatibility shims, no deprecation windows, no "reduce risk by splitting"
  hedging — experimental qfs takes the hard, correct shape.
- The brief is a design doc: the only gate it must pass is the `gen-docs --check` ratchet staying green
  (blueprint.md is not a generated file, but keep the doc tree consistent) and internal coherence with
  §3/§7. No code, no keyword, no version bump in this ticket.

## Quality Gate

**Verification method** (objective, checkable before `/drive` approval):

- [ ] `docs/blueprint.md` contains a new `transform` section carrying a **decision id** and
      `status: blueprint`, and the old Decision K text (*"qfs NEVER hosts or calls an LLM"*) plus the
      "LLM is always external" references are **rewritten in place** (grep the repo — the old wording
      no longer asserts qfs never calls a model).
- [ ] The section resolves **all four** design axes with an explicit ruling each: (1) semantics = the
      three cardinality modes and how each is disambiguated by the declared schemas; (2) grammar seam =
      contextual-identifier `transform`, **no new frozen keyword**, SemVer verdict = MINOR stated;
      (3) Decision K = reversed, with the new thesis (model calls only through this seam, behind the
      injected-applier boundary, gated by PREVIEW/COMMIT); (4) safety = irreversible, PREVIEW calls no
      model.
- [ ] The declaration model (input/output schema on the entity type system; provider behind a swappable
      seam; model + effort; `SECRET` by reference) and the auth-activation reuse of the existing
      account/vault surface are specified concretely enough to implement without further decisions.
- [ ] `cargo run -p xtask -- gen-docs --check` and `cargo fmt --all --check` still pass (no generated
      doc drift, no formatting break introduced by the edit).

**Acceptance criteria:** a reviewer can read the brief and, without asking a question, know exactly what
the implementation ticket must build — the predicate's surface, its three semantic modes, its plan
shape (local, schema-transforming, effect-node applied by an injected applier), its safety gate, and
its version impact.

**The gate that must pass:** the four checkboxes above, all true, verified by grep + the two `--check`
commands. Design coherence (no contradiction with blueprint §3 closed core / §7 preview-commit) is the
subjective half a human confirms at the `/drive` approval.

**Edge cases the brief must address:** the `convert` (codec) vs `transform` (LLM) naming split; a
definition whose input schema and output schema imply an ambiguous mode; PREVIEW of a `transform` that
must estimate cost **without** calling the model; a `transform` in the middle of a pipe followed by
`order by`/`where` (the downstream stages see the *output* schema).

**Division of assurance:** this ticket owns the *design* gate only. Test hermeticity, the mock provider,
and the live-provider end-to-end check are owned by the implementation ticket that `depends_on` this one.
