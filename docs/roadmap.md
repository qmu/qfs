# Where qfs is going next

This page is the **plan**, not a description of what works today. The [guide](/guide/concepts) and
[cookbook](/cookbook/) document only shipped behavior; this is the one page that talks about ideas
that are not built yet. Each item is tagged **✅ shipped** (available now) or **🧭 proposed** (planned).

## Orientation

qfs gives every external system **one address space and one query language**. A *path* names a
source, and you query it with a SQL-like, left-to-right pipe (`|>`). The sources qfs knows about:

| Path | What it is |
| --- | --- |
| `/local` | files and folders on your own computer |
| `/sql/<name>` | a SQL database (SQLite, Postgres, MySQL) you've connected |
| `/git/<name>` | a git repository on disk |
| `/mail` | Gmail |
| `/drive` | Google Drive |
| `/github`, `/slack` | GitHub and Slack |
| `/s3`, `/r2` | object storage — Amazon S3 and Cloudflare R2 |
| `/ga` | Google Analytics |
| `/sys` | qfs's own admin data (users, audit log, policies, settings) |

A few verbs you'll see below:

- **read** — run a query against a path and get rows back (`/sql/orders/customers |> where … |> select …`).
- **describe** — inspect a path's shape (its columns and which writes it allows) *without touching it*.
- **preview** — show exactly what a write *would* do, without doing it (qfs previews by default; you
  add `--commit` to actually apply).
- **connect** — give qfs the login a cloud service needs (a token / OAuth), stored encrypted.

## Shipped: the make-the-docs-true cycle

The product used to *describe* features that errored the moment a new user tried them. This cycle
wired the binary so the headline things actually run:

- **Convert a file's format in one line.** `/local/config.json |> decode json |> encode yaml` really
  turns that JSON file into YAML now (it used to silently do nothing).
- **Query a SQL database**, with the filtering done *inside* the database (`where total > 100` runs
  as real SQL), and **query a git repository** — its commit history, branches, tags, and the files
  in its latest snapshot.
- **Cloud sources fail closed with a connect hint.** Reading `/mail` or `/github` with no usable account now returns
  a clear, actionable *"connect a … account — run `qfs account add …`, then `qfs connect …`"*
  message instead of a cryptic internal error. Once you connect Gmail, reading `/mail` returns your
  real messages.
- **Quieter output.** Unrelated commands no longer print confusing sign-in warnings, and
  `describe` now reports the *actual* operations a path supports.

Every documentation page was rewritten and checked, example by example, against the running binary.

## Shipped: connections in the language

You now declare a connection with a `CREATE CONNECTION <name> DRIVER <driver> [AT '<locator>']
[SECRET '<ref>']` statement in a `connections.qfs` file (point at it with `QFS_CONNECTIONS=<file>`,
or `~/.config/qfs/connections.qfs`). Declared `DRIVER sqlite|postgres|mysql` connections mount
`/sql/<name>` and declared `DRIVER git` connections mount `/git/<name>` with **no env var** — the
declaration is the reviewable, committable source of truth. `SECRET` is a reference (`env:<VAR>` /
`vault:<driver>/<connection>`), never an inline value. The old `QFS_SQL_*` / `QFS_GIT_*` env vars
are a warned, deprecated fallback — `qfs connect --import-env` prints the equivalent declarations.
Extending this config-file declaration surface to cloud account mounts remains follow-up work.

A **connection** is a named pointer to one source: *which* database/repo/account it is, and how to
reach it. The problem this solved:

### What this replaced

Before this shipped, using a SQLite database at `/sql/orders` meant setting an environment variable
named `QFS_SQL_ORDERS=/path/to/orders.db`, and qfs turned the **name of that variable** into the
path segment `orders`. The connection was invisible: no statement to read, review, or check into a
repo. Cloud accounts still use the encrypted account store plus path bindings, so unifying those
under a config-file declaration remains the next connection-design gap.

### The shipped statement shape

A connection is now an **explicit statement**, just like the `CREATE TRIGGER` / `CREATE POLICY`
statements qfs already has — something you can read, review, and commit to a file. Secrets are never
written in the file; you *point* at where the secret lives (an environment variable, or qfs's
encrypted store).

