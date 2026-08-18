---
created_at: 2026-08-18T20:15:07+00:00
author: a@qmu.jp
assignees: []
depends_on:
mission:
merge_policy: review
verification_handoff:
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
