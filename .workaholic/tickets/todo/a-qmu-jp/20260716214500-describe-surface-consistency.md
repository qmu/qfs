---
created_at: 2026-07-16T21:45:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort:
commit_hash:
category:
depends_on:
mission:
---

# Describe-surface consistency: navigable roots, no cd-into-blob, no grammar-less verbs

## Overview

Bundles three small describe-surface gaps (concerns `sys-and-slack-do-not-describe`,
`cd-into-a-blob-file-is`, `slack-workspace-namespace-still-advertises-verb`) — one theme: what
`describe` advertises must match what the engine actually admits and executes. Verified against
source this session:

1. **`/sys` and `/slack` roots are not describable, so `cd` fails before the gate.** The cd
   gate (`exec/src/shell/session.rs:306 namespace_check`) calls `driver.describe(path)` FIRST
   (:317-319) and only then consults `NodeDesc.navigable` (:320). `/sys` with no table segment
   has no node → `driver-sys/src/lib.rs:117-121` returns `UnsupportedVerb`; `/slack`'s
   `SlackPath::parse` requires a workspace segment → `driver-slack/src/path.rs:288-292` returns
   InvalidPath "the Slack workspace root is not a node". Neither root models itself as a
   navigable catalog interior, so `cd /sys` / `cd /slack` error at describe.
2. **`cd` into a blob file is admitted.** `LocalFsDriver::describe` (`driver-local/src/lib.rs:
   137-148`) ignores the path and unconditionally answers navigable `BlobNamespace` — a
   deliberate purity choice for plan-time type-checking, but it means the cd gate admits a leaf
   file as a directory.
3. **Slack's Files namespace advertises `Verb::Rm` with no query grammar.** `caps_for`
   (`driver-slack/src/lib.rs:226-232`) includes `Verb::Rm` on `SlackNode::Files`, but the only
   working delete is the path-scoped `remove /slack/<ws>/files/<id>`
   (`effect.rs:278-286`; namespace-level REMOVE demands an `id` the namespace's grammar cannot
   push). The advertisement promises a form the engine refuses.

## Implementation Steps

1. **Navigable roots**: give `/sys` and `/slack` root nodes a describe answer — a catalog
   interior (`NodeDesc` navigable, children = the node/workspace listing that `ls` already
   renders). For `/sys` that is the `SysNode` table list; for `/slack` the workspace segment
   (and `/slack/<ws>` its fixed children). The cd gate itself is untouched — the fix is
   drivers answering describe at their roots, matching the enumerable-children rule.
2. **Blob cd refusal**: driver-local's describe stays pure, but stops being path-blind where it
   can be pure AND path-aware: describe answers non-navigable for a path whose sandbox
   metadata says regular-file... If purity forbids the stat (plan-time contract, see the
   comment `lib.rs:138-144`), move the refusal to the gate's read side instead: the session's
   `namespace_check` may consult the read facet's entry kind when the describe archetype is
   BlobNamespace. Rule which side owns it in the ticket's first commit; both must not stat
   during pure planning.
3. **Slack Rm advertisement**: either wire the namespace-level grammar (REMOVE ... WHERE id =
   '<id>' lowering to the same `DeleteFile`) or stop advertising `Rm` on `SlackNode::Files`
   and leave it on `SlackNode::File` only (`lib.rs:223` already has `[Select, Remove]`).
   Prefer the smaller truth: drop the namespace advert unless the WHERE lowering is trivial.
4. Regenerate whatever `gen-docs --check` says moved (capability tables render from the
   binary).

## Key Files

- `packages/qfs/crates/exec/src/shell/session.rs:296-332` — the cd gate (context; likely
  unchanged except possibly step 2's read-side consult).
- `packages/qfs/crates/driver-sys/src/lib.rs:114-121` — /sys root describe.
- `packages/qfs/crates/driver-slack/src/lib.rs:198-240`, `path.rs:234-292` — /slack root
  describe + caps.
- `packages/qfs/crates/driver-local/src/lib.rs:137-148` — the path-blind BlobNamespace answer.
- `packages/qfs/crates/driver-slack/src/effect.rs:278-286` — the id-scoped delete the
  advertisement must match.

## Policies

- `workaholic:design` / reachability — `ls`/`cd`/`describe` are the agent-facing map; a root
  that lists but cannot be entered, or a verb that cannot be spoken, is a false map.
- `workaholic:implementation` / `type-driven-design` — capabilities advertised = capabilities
  executable, enforced by conformance tests, not review.
- `workaholic:implementation` / `coding-standards` + `test`.

## Quality Gate

1. `cd /sys` and `cd /slack` (and `cd /slack/<ws>`) succeed and `ls` renders their children;
   both-directions: the same cds fail on current code.
2. `cd /local/<regular-file>` is refused with a structured not-a-namespace error; `cd` into a
   directory keeps working; pure planning still never stats.
3. Either `REMOVE /slack/<ws>/files WHERE id = …` works, or the Files namespace no longer
   advertises `Rm` — and a conformance test asserts advertised verbs are executable across the
   slack node kinds.
4. Baseline gates + patch bump; plugin bump only if a taught surface moved (the slack skill
   teaches the path-scoped remove — check).

## Considerations

- Three concerns, one commit-series; archive all three concerns against this ticket when it
  ships.
- Step 2 has a real design choice (purity vs path-awareness) — small enough to rule in the
  ticket itself, but record the ruling in the commit body.
