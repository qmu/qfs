---
created_at: 2026-07-06T19:45:36+09:00
author: a@qmu.jp
type: enhancement
layer: [Infrastructure]
effort: 4h
commit_hash: 09739aa
category: Changed
depends_on: []
---

# Dependency posture overhaul — Conservative Vendor Dependence audit + decision log

This ticket **is** the decision-making snapshot for the dependency audit run on 2026-07-06. It is
deliberately a *temporary* record: the durable outcome (a per-dependency decision log) lands in
`docs/blueprint.md §11 Build decisions`, which is the one living design doc — no parallel/ADR pile
(the repo retired numbered ADRs; git holds the history). When this ticket is executed and archived,
the reasoning survives in git and the settled decisions survive in the blueprint.

Policy anchor: **Conservative Vendor Dependence** (workaholic `design/vendor-neutrality` +
`implementation/vendor-neutrality`) — *implement by default; rely on external only when a criterion
is clearly met; and for every taken dependency leave a **decision log** (Reason / Assessment /
Monitoring plan / Exit strategy) in the repo.*

## The finding (measured, not assumed)

- Internal crates: **47**. Direct third-party deps: **30** (`tempfile` is dev-only → **29** shipped).
  Cargo.lock total packages: **464** (~377 third-party once transitive).
