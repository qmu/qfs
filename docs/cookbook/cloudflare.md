---
skill_name: qfs-cloudflare
skill_description: Use when a task needs Cloudflare through qfs — installing and querying the DECLARED /cloudflare driver (zones, DNS records, account-scoped KV/Queues/D1 listings, plus D1 SQL, KV get/put, and queue push) written in the query language itself. Covers installing the cloudflare.qfs declaration, connecting it to the stored Cloudflare token, reading, and the declared D1 SQL surface. For queue consumption (pull) and Artifacts Git repositories use the compiled /cf driver instead.
---

# Cloudflare (declared driver)

`/cloudflare` is a **declared driver**: an integration written in qfs's own query language
(`CREATE DRIVER … CREATE VIEW … CREATE SQL …`) rather than compiled Rust. Installing it is an
ordinary preview/commit; connecting it evaluates it. It turns Cloudflare's surface — zones, DNS
records, account-scoped listings, KV get/put, queue push, and D1's relational SQL — into
filesystem-shaped paths you read with the same pipe-SQL you use everywhere else.

::: tip `/cloudflare` (declared) vs `/cf` (compiled)
The declared **`/cloudflare`** is now the normal way to reach Cloudflare: D1 SQL (its planner —
`WHERE`/`JOIN`/aggregate pushdown, from the `CREATE SQL` arm), KV get/put, queue push, zones, DNS,
and the account-scoped listings — all as reviewable, user-extensible declaration rows you grow with
more `CREATE VIEW`/`MAP`/`SQL` statements. The compiled **`/cf`** is now a minimal fallback for only
what plain declared REST cannot express: **queue consumption (pull)** — a POST-to-read with no
declared primitive — and **Artifacts Git repositories**. On a name collision the compiled driver
wins, which is why this mounts at `/cloudflare`.
:::

## Example

