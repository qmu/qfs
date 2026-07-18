# Design brief: launching a Claude Code session through qfs (owner capability 2)

Status: **awaiting owner acknowledgment** (ticket `20260717010600-claude-session-create-launch.md`,
quality gate 1: the brief precedes every implementing commit). Written 2026-07-17 against the
shipped `v0.0.75` surface and the real Claude Code CLI on this box (`claude --help`,
`claude agents --help`, the `~/.claude/sessions/<pid>.json` liveness registry).

## 1. What a launch IS

Two shapes exist in the product today, verified on the CLI surface:

- `claude -p '<prompt>'` — a **one-shot turn**: runs, prints, exits. The process dies when the
  turn ends, so its liveness-registry record dies with it — it is *not* steerable and barely
  observable through `/hosts/<host>/claude/sessions` (a race with its own exit).
- `claude --bg '<prompt>'` — a **background agent session**: "start the session as a background
  agent and return immediately (manage with `claude agents`)". The process persists, writes its
  `sessions/<pid>.json` record on boot, and remains listed until it finishes.

**Recommendation: a launch is `--bg` — a persistent background session.** The mission's own
composition demands it: capability 4 ("send a message to a session") presumes the launched thing
outlives the launch. The one-shot `-p` form stays out of scope (it is a *turn*, not a session —
if ever wanted, that is a `|> transform`-adjacent design, not this one).

## 2. Grammar

Two candidates were named by the ticket:

- `CREATE SESSION ON /hosts/<host>/claude AT '<cwd>' [PROMPT '<text>']`
- `INSERT INTO /hosts/<host>/claude/sessions (cwd, prompt) VALUES ('<cwd>', '<text>')`

**Recommendation: the INSERT form.** Blueprint §3 rules "creating a resource is `INSERT INTO`
its collection … no per-driver create verb, ever" — a session is a resource in the `sessions`
collection, exactly the draft→`/mail/drafts` precedent. `CREATE <Noun>` is the *definition*
layer (structure that changes the address space); a session is runtime state, not a definition.
The INSERT keeps "everything is a path" true, previews legibly (one effect node, the
would-spawn cwd/prompt as the row payload), and needs zero grammar — only a deliberate
capability widening: `claude_node_capabilities(Sessions)` grows `Insert` beside `Select`, with
the write routed to a launcher effect in the applier lane (never the pure driver crate).
`RETURNING id` answers addressability (below).

## 3. Gating: cost and reversibility

A launch spends money (the turn runs on the operator's account) and starts an autonomous actor.
In qfs's classification a launch is **not reversible**: there is no inverse effect — the spend
and any actions the agent takes have happened (contrast the instructions append, which is
reversible in the ledger sense).

**Recommendation: the launch INSERT is declared irreversible** (the `Remove`/`mail.send`
precedent): PREVIEW shows exactly what would spawn (binary path as configuration, cwd + prompt
as data), and COMMIT requires the irreversible acknowledgement (`--commit-irreversible` /
interactive ack). Fail-closed without a configured store (`QFS_CLAUDE_SESSIONS` unset ⇒ no
applier, same as steering).

## 4. Identity

The spawned process runs **as the operator**: same uid, same `$HOME`, same `~/.claude`, same
billing account, same permission surface as a session the operator starts by hand. qfs adds no
identity, grant, or audit dimension of its own — that axis belongs to the
`support-create-agent-semantics-…` mission (agents as *principals*), deliberately separate
(owner decision 2026-07-16). This brief does not merge them: if a `CREATE AGENT` principal ever
*is* a Claude Code session, that is a later re-ruling of the agent model.

Consequences stated plainly: a qfs-launched session can do whatever the operator can, and its
spend lands on the operator's account. The irreversible gate (above) is the control.

## 5. Addressability

- **Return path**: the effect's `RETURNING` row carries the new session `id`. Mechanically the
  launcher learns the id from the spawned process (`--bg` prints the session handle; fallback:
  poll the liveness registry for the new pid's record — it appears when the session boots).
- **Visibility timing**: `sessions/<pid>.json` is written on boot, so the new id appears in
  `/hosts/<host>/claude/sessions` within the session's startup window — the live proof (QG 2)
  asserts launch → row visible → steerable, composing capabilities 2+4. NOTE: capability 4
  (steering) is itself still fail-closed pending its own medium ruling (see the steering
  investigation record, 2026-07-17); the composed live proof can only land after that ticket
  resolves.
- **A launch that dies immediately** (bad prompt/cwd): the liveness filter hides dead pids, so
  the row vanishes. The launch effect must still report honestly — RETURNING carries the id it
  observed plus the spawn outcome; post-mortem visibility beyond that is deliberately NOT built
  into the sessions relation (a `status='exited'` pseudo-row would fake the registry's
  semantics). The transcript file remains on disk for forensics.

## 6. Safety floor (unchanged commitments)

- No shell interpolation, ever: the launcher is `Command::new(<configured binary>)` with cwd and
  prompt passed as **arguments**; nothing user-supplied is joined into a shell line.
- The binary path is configuration (not a query input); cwd and prompt are data.
- Hermetic tests drive a fake launcher behind the seam (spawn effect, preview/commit gating,
  structured secret-free failures for bad cwd / unconfigured store); the live spawn is
  owner-attended (it spends money).

## Open questions for the owner

1. INSERT-into-sessions vs `CREATE SESSION` — the brief recommends INSERT (blueprint §3); veto?
2. Irreversible-gated launch — agreed?
3. Should the launch surface accept a `name` column (the store records one; it would make the
   returned session locatable by name in later queries)?

## Owner rulings (2026-07-18 — replan, all three questions closed)

The owner ruled all three open questions in a `/monitor` replan interrogation
(AskUserQuestion, 2026-07-18); the brief is **acknowledged** and quality gate 1 of ticket
`20260717010600` is satisfied. The rulings, verbatim in effect:

1. **Grammar — INSERT form approved.** `INSERT INTO /hosts/<host>/claude/sessions (cwd, prompt
   [, name]) VALUES (…)` is the launch verb. No `CREATE SESSION` noun; the `Sessions` relation's
   capabilities widen from `Select` to `Select+Insert` deliberately, with the write routed to a
   launcher effect in the applier lane (never the pure driver crate). `RETURNING id` answers
   addressability.
2. **Gating — irreversible acknowledgement required.** The launch INSERT is declared irreversible
   (the `Remove`/`mail.send` precedent): PREVIEW shows exactly what would spawn (binary path as
   configuration, cwd + prompt + name as the row payload); COMMIT requires the irreversible ack
   (`--commit-irreversible` / interactive ack). Fail-closed without a configured store.
3. **`name` column — accepted.** The launch surface accepts an optional `name` column
   (`INSERT (cwd, prompt, name)`; omittable). The real store records a session name, so a launched
   session is locatable by name in later `/hosts/<host>/claude/sessions` queries.

**What this unblocks and what it does not.** The launch *implementation* — INSERT grammar, the
capability widening, the launcher effect behind a fake seam, the irreversible gate, the `name`
column, and the hermetic tests (QG3) — is now fully specified and drive-ready. QG2's **live proof**
is not: it composes launch with steering (capability 4), and steering's transport medium is still
undecided pending an **owner-attended probe** (see ticket `20260717010500`). So the live,
money-spending composition waits for an owner-attended session; the hermetic implementation does
not.
