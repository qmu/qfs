---
created_at: 2026-08-18T20:15:07+00:00
status: done
author: a@qmu.jp
assignees: []
depends_on:
mission:
merge_policy: review
verification_handoff:
claim: work-20260818-210148
---

# A declared mount now carries two independent per-node tables, and which one answers depends on which mount you ask

## Overview

Two pull requests closed the same defect from opposite ends within a day of each other, and both
landed. `main` at `ece0116` therefore carries **two** per-node capability tables built from the same
declarations:

| Table | Built by | Lives on | Read by |
| --- | --- | --- | --- |
| `DeclaredNodeDesc { template, of, verbs }` | `declared_node_descs()` (PR #46) | `RestDriver.nodes` | `describe`, `children`, `capabilities` |
| `NodeMap { path, verbs, irreversible_verbs }` | `DeclaredDriver::declared_nodes()` (PR #64) | `RestApiConfig.nodes` | `RestDriver::caps_for`, `write_irreversible` |

Which one answers a question is decided by **which mount** was built, because only one of the two
mounts is given the first table:

- The **describe/plan** mount (`declared_describe_mount_with_types`, `declared_driver.rs:829-840`)
  calls `.with_declared_nodes(...)`, so `self.nodes` is non-empty and `capabilities()` returns
  before ever reaching `caps_for` — on this mount `caps_for`'s declared branch is dead code, while
  `write_irreversible` still reads `RestApiConfig.nodes`.
- The **live** mount (`declared_driver.rs:1544`) is `RestDriver::new(d.rest_config(), …)
  .with_procs(…)` with **no** `with_declared_nodes`, so `self.nodes` is empty, `capabilities()`
  falls through to `caps_for`, and the answer comes from `RestApiConfig.nodes` instead.

The two derivations agree today — both read the same `CREATE VIEW` / `CREATE MAP` rows — so this is
not a live wrong answer. It is the *shape* the declared stack has already been bitten by twice:
`20260728085253`'s own record states the pattern as "describe and the read path answering from
different sources", and its fix deliberately closed it "by construction rather than by keeping two
sources agreeing". This reintroduces exactly that, with one extra hazard the earlier instance did
not have — the two tables do not even carry the same fields (`of` on one, `irreversible_verbs` on
the other), so neither can simply be deleted without moving something first.

## Scope

**In scope:** collapse to one per-node table — `DeclaredNodeDesc` — by moving the irreversible
marking onto it, pointing `write_irreversible` at the same list `capabilities()` reads, giving the
live mount the same `with_declared_nodes(...)` the describe mount gets, and deleting `NodeMap`,
`RestApiConfig::nodes`, `with_nodes`, `declares_nodes`, `verbs_for_path`, `irreversible_for_path`
and `DeclaredDriver::declared_nodes()` with their tests.

**Out of scope:**

- `ResourceMap` / `resource_for_segment` — the per-segment aggregate the **wire** layer resolves a
  resource with, and the capability answer a **compiled** `/rest` mount keeps. Both survive.
- The declared describe surface itself (`of`, children, walkability) — shipped and unchanged.

## Policies

- `workaholic:implementation` / `policies/coding-standards.md` — one question, one answer; a second
  derivation of the same fact is a disagreement waiting to happen.
- `workaholic:implementation` / `policies/directory-structure.md`.
- `workaholic:operation` / `observability` — two tables that can drift make the capability answer
  unexplainable from the outside: you have to know which mount was asked.

## Key Files

- `packages/qfs/crates/driver-http/src/lib.rs` — `DeclaredNodeDesc`, `RestDriver::declared_node`,
  `capabilities`, `caps_for`, `write_irreversible`: the surviving table and the two readers.
- `packages/qfs/crates/driver-http/src/config.rs` — `NodeMap`, `RestApiConfig::nodes`,
  `verbs_for_path`, `irreversible_for_path`, `declares_nodes`: everything to delete.
- `packages/qfs/crates/qfs/src/declared_driver.rs` — `declared_node_descs()` (keep, extend),
  `DeclaredDriver::declared_nodes()` (delete), `rest_config()`'s `.with_nodes(...)` call (delete),
  and line 1544's live mount, which needs `.with_declared_nodes(...)` added.
- `packages/qfs/crates/driver-http/src/tests.rs` —
  `a_declared_mount_gates_per_node_not_per_leading_segment` and
  `a_node_template_addresses_its_own_path_only` are written against `NodeMap` and must be rewritten
  against `DeclaredNodeDesc`, not merely deleted.

## Related History

Both halves of this were driven by unattended runs a day apart, neither able to see the other's
in-flight branch — the collision is a property of two long-lived claim branches, not of either fix.

- [20260728085253-declared-driver-undiscoverable-through-describe.md](.workaholic/tickets/archive/work-20260729-145625/20260728085253-declared-driver-undiscoverable-through-describe.md) — PR #46, which made `capabilities` per node as a consequence of making `describe` per node
- [20260817001110-a-declared-mounts-verb-gate-is-leading-segment-coarse.md](.workaholic/tickets/archive/work-20260817-004052/20260817001110-a-declared-mounts-verb-gate-is-leading-segment-coarse.md) — PR #64, which made `capabilities` and `write_irreversible` per node from the config side

## Implementation Steps

1. Reproduce the split first, so the fix is measured rather than assumed: assert that the **live**
   mount (`declared_driver.rs:1544`) and the **describe** mount answer `capabilities` for the same
   declared path through different code — e.g. by asserting today that the live mount's
   `self.nodes` is empty while the describe mount's is not.
2. Add `irreversible_verbs: Vec<Verb>` to `DeclaredNodeDesc` and populate it in
   `declared_node_descs()` from each map's `irreversible` flag (the marking rides the verb it
   belongs to; a node reached by several declarations keeps each one's marking).
3. Point `write_irreversible` at `self.declared_node(path)` when `self.nodes` is non-empty, keeping
   the per-segment answer for a mount that declares none.
4. Give the live mount `.with_declared_nodes(declared_node_descs(d, types))` so both mounts answer
   from the one table. Check what the live path has in hand for `types` — the describe path is
   handed a `DeclaredTypeDefs`; if the live mount cannot resolve one, decide explicitly whether it
   passes an empty registry (verbs correct, `of` absent) or is given the real one, and record why.
5. Delete `NodeMap` and its `RestApiConfig` surface, rewrite the two `NodeMap`-based tests against
   `DeclaredNodeDesc`, and confirm `caps_for` retains exactly one caller shape: the compiled mount.

## Quality Gate

**Acceptance criteria**

- `grep -r NodeMap packages/qfs/crates` returns nothing.
- One table answers both `capabilities` and `write_irreversible` for a declared mount, and the live
  and describe mounts answer identically for every node of the shipped slack, chatwork and
  cloudflare declarations.
- `remove /slack/{ws}/files/{file}` is still gated irreversible and
  `INSERT INTO /slack/{ws}/{channel}/messages` is still not — the property PR #64 added, preserved
  through the collapse.
- A compiled `/rest` mount's capability and irreversibility answers are unchanged.

**Verification method**

- Hermetic tests built from the shipped `.qfs` bytes (the `shipped_mount` / `declared_from_script`
  helpers), walking every declared node of all three shipped declarations through both mounts.

**Gate**

- `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all --check`, `gen-docs --check`, `gen-skills --check` all exit 0.

## Patches

A working implementation of steps 2, 3 and 5 was built and tested against `1220711` (the PR #64
branch head before its merge) during the run that found this. It is reproduced here because it is
already verified — 48 `qfs-driver-http` tests green with it — not because it is the only shape.
Step 4 (the live mount) is **not** in it and is the part that still needs deciding.

> **Note**: These patches were written against the pre-merge tree; re-check the context lines
> against `main` before applying.

### packages/qfs/crates/driver-http/src/lib.rs

```diff
@@ -106,6 +106,11 @@ pub struct DeclaredNodeDesc {
     /// write verb from a `CREATE MAP`). Per NODE, so a node only ever advertises the verbs its own
     /// declarations gave it.
     pub verbs: Vec<Verb>,
+    /// The subset of `verbs` a `CREATE MAP … IRREVERSIBLE` marked irreversible (blueprint §7/§8).
+    /// Per NODE for the same reason `verbs` is: the Slack file detach is declared `IRREVERSIBLE` on
+    /// `/slack/{ws}/files/{file}` alone, and the message post on `/slack/{ws}/{channel}/messages`
+    /// must not inherit its gate.
+    pub irreversible_verbs: Vec<Verb>,
 }
```

```diff
     fn write_irreversible(&self, path: &Path, verb: Verb) -> bool {
+        if !self.nodes.is_empty() {
+            return self
+                .declared_node(path.as_str())
+                .is_some_and(|n| n.irreversible_verbs.contains(&verb));
+        }
         let Some(rv) = verb_to_rest_verb(verb) else {
             return false;
         };
```

### packages/qfs/crates/qfs/src/declared_driver.rs

In `declared_node_descs()`, the `push` closure takes a fourth `irreversible: bool` argument, gates
it through `let gated = verb.filter(|_| irreversible);`, and pushes `gated` onto a new
`irreversible_verbs` field. The view loop passes `false`; the map loop passes `m.irreversible`.

## Considerations

- Found by the `[Implement]` routine on 2026-08-18 while reconciling PR #64 with a `main` that had
  meanwhile merged PR #46. The run had reworked its own branch onto the surviving table before the
  PR was merged out from under it; the rework is the patch above.
- **Nothing here is a live wrong answer**, so it is not urgent — both tables derive from the same
  rows. It is worth doing before a third change touches either one, because the next edit to one
  table is the moment they start to disagree.
- The `verbs` half is genuinely settled: `capabilities()` is per node on both PRs' accounts, and
  `a_declared_node_advertises_only_its_own_declared_verbs` pins it over the shipped chatwork
  declaration. Only the plumbing is duplicated.

## Final Report

Development completed as planned. The two per-node tables are one: `DeclaredNodeDesc` carries the
irreversible marking, `write_irreversible` reads the same list `capabilities()` does, and the live
mount is given that list — so the answer no longer depends on which mount was asked.

### What changed

- **`DeclaredNodeDesc` gained `irreversible_verbs: Vec<Verb>`** (`driver-http/src/lib.rs`), and
  `declared_node_descs()`'s `push` closure takes a fourth `irreversible: bool`, gates it through
  `verb.filter(|_| irreversible)`, and merges it per template. Views pass `false`; maps pass
  `m.irreversible`. This is the ticket's `## Patches` shape, re-checked against `main`.
- **`write_irreversible` reads the declared table first**: when `self.nodes` is non-empty it answers
  from the matching nodes' `irreversible_verbs`; a mount that declares none keeps the per-segment
  answer, which is the compiled `/rest` case.
- **The live mount gets the table.** `live_rest_driver` now takes a `&DeclaredTypeDefs` and calls
  `.with_declared_nodes(declared_node_descs(d, types))`. Both call sites pass the real registry:
  `shell.rs` reuses the `DeclaredTypeDefs` it already loads for `set_declared_types` (bound once and
  cloned rather than opening the System DB twice), and `commit.rs`'s apply-lane twin loads it the
  same way. This resolves the ticket's step-4 open question — the live mount is given the **real**
  registry, not an empty one — because both call sites already had one in hand, so passing empty
  would have cost `of` on the live mount for no reason.
- **Deleted**: `NodeMap`, `RestApiConfig::nodes`, `with_nodes`, `declares_nodes`, `verbs_for_path`,
  `irreversible_for_path`, `DeclaredDriver::declared_nodes()`, and `caps_for`'s declared branch —
  which leaves `caps_for` with exactly one caller shape, the compiled mount. `grep -r NodeMap
  packages/qfs/crates` returns nothing.

### The decision the collapse forced: first-match or union

The two tables did not resolve a path the same way. `capabilities()` took the **first** matching
`DeclaredNodeDesc`; `verbs_for_path`/`irreversible_for_path` **unioned every** matching `NodeMap`.
Collapsing onto one table therefore had to pick, and the choice is not cosmetic: the two rules
differ exactly when two distinct templates address one concrete path (`{tenant}/things` and
`acme/things`).

**The union wins**, so `capabilities()` moved to it rather than `write_irreversible` moving to
first-match. The reason is the one `config.rs` already recorded for `verbs_for_path`: the apply seam
picks the first declared map matching the path *for the verb being written*, so a verb is
performable exactly when some matching node declares it, and a narrower gate would refuse writes the
applier would then have performed. A first-match gate would also have **narrowed
`write_irreversible`** — an irreversible marking on a later-matching template would have gone
unanswered, dropping a safety gate. `declared_nodes_for()` is the shared matcher; `declared_node()`
(first match) survives for `describe`, which needs one node's `OF` contract and child key, not a
union.

Note this also closes a disagreement the ticket did not name: on that two-template shape the live
mount (union, via the config) and the describe mount (first-match) could already have answered
differently. Both union now.

### Verification

New hermetic tests, built from the shipped `.qfs` bytes:

- `the_live_and_describe_mounts_answer_from_one_declared_node_table` walks **every** declared node
  of the shipped slack, chatwork and cloudflare declarations through **both** mounts and requires
  the same answer for all five universal verbs, on capabilities and on irreversibility.
- `the_live_mount_keeps_the_declared_irreversible_gate` pins PR #64's property on the live mount:
  `remove /slack/{ws}/files/{file}` is gated, `INSERT INTO /slack/{ws}/{channel}/messages` is not.

The first was **negative-controlled**: reverting only step 4 (dropping `.with_declared_nodes` from
`live_rest_driver`) turns it red — `slack /slack/{ws}/channels: the two mounts disagree on Insert` —
and restoring it turns it green. So the test measures the split rather than merely passing.

The two `NodeMap`-based tests in `driver-http/src/tests.rs` were rewritten against
`DeclaredNodeDesc` rather than deleted: `a_node_template_addresses_its_own_path_only` now reads the
match rule through `capabilities()`, its only consumer, and
`a_declared_mount_gates_per_node_not_per_leading_segment` builds its nodes with
`.with_declared_nodes(...)`. Two `declared_driver.rs` tests that asserted against `cfg.nodes` were
likewise re-pointed at `declared_node_descs()`.

Gate: `cargo test --workspace` exit 0 (48 `qfs-driver-http` tests green, 78 `declared_driver` tests
green), `cargo clippy --workspace --all-targets -- -D warnings` exit 0, `cargo fmt --all --check`
exit 0, `gen-docs --check` and `gen-skills --check` both in sync. Patch version `0.0.123`.

### Discovered Insights

- **Insight**: a duplicated table is rarely a duplicate of the same *rule*. These two derivations
  agreed on their data and disagreed on their **resolution** — first match versus union — so
  deleting either one silently changed behaviour in a way no test named. The collapse had to be a
  decision about which rule is right, not a mechanical deletion.
  **Context**: whenever two derivations of one fact are merged here, read the *lookup* as carefully
  as the contents. The rule that survives should be the one the layer below already assumes; here
  the applier's own first-matching-map-per-verb behaviour is what makes the union correct, and that
  argument was already written down in `config.rs`'s doc comment.
- **Insight**: `RestDriver::capabilities` short-circuits on `self.nodes.is_empty()`, so on the
  describe mount `caps_for`'s declared branch was unreachable while `write_irreversible` still ran
  it. Dead code on one mount and live code on the other, from one `if`.
  **Context**: when a builder is optional (`with_declared_nodes`), every reader that branches on the
  same field has to be checked at each construction site, not once. The construction site is the
  real switch, and there were two of them (`declared_describe_mount_with_types` and
  `live_rest_driver`) with only one carrying the call.
