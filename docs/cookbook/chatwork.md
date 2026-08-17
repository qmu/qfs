---
skill_name: qfs-chatwork
skill_description: Use when a task needs Chatwork through qfs — installing and querying the DECLARED /chatwork driver (rooms and their unread/mention counts, a room's latest messages with their sender, a room's unread messages, room file listings, file download, and file upload) written in the query language itself, and posting or replying to a message in a room. Covers installing the chatwork.qfs declaration, connecting it to a stored Chatwork API token, walking the declared surface with describe, selecting conversations by unread count, attributing a message to its sender, polling a room incrementally with a send_time filter, composing a threaded reply, posting via ENCODE form, downloading a file's bytes via the FOLLOW stage, and uploading via ENCODE multipart.
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
|> where unread_num > 0
|> select name, unread_num, mention_num
|> order by unread_num DESC
```

```text
name              unread_num  mention_num
Deploys                    7            1
Ops                        2            0
… 2 rows
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

A declared type is the **delivered contract**: the view projects each row to exactly these columns,
so a field the type omits is a field no caller can reach. That is why the shipped types carry
everything the endpoint answers rather than a convenient subset — the room type keeps
`unread_num` / `mention_num` / `message_num` / `last_update_time`, and the message type keeps the
sender and `update_time`.

Chatwork nests the sender under an `account` object, so the two sender columns are **lifted** in the
view body with `EXTEND` before the `OF` projection runs:

```qfs
CREATE VIEW /chatwork/rooms/{room}/messages OF chatwork/message AS
  /http/chatwork/rooms/{room}/messages?force=1 |> DECODE json
  |> EXTEND account_id = account.account_id, account_name = account.name
```

```qfs
CREATE VIEW /chatwork/rooms/{room}/messages/unread OF chatwork/message AS
  /http/chatwork/rooms/{room}/messages |> DECODE json
  |> EXTEND account_id = account.account_id, account_name = account.name
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

`qfs describe /chatwork/rooms` then reports the node credential-free.

## Walk the surface with `describe`

A declared mount answers `describe` the same way a compiled driver does, so you never have to read
the `.qfs` source to find out what is there. Start at the mount root and follow `children`:

```text
$ qfs describe /chatwork --json
{"path":"/chatwork", …
 "children":[{"segment":"rooms","path":"/chatwork/rooms","param":null,"node":true}]}

$ qfs describe /chatwork/rooms --json
{"path":"/chatwork/rooms","row_contract":"/type/chatwork/room",
 "columns":[{"name":"room_id","ty":"Text",…},{"name":"unread_num","ty":"Int",…}, …],
 "children":[{"segment":"{room}","path":"/chatwork/rooms/{room}","param":"room","node":false}]}

$ qfs describe /chatwork/rooms/{room} --json
{"path":"/chatwork/rooms/{room}","columns":[],
 "children":[{"segment":"messages","path":"/chatwork/rooms/{room}/messages","param":null,"node":true},
             {"segment":"files","path":"/chatwork/rooms/{room}/files","param":null,"node":true}]}
```

Three things a caller reads off that:

- **`children`** is the declared surface beneath a node, so the tree is walkable from the mount down.
- **`param`** marks a segment you bind rather than type literally — `{room}` wants a room id.
- **`node`** separates an addressable view from a segment that only exists to walk through.

The columns are the declared `OF` type's, named in `row_contract` — the same set a read of the path
delivers, so the statement you write from `describe` output is the statement that runs. A view with
no `OF` type reports an empty `columns` list and a null `row_contract`: "nothing declared here" is
stated, never papered over with a stand-in column.

## Find the conversations with something new

`GET /rooms` already answers the counts, so "which conversations need me" is a `WHERE`, not a scan
of every room's messages:

```qfs
/chatwork/rooms
|> where mention_num > 0 or unread_num > 5
|> select room_id, name, unread_num, mention_num, last_update_time
|> order by mention_num DESC, unread_num DESC
```

## Read the latest messages in a room

Address a room by its id (from `/chatwork/rooms`), newest first. `account_name` attributes each row
— without it you cannot tell who wrote what in any room with more than two people:

```qfs
/chatwork/rooms/123456/messages
|> select send_time, account_name, body
|> order by send_time DESC
|> limit 20
```

That path returns the room's most recent messages **every time it is read**. It is worth saying why
it needs its own spelling: `GET /rooms/{id}/messages` defaults to *unread messages only*, so a view
declared without `force=1` returns rows on the first read of a room and nothing on the next one
until someone posts again. The shipped `…/messages` view passes `force=1` — a path names a place,
and reading it twice has to answer the same question.

## Read only what is unread in a room

The unread reading is the API's cheap default call, and it keeps a name of its own:

```qfs
/chatwork/rooms/123456/messages/unread
|> select body, send_time
```

This one **consumes**: Chatwork marks the returned messages read, so the next read of the same room
returns only what arrived in between. Reach for it when you want "what is new since I last looked";
reach for `…/messages` when you want the room's latest messages.

## Poll a room for what arrived since you last looked

Keep the `send_time` of the newest row you have seen, and filter on it. Chatwork's messages endpoint
takes no since-parameter, so the predicate is applied **locally** — the request is the same one GET,
and only the new rows come back to you:

```qfs
/chatwork/rooms/123456/messages
|> where send_time > 1755400000
|> select send_time, account_name, body
|> order by send_time
```

This is the honest version of an incremental read: qfs never claims a server-side filter the API
does not have. `…/messages/unread` (below) is the cheap alternative when a *consuming* read is
acceptable.

## Reply to a message

A Chatwork reply is an ordinary message whose body carries a
`[rp aid=<account_id> to=<room>-<message_id>]` prefix. Both ids are columns now, so the reply
composes from the message you are replying to instead of being retyped:

```qfs
/chatwork/rooms/123456/messages
|> where account_name == 'Alice'
|> order by send_time DESC
|> limit 1
|> select CONCAT('[rp aid=', account_id, ' to=123456-', message_id, ']\nOn it 👍') as body
|> insert into /chatwork/rooms/123456/messages
```

Without the prefix every message qfs sends is an unthreaded post, which reads as a visible downgrade
from how a person uses the service.

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
