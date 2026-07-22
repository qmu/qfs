# Coverage inventory — the compiled slack / github / gdrive / gmail surfaces

*(Mission acceptance item 1, ticket 20260722091100. This document is the machine-checkable
gap ledger the rulings ticket (091200), the conciseness ticket (091400), and the playbook
ticket (091500) consume without re-enumerating. One row per surface; every row carries a
disposition and its evidence.)*

## Method (how each row was verified)

- **Enumeration source — the compiled describe registry, not prose.** The node/verb/`CALL`
  surface of each driver was read from the driver source that *builds* the connection-free
  describe registry (`qfs::describe::compiled_describe_registry` → `qfs::catalog::driver_catalog`,
  the `gen-docs` source that renders `docs/drivers.md`): each driver's `path.rs` `NodeKind` /
  `Namespace` enumerations, `schema.rs`, `procs.rs`, and `pushdown.rs` / `query.rs`. Where prose
  and the registry could disagree, the registry code wins (mission policy). Spot-checked against
  live `qfs describe <path> --json` output for a representative node of each driver.
- **Parse-check — the current binary, hermetically isolated.** Every *expressible today* snippet
  was placed in a scratch `.qfs` and run through
  `XDG_CONFIG_HOME=<tmp> XDG_DATA_HOME=<tmp> qfs plan <file>` against an **empty** isolated state
  dir (so the diff is `N to add, 0 to change, 0 to destroy` — no dependence on this machine's
  connected drivers, no network, no credentials). Acceptance signal: `qfs plan` **aborts with a
  `parse error [...]` and a non-zero column report on any statement the grammar rejects** (proven
  against a deliberately malformed `CREATE DRIVER foo AT 42 NONSENSE`), so a plan that reaches
  `Plan: N to add …` with no parse error means **every statement in the file was accepted by the
  binary**. The snippets below were checked this way against the worktree binary (built from this
  branch). The reconcile effect-count can be smaller than the statement count (distinct
  `/sys/drivers` rows collapse in the diff); that is a reconcile artifact, not a parse failure —
  the acceptance signal is the *absence* of a parse error.
- **Disposition vocabulary** (exactly one per surface):
  - **expressible today** — a declaration the current binary accepts, cited as evidence.
  - **needs a ruled semantic** — the concrete missing primitive is named; the surface's read/write
    cannot be declared without it. Fed to 091200.
  - **named park** — waiting is honest (push/watch, GraphQL, websockets, resumable/batch shapes
    with no coverage-bar surface forcing them).

Snippets are illustrative of the *shape* that parses (host/param names abbreviated); they are not
the final twin declarations (those are the downstream conversion missions).

## Cross-cutting gaps (the "needs a ruled semantic" set, deduplicated)

Every driver-specific *needs a ruled semantic* row below maps to one of these. 091200 rules each.

| # | Gap | Where it bites (coverage-bar surfaces) | The missing primitive |
|---|-----|----------------------------------------|------------------------|
| G1 | **read-over-POST** | Slack `conversations.open` (DM addressing), Slack `search.messages`; (also `/cf` queue pull, GraphQL) | a declared VIEW whose wire source is a **POST carrying a body**. Today a VIEW body's `/http/<drv>/…` source is always a GET; the only POST-to-read that ships is the `CREATE SQL … OVER …/query` arm. |
| G2 | **declared pushdown** | Slack `oldest`/`latest`/`limit`; GitHub `state`/`labels`/`assignee`/`per_page`; Gmail `q=`; Drive `q` | a clause declaring **which columns/predicates push to which wire query/body params**, each marked **exact** (residual dropped) or **pre-filter** (pushed *and* kept as residual) so pushdown honesty survives the declaration. No `PUSHDOWN` grammar exists. |
| G3 | **MIME / body assembly** | Gmail `drafts` INSERT, `mail.send` / `mail.reply` (RFC 5322 + base64url; `multipart/mixed` with attachments) | a codec or prelude function that assembles an RFC 5322 message and base64url-encodes it for the `raw` field. Only `|> ENCODE multipart` (form-data) ships; no message/MIME encoder. |
| G4 | **batch & subrequest fan-out** | Gmail list→detail hydration (`messages.list` returns id stubs; each needs a detail GET), Gmail `batch`; Slack DM `conversations.open`→`history`; Drive path→id parent-pointer walk | a **per-row fan-out** primitive (follow a delivered id/handle into a detail request and splice the result). Today `|> FOLLOW <field>` is **bytes-oriented and single-purpose** (one second GET → raw bytes), not a general per-row relational join-to-wire. |
| G5 | **declared `CALL` signature/typing** | every driver's procedures (Slack react/pin/unpin/update/delete; GitHub merge/dispatch/review; Drive copy; Gmail send/reply) | `CREATE MAP CALL <drv>.<action>` names only the action — it declares **no typed parameter list**, so a declared twin cannot reproduce the typed `CALL` signature the compiled describe registry reports (`react(channel: Text, ts: Text, emoji: Text)`). The effect maps fine; the *contract* does not. |
| G6 | **declared prelude aliases** | Slack `POST → slack.post`; Gmail `SEND → mail.send` | a clause to declare a prelude alias (a bare-verb shorthand for a `CALL`). Terseness device; no grammar exists. |
| G7 | **declared blob-namespace archetype** | Drive `cp`/`ls`/`mv`/`rm` filesystem builtins; (also Slack `files` `cp`/`rm`) | a declared driver exposes VIEWs + MAPs (relational/effect), not the **BlobNamespace archetype** whose shell builtins (`cp`/`ls`/`mv`/`rm`) give the filesystem ergonomics. The underlying ops are each expressible; the *archetype ergonomics* are not declarable. |
| G8 | **the non-REST arm** | (none in the four REST drivers; the question the mission must close) | whether the declared shape grows an arm for non-wire sources (`/git`, `/claude`, blob primitives). Ruled/parked in 091200. |

