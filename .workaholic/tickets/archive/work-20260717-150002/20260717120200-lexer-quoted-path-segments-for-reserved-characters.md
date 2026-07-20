---
created_at: 2026-07-17T12:02:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Added
depends_on:
mission:
---

# Lexer: quoted path segments so names with `?` and spaces are addressable

## Overview

Observed live (2026-07-17, same session as ticket 20260717102000): a Drive file whose name
contains an ASCII `?` or a space cannot be addressed as a single-file path — the statement dies
in the lexer with `lexing failed: UNEXPECTED_CHAR` before parsing begins. This is what pushed
the operator into the `remove <folder> where name == '…'` detour that then hit the over-delete
bug: the safer, more precise single-file spelling was unwritable.

Real-world Drive (and mail, and local FS) names routinely contain spaces, `?`, `#`, `&`,
parentheses, etc. The path grammar needs an escape hatch that keeps the frozen grammar's
one-token path shape while letting any segment carry reserved characters.

## Expected behavior

A **quoted path segment** form, e.g.:

```
/drive/my/Reports/'Q3 budget (final)?.xlsx'
```

- Inside quotes, any character except the quote itself is literal (with a doubling or backslash
  escape for the quote), including spaces, `?`, `#`, and Unicode.
- Unquoted segments keep today's lexing exactly (no breaking change).
- Works in every path position: read paths, effect targets, `id:` coordinates excluded (ids are
  already safe), and `describe`.
- The rendered/canonical form re-quotes segments that need it, so plans and previews round-trip.

## Key Files

- `packages/qfs/crates/lang/src/lex.rs` — the token that rejects `?`/space today.
- `packages/qfs/crates/parser/src/grammar.rs` — path segment assembly.
- Driver path parsers that re-split rendered paths (e.g.
  `packages/qfs/crates/driver-gdrive/src/path.rs`) must agree on the quoting rule.

## Policies

- `workaholic:design` — the language must reach what users and agents can see; an unaddressable
  visible file forces unsafe detours (this incident's root motivation).
- `workaholic:implementation` / frozen-grammar discipline — an additive token form, no change to
  existing statements' meaning.

## Quality Gate

1. Lexer/parser tests: quoted segments with spaces, `?`, quotes-in-quotes, and Unicode lex and
   parse; unquoted behavior is bit-identical to today (golden corpus unchanged).
2. An end-to-end driver test addresses a mock Drive file named with `?` and spaces through a
   single-file path (read and REMOVE).
3. gen-docs reflects the quoted-segment form in the language reference; cookbook articles show
   the recipe; gen-skills ratchet passes.
4. `cargo test --workspace`, clippy `-D warnings`, fmt all pass.
