---
created_at: 2026-07-13T12:30:00+09:00
author: a@qmu.jp
type: bugfix
layer: [Domain]
effort:
commit_hash:
category:
depends_on:
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
---

# Pure-read transform pipe over Gmail fails at commit: "READ is not serviced by the Gmail driver"

## Problem (found live, round 6, v0.0.59)

A transform chain with no terminal write, sourced from Gmail:

```qfs
/mail/inbox |> select subject |> limit 2 |> transform sumline |> transform digestline
```

previews fine (model-free) but `--commit --commit-irreversible` fails with:

> malformed EFFECT effect at "/mail/inbox": READ is not serviced by the Gmail driver;
> skipped (dependency NodeId(0) failed); skipped (dependency NodeId(1) failed)

Yet the T8/round-2 switch statement materialized the same `/mail/inbox` READ at commit
successfully — the difference is the terminal shape (switch arms ending in INSERTs vs a pure-read
chain returning rows). The commit path for a read-terminal transform plan services the source
READ through a facet Gmail doesn't implement, while the switch commit path reads it fine. The
same chain over a `/local` source commits and returns rows (round 6 proof used it).

## Fix

Route the source READ of a read-terminal transform plan through the same read materialization
the switch/write-terminal commit path uses (or wire the Gmail read facet the effect executor
wants). Add a hermetic lock: transform chain over a mocked gmail source, commit returns rows.

## Key files

- the commit executor's read-node servicing (where "READ is not serviced by" originates)
- `packages/qfs/crates/driver-gmail/` — read facet registration
- reference: round-2's switch commit path (materialize once → partition) which reads Gmail fine

## Resolution (2026-07-13, branch work-20260713-150833)

Root cause: the read-terminal transform commit path (`qfs_exec` `run_oneshot`, the
`transform_read && Statement::Query` arm) reads the source correctly through the read engine
(`block_on_read_with`) but then called `apply_via(&plan.clone())` on the WHOLE plan — including the
source `Read` node — to ledger the transform-consent markers. The interpreter dispatches that `Read`
node to the mount's WRITE applier, and Gmail's (like GitHub's/Slack's) services no READ →
"READ is not serviced by the Gmail driver". The switch/write-terminal path never hit this because
`consume_source_into_write` already strips source reads before apply; only the read-terminal path
lacked the equivalent.

Fix: added `strip_source_reads`, called on the plan before the consent-ledger `apply_via` in the
read-terminal arm. The source reads were already serviced by `block_on_read_with`, so they are
dropped (with their dep edges) and never reach a write applier — the same treatment the
write-terminal path already applies. A `/local`-sourced chain was unaffected only because the local
applier happens to service READ.

Hermetic lock (`exec/tests/oneshot.rs`):
`read_terminal_transform_commits_though_the_source_applier_rejects_read` — a `world_apply` that
rejects READ exactly like the Gmail applier (the in-memory `apply_commit` fallback accepts
everything, so it could not surface this), over
`/mail/inbox |> select subject |> transform extract |> transform summarize`, asserting the commit
returns the last stage's rows. Verified it FAILS with `strip_source_reads` disabled and PASSES with
it — a true regression lock.
