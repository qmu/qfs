---
created_at: 2026-07-06T16:35:21+09:00
author: a@qmu.jp
type: enhancement
layer: [Config]
effort:
commit_hash: 52e52d4
category: Added
depends_on:
---

# qfs FAQ / operational-reference Agent Skill (`qfs-faq`)

## Overview

Add a **comprehensive operator FAQ / "how do I…" reference** as a new qfs Agent Skill so a Claude
Code agent can answer operational questions — *"how do I add a connection for the Google Drive of
`<account>`?"*, *"why is my connect blocked with `org_internal`?"*, *"what does PREVIEW mean?"* —
**directly from the skill, without reading qfs source code**.

The skill is authored the same way every other qfs skill is: a single human **cookbook article**
`docs/cookbook/faq.md` carrying `skill_name` / `skill_description` frontmatter, rendered verbatim
into `plugins/qfs/skills/qfs-faq/SKILL.md` by the **existing** `cargo run -p xtask -- gen-skills`
pipeline. No generator code changes are needed — `gen_skills.rs` already discovers any cookbook
article with the two frontmatter keys. The freshness guarantee for the *shell-command* answers
(which the existing recipe ratchet does **not** cover) is scoped into the dependent ticket
`20260706163522-faq-cli-surface-antidrift.md`; this ticket delivers the content + wiring and relies
on the existing machinery for what it already covers.