- Structurally qfs already **nails the ACL half** of the policy: the `driver-*` crates are the
  anti-corruption layers, the pipe-SQL language is the domain vocabulary, and `tokio`/`futures`/
  `async-trait` are confined to `qfs-runtime` (enforced by `qfs-plan`'s purity dep-closure test).
- The real gap is **not "too many deps"** — it is that the ~15 load-bearing deps have **no decision
  log**, and a few hygiene items are outstanding. The highest-value move is to *document* the
  load-bearing set and act on the two genuine reduction signals, not to shave convenience crates.

## The four buckets (policy mapping)

- **A — error-is-fatal / far-from-core** (basis 1): `argon2 chacha20poly1305 p256 rand zeroize` (crypto),
  `time` (TZ), `keyring` (OS keychain), `rustix` (safe syscalls under `unsafe_code=forbid`).
  → **Cannot and must not self-implement.** Log only.
- **B — interop / protocol conformance** (basis 2): `reqwest` (HTTP/TLS), `url` (WHATWG),
  `rusqlite`/`postgres`/`mysql` (DB drivers = the target services themselves), `csv` (RFC 4180).
  → **qfs's reason to exist.** Log only.
- **C — cost/time efficiency** (basis 3): `serde`/`serde_json`/`serde_yaml`, `clap`, `toml`,
  `tracing`/`tracing-subscriber`, `tokio`/`futures`, `winnow`. → **Work one by one** (below).
- **D — ergonomics**: `thiserror`, `async-trait`, `base64`, `bytes`, `rpassword`.
  → **Park for now** (rationale below); the only one that will ever cleanly leave is `async-trait`,
  and that is a *wait-for-language*, not a rewrite.

## C — per-item verdicts (the "one by one" set)

| dep | measured | verdict |
| --- | --- | --- |
| `serde` / `serde_json` | 28 / 47 crates; the Rust serialization std | **KEEP + log.** Near-std tier; self-impl not worth it. |
| **`serde_yaml`** | `crates/codec` only | **REPLACE — the one clear C action.** Upstream is archived/unmaintained (dtolnay, 2024): a live "development status / sustainability" red flag. Swap to a maintained fork (`serde_yaml_ng`) or drop the YAML codec. Low effort, bounded to one crate. |
| `toml` | config/codec | **KEEP + log.** Rust-team-adjacent, actively maintained; low self-impl benefit. |
| `clap` | confined to `qfs-cmd`; 9 derive + 44 builder sites | **KEEP + log the seam.** Substantial CLI surface, but the whole surface is isolated to one crate → bounded exit. Not worth self-impl. |
| `tracing` / `tracing-subscriber` | **87 flat events vs 9 spans**; subscriber only at binary edges (`cmd`, `runtime`) | **EVALUATE / SLIM (genuine reduce candidate).** Flat-heavy usage means a minimal formatter could replace `tracing-subscriber`; keep the 9 spans (server observability). Medium effort, opportunistic. |
| `tokio` / `futures` | confined to `qfs-runtime` | **KEEP.** Writing an async runtime is basis-1 territory. Lever = **trim tokio features**, not removal. |
| `winnow` | `qfs-parser` private grammar; tiny transitive footprint | **KEEP + surface the existing decision.** Core-domain parser; the make/take call already exists (t02 spike, blueprint §11). Featherweight, so the supply-chain argument to drop it is weak. Optional long-term: hand-rolled recursive descent for full domain ownership — not urgent. |

## D — per-item verdicts (recommend: do NOT work on D yet)

| dep | measured | verdict |
| --- | --- | --- |
| `thiserror` | 32 crates | **KEEP.** std-tier reliability; self-impl = `Display`/`Error` boilerplate across ~30 crates for ~zero gain. |
| `async-trait` | **61 `dyn …Driver` sites**, 6 crates | **DEFER (monitored exit).** Native `async fn` in traits is stable, but `dyn` dispatch of async traits is still not ergonomic. Exit = remove when native dyn-async-trait matures. No action now. |
| `base64` | **110 sites** across `exec`/`oauth`/`qfs` | **KEEP for now.** Self-impl is small but 110 call sites + correctness adjacent to OAuth/secrets make it a poor "starter". |
| `bytes` | http/async buffers | **KEEP.** Present transitively via `hyper`/`reqwest` regardless — dropping the direct dep does not shrink the tree. |
| `rpassword` | TTY no-echo password read | **KEEP.** Cross-platform terminal handling = OS-compat, which leans **basis 1** ("ride the specialists"). Consistent with policy, not against it. |

**Bottom line on D:** every item is std-tier, blocked-on-language, entrenched+sensitive,
free-anyway, or actually-basis-A. Realistic crates removable today ≈ 0. Revisit opportunistically.

## Implementation steps

1. **Free win — remove the dead `anyhow` entry** from `packages/qfs/Cargo.toml`
   `[workspace.dependencies]` (declared but **no member opts in**; verified 2026-07-06).
2. **Write the decision log into `docs/blueprint.md §11`** — extend the existing make/take summary
   into a proper per-dependency log (Reason / Assessment / Monitoring / Exit) for the load-bearing
   set: crypto cluster (A), `reqwest` + `rusqlite`/`postgres`/`mysql` (B), `winnow`, `tokio`,
   `serde`. This satisfies the policy's "leave it in the repository" requirement.
3. **`serde_yaml` exit** — replace with `serde_yaml_ng` (drop-in) or drop the YAML codec; record the
   exit in the log. This is the single clearest C reduction.
4. **`reqwest` + `tokio` feature audit** — trim default features (e.g. TLS backend selection,
   `blocking`, unused tokio features) to shrink the ~377-crate transitive tree. This is the *real*
   supply-chain lever, far more than hand-writing convenience crates.
5. **`tracing` slim (evaluate)** — given 87:9 flat:span, prototype replacing `tracing-subscriber`
   with a minimal formatter while keeping the 9 spans; spin into its own follow-up if it pays off.
6. **Record `async-trait` as a monitored exit** (step 2's log) — remove when native dyn async
   traits are ergonomic. Explicitly **park** `thiserror`/`base64`/`bytes`/`rpassword` with the
   rationale above so a future reader doesn't re-litigate them.

Steps 1–3 are the core (this ticket, ~4h). Steps 4–5 are follow-ups that can each spin into their own
ticket (`/drive` scopes hermetic work; a feature trim is verifiable by `cargo tree` diff + green
`cargo test --workspace`).

## Key files

- `packages/qfs/Cargo.toml` (`[workspace.dependencies]` — `anyhow` removal, feature trims).
- `packages/qfs/crates/codec/Cargo.toml` + codec source (`serde_yaml` → replacement).
- `docs/blueprint.md` (`## 11. Build decisions` — the durable decision log lands here).
- `packages/qfs/crates/qfs/Cargo.toml` (reqwest/postgres feature surface).

## Considerations

- **Not a hard break of any taught surface** — no CLI/grammar/registry change, so no plugin
  re-version is required by these steps. (A `serde_yaml`→fork swap is invisible to the query
  language.) If the YAML codec is *dropped* rather than swapped, that removes a codec from the
  registry → MINOR + plugin version bump; decide swap-vs-drop in step 3.
- **Docs toolchain signal**: `docker compose up docs` reports 3 npm vulnerabilities (2 moderate,
  1 high) in the VitePress build chain — a *separate* supply chain from the shipped binary, but the
  policy's monitoring plan covers it. Note it; not in scope for the binary's log.
- **A/B are locked by policy** — do not open reduction work on crypto, TLS, TZ, OS-compat, or the DB
  drivers. Self-implementing them would violate the policy, not honor it.
- Experimental project: no backward-compat / deprecation-period design — a swap or drop is a clean
  hard change.