Once installed and connected (**[Setup](#setup)**), your account's zones are a path:

```qfs
/cloudflare/zones
|> select name, status
|> limit 10
```

```text
name              status
acme.com          active
acme-staging.com  active
shop.example      active
… 3 rows
```

That read runs live against Cloudflare's REST API — the token is resolved from qfs's vault, never
typed on the command line, and the declaration is **structurally unable** to address any host other
than Cloudflare's (host confinement, enforced at install).

## Setup

Installing a declared driver is two steps: **install** the declaration (a local, previewed write to
`/sys/drivers` — zero network), then **connect** it to the Cloudflare token you already hold.

### 1. Install the declaration

The shipped `cloudflare.qfs` declares the driver and its resources. Preview then commit each
statement (each desugars to one `/sys/drivers` row):

```qfs
CREATE DRIVER cloudflare
  AT 'https://api.cloudflare.com/client/v4'
  AUTH BEARER
```

```qfs
CREATE TYPE cloudflare/zone (
  id text PRIMARY KEY,
  name text NOT NULL,
  status text
)
```

```qfs
CREATE VIEW /cloudflare/zones OF cloudflare/zone AS
  /http/cloudflare/zones |> DECODE json |> EXPAND result
```

The declaration carries **no token** — `AUTH BEARER` names only the scheme; the value lives in the
account layer. Run each statement with `qfs run --commit`, or install the whole `cloudflare.qfs`
file statement by statement.

### 2. Connect it to your Cloudflare token

`/cloudflare` reuses the same token the compiled `/cf` driver uses. If you have not added one yet:

```sh
qfs init you@example.com                                   # the operator + the vault (once)
printf '%s' "$CF_API_TOKEN" | qfs account add cf mycf      # the Cloudflare API token (label: mycf)
```

Then bind the declared mount to that stored token with a `SECRET` reference:

```sh
qfs connect /cloudflare --driver cloudflare --secret 'vault:cf/mycf'
```

The `SECRET 'vault:cf/mycf'` points the declared driver's bearer auth at the vault-sealed `cf/mycf`
token. No token value ever appears in the declaration, in `/sys/drivers`, in `qfs dump`, or in
`qfs connect --list`.

## The Cloudflare surface as paths

| Cloudflare thing | qfs path | scope |
| ---------------- | -------- | ----- |
| your zones | `/cloudflare/zones` | token-scoped |
| a zone's DNS records | `/cloudflare/zones/{zone}/dns_records` | token-scoped |
| KV namespaces | `/cloudflare/accounts/{account}/storage/kv/namespaces` | account-scoped |
| a KV value (get/put) | `/cloudflare/accounts/{account}/storage/kv/namespaces/{namespace}/values/{key}` | account-scoped |
| Queues | `/cloudflare/accounts/{account}/queues` | account-scoped |
| a queue message (push) | `/cloudflare/accounts/{account}/queues/{queue}/messages` | account-scoped |
| D1 databases (listing) | `/cloudflare/accounts/{account}/d1/database` | account-scoped |
| a D1 table (SQL) | `/cloudflare/d1/{database}/{table}` | account-scoped |

Token-scoped resources need no account segment. Account-scoped paths take an explicit `{account}`
segment — the Cloudflare account id, visible in `qfs connect --list` (the id `qfs connect /cf`
auto-discovered). Substitute the concrete id in the path; a missing segment is a visible path error,
never a silent wrong-endpoint fetch.

Run `qfs describe /cloudflare/zones` for the node's archetype and verbs.

## Read the surface

**List active zones:**

```qfs
/cloudflare/zones
|> where status == 'active'
|> select id, name
```

**A zone's DNS records** — substitute the concrete zone id for `<zone>`:

```text
/cloudflare/zones/<zone>/dns_records
|> select type, name, content
|> limit 50
```

**Account-scoped listings** — substitute the concrete account id for `<account>`:

```text
/cloudflare/accounts/<account>/storage/kv/namespaces |> limit 20
/cloudflare/accounts/<account>/queues                |> limit 20
```

## D1 SQL (the declared relational surface)

D1's relational surface is declared with a `CREATE SQL` resource: a sqlite-dialect SQL endpoint over
the D1 query wire, with the table catalog declared **inline**. That is the declared twin of a
mount-time introspection — the mount reads the committed catalog instead of probing `sqlite_master`
live — so it installs and describes with zero network. The shipped `cloudflare.qfs` seeds it; replace
the `TABLES (…)` catalog with your own D1 schema:

```qfs
CREATE SQL /cloudflare/d1/{database} DIALECT SQLITE
  OVER /http/cloudflare/accounts/{account}/d1/database/{database}/query
  TABLES (
    events ( id integer PRIMARY KEY, name text NOT NULL, created_at text )
  )
```

Once installed and connected, a D1 table is a relational path the SQL planner pushes down into —
`WHERE`/`JOIN`/aggregate — substituting your D1 database id for `{database}`:

```qfs
/cloudflare/d1/mydb/events
|> where name == 'signup'
|> select id, created_at
|> limit 20
```

The `{database}` segment is the D1 database id (list them with the `d1/database` listing above); the
addressed segment is used as the Cloudflare D1 api id. The bearer is the same account-layer token the
rest of the driver uses — never inlined in the declaration. **Queue consumption (pull)** and
**Artifacts Git repositories** are the only Cloudflare surfaces still reached through the compiled
`/cf` driver; everything else — D1 SQL, KV get/put, queue push — is declared here.

## Extend it

`/cloudflare` is yours to grow. Add any Cloudflare REST resource with one more `CREATE VIEW` whose
body addresses `/http/cloudflare/…` (the confinement boundary keeps every addition on Cloudflare's
host). The shipped file also seeds a **write** with a `CREATE MAP` — creating a DNS record is an
ordinary `INSERT` mapped onto the REST endpoint:

```qfs
CREATE MAP INSERT /cloudflare/zones/{zone}/dns_records AS
  INSERT INTO /http/cloudflare/zones/{zone}/dns_records VALUES (row)
```

Add more maps the same way. Because a declared driver is just data (rows in `/sys/drivers`), the
same preview → commit → audit story and path-scoped policies apply to it exactly as to a compiled
driver.