**Named parks (honest waits, no coverage-bar surface forces them):** Gmail push/`watch` channels;
Slack Events API / RTM / Socket Mode; GraphQL; websockets; Drive **resumable** upload sessions
(simple + multipart upload are expressible); Gmail `batch` transport *as an optimization* (the
functional need is G4 fan-out, which rules the honest-but-chatty baseline; batch is the fast path).

---

## Slack — `/slack/<ws>/…` (AppendLog + Blob + Relational, multi-archetype)

Registry: `NodeKind = {Messages, Replies, Reactions, Dms, Files, Users}`; verbs
`SELECT(tail) INSERT(append) REMOVE`; procedures `react, pin, unpin, update, delete`; prelude
`POST → slack.post`; pushdown `where(oldest/latest on ts) limit`.

| Surface | Kind | Disposition | Evidence / missing primitive |
|---------|------|-------------|------------------------------|
| `<#channel>/messages` read | node/SELECT | expressible today | `CREATE VIEW /slackd/{channel}/messages OF slackd/message AS /http/slackd/conversations.history?channel={channel} \|> DECODE json \|> EXPAND messages;` (parses) |
| `<#channel>/messages` post | node/INSERT | expressible today | `CREATE MAP INSERT /slackd/{channel}/messages AS INSERT INTO /http/slackd/chat.postMessage VALUES (row);` (parses) |
| `<#channel>/messages` delete | node/REMOVE | expressible today | verb→POST decoupling: `CREATE MAP CALL slackd.delete /slackd/{channel}/messages AS INSERT INTO /http/slackd/chat.delete VALUES (row) IRREVERSIBLE;` (parses). Slack `chat.delete` is a POST, mapped through the INSERT-effect body. |
| `messages/{ts}/replies` | node/SELECT | expressible today | `CREATE VIEW /slackd/{channel}/messages/{ts}/replies … /http/slackd/conversations.replies?channel={channel}&ts={ts} …` (parses) |
| `messages/{ts}/reactions` | node/INSERT+REMOVE | expressible today | `CREATE MAP CALL slackd.react … AS INSERT INTO /http/slackd/reactions.add VALUES (row);` (parses) |
| `dms/{user}/messages` | node/SELECT | **needs a ruled semantic** | **G1 + G4**: addressing a DM by *user* requires `conversations.open` (a **POST that returns** a channel id) then `conversations.history`. No POST-read; no per-row resolution. |
| `files` list | node/LS | expressible today | `CREATE VIEW /slackd/{ws}/files AS /http/slackd/files.list \|> DECODE json \|> EXPAND files;` (view shape parses) |
| `files` upload | node/INSERT+UPSERT | expressible today | `|> ENCODE multipart` (chatwork precedent): `CREATE MAP INSERT /slackd/{ws}/files AS INSERT INTO /http/slackd/files.upload \|> ENCODE multipart VALUES (row);` |
| `files/{id}` download | node/SELECT(cp) | expressible today | `|> FOLLOW url_private` (bytes-oriented FOLLOW is exactly this case) |
| `files/{id}` delete | node/REMOVE(rm) | expressible today | `CREATE MAP CALL slackd.filesdelete … AS INSERT INTO /http/slackd/files.delete VALUES (row) IRREVERSIBLE;` |
| `users` | node/SELECT | expressible today | `CREATE VIEW /slackd/{ws}/users AS /http/slackd/users.list \|> DECODE json \|> EXPAND members;` (parses) |
| `react` `pin` `unpin` `update` `delete` | CALL effect | expressible today | each is a POST method endpoint → `CREATE MAP CALL slackd.<action> … AS INSERT INTO /http/slackd/<method> VALUES (row) [IRREVERSIBLE];` (parses) |
| `react`/`pin`/… typed signature | CALL contract | **needs a ruled semantic** | **G5**: declared `MAP CALL` carries no typed param list; `react(channel,ts,emoji)` contract not declarable. |
| `oldest`/`latest`/`limit` pushdown | pushdown | **needs a ruled semantic** | **G2**: without it a declared twin fetches whole history and filters locally (pathologically chatty). Registry semantics: inclusive bounds → `>=`/`<=` exact, `>`/`<` pre-filter+residual. |
| `POST → slack.post` prelude | alias | **needs a ruled semantic** | **G6**: prelude-alias declaration (terseness). |
| Events API / RTM / Socket Mode | (not a describe node) | named park | push/websocket; not a coverage-bar node. |

