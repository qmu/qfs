---
created_at: 2026-07-25T14:31:00+09:00
author: a@qmu.jp
assignees: [a@qmu.jp]
type: bugfix
layer: [Domain]
effort:
commit_hash:
category:
depends_on:
mission:
---

# The FAQ under-describes exit 2 — the most likely error this mission produces

## Overview

`docs/cookbook/faq.md` is the operator's troubleshooting surface (and the `qfs-faq` Agent Skill an
agent consults when a command fails). Its exit-code table describes `2` as:

> `2` | parse or CLI usage error (a relative path, a bad flag)

That is now incomplete in the one place it matters most. This mission added two new **structured
refusals**, and both are `ErrorKind::Usage` → **exit 2**:

- `where` on a column the relation does not carry → `unknown_column`
- `expand` on an absent, scalar, or `Json` column → `not_expandable`

Neither is a parse error and neither is a CLI usage error, so an operator (or an agent) who hits
the most likely new failure on this branch reads the table and concludes their **flags** are wrong.
The "common errors" table immediately above the exit codes lists neither row, so there is no entry
anywhere that says what to actually do — which is: run `qfs describe <path>` and use a real column.

## Scope

**In scope:**

- Broaden the exit-2 row so it names the class honestly (a malformed **question** — parse, CLI
  usage, or a stage naming something the relation does not have), keeping it one line.
- Add an `unknown_column` row and a `not_expandable` row to the common-errors table above, each
  with the "You see / What it means / Fix" shape the table already uses. The fix for both is
  `qfs describe <path>` — and for `unknown_column` specifically, the point worth stating: this is
  **not** "nothing matched"; a zero-row answer and a typo are now distinguishable.
- Re-run `cargo run -p xtask -- gen-skills` so `qfs-faq/SKILL.md` regenerates. **Never hand-edit a
  SKILL.md.**

**Out of scope:**

- Changing any error's `kind` or exit code. Both refusals are deliberately `Usage`/2.
- The other cookbook articles.
- Re-litigating whether `select` should refuse too (its own open ticket,
  `20260725113000-select-on-an-unknown-column-is-silently-dropped.md`) — do not document a refusal
  that does not exist.

## Key Files

- `docs/cookbook/faq.md` — the common-errors table (~200) and the exit-code table (~208).
- `packages/qfs/crates/exec/src/error.rs` — `ErrorKind::Usage` and the exit-code mapping, so the
  documented codes are read off the binary rather than remembered.
- `packages/qfs/crates/engine/src/lib.rs` — `apply_where` / `check_where_columns`, which produce the
  `unknown_column` refusal, and the split between a driver's residual and a caller's `WHERE`.
- `plugins/qfs/skills/qfs-faq/SKILL.md` — generated output; regenerate, never edit.

## Policies

- `workaholic:design` — 「推測するな、宣言して拒否せよ」. The refusal is the right behaviour; a
  troubleshooting page that mislabels it sends the operator to fix the wrong thing.
- `workaholic:implementation` / `objective-documentation` — the doc and the binary must agree.

## Quality Gate

1. Both new rows quote the **actual** error text and code produced by a real run, pasted into the
   ticket outcome with `echo "EXIT=$?"` shown — not paraphrased.
2. `cargo run -p xtask -- gen-skills --check` green after regenerating.
3. The plugin version is bumped if this changes the taught surface of `qfs-faq` (it does — all four
   plugin `version` fields, patch or minor per CLAUDE.md).
4. Workspace gates green with raw exit codes.

## Considerations

- Minted by the `/monitor` drive of this mission (run 20260725-101714). The refusals are this
  mission's own product, so under-describing them is this mission's own debt.
- Small and mechanical, but it is the surface an agent reads when a query fails — the point at which
  a correct refusal either teaches or confuses.

## Queue provenance — the `mission:` stamp was cleared on 2026-08-12

This ticket was minted under the mission **`a-where-predicate-is-honored-or-refused-never-dropped`**, which closed `achieved` while the ticket
itself stayed unfinished. `plan-units.sh` excludes any mission-stamped ticket from the developer's
backlog **without checking whether that mission is still active** (`plan-units.sh:432` — a non-empty
mission relation is excluded as `mission_member`), and only *active* missions are offered as mission
units. A ticket stamped with a closed mission is therefore reachable by neither path, and this one
had been invisible to every `/drive` survey since the close.

The stamp is cleared so the ticket returns to the ordinary backlog — the same correction
`20260804173000` received when its own mission closed. The provenance lives here in prose instead.

**Still-open evidence (verified 2026-08-12, read-only):** Still open: `docs/cookbook/faq.md` advertises "exit codes" in its skill description, but its body carries no exit-code section (headings: shape of an answer, connection setup, access blocked, the safety loop, common errors, skill routing).
