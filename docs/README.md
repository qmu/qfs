# Documentation

Welcome to the qfs docs. Here's the map.

## Start here

- **[Installation](/guide/installation)** — get the binary (no credentials needed to start).
- **[Your first queries](/guide/getting-started)** — the describe → preview → commit loop, with
  actual command output.
- **[How qfs works](/guide/concepts)** — paths, the four archetypes, the pipe-SQL language,
  preview vs. commit, and cross-service joins.
- **[Current design snapshot](/guide/design-snapshot)** — how paths, mounts, accounts, OAuth app
  labels, `/sys`, DDL history, dump/restore, and automation fit together today.
- **[Chatwork + Drive benchmark](/guide/chatwork-benchmark)** — current declared-driver benchmark
  for reading My Drive, finding a Chatwork room, and previewing a Slack investigation request.

## The Cookbook

- **[Cookbook](/cookbook/)** — common tasks and the single statement that solves each, one per
  service: [Gmail](/cookbook/gmail), [Google Drive](/cookbook/gdrive),
  [databases](/cookbook/databases), [git](/cookbook/git), [GitHub](/cookbook/github),
  [Slack](/cookbook/slack), [files & object storage](/cookbook/files),
  [cross-service joins](/cookbook/cross-service), and [automation](/cookbook/automation).

## Use it

- **[CLI reference](/guide/cli)** — every command and flag (`run`, `describe`, `connection`, `skill`,
  `serve`).
- **[Interactive shell](/guide/shell)** — explore your services like a filesystem.
- **[Connections & credentials](/guide/connections)** — store credentials encrypted and scope access.
- **[Current design snapshot](/guide/design-snapshot)** — the operator and agent map of current qfs
  state, safety gates, and backup/recovery surfaces.

## Reference

These pages are **generated from the binary**, so they always match the version you have installed:

- **[Language reference](/language)** — the full grammar, keywords, and codecs.
- **[Driver catalog](/drivers)** — every service, its archetype, and exactly which verbs it
  supports.
- **[Server guide](/server)** — the `CREATE …` binding forms (triggers, jobs, endpoints, views,
  policies) and deployment targets.

## Deeper

- **Architecture Decisions** — the recorded technical choices behind qfs (see the ADR pages in the
  sidebar).
- **[Security](/security/threat-model)** — the threat model.

## Maintaining these docs

These pages follow the objective-documentation rule: a heading names a subject, command, resource,
state, or operation, and prose states observable behavior, constraints, and verification facts
rather than tone. Two `rg` scans keep that checkable in review (run from the repo root):

- **Heading scan** — flag slogans, rhetorical questions, emoji, reaction phrasing, or status
  decoration in headings:

  ```sh
  rg -n '^#{1,6}[[:space:]]' docs --glob '!**/dist/**' \
    | rg -i 'just shipped|see it|🚧|✅|🧭|\?"?$|— react'
  ```

- **Wording scan** — flag bare evaluative adjectives:

  ```sh
  rg -in --glob '!**/dist/**' \
    '\b(simple|powerful|safe|honest|real|easy|intuitive|magic|seamless)\b' docs
  ```

  A wording hit is allowed only when the same sentence states the concrete invariant, check, or
  command output that makes the claim verifiable — e.g. `retry-safe (re-running converges)`, or
  `safe` tied to the `preview → --commit → --commit-irreversible` gate. Otherwise rewrite it to the
  observable fact.

`docs/language.md`, `docs/drivers.md`, and `docs/server.md` are generated from the binary
(`cargo run -p xtask -- gen-docs`); fix offending prose in `crates/qfs/src/docs.rs` and regenerate,
never by hand-editing the generated file. The `docs/cookbook/*.md` articles are the source for the
Agent Skills (`cargo run -p xtask -- gen-skills`); edit the article and regenerate.
