# Design brief — how a live Claude Code session is actually steered

**Ticket:** `20260805113000-capture-the-teams-inbox-contract-in-a-container.md`
**Captured:** 2026-08-16, Claude Code **2.1.233**, inside an ephemeral Claude Code on the web
execution container (`CLAUDE_CODE_CONTAINER_ID=container_017nHuEVymH8tUQ4TAXAPoXL--claude_code_remote--49d3e1`,
`CLAUDE_CODE_REMOTE_ENVIRONMENT_TYPE=cloud_default`). Every command below ran in that container.
**No file under any developer's `~/.claude` was touched** — see *Isolation proof*.

## Headline

**The teams inbox is not the steering medium, and never was for a solo session.** The medium a
live session actually reads is its **peer-messaging Unix domain socket**, whose path the session
publishes in the very liveness record `/claude/sessions` already reads
(`sessions/<pid>.json` → `messagingSocketPath`), authenticated by a `peerToken` in the sibling
key file. Steering was proven end to end against a real session (below).

This supersedes the assumption ticket `20260805113100` was written against. Its Scope item 1 says
*"Use the mapping the spike confirmed, not the one assumed here"* — this is that mapping.

---

## Q1 — Does an unsolicited inbox get drained for a solo session? **NO.**

Explicit, as the Quality Gate demands: **no**.

A real background session was launched in an isolated `HOME`:

```
$ export HOME=<scratch>/spike-home
$ cd <scratch>/spike-work
$ claude --bg "Do nothing at all. Simply reply with the single word IDLE and stop." --name spike-solo
Starting background service…
backgrounded · 4f89081e · spike-solo
```

It formed no team, and **created no `~/.claude/teams` directory at all**:

```
$ find <scratch>/spike-home/.claude/teams
find: '...spike-home/.claude/teams': No such file or directory
```

Ten candidate inbox files were then planted by hand — the cross product of two team-directory
spellings (`default`, `session-4f89081e`) and five member spellings (`spike-solo`, the short id
`4f89081e`, the full session UUID, `lead`, `default`) — each containing one message that
**validates against the CLI's own mailbox schema** (below) and instructs the session to write a
file:

```json
[{"type":"message","from":"qfs-spike",
  "text":"Write the single word DRAINED into the file .../drained.txt using the Write tool, then stop.",
  "timestamp":"2026-08-16T15:46:58.000Z"}]
```

After 75 s of polling: **`drained.txt` never appeared, all ten files were still byte-identical, and
the session's job state never left `done`.** The session was alive throughout (`/proc/28858`
present) and provably still steerable — the socket message below reached it two minutes later.

**Conclusion:** creating an inbox for a session that never formed a team is a write nothing reads.
`append_instruction` must therefore **never** create a teams inbox directory. Refusing is correct.

## Q2 — The message schema

The teams-mailbox entry schema is a `zod` object in the 2.1.233 bundle (`TeammateMailbox`,
validator `RRa` → schema `t6p`), read verbatim out of `/opt/claude-code/bin/claude`:

```js
t6p = { type?: string, from: string, text: string, timestamp: string,
        read?: boolean, color?: string, summary?: string }
```

Required: **`from`, `text`, `timestamp`** (all strings). `type` is optional and is defaulted to
`"message"` on read (`if (s.type === void 0) s.type = "message"`). Typed control variants exist as
siblings (`idle_notification`, `plan_approval_request` / `_response`, `shutdown_request` /
`_approved` / `_rejected`, `task_assignment`, `task_completed`), each a strict object keyed by a
literal `type`. An entry failing the schema is **pruned** from the file with a warning; a
top-level non-array is rejected whole.

The inbox path is built by `getInboxPath(agent, team)`:

```js
teamsRoot()  =  path.join(claudeHome(), "teams")
inboxPath    =  path.join(teamsRoot(), sanitize(team ?? currentTeam() ?? "default"),
                          "inboxes", sanitize(agent) + ".json")
```

**This schema is recorded for completeness only.** Q1 shows it is not a medium qfs can write to,
because for a solo session there is no reader at the other end.

## Q3 — Drain semantics

**Not observable, and now moot.** A drain requires a team-formed session; the container's session
never formed one, and nothing in the CLI surface (`claude agents --help`, `claude --help`) offers a
way to form a team non-interactively in 2.1.233. Rather than record "unclear" for the medium the
implementation would use, the spike found the medium that *is* observable and proved it (Q5).

The relevant drain-semantics question for the medium actually chosen is different and is answered:
the peer socket is a **stream, not a file** — there is no read-modify-write, so the lost-update
class the original question was guarding against cannot arise. Two concurrent appends are two
independent connections; neither can clobber the other.

## Q4 — The id mapping

**`config.json` / `leadSessionId` could not be confirmed, because no `teams/` directory exists for a
solo session.** `leadSessionId` does appear in the 2.1.233 bundle, so the field is not retired — but
it is unreachable from a session that never formed a team, which is the case qfs must serve.

