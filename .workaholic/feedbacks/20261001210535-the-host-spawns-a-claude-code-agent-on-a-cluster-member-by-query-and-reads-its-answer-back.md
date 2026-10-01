---
type: Feedback
title: The host spawns a Claude Code agent on a cluster member by query and reads its answer back
kind: instruction
source: development
subject: person:a@qmu.jp
created_at: 2026-10-01T21:05:35+09:00
author: a@qmu.jp
supersedes: 
review_surface: 
---

# The host spawns a Claude Code agent on a cluster member by query and reads its answer back

Stated by the developer in a Claude Code session on 2026-10-01, right after a three-machine cluster (a Mac Studio host, a Raspberry Pi member and an EC2 member over reverse SSH tunnels) was formed by hand from mission `a-qfs-cluster-lets-a-member-borrow-the-host-s-connections-and-shows-its-pools`.

The developer asked whether the cluster could talk to the Claude Code on each machine, have each tell a joke, and gather the answers. The session could only do it over SSH (`claude -p` per machine): the cluster has no path for the host to instruct a member. The developer said to proceed with that path.

## The ask

From the host, a query spawns a Claude Code agent on a chosen member with a prompt, and its outcome is readable back through the host - e.g. one prompt sent to every member, the answers gathered. This is the first step of the orchestrator vision's "spawn an agent on a server by query" (see the source feedback of the mission above), kept small: one-shot `claude -p` runs, no container sandbox, no account pool switching yet.

Security stays first: a member only runs what its host sent over the authenticated link, and a member operator must opt in to accepting spawns.
