---
skill_name: qfs-chatwork
skill_description: Use when a task needs Chatwork through qfs — installing and querying the DECLARED /chatwork driver (rooms, room messages, room file listings, file download, and file upload) written in the query language itself, and posting a message to a room. Covers installing the chatwork.qfs declaration, connecting it to a stored Chatwork API token, reading, posting a message via ENCODE form, downloading a file's bytes via the FOLLOW stage, and uploading via ENCODE multipart.
---

# Chatwork (declared driver)

`/chatwork` is a **declared driver**: an integration written in qfs's own query language
(`CREATE DRIVER … CREATE VIEW …`) rather than compiled Rust. Chatwork is an API-key REST API — the
token rides an `x-chatworktoken` header — so the whole surface is expressible as declarative config
over the generic `/http` wire primitive. Installing it is an ordinary preview/commit; connecting it
evaluates it. This is the API-key twin of the shipped `/cloudflare` declared driver.

## Example

Once installed and connected (**[Setup](#setup)**), your rooms are a path:

```qfs
/chatwork/rooms
|> select name, type, role
|> order by name
```

```text
name              type   role
Deploys           group  admin
Ops               group  member
… 6 rows
```

That read runs live against Chatwork's REST API — the token is resolved from qfs's vault, never
typed on the command line, and the declaration is **structurally unable** to address any host other
than Chatwork's (host confinement, enforced at install).

## Setup

Installing a declared driver is two steps: **install** the declaration (a local, previewed write to
`/sys/drivers` — zero network), then **connect** it to the Chatwork API token you hold.

### 1. Install the declaration

The shipped `chatwork.qfs` declares the driver, its row types, and its resources. Preview then commit
each statement (each desugars to one `/sys/drivers` row). `AUTH HEADER` names only the header — the
token value never appears in the script:

```qfs
CREATE DRIVER chatwork
  AT 'https://api.chatwork.com/v2'
  AUTH HEADER 'x-chatworktoken'
```

```qfs
CREATE TYPE chatwork/message (
  message_id text PRIMARY KEY,
  body text NOT NULL,
  send_time timestamp,
  update_time timestamp,
  account_id text,
  account_name text
)
```

The sender arrives as a **nested** `account` object on each message, so the view lifts
`account.account_id` and `account.name` into the top-level columns the `OF` contract names:

```qfs
CREATE VIEW /chatwork/rooms/{room}/messages OF chatwork/message AS
  /http/chatwork/rooms/{room}/messages
  |> DECODE json
  |> SELECT message_id, body, send_time, update_time,
            account.account_id AS account_id, account.name AS account_name
```

```qfs
CREATE MAP INSERT /chatwork/rooms/{room}/messages AS
  INSERT INTO /http/chatwork/rooms/{room}/messages |> ENCODE form VALUES (row)
```

`ENCODE form` is load-bearing on that last map. Chatwork's message endpoint — like most plain-REST
APIs of its generation — accepts parameters only as `application/x-www-form-urlencoded` and answers
a JSON request body with `400`. The generic form encoder renders the row struct's scalar fields as
percent-encoded `k=v&k=v` and sets the content type, so the declaration needs no Chatwork-specific
code.

### 2. Connect the token

Store your Chatwork API token in the vault, then bind it to the mount — the declaration stays
credential-free; the value lives in the account layer:

```text
qfs account add chatwork work        # paste the x-chatworktoken value (stdin, into the vault)
qfs connect /chatwork TO chatwork SECRET 'vault:chatwork/work'
```

## Discover the surface with `describe`

Start at the mount root — `describe` names the children beneath it, so the surface is walkable
without reading the declaration's source:

```text
qfs describe /chatwork
```

```text
path:      /chatwork
archetype: relational (SELECT JOIN INSERT UPDATE UPSERT)
columns:   (none declared — this node states no row type; run it to see its columns)
children:
  /chatwork/rooms
verbs:     (none)
procedures: (none)
aliases:   (none)
pushdown:  limit
```

The root bears no rows and answers no verb of its own — `verbs:` is the authoritative line, and the
parenthesised archetype vocabulary is the shape hint, not a claim about this node.

Walk down one segment at a time. A `{param}` child is legible as a parameter — substitute a real
value for it:

```text
qfs describe /chatwork/rooms      # children: /chatwork/rooms/{room}  (room — substitute a value)
qfs describe /chatwork/rooms/123456   # children: …/messages, …/files
```

At a leaf, `describe` reports the declared `OF` columns — the **same** columns `run` delivers, so
you can build the next statement straight from them:

```text
qfs describe /chatwork/rooms/123456/messages
```

```text
path:      /chatwork/rooms/123456/messages
archetype: relational (SELECT INSERT)
columns:   (declared OF /type/chatwork/message)
  | name         | type      | null
  | ------------ | --------- | ----
  | message_id   | Text      | yes
  | body         | Text      | no
  | send_time    | Timestamp | yes
  | update_time  | Timestamp | yes
  | account_id   | Text      | yes
  | account_name | Text      | yes
verbs:     SELECT INSERT
pushdown:  limit
```

A node declaring no row type says exactly that instead of reporting a placeholder column — the blob
view below is one, and its columns are whatever the wire delivers.

## Read the latest messages in a room

Address a room by its id (from `/chatwork/rooms`), newest first, attributed to their senders:

```qfs
/chatwork/rooms/123456/messages
|> select send_time, account_name, body
|> order by send_time DESC
|> limit 20
```

## Find the conversations with something new

The room type carries Chatwork's own counters, so "what needs my attention" is one query rather
than a scan of every room's name:

```qfs
/chatwork/rooms
|> where mention_num > 0 or unread_num > 0
|> select name, mention_num, unread_num, last_update_time
|> order by mention_num DESC
```

## Read only what arrived since you last looked

`send_time` is a column, so an incremental poll is a `where` on it:

```qfs
/chatwork/rooms/123456/messages
|> where send_time > 1700000000
|> select send_time, account_name, body
```

`describe` reports `pushdown: limit` for these nodes, and that is the honest answer: Chatwork's
messages endpoint takes no "since" parameter, so the filter runs **locally** in qfs over the page
the API returns. The query is still the right one to write — it just does not save the round trip.

## Reply to a message

A Chatwork reply is an ordinary message whose body carries a reply prefix naming the account being
replied to and the message being replied to. Both ids are columns, so the prefix composes from a row
you already read:

```text
[rp aid=<account_id> to=<room_id>-<message_id>] <your text>
```

Read the message you are answering, then post the composed body:

```qfs
/chatwork/rooms/123456/messages
|> where message_id == '1000000000'
|> select CONCAT('[rp aid=', account_id, ' to=123456-', message_id, '] On it') as body
|> insert into /chatwork/rooms/123456/messages
```

Without `account_id` this is not expressible — which is why the message type carries the sender.

## List the files shared in a room

```qfs
/chatwork/rooms/123456/files
|> select filename, filesize
|> order by filename
```

## Post a message to a room

An `INSERT` appends to the room. Like every write it previews first and sends only on `--commit`:

```qfs
insert into /chatwork/rooms/123456/messages values (body) ('Deploy shipped ✅')
```

The declared map's `ENCODE form` turns that row into the form body the endpoint requires. What the
encoder does with each field is stated, not implicit: scalar fields (text, int, float, bool,
timestamp) are percent-encoded in declaration order; a `NULL` field is **omitted** rather than sent
empty; and a bytes, array, or nested-struct field is **refused** with a structured error naming the
reason — a form body has no binary or nested encoding, so an upload belongs in `ENCODE multipart`
(below) instead of being silently flattened onto the wire.

## Download a file's bytes

The shipped `chatwork.qfs` declares a blob view over the two-step download: the metadata GET
returns a temporary `download_url` on a *different* host, and the generic `FOLLOW` stage performs
the second GET off that delivered field — the raw bytes arrive as a one-row `content` column. The
follow request carries **no credential** (the URL is self-authorizing), so the token never leaves
Chatwork's API host:

```qfs
/chatwork/rooms/123456/files/789/blob
```

The view behind it (already in `chatwork.qfs`, shown for the shape):

```qfs
CREATE VIEW /chatwork/rooms/{room}/files/{file}/blob AS
  /http/chatwork/rooms/{room}/files/{file}?create_download_url=1
  |> DECODE json |> FOLLOW download_url
```

## Upload a file to a room

`POST /rooms/{id}/files` is `multipart/form-data`; the declared map's `ENCODE multipart` produces
it generically — a bytes field becomes the file part (named by the sibling `filename` text field),
every other scalar field a plain part. Pipe a blob in from any service and shape the row to
`file` (bytes), `filename`, and an optional `message`:

```qfs
/drive/my/monthly.pdf
|> select content as file, name as filename, 'monthly report' as message
|> insert into /chatwork/rooms/123456/files
```

The map behind it (already in `chatwork.qfs`):

```qfs
CREATE MAP INSERT /chatwork/rooms/{room}/files AS
  INSERT INTO /http/chatwork/rooms/{room}/files |> ENCODE multipart VALUES (row)
```
