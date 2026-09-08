---
name: qfs
description: Use when the user asks to use QFS or its connected services. Discover the live connections and available paths before reading or writing through the qfs CLI; covers pipe-SQL, preview, and commit. For Slack channel discovery and threads, also read qfs-slack.
---

# Driving services with qfs

`qfs` exposes every external service as a **filesystem of paths** queried with **one small pipe-SQL
language**. Mail is `/mail/inbox`, a database table is `/sql/pg/orders`, a repo's pull requests are
`/github/acme/web/pulls`, a Drive folder is `/drive/Reports`, a bucket object is `/s3/bucket/key`.
The same grammar reads, joins, transforms, and writes across all of them.

Prefer **one-shot** commands (`qfs run '<statement>'`, `qfs describe <path>`) — each runs once and
exits, which is what you want as an agent. The interactive shell (`qfs` with no args) exists but you
generally won't use it.

## Start with the installed environment

This plugin supplies instructions, not a fixed inventory of connected services. Its examples and
the binary's `qfs skill` describe shipped capabilities; the user's installed declarations can add
paths that are absent from those examples or this repository.

1. Run `qfs --version` and `qfs connect --list`. Use the actual connection path and its account;
   example names such as `/slack` or `/mail` are not guaranteed mounts. If accounts and mounts
   appear inconsistent, `qfs account list` lists account labels without revealing tokens.
2. Run `qfs describe <connection> --json`. Follow its `children`, substituting concrete values for
   parameter segments, and describe the parent namespace before selecting a leaf. For Slack,
   read [qfs-slack](../qfs-slack/SKILL.md) and describe `<connection>/<workspace>`: that is where
   public and private channel collections can be discovered.
3. If a capability seems absent, inspect the **installed** declarations through
   `qfs describe /sys/drivers --json` and a targeted `SELECT name, kind, body` over `/sys/drivers`.
   Repository assets are not the installed registry. Do not create or replace a declaration merely
   because it was missing from an example.

An empty result means no matches in the collection actually queried. Before reporting that a
resource is unavailable or requesting its ID from the user, check the relevant sibling collections,
the listing's filters/pagination, and the exact error. A public-only listing cannot establish that
a private resource is absent; a generic evaluation error does not establish a permission failure.

## Prerequisites (check these first)

- **The binary.** Use an installed `qfs`, or build it: `cd packages/qfs && cargo build --release`
  → `packages/qfs/target/release/qfs`. `qfs --version` confirms it runs.
- **Existing setup.** Live reads and writes need the service's authorized account. A compiled
  `describe` can work offline even when a write preview fails because its path is not mounted.
  Previewing a write is not a substitute for connection setup. Reuse the existing account and
  unlock session; do not initialize or reconnect an already working environment. For new setup,
  the user authorizes an account once
  and mounts it at a path (ADR 0008: the mount carries the account). This needs
  **`QFS_PASSPHRASE`** exported first — the master passphrase that unlocks the local encrypted
  vault (an argon2id KDF over the at-rest store, NOT a service credential) — and reads the
  credential VALUE from stdin, never argv (argv leaks into the process table + shell history):

  ```sh
  read -rs QFS_PASSPHRASE; export QFS_PASSPHRASE          # unlock the vault, no shell-history leak
  qfs init you@example.com                                 # one-time: vault + operator identity
  printf %s "$TOKEN" | qfs account add github work         # token via stdin, never argv
  qfs connect /github --driver github --account work       # the mount carries the account
  ```

  A valid unlock mechanism (session, keychain, or `QFS_PASSPHRASE`) must be available. Labels are
  safe to print; the secret is never echoed. `qfs account list` shows authorized accounts and
  `qfs connect --list` the mounted paths. Two accounts of one service coexist as two mounts
  (`/mail` and `/mail2`).

## The loop (do this for every task)

1. **`qfs describe <path>`** — learn the node's archetype, columns, supported verbs, `CALL`
   procedures, and which filters push down. Pure: no creds, no network. **Always read this first.**
2. **Write a statement** against what describe told you.
3. **`qfs run '<statement>'`** — reads execute immediately; writes **preview by default**, printing
   the effect-plan (paths, affected counts, and an `irreversible` flag) before committing effects.
4. **Add `--commit`** to apply, once the preview looks right.

With a configured `/mail` mount:

```sh
qfs describe /mail/drafts --json                 # 1: the contract
qfs run "insert into /mail/drafts values ('alice@example.com','Hi','Body')"          # 2+3: PREVIEW
qfs run "insert into /mail/drafts values ('alice@example.com','Hi','Body')" --commit # 4: apply
```

## Path model

- Paths are **absolute** (start with `/`); there is no working directory in one-shot mode.
- A node belongs to one of four **archetypes**, which decides its verbs (`qfs describe` shows them):
  - **blob namespace** (files: local, S3/R2, Drive) — `SELECT`, `UPSERT`, `REMOVE`
  - **relational table** (SQL DBs, D1) — `SELECT`, `JOIN`, `INSERT`, `UPDATE`, `UPSERT`
  - **append log** (mail, Slack, queues) — `SELECT` (read tail), `INSERT` (append)
  - **object graph** (GitHub, Linear) — `SELECT`, `INSERT`, `UPDATE`, `REMOVE`, `CALL`
- **Using an unsupported verb is rejected up front** with a structured error listing the supported
  set — so describe, then pick a supported verb.
- Some paths take a coordinate, e.g. git: `/git/repo@v1.2/src/main.rs` reads as of a ref.
- `SELECT/INSERT/UPSERT/UPDATE/REMOVE` are the only verbs. `ls`/`cp`/`mv`/`rm` are just shell
  aliases for them (`ls`=SELECT listing, `cp`=UPSERT, `rm`=REMOVE) — interactive shell only.