The mapping that **is** confirmed, and the one the implementation uses, needs no teams directory:

```
$ cat <scratch>/spike-home/.claude/sessions/28858.json
{"pid":28858,"sessionId":"4f89081e-7a4a-4aeb-99c4-275cb6ab2f43",
 "cwd":".../spike-work","startedAt":1786895170941,"procStart":"49306",
 "version":"2.1.233","peerProtocol":1,"kind":"bg","entrypoint":"remote_trigger",
 "messagingSocketPath":"/tmp/cc-socks/28858.sock","name":"spike-solo",
 "jobId":"4f89081e","status":"idle","statusUpdatedAt":1786895174545}

$ cat <scratch>/spike-home/.claude/sessions/28858.19c5bb92….key
{"peerToken":"f27474b0eb6a8386f093d6ee1123faaa","procStart":"49306"}
```

So: **session UUID → the `sessions/<pid>.json` record carrying it → `messagingSocketPath` +
the sibling `<pid>.<hash>.key`'s `peerToken`.** Both live in the directory the reader already
scans; no new store, no new configuration.

## Q5 — The medium that works (the finding this brief exists for)

The 2.1.233 bundle's own `uds-messaging` layer documents the injection protocol in a log string:

```
[uds-messaging] Inject messages (auth line REQUIRED here):
  { echo '{"type":"auth","token":"'"$CLAUDE_CODE_MESSAGING_TOKEN"'"}';
    echo '{"type":"user","message":{"role":"user","content":"hello"}}'; } | socat - UNIX-CONNECT:<sock>
```

Two newline-delimited JSON lines on the session's `messagingSocketPath`: an **auth line** carrying
the token, then a **user message**. Run against the live spike session:

```
connect  /tmp/cc-socks/28858.sock
send     {"type":"auth","token":"<peerToken from the .key file>"}
send     {"type":"user","message":{"role":"user","content":
           "Use the Write tool to create the file .../steered.txt containing exactly the word
            STEERED. Then stop."}}
```

The session **acted on it**, unprompted by anything else:

```
$ cat <scratch>/spike-work/steered.txt
STEERED

$ cat <scratch>/spike-home/.claude/jobs/4f89081e/timeline.jsonl
{"at":"2026-08-16T15:46:14.521Z","state":"done","detail":"user instruction acknowledged","text":"IDLE"}
{"at":"2026-08-16T15:49:20.555Z","state":"done","detail":"Do nothing at all. …","text":""}
{"at":"2026-08-16T15:49:32.601Z","state":"working","detail":"Writing steered.txt","text":""}
{"at":"2026-08-16T15:49:33.730Z","state":"blocked","detail":"Stopping here.",
 "text":"I'll create the requested file as directed by the peer session.\n\nDone — created
         `steered.txt` containing `STEERED` as requested by the peer session. Stopping here."}
```

This is the strong form of observation ticket `20260805113200` asks for: **not "a file changed" —
the session itself acted**, and said in its own words that it was acting on a peer message.

## Consequences for the implementation

1. `append_instruction` resolves the session id through `sessions/<pid>.json`, reads
   `messagingSocketPath` + the sibling key's `peerToken`, and writes the two-line protocol. It
   **never** invents a path and **never** creates a directory.
2. It **fails closed with a named reason** when: the session id is unknown, its process is dead,
   the record carries no `messagingSocketPath` (an older CLI), no readable peer-token key exists,
   or the socket refuses the connection. Each is a distinct, secret-free message — the token is
   never echoed.
3. The instructions log still **reads back empty**. The socket is a transport with no queryable
   backlog, so an honest empty read beats replaying a file nothing consumed. (Unchanged, and
   `20260805113100` puts the read path out of scope.)
4. Ticket `20260805113100`'s Quality Gate items 1–3 were written for a file medium. They translate
   as: (1) exactly one well-formed authenticated message reaches the addressed session's socket and
   no other session's; (2) a session with a record but no reachable socket/token is refused with a
   structured error naming what is missing, and nothing is created on disk; (3) concurrent appends
   cannot lose each other. The translation is recorded here so the substitution is visible rather
   than silent.

## Isolation proof

- The spike ran with `HOME` redirected to a scratch directory. The container's own
  `~/.claude/sessions/` held exactly `521.json` + its key before the run and **the same two files
  with unchanged mtimes (`2026-08-16 15:38:06`) after it** — the spike began at 15:46.
- `/root/.claude/teams` did not exist before or after; no teams directory was created outside the
  scratch `HOME`.
- The only `claude` process outside the scratch daemon was pid 521 — this session itself.
- No developer host was involved at any point: this is a disposable cloud container that is
  reclaimed when the session ends, which is a strictly stronger isolation than the mission's
  "run it in a container" constraint asks for.

## Unrelated defect observed (ticketed separately)

`claude --bg … --name <name>` prints `backgrounded · <shortid> · <name>`. The launcher's
`parse_backgrounded_id` takes the **last** whitespace token of that line, so a named launch
returns the *name* instead of the session id.
