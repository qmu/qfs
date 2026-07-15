---
created_at: 2026-07-11T12:15:36+09:00
author: a@qmu.jp
type: enhancement
layer: [Infrastructure, Config]
effort:
commit_hash:
category:
depends_on:
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
---

# Assure no command-execution risk: audit, tests, and a governance lock on process spawning

## Overview

Turn "assure no command execution risk" from an audit claim into an **enforced invariant**, the
same move the one-seam lock made for model calls. Discovery confirms the current footprint is
small and shell-free: `Command::new("git")` in `driver-git/applier.rs` (commit application) and
`qfs/src/git.rs` + `migration_guard.rs` (repo introspection), a desktop opener in `tty.rs`
(Stdio::null), **no `sh -c`, no shell string interpolation, no user-input-derived program name
anywhere**. This ticket (a) documents that inventory as a blueprint security section, (b) adds
argument-hygiene tests for the git sites (path/ref arguments from query text must be passed as
argv elements, never shell-joined; `--`-separation where git supports it), and (c) lands a
**governance lock**: a workspace test that enumerates every `std::process` usage site and fails
when a new one appears or an existing one changes shape — so a future driver, transform output,
or declared-driver evaluation can never quietly grow an exec path from query text or fetched data.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout (applies to all code work)
- `workaholic:implementation` / `policies/coding-standards.md` — style conventions (applies to all code work)
- `workaholic:design` / `policies/defense-in-depth.md` — the lock is an independent boundary: even if an upstream gate is bypassed, no path from data to process execution exists
- `workaholic:safety` / `policies/standard.md` — the security-standard record of the audit and its enforcement
- `workaholic:design` / `policies/access-control.md` — the git subprocess acts only within the repo the path grants; no argument can widen it

## Key Files

- `packages/qfs/crates/driver-git/src/applier.rs` - Command::new(git) commit application (piped stdio)
- `packages/qfs/crates/qfs/src/git.rs` - Command::new(git) repo introspection; plus migration_guard.rs
- `packages/qfs/crates/qfs/src/tty.rs` - desktop opener (Stdio::null)
- `packages/qfs/crates/cmd/tests/dep_direction.rs` - existing workspace-shape governance test to model the lock on
- `docs/blueprint.md` - security/assurance section the inventory lands in

## Related History

No dedicated prior ticket; the one-seam lock is the pattern precedent.

- [20260709104300-transform-one-seam-lock.md](.workaholic/tickets/archive/work-20260709-023822/20260709104300-transform-one-seam-lock.md) - prose invariant → enforced lock (the move this ticket repeats for exec)

## Implementation Steps

1. Inventory test (the lock): a test that scans the workspace source (or uses cargo-metadata + a grep harness like the existing dep-direction test) for `std::process`/`Command::new` and asserts the exact allowlisted set of (crate, file, spawned program) triples; any addition/change fails with a message demanding a deliberate allowlist edit + review.
2. Argument-hygiene hardening: audit the git sites for query-derived arguments (paths, refs, messages); enforce argv-element passing with `--` separators where applicable; add tests feeding hostile values (`--upload-pack=…`, `-c core.sshCommand=…` class injections via ref/path/remote strings) and asserting they are neutralized or rejected.
3. Data-path assertion: tests that transform output, declared-driver rows, and codec-decoded content cannot reach any spawn site (type-level argument: the allowlisted sites take no such inputs — document the reasoning in the blueprint section).
4. Blueprint security section: the inventory, the lock, the hygiene rules, and the standing rule that new spawn sites require a blueprint edit in the same PR.

## Quality Gate

**Acceptance criteria**

- The exec-inventory lock test enumerates exactly today's allowlisted spawn sites and fails on any new/changed site.
- Hostile-argument tests for the git sites pass (injection-shaped refs/paths rejected or inert).
- The blueprint carries the exec-risk section; no `sh -c`/shell-join exists anywhere (asserted by the lock).

**Verification method**

- `cargo test --workspace` green including the new lock and hygiene tests; `gen-docs --check` clean if generated docs reference the section.

**Gate**

- Hermetic suite green + blueprint section landed is the /drive approval gate; no live round needed (local-only assurance).

## Considerations

- The lock must not false-positive on test code spawning cargo/git in the test harness — scope the allowlist by crate and file (`packages/qfs/crates/cmd/tests/dep_direction.rs` pattern)
- git's option-injection surface via refs (`--upload-pack`) is the realistic vector — the `--` separator and ref validation are the concrete fixes (`packages/qfs/crates/driver-git/src/applier.rs`)

## Resolution (2026-07-14, v0.0.62 → v0.0.63)

**Already fully landed — this pass verified it and closed the mission acceptance box.** The
implementation shipped earlier in commit `6b4b29f` ("Lock command-execution risk to an enforced
invariant"), but the ticket was archived without a Resolution and the mission line was never ticked;
the post-v0.0.60 resume checkpoint carried it forward as an open desk task. Re-verified every
acceptance criterion against HEAD:

- **Inventory lock** — `crates/cmd/tests/exec_inventory.rs` enumerates the exact
  `(file, spawned-program)` allowlist (git ×2 in `driver-git/applier.rs`, git ×2 in `qfs/git.rs`,
  git ×2 in `migration_guard.rs`, `OPENER` in `tty.rs`, build-only `cmd`/`program` in `xtask`) and
  fails on any new/changed/multiplied `Command::new` site. **`every_process_spawn_site_is_on_the_allowlist` PASSES.**
- **No shell** — `no_shell_string_execution_anywhere_in_production_source` forbids
  `sh -c`/`bash -c`/`Command::new("sh"|"bash"|"cmd"|"powershell")` in production source. **PASSES.**
- **Argument hygiene** — `driver-git`'s `applier::hygiene_tests` pins the two structural defenses:
  ref names route through `qualify_ref` (leading-`-` can never present as a git flag), oids route
  through `Oid::parse` (40-hex only). **`qualify_ref_neutralizes_option_injection_in_branch_names`,
  `qualify_ref_passes_through_qualified_refs_and_head_verbatim`,
  `oid_parse_rejects_flag_shaped_and_non_hex_strings` all PASS** (feeding `--upload-pack=…` /
  `-c core.sshCommand=…` class values).
- **Data-path argument + blueprint section** — blueprint **§17 "Command-execution assurance"**
  documents the inventory table, the no-shell lock, the git argument-hygiene defenses, the
  type-level data-path argument (transform output / declared-driver rows / codec content reach no
  spawn site), and the standing rule (a new spawn site requires a same-PR blueprint + allowlist +
  hygiene edit).

Verified green this pass: `cargo test -p qfs-cmd --test exec_inventory` (2/2),
`cargo test -p qfs-driver-git --lib` (39/39, incl. the 3 hygiene tests). Closes mission acceptance
line "Command-execution-risk assurance recorded".
