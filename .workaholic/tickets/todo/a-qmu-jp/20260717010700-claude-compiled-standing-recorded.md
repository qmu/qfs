---
created_at: 2026-07-17T01:07:00+09:00
author: a@qmu.jp
type: housekeeping
layer: [Domain]
effort:
commit_hash:
category: Changed
depends_on:
mission: claude-code-sessions-are-queryable-and-steerable-as-qfs-paths
---

# Record /claude's compiled standing in the declared-drivers mission

## Overview

Mission acceptance item 7. The `declared-drivers-are-the-normal-way-to-add-a-service` mission
states an absolute rule ("never a compiled-Rust driver") and today names only `/cf` as the
compiled counter-example — while `/claude` is a second, unnamed one. `/claude` mechanically
cannot be declared: the declared shape is REST-shaped (`base_url`/`auth`/`pagination`/`verb`/
`body`) and `/claude` has no base URL, no auth, no wire. Blueprint §13 (`blueprint.md:915-917`)
frames compiled drivers as a **ratchet, not a partition**: "Compiled drivers remain until their
script twin passes the conformance suite" — the ratchet has not reached `/claude`, so it is not
in violation; it is just unnamed.

Ship the naming: the declared-drivers mission text names `/claude` alongside `/cf` as a
compiled counter-example with the §13 ratchet framing as what governs it, so the rule stops
reading as absolute while two unnamed exceptions exist. This is the only integration between
the two missions the evidence supports — no code change, no driver conversion (explicitly out
of this mission's scope).

## Policies

- `workaholic:development` / change history — a standing rule with silent exceptions decays;
  the record is the fix, not a code workaround.

## Quality Gate

1. The declared-drivers mission document names `/claude` with the ratchet framing and cites
   blueprint §13.
2. The claude mission's acceptance item 7 checkbox can be ticked with a changelog line
   pointing at the edit.
3. No code, schema, or generated doc changes (gen-docs `--check` trivially green).

## Considerations

- Keep the edit inside the declared-drivers mission's own voice (it owns the rule); the claude
  mission only demanded the naming.
- If the declared shape ever grows a non-REST arm, the ratchet question reopens there — do not
  pre-rule it here.
