---
type: Mission
title: A qfs cluster lets a member borrow the host's connections and shows its pools
slug: a-qfs-cluster-lets-a-member-borrow-the-host-s-connections-and-shows-its-pools
status: active
merge_policy:
created_at: 2026-09-28T02:10:53+09:00
author: a@qmu.jp
assignees: [a@qmu.jp]
assignee:
predicted_hours:
actual_hours:
feedback: [20260928020955-qfs-becomes-an-orchestrator-a-cluster-whose-members-borrow-the-host-s-connections-seen-through-a-pool-gui.md]
tickets: []
stories: []
gate_type:
gate_target:
gate_assert:
claim: work-20260928-021457
---

# A qfs cluster lets a member borrow the host's connections and shows its pools

## Goal

A mount registered on one machine's qfs is unusable on another. Make qfs instances
on one network form a cluster, so a member uses the host's connections without
holding its credentials, and the pools are visible. First slice of qfs as orchestrator.

## Experience

A host runs `qfs cluster host`; a member runs `qfs cluster join <url> --token ...`
and stays linked over a WebSocket. From the member, a Slack thread reply through a
mount on the host lands in Slack, and only the host held the token. The host's GUI
lists servers (CPU/mem/disk), the account pool and each member's sessions.

## Acceptance

- [x] A member joins a host over an authenticated WebSocket, and the host lists it with CPU, memory and disk (#20260928021122-cluster-members-join-a-host-over-an-authenticated-websocket-and-report-their-resources.md)
- [x] A member posts a Slack thread reply through a host mount; the credential never leaves the host (#20260928021122-a-member-runs-a-statement-against-a-host-mount-without-receiving-the-credential.md)
- [ ] A GUI on the host shows server pool, account pool and session allocation columns (#20260928021122-the-host-serves-a-column-gui-over-its-server-pool-account-pool-and-sessions.md)

## Changelog

- 2026-09-28: proposed by /specificate from the developer's orchestrator vision.
- 2026-09-28 — ticket archived — 20260928021122-cluster-members-join-a-host-over-an-authenticated-websocket-and-report-their-resources.md
- 2026-09-28 — ticket archived — 20260928021122-a-member-runs-a-statement-against-a-host-mount-without-receiving-the-credential.md
