---
skill_name: qfs-slack
skill_description: Use when a task needs Slack through qfs — read the latest messages in a channel, list and download the files shared in a channel or DM (newest first), upload a file's bytes and detach (delete) it, and post a message over /slack, as an append log. Covers connecting a Slack workspace.
---

# Slack

A Slack channel is an **append log** with a filesystem shape: its messages become a queryable path
you read the tail of, and post to — the same pipe-SQL language you already use on a mailbox, a
database, or a git repo.

## Example

**Catch up on a channel** — the latest messages in `#general`, newest first:

```qfs
/slack/acme/general/messages
|> select ts, user, text
|> order by ts DESC
|> limit 20
```

```text
ts                   user     text
2026-06-30 16:42     jordan   shipping the Q3 build now 🚀
2026-06-30 15:10     priya    review's done, LGTM
2026-06-30 11:58     taylor   standup moved to 10:30 tomorrow
… 20 rows
```

That read runs the instant you connect a workspace. Posting back is just as direct — one statement
appends a message, and previews before it sends anything:

```qfs
insert into /slack/acme/general/messages
  values ('Deploy finished ✅')
```

```text
PREVIEW: 1 effect(s)
  #0 INSERT -> slack:/slack/acme/general/messages [affected 1]
  total affected: 1
```

::: tip Reads run now; writes preview
Every **read** returns rows immediately. Every **write** (`insert`) *previews* by default and posts
nothing — add `--commit` to actually send it. Paste any recipe below and safely watch what it
*would* do first.
:::

Slack isn't reachable until you connect a workspace to a path — one command, in **[Setup](#setup)**.
After that every recipe on this page works verbatim.

## Setup

::: tip Prerequisites — an operator, an account, a mount
Reaching a cloud service takes three one-time steps: a signed-in operator (`qfs init` —
**[The operator identity](/guide/operator)**), an authorized account (`qfs account add …`), and a
mount binding that account to a path (`qfs connect …`). The happy path below is exactly those
three.
:::

A Slack read needs a workspace token bound to a mount:

```sh
qfs init you@example.com                               # 1. the operator + the vault (once per machine)
printf '%s' "$SLACK_TOKEN" | qfs account add slack     # 2. the workspace token (label: `default`)
qfs connect /slack --driver slack --account default    # 3. mount it at /slack
```

The token comes in on **stdin**, never argv, and is sealed in qfs's encrypted credential store.
Until the mount is bound, a read fails with an actionable hint naming the
`qfs account add slack …` / `qfs connect …` to run. Posting a message previews with no account
(above); it sends only once connected and committed.

## The channel as a path

Once connected, a workspace's channels hang off `/slack` in a filesystem shape:

| Slack thing | qfs path | it is a… |
| ----------- | -------- | -------- |
| a workspace | `/slack/acme` | directory of channels |
| a channel's log | `/slack/acme/general/messages` | the append log you read and post to |

Message columns: `ts`, `user`, `text`. Run `qfs describe /slack/acme/general/messages` for the exact
schema and verbs of the node.

## Read the channel

**Read the latest messages** — the tail of the log:

```qfs
/slack/acme/general/messages
|> select text
|> limit 20
```

**Search a channel for anything that looks like an incident** — `WHERE` narrows the log before it
comes back:

```qfs
/slack/acme/incidents/messages
|> where text ~ '(?i)(outage|sev[0-9]|rollback|paging)'
     OR text LIKE '%down%'
|> select ts, user, text
|> order by ts DESC
|> limit 100
```

## Files shared in a channel or DM

A channel's or DM's shared files are their own listing — `/slack/acme/general/files` (a channel, by
`#name`) and `/slack/acme/dms/U07ALICE/files` (a DM, by the peer's Slack **user id**). The listing is
scoped by Slack's own file-share record for that conversation (not by who uploaded a file, nor by
upload time alone), so "the latest file in this DM" is provably that DM's newest share. File
columns: `id`, `name`, `mimetype`, `size`, `created`, `user`. These listings are read-only.

A DM is addressed by the peer's **user id** (`U…`, the same form `/slack/<ws>/dms/<user>/messages`
uses), not a display name — qfs opens the IM channel (`conversations.open`) from that id. Look the id
up in the workspace directory: `/slack/acme/users |> where name == 'alice' |> select id`.

**The latest file dropped in a DM:**

```qfs
/slack/acme/dms/U07ALICE/files
|> order by created DESC
|> limit 1
```

**Files shared in a channel, newest first:**

```qfs
/slack/acme/incidents/files
|> select name, size, created, user
|> order by created DESC
```

Download one by its id — `/slack/acme/files/F0123` returns a `content` column carrying the bytes,
which you can write on to Drive or disk (see [files & object storage](/cookbook/files)).

