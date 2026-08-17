---
name: qfs-chatwork
description: Use when a task needs Chatwork through qfs — installing and querying the DECLARED /chatwork driver (rooms, a room's latest messages, a room's unread messages, room file listings, file download, and file upload) written in the query language itself, and posting a message to a room. Covers installing the chatwork.qfs declaration, connecting it to a stored Chatwork API token, reading the latest messages versus the unread-only ones, posting a message via ENCODE form, downloading a file's bytes via the FOLLOW stage, and uploading via ENCODE multipart.
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
  send_time timestamp
)
```

```qfs
CREATE VIEW /chatwork/rooms/{room}/messages OF chatwork/message AS
  /http/chatwork/rooms/{room}/messages?force=1 |> DECODE json
```

```qfs
CREATE VIEW /chatwork/rooms/{room}/messages/unread OF chatwork/message AS
  /http/chatwork/rooms/{room}/messages |> DECODE json
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

`qfs describe /chatwork/rooms` then lists the declared views credential-free.

### 3. Check later whether your installation is still current

The rows you committed in step 1 **are** the driver from now on — the shipped `chatwork.qfs` is
never read again. When a later qfs release corrects the declaration (this one has been corrected
twice), your mount keeps running the text you installed. Ask:

```qfs
/sys/declarations |> where status == 'stale'
```

An empty answer means every installed declaration matches the one your binary ships. A `chatwork`
row means it does not, and its `differs` column names the `CREATE …` statements to preview and
commit again. Nothing is upgraded for you: re-installing is your write to make, so a declaration
you customised is never silently overwritten. See the FAQ for the full three-value `status`.

## Read the latest messages in a room

Address a room by its id (from `/chatwork/rooms`), newest first:

```qfs
/chatwork/rooms/123456/messages
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
