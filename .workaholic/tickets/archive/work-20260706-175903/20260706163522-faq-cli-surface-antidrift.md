---
created_at: 2026-07-06T16:35:22+09:00
author: a@qmu.jp
type: enhancement
layer: [Infrastructure]
effort:
commit_hash: 67e9743
category: Added
depends_on: [20260706163521-qfs-faq-reference-skill.md]
---

# Keep the FAQ's CLI answers true as qfs changes (CLI-surface anti-drift check)

## Overview

The FAQ skill (`20260706163521-qfs-faq-reference-skill.md`) answers most operator questions with
**shell commands** — `qfs connect --driver … --account …`, `qfs account add google`,
`qfs app add google` — because that is how connections are actually made. But the existing
verified-true ratchet (`cookbook_skills.rs`) only parse-checks ```` ```qfs ```` *statement* recipes;
it does **not** see shell commands. So the single most important part of the FAQ — the connection
setup answers — has **no machine guarantee** of staying true when the CLI surface changes (a renamed
or removed flag would leave the FAQ silently wrong).

This ticket closes that gap so the FAQ is *"updated when qfs changes"* in the strong, machine-checked
sense the request asked for: add a check that asserts **every `qfs <command> [--flags]` the FAQ cites
exists in the binary's actual clap surface**, failing CI until the FAQ is brought back in line.

This is the same anti-drift philosophy already in the tree — `gen-docs --check`, `gen-skills --check`,
`check-migrations`, and the `cookbook_skills.rs` recipe ratchet — extended to the FAQ's shell surface.

**Depends on** `20260706163521-qfs-faq-reference-skill.md` (the article must exist to check against).

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — a new test lives beside
  `cookbook_skills.rs` in `crates/test/tests/` (or a new `xtask` check beside the others); same role,
  same place (applies to all code work).
- `workaholic:implementation` / `policies/coding-standards.md` — Rust; prefer structured clap
  reflection over regex-scraping help text so the compiler/типed API catches breakage (applies to all
  code work).
- `workaholic:implementation` / `policies/test.md` — this is a regression test that writes down actual
  behavior (the FAQ's commands exist) and tests against the real binary surface, not a mock.
- `workaholic:implementation` / `policies/objective-documentation.md` — enforces that the FAQ's
  claims about the CLI remain verifiable against the code, machine-checked rather than asserted.
- `workaholic:operation` / `policies/ci-cd.md` — the check runs as part of the single inspection
  command family, identically locally and in CI; a green result is backed by evidence of what was
  verified.
- `workaholic:implementation` / `policies/command-scripts.md` — expose it through the existing
  runnable verbs (`cargo test` and/or a `cargo run -p xtask -- <check>`) so CI invokes the same command
  a developer runs.

## Key Files

- `packages/qfs/crates/test/tests/cookbook_skills.rs` — the sibling ratchet and the model to follow
  (glob `docs/cookbook/*.md`, extract fenced blocks, assert against the binary).
- `docs/cookbook/faq.md` — the article whose ```` ```sh ```` `qfs …` commands are the check's input
  (created by the dependency ticket).
- `packages/qfs/crates/cmd/src/lib.rs` — the clap `Command` tree that is the source of truth for which
  subcommands/flags exist (introspect this, or invoke `qfs <cmd> --help` and match).
- `packages/qfs/xtask/src/main.rs` + `packages/qfs/xtask/src/gen_skills.rs` — if implemented as an
  `xtask` `--check` verb instead of a test, this is where it registers (mirrors `gen-skills --check`).
- `CLAUDE.md` `## Build & test` — add the new check to the documented anti-drift command list if it is
  an `xtask` verb (keep the list authoritative).

## Related History

- [20260701173124-cookbook-articles-as-agent-skills.md](.workaholic/tickets/archive/work-20260629-110121/20260701173124-cookbook-articles-as-agent-skills.md) — established the ratchet pattern this test extends.
- [20260629111140-fix-skill-md-steers-ai-into-errors.md](.workaholic/tickets/archive/work-20260629-110121/20260629111140-fix-skill-md-steers-ai-into-errors.md) — the failure mode (a skill teaching commands the binary rejects) this check is designed to prevent for the CLI surface.

## Implementation Steps

1. **Choose the surface extractor.** Parse `docs/cookbook/faq.md`, collect every ```` ```sh ```` fence,
   and pull out each `qfs …` invocation (first token `qfs`, then subcommand path + `--flags`).
2. **Assert against the binary's clap tree.** For each extracted `qfs <cmd…> [--flag…]`, verify the
   subcommand path and every long flag exist in the real clap `Command` — prefer clap reflection
   (walk `Command::get_subcommands()` / `get_arguments()`) over scraping `--help` text (less brittle,
   compiler-typed). Ignore argument *values* and placeholders.
3. **Fail loudly on drift.** An unknown subcommand or flag fails the check with a message naming the
   FAQ line and the offending token, so a `connect`/`account` flag rename breaks CI until the FAQ is
   updated.
4. **Wire into the standard command family.** Implement as a test in `crates/test/tests/`
   (runs under `cargo test --workspace`) *or* as `cargo run -p xtask -- <check>` beside the other
   `--check` gates; if the latter, document it in `CLAUDE.md` `## Build & test`.
5. **Guard against vacuous pass.** Enforce a floor on the number of `qfs` commands extracted (mirror
   `MIN_STATEMENTS` in `cookbook_skills.rs`) so a broken extractor can't pass by finding nothing.

## Quality Gate

**Acceptance criteria:**

- Every `qfs <command> [--flags]` shown in `docs/cookbook/faq.md` is asserted to exist in the binary's
  clap surface; the check extracts a non-zero, floored count of commands (no vacuous pass).
- Renaming or removing a flag the FAQ cites makes the check **fail** (demonstrated by a temporary
  local edit to a flag name, reverted after).
- On the shipped FAQ, the check is **green**.
- The check runs from the same command a developer and CI both use.

**Verification method:**

- New check green: `cargo test --workspace` (if a test) or `cargo run -p xtask -- <check>` (if a verb).
- Negative proof: temporarily rename `--account` → `--acct` in a `docs/cookbook/faq.md` `sh` block (or
  in the clap def) and confirm the check goes red; revert.
- `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`.

**Gate:** the new check is green on the true FAQ, demonstrably red on an injected drift, and part of
the standard local+CI command family — all confirmed before approval.

## Considerations

- **Prefer clap reflection over `--help` regex** — help text is prose and reflows; the typed
  `Command` tree is stable and compiler-visible (`packages/qfs/crates/cmd/src/lib.rs`).
- **Scope to `qfs` shell invocations only** — the in-language `CONNECT`/`CREATE ACCOUNT` statements are
  already covered by `cookbook_skills.rs`; don't double-check them.
- **Test vs xtask verb is a real fork** — a `cargo test` sibling is lower-ceremony and runs in the
  existing `cargo test --workspace`; an `xtask --check` verb matches the `gen-*`/`check-migrations`
  family and shows up in `CLAUDE.md`. Recommend the test-crate approach (closest to the existing
  `cookbook_skills.rs` ratchet) unless the FAQ needs help-text rendering later.
- **This ticket is optional-but-recommended.** If dropped, the FAQ still ships (dependency ticket) and
  stays true for its ```` ```qfs ```` recipes + via version-bump discipline; only the shell-command
  freshness guarantee is forgone. Splitting it out lets that trade-off be an explicit decision.
