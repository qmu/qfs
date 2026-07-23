---
created_at: 2026-07-22T09:04:00+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category: Changed
depends_on: [20260723100000-wire-read-by-path-mount-for-registered-views.md]
mission: a-file-collection-is-a-declared-set-over-any-blob-source
---

# The compiled /markdown driver retires on the ratchet

## Overview

Mission acceptance item 6. With the equivalence test green (depends_on), the specialized
compiled driver retires — the ratchet, not a leap:

1. Delete `crates/driver-markdown`'s **driver surface**. Its pure parser survives wherever the
   codec layer homed it in the previous tickets — delete the driver shell, not the
   interpretation.
2. `CONNECT … TO markdown` maps onto the registered-set shape, or is retired with the driver —
   whichever the declaration surface supports cleanly; record the choice in the mission
   changelog.
3. Regenerate everything derived: `cargo run -p xtask -- gen-docs` (drivers.md loses the
   compiled entry), `gen-skills` for any cookbook article that taught the `/markdown` surface —
   correct the articles first, regenerate after.
4. **Plugin version bump per CLAUDE.md**: a taught CLI surface moves, so bump all four plugin
   `version` fields in the same PR (minor — this is a taught-surface break).

Owner authorization (2026-07-22): deletion, regeneration, and the plugin bump are authorized
for the overnight run **provided the equivalence gate is green**. If the equivalence test is
not green, stop at the previous ticket — do not delete against a red or skipped oracle.

## Policies

- Hard break sanctioned; no deprecation shim, no compatibility alias for the retired paths.
- The retirement is gated on the ratchet: equivalence green first, deletion second, in that
  order within the ticket too.
- Nothing the viewer could do before is lost — the declared views are already proven
  row-equivalent when this ticket starts.

## Drive note (2026-07-23) — BLOCKED: equivalence gate not green

This ticket's `depends_on` (20260722090300) did NOT reach its registration-level equivalence
gate this run — that gate is disk-blocked (the declared-VIEW registration + equivalence test need
the full `qfs` binary build, which the shared host's free disk could not accommodate; see t3's
Drive note). Per this ticket's own guard ("If the equivalence test is not green, stop at the
previous ticket — do not delete against a red or skipped oracle") and the overnight authorization,
NOTHING here was done: `crates/driver-markdown` is untouched, no docs/skills regenerated, no
plugin version bumped. Unblock only once t3's registration-level equivalence test is GREEN.

## Drive note (2026-07-23, second leaf) — STILL BLOCKED, but for a DIFFERENT reason: no wired production replacement

t3's registration-level **equivalence gate is now GREEN** (commit `c6d834d`): the declared
`documents`/`links` views read row-equivalent to the compiled driver through the registration read
+ `/local` root-relative derivation, `DESCRIBE` matches, and `CREATE VIEW … AS /local/**/*.md |>
decode md.<relation>` desugars to a `/server/views` INSERT that rehydrates to the read body. So the
disk-blocked reason from the first leaf is resolved (builds now run in the tmpfs memory-cap wrapper).

**Deletion is still NOT done — deliberately, and it is the safe call.** t3's equivalence was proven
at the **registration-read helper** level (`qfs_exec::read_registered_collection`, exercised by the
hermetic tests over a real `/local` scan). It is the NECESSARY oracle-green precondition, but it is
NOT a wired production surface: **nothing in the binary resolves a registered collection view BY
PATH yet.** Critically, the `/local` **root-relative strip lives only in the registration helper**,
NOT in the generic `decode md.<relation>` query path — which, per design-brief Ruling 3, deliberately
keeps the raw decode VFS-anchored. A viewer/agent running the bare pipeline `/local/docs/**/*.md |>
decode md.links` therefore gets VFS-anchored `source_doc` (`/local/…`) and `target_doc` (`local/…`,
no leading slash) that do **not** self-join — so the raw generic path is NOT a drop-in replacement,
and the materialized-view refresh path (`view.rs` → `block_on_read`) does not apply the strip either.

Retiring the compiled `/markdown` driver now would remove the ONLY wired documents/links-by-path
surface the viewer depends on, with no wired replacement — a regression this ticket's own guard and
the mission policy ("Nothing the viewer could do before is lost") forbid. So: `crates/driver-markdown`
is UNTOUCHED, no docs/skills regenerated, no plugin version bumped.

**To unblock (follow-up):** wire the registered collection view for **read-by-path** in the binary
(a `/collections/<name>` or `/markdown/<name>`-shaped mount whose read facet runs
`read_registered_collection` over the declared body's `/local` scan, applying the root-relative
strip), so a live query and `DESCRIBE` reach the declared views the way the compiled driver's
mount does today. Once that live surface is proven equivalent (not just the helper), the deletion +
`CONNECT … TO markdown` remap/retire + docs/skills regen + plugin MINOR bump are safe to land.

## Drive note (2026-07-23, third leaf) — RETIRED (equivalence gate was GREEN)

Ticket 1 (20260723100000, commit dcfd0b6) proved the LIVE `/collections/<view>` by-path surface
row-equivalent to the compiled `/markdown` driver on all four dimensions plus the `links`→`documents`
self-join, so the ratchet's precondition was met. Done this leaf:

- **Deleted the whole `crates/driver-markdown` crate** (driver shell + its now-redundant parser). The
  markdown interpretation already lives in the codec layer (`qfs-codec`, re-exported through
  `qfs-core`), so nothing was lost. Removed the `qfs-driver-markdown` dep from the binary, its
  `dep_direction` allowlist entry, `crate::markdown` (deleted `src/markdown.rs` + the `pub mod`), the
  `register_markdown_mounts` wiring in `shell.rs`, the `MarkdownDriver` registration in
  `describe.rs`'s compiled catalogue, and the `/markdown` catalog representative-node arm.
- **`CONNECT … TO markdown`: retired WITH the driver** (not remapped). There is no compiled or
  declared `markdown` driver id any longer, so a `CONNECT … TO markdown` no longer resolves — the
  sanctioned hard break. The replacement is an ordinary registered collection view reached by path
  through `/collections/<view>` (ticket 1). No deprecation shim, no alias.
- **Regenerated docs**: `docs/drivers.md` lost the `/markdown` entry (`gen-docs --check` back in
  sync). **Skills unchanged**: no cookbook article ever taught the `/markdown` query surface (the only
  `/markdown` mentions were the `text/markdown` MIME type in gdrive examples), so `gen-skills`
  regenerated byte-identical and `gen-skills --check` is green.
- **Plugin MINOR bump** (taught-surface break): all four `version` fields `0.14.1 → 0.15.0`
  (`plugins/qfs/.claude-plugin/plugin.json`, `plugins/qfs/.codex-plugin/plugin.json`, and both
  `version` fields in `.claude-plugin/marketplace.json`).

Gates (memory-capped tmpfs wrapper, per-crate): `qfs` full suite green after the doc regen (the
`committed_docs_match_generated_output` anti-drift test passes), `qfs-cmd` `dep_direction` green (19),
clippy `-D warnings` clean on `qfs`+`qfs-cmd`, fmt clean, `gen-docs --check` + `gen-skills --check` in
sync.

## Quality Gate

- `crates/driver-markdown` no longer registers a driver; the workspace builds without it.
- `cargo test --workspace` (the equivalence test now runs against the declared views alone),
  clippy `-D warnings`, `cargo fmt --all --check` pass.
- `gen-docs --check` and `gen-skills --check` pass after regeneration; no hand-edited
  generated file.
- All four plugin `version` fields are bumped in the same change.
