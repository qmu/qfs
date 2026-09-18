---
created_at: 2026-09-19T00:15:00+09:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
feedback:
merge_policy:
verification_handoff: 
---

# Install a shipped declaration from the binary that carries it

## Overview

The binary embeds four declared-driver programs and gives an operator no way to install one from
it. `/sys/declarations` will tell you your `/slack` is stale and name every statement that differs
— and then the only way to act on that answer is to fetch the asset from GitHub by hand and paste
its statements one at a time. A capability the artifact carries and cannot reach.

Measured on 2026-09-18, as the cost rather than the theory: an operator on a current binary
(v0.0.137, shipping the scoped Slack file-content read since v0.0.136) had a Slack mount that could
list attachments and not read one, because the installed declaration predated the read. Config and
code move independently — upgrading the binary never updates a declaration — so this is a permanent
gap, not a one-off, and every service qfs declares has it.

## Policies

- `workaholic:design` — information and function must be reachable through the paths people
  actually use; a shipped capability with no install path is unreachable by construction
- `workaholic:implementation` / `policies/coding-standards.md` — style and structure conventions

## Key Files

- `packages/qfs/crates/cmd/src/lib.rs` — the clap surface and the injected-launcher composition.
  `SkillProvider` is the precedent: the binary owns the `qfs-skill` edge, qfs-cmd only routes.
- `packages/qfs/crates/qfs/src/main.rs` — the composition root that passes each provider.
- `packages/qfs/crates/skill/src/lib.rs` — `DECLARED_DRIVERS`, the shipped set, already carrying
  the label/source pair a re-install needs.
- `packages/qfs/crates/core/src/ddl/document.rs` — `split_document`, the one shipped splitter
  (the reconcile loop and the shell boot both use it; a private chunker would be a second answer).
- `packages/qfs/crates/qfs/src/declaration_currency.rs` — the existing `/sys/declarations` read
  this command is the missing other half of.
- `docs/cookbook/faq.md` §"Is my declared driver still the shipped one?" — the answer that
  currently ends without an action.

## Implementation Steps

1. Add `qfs declare [<driver>] [--commit]`: no argument lists what the binary ships (driver, asset,
   statement count); a driver splits that declaration and runs each statement through the same
   one-shot path `qfs run` uses, PREVIEW by default.
2. Inject the shipped set as a `DeclarationProvider`, exactly as the skill is injected — qfs-cmd
   must not grow a dependency on `qfs-skill`.
3. Derive the driver name from the declaration's own `CREATE DRIVER` statement; never carry it
   beside the asset (a second claim about one fact drifts the first time an asset is renamed).
4. Stop at the first failing statement and return its exit code.
5. Document it in the FAQ beside the currency check, and in the Slack cookbook where the manual
   re-install is currently prescribed. Regenerate the skills.
6. Bump the patch and all four plugin versions (the taught surface grows).

## Quality Gate

**Acceptance criteria**

- `qfs declare` lists the four shipped declarations with their statement counts and writes nothing.
- `qfs declare <driver>` lists the statements; `--commit` installs them and reports the count.
- A driver the binary does not ship is a usage refusal (exit 2) naming what it does ship.
- Re-running an install is safe, and a declaration carrying local extras is explained rather than
  reported as a failed install.
- The listing survives being piped into `head` (no broken-pipe panic).

**Verification method**

- `cd packages/qfs && cargo test --workspace`
- `cd packages/qfs && cargo clippy --workspace --all-targets -- -D warnings`
- `cd packages/qfs && cargo fmt --all --check`
- `cargo run -p xtask -- gen-skills --check`, `python3 scripts/check-plugin.py`
- Live: install all four shipped declarations on this host and re-read `/sys/declarations`.

## Considerations

- **Why not `qfs apply`.** `apply` converges the whole configuration to a document, so pointing it
  at one driver's asset proposes removing every other driver's rows — measured: 52 effects, mostly
  REMOVE. A declaration install is additive by nature and needs its own verb.
- **Never removes.** `declare` adds and replaces. Nodes the binary does not ship are left alone,
  so a declaration with local extras still reads `stale` afterwards; the command says so rather
  than implying the install failed. Removing a node stays an explicit `REMOVE VIEW|MAP|TYPE`.
- **The listing is a manifest, not a dump.** Each line stops at the `AS` seam: the node a statement
  declares, never its body.
