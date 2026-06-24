# Showcase: what the query language can do

One small language, every service. This page is a tour of real problems and the single qfs
statement that solves each one. Every statement uses the same grammar you already know from
[Getting started](/guide/getting-started) — `FROM … |> WHERE … |> SELECT …`, plus effects and
server bindings.

::: tip Remember
`qfs run` **previews** by default — it shows the plan and changes nothing. Add `--commit` to act
(and `--commit-irreversible` for things like sending mail or merging a PR). So you can paste any of
these and safely see what it *would* do first.
:::

---

## Inbox triage

**Find unread invoices, newest first:**

```text
FROM /mail/inbox |> WHERE subject LIKE '%invoice%' |> SELECT date, from, subject |> ORDER BY date DESC |> LIMIT 20
```

**Count mail by sender to see who's flooding you:**

```text
FROM /mail/inbox |> GROUP BY from |> AGGREGATE count(id) AS n |> ORDER BY n DESC |> LIMIT 10
```

**Trash everything matching a pattern** (irreversible — preview shows it as a gate):

```text
REMOVE /mail/inbox WHERE subject LIKE '%[newsletter]%'
```

**Draft an email and send it** — the draft is reversible, the send is the irreversible step:

```text
INSERT INTO /mail/drafts VALUES ('alice@example.com', 'Q3 report', 'Attached.')
FROM /mail/drafts |> CALL mail.send
```

---

## Query your databases

**Filter and sort — the `WHERE` and `LIMIT` run inside the database (pushdown):**

```text
FROM /sql/pg/orders |> WHERE total > 100 |> SELECT id, total |> ORDER BY total DESC |> LIMIT 5
```

**Ranges and sets read naturally:**

```text
FROM /sql/pg/orders |> WHERE total BETWEEN 50 AND 100 |> SELECT id, total
FROM /sql/pg/orders |> WHERE status IN ('open', 'pending') |> SELECT id, status
```

**Summaries with grouping:**

```text
FROM /sql/pg/orders |> GROUP BY status |> AGGREGATE count(id) AS n |> ORDER BY n DESC
```

**Change rows — preview shows exactly which:**

```text
UPDATE /sql/pg/orders SET status = 'shipped' WHERE id = 7
```

**Combine two databases with a set operation:**

```text
FROM /sql/pg/users |> UNION FROM /sql/mysql/users
```

---

## Join across completely different services

This is where qfs shines: because every service is the same kind of path, you can `JOIN` them in
one statement. qfs pushes each side's filters down to its service, then joins locally.

**Database orders ⋈ GitHub issues:**

```text
FROM /sql/pg/orders |> JOIN /github/acme/web/issues ON id = issue_id |> SELECT id, title
```

**Database users ⋈ git commit history** — match accounts to the commits they authored:

```text
FROM /sql/pg/users |> JOIN /git/myrepo/commits ON id = author_id |> SELECT name, message
```

**Enrich a query with a local CSV** — join a service to a file on your laptop:

```text
FROM /sql/pg/orders |> JOIN /local/regions.csv ON region = code |> SELECT id, region
```

---

## Convert and move data between formats

Codecs turn bytes into rows (`DECODE`) and rows into bytes (`ENCODE`). Formats: `json`, `jsonl`,
`yaml`, `toml`, `csv`, `md`.

**Convert a JSON file to YAML in one line:**

```text
FROM /local/config.json |> DECODE json |> ENCODE yaml
```

**Read a JSON file, filter it, and write a CSV:**

```text
FROM /local/events.json |> DECODE json |> WHERE level = 'error' |> ENCODE csv
```

**Export a database table to JSONL:**

```text
FROM /sql/pg/orders |> SELECT id, total, status |> ENCODE jsonl
```

---

## Move and back up files across clouds

Files in Drive, S3/R2, and your local disk are all blob paths, so copying between them is just an
`UPSERT` (retry-safe: re-running it converges instead of duplicating).

**Back up a local file to S3:**

```text
UPSERT INTO /s3/backups/2026/db.sql VALUES ('…bytes…')
```

**Stage a report into a Drive folder:**

```text
UPSERT INTO /drive/my/Reports/q3.pdf VALUES ('…bytes…')
```

**List what's in a bucket, biggest first:**

```text
FROM /s3/my-bucket/logs |> WHERE size > 1000000 |> SELECT name, size |> ORDER BY size DESC
```

---

## Work with code and versions

Git is both a versioned file tree and a relational history, so you can read a file *as of* a ref,
or record a commit.

**Read a file as it was at a tag:**

```text
FROM /git/myrepo@v1.2/src/main.rs |> SELECT path
```

**Squash-merge a pull request** (irreversible — a gate):

```text
FROM /github/acme/web/pulls/42 |> CALL github.merge(method => 'squash')
```

**List recently opened pull requests:**

```text
FROM /github/acme/web/pulls |> WHERE state = 'open' |> SELECT number, title |> ORDER BY number DESC |> LIMIT 10
```

---

## Team chat as data

A Slack channel is an append log — read the tail, append a message.

**Post a message:**

```text
INSERT INTO /slack/acme/general/messages VALUES ('Deploy finished ✅')
```

**Read the last few messages in a channel:**

```text
FROM /slack/acme/general/messages |> SELECT text |> LIMIT 20
```

---

## Turn any query into automation (the server)

The same language has a server side. Each `CREATE …` binding takes a query you already understand
and runs it on an event, a schedule, or an HTTP request. (Preview a binding to see exactly the plan
it would install — no socket, no backend needed.)

**Trigger — when mail arrives, ping Slack:**

```text
CREATE TRIGGER notify ON /mail/inbox DO INSERT INTO /slack/acme/general/messages VALUES (NEW.subject)
```

**Conditional trigger — only escalate high-priority mail:**

```text
CREATE TRIGGER escalate ON inbox WHERE NEW.priority > 3 DO INSERT INTO /slack/acme/ops/messages VALUES ('urgent mail')
```

**Scheduled job — nightly cleanup of old scratch files:**

```text
CREATE JOB nightly EVERY '1h' DO REMOVE /tmp/scratch WHERE age > 7
```

**HTTP endpoint — expose a query as a tiny API:**

```text
CREATE ENDPOINT recent ON 'GET /recent' AS FROM /mail/inbox |> LIMIT 5
```

**Materialized view — cache an expensive query:**

```text
CREATE MATERIALIZED VIEW cached AS FROM /mail/inbox |> LIMIT 50
```

---

## Guardrails: least privilege

On the server, a `POLICY` constrains what a handler is allowed to do — allow some verbs, deny the
rest, optionally scoped to a path pattern.

**Read-only API access:**

```text
CREATE POLICY api ALLOW SELECT DENY INSERT, UPDATE, REMOVE, CALL
```

**Allow only uploads to one bucket prefix:**

```text
CREATE POLICY uploads ALLOW UPSERT ON 's3/*'
```

---

## Why this matters for AI agents

Look back over this page: **mail, databases, GitHub, Slack, files, git, the cloud, automation** —
all expressed in the *same* grammar, with the *same* preview-then-commit safety, with the *same*
"a path only allows the verbs that make sense" rule.

That's the point. An AI agent learns this one language once and can safely operate every service
you connect — no per-vendor SDK, no bespoke glue, and a preview it can read before it ever acts. To
see how an agent uses it, run `qfs skill` (the operating procedure ships inside the binary) or read
the [Language reference](/language) and [Driver catalog](/drivers).