## Upload a file to Slack (and detach)

Write a file's **bytes** into the workspace file namespace with `UPSERT INTO /slack/<ws>/files`. The
row carries the same `{filename, mime, bytes}` vocabulary a Gmail attachment and a Drive blob speak,
so a file flows in from any service without reshaping. Add an optional `channel` to share it there:

```qfs
/drive/my/report.pdf
|> select name as filename, mime_type as mime, content as bytes, 'C0INCIDENTS' as channel
|> upsert into /slack/acme/files
```

Under the hood this is Slack's external-upload flow (reserve an upload URL, send the bytes, complete
the share); the legacy `files.upload` is retired for new apps. Like every write it previews first and
sends the bytes only on `--commit`. The bytes travel out-of-band of the JSON API, so no file content
ever lands in a request log.

**Detach** — remove a file by its id. A delete is irreversible, so it needs the explicit gate:

```qfs
remove /slack/acme/files/F0123
```

```text
qfs run -e "remove /slack/acme/files/F0123" --commit-irreversible
```

## Post a message

**Post to a channel** — an `INSERT` appends to the log. It previews the append and applies nothing
until `--commit`:

```qfs
insert into /slack/acme/general/messages
  values ('Deploy finished ✅')
```

```text
PREVIEW: 1 effect(s)
  #0 INSERT -> slack:/slack/acme/general/messages [affected 1]
  total affected: 1
```

::: tip
Want a deploy to post to Slack by itself? Wire it up once with a trigger — see
[Automation](/cookbook/automation).
:::

::: tip One positional value binds to `text`
`values ('…')` with a single value posts that text — the bare form above and the explicit
`values (text) ('…')` form are equivalent, and both apply the same at `--commit` as they preview.
Reach for the named-column form (`values (text) ('…')`) when a row also carries other columns.
:::

## Post as yourself (a user token)

By default a workspace mount posts as the **bot** the app installed. To post as a **human user** —
no bot badge, the message attributed to that person — bind a **user token** (`xoxp-…`) on its own
mount. The driver hands whatever credential the mount holds to Slack verbatim; Slack's own semantics
decide the author, so nothing inside qfs changes.

1. On the Slack app config (api.slack.com/apps) add the **User Token Scopes** you need —
   `chat:write` to post (add `channels:history` too if this mount should also read the tail) — then
   **Reinstall to Workspace** and authorize as the target user. Copy the **User OAuth Token**
   (`xoxp-…`).
2. Register it as a second account and mount it beside the bot, keeping the bot mount intact:

```sh
printf %s "$XOXP_TOKEN" | qfs account add slack me   # a second account, labelled `me`
qfs connect /slack-me --driver slack --account me     # its own mount, bound to that account
```

3. Post through the user mount — it lands as the person, not the app:

```qfs
insert into /slack-me/acme/general/messages
  values ('Sent from my own account 👋')
```

::: warning The app page shows only the installer's token
`api.slack.com/apps` only ever reveals the **installer's** user token. For anyone else to obtain
their own `xoxp-`, they must complete a real OAuth consent flow (authorize URL → code →
`oauth.v2.access`) — the same shape as a Gmail code-flow helper: open a URL, click approve.
:::

## Team proxy pattern — everyone's AI posts as themselves

One workspace app with `user_scope=chat:write`; each member authorizes once and receives their own
`xoxp-` token. **Where those tokens live is the design decision** — two shapes, two trust
boundaries:

| topology | where tokens live | who can speak as you |
| -------- | ----------------- | -------------------- |
| **Per-member vault** (recommended) | each member's own machine (`qfs account add slack …` run locally) | only that member's own agent |
| **Central box** | one operator's vault holds every member's token | whoever controls that one host |

The central box **concentrates impersonation power in a single host** — anyone who controls it can
post as any member. If you must run it, treat it like a production secret store: minimal scopes
(`chat:write` only), explicit consent recorded from every member, and rotate **all** tokens if the
host is ever compromised. The per-member vault keeps each person's impersonation power on their own
machine and is the default recommendation.

Operational notes: agree a **disclosure norm** up front (whether AI-authored messages are marked as
such), and remember revocation is per-user from Slack's app-management page — a member can cut their
own token without disturbing anyone else's.

### Scopes at a glance

| you want to… | token | scope |
| ------------ | ----- | ----- |
| post as the bot | bot (`xoxb-…`) | `chat:write` |
| post as yourself | user (`xoxp-…`) | `chat:write` (a **user** scope) |
| read the channel tail on that mount | either | `channels:history` |

A mount posts fine without `channels:history`; it needs that scope only to *read* the log — a bot
token missing it posts successfully but returns nothing on a tail read, worth checking first if
reads come back empty.
