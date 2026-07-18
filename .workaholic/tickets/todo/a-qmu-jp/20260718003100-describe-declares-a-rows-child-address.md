---
created_at: 2026-07-18T00:31:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category:
depends_on:
mission:
---

# `describe` declares a row's child address, so a generic consumer can drill any archetype

## Overview

A generic qfs consumer (the qfs-viewer column UI) drills into a table only when a row **names a child path**. Today that works for a handful of paths and silently fails for most, and the divide is **archetype**. This ticket is the qfs half of a cross-repo pair: make `describe` (or the row shape) declare, per archetype, the address a row selects — so "click a row, open the record" is answerable from what qfs returns, without the consumer inventing addresses per service.

The consumer half is `qfs-viewer` ticket `20260718003000` (a row's primary key becomes the next address — `@選択`). **Neither should ship until the `@選択` spelling is settled with the plan-book owner** (`strategy/docs/plan.md:175`). This ticket does not invent the grammar; it makes qfs able to serve it.

## Settled (2026-07-18 design session — recorded in strategy `plan.md`「番地」)

The design options below are **decided**, by the plan-book owner:

- **Option 1 plus the grammar, together — and both land here.** describe **declares the key columns** per node (the identity that selects a child), and **qfs itself owns the `@` selection segment**: the lexer and the single lowering site (`/x/@A` → `|> where <key> = A`). The synthetic-path-column option is rejected. Spelling: keys only; single key `@A`; composite keys positional in declared key order, `@2024,INV-003`, values percent-encoded (column names live in describe, never in the address). Precedent that the lexer absorbs new segment kinds cleanly: quoted path segments, v0.0.77.
- **enumerate becomes a facet.** `ls` of a table node lists **row addresses** — the read projected to (address, label), same source, default order, and limit as read. The REPL's `ls`, the viewer's column, and a REST listing are the same observation. This is the same missing root plumbing the stopped describe-surface ticket measured (`ls /sys` erroring today) — that work now has its concept instead of a false premise.
- Secondary axes (`thread_id`, sender) are **relation segments**, a later phase with the relation-metadata layer. "No child" stays declarable — not every table is a tree.
- **This ticket goes first in the cross-repo order.** The viewer (qfs-viewer `20260718003000`) is blocked on it and will pass trails through rather than lower them — the plan's one-lowering rule is why qfs must learn `@` before any consumer does.

## Measured (2026-07-18 against `qfs 0.0.77`, structure only — no client values read)

The consumer's containment rule links a row **only when the row carries a `path` column naming a valid child**. Surveyed every live binding by `qfs describe` (archetype + column names only):

| archetype | example paths | row carries a `path` column? | drillable in the consumer? |
| --- | --- | --- | --- |
| `relational_table` | `/sys/paths`, `/markdown/<n>/documents`, `/markdown/<n>/links` | **yes** (`path`, …) | yes |
| `relational_table` | `/chatwork` | **no** — columns are `{value}` | no |
| `append_log` | `/mail`, `/mail/INBOX`, `/hss/mail` | **no** — `/mail/INBOX` rows are `{id, thread_id, date, from, subject, snippet, label_ids, …}` | no |
| `blob_namespace` | `/drive`, `/hss/drive` | **no** — `/drive` rows are `{id, name, mime_type, parents, size, modified_time, md5, is_google_doc}` | no |

**Two findings worth carrying:**

1. It is **not** an archetype the consumer can dispatch on: a `relational_table` (`/chatwork`) fails the same way as `append_log`, because the real predicate is "has a `path` column", which most rows do not. So drilling works for `/sys/*` and `/markdown/*` and essentially nothing else.
2. **A shipped comment is wrong.** The consumer's `containedChild` documents *"blob namespaces answer a `path` per entry"*. Measured, `/drive` rows answer `id` + `name` + `parents`, **no `path`**. Whatever contract that comment assumed, `describe`/the read does not honor it for gdrive today. This is the same "documentation promises what the binary does not deliver" family as the `/cf` catalog 16-vs-15 drop noted in the 2026-07-17 design measurements.

## The question this carries (do not decide it unilaterally)

The row-to-child address differs per archetype, and qfs is the right place to declare it because only qfs knows each driver's identity columns:

- `append_log` (`/mail/INBOX`): `id` → the message address (`/mail/INBOX/<id>` or `/mail/<id>` — **decide and declare which**). `thread_id` is a second legitimate axis (the thread).
- `blob_namespace` (`/drive`): a folder entry (`mime_type` = folder) → descend into it; a file entry → its content/metadata leaf. `parents`/`id` carry the structure; there is no `path` today.
- `relational_table` with a natural key but no `path` (`/chatwork` = `{value}`): may have no child at all, and that is a valid answer — not every table is a tree.

**Design options — pick with the plan-book owner, then implement:**

1. **describe declares it.** Add to a node's `describe` a per-archetype statement of "the column(s) that select a child, and how they compose into a child address". The consumer reads it and builds the link generically. Keeps service knowledge in qfs; keeps the consumer archetype-agnostic (its stated virtue).
2. **The read carries a synthetic `path`/`address` column.** Every drillable row answers a child address column directly. Simpler for the consumer, but bakes a column into read results that is really describe/metadata, and risks colliding with real columns.
3. **The grammar owns it (strategy's `@選択`).** qfs exposes the identity columns via describe; the *spelling* of the selection segment (`@<id>`, composite keys) is strategy's, lowered to a qfs-query prefix per `plan.md:80`. qfs's job is then only to make `/mail/INBOX/<id>`-style addresses **resolvable** — verify they are (some may not exist as addresses yet).

Whichever is chosen, the anti-drift and objective-documentation policies apply: if a comment or doc claims a row answers a `path`, the binary must actually answer it (finding #2 above must be reconciled, not left).

## Policies

- `workaholic:implementation` / `objective-documentation.md` — the `containedChild` comment claiming blob `path` entries is falsified by measurement; a describe contract must not repeat that.
- `workaholic:implementation` / `anti-corruption-structure.md` — keep per-service knowledge (which column selects a child) inside qfs, so the consumer needs no per-service branch.
- `workaholic:design` / `self-explanatory-ui.md` (consumer-facing rationale) — a described row that cannot be drilled must be honestly inert, not a dead link; describe should make "no child" expressible, not only "has a child".

## Key Files

- `packages/qfs/crates/*driver*/` — where each archetype's row shape and (proposed) child-address declaration live
- the `describe` lowering that emits `archetype` + columns (the node-describe path exercised by `qfs describe`)
- `/home/ec2-user/projects/strategy/.worktrees/qfs-viewer/.../Describe.ts` (read-only; other repo) — `containedChild`, the consumer's single rule, and its incorrect blob comment
- `/home/ec2-user/projects/strategy/docs/plan.md` (read-only; other repo) — `@選択` open question (`:60`, `:80`, `:175`)

## Quality Gate

**Acceptance criteria**

- The chosen mechanism is recorded with the plan-book owner before code moves.
- For at least one `append_log` path, `qfs describe` (or the read) lets a generic consumer derive a valid child address from a row, and `qfs describe <that child>` resolves — proven by a test that **watched the missing address fail first**.
- Finding #2 is reconciled: no shipped comment or doc claims a row answers a column the binary does not answer. If gdrive should answer a `path`/child, it does; if not, the false comment is corrected on the consumer side (filed there).
- "No child" is expressible: a table like `/chatwork` `{value}` reports no child address rather than a broken one.
- The gate suite passes with bare exit codes (`fmt`, `clippy -D warnings`, `test --workspace`, `gen-docs --check`, `gen-skills --check`).

**Verification method**

- `qfs describe /mail/INBOX` shows the child-selecting column(s); the derived `/mail/INBOX/<id>` (or agreed spelling) resolves via `qfs describe`/`run` with a bare exit code. No client values in the report.

**Gate**

- The `@選択` spelling and the child-address mechanism are answered by the developer/plan-book owner. This ticket does not commit a grammar the strategy plan has not adopted.

## Considerations

- **Not mail-specific** (developer note, 2026-07-17): the same inertness hits chat and files, so the fix must be the archetype-general contract, not a gmail special case.
- The 2026-07-17 design measurements already flagged the `/cf` catalog 16-vs-15 silent drop as the same "docs promise, binary under-delivers" family; the blob `path` comment (finding #2) is a second instance. Worth a scan for others while here — but do not expand scope to fix them unasked; report.
- This ticket was filed onto the `work-20260717-194500` bugfix branch (the only qfs desk) as a **separate commit** so its concern does not entangle the REPL/span fixes. It changes no shipped code.
