---
created_at: 2026-07-16T00:50:29+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort:
commit_hash:
category:
depends_on:
mission: declared-drivers-are-the-normal-way-to-add-a-service
---

# Unify the three `.qfs` statement splitters onto one token-accurate splitter in qfs-core

## Overview

Three components read a `.qfs` document, they disagree on the same text, and none of them
matches the lexer that defines what the text means.

`qfs-server`'s `statements()` is the splitter behind `qfs serve` and — because it is `pub` and
shared — behind `qfs plan` / `qfs apply` too. It strips comments **line-locally** with
`line.find("--")` (`server/src/runtime.rs:533-541`) and splits statements with `rest.find(';')`
(`:504`), both with no quote, escape, or token awareness. So a `--` or a `;` inside a path or a
quoted locator silently mangles the document.

This is not a cosmetic truncation. Reproduced through the shipped binary during discovery:

```
$ printf 'CREATE VIEW v AS /local/data/a--b.txt;\nCREATE VIEW w AS /local/data/c.txt;\n' > two.qfs
$ qfs plan two.qfs
qfs: error: line 1: parse error [RESERVED_AS_IDENTIFIER]: a reserved keyword cannot be used here
```

The truncation swallowed the statement-terminating `;`, so statement 2 was **merged into**
statement 1 and surfaced as an unrelated parse error pointing at the wrong thing. The path is
legal: `qfs run "/local/<abs>/a--b.txt"` returns the row, because `is_path_delimiter`
(`lang/src/lex.rs:659-665`) does not include `-`. The language accepts the path; only the splitter
breaks it.

Two more reproductions of the same class:

```
$ printf "CREATE CONNECTION x DRIVER sqlite AT '/data/a--b.db';\n" > q.qfs && qfs plan q.qfs
qfs: error: line 1: parse error [UNEXPECTED_TOKEN]: lexing failed: UNTERMINATED_STRING

$ printf 'CREATE VIEW v AS /local/data/c.txt; # note; more\n' > hash.qfs && qfs plan hash.qfs
qfs: error: line 1: parse error [UNEXPECTED_EOF]: unexpected end of input
```

The second one is a doc/code divergence worth naming: `runtime.rs:485-486` documents that a
**trailing** `--`/`#` comment is stripped, but the code only honours `#` when it *leads* the
trimmed line (`:534-536`). The doc describes the intended behaviour; the code has a bug; the doc
reads as if the bug were the spec.

**Why this must be one splitter rather than a patch.** `statements()`'s own doc comment
(`runtime.rs:491-493`) says it is exposed to the provisioning loader precisely so that "the
reconcile loop never forks a second, drifting statement chunker". That claim is already false:
`core/src/ddl/connections.rs:65 split_statements` **is** that second chunker, reading the third
`.qfs` surface (`connections.qfs`, via `qfs/src/connections_config.rs:12`). The same text gets two
answers today — a `--` inside a quoted locator is literal under `connections.rs` and a truncation
bug under `runtime.rs`. Patching only `strip_line_comment` fixes the symptom and leaves the fork
the code already denies having.

**`connections.rs` is not the answer to copy.** It is correct on the quote axis only. It is
escape-blind (`:72-75` toggles `in_quote` on every `'`, desynchronising on the `\'` the lexer
accepts at `lex.rs:279`), token-blind (an **unquoted** `/local/a--b.txt` is still cut at `:78` —
and bare paths are how every fixture is written), `#`-blind, and line-number-less. Dropping it into
`statements()` verbatim would break every boot fixture and every line-located error.

The authority is `qfs_lang::lex`. It is the only component that gets all of it right, and its
rules are the spec this splitter must mirror.

## Scope

Build **one** line-attributing, token-accurate statement splitter, host it in `qfs-core`, and have
all three readers call it. Owner decision, 2026-07-16.

Mirror `lex.rs`'s three rules exactly:

1. `'…'` strings are opaque, including `\'` escapes (`lex.rs:254-296`).
2. A `/`-led path token consumes to a real delimiter — `is_path_delimiter` (`lex.rs:658-665`)
   excludes `-`, so `--` inside a path is path text.
3. `--` and `#` open a line comment **only** at a token boundary outside (1) and (2)
   (`lex.rs:156-173`). Both lexicons, everywhere on the line — not `#`-only-at-line-start.

