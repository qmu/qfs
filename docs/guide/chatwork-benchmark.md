# Chatwork + Drive benchmark

This page records the current benchmark for using a declared Chatwork integration with the local qfs
environment. It is intentionally a guide page, not a cookbook chapter: the Chatwork declaration
surface installs as qfs configuration and can resolve the token from the encrypted qfs vault, while
live Chatwork reads still need benchmark verification against a real room before the docs can claim
full cookbook behavior.

What is verified in this worktree:

- `/drive` is mounted as `gdrive`, and `/drive/my` returns real My Drive rows after qfs auth.
- `.env` can be used as a bootstrap source for `CHATWORK_API_TOKEN=`, verified without printing the
  token value.
- The Chatwork `CREATE DRIVER`, `CREATE TYPE`, `CREATE VIEW`, `CREATE MAP`, and `CONNECT` statements
  below install as qfs configuration writes.
- `qfs account add chatwork work` stores the Chatwork token in the encrypted qfs vault.
- `CONNECT /chatwork TO chatwork SECRET 'vault:chatwork/work'` resolves the vault credential at
  request time without storing the token value in qfs config rows.

## Environment contract

Prefer the encrypted qfs vault for the durable credential. `.env` can be used only as a bootstrap
source and should never be committed:

```dotenv
CHATWORK_API_TOKEN=...
```

Import the token into the qfs vault by piping it on stdin, never as an argument:

```sh
set -a
. ./.env
set +a
test -n "${CHATWORK_API_TOKEN:?CHATWORK_API_TOKEN is required}"
printf %s "$CHATWORK_API_TOKEN" | qfs account add chatwork work
```

The repository includes `.env.example` with the variable name and ignores real `.env` files.

## Drive probe

The Drive side of the benchmark uses My Drive:

```sh
qfs connect --list
qfs run "/drive/my |> select name, mime_type |> limit 5" --json
```

The first command should show `/drive gdrive`. The second command should return rows from My Drive.

## Declare Chatwork

Preview each declaration first. These statements insert secret-free configuration rows into
`/sys/drivers`; the auth line names the Chatwork header scheme, not the token value.

```sh
qfs run "CREATE DRIVER chatwork AT 'https://api.chatwork.com/v2' AUTH HEADER 'x-chatworktoken'" --json
qfs run "CREATE TYPE chatwork/room (room_id int PRIMARY KEY, name text NOT NULL)" --json
qfs run "CREATE TYPE chatwork/message (message_id text PRIMARY KEY, body text NOT NULL, send_time timestamp)" --json
qfs run "CREATE VIEW /chatwork/rooms OF chatwork/room AS /http/chatwork/rooms |> DECODE json" --json
qfs run "CREATE VIEW /chatwork/rooms/{room}/messages OF chatwork/message AS /http/chatwork/rooms/{room}/messages |> DECODE json" --json
qfs run "CREATE MAP INSERT /chatwork/rooms/{room}/messages AS INSERT INTO /http/chatwork/rooms/{room}/messages VALUES ({body: row.body}) IRREVERSIBLE" --json
```

After the previews match the expected `/sys/drivers` inserts, re-run them with `--commit` to install
the declarations.

Bind the declared driver at `/chatwork` with a secret reference:

```sh
qfs run "CONNECT /chatwork TO chatwork SECRET 'vault:chatwork/work'" --json
```

Preview first; add `--commit` only when the row should be persisted. The secret reference names the
vault selector and stores no token value in qfs config rows.

## Benchmark flow

After Chatwork declarations and the `/chatwork` binding are installed, the benchmark is:

```qfs
/chatwork/rooms
|> where name LIKE '%<room-name-fragment>%'
|> select room_id, name
```

Use the chosen `room_id` to read the latest message:

```qfs
/chatwork/rooms/<room_id>/messages
|> select message_id, body, send_time
|> order by send_time DESC
|> limit 1
```

Then preview a Slack investigation request. Replace the Slack path with the target workspace and
channel:

```qfs
insert into /slack/<workspace>/<channel>/messages
  values ('Please investigate Chatwork room <room-name> (<room_id>), latest message <message_id> at <send_time>: <excerpt>')
```

Do not commit the Slack post until a human has checked the rendered message. If the Slack message is
approved, apply it with `--commit`.

## Current boundary

The first live Chatwork latest-message read is still the acceptance point. Treat a successful room
lookup and latest message read as the proof that the declared driver, vault credential, and Chatwork
API shape all line up. Until that proof is captured, keep this page as the benchmark guide rather
than promoting it to the cookbook.
