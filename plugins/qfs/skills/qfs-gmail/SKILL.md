---
name: qfs-gmail
description: Use when a task needs to read or triage Gmail through qfs — search, read, summarize, draft, send, relabel, or trash mail via the /mail path and its pipe-SQL queries. Covers connecting a Google account and the label, message, thread, attachment, and draft surface.
---

# Gmail

Your whole mailbox becomes a set of queryable paths. Labels are directories, messages are files, and
one pipe-SQL language searches, triages, drafts, sends, relabels, and cleans up — the same verbs you
already use on a database, a git repo, or a folder of files.

## Example

**Show me the mail that actually needs me** — unread, this quarter, from a real human (not a
`noreply` robot), newest first:

```qfs
/mail/unread
|> where date > '2026-04-01'
     AND NOT from LIKE '%noreply%'
|> select date, from, subject
|> order by date DESC
|> limit 20
```

```text
date        from                    subject
2026-06-30  jordan@northwind.com    Re: Q3 renewal — a couple of questions
2026-06-29  taylor@acme.io          Contract redlines attached
2026-06-27  priya@boldpeak.dev      Can we move the review to Thursday?
… 20 rows
```

That read runs the instant you connect an account. Now the **smart** part — star and mark-read a
message you found (a Gmail write acts on an **exact message id**; find it with a `select` first),
and it previews before it touches anything:

```qfs
update /mail/inbox
  set add_labels = 'STARRED', remove_labels = 'UNREAD'
  where id == '18f2a9c1b7'
```

```text
PREVIEW: 1 effect(s)
  #0 UPDATE -> mail:/mail/inbox [affected 1]
  total affected: 1
```

::: tip Reads run now; writes preview
Every **read** returns rows immediately. Every **write** (`update`, `insert`, `remove`, `call`)
*previews* by default and changes nothing — add `--commit` to apply it, `--commit-irreversible` for
the ones that can't be undone (sending, trashing). Paste any recipe below and safely watch what it
*would* do first.
:::