`;` splits statements only outside (1) and (2), by the same construction.

**Out of scope:** building on `qfs_lang::lex` itself. Neither `qfs-server` nor `qfs-provision`
carries a `qfs-lang` dep, and `lex` fails over the whole document — which would lose the
per-statement line-located error both callers contract for (`ServerError::Parse{line}`,
`LoadError::Parse{line}`). A hand-rolled scanner mirroring `lex.rs`'s rules is the deliberate
middle. Record the imperative choice in the PR description per
`workaholic:implementation` / `functional-programming` (imperative is sanctioned for parsers; keep
the outward interface a pure function).

## Key Files

- `packages/qfs/crates/server/src/runtime.rs:533-541` — `strip_line_comment`, the defect. Its only
  caller is `statements()` (`:501`).
- `packages/qfs/crates/server/src/runtime.rs:494-529` — `statements() -> Vec<(usize, String)>`. The
  `;` split at `:504` is the same defect class. The 1-based line attribution is a contract, not a
  convenience.
- `packages/qfs/crates/server/src/runtime.rs:484-493` — the doc comment: states the trailing-`#`
  behaviour the code lacks, and asserts the no-second-chunker property the repo violates. Both
  need to become true.
- `packages/qfs/crates/core/src/ddl/connections.rs:65-94` — `split_statements`, private, quote-aware
  only. The unification target: this fn and `statements()` collapse into one.
- `packages/qfs/crates/core/src/ddl/connections.rs:31` — `parse_connections`, the third reader.
- `packages/qfs/crates/lang/src/lex.rs:156-173, 254-296, 189-231, 658-665` — the semantics to
  mirror: comment lexicon, string+escape, `lex_path`, `is_path_delimiter`.
- `packages/qfs/crates/provision/src/load.rs:105-128` — second consumer; `LoadError::Parse{line}`
  at `:109-113` depends on the line attribution.
- `packages/qfs/crates/qfs/src/connections_config.rs:12` — re-exports `parse_connections`.
- `packages/qfs/crates/server/src/tests.rs:674-684` —
  `statement_splitter_handles_comments_and_semicolons`, the only test of `statements()`. Encodes
  the naive cases only; passes today and must keep passing.
- `packages/qfs/crates/provision/src/tests.rs:301-326` — `cosmetic_formatting_is_not_drift`, the
  `#`-tolerance guard through `load()`. **This is the landmine** any refactor must keep green.
- `packages/qfs/crates/core/src/ddl/connections.rs:96-153` — four existing splitter tests including
  the `;`-in-locator and `--`-in-locator guards. The regression pattern to follow.
- `packages/qfs/crates/provision/src/emit.rs:175-190` — emits `#` headers only; proves `#` support
  is non-negotiable and pins the emit→load round-trip.

## Implementation Steps

1. Write the splitter in `qfs-core` beside `ddl/connections.rs` (or replacing `split_statements`),
   `pub`, returning `Vec<(usize, String)>` with 1-based line attribution. Mirror the three `lex.rs`
   rules above; handle `#` and `--` identically.
2. Point `statements()` (`runtime.rs:494`) at it. Keep the `pub` re-export
   (`server/src/lib.rs:65-66`) so `provision` is unchanged at its call site, or have `provision`
   import from `qfs-core` directly and drop the reason for the `qfs-server` dep comment at
   `provision/Cargo.toml:17`.
3. Point `parse_connections` (`connections.rs:31`) at it. Three readers, one splitter.
4. Correct the `runtime.rs:484-493` doc comment: the trailing-`#` claim becomes true in code, and
   the "never forks a second chunker" claim becomes true in fact.
5. Tests per the Quality Gate below.
6. Bump the patch in `packages/qfs/crates/qfs/Cargo.toml` (`0.0.71` → `0.0.72`) per `CLAUDE.md`.

No Cargo.toml dependency change is needed: `qfs-server` (`Cargo.toml:15`) and `qfs-provision`
(`:15`) both already depend on `qfs-core`, so no new edge is created and
`crates/cmd/tests/dep_direction.rs` stays green.

## Quality Gate

Owner-selected, 2026-07-16. All must hold before approval:

1. **The TMPDIR reproduction goes green.** The two `job::tests` that fail whenever `$TMPDIR`
   contains `--` pass. These are the pre-existing latent regression test for this bug, recorded in
   story `work-20260707-180554:149-151` and in the concern — reproduce with a `--`-bearing TMPDIR
   rather than writing a repro from scratch. **They are green in CI today only because CI's TMPDIR
   has no `--`** — the suite's silence is an artifact of the environment, not evidence.
2. **The three binary reproductions above are gone**, verified through `qfs plan`: the unquoted
   `--` path no longer merges the next statement; the quoted `AT '/data/a--b.db'` no longer raises
   UNTERMINATED_STRING; the trailing `# note; more` no longer raises UNEXPECTED_EOF.
3. **Both loaders have unit tests and agree.** Through `server`'s `statements()` **and** through
   `provision`'s `load()`: `--` in an unquoted path, `--` in a quoted locator, `;` in a quoted
   locator, a trailing `#` comment containing `;`, and a `\'` escape (`AT '/data/o\'brien--x.db'`)
   that must not desynchronise quote tracking. Same input, same answer from both.

Baseline regardless of the above (`CLAUDE.md:22-24`): `cargo test --workspace` (or per-crate —
`qfs-server`, `qfs-core`, `qfs-provision` — if the shared host's disk fills at link),
`cargo clippy --workspace --all-targets -- -D warnings` (**not** `--all-features`), and
`cargo fmt --all --check`. Keep `server/src/tests.rs:674-684` and
`provision/src/tests.rs:301-326` green — the latter is the `#`-tolerance guard the unification most
plausibly breaks. `gen-docs --check` / `gen-skills --check` / `check-migrations` should pass
unchanged; run them to confirm.

## Considerations

- **No plugin version bump.** `CLAUDE.md:40-46` conditions the four-field bump on a change to a CLI
  surface the skills mention. This is an internal parser fix: no verb, flag, or taught statement
  changes, and neither `docs/cookbook/automation.md` nor the `qfs-automation` skill teaches `--`
  comments. This flips only if the fix documents the corrected behaviour in `automation.md` — then
  re-run `gen-skills` and bump all four fields (patch: nothing taught is broken, only added).
- **Nothing can be depending on the current behaviour.** No `.qfs` read by `statements()` contains
  a `--` comment at all — every server/provision fixture (`server_boot.qfs`, `watchtower.qfs`,
  `deploy_boot.qfs`) uses `#` exclusively, and provision's emitter only writes `#`. Behaviour
  changes only for text that is silently corrupted today. The `#` half is a strict widening: today's
  UNEXPECTED_EOF becomes a correct parse.
- **The dropped clause is a policy bound, not a cosmetic loss.** The concern's own example is
  `DO REMOVE /local/a--b/x POLICY p` losing its POLICY clause. A POLICY is the least-privilege bound
  a handler runs under, and the AST records the fail-closed intent explicitly
  (`parser/src/ast.rs:504`: `None = no policy attached (fail-closed…)`). So a silent truncation
  either degrades a handler to fail-closed with no diagnostic, or diverges the operator's written
  intent from the machine's behaviour with no signal. That is why the fix is correct tokenization
  and **not** a "suspicious `--` detected" warning — eliminate the failure mode rather than report
  it (`workaholic:implementation` / `type-driven-design`: a premise expressible in the type must not
  be substituted by a comment or a runtime signal).
- `strip_line_comment(&str) -> &str` is a signature that structurally cannot report the failure it
  causes. Correct tokenization removes the need for a failure channel; the genuine fail-loud surface
  already exists downstream and is right (`LoadError::Parse{line, code, message}`), which is exactly
  why the line attribution must survive the refactor.
- The precedent for the fix's shape is archived ticket
  `20260706120100-connect-declared-registry-followups.md` Part B (commit `11b910f`), which shipped
  the quote-aware handling in `connections.rs`. Read it for the shape; it does not cover
  `runtime.rs`, and its result is the fork this ticket closes.
- Mission acceptance item 5 has been mis-stated twice in two days by paraphrase (first naming the
  wrong parser, then over-crediting `connections.rs` as "correct"). Both corrections are in the
  mission's Changelog. Work from the source and this ticket's cited lines, not from a summary.
