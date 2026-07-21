---
type: Strategy
title: Integrations are declared, not compiled
slug: integrations-are-declared-not-compiled
status: active
created_at: 2026-07-22T08:52:02+09:00
author: a@qmu.jp
---

# Integrations are declared, not compiled

## Direction

Far more services and file kinds must be supportable than compiled Rust can keep up with, so
per-integration compiled code gives way to declarations over one generic engine (owner
direction, 2026-07-20). A wire service enters qfs as a driver declaration written in the query
language itself; a file kind enters as a codec riding the generic collection path; a knowledge
surface is a registered set — a stored view — over paths that already exist. Compiled Rust
remains only as a shrinking set of *named* structural exceptions (a local repo, a local store,
blob and SQL primitives), never as the default answer to "how do we add X". Where the declared
shape is not expressive enough, the runtime semantics are redesigned — qfs is experimental,
hard breaks are correct — rather than the integration falling back to Rust. The observable
consequence: an agent or human adds a service by writing one screen of declaration, and the
reviewer reads the whole integration in that screen.

## Changelog

<!-- Append-only, dated timeline. One line per event ("- YYYY-MM-DD — event — filename");
     never rewrite past lines. Retirement (rare) is a recorded transition, not a deletion. -->
