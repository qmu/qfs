---
created_at: 2026-09-17T21:13:25+09:00
author: a@qmu.jp
assignees: [a@qmu.jp]
depends_on:
feedback: [20260917211211-fb-untracked-moderate-tick-logs-stop-every-implement-freshen.md]
merge_policy:
verification_handoff: 
---

# Ignore the /moderate tick-log directory so the [Implement] freshen stops refusing

## Overview

`/moderate` writes its tick log to `.workaholic/moderations/<date>.md` and its `persist-log`
step deliberately leaves the file **untracked** in the checkout. `/implement`'s first act
(`workaholic:drive` §1) is `branching/scripts/sync-main.sh`, which refuses any dirty tree —
and `branching/scripts/check-workspace.sh`, the reader it composes, counts an untracked path
as dirty. So every successful `/moderate` run leaves behind exactly the condition that stops
every later `[Implement]` tick before it reaches the survey, and the tick ends `pending`.

The loop cannot recover on its own: the rescue step, `branching/scripts/clear-proved-residue.sh`,
refuses `untracked_present` and states in its own header that an untracked file "is on no ref
and is never removed here" — the deletion is irreversible and no proof covers it.

Measured on this repository, 2026-09-17:

```
$ sync-main.sh
{"ok": false, "reason": "dirty_workspace", "branch": "main", "summary": "1 untracked"}
$ git status --porcelain
?? .workaholic/moderations/
```

This is the same shape as the 2026-09-08 mission
`clear-the-residue-the-base-already-holds-and-never-stop-silently` (21 ticks, ~100 minutes idle).

Add one line to `.gitignore`. `sync-main.sh` then passes and `/moderate`'s log stays in the
checkout exactly as its own contract intends. The server's checkout is currently unblocked by
a local-only `.git/info/exclude` line, which travels to no other checkout and to no clean
clone — the committed `.gitignore` line is what makes the fix real everywhere.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout
- `workaholic:implementation` / `policies/coding-standards.md` — style and structure conventions
- `workaholic:operation` — the delivery path must not stop on state the repository itself produces

## Key Files

- `.gitignore` — the only file this ticket changes; today it carries no `.workaholic/` entry
  except `.workaholic/leak-denylist`, so the tick-log directory is untracked and therefore dirty.
- `.workaholic/moderations/` — the directory `/moderate`'s `persist-log` step writes to and
  deliberately leaves untracked. Nothing here is committed; `git ls-files` on it is empty on
  `main`.
- `.git/info/exclude` — carries the same line today as a **local-only** unblock. Not part of the
  fix and not to be relied on; it exists so this one checkout is not stopped meanwhile.

## Implementation Steps

1. **Reproduce the refusal before changing anything.** In a checkout whose
   `.git/info/exclude` does *not* carry the line (or with the line temporarily removed), run
   `/moderate` or simply `mkdir -p .workaholic/moderations && touch .workaholic/moderations/probe.md`,
   then `bash <plugin>/skills/branching/scripts/sync-main.sh`. Confirm
   `{"ok": false, "reason": "dirty_workspace", ... "summary": "1 untracked"}` and that
   `clear-proved-residue.sh` answers `{"ok": false, "reason": "untracked_present"}`. This step
   localizes the failure to the untracked path rather than to the freshen logic.
2. **Add the ignore entry** to `.gitignore`, in the `.workaholic` area beside the existing
   `.workaholic/leak-denylist` line, with the comment that says why it is deliberate:

   ```
   # /moderate writes its tick log here and deliberately leaves it untracked; without this
   # every sync-main.sh freshen refuses dirty_workspace and the [Implement] loop stalls.
   .workaholic/moderations/
   ```

3. **Confirm nothing tracked is newly ignored**: `git ls-files .workaholic/moderations` must be
   empty, so the entry hides no file that is already in the index.
4. **Re-run the probe.** With the tick log present and `.git/info/exclude` carrying nothing,
   `git status --porcelain` is empty and `sync-main.sh` answers `{"ok": true, ...}`.
5. **Remove the local-only workaround** from `/home/ec2-user/projects/qfs/.git/info/exclude`
   on this server once the committed line is on `main`, so the two do not silently diverge and
   so the committed line is what is actually being exercised.

## Quality Gate

**Acceptance criteria** — the checkable conditions that must hold:

- `.gitignore` carries `.workaholic/moderations/` with the comment explaining why.
- With a `/moderate` tick log present and no `.git/info/exclude` entry covering it,
  `git status --porcelain` reports nothing and `sync-main.sh` returns `ok: true`.
- No file that was tracked before the change becomes ignored: `git ls-files .workaholic/moderations`
  is empty.

**Verification method** — the commands/tests/probes that prove them:

- `git check-ignore -v .workaholic/moderations/probe.md` names the new `.gitignore` line.
- `mkdir -p .workaholic/moderations && touch .workaholic/moderations/probe.md && git status --porcelain`
  → empty.
- `bash <plugin>/skills/branching/scripts/sync-main.sh` → `{"ok": true, ...}` with the probe file
  still on disk.
- `git ls-files .workaholic/moderations` → empty.

**Gate** — what must pass before approval:

- The four probes above, run in a checkout whose `.git/info/exclude` does not carry the line.
- The repository's own gates are untouched by this change (it edits no code and no generated
  doc), so `cargo`/`check-all.sh` gates are unaffected; do not skip them if the branch carries
  anything else.

## Considerations

- **The reporter's proposed mechanism was checked, not assumed.** The `.gitignore` line is the
  reporter's proposal; this run confirmed the causal chain independently against the scripts —
  `check-workspace.sh` counts `??` rows as dirty, `sync-main.sh` maps that to `dirty_workspace`,
  and `clear-proved-residue.sh` refuses `untracked_present` by design. So the fix is being
  applied to a diagnosed cause, not to a reported symptom.
- **Why ignore rather than track.** Committing tick logs would make every `/moderate` run a
  base write, which the loop's own rules forbid for an unattended role; and `/moderate`'s
  `persist-log` step states the untracked-in-checkout behaviour as intent. Ignoring is the
  change that leaves both contracts as written.
- **Why not widen the scripts instead.** `sync-main.sh` refusing an untracked tree and
  `clear-proved-residue.sh` never deleting untracked files are both deliberate safety
  properties, and they live in the `workaholic` plugin repository rather than here — changing
  either from this repository is out of reach and would trade a one-line fix for a weakened
  guarantee everywhere.
- **Out of scope, observed in the same tick**: uncommitted `npm init -y` boilerplate in
  `package.json` (`main: index.js`, `license: ISC`, empty `keywords`/`author`) also stopped the
  freshen, as `divergent_residue`. It was stashed, not discarded
  (`git stash list` → `drive-residue: npm-init boilerplate ...`). Whoever intended that change
  should pop and commit it; this ticket does not touch it.