**Motivating incident (fold into content):** an operator tried to connect a second Google account
that lives in a **different Workspace organization** (a client's domain) and hit
`アクセスをブロック … 組織内でのみ利用可能です / エラー 403: org_internal`. That is an OAuth
consent-screen configuration, not a qfs bug — exactly the kind of "common wall" this FAQ must catch.

## Policies

The standard engineering policies (synced from qmu.co.jp into the `workaholic` policy skills) that
govern this ticket. The implementing session **MUST** read each linked hard copy before writing and
keep the change defensible against its Goal (目標) / Responsibility (責務) / Practices (実践).

- `workaholic:implementation` / `policies/directory-structure.md` — the article lands in
  `docs/cookbook/` beside its siblings and the generated skill under `plugins/qfs/skills/qfs-*/`;
  same role, same place, no ad-hoc location (applies to all code work).
- `workaholic:implementation` / `policies/coding-standards.md` — the minimal wiring (marketplace
  entry, version bumps) and any Rust touched follow house style; maximize compiler-caught errors
  (applies to all code work).
- `workaholic:implementation` / `policies/objective-documentation.md` — **core policy**: the FAQ must
  describe what the binary *actually* does, in verifiable language, reviewed in the same PR; the
  anti-drift ratchet is the machine-checkable embodiment of "documentation verifiable against the
  code stays current."
- `workaholic:implementation` / `policies/accessibility-first.md` — the FAQ is *another access path
  for AI agents*: a path an agent cannot reach is a defect, so structured, reachable operator
  guidance (not source-spelunking) is the reach this policy demands.
- `workaholic:implementation` / `policies/test.md` — the `cookbook_skills.rs` ratchet is a regression
  test that parse-checks every recipe against the real binary parser; the FAQ's `qfs` recipes pass
  through it, so it can never teach a statement the binary rejects.
- `workaholic:design` / `policies/self-explanatory-ui.md` — answers are framed in the operator's
  vocabulary ("add a Google Drive connection"), mapped to the real `init` / `account add` / `connect`
  surface, not restated internals.
- `workaholic:design` / `policies/modeless-design.md` — a FAQ that answers arbitrary "how do I…"
  questions keeps every operation reachable without forcing a fixed sequence.
- `workaholic:operation` / `policies/ci-cd.md` — freshness rests on the `--check` gate; it must be the
  same `cargo run -p xtask -- gen-skills --check` a developer and CI both run.
- `workaholic:implementation` / `policies/command-scripts.md` — regeneration/check go through the
  existing `xtask` verbs, the single reproducible entry point.
- **qfs-repo-local (`CLAUDE.md`)** — generated `SKILL.md` files are **never** hand-edited (edit the
  article, regenerate); a skill-affecting change bumps **all four** plugin `version` fields; every
  shipped PR bumps the qfs patch version.

## Key Files

- `docs/cookbook/faq.md` — **NEW**. The source article (frontmatter `skill_name: qfs-faq` +
  `skill_description`; body of operator-vocabulary FAQ entries). This is the only hand-authored file.
- `packages/qfs/xtask/src/gen_skills.rs` — the generator. Auto-discovers the new article
  (`collect_sources()` reads every `docs/cookbook/*.md` with the two frontmatter keys); constants at
  L30-37; the marketplace-registration check at L92-98. **No change expected** unless the FAQ needs
  behavior beyond verbatim copy.
- `.claude-plugin/marketplace.json` — register `"./skills/qfs-faq"` in `skills[]` (gen-skills
  `--check` fails until this is added); holds two of the four `version` fields (top-level + `plugins[0]`).
- `plugins/qfs/.claude-plugin/plugin.json` — third `version` field to bump.
- `plugins/qfs/.codex-plugin/plugin.json` — fourth `version` field (its `skills: ./skills/` is
  directory-level, so it auto-discovers the new skill dir — no per-skill entry needed).
- `packages/qfs/crates/qfs/Cargo.toml` — qfs patch version bump (per CLAUDE.md, every shipped PR).
- `packages/qfs/crates/test/tests/cookbook_skills.rs` — the verified-true ratchet
  (`every_cookbook_skill_recipe_parses`, floor `MIN_STATEMENTS=45`) that will now also parse-check the
  FAQ's `qfs` recipes.
- **Answer-source material (true, existing prose to draw from — do not link to source, restate for
  operators):** `docs/guide/connect.md` (per-service connect walkthrough), `docs/guide/getting-started.md`
  (describe→preview→commit loop, `unknown_source`, output formats), `docs/cookbook/gmail.md` `## Setup`
  (Google connect happy path), `docs/cookbook/gdrive.md` `## Setup` (mount Drive).
- **Source of truth for the CLI surface the FAQ documents (read to keep answers accurate, never quote
  source into the skill):** `packages/qfs/crates/cmd/src/lib.rs` (clap: `connect`, `account`, `app`,
  `init`, `auth`, `describe`), `packages/qfs/crates/parser/src/grammar.rs` (the in-language twins
  `CONNECT` / `CREATE ACCOUNT` / `CREATE CONNECTION`).

## Related History

The generation machinery, the plugin-version rule, and the connection surface this FAQ documents were
all built by prior tickets; this skill is a new content artifact riding that same, already-shipped
pipeline (moderation: **clear** — no duplicate or overlapping open ticket).

- [20260701173124-cookbook-articles-as-agent-skills.md](.workaholic/tickets/archive/work-20260629-110121/20260701173124-cookbook-articles-as-agent-skills.md) — built the `xtask gen-skills [--check]` pipeline + the verified-true ratchet (the machinery this reuses).
- [20260703150400-plugin-cache-staleness.md](.workaholic/tickets/archive/work-20260704-181053/20260703150400-plugin-cache-staleness.md) — established the "bump all four plugin `version` fields on a skill-affecting change" rule this ticket inherits.
- [20260703040000-create-account-language-surface.md](.workaholic/tickets/archive/work-20260705-173620/20260703040000-create-account-language-surface.md) — defines the `CREATE ACCOUNT` / `CONNECT` / `/sys/accounts` surface the FAQ's connection answers must reflect.
- [20260703150300-agent-facing-doc-gaps.md](.workaholic/tickets/archive/work-20260704-181053/20260703150300-agent-facing-doc-gaps.md) — agent-facing gotchas (`--json` envelope, exit codes, PREVIEW semantics) to fold into FAQ answers.
- [20260629111140-fix-skill-md-steers-ai-into-errors.md](.workaholic/tickets/archive/work-20260629-110121/20260629111140-fix-skill-md-steers-ai-into-errors.md) — prior fix where a skill taught commands the binary rejects; motivates holding every FAQ answer to the verified-true bar.

## Implementation Steps

1. **Author `docs/cookbook/faq.md`.** Frontmatter (flat two-key block, exactly like `gdrive.md`):
   - `skill_name: qfs-faq`
   - `skill_description:` a "Use when …" operator how-do-I / troubleshooting reference covering
     connection setup, the query-and-commit loop, common errors, and per-service routing.
2. **Write the FAQ body** in operator vocabulary. Cover the four areas (confirm scope at approval):
   - **Connection & account setup.** The happy path (`qfs init` → `cat credentials.json | qfs app add
     google` → `qfs account add google` → `qfs connect /drive --driver gdrive --account you@example.com`);
     the "mount carries the account" model; **connecting a second account at a second path**
     (`qfs connect /work/drive … --account other@example.com`); per-service variants (GitHub/Slack
     token on stdin, S3/R2 access keys, local SQL/git via `CREATE CONNECTION`).
   - **`org_internal` / cross-org troubleshooting** (the motivating incident). Symptom: connect blocked
     with `403 org_internal` / "組織内でのみ利用可能". Cause: the OAuth app's consent screen is
     **Internal**, so only the app-owning Workspace org's users may authorize; an account in a *different*
     org is refused. Fixes, ranked: (a) flip the consent screen to **External** + add the account as a
     **test user** — note the ~7-day refresh-token expiry for sensitive Drive scope in Testing, resolved
     by publishing to Production; (b) authorize via an OAuth client **issued by the target account's own
     org** (Internal there → works, no verification); (c) avoid cross-org auth by having the other org
     **share** the needed Drive items into an account you already connect. **State the current qfs
     limitation plainly**: `qfs app add` holds **one OAuth app per provider**, so a second Google
     credential cannot coexist with the first today (cross-links the enhancement, if opened).
   - **The query & safety loop.** `describe` runs offline (no credentials); reads return rows;
     writes **PREVIEW** by default and change nothing; `--commit` applies; `--commit-irreversible`
     for the gated ones; output formats.
   - **Common errors & fixes.** `unknown_source` (path not connected / fail-closed), the irreversible
     `(!)` gate, previews affecting nothing.
   - **Per-service quick-answer index.** One line each routing "how do I read/write X" to the existing
     `qfs-gmail` / `qfs-gdrive` / `qfs-databases` / `qfs-files` / `qfs-git` / `qfs-github` / `qfs-slack`
     skills.
   - **Fence discipline (ratchet-critical):** put runnable in-language statements
     (`CONNECT …`, `CREATE ACCOUNT …`) in ```` ```qfs ```` fences (they are parse-checked); put shell
     commands in ```` ```sh ```` fences; keep prose placeholders like `<account>` / `<msg-id>`
     **outside** ```` ```qfs ```` fences so they don't fail `cookbook_skills.rs`.
3. **Regenerate** the skill: `cd packages/qfs && cargo run -p xtask -- gen-skills` — writes
   `plugins/qfs/skills/qfs-faq/SKILL.md` and the `.claude/skills/qfs-faq` symlink. Never hand-edit the
   generated `SKILL.md`.
4. **Register** `"./skills/qfs-faq"` in `.claude-plugin/marketplace.json` `skills[]`.
5. **Bump versions:** all four plugin `version` fields (marketplace top-level + `plugins[0]`,
   `plugin.json`, `.codex-plugin/plugin.json`) — **minor** (a new taught surface) per CLAUDE.md — and
   the qfs patch in `packages/qfs/crates/qfs/Cargo.toml`.
6. **Verify** (see Quality Gate).

## Quality Gate

Proposed gate (the mandatory Step-4b interrogation was interrupted by a live incident; every item
below is objective and repo-real — **confirm/adjust at `/drive` approval**).

**Acceptance criteria** — the checkable conditions that must hold:

- A new skill `qfs-faq` exists (`plugins/qfs/skills/qfs-faq/SKILL.md` + `.claude/skills/qfs-faq`
  symlink + `marketplace.json` `skills[]` entry), generated from `docs/cookbook/faq.md`.
- A reviewer can answer **"how do I add a Google Drive connection (including a second / different-org
  account)"** and **"why is my connect blocked with `org_internal` and how do I fix it"** using
  **only** `plugins/qfs/skills/qfs-faq/SKILL.md`, without reading qfs source.
- Every ```` ```qfs ```` recipe in the article parses against the real binary; the `MIN_STATEMENTS`
  floor still holds.
- All four plugin `version` fields **and** the qfs patch version are bumped in the same PR.

**Verification method** — the commands/probes that prove them:

- `cargo run -p xtask -- gen-skills --check` exits 0 (SKILL.md up to date, symlink present,
  marketplace registered).
- `cargo run -p xtask -- gen-docs --check` exits 0 (no unrelated doc drift).
- `cargo test --workspace` green — in particular
  `cookbook_skills.rs::every_cookbook_skill_recipe_parses`.
- `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`.
- Manual: the two example questions answered from the SKILL.md alone (record in the PR).

**Gate** — what must pass before approval: all of the above green, plus the manual answerability
check, on the branch.

## Considerations

- **Cross-org / multi-tenant limit is real, document it as current truth.** `qfs app add` is keyed
  per provider (one Google app), so the "issue a credential in the client's org" approach (§ the
  incident, option b) cannot coexist with the existing account today. The FAQ states this as the
  present behavior; if the multi-app enhancement is opened, update this entry then — that update *is*
  the anti-drift model working. (`packages/qfs/crates/cmd/src/lib.rs` app model)
- **Do not hand-edit `plugins/qfs/skills/qfs-faq/SKILL.md`** — it is generated; edit the article and
  regenerate (`CLAUDE.md`).
- **Placeholders must stay out of ```` ```qfs ```` fences** or `cookbook_skills.rs` fails the build
  (`packages/qfs/crates/test/tests/cookbook_skills.rs`).
- **Answer content must be verified against the binary**, not from memory — the existing ratchet only
  guarantees the `qfs`-statement recipes; the shell-command answers' freshness is hardened by the
  dependent ticket `20260706163522-faq-cli-surface-antidrift.md`.
- **Codex plugin** needs no per-skill registration (directory-level `skills`), but its `version`
  field still bumps.