## The query language (syntax)

A statement is a **source** followed by **stages** joined by `|>` (a pipe). Write multi-stage
statements one stage per line:

```qfs
/sql/pg/orders
|> where total > 100 AND status IN ('open', 'pending')
|> select id, total, status
|> order by total DESC
|> limit 5
```

The **source is a leading `/path`** (a `LET`-bound name also works); there is no `FROM` keyword — it
was removed from the closed core. Bind with `=`, compare with `==`. Read/transform stages:
`where <cond>` (`==`, `<>`, `<`, `>`, `<=`, `>=`, `LIKE`, `IN`, `BETWEEN`, `AND`/`OR`),
`select <cols>` (rename with `as`), `extend <col> = <expr>`, `join <path> on <cond>` (works
**across services**), `aggregate <fn>(<col>) as <name>` (+ `group by`), `order by <col> [DESC]`,
`limit <n>`, `distinct`, and `union`/`except`/`intersect <path>`.

Effect (write) stages and statements:

```qfs
insert into /slack/acme/general/messages values ('Deploy done')
upsert into /drive/my/Reports/report.pdf values ('…bytes…')   -- retry-safe blob write
update /sql/pg/orders set status = 'shipped' where id == 7
remove /mail/inbox where subject LIKE '%spam%'             -- REMOVE takes a path + WHERE
/github/acme/web/pulls/42 |> call github.merge(method => 'squash')
```

Codecs convert formats — `decode`/`encode` with `json`, `jsonl`, `yaml`, `toml`, `csv`, `md`:

```qfs
/local/config.json
|> decode json
|> encode yaml
```

Cross-service join (qfs pushes each side's filters down, then joins locally):

```qfs
/sql/pg/orders
|> join /github/acme/web/issues on id == issue_id
|> select id, title
```

See `docs/cookbook/` in the repo for many more recipes.

## JSON output (prefer this as an agent)

`qfs` prints a human table on a TTY and JSON when piped; force it with `--json` (or
`--format json`). Parse the JSON rather than scraping the table:

```sh
qfs describe /mail/drafts --json | jq '.verbs, .procedures'
qfs run "/sql/pg/orders |> where total > 100 |> select id" --json
```

A preview's JSON includes `preview.rows` (each with `verb`, `target`, `affected`, `irreversible`),
`total_affected`, and `committed: false`. After `--commit`, `committed: true`.

## Safety model — preview vs. commit (non-negotiable)

- Writes through `qfs run` **preview by default**; reads execute immediately. Apply a write only
  when the user's request authorizes it, using `--commit` after checking its preview.
- **Irreversible effects** — sending mail (`CALL mail.send`), merging a PR (`CALL github.merge`),
  deleting/trashing (`REMOVE`) — are flagged `irreversible: true` in the preview. In a one-shot,
  applying them needs **both** `--commit` and `--commit-irreversible`; without the extra flag qfs
  refuses (fails closed). Treat these as gates: preview, confirm intent, then commit.
- `UPSERT` is the retry-safe default for writes (create-or-replace; re-running converges).

```sh
# Reversible: --commit is enough
qfs run "insert into /mail/drafts values ('alice@example.com','Hi','Body')" --commit
# Irreversible: needs the explicit ack
qfs run "/mail/drafts |> call mail.send" --commit --commit-irreversible
```

## Error / exit contract (for scripting)

- Exit `0` = success. Non-zero = failure; the error body goes to **stderr** as JSON with a `kind`:
  `parse` (bad syntax), `usage` (e.g. a relative path), `capability` (unknown/unsupported
  source or verb), `auth`, `internal`.
- A `capability` error on a read often just means no account/backend is connected for that
  service yet — the statement's syntax is fine.
- Credentials never appear in any output (not in `describe`, not in logs, not in errors).

## Gotchas

- Statements use **absolute paths** in one-shot mode — no `cd`, no relative paths.
- `remove` is `remove <path> where …`, not a pipe stage.
- Don't reach for a verb describe didn't list (e.g. `update` on an append log) — it's rejected.
- Keywords are **canonical lowercase** (recognition is case-insensitive, but write lowercase). There
  is **no `from`** — a leading `/path` is the source. Bind with `=`, compare with `==`.
- Quote interval literals: `create job nightly every '1h' do …`.
- Server bindings (`create endpoint|trigger|job|view|policy`) are statements too — preview them to
  see the exact plan they'd install before `qfs serve` runs them.

## Beyond the loop (the shipped surface)

- **Three faces, one engine.** The same describe → preview → commit loop is reached over the **CLI**,
  the **MCP endpoint** (exposed as MCP tools), or the **web dashboard** whose approval cards route a
  pending irreversible commit to a human for sign-off. Same grammar, same gates everywhere.
- **`/sys/*` is administration as paths.** Query the deployment's own state with the same grammar:
  `/sys/{users,projects,audit,connections,policies,metrics,settings,billing}` (e.g.
  `/sys/audit |> order by seq desc |> limit 20`). `/sys/audit` is append-only + hash-chained;
  `/sys/connections` is names/metadata only.
- **Selectable safety mode** lives in `/sys/settings`, above the always-on floor (preview default +
  the irreversible ack) — you never bypass it; preview, then commit, and let the gate decide.
- **Teams:** `qfs invite create` → `qfs invite redeem` adds a member; a `POLICY` / the ACL (not
  membership) is what authorizes an action (default-deny).
- **Credential lifecycle:** `qfs account rotate|revoke` for offboarding, `qfs vault rekey` for key hygiene.
