# Current design snapshot

This page is the current operating model for qfs. Generated references still own the exact grammar,
driver catalog, and server binding tables; this page explains how those surfaces fit together.

## Mental model

qfs presents services as absolute paths and runs one pipe-SQL language over them. A path can point at
a local file, a SQL table, a Git repository, a mailbox, Drive, GitHub, Slack, object storage, or qfs's
own `/sys` administration surface.

The safety loop is always:

1. `qfs describe <path>` inspects a path offline.
2. `qfs run "<query-or-effect>"` previews a read or write plan.
3. `qfs run "<effect>" --commit` applies the plan.

Irreversible effects, such as sending mail, merging a pull request, or destructive deletes, require
the extra irreversible acknowledgement. Preview is a plan projection, not an apply dry-run: an effect
can preview cleanly and still fail at commit if its live credential, policy, or backend is unavailable.

## State stores

qfs keeps configuration state in two SQLite stores.

| Store | Owns | Examples |
| --- | --- | --- |
| **Project DB** | Project-local bindings and credential references | `path_binding`, account consent references, secret-store metadata, defined paths from `qfs connect` / `CONNECT` |
| **System DB** | Host/operator administration and global qfs configuration | `/sys` rows, declared drivers, policies, settings, billing labels, DDL/config event history, audit metadata |

The encrypted vault is credential storage, not a configuration export. qfs state rows may reference a
credential as `vault:provider/account`, but token values and OAuth client secrets stay outside dumps
and logs.

## Defined paths and mounts

A path exists because it is either a built-in source (`/local`, `/sys`) or a defined path you bind.
The CLI and query-language forms are twins:

```sh
qfs connect /mail --driver gmail --account you@example.com --app qmu
qfs connect /sql/app --driver sqlite --at file:app.db
qfs connect /github --driver github --account work
```

```qfs
CONNECT /mail TO gmail ACCOUNT 'you@example.com' APP 'qmu'
CONNECT /sql/app TO sqlite AT 'file:app.db'
CONNECT /github TO github ACCOUNT 'work' HOST 'local'
```

The mount path is the qfs routing name. The driver is the service implementation. Google Drive uses
the public driver kind `gdrive`; internally its file namespace is the Drive namespace, so a mount
named `/gdrive` still routes as `/gdrive/...` while the Drive driver parses the inner path.

The mount carries the account. There is no active account selector:

```sh
qfs connect /drive      --driver gdrive --account you@example.com
qfs connect /work/drive --driver gdrive --account teammate@work.com
```

Both mounts can exist in the same process, and the query path decides which account is used.

## Accounts and OAuth apps

A service account is the external account qfs may act as. A Google OAuth app is the client
registration used to obtain consent for that account. Google apps are labeled, so multiple app
registrations for the same provider can coexist:

```sh
qfs init you@example.com
cat credentials.json | qfs app add google qmu
qfs account add google --app qmu
printf %s "$REFRESH_TOKEN" | qfs account add google you@example.com --app qmu
qfs connect /mail --driver gmail --account you@example.com
```

For non-Google services the token is still read from stdin and sealed under the account label:

```sh
printf %s "$GH_TOKEN" | qfs account add github work
qfs connect /github --driver github --account work
```

The in-language account declaration records consent and selectors only. Secrets stay out of the
statement text:

```qfs
CREATE ACCOUNT google 'you@example.com' APP 'qmu'
CREATE ACCOUNT github 'work'
```

A Google account consent can serve Gmail, Drive, and Analytics. The consent row records which app
label minted or services that account; a mount can override with `--app` / `APP` when needed.

## `/sys` administration

`/sys` exposes current qfs administration state as queryable rows. It is the operator-facing current
state, not a credential reader. Important nodes include:

| Path | Purpose |
| --- | --- |
| `/sys/paths` | Defined path bindings written by `qfs connect` / `CONNECT` |
| `/sys/accounts` | Account consent metadata, never token values |
| `/sys/settings` | Runtime settings |
| `/sys/policies` | Least-privilege policy rows |
| `/sys/billing` | Plan labels and subscription state |
| `/sys/drivers` | Declared drivers, types, views, and maps |
| `/sys/audit` | Bounded audit metadata |

Writes to System DB-backed configuration rows append DDL/config history in the same transaction.
Those events are hash-chained metadata: they record what changed, who acted, when, the target path,
the verb, and a secret-redacted payload.

## DDL history, dump, and restore

qfs keeps both the current-state snapshot and an append-only DDL/config event trail. This is
event-sourcing style history for configuration changes, not a database schema migration tool with
mandatory `down` migrations.

```sh
qfs dump > qfs-state.jsonl
qfs dump --include-events > qfs-state-with-events.jsonl
qfs restore qfs-state.jsonl
qfs restore qfs-state.jsonl --commit
cat qfs-state.jsonl | qfs restore - --commit
```

`qfs dump` emits deterministic, secret-free JSONL records for current qfs configuration: declared
drivers, settings, policies, billing labels, and defined path bindings. `--include-events` appends
the historical DDL event rows after the current snapshot.

`qfs restore` previews by default. A committed restore replays supported current-state records into
the local System/Project DBs and records new local audit/DDL events. Historical dumped `ddl_event`
rows are external provenance; restore does not import them into the local hash chain.

Credential values are deliberately excluded. Restoring a state dump can recreate references such as
`vault:provider/account`, but the encrypted vault backup must be handled separately.

## Declared drivers and automation

The generated [language reference](/language), [driver catalog](/drivers), and [server guide](/server)
own the detailed forms. The current design groups them this way:

| Surface | Design role |
| --- | --- |
| `CREATE DRIVER` | Declare an HTTP-style integration endpoint |
| `CREATE TYPE` | Define a row shape for declared integrations |
| `CREATE MAP` | Map qfs verbs or calls to declared-driver effects |
| `CREATE VIEW` | Save a query surface over paths |
| `CREATE JOB` | Save a named plan and intended cadence |
| `CREATE TRIGGER` | Save an event-driven plan |
| `CREATE ENDPOINT` / `CREATE WEBHOOK` | Expose a saved plan over HTTP input |

qfs is not an internal scheduler. A job row is a saved plan plus cadence metadata; an external
scheduler invokes it with `qfs job run ...`, and the same preview/commit/policy gates apply.

## Operational gates

The docs should treat these boundaries as product behavior:

- `describe` and preview are credential-free, offline plan surfaces.
- Reads and committed writes to cloud services need a connected mount and a usable account.
- A mount whose credential is missing, revoked, locked, or unauthorized fails closed before a secret
  is decrypted.
- A driver without a live commit facet can still preview write effects, but `--commit` fails with an
  actionable capability error.
- Live-only acceptance paths, including external providers such as Cloudflare and Postgres, require
  real credentials and network access; local parser or preview tests do not prove those paths apply.
- Secrets are read from stdin, a TTY prompt, the encrypted vault, or runtime environment references,
  never from qfs statement text or generated documentation.