## GitHub — `/github/{owner}/{repo}/…` (ObjectGraphWorkflow)

Registry: `Namespace = {issues, pulls, comments, reviews, runs, releases, files, branches}`; object
by id; sub-collections (`issues/{n}/comments`, `pulls/{n}/reviews`); verbs `SELECT INSERT UPDATE`;
procedures `merge, dispatch, review`; pushdown `where(state/labels/assignee) limit(per_page)`.

| Surface | Kind | Disposition | Evidence / missing primitive |
|---------|------|-------------|------------------------------|
| namespace list (each of 8) | node/SELECT | expressible today | `CREATE VIEW /ghd/{owner}/{repo}/pulls OF ghd/pull AS /http/ghd/repos/{owner}/{repo}/pulls \|> DECODE json;` (parses; `AUTH ACCOUNT 'github'`, `PAGINATE LINK MAX 50`) — one view per namespace. |
| object by id | node/SELECT | expressible today | `CREATE VIEW /ghd/{owner}/{repo}/pulls/{n} AS /http/ghd/repos/{owner}/{repo}/pulls/{n} \|> DECODE json;` |
| sub-collection (`issues/{n}/comments`) | node/SELECT | expressible today | `CREATE VIEW /ghd/{owner}/{repo}/issues/{n}/comments AS /http/ghd/repos/{owner}/{repo}/issues/{n}/comments \|> DECODE json;` (parses) |
| create issue / comment | node/INSERT | expressible today | `CREATE MAP INSERT /ghd/{owner}/{repo}/issues AS INSERT INTO /http/ghd/repos/{owner}/{repo}/issues VALUES (row);` (parses) |
| edit issue / PR | node/UPDATE | expressible today (verbose) | `CREATE MAP UPDATE /ghd/{owner}/{repo}/issues/{n} AS UPDATE /http/ghd/repos/{owner}/{repo}/issues/{n} SET state = row.state;` (parses). Whole-row PATCH needs per-column `SET`; a whole-row PATCH body is a minor terseness gap (relates G5-class typing, not blocking). |
| `files` (contents) read | node/SELECT | expressible today (+ per-row decode) | list/metadata is a plain view; file **content** is base64 in the JSON — the decode rides the sibling mission's per-row codec ruling, not a new primitive here. |
| `runs` (workflow runs) | node/SELECT | expressible today | `CREATE VIEW /ghd/{owner}/{repo}/runs AS /http/ghd/repos/{owner}/{repo}/actions/runs \|> DECODE json \|> EXPAND workflow_runs;` |
| `merge` `dispatch` `review` | CALL effect | expressible today | `CREATE MAP CALL ghd.merge /ghd/{owner}/{repo}/pulls AS INSERT INTO /http/ghd/repos/{owner}/{repo}/pulls/merge VALUES (row) IRREVERSIBLE;` (parses). `merge` is a PUT (irreversible), `dispatch`/`review` POST. |
| `merge`/`dispatch`/`review` signature | CALL contract | **needs a ruled semantic** | **G5**: typed `merge(method: Text, sha: Text)` contract not declarable. |
| `state`/`labels`/`assignee`/`per_page` pushdown | pushdown | **needs a ruled semantic** | **G2**: registry semantics `state=` exact, `labels=` pre-filter+residual (set-membership), `per_page`=limit. |
| GraphQL (v4) | (not a describe node) | named park | body-carried GraphQL is G1-shaped; no coverage-bar node forces it. |

