# Cookbook: Mail

`/mail` is an **append log**. You read the tail of `/mail/inbox` (and relabel or trash its
messages); you *append* new mail by writing to `/mail/drafts`. Columns include `date`, `from`,
`subject`, `snippet`, `label_ids`, and `attachments`. Sending is an irreversible `CALL`.

Run `qfs describe /mail/inbox` to see the full schema for your mailbox.

::: warning Needs a connected account
The **read** recipes below run only once a Google account is connected. Without one, a `/mail/…`
read returns an actionable error: *connect a Google account to read mail — run
`qfs identity signup <email>`, then `qfs connection add gmail`*. The **write previews** (draft,
send, trash) work right now with no account — they show the plan and change nothing until you
`--commit`.
:::

## Search & read

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
|> select date, subject
```

**Mail with attachments in a date range:**

```qfs
/mail/inbox
|> where date BETWEEN '2026-01-01' AND '2026-03-31'
|> select date, from, subject, attachments
```

**Read a single message by id:**

```qfs
/mail/inbox
|> where id == '18f1a2b3c4'
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

## Write

**Draft an email** (reversible — creating a draft sends nothing). Writing a draft *previews* with no
account; it only reaches Gmail once you connect one and `--commit`:

```qfs
insert into /mail/drafts
  values ('alice@example.com', 'Q3 report', 'See attached.')
```

```text
PREVIEW: 1 effect(s)
  #0 INSERT -> mail:/mail/drafts [affected 1]
  total affected: 1
```

**Draft, then send it.** The draft is reversible; the send is the irreversible step:

```qfs
insert into /mail/drafts
  values ('alice@example.com', 'Q3 report', 'See attached.')

/mail/drafts
|> call mail.send
```

::: warning Irreversible
`CALL mail.send` can't be undone. In a one-shot it needs `--commit --commit-irreversible`. A retry
re-sends the **same** draft (de-duplicated), never a fresh message.
:::

## Clean up

**Trash newsletters** (also irreversible — the preview shows it as a gate):

```qfs
remove /mail/inbox
  where subject LIKE '%unsubscribe%'
```

```text
PREVIEW: 1 effect(s)
  #0 REMOVE -> mail:/mail/inbox [affected ?] (!)
  (!) irreversible: 1 node(s) [#0]
  total affected: ?
```

The `(!)` marks the irreversible gate: a one-shot needs `--commit --commit-irreversible` to apply it.

**Trash by sender:**

```qfs
remove /mail/inbox
  where from == 'noreply@spammy.example'
```

::: tip describe never lies about verbs
`qfs describe` reports the **exact** verb set for each node, derived from its real capabilities.
A mailbox and its drafts differ:

- `describe /mail/inbox` → `native_verbs: SELECT(tail) UPDATE REMOVE`. The inbox is a tail you read,
  *relabel* (`UPDATE`), and trash (`REMOVE`) — you don't append new mail to your own inbox, so
  there's no `INSERT`.
- `describe /mail/drafts` → `native_verbs: SELECT(tail) INSERT(append) UPSERT REMOVE`. Drafts is the
  append log you write to.

If a verb isn't listed, the statement is rejected — never silently dropped.
:::
