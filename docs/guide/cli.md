# CLI reference

`qfs` is one binary with a handful of subcommands. With **no** subcommand it starts the
[interactive shell](/guide/shell).

```text
qfs [OPTIONS] [COMMAND]

Commands:
  run        Run one statement and exit (preview by default)
  describe   Describe a path: archetype, columns, verbs, procedures, pushdown
  skill      Print the embedded AI operating procedure
  dump       Dump secret-free qfs configuration state as JSONL
  restore    Restore a qfs JSONL state dump (preview by default)
  serve      Start the server (CLI + MCP endpoint + web dashboard) from a .qfs config
  init       Ready this machine: create the encrypted vault + register the operator
  connect    Bind a defined path to a driver + account (the CLI twin of CONNECT)
  disconnect Remove a defined path (idempotent)
  app        Manage OAuth app registrations (today: Google's credentials.json)
  account    Manage service accounts: authorize, list, remove, rotate, revoke
  vault      Manage the vault's key slots: slots, enroll, revoke, rekey
  auth       Warm the time-boxed local session (`--lock` to drop it)
  host       Manage the qfs hosts this CLI can act on (`local` is implicit)
  identity   Local identity: look yourself up (signing up is `qfs init`)
  invite     Team invites & membership: create, redeem, revoke
  job        Run / schedule a saved JOB (an external scheduler drives it)
  help       Print help for any command

Global options:
  --json        Machine-readable JSON instead of the human table
  -h, --help    Help
  -V, --version Version (with build details)
```

## `qfs run` — execute one statement

```sh
qfs run "<statement>"        # positional
qfs run -e "<statement>"     # the -e form
echo "<statement>" | qfs run -   # read from stdin
```

**Previews by default** — it plans and shows the effects but changes nothing.

| Flag | Meaning |
| --- | --- |
| `--commit` | Apply the plan (a trailing `COMMIT` keyword does the same) |
| `--commit-irreversible` | Required to apply an irreversible effect (send, merge, delete) in a one-shot |
| `--format json\|table` | Force output format (default: table on a terminal, JSON when piped) |
| `--json` | Shorthand for `--format json` |
| `-q, --quiet` | Suppress progress output (never suppresses errors) |

```sh
# Preview, then commit:
qfs run "insert into /mail/drafts values ('alice@example.com','Hi','Body')"
qfs run "insert into /mail/drafts values ('alice@example.com','Hi','Body')" --commit

# Irreversible needs the extra ack (this CALL needs a connected mail account first —
# see `qfs connect`; without one it returns a capability error):
qfs run "/mail/drafts |> call mail.send" --commit --commit-irreversible
```

### The `--json` result envelope

A read result (`--json`, or piped) is one **stable, schema-carrying envelope** — the same shape
the HTTP endpoint face returns, so an agent decodes one shape everywhere (status: *stabilizing* —
not yet part of the versioned surface):

```json
{
  "schema": [ {"name": "date", "type": "timestamp"}, {"name": "content", "type": "bytes"} ],
  "rows":   [ {"date": 1751600000000, "content": "aGVsbG8gcWZzCg=="} ],
  "meta":   {"row_count": 1, "truncated": false, "limit": null, "offset": null, "affected": null}
}
```

- **`schema`** is always present, in column order — each entry is the column `name` and its type
  token (`text`/`int`/`bool`/`timestamp`/`bytes`/…, or `"unknown"` for an unresolved column).
- **`rows`** is an array of objects keyed by column name — read `row.subject` directly.
- **`meta`** is execution fact: `row_count`; `truncated` plus the `limit`/`offset` bound that
  cut the result (both `null` when nothing was bounded); `affected` (non-null only when effects ran).
- **Encodings** are discoverable from the schema type: a `timestamp` is **epoch milliseconds** (an
  integer, UTC); a `bytes`/blob value is **base64** (a 758 KB file is a base64 string, not a JSON
  array of byte integers).

An **effect** statement instead renders `{"preview": {…}, "committed": <bool>}` (the plan dry-run;
`committed` is `true` only under `--commit`); an error renders `{"error": {"code","kind","message",…}}`.

### Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Success (a read, or a committed apply) |
| `2` | Usage / parse error (a malformed statement or bad flags) |
| `3` | Capability error — the path does not support the requested verb (e.g. a `CALL` with no connected account) |
| `4` | A destructive/irreversible plan was **previewed** but not committed — re-run with `--commit` (and `--commit-irreversible` for an irreversible effect) |
| `5` | Commit failed (a driver or commit-time error) |
| `6` | Auth error — the vault is locked or the account is not authorized |

`qfs run` **previews by default**: a destructive statement without `--commit` exits `4` with the
preview on stdout — that is the plan projection, *not* an apply dry-run, so an effect that would only
fail at commit still previews cleanly.

## `qfs describe` — inspect a path

```sh
qfs describe <path>
qfs describe <path> --json | jq .verbs
```