## Google Drive — `/drive/…` (BlobNamespace)

Registry: virtual root → `my`/`shared`; folder/file tree over parent pointers; `id:<fileId>`
addressing; `@<rev>` revision pin; verbs `SELECT INSERT UPSERT UPDATE REMOVE LS CP MV`; procedure
`copy`; pushdown `where(Drive q=) limit`.

| Surface | Kind | Disposition | Evidence / missing primitive |
|---------|------|-------------|------------------------------|
| listing by **id** | node/SELECT,LS | expressible today | `CREATE VIEW /drived/files AS /http/drived/files \|> DECODE json \|> EXPAND files;` (parses); filter to a parent with the Drive `q` param. |
| listing by **path** (`/drive/my/A/B`) | node/LS | **needs a ruled semantic** | **G4**: resolving a human path to a file id walks parent pointers (one `files.list` per segment) — per-row/name resolution, not expressible with single-purpose FOLLOW. `id:` addressing sidesteps it. |
| file download | node/SELECT(cp) | expressible today | `GET files/{id}?alt=media` returns bytes: `CREATE VIEW /drived/files/{id}/blob AS /http/drived/files/{id}?alt=media;` (bytes view) |
| Google-native export (Docs/Sheets) | node/SELECT | expressible today | `GET files/{id}/export?mimeType=…`: `CREATE VIEW /drived/files/{id}/export AS /http/drived/files/{id}/export?mimeType=text%2Fplain;` |
| simple / multipart upload | node/UPSERT,INSERT | expressible today | `|> ENCODE multipart` for a metadata+media upload; `CREATE MAP UPSERT /drived/files/{id} AS UPSERT INTO /http/drived/files/{id} VALUES (row);` (parses) |
| **resumable** upload | (transport variant) | named park | session-protocol upload; simple/multipart cover the functional need. |
| metadata update | node/UPDATE | expressible today | `CREATE MAP UPDATE /drived/files/{id} AS UPDATE /http/drived/files/{id} SET name = row.name;` |
| trash / delete | node/REMOVE,RM | expressible today | PATCH `trashed=true` (MAP UPDATE) or DELETE (MAP REMOVE). |
| move (`mv`, add/remove parents) | node/MV | expressible today (as UPDATE) | PATCH `addParents`/`removeParents` via a MAP UPDATE. |
| `copy` | CALL effect | expressible today | `POST files/{id}/copy`: `CREATE MAP CALL drived.copy /drived/files AS INSERT INTO /http/drived/files/{id}/copy VALUES (row);` |
| `copy` typed signature | CALL contract | **needs a ruled semantic** | **G5**: `copy(file_id, parent_id, parent_path, name)` typed contract not declarable. |
| `cp`/`ls`/`mv`/`rm` shell ergonomics | archetype | **needs a ruled semantic** (or named park) | **G7**: the BlobNamespace archetype's filesystem builtins are not what a view/map declaration produces; the raw ops (rows above) are expressible, the archetype ergonomics are not. |
| Drive `q=` pushdown | pushdown | **needs a ruled semantic** | **G2**: rich query translation (`name`/`mimeType`/`in parents`/`fullText`/`modifiedTime`/`trashed`), exact vs pre-filter per registry. |

## Gmail — `/mail/…` (AppendLog)

Registry: virtual root → labels; `/mail/<label>` → messages; `/mail/drafts`; `id:<msg>`,
`id:thread:<id>`; attachments nested; verbs `SELECT(tail) INSERT(append) UPSERT` (+ `UPDATE`/`REMOVE`
on label nodes); procedures `send, reply`; prelude `SEND → mail.send`; pushdown `where(q=) limit`.

