---
created_at: 2026-07-18T20:33:35+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Added
depends_on: [20260718203334-agent-scheduled-launch-sweeper.md]
mission: support-create-agent-semantics-that-introduce-a-new-user-principal-with-query-functions-scheduled-launch-and-access-control-to-resources
---

# Owner-attended live round: a narrowly-granted scheduled agent runs, and its over-reach is visibly denied

## Overview

With the full chain shipped (grammar → subject → functions → cadence), run one owner-attended
live round on the owner's daemon to observe the agent principal end-to-end:

- `CREATE AGENT` with ONE narrow `ALLOW…AT` grant and a real query function on a cadence.
- Observe the fire land on the agent's run history and the audit ledger, recorded under the
  AGENT identity.
- Attempt an over-reach (a path the operator could read but the agent was not granted) and show
  the recorded denial.

Live-writes policy applies: self-visible resources only, owner triggers all commits. Everything
is rehearsed first by the hermetic suites in the tickets above; this is the one non-hermetic
acceptance item.

## Policies

- Live-round key & TTY pattern: the owner triggers commits in the real terminal; the agent session previews and verifies only.
- Never probe live cloud without explicit ask: no live probe before the owner is present.
- Self-visible only: touch only resources the owner alone can see.

## Quality Gate

1. One REAL fire under the agent principal, read back from the agent's runs AND the audit ledger.
2. One denied over-reach with a `deny_reason` naming the agent subject, recorded in the ledger.
3. The mission checklist is updated with observed EVIDENCE (paths/timestamps), not prose.
4. No live probe before the owner is present; only self-visible resources touched.
5. Verification: owner-attended live observation (the one non-hermetic item, per the mission's acceptance wording); everything rehearsed first by the hermetic suites above.

## Considerations

- Do not begin the live round until the owner is present at the real terminal; the agent session has no TTY for cloud writes.
- The narrow grant and the over-reach path must both be self-visible to the owner — never involve a resource others can see or receive.
- Capture concrete evidence (run-history row paths, ledger timestamps, the deny_reason string) into the mission checklist; prose is not acceptance.