Completely **offline and credential-free**. It returns the node's archetype, columns (name, type,
nullability), supported verbs, `CALL` procedures (with which are irreversible), prelude aliases, and
which filters push down to the service. This is the first thing to run against any unfamiliar path.

## `qfs dump` — export qfs configuration state

`qfs dump` emits a deterministic, secret-free JSONL backup/review stream. It includes current qfs
configuration state such as declared drivers, settings, policies, billing labels, and defined path
bindings. It does **not** export credential values; encrypted vault material must be backed up
separately.

```sh
qfs dump > qfs-state.jsonl
qfs dump --include-events > qfs-state-with-events.jsonl
```

The first record is a header with the qfs version, migration counts, generation timestamp, and DDL
event head. Each following line is one JSON object. `--include-events` appends the replayable
`sys_ddl_events` history after the current snapshot records.

## `qfs restore` — recover qfs configuration state

`qfs restore <dump.jsonl>` parses a dump and previews what it can restore. It writes only with
`--commit`.

```sh
qfs restore qfs-state.jsonl             # preview only
qfs restore qfs-state.jsonl --commit    # apply supported current-state records
cat qfs-state.jsonl | qfs restore - --commit
```

Restore supports the JSONL format produced by `qfs dump`: settings, declared drivers, policies,
billing labels, and defined path bindings. Committed restore records new local audit/DDL events for
System DB-backed writes; dumped historical `ddl_event` rows are treated as external provenance and
are not imported into the local hash chain. Credential values are still excluded: restore can rebuild
references like `vault:provider/account`, but the encrypted vault backup must be restored separately.

## `qfs init` — ready this machine

The first-run wizard: creates the encrypted vault (walking you through choosing its passphrase —
the passphrase key slot is enrolled automatically) and registers the **operator identity**. There
is no password — your OS login is the authentication; the email is an accountability label.
Idempotent: re-running reports what exists.

```sh
qfs init you@example.com     # or omit the email on a terminal to be prompted
```

## `qfs connect` / `qfs disconnect` — defined paths

A cloud path exists only after a connect. `connect` binds a path you choose to a driver plus the
account it uses (the mount carries the account — there is no selection state); `disconnect`
removes it. The CLI twin of the `CONNECT` / `DISCONNECT` statements:

```sh
qfs connect /mail --driver gmail --account you@gmail.com   # mount Gmail at /mail
qfs connect /sql/app --driver sqlite --at 'file:app.db'    # SQL: connect AT the /sql/<conn> path
qfs disconnect /mail                                       # remove the defined path (idempotent)
qfs connect --list                                         # list the defined paths
qfs connect --import-env    # print CREATE CONNECTION declarations for QFS_SQL_*/QFS_GIT_* env vars
```

A **relational** connection (`sqlite`/`postgres`/`mysql`) is bound AT its `/sql/<conn>` path — the
canonical local-connection mechanism — so both `qfs run` over `/sql/<conn>/<table>` and `qfs describe
/sql/<conn>/<table>` resolve through the same persisted binding (the `QFS_SQL_*` env vars and a
`connections.qfs` file remain as read-only fallback shims). Above, `/sql/app` mounts `app.db` as
connection `app`; its tables are `/sql/app/<table>`.

## `qfs app` — OAuth app registrations

