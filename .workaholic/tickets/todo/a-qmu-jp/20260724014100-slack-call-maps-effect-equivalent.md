---
created_at: 2026-07-24T01:41:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Added
depends_on: [20260724014000-declare-the-slack-twin-and-prove-read-equivalence.md]
mission: the-declared-slack-twin-retires-the-compiled-driver
---

# Slack CALL maps effect-equivalent

## Overview

Playbook §13.3 entry #1, second half of the equivalence bar: the five compiled Slack CALLs —
**react, pin, unpin, update, delete** — become **typed CALL maps** in `slack.qfs` per the G5
ruling (typed CALL signatures, blueprint §13.1), each effect-equivalent to its compiled
counterpart on hermetic wire fixtures.

The v0.0.89 compiled-driver behavior is the contract to reproduce: every ID-requiring call routes
through channel-name→id resolution (one address, one meaning), and unresolvable names fail at
PREVIEW time as usage errors, never as garbage ids at commit.

## Policies

- workaholic:design / 「推測するな、宣言して拒否せよ」 — an unresolvable channel/user reference is
  refused at preview, exactly as the compiled driver does since v0.0.89.
- Blueprint §13.1 G5 — CALL signatures are typed; a wrong-shaped argument is a parse/typecheck
  error, not a wire error.
- workaholic:development / hermetic gates — no live Slack tokens; fixtures only.

## Quality Gate

1. Each of the five CALL maps produces the same wire request as the compiled CALL on the shared
   fixtures (method, endpoint, resolved channel id, payload) — five effect-equivalence tests.
2. The name→id resolution behavior matches: a fixture case proves a name-addressed channel
   resolves before the effect fires, and an unresolvable name is a structured preview-time error.
3. Typed signatures reject a malformed argument at typecheck (one negative case per distinct
   signature shape).
4. Workspace gates green: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
   `cargo fmt --all --check`.

## Considerations

- If the honest verb set the declaration states naturally resolves the tracked concern
  `slack-workspace-namespace-still-advertises-verb` (Verb::Rm advertised without grammar), record
  that in the final report so the ship-time concern judge can close it — but do not add
  file-delete capability to force it.