| Surface | Kind | Disposition | Evidence / missing primitive |
|---------|------|-------------|------------------------------|
| label listing (`/mail` root) | node/LS | expressible today | `CREATE VIEW /maild/labels AS /http/maild/users/me/labels \|> DECODE json \|> EXPAND labels;` |
| messages in a label (`/mail/<label>`) | node/SELECT | **needs a ruled semantic** | **G4**: `messages.list?labelIds=` returns `{id, threadId}` **stubs**; each message must be hydrated by a detail GET (`messages/{id}`). Per-row fan-out, not single-purpose FOLLOW. |
| single message (`id:<msg>`) | node/SELECT | expressible today | `CREATE VIEW /maild/messages/{id} OF maild/message AS /http/maild/users/me/messages/{id} \|> DECODE json;` |
| thread (`id:thread:<id>`) | node/SELECT | expressible today | `CREATE VIEW /maild/threads/{id} AS /http/maild/users/me/threads/{id} \|> DECODE json \|> EXPAND messages;` |
| attachment fetch | node/SELECT | expressible today (+ per-row decode) | `GET messages/{id}/attachments/{att}` → base64url `data`; the decode rides the sibling mission's per-row codec ruling. |
| `q=` search over messages | node/SELECT (pushdown) | **needs a ruled semantic** | **G2**: Gmail `q=` (`from:`/`subject:`/`after:`/`is:unread`/`label:`), exact vs pre-filter per registry. |
| draft create (`/mail/drafts` INSERT) | node/INSERT | **needs a ruled semantic** | **G3**: draft body is `{message:{raw:<base64url RFC 5322>}}`; MIME assembly + base64url not expressible (only form `ENCODE multipart` ships). |
| `send` / `reply` | CALL effect | **needs a ruled semantic** | **G3**: same MIME/base64url assembly; attachments → `multipart/mixed`. The POST effect is trivial; the *body* is the wall. |
| `send`/`reply` typed signature | CALL contract | **needs a ruled semantic** | **G5**: `send(to, subject, body, attachments: Array(Struct{...}))` typed contract not declarable. |
| `SEND → mail.send` prelude | alias | **needs a ruled semantic** | **G6**: prelude-alias declaration. |
| `batch` (batchModify / batch GET) | (transport) | named park | fast path for G4; functional need is the fan-out ruling, not batch itself. |
| push / `watch` channels | (not a describe node) | named park | Pub/Sub push; no coverage-bar node forces it. |

---

## Roll-up (spot-check counts)

- **Slack** — 6 node kinds, 3 verbs, 5 procedures, 1 prelude, 2 pushdown params. Expressible: message read/post/delete, replies, reactions, file list/upload/download/delete, users, all 5 CALL effects. Ruled-semantic: DM (G1+G4), CALL typing (G5), pushdown (G2), prelude (G6). Parks: Events/RTM.
- **GitHub** — 8 namespaces + objects + sub-collections, 3 verbs, 3 procedures, pushdown. Expressible: every list/object/sub-collection read, create, edit (verbose), runs, all 3 CALL effects. Ruled-semantic: CALL typing (G5), pushdown (G2). Parks: GraphQL.
- **Drive** — blob tree + id addressing, 8 verbs, 1 procedure, rich pushdown. Expressible: id-listing, download, export, upload (simple/multipart), update, trash, move, copy. Ruled-semantic: path→id (G4), CALL typing (G5), blob archetype ergonomics (G7), pushdown (G2). Parks: resumable upload.
- **Gmail** — labels/messages/threads/attachments/drafts, verbs, 2 procedures, 1 prelude, pushdown. Expressible: label list, single message, thread, attachment fetch. Ruled-semantic: list hydration (G4), q= pushdown (G2), draft/send/reply MIME (G3), CALL typing (G5), prelude (G6). Parks: batch, push/watch.

**Eight cross-cutting gaps** (G1–G8) carry every "needs a ruled semantic" row. G1 (read-over-POST)
and G2 (declared pushdown) gate the slack/github conversions and full `/cf` retirement, per the
mission; G3–G5 gate Gmail; G7 is the Drive blob-ergonomics question; G8 is the non-REST-arm
decision. 091200 rules each; 091300 ships G1 (read-over-POST) with a hermetic end-to-end proof.

## Conciseness measurements (filled by ticket 091400)

*(Per 091400's policy: statement-line counts per "expressible today" family land here so a later
conversion mission reads bar and evidence in one place. Placeholder until 091400 runs.)*
