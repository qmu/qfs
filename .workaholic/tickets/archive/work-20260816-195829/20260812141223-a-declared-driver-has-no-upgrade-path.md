---
created_at: 2026-08-12T14:12:23+09:00
status: done
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
mission:
merge_policy: review
claim: work-20260816-195829
---

# A declared driver has no upgrade path, so a shipped declaration fix never reaches a live mount

## Overview

Minted from the open concern `a-declared-driver-has-no-upgrade` (feedback
`20260804205042-a-declared-driver-has-no-upgrade.md`, severity moderate, raised out of PR #32 and
never turned into work until now).

A **declared driver** is an external service defined as qfs statements rather than compiled Rust
(`CREATE DRIVER` / `TYPE` / `VIEW` / `MAP` / `LOOKUP`). The project ships several as assets
(`crates/skill/assets/examples/*.qfs`, surfaced as `qfs_skill::CHATWORK_DRIVER`,
`CLOUDFLARE_DRIVER`, `GITHUB_ACCOUNT_DRIVER`, `SLACK_DRIVER`). An operator installs one by previewing
and committing its statements, which desugar to rows in the System DB table `sys_drivers`. From then
on **the installed rows are the driver** — the shipped file is never consulted again.

That is the gap. When a shipped declaration is corrected, the operator who already installed it keeps
running the old rows and has no way to find out. Two real cases have already occurred:

