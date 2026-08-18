---
created_at: 2026-08-16T16:11:43+00:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
mission:
merge_policy: review
verification_handoff:
---

# The server guide's config example does not parse: missing `;` and a `do` the parser rejects

## Overview

`docs/server.md`'s worked config, copied verbatim into a file and handed to `qfs serve`, fails to
boot. Two independent defects, both hit while standing up an endpoint on 2026-08-16:

1. **Statements need a trailing `;`.** Two bindings on separate lines are rejected; each parses
   alone. Adding `;` to both makes the identical file boot.

   ```
   $ printf '%s\n%s\n' "create policy readsessions allow select on claude" \
       "create endpoint sessions on 'GET /claude/sessions' policy readsessions as /hosts/local/claude/sessions" > t.qfs
   $ qfs serve t.qfs
   qfs serve: config parse error at line 1: a reserved keyword cannot be used here [RESERVED_AS_IDENTIFIER]

   $ printf '%s;\n%s;\n' … > t.qfs      # same two statements, semicolons added
   $ qfs serve t.qfs                     # boots
   ```

   The guide's fenced example shows no semicolons, so it is a copy-paste failure for every reader.

2. **The bindings table says `endpoint <name> do <stmt>`; the parser wants `as`.**
   `create endpoint live do /hosts/local/claude/sessions` is a parse error, while
   `create endpoint live on 'GET /live' as /hosts/local/claude/sessions` boots. The guide's own
   prose example already uses the `on … as …` form, so the table and the example disagree with each
   other as well as with the binary.

3. **The error's line number is wrong**, which is what made this cost time: the failure is reported
   at `line 1` when line 1 parses in isolation and the offending construct is the file's second
   statement.

`docs/server.md` is generated (`cargo xtask gen-docs`), so the fix belongs in the generator's source,
not in the rendered file.

## Policies

- `workaholic:implementation` / `policies/coding-standards.md` — all code work.
- `workaholic:implementation` / `policies/directory-structure.md` — all code work.
- `workaholic:implementation` / `objective-documentation` — a documented example that does not run is
  worse than no example: it teaches a form the binary rejects. The repository already holds the
  pattern that prevents this (`crates/test/tests/cookbook_skills.rs` parse-checks every cookbook
  recipe); the server guide has no equivalent ratchet.

## Key Files

- `docs/server.md` — the rendered guide (never hand-edit; regenerate).
- `packages/qfs/xtask` — `gen-docs`, which renders the guide from the server binding forms.
- `packages/qfs/crates/test/tests/cookbook_skills.rs` — the existing verified-true ratchet to model
  the new one on.
- The config parser reached by `qfs serve` — for the line-number attribution.

## Implementation Steps

1. Reproduce all three: the semicolon-less two-statement file, the `do` form, and the misreported
   line number.
2. Fix the generated guide at its source — semicolons in every config example, and the bindings
   table's shape column reading `endpoint <name> on '<METHOD /route>' [policy <name>] as <stmt>`.
3. Correct the parse error's line attribution to the statement that actually failed.
4. Add a ratchet in the shape of `cookbook_skills.rs`: every `.qfs` config block in `docs/server.md`
   must parse. A documented config that does not boot then cannot ship again.
5. Regenerate: `cargo run -p xtask -- gen-docs`.

## Quality Gate

**Acceptance criteria**

- Every config example in `docs/server.md` parses.
- The bindings table's `endpoint` shape matches what the parser accepts.
- A two-statement config reports its parse error against the failing statement's line.

**Verification method**

- The new ratchet test is green and fails when a `;` is removed from a documented example.
- `cargo run -p xtask -- gen-docs --check` clean.
- Manual: the guide's example config, copied verbatim, boots under `qfs serve`.

**Gate**

- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all --check`, `gen-docs --check` all green.

## Considerations

- Check whether other generated guides (`docs/language.md`, `docs/drivers.md`) carry examples with
  the same untested status; the ratchet may be worth widening rather than scoping to one page
  (`packages/qfs/crates/test/tests/cookbook_skills.rs`).
