---
type: Feedback
title: [FB] Slack worked example in `qfs skill` posts an empty message, and the correct spelling is ungrammatical
kind: instruction
source: discussion
subject: person:tamurayoshiya
created_at: 2026-09-16T06:56:38+09:00
author: a@qmu.jp
supersedes: 
---

# [FB] Slack worked example in `qfs skill` posts an empty message, and the correct spelling is ungrammatical

`qfs 0.0.130` (commit `c9a956d`, aarch64-unknown-linux-gnu), Slack driver.

## 1. The Slack worked example in `qfs skill` sends a message with no text

`qfs skill` says DESCRIBE is the only thing an agent reads, and rule 5 says a gap is a driver
contract bug rather than prose to bolt on. An agent therefore writes the worked example verbatim.
The Slack example is:

```text
insert into /slack/acme/general/messages values ('Deploy finished')
```

`/type/slack/message` declares `columns: ts, user, text, thread_ts, subtype`. A positional
`values (<one value>)` binds to the **first** column, so the body lands in `ts` and `text` stays
null. The registered map is

```text
name: /slack/{ws}/{channel}/messages   verb: INSERT   -> http/slack/chat.postMessage
body: {channel: path.channel, text: row.text}
```

so `chat.postMessage` is called with a null `text` and Slack refuses it. `values (null, null,
'Deploy finished')` delivers on the same mount, same credential, same channel.

**PREVIEW cannot catch this, and neither can the golden corpus.** Both spellings produce
`{"preview":{"rows":[{"verb":"INSERT",...}],"total_affected":{"exact":1}},"committed":false}`.
The example is labelled "Statement + PREVIEW (runs now)" and the section header says PREVIEW, so
it is verified at exactly the stage that is blind to the defect: the plan is well formed either
way, and only COMMIT reaches the service that rejects it.

## 2. The correct spelling is ungrammatical, so the example cannot simply be rewritten to name the column

Naming the column is the fix a reader would reach for, and the grammar refuses every form:

```text
insert into <path> (text)    values ('x')  -> parse_error RESERVED_AS_IDENTIFIER
insert into <path> ("text")  values ('x')  -> parse_error UNEXPECTED_CHAR / UNEXPECTED_TOKEN
insert into <path> (`text`)  values ('x')  -> parse_error UNEXPECTED_CHAR / UNEXPECTED_TOKEN
insert into <path> ([text])  values ('x')  -> parse_error UNKNOWN_KEYWORD
```

`text` is a reserved keyword with no quoting escape. So every caller writing to a Slack message
node must count columns positionally and pad with nulls, against a schema that can gain a column
later — a silent breakage waiting on a schema change. This is why item 1 is not just a typo in a
document: the safe spelling is unavailable, and the unsafe one is what ships as the example.

Either would settle it: allow a quoted/escaped identifier for a reserved column name, or change
the worked example to `values (null, null, 'Deploy finished')` with the column order written out
beside it. The first is better — the second leaves every caller depending on positional order.

## 3. A rejected write discards the service's own error

Slack answers an application-level refusal with **HTTP 200** and `{"ok": false, "error": "..."}`.
The driver collapses that to

```text
{"error":{"code":"commit_failed","kind":"commit_failed",
          "message":"Terminal { reason: \"service response failed: service_rejected\" }"}}
```

and the response body is not surfaced anywhere. At `RUST_LOG=debug` the applier logs only

```text
qfs_driver_http::applier: rest request method=POST url=https://slack.com/api/chat.postMessage status=200
```

and `RUST_LOG=trace` adds nothing carrying the body. The caller cannot distinguish a null field
from `missing_scope`, `not_in_channel`, `restricted_action` or `is_archived` — different causes,
different fixes, different people.

Measured cost: with items 1 and 2 in play, this turned a one-line documentation defect into a long
elimination across accounts, mounts, OAuth scopes and channel membership, because the one sentence
that would have ended it immediately was discarded. Carrying the service's `error` field into the
terminal reason, or logging the body at debug when a 2xx is classified `service_rejected`, would
have made items 1 and 2 self-diagnosing.

## 4. A mount not named `/slack` cannot call the procedures DESCRIBE advertises for it

Two accounts of one driver are two mounts, as the skill states. Mounted as `/slack-a` and
`/slack-b`, `qfs describe /slack-a/<ws> --json` advertises `react(channel, ts, emoji)`, `pin`,
`unpin`, `update`, `delete`. Neither documented call form reaches them:

```text
/slack-a/<ws>/<cid>/messages |> call slack.react(channel => '<cid>', ts => '<ts>', emoji => 'eyes')
  -> {"error":{"code":"unknown_driver","kind":"capability",
               "message":"no driver is mounted for namespace `slack`"}}

CALL /slack-a/<ws>.react('<cid>', '<ts>', 'eyes')
  -> {"error":{"code":"parse_error","kind":"parse",
               "message":"a reserved keyword cannot be used here","detail":"RESERVED_AS_IDENTIFIER"}}
```

The procedure namespace appears to follow the mount name — the PREVIEW plan for an INSERT on the
same path reports `"target":{"driver":"slack-a", ...}` — and `slack-a` is a hyphenated identifier
the parser will not accept in a `CALL`. INSERT works on these mounts while every advertised
procedure does not, which makes DESCRIBE's `procedures` list unreliable precisely on the
multi-account setup the skill recommends.

Either would settle it: let `CALL` name the driver namespace of the mount it is piped from (or
accept a quoted namespace), or have DESCRIBE omit procedures that cannot be called on that mount.
The two disagreeing is the problem.

## What is not claimed

Item 3 is about error reporting, not about any particular Slack error being the cause of anything.
Item 4 was measured against the `CALL` forms in `qfs skill`; if another form reaches those
procedures, it is not discoverable from DESCRIBE's output.

Source: https://github.com/qmu/qfs/issues/109
