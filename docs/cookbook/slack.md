---
skill_name: qfs-slack
skill_description: Use when a task needs Slack through qfs — read the latest messages in a channel, list and download the files shared in a channel or DM (newest first), upload a file's bytes and detach (delete) it, and post a message over /slack, as an append log. Covers creating the Slack app, the bot-token scopes it needs, and connecting a Slack workspace.
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

Slack isn't reachable until an app in the workspace hands qfs a token and that token is bound to a
path — **[What to create on the Slack side](#what-to-create-on-the-slack-side)**, then
**[Setup](#setup)**. After that every recipe on this page works verbatim.

## What to create on the Slack side

The `$SLACK_TOKEN` that [Setup](#setup) pipes into the vault is a **bot token** (`xoxb-…`), issued by
a Slack app you install into the workspace you want to reach. There is no way to obtain one without
creating that app, so this is step zero for every workspace — including a second one added alongside
a workspace you already read.

1. Open **api.slack.com/apps** and choose **Create New App** → **From scratch**. Give the app a name
   and pick the workspace to install it into. That choice is permanent for this token: the app
   reaches one workspace, so a second workspace means a second app.
2. In the app's sidebar open **OAuth & Permissions**, scroll to **Scopes**, and add under **Bot
   Token Scopes**: `chat:write` to post, and `channels:history` to read a **public** channel's tail —
   or `groups:history` if the channel you actually want to read is **private**, which is a separate
   scope in Slack's model, not a stronger version of the same one. Add nothing else yet: every
   further capability on this page has its own scope, listed in
   [Scopes at a glance](#scopes-at-a-glance), and a scope you never granted is one that cannot be
   used against you.
3. Scroll back up to **OAuth Tokens for Your Workspace**, press **Install to Workspace**, and
   approve the consent screen. The page then shows a **Bot User OAuth Token** beginning `xoxb-`.
4. Copy that token and give it to qfs through [Setup](#setup) below — on **stdin**, never as a
   command-line argument.
5. In Slack itself, invite the app to every channel it must read: type `/invite @<your app name>` in
   that channel.

::: warning A channel the app has not joined refuses the read
Step 5 is the one that gets skipped, and nothing in the Slack app config mentions it. A scope grants
the *ability* to read history; it never grants membership of a particular channel, and a tail read is
`conversations.history` against that one channel. The read fails until the app is a member — and on a
**private** channel it fails as `channel_not_found`, because a channel the app cannot see is reported
as one that does not exist rather than one it has not joined.

Observed on a private channel, in this order:

| state | what comes back |
| ----- | --------------- |
| app not invited | `channel_not_found` |
| app invited, only `chat:write` + `channels:history` granted | `missing_scope`, naming `groups:history` |
| `groups:history` granted and the app reinstalled | the messages |

Two different causes with two different fixes, and the error names which one you are looking at.
Read it before changing scopes: `channel_not_found` on an id you copied out of Slack itself means
the invite, not the grant.
:::

**A second workspace is a second app, a second account label, and a second mount** — nothing is
shared between them. Repeat the steps above in the other workspace, then give its token its own
label and its own path, leaving the first mount untouched:

```sh
printf %s "$SLACK_TOKEN" | qfs account add slack acme   # a second account, labelled `acme`
qfs connect /slack-acme --driver slack --account acme   # its own mount, bound to that account
```

The label and the mount path are yours to choose; the `default` label and the `/slack` path in
[Setup](#setup) are one instance of these same two commands, not fixed names.

**Undoing it** is those two layers in reverse. On the Slack side, uninstall the app — or revoke its
token — from the workspace's app-management page. On the qfs side, `qfs disconnect /slack-acme`
removes the mount and `qfs account remove slack acme` deletes the token together with its consent
record.

To post as a **person** instead of as the app, the token is a different one and its scopes live under
**User Token Scopes** — see [Post as yourself (a user token)](#post-as-yourself-a-user-token).

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

Grant only the rows you use. Each names the scope Slack requires for the Web API method that path
actually calls, so a scope you omit shows up as that one path failing while everything else keeps
working.

| you want to… | token | scope |
| ------------ | ----- | ----- |
| post a message (`insert into …/messages`) | bot (`xoxb-…`) | `chat:write` |
| post as yourself | user (`xoxp-…`) | `chat:write` (a **user** scope) |
| read a **public** channel's tail, its thread replies, and its reactions | either | `channels:history` |
| read a **private** channel's tail, its thread replies, and its reactions | either | `groups:history` |
| list the workspace's public channels, and name one by `#name` in a `CALL` | either | `channels:read` |
| list the workspace's private channels the app is in | either | `groups:read` |
| read the user directory — including looking up a DM peer's `U…` id | either | `users:read` |
| open a DM (`/slack/<ws>/dms/<user>`) | either | `im:write` |
| read a DM's messages | either | `im:history` |
| list the files shared in a channel, a DM, or the workspace | either | `files:read` |
| add a reaction (`slack.react`) | either | `reactions:write` |
| pin or unpin a message (`slack.pin` / `slack.unpin`) | either | `pins:write` |
| edit or delete a message (`slack.update` / `slack.delete`) | either | `chat:write` |

The `channels:` and `groups:` pairs are separate grants, not a weak and a strong form of one grant:
an app holding `channels:history` reads nothing from a private channel, and the refusal names
`groups:history` as what it needed. Adding a scope after installing does not take effect until you
**Reinstall to Workspace**.

Every scope above is an *ability*, never access to a particular conversation — the app still has to
be in the channel. See
[What to create on the Slack side](#what-to-create-on-the-slack-side) for the two failures that
distinguishes and how to tell them apart.
