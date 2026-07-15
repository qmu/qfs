---
name: qfs-automation
description: Use when a task needs the qfs SERVER side — scheduled jobs, triggers, HTTP endpoints, and cached views created with CREATE bindings over queries you already write.
---

# Automation (the server)

The same language has a server side. Any query you can already write becomes **standing
automation** — a `CREATE …` binding runs it on an **event**, on a **schedule**, or on an **HTTP
request**, and a `VIEW` names and caches it. You collect bindings in a `.qfs` config and serve them
with `qfs serve config.qfs`.

## Example

**Turn "when mail arrives, tell the team" into a permanent reflex** — one binding, and every new
inbox message posts its subject to Slack, forever, unattended:

```qfs
create trigger notify
  on /mail/inbox
  do insert into /slack/acme/general/messages values (NEW.subject)
```

```text
rows: []
is_pure: true
```

A binding has no rows to return, so previewing it with `qfs run` yields that empty, **pure** plan —
proof the statement parses and type-checks, not an install report. The trigger is just the read you
already know (`/mail/inbox`) wired to fire on the `NEW` row.

::: tip Bindings are queries you preview before they go live
A `CREATE …` binding wraps a query, so you inspect it exactly like any other statement. Preview a
single binding with `qfs run` (no socket, no backend) to confirm it's valid; run a `JOB` once with
`qfs job run` and it **previews by default**, changing nothing until you pass `--commit` — through
the same policy gate and irreversible gate as `qfs run`. Pair any handler with a `POLICY` so an
automation, or an AI agent, acts only within bounds you set.
:::

