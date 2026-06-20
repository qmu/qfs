# Design v1

Author: Constructor
Status: draft
Reviewed-by: none yet

## Content

This design ports `gdrive-ftp` (an FTP-style CLI over the Google Drive v3 API)
to `gmail-ftp` (an FTP-style CLI over the Gmail v1 API). The guiding constraint
is **structural parity**: same package layout, same CLI wiring, same auth/token
discipline, same JSON contract shape, same audit-log mechanism, same plugin/skill
packaging, and the same unit-test bar. Gmail's domain (labels, threads, messages,
attachments, drafts) is mapped onto the existing FTP verb vocabulary so the user
experience is recognizably "the same tool, for mail."

The central modeling decision — and the one most reviewed below — is the
**virtual filesystem mapping**: Gmail has no directory tree, so we synthesize one
from labels and threads. The mapping mirrors gdrive-ftp's two-level "virtual root
→ drive → folders/files" shape:

| gdrive-ftp level            | gmail-ftp level                                   |
| --------------------------- | ------------------------------------------------- |
| virtual root (lists drives) | virtual root (lists **labels**: INBOX, SENT, …)   |
| a drive (My Drive / Shared) | a **label** (e.g. `INBOX`, `[Gmail]/Sent Mail`)   |
| a folder inside a drive     | a **thread** inside a label (named by subject)    |
| a file inside a folder      | a **message** inside a thread; attachments leaf   |