The client credentials **your** OAuth app authenticates with (today: Google's `credentials.json`).
Read from stdin, never printed back:

```sh
cat credentials.json | qfs app add google qmu
qfs app list                 # provider + label + created_at — never a secret
qfs app remove google qmu    # account tokens stay
```

## `qfs account` — service accounts

Authorize an external account (providers: `google`, `github`, `slack`, `objstore`, `cf`). On a
terminal `qfs account add google --app <label>` runs the live paste-back browser consent — open the printed URL
in your **local** browser, approve, and paste the `http://localhost/...` redirect URL back (works
over plain SSH; no listener, no port-forward); automation pipes the token on **stdin**, never
argv:

```sh
qfs account add google --app qmu                                        # paste-back browser consent on a TTY
printf %s "$REFRESH_TOKEN" | qfs account add google you@gmail.com --app qmu   # automation; email = label
printf %s "$GH_TOKEN" | qfs account add github work
qfs account list                          # labels + metadata only, never tokens
qfs account remove <provider> <label>     # delete the token AND its consent record
```

For offboarding and key hygiene, an account can be **rotated** or **revoked** (the new secret is
read from stdin, never argv):

```sh
printf %s "$NEW" | qfs account rotate <provider> <label>   # re-mint the secret, clear any revoke
qfs account revoke <provider> <label>                      # mark unresolvable (fails closed at bind)
```

## `qfs vault` — key slots

The vault's data-key is wrapped once per **key slot** (KeyGuardian). The passphrase slot is
enrolled by `qfs init`; enroll the OS keychain and this host unlocks with no passphrase at all:

```sh
qfs vault slots                          # id, guardian kind, created_at (+ any live session) — never key bytes
qfs vault enroll keychain                # OS keychain slot — no passphrase per pane thereafter
qfs vault revoke <slot>                  # the last remaining slot is refused
printf %s "$NEWPASS" | qfs vault rekey   # re-wrap the data-key under a new passphrase
```

## `qfs auth` — warm the local session

`qfs auth` is the one short command you run up front to unlock for the day. It enters the passphrase
**once** (echo off — or unlocks via `QFS_PASSPHRASE` / the OS keychain) and caches the unlock in a
`0600` **time-boxed session** beside `project.db`, so later `qfs` one-shots — in other panes, or a
delegated agent's **separate processes** — skip the prompt until it expires:

```sh
qfs auth                          # warm the session; prints the remaining TTL (default 8h)
QFS_SESSION_TTL=2h qfs auth       # warm a 2-hour session instead (override; clamped 1m..7d)
qfs auth --lock                   # drop the session now — the next command re-prompts
qfs vault slots                   # shows the live session beside the key slots
```

On a host with no terminal **and** no `QFS_PASSPHRASE`, `qfs auth` fails with a clear error rather
than hanging. See [The QFS passphrase](/guide/passphrase) for the full story.

## `qfs identity` — who you are

Authentication only — the operator is an identity, not an authorization (that's policies and
the ACL). Signing up is part of [`qfs init`](#qfs-init-ready-this-machine):

```sh
qfs identity whoami [a@b.com]   # print a user's email + id
```

## `qfs invite` — teams & membership

An operator mints a one-time, expiring invite; the invitee redeems it to create their local
identity and join. The token is shown **once** at create (store it then); redeem is single-use.

```sh
qfs invite create --scope host --role member --ttl 86400   # prints the one-time URL/token once
printf %s "$PW" | qfs invite redeem <token> a@b.com         # create the user + membership
qfs invite revoke <id>                                      # cancel a still-pending invite
```

## `qfs job` — run a saved JOB

**qfs is not a scheduler.** A `CREATE JOB … EVERY … DO …` row is a *saved named plan plus its
intended cadence*; an external scheduler (OS `cron` / Cloudflare Cron Triggers) owns the *when*.
`run` previews by default and applies through the same policy + irreversible gates as `qfs run`:

```sh
qfs job run app.qfs nightly --commit      # invoke the saved plan once (the scheduler's entrypoint)
qfs job cron app.qfs nightly              # emit the crontab line for the host crontab
```

## `qfs view` — refresh a materialized view

A materialized view stores cached rows plus a `last_run` freshness marker. Refresh is explicit: an
operator, OS `cron`, or another external scheduler invokes it when a new snapshot is needed.

```sh
qfs view refresh app.qfs cached           # execute the saved query and stamp last_run on success
qfs view refresh app.qfs cached --quiet   # same operation, success output suppressed
qfs --json view refresh app.qfs cached    # machine-readable receipt
```

## `/cf` live configuration

Cloudflare D1/KV/Queues/Artifacts reads and commits are live after the operator stores a Cloudflare
API token in the qfs vault and connects a mount:

```sh
printf %s "$CLOUDFLARE_API_TOKEN" | qfs account add cf mycf
qfs connect /cf --driver cf --account mycf
```

When the token can see exactly one Cloudflare account, qfs discovers and persists that account id.
If the token can see multiple accounts, pass `--at <cloudflare_account_id>` to choose one.

At mount registration, qfs discovers D1 databases, KV namespaces, Queues, and Artifacts namespace
access from the Cloudflare API. D1 databases are registered under their human name and use
Cloudflare's `uuid` internally; KV namespaces are registered under their title and use Cloudflare's
`id` internally. Artifacts repositories are exposed as `/cf/artifacts` rows with
`namespace`, `name`, `id`, `remote_url`, and other non-secret metadata; the repo token returned by
create is sealed into qfs's vault and is never returned as a column. Without the stored account and
connected mount, `/cf` is not registered for reads or commits.

## `qfs skill` — the embedded AI procedure

Prints the operating procedure an AI agent follows, straight from the binary:

```sh
qfs skill                # the procedure
qfs skill --examples     # plus one worked example per service
```

## `qfs serve` — run the server

Starts the server from a `.qfs` config file containing `CREATE …` bindings (triggers, jobs,
endpoints, views, policies). The one process presents the same engine as **three faces**: the HTTP
API, the **MCP endpoint** an AI agent connects to, and the **embedded web dashboard** whose approval
cards let a human review and approve a pending irreversible commit.

```sh
qfs serve ./myserver.qfs
```

See the [Server guide](/server) for the binding forms.

## `qfs --version`

The long form prints the version, the exact build commit, and the target it was built for — handy
when reporting an issue:

```text
qfs 0.0.14
commit:  <git-sha>
target:  x86_64-unknown-linux-gnu
wasm32:  false
```
