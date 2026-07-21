---
type: Strategy
title: Access derives from the resolved principal
slug: access-derives-from-the-resolved-principal
status: active
created_at: 2026-07-22T08:52:03+09:00
author: a@qmu.jp
---

# Access derives from the resolved principal

## Direction

What a caller can see and do derives from who the caller is — a resolved principal, never an
ambient default (the strategy repo's アクセス制御 direction). Humans and AI are peer principals;
roles bundle subjects (RBAC) and policies grant (subject, verb, path-pattern) triples (PBAC);
the resource unit is the path, so one policy governs every face — screen, query, console, HTTP —
and per-face permission drift is structurally impossible. The signed-out state is a first-class
answer, not an error, which is what makes a consumer's sign-in-only view derivable rather than
guessed. Fail-closed is permanent: an unresolved or unrecognized actor gets least privilege, and
no threading of identity ever widens a default. Identity stays separate from authorization —
knowing who is asking never silently becomes a grant.

## Changelog

<!-- Append-only, dated timeline. One line per event ("- YYYY-MM-DD — event — filename");
     never rewrite past lines. Retirement (rare) is a recorded transition, not a deletion. -->