`ls /` lists labels; `cd INBOX` enters a label and `ls` lists its threads;
`cd` into a thread and `ls` lists the messages (and their attachments). `get`
downloads a message (as `.eml`) or an attachment (raw bytes); `put` creates a
draft; `rm` trashes a thread/message (reversible, mirroring Drive's trash). This
keeps the "navigate a tree, get/put leaves, rm trashes" mental model intact.

### 1. Scope and inventory

File-by-file inventory mirroring gdrive-ftp's layout. `<same>` means the file is
structurally a near-copy with names/strings rebranded; differences are called out.

| Path | Purpose | Difference from gdrive-ftp counterpart |
| ---- | ------- | -------------------------------------- |
| `go.mod` | Module `gmail-ftp`, Go 1.25.x | Replace `google.golang.org/api/drive/v3` usage with `gmail/v1`; same `golang.org/x/oauth2`, `golang.org/x/term`, `google.golang.org/api`. `go.sum` regenerated. |
| `main.go` | CLI wiring: flags, one-shot vs interactive, `auth`/`log`/`completion zsh`/`__complete` subcommands, config-dir path helpers | Near-identical. Rename `gdrive`→`gmail` imports, banner "Connected to Gmail.", config dir `~/.config/gmail-ftp/`, default creds/token/log paths. `gmail.New(ctx, hc)` instead of `gdrive.New`. Flags `-creds/-token/-json/-no-log` unchanged. |
| `internal/auth/auth.go` | OAuth2 terminal consent flow + token cache/refresh (`savingSource`, OSC 52 clipboard, `codeFromRedirect`, `tokenFromFile`/`saveToken`) | **Copy almost verbatim.** Only change: the scope. Replace `drive.DriveScope` with the Gmail scopes (see §2). The flow, clipboard, state/CSRF handling, and token persistence are domain-agnostic and reused unchanged. |
| `internal/auth/auth_test.go` | Tests `codeFromRedirect` + `clipboardSeq` | Copy verbatim (these test domain-agnostic helpers); only the sample scope string in a redirect URL changes cosmetically. |
| `internal/gmail/client.go` | Thin Gmail v1 wrapper (replaces `internal/gdrive/client.go`) | **New.** Exposes: `ListLabels`, `ListThreads(labelID, query)`, `GetThread(id)`, `GetMessage(id)`, `GetAttachment(msgID, attID)`, `GetRawMessage(id)` (RFC822 `.eml`), `Search(query)`, `CreateDraft`, `SendDraft`/`Send`, `TrashThread`/`TrashMessage`, `ModifyLabels(add,remove)`. Owns `Ref`-equivalent types, `ErrNotFound`/`ErrAmbiguous`, and MIME/header parsing helpers. No folders/upload-media; instead message-part traversal. |
| `internal/gmail/client_test.go` | Unit tests for pure helpers | **New.** Test header parsing (subject/from/date extraction), attachment-part walking, MIME-decoding of body parts, label-name normalization — all from synthetic `gmail.Message` structs (no live API). |
| `internal/shell/shell.go` | REPL, raw-mode line editor, tokenizer, path resolution, Tab completion, `friendlyErr` | `<same>` skeleton. Path resolution now resolves label→thread→message instead of drive→folder→file. `Ref` carries `{ID, Name, Kind}` where Kind ∈ {label, thread, message}. `friendlyErr` rewrites the "Gmail API disabled" 403 (URL `gmail.googleapis.com`). Banner string `gmail:/>`. Tokenizer, quoting, completion plumbing copied verbatim. |
| `internal/shell/commands.go` | Command implementations + formatting helpers | `<same>` structure, remapped verbs (see §2 table). `ls`/`cd`/`pwd`/`get`/`put`/`rm`/`find` retained; gmail adds `search`, `label`/`unlabel`, `send`; `mkdir` is **dropped or remapped** (Gmail can create user labels — see §2). Local helpers `lcd`/`lls`/`lpwd` copied verbatim. |
| `internal/shell/output.go` | Owned JSON DTOs + `emit()` seam | `<same>` pattern. `fileEntry`→`entry` with `{path,name,id,kind,from,subject,date,unread,size,...}`. `actionResult` action vocabulary becomes `downloaded`/`drafted`/`sent`/`trashed`/`labeled`/`unlabeled`. The "never marshal the vendor struct" rule is preserved: translate `gmail.Message`→owned DTO. |
| `internal/shell/shell_test.go` | Pure-function tests (tokenize, splitPath, pwd, parseIDArg, byteCount, emit JSON, friendlyErr, …) | Copy the domain-agnostic tests verbatim (tokenize, splitPath, lastTokenStart, quoteArg, longestCommonPrefix, filterByPrefix, byteCount, emit). Rewrite the gdrive-specific stack tests (`TestPwd`, `TestCurrentID`, `TestSingleDriveArg`) for the label/thread stack. Add `TestArgKind`/`TestFriendlyErr` adapted to gmail verbs. |
| `internal/audit/audit.go` | Append-only JSONL logger + rotation | **Copy near-verbatim.** Only the `Operation` constants change: `OpDraft`/`OpSend`/`OpTrash`/`OpLabel`/`OpUnlabel` replace `OpUpload`/`OpTrash`/`OpMkdir`. `Entry` keeps `time/op/name/id/cwd` and adds optional `threadId`, `labelIds`. Rotation, perms (0700/0600), nil-logger no-op: unchanged. |
| `internal/audit/reader.go` | `Read`, `Verb`, `WriteJSON`, `WriteText` | `<same>`. `Verb` maps the new ops to user words (`drafted`/`sent`/`trashed`/`labeled`/`unlabeled`). |
| `internal/audit/browser.go` | tig-like read-only TUI | **Copy verbatim** except `render` header string ("gmail-ftp audit log") and `showDetail` fields (add thread/labels). Cursor/viewport/render logic is domain-agnostic. |
| `internal/audit/*_test.go` | audit/reader/browser tests | Copy near-verbatim; adjust the op constants and the `Verb` expectations. The rotation, read-skip-corrupt, moveCursor, viewportTop, rowText tests are reused unchanged. |
| `README.md` | User-facing spec | Rewritten for Gmail: concept, build, one-time Google setup (enable **Gmail API**, scopes), usage, command table, JSON output, audit log, notes/limitations, project layout, plugin install. Same section structure and tone. |
| `LICENSE` | MIT | Copy verbatim. |
| `.gitignore` | ignore secrets + binary | `<same>`; `gdrive-ftp`→`gmail-ftp` binary name. Keeps `credentials.json`/`token.json`/`*.token.json` ignored. |
| `plugins/gmail-ftp/skills/gmail-ftp/SKILL.md` | Agent skill | Rewritten: teach an agent to drive the Gmail CLI (auth prerequisite, label/thread/message model, one-shot usage, JSON contract, gotchas). Same skeleton as gdrive's SKILL.md. |
| `plugins/gmail-ftp/.claude-plugin/plugin.json` | Claude plugin manifest | `<same>` rebranded. |
| `plugins/gmail-ftp/.codex-plugin/plugin.json` | Codex plugin manifest | `<same>` rebranded; `repository` → gmail-ftp. |
| `.claude-plugin/marketplace.json` | Claude marketplace | `<same>` rebranded. |
| `.agents/plugins/marketplace.json` | Codex marketplace | `<same>` rebranded. |

**Not ported / intentionally out of scope for v1:** Shared-Drive corpus handling
(no Gmail analogue), media byte-for-byte upload (Gmail has no file upload; `put`
becomes drafting), and Google-native-doc export. The `id:` direct-addressing
convention **is** ported (message/thread/attachment IDs), because it is the same
"act on an item by its stable ID" affordance and is highly useful for agents.

### 2. Implementation approach

**Gmail API client wrapper (`internal/gmail/client.go`).** Build the service with
`gmail.NewService(ctx, option.WithHTTPClient(hc))` (mirrors `drive.NewService`).
The Gmail v1 resource shape differs from Drive, so the wrapper hides it:

- `ListLabels(ctx)` → `Users.Labels.List("me")`; returns owned `Label{ID,Name,Type}`
  refs sorted with system labels (INBOX/SENT/DRAFT/…) first, then user labels.
- `ListThreads(ctx, labelID, query)` → `Users.Threads.List("me").LabelIds(labelID).Q(query)`,
  paginated via `Pages`. Each thread is fetched with `Format("metadata")` for the
  subject/from/date snippet (one batched-enough call set; see quota risk §5).
- `GetThread(ctx, id)` → `Users.Threads.Get("me", id).Format("full")` returns the
  full message list.
- `GetMessage(ctx, id, format)` → `Users.Messages.Get`. `format=metadata` for
  listings, `full` for body, `raw` for `.eml`.
- `GetAttachment(ctx, msgID, attID)` → `Users.Messages.Attachments.Get`; the body
  data is base64url — decode in the wrapper and stream bytes out.
- `Search(ctx, query)` → `Users.Messages.List("me").Q(query)`; this is Gmail's
  native search, the direct analogue of Drive's `name contains`.
- `CreateDraft(ctx, raw)` / `SendDraft(ctx, id)` / `Send(ctx, raw)` → `Users.Drafts.*`,
  `Users.Messages.Send`. `raw` is an RFC 5322 message we assemble from a local file
  (the `put` payload) plus headers.
- `TrashThread`/`TrashMessage` → `Users.Threads.Trash` / `Users.Messages.Trash`
  (reversible; mirrors Drive's `Trashed:true`).
- `ModifyLabels(ctx, id, add, remove)` → `Users.Messages.Modify` /
  `Users.Threads.Modify`. Backs `label`/`unlabel`.

Pure helpers (unit-testable without the API): `headerValue(msg, "Subject")`,
`decodePart(part)` (base64url body), `walkParts(payload)` to collect attachments,
`isUnread(msg)` (LABEL_IDS contains `UNREAD`), `normalizeLabelName` (strip
`[Gmail]/` for display while keeping the ID).

**OAuth scopes.** gdrive-ftp uses the single broad `drive` scope. For Gmail the
quality-conscious choice is the **narrowest set that covers the command surface**,
not a single super-scope:

- `https://www.googleapis.com/auth/gmail.readonly` — `ls`/`cd`/`get`/`find`/`search`.
- `https://www.googleapis.com/auth/gmail.compose` — `put` (draft) + `send`.
- `https://www.googleapis.com/auth/gmail.modify` — `rm` (trash) + `label`/`unlabel`.

`gmail.modify` is a superset of read + most write (it cannot permanently delete —
which is exactly the safety property we want, matching Drive's "trash not delete").
A pragmatic v1 may request **`gmail.modify` + `gmail.compose`** to cover everything
except hard-delete. The scope decision is a documented config constant in
`auth.go` so it is auditable in one place. (Flagged in §5 as the highest-sensitivity
risk for OAuth-consent review.)

**Command set mapping.**

| gdrive verb | gmail verb | Behavior |
| ----------- | ---------- | -------- |
| `ls [dir]` | `ls [dir]` | root → labels; in a label → threads; in a thread → messages + attachments. |
| `cd [dir]` | `cd [dir]` | navigate label → thread. Messages are leaves (cannot `cd` into). |
| `pwd` | `pwd` | print `/INBOX/<thread-subject>`. |
| `find <pat> [dir]` | `find <pat> [dir]` | substring match over thread subjects in scope (client-side, mirrors gdrive's re-filter). |
| (n/a) | `search <gmail-query>` | **new**: raw Gmail search syntax (`from:x is:unread`), the power tool; `find` stays the simple subject substring. |
| `get <remote> [local]` | `get <remote> [local]` | a message → `.eml` (raw); an attachment leaf → raw bytes; atomic temp-rename (`saveToFile` reused verbatim). |
| `put <local> [remote]` | `put <local> [remote]` | create a **draft** from a local RFC822 file (or a body file + `remote` as recipient). Returns draft id. Logged. |
| (n/a) | `send <local>` / `send id:<draftID>` | **new**: send a draft or a composed message. Logged. Highest-impact mutation. |
| `mkdir <name>` | `label <msg> <name>` / `mklabel <name>` | Gmail has no folders; `mkdir`'s "create container" maps to **creating a user label**; applying a label to a thread/message is `label`. v1 keeps `label`/`unlabel`; `mklabel` is optional. |
| `rm <name>` | `rm <name>` | trash a thread (default) or message; reversible. Logged. |
| `lcd`/`lls`/`lpwd` | same | copied verbatim (local FS helpers). |
| `help`/`?` | same | regenerated table. |

**Shell-loop reuse.** `runTerminal`/`runScanner`/`dispatch`/`Execute`,
`crlfWriter`, the tokenizer, `quoteArg`, `lastTokenStart`, `splitPath`,
`filterByPrefix`, `longestCommonPrefix`, and the whole Tab-completion machinery
(`autoComplete`/`completeInput`/`Complete`/`argKind`/`remoteNames`/`localNames`)
are **structurally unchanged**. Only `remoteNames` and the resolver call into the
gmail client (labels/threads/messages) instead of the drive client, and `argKind`
gains `search`/`send`/`label`/`unlabel` entries. The `id:` parsing
(`parseIDArg`) is copied verbatim and now resolves message/thread/attachment IDs.

### 3. Quality strategy

The quality bar is **gdrive-ftp's own bar**: every pure function has a table-driven
`_test.go`, the SDK struct is never marshaled directly, mutations are audited, and
`go vet` is clean. Concretely:

**Compiler / static checks (must pass before reporting done):**
- `go build ./...`
- `go vet ./...`
- `gofmt -l .` returns empty (no unformatted files).
- `go test ./...` green. (If `golangci-lint` is available project-locally it is run;
  it is **not** installed globally — system-safety forbids global installs, so a
  missing linter is a documented soft check, never a blocker.)

**Unit tests (no live Gmail credentials required).** Everything the API does not
touch is tested against synthetic structs and in-memory writers, exactly as
gdrive-ftp does:

- `internal/auth`: `codeFromRedirect` (state/CSRF, bare-code, error param) and
  `clipboardSeq` (OSC 52 + tmux passthrough) — copied verbatim.
- `internal/gmail`: header extraction, MIME body base64url decode, attachment-part
  walking, unread detection, label-name normalization — all from hand-built
  `gmail.Message`/`gmail.MessagePart` literals.
- `internal/shell`: tokenize, splitPath, lastTokenStart, quoteArg,
  longestCommonPrefix, filterByPrefix, parseIDArg, argKind, pwd over a
  label/thread stack, byteCount, `emit` JSON-vs-text seam, `encodeErrorJSON`,
  `toEntry` DTO translation (omitempty of size/subject), `friendlyErr` for the
  Gmail-disabled 403.
- `internal/audit`: marshal/omitempty, append+perms (0600/0700), nil-logger no-op,
  rotation at cap, ring shift+drop, read-concatenate-oldest-first,
  skip-corrupt-line, moveCursor/viewportTop/rowText — copied with op-constant edits.

**Testability via interfaces/mocks.** To keep command dispatch testable without a
network, the shell depends on a **narrow `gmailClient` interface** (the set of
methods commands call) rather than the concrete `*gmail.Client`. This is a
deliberate, low-cost improvement over gdrive-ftp (which used the concrete client);
it lets a fake client return canned threads/messages so `cmdLs`/`cmdGet`/`cmdRm`
output formatting and `id:` dispatch are unit-tested end-to-end without Gmail.
The interface is defined in `shell` and satisfied by `*gmail.Client`.

### 4. Delivery plan

Ordered, each step independently buildable, testable, and committable
(`go build ./... && go test ./...` green at every step):

1. **Scaffold module.** `go.mod` (`module gmail-ftp`, Go 1.25.x), `LICENSE`,
   `.gitignore`, empty package dirs. `go mod tidy` pulls `gmail/v1`. Commit.
2. **Auth.** Copy `internal/auth/auth.go`+test verbatim; swap the scope constant.
   `go test ./internal/auth/...` green. Commit.
3. **Gmail client.** `internal/gmail/client.go` + `client_test.go`: types, service
   constructor, list/get/search/draft/send/trash/modify wrappers, pure parsing
   helpers with full unit tests. Commit.
4. **Shell + commands + output.** Port `shell.go`/`commands.go`/`output.go` and
   `shell_test.go`; wire the `gmailClient` interface, the verb table, the DTOs,
   and `friendlyErr`. Add a fake client for command-level tests. Commit.
5. **Audit.** Port `internal/audit/*` (+ tests) with the new op constants. Commit.
6. **main.go.** CLI wiring, flags, config-dir helpers, `auth`/`log`/`completion`/
   `__complete` subcommands, banner. `go build` produces the `gmail-ftp` binary.
   Commit.
7. **Plugin + README + skill.** `plugins/gmail-ftp/**`, both `marketplace.json`,
   `README.md`, `SKILL.md`. Commit.

(Per trip protocol, the Constructor does not run git directly in this shared
branch-only tree; the lead serializes commits. The steps above are the commit
*boundaries* the lead can land.)

### 5. Risk assessment

| # | Risk | Likelihood / impact | Mitigation |
| - | ---- | ------------------- | ---------- |
| R1 | **OAuth scope sensitivity.** Gmail scopes are "restricted"; broad scopes trigger Google's verification and look alarming on the consent screen. | High sensitivity | Request the **narrowest** scopes covering the surface (`gmail.modify` + `gmail.compose`, never the full `https://mail.google.com/`). Centralize scope choice as one documented constant in `auth.go`. Document in README that this is a personal "Desktop app" OAuth client with the user as a test user (same as gdrive-ftp), so no Google verification is needed. **Never** request hard-delete capability. |
| R2 | **API quotas / N+1 thread fetches.** Listing a label returns thread IDs; fetching each thread's subject is an extra call → quota burn and latency on large mailboxes. | Medium / Medium | Use `Format("metadata")` with a restricted `metadataHeaders` set (Subject/From/Date) to minimize payload. Cap default listing page size; paginate transparently. Use Gmail's `Q` to scope. Cache thread metadata within a single command (as gdrive's `find` caches parent lookups). Document the latency note in README, mirroring gdrive's "expect a brief pause on large folders." |
| R3 | **Threads-vs-messages modeling ambiguity.** A "path" can name a label, a thread, or a message; subjects are non-unique and may be empty. | Medium / Medium | Resolver returns `ErrAmbiguous` (ported verbatim semantics) when a subject matches multiple threads, refusing to guess — exactly gdrive's duplicate-name discipline. Always allow `id:` addressing as the unambiguous escape hatch (ported). Render empty subjects as `(no subject)` but key navigation off IDs. Document the model prominently in README + SKILL.md. |
| R4 | **Attachment handling.** Attachment bytes are base64url-encoded inside message parts, can be large, and parts nest. | Medium / Low | `walkParts` recursion is unit-tested against nested synthetic payloads. Decode in the wrapper and stream to `saveToFile` (atomic temp-rename, length-aware) — reuse gdrive's proven download path so an interrupted transfer never corrupts a local file. |
| R5 | **Credential storage.** Same secret-handling risk as gdrive: `credentials.json`/`token.json` grant mailbox access. | High impact / Low likelihood | Reuse gdrive's exact discipline: token cached `0600` under `0700` config dir; both files git-ignored; audit log never records contents or credentials; SKILL.md repeats "never read/print/commit these." Gmail raises the stakes (mailbox > drive), so README's warning banner is strengthened accordingly. |
| R6 | **Send is irreversible.** Unlike trash, a sent email cannot be recalled. | Low likelihood / High impact | Make `send` an explicit, separate verb (never a side effect of `put`); `put` only ever creates a **draft**. `send` is audited with full recipient/subject metadata. Consider (note for review) a confirmation prompt or a `--yes` gate in interactive mode; for one-shot/agent use it must be explicit by construction. |
| R7 | **Gmail API disabled for the project (403).** Same failure class gdrive rewrites. | Low / Low | Port `friendlyErr` with the `gmail.googleapis.com` activation URL + project number extraction; covered by a `TestFriendlyErr` unit test. |
| R8 | **System-safety constraint.** Detection returned `system_changes_authorized:false` (regular project). | n/a | All dependencies are project-local via `go.mod`/`go.sum`; no global installs, no shell-profile or system edits. A missing global linter is a documented soft check, never worked around with a global install. |

## Review Notes

(placeholder — to be filled by reviewers in Step 2)