Bindings don't run until a server hosts them — collect them in a `.qfs` file and start it, once, in
**[Setup](#setup)**. After that every recipe on this page installs verbatim.

## Setup

::: tip Prerequisites — an operator, an account, a mount
A binding that touches a cloud service needs the same three one-time steps as any other query
against it: a signed-in operator (`qfs init` — **[The operator identity](/guide/operator)**), an
authorized account (`qfs account add …`), and a mount binding that account to a path
(`qfs connect …`). Do them via each service's cookbook first; every step below assumes them.
:::

Put your `CREATE …` bindings in one config and serve them. The happy path is a single command:

```sh
qfs serve config.qfs
```

`qfs serve` is one process presenting the engine as three faces: the HTTP API, an **MCP endpoint**
an AI agent connects to, and an **embedded web dashboard** whose **approval cards** let a human
review and approve a pending irreversible commit. So an unattended binding can still route an
irreversible effect to a person for sign-off instead of failing closed.

::: warning Separate every statement in a `.qfs` config
A config holds several `CREATE …` statements. **End each one with a `;`** (or a blank line) —
adjacent statements with no separator fail to parse (`RESERVED_AS_IDENTIFIER`, because the parser
reads the next `create` as an identifier). `qfs serve` / `qfs job` only start once the whole file
parses.
:::

## The binding types

Once served, each binding shape wraps a query and fires on a different signal:

| binding | fires on | what it is |
| ------- | -------- | ---------- |
| `TRIGGER` | a new row on a path (`NEW`) | react to an event |
| `JOB` | a schedule an external scheduler runs | a saved named plan + cadence |
| `ENDPOINT` | an incoming HTTP request | a query exposed as an API route |
| `VIEW` / `MATERIALIZED VIEW` | every read / a cached read | a named (optionally cached) query |
| `POLICY` | — | least-privilege bounds a handler runs under |

## Trigger

**When mail arrives, post its subject to Slack:**

```qfs
create trigger notify
  on /mail/inbox
  do insert into /slack/acme/general/messages values (NEW.subject)
```

**Only escalate high-priority mail** — triggers can filter on the new row with `NEW`:

```qfs
create trigger escalate
  on /mail/inbox
  where NEW.priority > 3
  do insert into /slack/acme/ops/messages values ('urgent mail')
```

::: warning Object-store write targets aren't wired yet
A trigger whose body writes to `/s3` or `/r2` — e.g. `do upsert into /s3/archive/mail values
(NEW.id)` — can't install today: that `UPSERT` resolves to `unsupported_verb` (`supported: []`).
Route archival to `/local` (or a `/sql` table) until object-store writes land.
:::

## Job — a saved plan the daemon (or an external scheduler) fires

A `JOB` is a *saved named plan plus its cadence*. A running `qfs serve` daemon **fires it itself**:
a sweeper checks every job's `EVERY` interval on a real clock and commits due plans through the
same policy gate and irreversible gate as any commit (a policy-less job is default-denied — a
visibly recorded denial, zero effects). The semantics are deliberate: a missed interval collapses
to **one** catch-up fire (no storm after downtime), an in-flight job is not re-fired, all times
are UTC, and `last_run` survives a daemon restart. The external-scheduler path below (OS `cron`,
Cloudflare Cron Triggers) remains for hosts that run no daemon.

**Define the saved plan** in your config (`EVERY` takes a quoted interval; attach a `POLICY` for
least privilege — and make sure the policy's path pattern covers the path the job touches):

```qfs
create policy cleanup ALLOW remove on 'local/*'
```

```qfs
create job nightly
  every '1h'
  do remove /local/srv/scratch where name LIKE '%.tmp'
  policy cleanup
```

**Run it once** — the unit an external scheduler invokes. PREVIEW by default; `--commit` applies
through the same policy gate + irreversible gate as `qfs run`:

```sh
qfs job run app.qfs nightly
```

```text
PREVIEW job 'nightly' (policy cleanup, 1 effect(s); nothing applied — pass --commit):
  REMOVE local:/local/srv/scratch
```

```sh
qfs job run app.qfs nightly --commit
# an irreversible plan (REMOVE / CALL) additionally needs --commit-irreversible (fail-closed)
qfs job run app.qfs nightly --commit --commit-irreversible
```

**Individual / local — OS `cron`.** `qfs job cron` emits the crontab line for the saved cadence;
drop it into a host crontab (ensure cron's environment carries `QFS_PASSPHRASE` and any connection
credentials):

```sh
qfs job cron app.qfs nightly
```

```text
# qfs JOB 'nightly' — EVERY 1h (decision M: OS cron owns the *when*; qfs is not a scheduler).
# Ensure cron's environment carries QFS_PASSPHRASE (+ any connection creds) for the commit.
# An irreversible plan (REMOVE / CALL) additionally needs --commit-irreversible (fail-closed).
0 * * * *	qfs job run app.qfs nightly --commit
# Managed tier (Cloudflare Cron Triggers): [triggers] crons = ["0 * * * *"]
```

**Read a job's firing history** — every daemon firing (fired / denied / blocked / failed) lands on
a read-only per-job collection beside the job row:

```qfs
/server/jobs/nightly/runs
|> order by scheduled_at DESC
|> limit 10
```

```text
scheduled_at          outcome  detail                                    affected
2026-07-12 03:00:00   fired                                              1
2026-07-12 02:00:00   denied   default-deny (no matching rule)           0
```

::: warning Idempotency is yours
Scheduling is at-least-once everywhere (the daemon re-fires a failed plan next sweep; a Cron
Trigger can double-fire on retry). Keep effects idempotent — `UPSERT` / `@version` preconditions —
so a re-run is a no-op.
:::

## Cloudflare live resources

`/cf` is live for D1, KV, Queues, and Artifacts repositories after the operator stores a Cloudflare
API token in the qfs vault and connects a mount:

```sh
printf %s "$CLOUDFLARE_API_TOKEN" | qfs account add cf mycf
qfs connect /cf --driver cf --account mycf
```

When the token can see exactly one Cloudflare account, qfs discovers and persists that account id.
If the token can see multiple accounts, pass `--at <cloudflare_account_id>` to choose one.

With that mount, qfs discovers Cloudflare resources live: `/cf/d1/<db>/<table>` reads and writes
through D1, `/cf/kv/<namespace>/<key>` reads and upserts KV entries, `/cf/queue/<queue>` tails or
appends messages, and `/cf/artifacts` lists or creates Artifacts Git repositories. Repo create seals
the returned Git token into qfs's vault and returns only non-secret metadata through the table.
Without the stored account and connected mount, `/cf` is not registered for reads or commits.

```qfs
/cf/artifacts
|> select namespace, name, remote_url
```

```qfs
upsert into /cf/artifacts
  values ('default', 'starter-repo', null, null, null, null, null, 'Automation repo', 'main', null, false)
```

```qfs
remove /cf/artifacts/default/starter-repo
```

## Endpoint — expose a query as an HTTP API

**A read-only `GET /recent` that returns the latest inbox items:**

```qfs
create endpoint recent
  on 'GET /recent'
  as /mail/inbox |> limit 5
```

**Paging** — an endpoint result is requestable in bounded pages with the `limit` and `offset` query
knobs: `GET /recent?limit=10&offset=20` returns rows 21–30. This shares the result envelope's `meta`
vocabulary — the JSON response carries `meta.{limit, offset, truncated}`, and `truncated` is `true`
when rows remained beyond the page. (There is no cursor dialect: qfs sources cannot generally
guarantee the stable sort key a cursor needs.)

## View — name and cache a query

**A plain view** (a named query):

```qfs
create view recent_mail
  as /mail/inbox |> limit 50
```

**A materialized view** (cached result):

```qfs
create materialized view cached
  as /mail/inbox |> limit 50
```

**Freshness as data** — a materialized view's `/server/views` row carries a `last_run` column (the
last successful refresh time). Read it to show "updated 5 minutes ago"; a view that has never
refreshed reports `last_run` as `null` (honest — never a fabricated timestamp):

```qfs
/server/views
|> where materialized == true
|> select name, last_run
```

Refresh explicitly when an operator or external scheduler wants a new snapshot. qfs does not run a
hidden materialized-view scheduler:

```sh
qfs view refresh app.qfs cached
qfs view refresh app.qfs cached --quiet
qfs --json view refresh app.qfs cached
```

The refresh command executes the saved query through the normal read registry, caches the returned
rows inside server state, and stamps `last_run` only after the read succeeds. A failed refresh leaves
the previous cache and freshness marker unchanged.

## Policy — least privilege

A `POLICY` constrains what a handler may do: allow some verbs, deny the rest, optionally scoped to a
path pattern. (Scope the pattern to the **driver/path the handler actually touches** — a job that
removes a `/local` path needs `on 'local/*'`, not `'tmp/*'`, or the gate denies it.)

**Read-only API access:**

```qfs
create policy api
  ALLOW select
  DENY INSERT, update, remove, call
```

**Allow uploads to one bucket prefix only:**

```qfs
create policy uploads
  ALLOW UPSERT on 's3/*'
```

::: tip Why this is safe
A binding is just a query, so you preview it exactly like any other statement before it goes live.
Pair handlers with a `POLICY` so an automation — or an AI agent — can act, but only within the
bounds you set.
:::

## Transform — call a model over rows

A `transform` runs a model over a relation as an ordinary pipe stage. You declare it once — its
input and output columns, the provider and model, and a **secret reference** (an `env:`/`vault:`
pointer, never an inline token) — then use it in any pipeline. The shape of the input decides how
rows are fed to the model, so you never wire that by hand.

Declare it:

```qfs
create transform classify
  input (subject text)
  output (label text)
  provider claude model 'claude-sonnet-5' secret 'vault:models/key'
```

Use it — the model labels each message, and the result is an ordinary relation you keep filtering:

```qfs
/mail/inbox |> transform classify |> where label == 'urgent'
```

A transform spends tokens and is non-deterministic, so it is **irreversible**: the run above
*previews* the spend (provider, model, row count) and calls **no** model — add
`--commit --commit-irreversible` to actually run it. The preview is always model-free, so you see
what a run would cost before a single token is spent.

Retire a definition when you no longer need it:

```qfs
remove transform classify
```

## Reconcile the whole configuration as code — `qfs plan` / `qfs apply`

Instead of applying bindings one at a time, keep the **whole** configuration in one `.qfs`
document — the same `CREATE` bindings you already write — and reconcile the live deployment to it,
Terraform-style. The document is the source of truth: what it declares is created or updated, and
what it **omits** is removed.

```qfs
create endpoint recent
  on 'GET /recent'
  as /mail/inbox |> limit 5

create policy api
  ALLOW select
  DENY INSERT, update, remove, call
```

`qfs plan` shows the add/change/destroy diff against what is live and writes nothing. Its exit code
distinguishes an empty plan from a pending one — `0` = no changes, `2` = changes pending, `1` =
error (the Terraform `-detailed-exitcode` convention), so a CI job can gate on drift:

```sh
qfs plan config.qfs
```

```text
Plan: 1 to add, 0 to change, 0 to destroy.
```

`qfs apply` converges the deployment. A plan containing a **destroy** is irreversible, so it is
refused unless you acknowledge it; and if the deployment moved since you fetched the document (its
generation stamp no longer matches), apply refuses on the stale base unless you override:

```sh
qfs apply config.qfs --commit-irreversible
qfs apply config.qfs --allow-stale-base   # only if the base moved under you
```

::: tip Two stores, one document
The document reconciles both the `/sys` administrative config (connections, settings, sys policies,
path bindings) and the `/server` bindings. Secret values never enter it — every credential is a
reference (`env:` / `vault:`), and secretish settings are excluded entirely. The `/server` half is
read from and applied through a running `qfs serve` daemon; with no daemon reachable, a document
that configures `/server` is refused (never treated as an empty configuration), while a
`/sys`-only document reconciles with no daemon.
:::
