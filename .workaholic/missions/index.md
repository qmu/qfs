# missions

A mission is a **standing property the product should have** — not an episode of work we are doing.
Reframed 2026-07-15 (owner-approved): missions named for activities ("design review", "capability
tryout") end while their residue does not, which twice orphaned live concerns onto missions that
thought they were finished. A mission closes when its property holds; anything it leaves behind is
re-homed **before** it closes, never after.

**Not everything needs a mission.** Tickets and concerns are allowed to belong to no mission at all
— a legitimate, named state, not an oversight. Forcing every concern to claim a parent is what
produced the mis-homing. Isolated defects, deliberate scope cuts, watch items, cross-repo tooling
fixes, and the owner-attended live-verification backlog all live mission-free; see
[concerns](../concerns/index.md).

## active

* [declared-drivers-are-the-normal-way-to-add-a-service](active/declared-drivers-are-the-normal-way-to-add-a-service/mission.md) - Adding a service is a reviewable qfs declaration you commit, not compiled Rust (0/7)
* [support-create-agent-semantics-that-introduce-a-new-user-principal-with-query-functions-scheduled-launch-and-access-control-to-resources](active/support-create-agent-semantics-that-introduce-a-new-user-principal-with-query-functions-scheduled-launch-and-access-control-to-resources/mission.md) - An agent is a first-class principal: its own identity, least-privilege grant, and audit trail (0/6)

## archive

* [language-design-review-layering-principles-and-semantic-gaps](archive/language-design-review-layering-principles-and-semantic-gaps/mission.md) - Language design review: layering principles and semantic gaps (achieved 2026-07-15, 11/11; its nine concerns re-homed before archiving)
* [qfs-capability-tryout-file-handling-transformation-and-platform-hardening](archive/qfs-capability-tryout-file-handling-transformation-and-platform-hardening/mission.md) - qfs capability tryout: file handling, transformation, and platform hardening (achieved; **goal #2 "less platform, more language" was unfinished at archive time** — carried on by [declared-drivers…](active/declared-drivers-are-the-normal-way-to-add-a-service/mission.md))
