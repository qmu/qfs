---
skill_name: qfs-cross-service
skill_description: Use when a task spans MORE THAN ONE service in a single qfs query — joining a database to GitHub, a file to a table, or federating several services with one JOIN or UNION over their paths.
---

# Cross-service

This is what qfs is *for*: one pipe-SQL statement that reaches across more than one service at once —
a `JOIN`, `UNION`, `INTERSECT`, or `EXCEPT` whose paths live in *different* services. Because every
service is the same kind of path yielding rows, a database table, a GitHub issue list, and a mailbox
combine in a single query as easily as two tables would.

::: tip Prerequisites — an operator, an account per service, a mount per service
A cross-service query needs **every** participating service connected. That means the one-time
setup gates: a signed-in operator (`qfs init` — **[The operator identity](/guide/operator)**), and,
per cloud service, an authorized account (`qfs account add …`) bound to its mount
(`qfs connect …`). Connect each one via its own cookbook (linked below) first.
:::

## Example

**Stitch one customer's whole story together from three services at once** — inbound mail joined to
the customer row in your database and the open GitHub issue they filed, one statement, newest first:

```qfs
/mail/inbox
|> join /sql/pg/customers on inbox.from == customers.email
|> join /github/acme/support/issues on customers.email == issues.reporter_email
|> where issues.state == 'open'
|> select customers.legal_name, inbox.subject, issues.number, issues.title
|> order by issues.created_at DESC
```

```text
legal_name         | subject                       | number | title
------------------ | ----------------------------- | ------ | -----------------------------
Northwind Traders  | Re: Q3 renewal — a question   |    412 | Renewal quote looks wrong
Boldpeak Dev       | Can we move the review?       |    398 | Review scheduling conflict
(2 row(s))
```

Three services that never speak to each other, federated in one query — qfs pushes each leg's filter
into its own service, then joins the results locally. Now the **write** payoff: reads federate, but
so do writes — **download a file from Google Drive and pack it straight into a Gmail draft**, one
composable statement that previews before it touches anything:

```qfs
/drive/my/report.pdf
|> select {filename: name, mime: mime_type, bytes: content} as att
|> aggregate array_agg(att) as attachments
|> extend to = 'a@example.com', subject = 'Q3 report', body = 'See attached.'
|> insert into /mail/drafts
```

```text
PREVIEW: 1 effect(s)
  #0 INSERT -> mail:/mail/drafts [affected 1]
  total affected: 1
```

::: tip Reads run now; writes preview
Every **read** — every `JOIN`, `UNION`, `INTERSECT`, `EXCEPT` — returns rows immediately. Every
**write** (`insert`, `upsert`, `call`) *previews* by default and changes nothing — add `--commit` to
apply it, `--commit-irreversible` for the ones that can't be undone (sending, trashing). Paste any
recipe below and safely watch what it *would* do first.
:::

