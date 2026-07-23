---
created_at: 2026-07-24T01:43:00+09:00
author: a@qmu.jp
type: housekeeping
layer: [Domain]
effort:
commit_hash:
category: Removed
depends_on:
mission: the-declared-slack-twin-retires-the-compiled-driver
---

# /cf queue-pull twin and retirement

## Overview

Blueprint §13.3's honest-tiering table records exactly one exception whose reason is "not yet
done", not "cannot be done": the compiled **`/cf` queue-pull** is a read-over-POST whose declared
spelling now exists (G1, shipped v0.0.85), but the compiled implementation is still present at
HEAD. Close the exception with the same twin-and-retire arc, deliberately kept out of the G1 ship
ticket as a mechanical follow-up:

1. Declare the queue-pull twin (the `|> POST` read shape over the queue-pull endpoint) in the
   cloudflare declaration.
2. Prove it row-equivalent on the existing wire fixture (the G1 ship already drives a declared
   queue-pull twin through the real tier-2 evaluator in its hermetic test — reuse/extend it as
   the equivalence gate).
3. Delete the compiled queue-pull path from `driver-cf` (the queue-pull only — `/cf` Artifacts
   stays compiled per G8, it is a git-repo surface).
4. Update the §13.3 honest-tiering table: the exception's status flips from "not yet done" to
   closed, so the table keeps its promise that no silent exception rides the conversions.

Independent of the slack tickets (`depends_on` empty) — it can land first or last in the night.

## Policies

- Blueprint §13.1 G8 — the git-shaped `/cf` Artifacts surface is NOT touched; only the REST
  queue-pull converts.
- Blueprint §13 twin-and-retire ratchet — equivalence before deletion.
- CLAUDE.md plugin re-versioning — if any skill-taught surface names the compiled queue-pull,
  the ticket shares the mission's plugin MINOR bump (do not bump twice; coordinate with ticket
  20260724014200 in whichever PR ships).

## Quality Gate

1. The declared queue-pull twin reads row-equivalent to the compiled queue-pull on the wire
   fixture (hermetic).
2. The compiled queue-pull code path is deleted; `/cf` Artifacts and the rest of driver-cf are
   untouched; workspace builds clean.
3. `docs/drivers.md` regenerated if the described surface changed (gen-docs --check green).
4. Blueprint §13.3's tiering table row for the queue-pull records the closure with the commit.
5. Workspace gates green: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo fmt --all --check`.

## Considerations

- Queue CONSUMPTION semantics (ack/visibility) beyond the existing compiled pull's behavior are
  out of scope — equivalence to what exists, nothing more.
