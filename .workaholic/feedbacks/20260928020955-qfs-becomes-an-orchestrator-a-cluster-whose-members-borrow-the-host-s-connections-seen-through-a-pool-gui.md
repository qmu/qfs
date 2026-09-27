---
type: Feedback
title: qfs becomes an orchestrator - a cluster whose members borrow the host's connections, seen through a pool GUI
kind: instruction
source: development
subject: person:a@qmu.jp
created_at: 2026-09-28T02:09:55+09:00
author: a@qmu.jp
supersedes: 
review_surface: 
---

# qfs becomes an orchestrator - a cluster whose members borrow the host's connections, seen through a pool GUI

Stated by the developer in a Claude Code session on 2026-09-28, prompted by a concrete miss: the Slack account mount this repository's work loop declares is registered in the qfs of another machine, so the loop on this machine could not read or post at all. If this machine's qfs were a member of a cluster, it could use the host's connection transparently. That is the ask.

## The first, small vertical slice asked for

1. qfs server instances on the same network form a cluster: one host (plus, later, a secondary) and members kept connected over a persistent WebSocket.
2. A session or agent on a member can reply into a Slack thread through a Slack profile registered on the host. The member must prove it is a legitimate cluster member (later: an agent the host spawned); the credential is never persisted on the member.
3. A GUI (plgg / plggmatic, column-oriented SPA) showing the server pool (members' CPU, memory, disk), the account pool (Claude Code / Codex accounts) and session allocation (which server, which directory, which session, how far along).

Security is the first priority: the cluster's trust model must be designed and written down, not bolted on. The developer asked for this slice to be carried until it can be tried by hand.

## The longer direction (the developer's vision, summarized)

- From a "Swiss army knife" for AI agents to an **orchestrator**, keeping the query language.
- A full server mode; members other than the host and its secondary are stateless. Querying the host returns members' CPU/memory/disk, members' Claude Code / Codex session state, and Claude Code / Codex account info and usage.
- An agent can issue a query to the host naming server, LLM provider, model, effort, container sandbox (default podman) and prompt; the host spawns the agent there and its state is queryable - "Text to QFS" for agent allocation, like Text to SQL.
- The host lends its stored credentials and query targets to members, securely and temporarily.
- Accounts become data: an **account pool** of agent identities (an agent's own mailbox and the Claude Code / Codex plans contracted with it), not "whose account". When one server hits a usage limit, a query switches it to a free account from the pool and the work continues.
- An agent the host spawned reaches the host's connections through its local member transparently, once it can prove that provenance - e.g. several Slack bot profiles on the host, usable from any session on any server.
- Security first: credential lending and spawn instructions need a convincing security model.
- A plggmatic column-oriented SPA to traverse qfs data and query results, covering the new concepts (server pool, account pool).