There's no setup on this page: a cross-service query is only as connected as its legs, and each
service is wired up in **its own chapter** — [Gmail](/cookbook/gmail),
[Databases](/cookbook/databases), [GitHub](/cookbook/github), [Slack](/cookbook/slack),
[git](/cookbook/git), [Google Drive](/cookbook/gdrive), and
[Files & object storage](/cookbook/files). Two of the local drivers (`/sql/<conn>`, `/git/<repo>`)
need no account at all, so a database↔git join runs the instant you point qfs at them — see
[Join a database to git history](#join-a-database-to-git-history) just below.

## How a mixed-source query resolves

Every leg of a cross-service statement is pushed down to its own service — a `/sql` subtree becomes
one SQL query inside the database, a `/github` subtree becomes a filtered API fetch — and only the
residual that genuinely spans the sources (the cross-source `JOIN … ON`, the post-join `WHERE`, the
`SELECT`, the `ORDER BY`) runs locally in qfs's own engine. That is why a SQL table and a git repo
combine as easily as two database tables: whatever the pushdown split, qfs over-fetches safely and
re-checks locally, so you never get wrong rows.

The recipes below mix sources that **read today** (`/sql/<conn>/…`, `/git/<repo>/…`) with ones that
need a connected account (`/github/…`, `/slack/…`, `/mail/…`, `/drive/…`). Each is marked.

## Join a database to git history

**Match author records in a table to the commits they wrote** — `/sql` and `/git` both read, so this
runs end to end, no account required:

```qfs
/sql/orders/authors
|> join /git/myrepo/commits on name == author
|> select name, team, message
```

```text
name           | team     | message
-------------- | -------- | ---------------
Test <t@e.com> | platform | add feature
Test <t@e.com> | platform | initial commit
(2 row(s))
```

(`/sql/<conn>` is registered with `QFS_SQL_<CONN>`, `/git/<repo>` with `QFS_GIT_<REPO>` — see the
[Databases](/cookbook/databases) and [git](/cookbook/git) chapters.)

## Join a database to GitHub

**Match orders to the GitHub issues that track them:**

```qfs
/sql/orders/orders
|> join /github/acme/web/issues on id == issue_id
|> select id, title, status
```

::: warning Needs a connected account
The `/sql` leg reads today, but the `/github` leg fails with an actionable hint until an account is
bound to the mount (`qfs account add github …`, then `qfs connect /github --driver github
--account …` — see the [GitHub cookbook](/cookbook/github)). Once connected, the join runs end to
end.
:::

## Federate several services into one feed

**Combine every channel a contact reached you through — mail, Slack, and GitHub — into one unified
touch log** with `UNION`, normalized to a single shape and sorted newest first:

```qfs
/mail/inbox
|> select from as contact, 'email' as channel, subject as detail, received_at as at
|> union
   /slack/acme/support/messages
|> select user as contact, 'slack' as channel, text as detail, ts as at
|> union
   /github/acme/support/issues
|> select reporter_email as contact, 'github' as channel, title as detail, created_at as at
|> order by at DESC
```

Each leg selects into the same `contact, channel, detail, at` shape, so `UNION` stacks three
services into one timeline. This one needs all three accounts connected.

## Combine the same shape from two sources

**Everyone, across two tables, de-duplicated** — `UNION` runs entirely on sources that read today:

```qfs
/sql/orders/orders
|> select customer
|> union /sql/orders/orders
|> select customer
```

```text
customer
--------
alice
bob
carol
dave
(4 row(s))
```

## Move data between services

Because reads and writes share one language, "copy from here to there" spans services too.

**Snapshot a database table to JSONL** — the read and the `ENCODE` both run:

```qfs
/sql/orders/orders
|> select id, customer, total
|> encode jsonl
```

```text
content
-------------------------------------------------
{"customer":"alice","id":1,"total":150.0}
{"customer":"bob","id":2,"total":80.0}
{"customer":"carol","id":3,"total":220.0}
{"customer":"dave","id":4,"total":55.0}
```

…then write those bytes to a store with an `UPSERT INTO`. (Today these are two steps; the point is
they speak the same language end to end. `UPSERT` into `/local` runs now; `/s3`/`/r2` writes are
not yet implemented — see [Files & object storage](/cookbook/files).)

## Attach a Drive file to a Gmail draft

The dogfooding payoff from the hero, in full: **download a file from Google Drive, pack it into a
Gmail draft's `attachments` column, and address the draft** — one composable statement, no bespoke
`pack()`. It uses two small, reusable primitives: a **struct-over-columns** constructor (`{ … }` whose
field values are columns) and single-column **`array_agg`** (N rows → one `Array`).

```qfs
/drive/my/report.pdf
|> select {filename: name, mime: mime_type, bytes: content} as att
|> aggregate array_agg(att) as attachments
|> extend to = 'a@example.com', subject = 'Q3 report', body = 'See attached.'
|> insert into /mail/drafts
```

Read left to right: each Drive row becomes a `{filename, mime, bytes}` **struct** (`content` is the
file's bytes); `array_agg` collapses those structs into one `attachments` **array**; `extend` adds the
draft's `to`/`subject`/`body`. The struct constructor and `array_agg` are **general** — usable in any
read pipeline, not just this recipe:

```qfs
/local/reports
|> select {name: name, size: size} as entry
|> aggregate array_agg(entry) as entries
```

```text
entries
--------------------------------------------------
[{"name":"a.csv","size":120},{"name":"b.csv","size":98}]
(1 row(s))
```

The **read pipeline above runs today** — the struct/array constructors and `array_agg` execute
locally and produce the packed `attachments` value. The terminal `CALL mail.send` stays behind the
explicit irreversible gate (`--commit-irreversible`), never automatic. For the *reply* direction —
threading a reply onto an existing message with a file from another service — the whole pipe,
including the commit-time materialisation, is wired today; see the next recipe.

## Reply to a thread with a file from another service

Thread a reply onto an existing Gmail message, carrying a file whose bytes come from Drive — **one
statement**. A reply is an append into the parent message's `replies` log
(`/mail/<label>/<msg>/replies`), so the whole cross-service pipe materialises at commit exactly like
the Drive→Drive-folder transfer below:

```qfs
/drive/my/report.pdf
|> select {filename: name, mime: mime_type, bytes: content} as att
|> aggregate array_agg(att) as attachments
|> extend body = 'See the attached quarterly report.'
|> insert into /mail/inbox/198f2a/replies
```

Read left to right: the Drive file becomes a `{filename, mime, bytes}` struct, `array_agg` packs it
into the `attachments` array, `extend` adds the reply `body`, and `insert into
/mail/<label>/<msg>/replies` threads the reply onto message `198f2a`. At commit qfs re-reads the
Drive bytes and materialises them straight into the reply — nothing cached. The parent's `thread_id`
and a `Re:` subject default from the message at commit; `to`/`cc`/`subject` are optional overrides
(`|> extend to = '…'`). Like `mail.reply`, this **drafts** a threaded reply and is **reversible** —
nothing sends until a separate `CALL mail.send`. (This is the composable sibling of `<msg> |> CALL
mail.reply(...)`: a `CALL`'s arguments are literals, so only the `INSERT … FROM` form can source its
attachments from another service.)

::: tip The same shape on other services
**Slack** — share a cross-sourced file *into a channel* with `… |> upsert into /slack/<ws>/files`
and a `channel` column (see the [Slack cookbook](/cookbook/slack#upload-a-file-to-slack-and-detach));
a *threaded* file-reply (`thread_ts`) is a recorded follow-up. **Chatwork** — posting a room message
reply works, but attaching a file is a recorded gap (it needs a generic multipart-upload primitive
in the declared driver), so a Chatwork file-reply is not yet expressible.
:::

## Extract a PDF's text into a Drive folder

A `transform` whose **INPUT is a single `bytes` column** runs in **Extraction** mode — one document
blob in, structured rows out. Point it at a PDF, and its OUTPUT rows upsert straight into Drive, all
in one statement. First declare the transform (once), then pipe a document through it:

```qfs
create transform extract
  input (blob bytes)
  output (name text, mime_type text, bytes text)
  provider anthropic
  model 'claude-sonnet-5'
```

```qfs
/local/report.pdf
|> select content as blob
|> transform extract
|> upsert into /drive/my/Extracted
```

A single-file read (`/local/<file>`, and Drive/Gmail file reads) carries the file's bytes in a
`content` column alongside its metadata, so `|> select content as blob` narrows the row to the
single `bytes` column Extraction expects (and matches the transform's `input (blob bytes)`). Selecting
`content` type-checks at plan time — the blob node advertises it.

The PDF's bytes are sent to the chosen provider as a **native document** (an Anthropic `document`
content block, an OpenAI `file` part, or a Gemini `inlineData` part — chosen by the transform's
`provider`), not as inlined text. The model returns the OUTPUT-schema rows, and `upsert into /drive`
lands them as a file (`bytes` carries the extracted text). Preview is **model-free**; the model runs
only at `--commit` (behind the transform-consent ack).

::: warning Informed egress
The PDF's content **transits to the chosen model provider** (`anthropic`/`openai`/`google`). That is
the point of the transform, but it is an outbound send of the document — pick the provider
deliberately. A document over the provider's inline cap (single-digit to tens of MB) fails **before**
the request is built, naming the limit; splitting a long PDF into chunks is a separate step.
:::

## Chain transforms — extract then summarize

Transforms **compose**: `… |> transform a |> transform b` feeds stage `a`'s OUTPUT rows into stage
`b`'s INPUT. The handoff is **schema-checked at plan time** — if `a`'s OUTPUT does not carry every
column `b` declares as INPUT, the statement fails at **preview** naming the missing column, before
any model runs. Declare the two stages (the second's INPUT matches the first's OUTPUT), then chain:

```qfs
create transform extract
  input (blob bytes)
  output (body text)
  provider anthropic
  model 'claude-sonnet-5'
```

```qfs
create transform summarize
  input (body text)
  output (digest text)
  provider anthropic
  model 'claude-sonnet-5'
```

```qfs
/local/report.pdf
|> transform extract
|> transform summarize
```

Each stage is its own model call: `extract` turns the PDF into `body` text, `summarize` turns that
`body` into a `digest`. Preview stays **model-free for every stage**; both calls run only at
`--commit` under one consent ack, and a failure in any stage aborts the whole statement (no partial
result). Two model calls double the cost, so a chain recipe favours small models and capped output.

## Let the model pick the tool — switch routing

The routing capability: a `transform` whose OUTPUT carries a **choice column**, followed by a
`switch` that routes each row to one of several **declared effect arms** by that column's value —
"triage my inbox: alert the urgent ones to Slack, log the reports to the database, draft a reply
for the rest", one statement, services picked per row by the model. Declare the router (once),
then switch on its output:

```qfs
create transform triage
  input (subject text, body text)
  output (route text)
  provider anthropic
  model 'claude-sonnet-5'
```

```qfs
/mail/inbox
|> select subject, body
|> transform triage
|> switch route {
     'urgent' => select subject as text |> insert into /slack/acme/ops-alerts/messages,
     'report' => select subject, body |> insert into /sql/pg/triage_log,
     else     => select subject, body |> insert into /mail/drafts
   }
```

Read left to right: `transform triage` adds the model's per-row choice as the `route` column;
`switch route { … }` **partitions** the rows by that value and each arm's continuation runs over
its own partition — `'urgent'` rows become Slack messages, `'report'` rows become database rows,
everything else becomes a draft. An arm can also end in a procedure
(`else => call mail.send(to => 'ops@example.com')`), and the `else` arm is **mandatory and last**
— every row has somewhere total to land.

What makes this safe to hand a model (blueprint §18):

- **Every arm is previewed before any model runs.** Preview is model-free, so the taken arm is
  unknowable — the statement's declared effect set is the **union** of every arm's effects. The
  model chooses *among* tools you already consented to; it can never invent one.
- **One model call, one materialization.** At `--commit` the source runs once, the rows partition
  by `route`, and each arm's effects batch over its partition in declaration order.
- **An untaken arm never fires.** Consented at preview, pruned at commit — an arm whose partition
  is empty spends nothing and the committed summary lists only what ran.
- **Arms are gated like any write.** Each arm's target passes the same capability and
  irreversibility gates a standalone `insert into` / `call` would; the model's label can select
  among pre-authorized plans, never escalate to an ungranted path.

Arm continuations are the row-local stages (`where`/`select`/`extend`/`aggregate`/`order by`/
`limit`); a `join`, codec, nested `switch`, or second `transform` inside an arm is a structured
refusal — routing composes existing gates, it does not widen them.

## Save a Gmail attachment into a Drive folder

The mirror direction, and the simpler one — no `array_agg`, just a single blob moving from one
service to another in **one statement**. Read an attachment's bytes from `/mail`, rename its columns
to the Drive upload shape, and `insert into` the destination folder:

```qfs
/mail/inbox/198f2a/att-1
|> select filename as name, mime as mime_type, content as bytes
|> insert into /drive/my/Reports
```

Read left to right: `/mail/<label>/<msg-id>/<att-id>` is the **attachment node** — one row carrying
the file's `filename`, `mime`, `size`, and `content` (the bytes). The `select` renames those columns
to what a Drive upload expects — `name`, `mime_type`, `bytes` — and `insert into /drive/my/Reports`
creates the file **inside that folder** (the terminal path segment is the destination folder; the
`name` column is the file name). At commit, qfs re-reads the attachment and materialises its bytes
straight into the upload — the content moves source→destination within the one statement, nothing
cached. A destination whose path shape can't take a columned insert (a file path) fails with a
structured error at preview; a folder that doesn't exist fails with a structured `not_found` error
at commit, when the live name→id walk runs — preview performs no network reads, so it cannot see
whether the folder is really there. Either way nothing is written. A source that produces several
rows creates one file per row, and the committed `affected` equals the files actually created.

::: tip How to know what joins
`qfs describe <path>` reports a node's verbs and its **pushdown** line — which filters run inside
the service vs. locally. It answers today for `/local`, `/mail`, and `/github` nodes; `describe` for
`/sql` and `/git` is still being wired (it returns `unknown_mount`), though the joins themselves
already run. Whatever the pushdown split, qfs over-fetches safely and re-checks locally, so you never
get wrong rows — only a bigger or smaller share of the work pushed down.
:::
