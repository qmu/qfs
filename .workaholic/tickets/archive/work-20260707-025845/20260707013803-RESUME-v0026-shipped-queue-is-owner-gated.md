---
created_at: 2026-07-07T01:38:03+09:00
author: a@qmu.jp
type: housekeeping
layer: [Infrastructure]
effort: 0.1h
commit_hash: 96829cf
category: Changed
depends_on: []
---

# RESUME checkpoint — v0.0.26 shipped; the todo queue is entirely owner-gated

**This is a `/carry` resumption checkpoint, not a build ticket.** It records session position so a
fresh session picks up the next action without re-deriving it. Delete this file once the queue's
next-build decision is made (owner picks one of the paths below and it lands on its own branch).

## Where things stand (verified 2026-07-07 01:38 JST)

- **v0.0.26 is fully shipped and production-confirmed.** PR **#24** merged to `main` (merge commit
  `0b55482`); tag `v0.0.26` pushed; release workflow run `28795103779` finished **success** (6m2s);
  GitHub Release `v0.0.26` is published (`isDraft: false`) with all four native tarballs
  (`x86_64`/`aarch64` × `linux-musl`/`apple-darwin`) plus their `.sha256` checksums. `install.sh`
  consumes them. Nothing about v0.0.26 remains to do.
- v0.0.26 content: the dependency-posture overhaul (dead `anyhow` dropped, `serde_yaml` →
  `serde_yaml_ng`, per-dependency decision log in `docs/blueprint.md §11`). No CLI/grammar/registry
  change → **no plugin re-version** was owed. Story `.workaholic/stories/work-20260706-204906.md`,
  release note `.workaholic/release-notes/work-20260706-204906.md`.
- Branch `work-20260706-204906` is merged; `main` is at `0b55482`. Start the next build from a
  **fresh main-based branch** (`git fetch origin main` first).

## THE NEXT ACTION — the todo queue is entirely owner-gated (nothing is autonomously driveable)

Four tickets remain in `.workaholic/tickets/todo/a-qmu-jp/`. Driveability map (still accurate):

| ticket | state | why it can't just be `/drive`n |
| --- | --- | --- |
| `20260706175249-multi-oauth-app-per-provider` | **HERMETIC but large** | The only truly driveable one. Taught-surface **hard break** → needs the **four plugin `version` fields** bumped + `gen-docs`/`gen-skills` regen + **migration #12**. Do it on its **own** main-based branch. Design fork 2(a)/2(b) is **pre-resolved** by the ticket: persist the account→app binding **(a) on the account/consent record** as primary, add **(b) `path_binding.app` per-mount override** as optional. Confirm 2a/2b with owner at `/drive` approval, then implement. |
| `20260630203090-cf-live-d1-kv-queue` | **LIVE-BLOCKED** | Acceptance needs the owner to paste a live Cloudflare API token + account id. Logic is mock-testable but cannot be closed without live creds. |
| `20260706183441-postgres-value-round-trips` | **LIVE-BLOCKED** | `postgres::Row` is not hermetically constructible; acceptance is a live-PG `SELECT` of NUMERIC/TIMESTAMP/UUID/JSON. Needs the owner's live Postgres. |
| `20260706120400-materialized-view-refresh-last-run` | **DESIGN-BLOCKED** | No refresh engine exists yet; execution model + cache read-path + minimal-vs-full are an owner design decision. Needs a design brief before coding. |

So the realistic next build is **multi-oauth on its own branch** (owner confirms 2a/2b at the
approval gate), or a **live-credential session** for cf/postgres, or a **design brief** for
materialized-view refresh. Ask the owner which to take; do not autonomously drive a live/design
blocked one.

## Environment gotchas (save the next agent the pain)

- **Bash CWD persists across calls and is NOT the repo root after a `cd`.** The ship gate ran
  `cd packages/qfs`, so later relative paths like `.workaholic/...` silently resolved under
  `packages/qfs` and appeared "missing." Use **absolute paths** or `git -C /home/ec2-user/projects/qfs`
  for repo-root operations; don't trust a bare relative path after any `cd`.
- **cargo/rustc are not on PATH by default** — `source ~/.cargo/env` first.
- Build with `CARGO_INCREMENTAL=0` and redirect `TMPDIR` to the scratchpad
  ([[build-host-tmpfs-and-rm-trash]]). Disk was healthy this session (~52G free on `/`), but
  `packages/qfs/target` (~24G) and podman storage (~19G) are the swellers; `podman image prune -a -f`
  is the endorsed reclaim if it returns. **Do NOT `cargo clean`** the shared target casually
  ([[shared-tree-concurrent-drive]]).
- `rm` is trash-aliased ([[build-host-tmpfs-and-rm-trash]]); use `command rm` when a real delete is
  needed.
- **Full gate is green on `main` as of v0.0.26**: `fmt`, `build`, `clippy -D warnings`, `test`
  (workspace, exit 0), and all three anti-drift gates (`gen-docs`/`gen-skills`/`check-migrations`).

## Considerations

- Capture-only: this checkpoint made **no** code/commit/archive change. It supersedes and replaces
  the prior RESUME ticket `20260706211515` (whose only next-action — `/report` then `/ship` — is now
  done); that stale file was removed when this one was written.
- v0.0.26's own recorded follow-ups (opportunistic, unticketed by choice): `reqwest`/`tokio` feature
  trim and `tracing-subscriber` slim — see `docs/blueprint.md §11` and the archived ticket
  `20260706194536`. Spin into fresh tickets only if wanted.

## Final Report

Archived as a resolved checkpoint. The next-build decision recorded here was acted on in this night
drive: the hermetic multi-OAuth ticket was implemented and archived on branch
`work-20260707-025845`. The remaining live/design-gated tickets stay in todo with their blockers.
