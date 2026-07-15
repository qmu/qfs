---
name: qfs-chatwork
description: Use when a task needs Chatwork through qfs — installing and querying the DECLARED /chatwork driver (rooms, room messages, room file listings, file download, and file upload) written in the query language itself, and posting a message to a room. Covers installing the chatwork.qfs declaration, connecting it to a stored Chatwork API token, reading, posting, downloading a file's bytes via the FOLLOW stage, and uploading via ENCODE multipart.
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
  /http/chatwork/rooms/{room}/messages |> DECODE json
```

```qfs
CREATE MAP INSERT /chatwork/rooms/{room}/messages AS
  INSERT INTO /http/chatwork/rooms/{room}/messages VALUES (row)
```

### 2. Connect the token

Store your Chatwork API token in the vault, then bind it to the mount — the declaration stays
credential-free; the value lives in the account layer:

```text
qfs account add chatwork work        # paste the x-chatworktoken value (stdin, into the vault)
qfs connect /chatwork TO chatwork SECRET 'vault:chatwork/work'
```

`qfs describe /chatwork/rooms` then lists the declared views credential-free.

## Read the latest messages in a room

Address a room by its id (from `/chatwork/rooms`), newest first:

```qfs
/chatwork/rooms/123456/messages
|> order by send_time DESC
|> limit 20
```

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