Gmail isn't reachable until you connect a Google account to a path — about five minutes, once, in
**[Setup](#setup)**. After that every recipe on this page works verbatim.

## Setup

::: tip Prerequisites — an operator, an account, a mount
Reaching a cloud service takes three one-time steps: a signed-in operator (`qfs init` —
**[The operator identity](/guide/operator)**), an authorized account (`qfs account add …`), and a
mount binding that account to a path (`qfs connect …`). The happy path below is exactly those
three, plus registering your OAuth app.
:::

You connect Gmail once. The happy path is four commands:

```sh
qfs init you@example.com                                   # 1. the operator + the vault
cat credentials.json | qfs app add google qmu              # 2. your OAuth app
qfs account add google --app qmu                           # 3. authorize (paste-back consent)
qfs connect /mail --driver gmail --account you@gmail.com   # 4. mount it at /mail
```

The rest of this section explains each line and the alternatives.

### 0. Prerequisites

- A Google account.
- A Google **Desktop-app** OAuth client (`client_id` + `client_secret`), downloaded from the
  [Google Cloud console](https://console.cloud.google.com/apis/credentials) as `credentials.json`,
  with the **Gmail API** enabled for the project.

### 1. Ready the machine

Cloud drivers require a signed-in operator — qfs fails closed for an anonymous one. `qfs init`
creates the encrypted credential store (where the refresh token is sealed at rest — see
**[The QFS passphrase](/guide/passphrase)**) and registers you as this machine's operator. There is
no password: your OS login is the authentication, and the email is an accountability label.
Re-running it is safe — it reports what already exists:

```sh
qfs init you@example.com
```

### 2. Hand qfs your OAuth app credentials

Register the downloaded `credentials.json` in qfs's own encrypted store, so it no longer depends on
a file on disk:

```sh
cat credentials.json | qfs app add google qmu
```

CI and agents can instead export `QFS_GOOGLE_CLIENT_ID` and `QFS_GOOGLE_CLIENT_SECRET`.

### 3. Authorize your account (get a refresh token)

Pick **one** path. Either way, **one authorization can serve Gmail, Google Drive, and Google
Analytics** — the account is stored once, under your email as its label and the chosen app label.

**A — Fresh browser consent (recommended).** On a terminal, one command prints a Google consent
URL. Open it in your **local** browser (press `c` to copy it to your local clipboard — this works
across SSH and tmux), approve, and your browser lands on a `http://localhost/...` URL that fails
to load — that's expected: nothing listens there. Paste that entire URL (or just its `code=`
value) back into the terminal, and qfs seals the refresh token and records your consent. Because
the redirect never needs to reach the qfs host, this works the same on your laptop or over plain
SSH — no port-forward:

```sh
qfs account add google --app qmu
```

**B — Pipe an existing refresh token** (reuse one a prior tool already obtained, or automate on a
headless box with no browser). The token comes in on **stdin**, never argv, and the email is the
account's label:

```sh
printf '%s' "$REFRESH_TOKEN" | qfs account add google you@gmail.com --app qmu
```

`qfs account list` shows the authorized accounts; `qfs account remove google you@gmail.com` deletes
one along with its consent record.

### 4. Connect the path

Mount Gmail wherever you like — the mount carries the account, and the rest of this cookbook
assumes `/mail`:

```sh
qfs connect /mail --driver gmail --account you@gmail.com
```

`qfs connect --list` now lists it, and `qfs describe /mail` shows the schema and verbs.
`qfs disconnect /mail` removes the mount.

::: info The mount path is yours — and so is the account it carries
`/work/gmail` works just as well as `/mail` — mount with
`qfs connect /work/gmail --driver gmail --account you@gmail.com` and every `/mail/…` recipe below
simply becomes `/work/gmail/…`. Two Gmail accounts are simply two mounts in the same process:

```sh
qfs connect /mail  --driver gmail --account work@example.com
qfs connect /mail2 --driver gmail --account home@example.com
```
:::

### 5. Read real mail

```sh
qfs run "/mail/inbox |> select date, from, subject |> limit 5"
```

Real messages come back. If a read reports *connect a Google account to read mail*, revisit steps
1–3: the path resolved (you're past addressing), but the cloud bind gate has no signed-in operator
or recorded consent yet.

## The mailbox as paths

Once connected, `/mail` is your Gmail account as an **append log** mapped onto a filesystem shape:

| Gmail thing | qfs path | it is a… |
| ----------- | -------- | -------- |
| a label | `/mail/inbox`, `/mail/sent`, `/mail/<UserLabel>` | directory of messages |
| a message | `/mail/inbox/<msg-id>` or `id:<msg-id>` | file |
| a thread | `id:thread:<thread-id>` | file (the whole conversation) |
| an attachment | `/mail/inbox/<msg-id>/<att-id>` | nested entry |
| your drafts | `/mail/drafts` | the append target you write new mail to |

Message columns: `id`, `thread_id`, `date`, `from`, `subject`, `snippet`, `label_ids`,
`attachments`. Attachment columns: `filename`, `mime`, `size`. You address an attachment by its
**position** — `att0` is the first, `att1` the second (Gmail's internal attachment id is ephemeral,
so qfs addresses by the stable index; see below). Run `qfs describe /mail/inbox` for the exact schema
and verbs of any node.

::: tip Labels are written verbatim
A label segment reaches Gmail **exactly as you write it** — qfs never rewrites the case. It becomes a
`label:` search term, which Gmail matches case-insensitively, so lowercase just works: `sent`,
`spam`, `trash`, `starred`, `important`, `unread`, the `category_*` tabs, and your own user labels
all read the same way. `drafts` is qfs's reserved write collection (the INSERT target), not a label.
:::

## Browse the mailbox

**List your labels** — the directories under `/mail`:

```qfs
/mail |> select name
```

**Read any label** — the system ones (`inbox`, `sent`, `starred`, `important`, `spam`, `unread`) and
your own (e.g. `/mail/Receipts`):

```qfs
/mail/inbox |> select date, from, subject |> limit 20
```

```qfs
/mail/sent |> select date, subject |> limit 20
```

```qfs
/mail/starred |> select date, from, subject
```

## Search & read

`WHERE` and `LIMIT` push down into Gmail's own search — qfs turns the predicate into a Gmail `q`
query and re-filters locally only the parts Gmail can't express exactly.

**Find invoices, newest first:**

```qfs
/mail/inbox
|> where subject LIKE '%invoice%'
|> select date, from, subject
|> order by date DESC
|> limit 20
```

**Everything from one sender:**

```qfs
/mail/inbox
|> where from == 'billing@stripe.com'
|> select date, subject, snippet
```

**A date range, with the attachments column:**

```qfs
/mail/inbox
|> where date BETWEEN '2026-01-01' AND '2026-03-31'
|> select date, from, subject, attachments
```

**Unread from the last quarter, across the whole mailbox:**

```qfs
/mail/unread
|> where date > '2026-04-01'
|> select date, from, subject
|> order by date DESC
```

## Read one message or attachment

**A single message** as a file under its label — the message id is the last segment (a listing's
`id` and `thread_id` columns give you the ids):

```qfs
/mail/inbox/18f1a2b3c4
|> select date, from, subject, snippet
```

**List a message's attachments** — they come back in order, so the first is `att0`, the second
`att1`. Then **download one** by that index: the attachment node `/mail/<label>/<msg-id>/att<N>`
reads `filename`, `mime`, `size`, and the decoded `content` bytes:

```qfs
/mail/inbox/18f1a2b3c4
|> select attachments

/mail/inbox/18f1a2b3c4/att0
|> select filename, mime, size, content
```

Addressing by index (not the raw Gmail attachment id) is deliberate: the id is **ephemeral** —
regenerated on every read — so qfs resolves the live id for you inside the read. That is what lets a
one-statement transfer work, e.g. **save the first attachment straight into a Drive folder**:

```qfs
/mail/inbox/18f1a2b3c4/att0
|> select filename as name, mime as mime_type, content as bytes
|> insert into /drive/my/Reports
```

## Summarize

**Who emails you most?**

```qfs
/mail/inbox
|> group by from
|> aggregate count(id) as messages
|> order by messages DESC
|> limit 10
```

## Triage — relabel, star, archive

Relabeling is an `UPDATE` that names an **exact message** — Gmail writes act on a specific message
id, not a fuzzy search (Gmail's search is lossy, so a set-wide write could touch the wrong mail).
The triage flow is **read to find, then act by id**: `select id, from, subject …` to find the
message, then `update` it. `set add_labels` / `remove_labels` (comma-separated label ids) picks what
to add and remove; it previews like any effect and only applies on `--commit`.

You can name the message two equivalent ways — by the collection with an exact `where id ==`, or by
its message node `/mail/inbox/<id>`:

**Star and mark-read one message:**

```qfs
update /mail/inbox
  set add_labels = 'STARRED', remove_labels = 'UNREAD'
  where id == '18f2a9c1b7'
```

**Archive a message** — archiving is just *removing the `INBOX` label*; the message stays in All
Mail. Here by its message node:

```qfs
update /mail/inbox/18f2a9c1b7
  set remove_labels = 'INBOX'
```

**File a receipt under a user label:**

```qfs
update /mail/inbox
  set add_labels = 'Receipts'
  where id == '18f2a9c1b7'
```

**Create a new label first.** Labels are the mailbox's directories, so making one is an `INSERT`
into the `/mail/labels` collection — a nested `Parent/Child` name creates the hierarchy. It previews
like any write and only applies on `--commit`:

```qfs
insert into /mail/labels
  values ('Work/Receipts')
```

## Write — draft and send

**Draft an email** — reversible; creating a draft sends nothing. It *previews* until you `--commit`.
Name the columns you write (`to`, `subject`, `body`): drafts are an append target whose *read* shape
(message rows) differs from its *compose* fields, so a bare positional `values ('a','b','c')` is
rejected — name them so `to` is unambiguous:

```qfs
insert into /mail/drafts
  values (to, subject, body)
         ('alice@example.com', 'Q3 report', 'See attached.')
```

```text
PREVIEW: 1 effect(s)
  #0 INSERT -> mail:/mail/drafts [affected 1]
  total affected: 1
```

**Draft with an attachment.** The `attachments` column is an **array of structs**, each
`{ filename, mime, bytes }`. Give `bytes` a hex `X'…'` literal (or a plain string for text), and name
the columns so the array lands in `attachments`:

```qfs
insert into /mail/drafts
  values (to, subject, body, attachments)
         ('alice@example.com', 'Q3 report', 'See attached.',
          [ { filename: 'hello.txt', mime: 'text/plain', bytes: X'68656c6c6f' } ])
```

The draft is built as a `multipart/mixed` message with the file attached — still a reversible preview
until you `--commit`. Piping a **Google Drive** download straight into this column (instead of an
inline literal) is the [cross-service attach-and-send recipe](/cookbook/cross-service).

**Draft, then send it.** The draft is reversible; the send is the irreversible step. Create the
draft, read `/mail/drafts` to find its **draft id** (the `id` column), then send *that* draft by
addressing it — `/mail/drafts/<draft-id> |> call mail.send`:

```qfs
insert into /mail/drafts
  values (to, subject, body)
         ('alice@example.com', 'Q3 report', 'See attached.')

/mail/drafts |> select id, subject

/mail/drafts/r8a1f2c4 |> call mail.send
```

Prefer a one-shot? Compose and send in a single step by passing the recipients to the call — no
draft to look up first:

```qfs
/mail/drafts |> call mail.send(to => 'alice@example.com', subject => 'Q3 report', body => 'See below.')
```

::: warning Irreversible
`CALL mail.send` can't be undone. In a one-shot it needs `--commit --commit-irreversible`. A retry
re-sends the **same** draft (de-duplicated by draft id), never a fresh message. A bare
`/mail/drafts |> call mail.send` with no addressed draft and no recipients is refused at plan time —
address a draft or pass `to`.
:::

**Reply into a thread.** To answer an email *in its conversation* (not as a new standalone message),
address the **parent message** and `call mail.reply`. It builds a **reversible** reply draft that
lands in the parent's thread — the `to` defaults to the parent's sender and the `subject` to
`Re: <their subject>`, so the shortest reply is just a body:

```qfs
/mail/inbox/18f2a9c1b7 |> call mail.reply(body => 'Thanks — Thursday works. See you then.')
```

```text
PREVIEW: 1 effect(s)
  #0 CALL -> mail:id:18f2a9c1b7 [affected 1]
  total affected: 1
```

Override any default — reply to a different address, add a `cc`, or set your own `subject`:

```qfs
/mail/inbox/18f2a9c1b7
|> call mail.reply(body => 'Looping in Sam, who owns the renewal.',
                   cc => 'sam@example.com')
```

The reply is a normal draft (reversible — nothing is sent yet), so **sending it is the same
`call mail.send`** as any other draft: read `/mail/drafts` for its id, then send *that* draft. Because
the draft was created in the thread, the sent message threads for every mail client — Gmail's view
and the `In-Reply-To`/`References` headers both point at the parent:

```qfs
/mail/drafts |> select id, subject

/mail/drafts/r9b2c1 |> call mail.send
```

::: tip Reply is reversible; send is not
`mail.reply` only **drafts** — it previews and can be re-run freely. The irreversible step is the
same `mail.send` you already know. A `mail.reply` with no `body`, or addressed at anything but a
parent message, is refused at plan time.
:::

## Attach & detach files

Attaching works the **same way on every form** — the `attachments` column is always an **array of
structs** `{ filename, mime, bytes }`. You already saw it on a draft (`insert into /mail/drafts`);
it rides a one-shot send and a reply too.

**Attach on a one-shot send** — pass `attachments` right in the call:

```qfs
/mail/drafts
|> call mail.send(to => 'alice@example.com', subject => 'Q3 report',
                  body => 'See attached.',
                  attachments => [ { filename: 'q3.txt', mime: 'text/plain', bytes: X'68656c6c6f' } ])
```

**Attach on a reply** — same `attachments`, alongside the reply `body`:

```qfs
/mail/inbox/18f2a9c1b7
|> call mail.reply(body => 'Full breakdown attached.',
                   attachments => [ { filename: 'breakdown.csv', mime: 'text/csv', bytes: X'612c62' } ])
```

**Detach = replace the draft's attachment set.** A draft's attachments are exactly whatever the
latest `upsert` row says — so you *detach* by re-upserting the set you want to keep, and *clear* them
by omitting `attachments` entirely. Keep one of two, dropping the other:

```qfs
upsert into /mail/drafts
  values (draft_id, to, subject, body, attachments)
         ('r8a1f2c4', 'alice@example.com', 'Q3 report', 'One file only now.',
          [ { filename: 'q3.txt', mime: 'text/plain', bytes: X'68656c6c6f' } ])
```

Detach **everything** — same `upsert`, no `attachments` column:

```qfs
upsert into /mail/drafts
  values (draft_id, to, subject, body)
         ('r8a1f2c4', 'alice@example.com', 'Q3 report', 'No attachment after all.')
```

::: tip Re-attaching a *received* file
An attachment **listing** carries metadata only — `filename`, `mime`, `size`, **no bytes**. To
forward a file you received, fetch its **bytes** from the attachment node first (its `content`
column), then feed those bytes into an `attachments` `bytes` field. Read one file's bytes with:

```qfs
/mail/inbox/18f2a9c1b7/att-9 |> select filename, mime, content
```

Piping that `content` straight into a draft's `attachments` (rather than a hand-typed `X'…'`
literal) is the [cross-service attach-and-send recipe](/cookbook/cross-service) — the same shape
works to re-attach a Gmail file or a Google Drive download.
:::

## Clean up — trash

Trashing names an **exact message id** too — the same *read to find, then act by id* flow as a
relabel (Gmail's lossy search makes a set-wide trash unsafe, so it is refused). Find the offenders
with a `select … where subject LIKE '%unsubscribe%'`, then trash each by its id.

**Trash one message** by its message node (the id is the last segment):

```qfs
remove /mail/inbox/18f1a2b3c4
```

```text
PREVIEW: 1 effect(s)
  #0 REMOVE -> mail:/mail/inbox/18f1a2b3c4 [affected 1] (!)
  (!) irreversible: 1 node(s) [#0]
  total affected: 1
```

The `(!)` marks the irreversible gate: a one-shot needs `--commit --commit-irreversible` to apply it.

**Trash by exact id on the collection** — the equivalent collection form:

```qfs
remove /mail/inbox
  where id == '18f1a2b3c4'
```

::: tip describe never lies about verbs
`qfs describe` reports the **exact** verb set for each node, derived from its real capabilities —
they differ by node, and a verb that isn't listed is rejected at parse time, never silently dropped:

- `describe /mail` → `LS SELECT` — the mailbox root lists your labels.
- `describe /mail/inbox` → `SELECT UPDATE REMOVE` — a label is a tail you read, *relabel* (`UPDATE`),
  and trash (`REMOVE`). You don't append new mail to your own inbox, so there's no `INSERT`.
- `describe /mail/drafts` → `SELECT INSERT UPSERT` — drafts is the append log you write to; its rows
  carry the **draft id** (the sendable identity). A single draft `/mail/drafts/<draft-id>` reads
  (`SELECT`) and sends (`CALL mail.send`).
- `describe id:<msg>` → `SELECT REMOVE`; `describe id:thread:<id>` → `REMOVE`; an attachment →
  `SELECT`.
:::