```text
✅ shipped for SQL and git; cloud account declarations remain proposed

CREATE CONNECTION orders                    -- usable at /sql/orders/<table>
  DRIVER sqlite
  AT '/data/orders.db'                       -- a local file: no secret needed

CREATE CONNECTION analytics                 -- usable at /sql/analytics/<table>
  DRIVER postgres
  AT 'postgres://db.internal/analytics'      -- where the database is
  SECRET env:PG_PASSWORD                      -- the password comes from the PG_PASSWORD env var

-- proposed cloud extension
CREATE CONNECTION work                      -- intended usable at /mail/work/...
  DRIVER gmail
  SECRET vault:gmail/work                     -- the Gmail token from qfs's encrypted store
```

- **`DRIVER`** says what kind of source it is, which decides the path: `sqlite`/`postgres`/`mysql`
  live under `/sql`, `git` under `/git`, `gmail` under `/mail`, and so on. The **name** you give the
  connection (`orders`, `work`) is the segment you type in the path.
- **`AT`** is the plain, non-secret location — a file path, a database URL, a bucket. (Omitted when
  the location is fixed, e.g. Gmail.)
- **`SECRET`** *references* the secret, never the value: `env:NAME` reads an environment variable;
  `vault:path` reads qfs's encrypted credential store (the same store `qfs account add` writes to).
  A local SQLite file or git repo needs no secret at all.
- These declarations live in a plain config file (e.g. `connections.qfs`) you can keep in a repo —
  not in the names of environment variables.

The old `QFS_SQL_…` env-var trick stays working as a deprecated fallback, with a one-command
migration that prints the equivalent `CREATE CONNECTION` lines for you.

### What shipped and what's left

| Step | What it does |
| --- | --- |
| **Parser** | ✅ `CREATE CONNECTION` parses with `DRIVER`, `AT`, and `SECRET` references |
| **SQL/git registry** | ✅ config-file declarations build `/sql/<name>` and `/git/<name>` mounts |
| **Secret resolution** | ✅ SQL passwords resolve lazily from supported secret references |
| **Env fallback** | ✅ `QFS_SQL_…` / `QFS_GIT_…` remain as warned deprecated fallbacks with import output |
| **Cloud account declarations** | 🧭 still proposed; cloud mounts currently use `qfs account add` + `qfs connect` |

## Near-term backlog: known gaps

A batch of these just shipped (see *Just shipped from this backlog* below); what still remains:

- **Finish the Cloudflare & HTTP/REST mounts.** Both drivers are now **reachable as paths** — `/rest`
  describes and appears in the driver catalogue, and `/cf` is a mounted, plannable path (no longer
  `unknown_mount`). What remains is the per-resource config they need (which D1/KV/queues; which REST
  resource maps) and their live credentialed read/commit — best sourced from a richer `CREATE
  CONNECTION` declaration than the current `(driver, locator, secret)` shape carries.
- **Spell out `/ga`.** Reading `/ga/<property>` works now, but `/ga` is a cryptic mount name — it
  should be spelled out or aliased to `/analytics`, as a deliberate, deprecation-guarded change to
  the (versioned) path surface.
- **Push even more into the source.** A SQL backend now runs the `WHERE`/`ORDER BY`/`LIMIT`, Gmail
  runs its `q=` search, and Google Analytics runs the whole `runReport`. Still done locally: SQL/GA
  **column projection** (it changes the row shape vs. the described schema and can strip a column a
  residual still needs), plus cross-source aggregates and joins. Worth deepening over time.

### Shipped from this backlog

- **Read Google Drive and Google Analytics for real.** `/drive/...` lists a folder's children
  (resolving each path name to its Drive folder id), and `/ga/<property> |> select … |> where date …`
  runs a real GA4 report — both for a connected account.
- **Read a git repo as it was in the past.** `/git/app@v1.2/…` reads the tree at that commit/tag
  instead of the latest.
- **Markdown as a file format.** `… encode md` / `decode md` now resolves to the front-matter codec.
- **Write local files.** `upsert into /local/<file>` persists on `--commit`, including a positional
  `values ('hi')` payload.
- **Faster source-side queries.** Gmail's search filters, and SQL's `ORDER BY`/`LIMIT`, now run in
  the source instead of in qfs after fetching.
- **Onboarding & polish.** Plain-language passphrase text; a local-first first command after install;
  a per-source [Connect a service](/guide/connect) guide; an end-to-end [Get started](/guide/getting-started)
  on-ramp; and TTY-aware terminal color (honoring `NO_COLOR` / `--no-color`).
