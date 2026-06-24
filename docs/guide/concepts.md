# Core concepts

Five ideas explain almost everything in qfs. None of them is complicated.

## 1. Everything is a path

Every service is mounted as a tree of paths, like a filesystem:

| Path | What it is |
| --- | --- |
| `/mail/inbox`, `/mail/drafts` | Your mailbox |
| `/drive/Reports/q3.pdf` | A file in cloud Drive |
| `/s3/my-bucket/logs/app.log` | An object in S3 / R2 |
| `/sql/pg/orders` | A table in a Postgres database |
| `/github/acme/web/pulls/42` | Pull request #42 in a repo |
| `/slack/acme/general/messages` | A Slack channel |
| `/git/myrepo/commits` | A git repository's history |
| `/local/notes.md` | A file on your machine |

Paths are always **absolute** (they start with `/`) — there's no "current directory" to get lost
in. You address a single item the same way you'd point at a file.

Some paths take a **coordinate**. Git, for example, lets you read a file as of a tag or commit:

```text
FROM /git/myrepo@v1.2/src/main.rs |> SELECT path
```

## 2. Four archetypes

Behind the scenes every path is one of four **archetypes** — the shape of the data — which decides
what you can do with it. You rarely think about this directly; `describe` just tells you. But it's
useful to know the family:

| Archetype | Shaped like | Verbs you get | Examples |
| --- | --- | --- | --- |
| **Blob namespace** | A folder of files | `ls`, `cp`, `mv`, `rm`, `upsert` | Local files, S3/R2, Drive |
| **Relational table** | A SQL table | `SELECT`, `JOIN`, `INSERT`, `UPDATE`, `UPSERT` | Postgres, MySQL, D1 |
| **Append log** | A feed you add to | `SELECT` (tail), `INSERT` (append) | Mail, Slack, queues |
| **Object graph** | Things with actions | CRUD + `CALL` procedures | GitHub, Linear |

The key rule: **a path only offers the verbs that make sense for it.** You can't `UPDATE` a Slack
message (an append log doesn't support it) — and qfs rejects it up front with a clear error instead
of failing halfway. `describe` always shows the supported set.

## 3. The pipe-SQL language

You query and change paths with one small SQL-like language. A query is a **source** followed by
**stages** joined by `|>` (a pipe):

```text
FROM /sql/pg/orders |> WHERE total > 100 |> SELECT id, total |> ORDER BY total DESC |> LIMIT 5
```

Read it left to right: start from a table, keep the big orders, pick two columns, sort, take five.

The read/transform stages you'll use most:

| Stage | Does |
| --- | --- |
| `WHERE <condition>` | Filter rows (`=`, `<`, `>`, `LIKE`, `IN`, `BETWEEN`, `AND`/`OR`) |
| `SELECT <cols>` | Pick columns; rename with `AS`; call functions like `UPPER(name) AS u` |
| `EXTEND <col> = <expr>` | Add a computed column |
| `JOIN <path> ON <cond>` | Combine with another path — **even on a different service** |
| `AGGREGATE <fn> AS <name>` | Summarize (`AGGREGATE count(id) AS n`) |
| `GROUP BY <cols>` | Group for aggregation |
| `ORDER BY <col> [DESC]`, `LIMIT <n>`, `DISTINCT` | Sort, cap, dedupe |
| `UNION` / `EXCEPT` / `INTERSECT` `FROM <path>` | Set operations across sources |

The write stages (effects):

| Stage | Does |
| --- | --- |
| `INSERT INTO <path> VALUES (…)` | Add |
| `UPSERT INTO <path> VALUES (…)` | Add-or-replace (retry-safe) |
| `UPDATE <assignments>` | Change matching rows |
| `REMOVE` | Delete matching rows / trash a message |
| `CALL <service>.<action>(…)` | Run a built-in action, e.g. `CALL mail.send` |

And codecs convert formats: `DECODE json`, `ENCODE csv` (more below).

## 4. Preview vs. commit

This is the safety model, and it's simple:

- **`qfs run` previews by default.** It plans the whole thing and shows you the effects — what
  paths, how many rows, and whether anything is **irreversible** — but touches nothing.
- **`--commit`** applies the plan.
- **Irreversible effects** (sending mail, merging a PR, deleting) need an *extra* acknowledgement
  (`--commit-irreversible`) in a one-shot. Without it, qfs refuses rather than guess.

Reads are always pure — there's nothing to commit. This is what makes qfs safe to hand to an
automation or an AI agent: it can plan freely and only ever acts when explicitly told to.

## 5. Federation: one query, many services

Because every service is the same kind of path, qfs can **combine them in a single query**. It
pushes the parts a service can do natively (a `WHERE`, a `LIMIT`) *down* to that service, then does
the rest — joins, extra filtering, sorting — locally:

```text
FROM /sql/pg/orders |> JOIN /github/acme/web/issues ON id = issue_id |> SELECT id, title
```

That's a Postgres table joined to GitHub issues in one line. `describe` shows each path's
**pushdown** so you know what runs where. The [Showcase](/showcase) is full of these.

### Codecs: formats are just another stage

A blob of bytes becomes rows with `DECODE`, and rows become bytes with `ENCODE`. Supported formats:
`json`, `jsonl`, `yaml`, `toml`, `csv`, `md`. So converting a file's format is one line:

```text
FROM /local/config.json |> DECODE json |> ENCODE yaml
```

## Credentials, briefly

`describe` and `preview` never need a credential. To **commit** against a live service you store one
once with `qfs account add <service> <name>` — and qfs never prints it back. See
[Accounts & credentials](/guide/accounts).

**Next:** put it all together in [the Showcase →](/showcase)