- The Chatwork messages view was returning unread-only, so a second read of the same room came back
  empty; the fix added `force=1` to the shipped asset (PR #32).
- The Slack twin's channel lookup was rewritten to a shared `CREATE LOOKUP` (PR #36).

Re-installing does work — `assemble` in `crates/qfs/src/declared_driver.rs` resolves **newest row per
`(kind, name, verb)`**, so a re-install supersedes. But nothing tells an operator that a re-install is
needed, and nothing detects that their installed declaration is older than the shipped one. Note the
one thing re-installing does *not* do: a statement REMOVED from the shipped script leaves its old row
behind, because superseding is per-key and there is no key to supersede.

## Scope

Give an installed declaration an identity, and give the operator a way to ask whether it is current.

**In scope.**

1. **Stamp the installed declaration.** Record, per driver, the identity of the declaration that was
   installed — a content hash of the script text is the cheapest honest answer (a hand-authored
   `VERSION` clause is a second option and is weaker: it is a claim the author maintains, whereas a
   hash is a fact). Where it lives — a new `sys_drivers` row kind, or a column — is part of the
   ruling; a new **row kind** needs no schema migration, which is how `CREATE LOOKUP` landed.
2. **Answer the question.** An operator must be able to ask "is my `/chatwork` running the current
   shipped declaration?" and get one of: current / older than shipped / not a shipped declaration at
   all (a locally authored one, which must NOT be reported as stale). Reuse an existing surface —
   `DESCRIBE` on the mount, or the driver-catalog read — rather than adding a verb, unless the
   ruling finds a reason.
3. **Rule whether upgrade is automatic or operator-initiated, and record the reason.** Automatic
   re-install on mismatch is a silent write to the operator's System DB; operator-initiated leaves a
   known-stale mount running. This is the decision the ticket exists to force — it is not obvious,
   and it must be recorded in the blueprint beside §13, with the rejected option and why.

**Out of scope.** Removing stale rows for statements that vanished from a shipped script (state
separately if the ruling needs it); any network fetch of a declaration — the comparison is against
the binary's own embedded assets.

## Key Files

- `packages/qfs/crates/qfs/src/declared_driver.rs` — `assemble` (newest-row-per-key resolution),
  `load_declared_drivers`, the in-memory model every mount is built from.
- `packages/qfs/crates/parser/src/grammar.rs` — the `CREATE …` desugars that write `/sys/drivers`
  rows (`driver_row_values`, `insert_sys_drivers`); a new row kind is added here.
- `packages/qfs/crates/skill/src/lib.rs` — the embedded shipped declarations the comparison is made
  against.
- `packages/qfs/crates/qfs/src/describe.rs` — the two-source registry and the natural home for
  reporting staleness (it already reports shadowed declarations rather than hiding them).
- `packages/qfs/crates/store/src/lib.rs` — System DB migrations, if the ruling needs a column rather
  than a row kind.

## Implementation Steps

1. Rule (1)–(3) above and record the ruling in `docs/blueprint.md` beside §13, rejected options
   included.
2. Stamp the installed identity at install time, in the desugar, so it is written by the same
   previewed-and-committed statement flow as everything else — never a side-channel write.
3. Compute the shipped identity from the embedded asset and compare on the answering surface.
4. Cover with hermetic tests: a freshly installed declaration reports current; an installed
   declaration whose shipped text then changes reports stale; a locally authored declaration reports
   "not shipped" rather than stale; a re-install returns it to current.
5. Teach it where an operator will look — `docs/cookbook/faq.md` (troubleshooting) and the relevant
   per-service article, then regenerate skills.

## Policies

- `workaholic:implementation` / observability — an installed declaration that silently differs from
  the shipped one is state the operator cannot see. The fix is to make it answerable, not to guess.
- `workaholic:design` / 「推測するな、宣言して拒否せよ」 — a locally authored declaration must be
  reported as "not a shipped declaration", never as stale; the two are different facts.
- Blueprint §13 — a declared driver is ordinary previewed-and-committed local state; the identity
  stamp rides that same path.

## Quality Gate

1. **Acceptance:** an operator can ask one question and learn whether a given declared mount runs the
   current shipped declaration, with the three outcomes above distinguished.
2. **Acceptance:** the upgrade ruling (automatic vs operator-initiated) is recorded in the blueprint
   with its rejected alternative.
3. **Verification:** hermetic tests covering fresh install / drifted shipped text / locally authored
   / re-install, with no network.
4. **Gate:** `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo fmt --all --check`, `gen-docs --check`, `gen-skills --check`, `check-migrations` all exit 0.

## Considerations

- The comparison must be **content-based**, not timestamp-based: an operator's clock and the release
  date are unrelated facts.
- If a hash is chosen, decide what it is taken over — the raw file bytes are simplest but make a
  comment-only edit look like a change. Normalising to the parsed statement list is more honest and
  costs a parse; the install path already parses every statement, so the cost is already paid.
- This generalises past Chatwork and Slack: every declared driver the project ships will hit it the
  first time it needs a fix, and the declared shape is meant to be *the normal way* to add a service.

## Final Report

Development completed as planned. The three rulings the ticket exists to force are settled and
recorded in `docs/blueprint.md` §13.4, and the answering surface ships as `/sys/declarations`.

**Resolved: the open question PR #59's survey left.** That survey settled two of the three rulings
from the evidence (identity is derived, not stamped; upgrade is operator-initiated) and left the
third — *which existing surface answers* — open, having read three candidates and found each
blocked: `DescribeReport` carries no free-text slot and widening it ripples through every driver's
output and the generated goldens; `driver_catalog` is deliberately built from the **compiled**
registry only, so `gen-docs` stays a pure function of the binary; `/sys/drivers` is a real table
whose columns are stored row data. Its recommendation was a CLI-level fact line beside `describe`,
outside the core report. **That was resolved against, and the reason is the product's own thesis**:
a fact reachable only from one CLI subcommand is not queryable, cannot be filtered, and cannot be
joined. `SysNode` is documented as a closed set where "a new admin view adds a variant here, never a
side-channel API (the one-engine constraint)", and `/sys/whoami` is the standing precedent for a
`/sys` node whose rows are *derived* rather than read from the System DB. So the answer is a path,
and `/sys/declarations |> where status == 'stale'` is an ordinary query. That also required no
change to `DescribeReport`, no change to the driver catalog, and no migration.

**What was built.**

- `crates/qfs/src/declaration_currency.rs` — the derivation. Both sides reduce to the same
  `(key, canonical value)` map, one entry per `CREATE DRIVER`/`TYPE`/`VIEW`/`MAP`/`LOOKUP`; the
  installed side goes through the same newest-row-per-`(kind, name, verb)` resolution the live
  registry applies, and the shipped side is the embedded asset parsed through the **production**
  splitter (`qfs_core::ddl::document::split_document`) and grammar.
- `SysNode::Declarations` (`/sys/declarations`), SELECT-only, columns `driver` / `status` /
  `shipped` / `differs`, scanned in `crates/qfs/src/sys.rs`.
- `qfs_skill::DECLARED_DRIVERS` — the manifest of shipped declaration programs. It carries the
  asset label only; the driver name is derived by parsing, so the manifest cannot disagree with the
  declaration it points at.
- `docs/blueprint.md` §13.4, `docs/cookbook/faq.md`, `docs/cookbook/chatwork.md`, skills regenerated.

**Quality gate, item by item.**

| Gate item | Status |
| --- | --- |
| One question, three outcomes distinguished | `/sys/declarations` → `status` ∈ `current` / `stale` / `local`, with `shipped` NULL exactly for `local` |
| The upgrade ruling recorded in the blueprint with its rejected alternative | §13.4 (3) — operator-initiated; automatic re-install rejected, with both reasons |
| Hermetic tests: fresh install / drifted shipped text / locally authored / re-install | 11 tests in `declaration_currency` (10 pure + 1 end-to-end over a real `sys_drivers` table and the real shipped asset), plus the `/sys` node's describe/capability test. No network, no credentials |
| `cargo test --workspace`, clippy, fmt, gen-docs, gen-skills, check-migrations | all exit 0 |

Two gate items got more than the letter asked for. The ticket named four test cases; the suite also
pins **install order is not identity**, **a `CREATE TYPE` change counts** (the Chatwork fix that
provoked the ticket was a type change — a comparison reading only driver/view/map rows would have
called that installation current, the exact untruth this node exists to prevent), **a statement
removed upstream reports stale** (the case the ticket flags as unhandled by superseding), **one
driver's declaration never reads another's rows**, and a ratchet asserting **every shipped
declaration compares current against itself** — which fails loudly if an asset stops desugaring or
the comparison reads a column the desugar does not write, rather than telling every operator their
up-to-date installation is stale.

### Discovered Insights

- **Insight**: A `/sys` node whose rows are *derived per read* is an established shape, not a new
  one — `/sys/whoami` resolves from the request principal and never touches the System DB, and its
  doc comment says so explicitly.
  **Context**: This is what made "no stamp, no migration, no new row kind" implementable rather than
  merely preferable. Anything answerable from state qfs already holds can be a `/sys` relation
  without becoming stored state, and the closed `SysNode` set is where it goes.

- **Insight**: The binary takes no direct `qfs-parser` edge; the AST types it needs ride through
  `qfs-exec` re-exports, each with a comment saying why (`Expr` for a refinement predicate,
  `Statement` for a stored collection body). Reading a desugared row back out of a parsed statement
  needed exactly two more (`EffectBody`, `Literal`).
  **Context**: The existing re-exports say "the binary never inspects it; it round-trips the body".
  This change is the first place the binary looks *inside* a statement, and it looks only at the
  effect's declared `VALUES`. A future reader wondering why those two types are re-exported will
  find the reason on the re-export itself.

- **Insight**: Recognising a desugared declaration row by its **column set** rather than by its
  `/sys/drivers` target path keeps the reader off the path AST entirely, and is more honest: the
  column set is what the row *is*.
  **Context**: The desugar writes exactly ten named columns for every `CREATE` form (`kind`, `name`,
  `base_url`, `auth`, `pagination`, `of_type`, `verb`, `body`, `irreversible`, `pushdown`) and the
  values carry column names, so the read maps by name and never depends on positional order.

- **Insight**: `create.sh` for feedback records accepts `source` ∈ `meeting | slack | discussion`,
  but `feedback/SKILL.md`'s schema block also lists `development`. Passing `development` is refused
  with `{"created": false, "reason": "bad_source"}`.
  **Context**: Hit twice this run while filing tooling feedback. The doc and the validator disagree;
  the validator wins. Unrelated to this ticket's code, recorded here because the next run that
  files a `kind: concern` from development will hit the same wall.
