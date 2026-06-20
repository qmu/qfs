# Model v1

Author: Architect
Status: draft
Reviewed-by: none yet

## Content

This model bridges the business vision ("a Gmail version of gdrive-ftp — same
concept, same directory structure, same experience, but email instead of
files") to a technical build. It is grounded in a close read of the reference
codebase `../gdrive-ftp` (main.go, internal/auth, internal/gdrive,
internal/shell, internal/audit, README.md, the SKILL.md agent skill, and the
plugin manifests). The governing constraint is **fidelity to the gdrive-ftp
structure and FTP experience**: an interactive `ls/cd/pwd/get/put/mkdir/rm`
shell plus one-shot mode, `-json` output, an audit log of mutations, terminal
OAuth, Tab/zsh completion, and an agent skill. We preserve every one of those
seams and re-point only the backend domain.

### 1. System coherence mapping

How gdrive-ftp is structured today, and the Gmail equivalent for each part:

| gdrive-ftp structure (today) | Responsibility | gmail-ftp equivalent |
| --- | --- | --- |
| `main.go` | CLI wiring: flags (`-creds`, `-token`, `-json`, `-no-log`), config-dir paths, subcommand routing (`auth`, `log`, `completion zsh`, hidden `__complete`), interactive-vs-one-shot dispatch, `signal.NotifyContext` for Ctrl-C, `fatal`. | Same file, near-identical. Only the imported backend package, the connect banner, scope, and config dir name (`gmail-ftp`) change. |
| `internal/auth/auth.go` | Google OAuth2 desktop-app consent over the terminal (OSC 52 clipboard, paste-back redirect, CSRF `state`), token caching + self-refreshing `savingSource`. | Reused almost verbatim. The **only** substantive change is the requested scope: swap `drive.DriveScope` for a Gmail scope (see §4). The consent UX, token cache, and refresh logic are domain-agnostic and must not be reinvented. |
| `internal/gdrive/client.go` | The backend abstraction: a thin, FTP-flavored wrapper over Drive v3 exposing exactly what the shell needs (`List`, `ListDrives`, `GetByID`, `Search`, `FindChildren`, `FindDir`, `FindOne`, `Mkdir`, `Upload`, `Download`, `Export`, `Trash`) plus `Ref`, `IsFolder`, `IsGoogleDoc`, MIME/export tables. | Replaced by `internal/gmail/client.go`: a thin wrapper over **Gmail API v1** exposing the same *shape* of operations re-expressed in email terms (list labels, list/search messages, get message/thread, fetch attachment, create draft, send, trash). This is the largest single piece of new work; its method surface is what makes the shell reusable. |
| `internal/shell/shell.go` | The REPL: raw-mode line editor + Tab completion, plain scanner fallback, the `Ref`-stack working directory, path resolution (`resolveDir`/`resolveFile`/`startStack`), tokenizer, `id:` addressing, `friendlyErr` API-disabled hint. | Reused with the same architecture. The cwd `Ref` stack, tokenizer, `id:` mechanism, completion plumbing, and `friendlyErr` carry over directly. Path *resolution* changes because the hierarchy is label-based, not folder-tree-based (see §2/§3). |
| `internal/shell/commands.go` | The verb implementations (`cmdLs`, `cmdCd`, …) and formatting helpers. | Same dispatch table and verb names; bodies re-point to the gmail client and the new domain model. `get`/`put`/`rm`/`find` take on email semantics (see §2). |
| `internal/shell/output.go` | The owned JSON DTO layer (`fileEntry`, `actionResult`, `pwdResult`, `errorResult`, `emit`) that deliberately never marshals the vendor `*drive.File` directly. | Same pattern, same discipline: owned DTOs translate `*gmail.Message`/label structs so the JSON contract stays decoupled from the Gmail SDK. DTO field set is extended for email (see §2). |
| `internal/audit/*` | Append-only JSONL log of *mutations* (upload/trash/mkdir), size-rotated ring, read/JSON/text/TUI-browser, `gdrive-ftp log`. | Reused with the mutation vocabulary re-mapped to email mutations (send, draft-create, trash, label-change). The logger, rotation ring, reader, JSON/text emit, and `tig`-like browser are domain-neutral and carry over; only the `Operation` constants and `Entry` fields change. |
| `plugins/.../SKILL.md` + manifests | The agent skill teaching a coding agent to drive the one-shot CLI, the path model, auth prerequisite, JSON contract, gotchas; Claude/Codex/skills marketplaces. | A parallel `gmail-ftp` skill and manifests, same shape, re-authored for the email path model and gotchas. |

The coherence principle: **auth, shell, audit, output, and the agent skill are
backend-agnostic plumbing; only the backend-client package and the domain model
inside the command bodies are genuinely email-specific.** That keeps the
translation traceable and bounds the new work.

### 2. Domain model — email onto the filesystem metaphor

gdrive-ftp's hierarchy is: *virtual root → drive → folder tree → file*. The core
translation decision is how Gmail's flat, label-tagged, thread-grouped message
store is presented as that same hierarchy. Proposed mapping:

| Filesystem concept (gdrive-ftp) | Gmail domain entity | Mapping definition |
| --- | --- | --- |
| Virtual root `/` (lists drives) | The mailbox | `ls /` lists the **labels** (system labels INBOX, SENT, DRAFT, STARRED, IMPORTANT, TRASH, SPAM, plus the user's own labels). A label is the first path component, exactly as a drive name is in gdrive-ftp. |
| Drive (first path component) | Label | `cd INBOX`, `cd "Work/Receipts"`. A label is the top-level navigable container. Gmail nested labels (`Work/Receipts`) map naturally to the `/`-separated path, though they are flat tags, not a tree (see §3). |
| Folder | Label, and *only* a label | There is **no second tier of folders** inside a label. The hierarchy is exactly two conceptual levels: label, then messages. (Nested labels give apparent depth, but every level is itself a label, addressable from the root.) |
| File | **Message** (default) or **thread** | A leaf entry under a label is a message, rendered with a stable, sortable name (see naming below). `get`-ing it downloads the message. Threads are surfaced via a per-label grouping option / `find -json` and addressed by `id:` (see §3 thread decision). |
| File content (bytes via `get`) | The message itself | `get <message>` writes the message to a local file as **RFC 822 / `.eml`** (the raw MIME, the email analog of "byte-for-byte download"). A `-json`/text export to a readable `.txt` (headers + body) is the email analog of gdrive-ftp's *export* of native Google docs — the message has no single "raw byte" form a user wants by default, so `.eml` is the faithful raw form and `.txt` the "exported" convenience form. |
| Nested file (attachment) | Attachment | An attachment is a leaf *inside* a message. `ls <message>/` lists its attachments; `get <message>/<attachment>` (or `get id:<attachmentRef>`) downloads one attachment byte-for-byte. This reuses gdrive-ftp's "a thing can contain things" listing seam without inventing real subfolders. |
| Search (`find`) | Gmail search query | `find <query> [label]` runs Gmail's native search (`users.messages.list` with `q=`), scoped to a label when anchored — directly analogous to Drive's `name contains`, but far richer (`from:`, `subject:`, `is:unread`, date ranges). Matches print as paths/ids; `find` never mutates. |
| `put <local>` (upload) | Create draft / send | `put <local.eml> <label>` imports/creates a **draft** from a local RFC 822 file. Sending is an explicit, separate verb (`send`) rather than implicit, because sending is irreversible (see §3 send decision). A draft is the writable leaf — the email analog of an uploadable file. |
| `mkdir <name>` | Create a label | `mkdir Work/Receipts` creates a user label. Direct, faithful analog. |
| `rm <name>` | Trash a message (reversible) | `rm <message>` moves the message to TRASH — exactly gdrive-ftp's "trash, not hard-delete, reversible" promise. `rm <label>` (a top-level container) deletes the *label* (untags; messages survive), which must be called out as semantically different from trashing a message (see §3). |
| `modifiedTime`, `size` (DTO) | `internalDate`, `sizeEstimate`, plus `from`/`subject`/`unread` | The owned `fileEntry` DTO gains email fields: `from`, `subject`, `date`, `unread`, `labels[]`, `threadId`. Listing rows render `subject` (and sender/date) as the "name", because a message has no filesystem name. |

**Message naming.** A message has no native filename, so the shell synthesizes a
stable, human-and-Tab-friendly display name and resolves it back to a message ID.
Recommended form: a short prefix derived from `internalDate` + a slug of the
subject (e.g. `2026-06-18 Quarterly report`), with the underlying Gmail message
ID always available via `id:` and via every `-json` row. Because synthesized
names can collide or be ambiguous, **`id:` addressing (already in gdrive-ftp) is
promoted from a convenience to the primary, canonical way to act on a specific
message** — the model leans on an existing seam rather than inventing one.

### 3. Translation fidelity analysis

Where the filesystem metaphor fits email cleanly, and where it strains, with a
recommended resolution for each strain:

**Fits cleanly:**
- *Label → directory* and *root lists labels → root lists drives*: structurally
  identical; the `Ref`-stack cwd model carries over unchanged.
- *Search → find*: Gmail search is a strictly richer version of Drive's
  `name contains`; the `find` verb, subtree/anchor narrowing, and "act on a
  result by `id:`" pattern all transfer.
- *Trash is reversible*: Gmail's TRASH label is an exact match for gdrive-ftp's
  "rm trashes, never hard-deletes" guarantee — a load-bearing safety promise we
  keep verbatim.
- *Attachment → nested file*: the `ls <leaf>/` → list-children seam already
  exists conceptually and maps onto attachments without new architecture.

**Strains, with resolutions:**

1. **Labels are mutable, many-to-many tags, not a tree.** A message can carry
   several labels, so it appears under several "directories" at once, and a
   label is not a parent that owns its children. *Resolution:* treat a label as a
   **view/filter**, not an owner — the same message legitimately appears under
   `/INBOX` and `/Work`. The cwd stack stays a label path; listing a label is a
   query (`label:X`), which is exactly how `List` already works (a query, not a
   directory read). No "move" semantics are implied; `cd` between labels is just
   re-querying. Document explicitly in README/SKILL that a message can appear in
   multiple labels — this is a feature of the metaphor, not a bug.

2. **Many-to-many membership breaks "rename/move".** Drive has parents; Gmail has
   tags. There is **no true rename of a message** and "move" is add-label /
   remove-label. *Resolution:* do **not** expose a `mv`/rename verb for messages
   (gdrive-ftp's `put`-rename of a remote target is dropped for messages). Offer
   label mutation as explicit, audited operations — recommend a small `label`
   verb (`label add/rm <message> <label>`) rather than overloading `put`/`mv`,
   keeping each mutation legible and matching gdrive-ftp's "never guess a
   destructive target" stance. (This is an *addition* beyond the gdrive-ftp verb
   set; the Constructor/Planner should weigh whether to ship it in v1 or treat
   label membership as read-only first.)

3. **Threads vs messages.** Gmail's natural unit is the *thread*; the FTP leaf is
   naturally a single *message*. Exposing both as "files" in the same listing is
   ambiguous. *Resolution:* make the **message the default leaf** (one row per
   message, the most file-like unit and the unit you `get` as one `.eml`), and
   expose the thread as **(a)** a `threadId` field on every row and **(b)** an
   addressable container via `id:thread:<id>` or `get`-ing a thread to an `.mbox`.
   Default listings are per-message to keep the metaphor crisp; thread grouping
   is opt-in. This avoids a two-kinds-of-file listing while preserving thread
   access for agents.

4. **No true rename / immutable content.** A sent/received message's bytes are
   immutable. *Resolution:* embrace it — messages are read-only leaves (`get`,
   `rm`-to-trash, label changes only). The single *writable* email object is the
   **draft**, which is where `put` (create draft from local `.eml`) lives.

5. **Send semantics are irreversible — unlike every gdrive-ftp mutation.**
   gdrive-ftp's most dangerous op (`rm`) is still reversible (trash). Sending an
   email cannot be undone. *Resolution:* **never make `put` send.** `put` only
   creates a draft. Introduce an explicit, separate `send` verb that operates on
   an existing draft, is always audited, and (recommended) prints a clear
   one-line confirmation of recipients. This honors gdrive-ftp's safety culture
   ("never act on a guessed target", "trash is reversible") by isolating the one
   genuinely irreversible action behind its own verb.

6. **`rm` overload (message vs label).** `rm <message>` = trash (reversible);
   `rm <label>` = delete a label (untag, reversible-ish but different). *Resolution:*
   keep `rm` for the leaf (trash a message), and route label deletion through the
   same explicit care as `mkdir`'s inverse — either an `rmdir`-style guard or a
   confirmation, so a user never trashes a whole label thinking they trashed one
   message. Audit both distinctly.

### 4. Boundary integrity assessment

**Package boundaries to preserve unchanged** (these are the source of the "same
experience" guarantee):
- **auth boundary** — `auth.Client(ctx, creds, token) (*http.Client, error)`.
  Keep the signature and the terminal-OAuth/token-cache behavior identical;
  change only the requested OAuth **scope**.
- **backend-client boundary** — the shell depends only on a narrow set of client
  methods. Preserve that narrowness: the new `gmail.Client` must expose a
  similarly small, FTP-flavored surface, so the shell stays thin and the SDK
  stays quarantined behind one package.
- **shell boundary** — `shell.New(ctx, client, out, jsonOut, log)` and the verb
  dispatch table. Keep verb names and the REPL/completion architecture.
- **audit boundary** — `audit.New(path)`, `Logger.Record(ctx, Entry)`, `Read`,
  `WriteJSON`/`WriteText`/`Browse`. Keep the JSONL ring + browser intact; change
  only the `Operation` vocabulary and `Entry` fields.
- **output/DTO boundary** — owned DTOs only; the vendor `*gmail.Message` is never
  marshaled directly, mirroring the existing `*drive.File` discipline.

**Gmail-specific API surface (the new boundary content):**
- **Scopes.** Choose the least-privilege scope set that still supports the verb
  set. `https://mail.google.com/` grants full access (read, modify, send, delete)
  — the closest analog to gdrive-ftp's full `drive` scope and the simplest, but
  broad. A tighter alternative is the union of
  `gmail.readonly` + `gmail.modify` + `gmail.compose`/`gmail.send`. Recommend the
  scoped union over full `mail.google.com` and document it, since the
  irreversibility of send makes least-privilege more valuable here than in
  gdrive-ftp. (Flag this as a Planner/Constructor decision.)
- **Resources the client wraps:** `users.labels` (list/create/delete),
  `users.messages` (list with `q=`, get, trash, modify-labels),
  `users.messages.attachments` (get), `users.threads` (list/get),
  `users.drafts` (create/list/get), `users.messages.send` /
  `users.drafts.send`. Each maps to one or a few client methods, mirroring how
  `gdrive.Client` wraps `Files`/`Drives`.
- **The Gmail-API-disabled `friendlyErr` hint** must be re-pointed at the Gmail
  API enablement URL/console, preserving the existing helpful-error behavior.

### 5. Component taxonomy

Proposed package layout for `gmail-ftp`, mirroring `gdrive-ftp` one-for-one:

```
main.go                                   CLI wiring, flags, subcommand routing,
                                          interactive-vs-one-shot dispatch
                                          (≈ unchanged; new backend import, scope,
                                          banner, config dir "gmail-ftp")
internal/auth/auth.go                     Google OAuth2 terminal consent + token
                                          cache/refresh (≈ verbatim; Gmail scope)
internal/gmail/client.go                  Gmail API v1 wrapper: ListLabels,
                                          ListMessages(query/label), GetMessage,
                                          GetThread, GetAttachment, CreateDraft,
                                          Send, Trash, ModifyLabels, CreateLabel,
                                          DeleteLabel (replaces internal/gdrive)
internal/gmail/model.go (optional)        Owned Ref/Message/Label structs +
                                          message-name synthesis + MIME helpers
internal/shell/shell.go                   REPL, label-path resolution, tokenizer,
                                          id: addressing, completion, friendlyErr
internal/shell/commands.go                Verb implementations re-pointed at the
                                          gmail client and email domain model
internal/shell/output.go                  Owned JSON DTOs (message/label entries)
                                          + emit seam (no vendor struct marshaled)
internal/audit/audit.go                   Append-only JSONL log of email mutations
internal/audit/reader.go                  Read/WriteJSON/WriteText of the log
internal/audit/browser.go                 tig-like read-only log browser
plugins/gmail-ftp/skills/gmail-ftp/       Agent skill (how to drive this CLI for
                                          Gmail: path model, auth, JSON, gotchas)
.claude-plugin/marketplace.json           Claude Code plugin marketplace
.agents/plugins/marketplace.json          Codex plugin marketplace
```

One-line responsibilities are inlined above. The taxonomy deliberately keeps the
package count, names, and dependency direction identical to gdrive-ftp; the only
renamed package is `internal/gdrive → internal/gmail`, and the only genuinely new
optional file is a `model.go` to house message-name synthesis (which has no
gdrive-ftp counterpart because Drive files already have names).

**Open questions for Direction/Design to resolve:**
- Scope choice (full `mail.google.com` vs scoped union) — §4.
- Whether label mutation (`label add/rm`) and an explicit `send` verb ship in v1
  or v1 is read-plus-trash-plus-draft only — §3 items 2 and 5.
- Default leaf granularity (message vs thread) and the thread-export format — §3
  item 3.
- Message display-name format and collision strategy (lean on `id:`) — §2.

## Review Notes

(placeholder — to be filled during the one-turn review round)
