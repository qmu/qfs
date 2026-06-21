# Architect Review Criteria — gmail-ftp (Coding Phase)

Author: Architect
Status: prepared (pre-review; Constructor's source not yet inspected)
Applies-to: the Review-and-Testing step of the Coding Phase

This is the **structural review checklist** the Architect will apply to the
Constructor's `gmail-ftp` implementation. It is grounded in a fresh re-read of the
reference `../gdrive-ftp` boundary contracts (main.go, internal/auth, internal/gdrive,
internal/shell, internal/audit) and in the locked specs: `models/model-v2.md`,
`designs/design-v2.md`, and `plan.md` Amendment 1.

Scope discipline for the review: **analytical / code review only** — no test
execution, no build. Each finding must follow the Critical Review Policy: at least
one concern or trade-off even on approval, with a concrete structural proposal.

Reference boundary signatures captured for parity checks:
- `auth.Client(ctx, credsPath, tokenPath) (*http.Client, error)` — scope is the
  *only* substantive change point (`drive.DriveScope` today, line auth.go:43).
- `gdrive.New(ctx, hc) (*Client, error)`; client method surface List / ListDrives /
  GetByID / Search / FindChildren / FindDir / FindOne / Mkdir / Upload / Download /
  Export / Trash; sentinels `ErrNotFound`, `ErrAmbiguous`.
- `shell.New(ctx, client, out, jsonOut, log) *Shell`; dispatch table in `init()`;
  `Ref{ID,Name,DriveID}`; `idPrefix = "id:"`; `parseIDArg`.
- `audit.New(path)`, `Logger.Record(ctx, Entry)`, `Op*` constants, `Entry`.
- `shell.EncodeErrorJSON(w, err)`; owned DTOs `fileEntry`/`actionResult`/`pwdResult`/
  `errorResult`; the "never marshal the vendor struct" rule (output.go:13-16).

---

## A. Package-boundary fidelity vs gdrive-ftp

A1. **Package layout one-for-one.** `main.go`, `internal/auth/`, `internal/gmail/`
    (renamed from `internal/gdrive/`), `internal/shell/`, `internal/audit/`,
    `plugins/gmail-ftp/...`, both `marketplace.json`. The *only* renamed package is
    `gdrive → gmail`; the *only* genuinely new file is `internal/gmail/model.go`
    (model-v2 §5, design-v2 §1). Flag any extra packages, merged packages, or
    leaked responsibilities.

A2. **Dependency direction unchanged.** `shell` depends on `gmail` + `audit`;
    `main` wires `auth → gmail → shell`; `audit` depends on nothing project-local;
    `gmail` is the *only* package importing `google.golang.org/api/gmail/v1`. No
    import cycles, no SDK import leaking into `shell`/`audit`/`main`.

A3. **auth boundary — signature identical, scope is the sole change.**
    `auth.Client(ctx, creds, token) (*http.Client, error)` must be byte-for-byte in
    behavior (consentFlow, OSC 52 clipboard, CSRF `state`, `savingSource` refresh,
    token cache 0600/dir 0700). Confirm the consent UX, `codeFromRedirect`,
    `tokenFromFile`/`saveToken` were copied, **not reinvented**. The diff against
    gdrive's auth.go should be limited to: package doc, the scope constant, and
    cosmetic "gmail-ftp" strings.

A4. **backend-client boundary — thin, FTP-flavored, SDK quarantined.** The new
    `internal/gmail/client.go` exposes a small surface (model-v2 §4 / design-v2 §1):
    ListLabels, ListMessages(labelID,query), GetMessage(id,format), GetThread(id),
    GetAttachment, GetRawMessage/raw `.eml`, Search, CreateDraft, TrashMessage,
    TrashThread, ModifyLabels, CreateLabel, DeleteLabel (Send only in v1.1, see E).
    The SDK service is built once via `gmail.NewService(ctx, option.WithHTTPClient(hc))`
    mirroring `drive.NewService`. **All thread/batch-fetch/N+1 complexity lives behind
    this boundary, never in the shell** (model-v2 §4). Flag any `*gmail.Service` or
    `gmail.Message` handling that escaped into `shell`.

A5. **shell boundary — constructor + dispatch preserved.**
    `shell.New(ctx, client, out, jsonOut, log)` signature and the REPL architecture
    (runTerminal/runScanner/dispatch/Execute, crlfWriter, tokenizer, quoteArg,
    lastTokenStart, splitPath, filterByPrefix, longestCommonPrefix, the whole
    Tab-completion machinery) must be structurally unchanged. Verb names preserved;
    new verbs additive only. Banner re-pointed (`gmail:/>`), `friendlyErr` re-pointed
    to the Gmail enablement URL (`gmail.googleapis.com`).

A6. **audit boundary — JSONL ring + browser intact, vocabulary remapped.**
    `audit.New(path)`, `Logger.Record(ctx, Entry)`, `Read`, `WriteJSON`/`WriteText`/
    `Browse` signatures unchanged. Rotation ring (5 MiB × keep 3), 0600/0700 perms,
    nil-logger no-op, best-effort write (never breaks the command), the empty-`Op`
    guard — all preserved. Only `Operation` constants and `Entry` fields change
    (design-v2 §1 audit row).

A7. **output/DTO boundary — owned DTOs only.** The vendor `*gmail.Message`/label
    struct is **never** marshaled directly; a `toEntry`-style translator converts to
    an owned DTO, mirroring output.go:13-16 and `toFileEntry`. `emit(v, text)` seam
    and `EncodeErrorJSON` preserved. Confirm no `json` tag is on a vendor type.

A8. **main.go wiring parity.** Flags `-creds/-token/-json/-no-log` unchanged; the
    pre-auth branches preserved in order: `completion zsh` and `log` branch *before*
    `auth.Client`; `__complete` must bail before `auth.Client` (never launch OAuth on
    Tab — main.go:62-66, completeForShell:137-139); `auth` subcommand; one-shot vs
    interactive dispatch; `signal.NotifyContext` Ctrl-C; `fatal`. Config dir
    `gmail-ftp`, log path, banner "Connected to Gmail." re-pointed.

---

## B. 2-level navigation mapping (`Ref.Kind ∈ {label, message}`)

B1. **Ref carries a Kind narrowed to exactly {label, message}.** Verify `Ref` is
    `{ID, Name, Kind, ThreadID}` (design-v2 §1 shell row) and that **no `thread`
    Kind value exists**. `ThreadID` is a *field*, never a stack frame. Reject any
    `Ref.Kind == "thread"` or a third stack tier.

B2. **`ls` realizes the two levels.** `ls /` lists **labels**; inside a label `ls`
    lists **messages**; `ls <message>/` lists that message's **attachments** as
    leaves inside the message. No thread grouping in any listing (design-v2 §0).

B3. **`cd` targets root→label only.** `cd INBOX`, `cd "Work/Receipts"` enter a
    label; a message is a leaf you **cannot** `cd` into; a thread is **never** a `cd`
    target. Confirm the resolver (`resolveDir`/`startStack` analog) walks label
    segments only and refuses to descend into a message. `pwd` prints `/INBOX` (the
    label), never `/INBOX/<thread>` or `/INBOX/<message>`.

B4. **Resolver matrix is label→message (two levels), not drive→folder→file and not
    label→thread→message.** Path resolution must map the first segment to a label and
    the leaf to a message/attachment. Nested labels (`Work/Receipts`) map onto
    `/`-separated segments but each level is still a label addressable from root
    (model-v2 §2). Flag any folder-tree recursion carried over verbatim that assumes
    real subfolders.

B5. **Attachments as message leaves — reuse the "thing contains things" seam.**
    `ls <message>/` → list attachments; `get <message>/<attachment>` or
    `get id:<attachmentRef>` downloads one attachment byte-for-byte (model-v2 §2).
    No invented subfolder type, no thread depth.

B6. **Message naming + collision strategy.** Synthesized stable name
    (date-prefix + subject slug; empty subject → `(no subject)`), with `id:`
    addressing as the **primary/canonical** way to act on a specific message
    (model-v2 §2, design-v2 R3). `ErrAmbiguous` must be returned (ported semantics)
    when a synthesized name matches multiple messages — the resolver must **refuse to
    guess**, exactly as `FindOne`/`FindDir` do today.

---

## C. Thread opt-in (`threadId` field / `id:thread:<id>`)

C1. **Thread surfaced only two ways at the shell boundary.** (a) a `threadId` field
    on every DTO row, and (b) `id:thread:<id>` addressing. No `cd thread`, no thread
    `Ref.Kind`, no resolver disambiguation at a shared depth (model-v2 §3 item 3).

C2. **`id:thread:` parsing extends `parseIDArg`, not a new command mode.** Confirm
    `parseIDArg` (or its gmail analog) recognizes both `id:<id>` and `id:thread:<id>`
    and that `id:thread:<id>` routes to `GetThread`/`TrashThread`, while plain
    `id:<id>` routes to the single-message path. The literal, case-sensitive,
    no-`/`-in-id discipline of the reference `parseIDArg` (shell.go:665-674) must
    hold.

C3. **`GetThread` is an internal batching / opt-in export only.** It must back
    `id:thread:<id>` access and the `.mbox` export; it must **not** produce a `Ref`
    frame or appear as a navigation tier (design-v2 §2). No second N+1 tier introduced
    (model-v2 §3a) — confirm thread expansion is not triggered during ordinary `ls`.

---

## D. Least-privilege OAuth scope constant

D1. **Single source of truth in auth.go.** The scope set is declared **once** as a
    documented constant in `auth.go` (design-v2 §2, Architect Sug 4) — no scope
    wording duplicated/drifting across auth.go, README, SKILL.md.

D2. **Exact scope set = `gmail.modify` + `gmail.compose`.** Verify the constant is
    the scoped union `https://www.googleapis.com/auth/gmail.modify` +
    `https://www.googleapis.com/auth/gmail.compose` (plan Amendment 1; model-v2 §4;
    design-v2 §2/R1).

D3. **Forbidden scopes absent.** **Never** `https://mail.google.com/` (full), and
    **never** any hard-delete capability. The constant's comment should state that
    `modify` subsumes read and cannot hard-delete (the safety property mirroring
    Drive's "trash not delete").

D4. **`send` scope deferred.** No `gmail.send`-granting scope is wired into the v1
    constant (send is v1.1, see E2). If the constant lists a send scope, it is a
    least-privilege regression for v1.

---

## E. Verb safety + deferred-verb discipline (plan Amendment 1)

E1. **`rm` = single-message trash by default (reversible).** `rm <message>` must
    call `TrashMessage` (Gmail TRASH label), **never** `TrashThread`. A whole thread
    is trashed only via the explicit `rm id:thread:<id>` opt-in. This is the
    load-bearing minimal-blast-radius safety promise (plan Amendment 1; model-v2 §3
    item 6; design-v2 R7). Confirm the fake-client unit test asserts
    `rm <message> → TrashMessage, never TrashThread` exists (design-v2 §3) — its
    *presence/shape* is reviewed analytically; I do not run it.

E2. **`send` / `label` / `unlabel` deferred to v1.1 — stub only, NOT wired into v1
    dispatch.** Per plan Amendment 1 the Constructor's design *may define* these verbs
    but must mark them deferred/stubbed and **not register them in the v1 dispatch
    table** nor in `argKind`/help. Verify: (a) no `send`/`label`/`unlabel` entry in
    the live `commands` map, (b) any stub is clearly inert, (c) the v1 OAuth scope
    does not pre-grant send. Flag any irreversible `send` reachable in v1.

E3. **`put` = create draft, never send.** `put <local.eml>` creates a **draft**
    only (`CreateDraft`); sending is never an implicit side effect (model-v2 §3 item
    5; design-v2 §2). Confirm there is no code path where `put` calls a send method.

E4. **`mkdir` = create a Gmail user label.** `mkdir <name>` → `CreateLabel`
    (model-v2 §2; plan Amendment 1). Confirm it does not create a folder analog or a
    message.

E5. **Mutation auditing.** Each shipped mutation (`put`-draft, `rm`-trash,
    `mkdir`-label) records an audit Entry with the remapped `Op*` constant; the
    `rm`-message vs (deferred) thread distinction is auditable distinctly. Audit
    remains best-effort (never breaks the command). Deferred verbs add no live audit
    paths in v1.

E6. **`get` semantics.** message → `.eml` (raw RFC 822); attachment leaf → raw
    bytes; optional `.txt` readable export = the "export" analog. Atomic temp-rename
    download path (`saveToFile` reused verbatim) preserved (design-v2 §2).

---

## F. Testability — narrow `gmailClient` interface

F1. **Shell depends on an interface, not the concrete `*gmail.Client`.** Verify a
    narrow `gmailClient` interface (exactly the methods the commands call) is defined
    **in `shell`** and satisfied by `*gmail.Client` (design-v2 §3). This is the
    deliberate, low-cost improvement over gdrive-ftp and the seam that makes
    command-dispatch tests network-free.

F2. **Interface is minimal and stable.** It should not leak SDK types
    (`*gmail.Message` may cross only as an owned/translated type or behind a method
    that returns owned structs where the design intends). Flag an over-broad
    interface that re-exposes the whole SDK, defeating quarantine.

F3. **Fake-client tests cover the safety-critical paths.** Analytically confirm the
    presence/shape (not execution) of fake-client tests for `cmdLs`/`cmdGet`/`cmdRm`
    output + `id:` dispatch, and specifically the `rm`-single-message assertion
    (design-v2 §3). Pure-helper tests (tokenize, splitPath, parseIDArg incl.
    `id:thread:`, messageName + collisions, MIME/base64url decode, walkParts) should
    exist per design-v2 §3.

---

## G. Traceability — Direction → Model → Design → code

G1. **Every shipped behavior traces to a locked decision.** For each verb and the
    navigation model, confirm a clear line: plan Amendment 1 / model-v2 §N /
    design-v2 §N → the implementing code. No code realizing a *rejected* alternative
    (3-level navigation, thread `cd`, full-mailbox scope, `put`-sends, v1 `send`).

G2. **Docs match code (single hierarchy ships).** README command table and
    SKILL.md must document the resolved 2-level model **verbatim** so a persona's
    first `ls` matches the docs (design-v2 §0). The N+1 listing cost and the `id:` /
    `id:thread:` addressing must be documented (model-v2 §3a). Flag any doc that
    still describes a thread tier or a send verb as shipping in v1.

G3. **No orphan structure.** Anything new beyond `internal/gmail/model.go` (the one
    sanctioned new file) must trace to an explicit design decision, or it is flagged
    as scope creep.

---

## H. Structural risks to watch (carried into the review)

These are the highest-leverage risks; each will get a concrete structural proposal
if observed:

- **R-A. Quarantine leak.** SDK `*gmail.*` types or `*gmail.Service` escaping the
  `gmail` package into `shell`/`output`/`audit`, breaking the "vendor struct never
  marshaled / SDK behind one package" boundary.
- **R-B. Resolver carry-over bug.** gdrive's drive→folder→file recursion copied
  verbatim such that a message becomes `cd`-able or a phantom third tier appears —
  i.e. `Ref.Kind` not truly narrowed.
- **R-C. Blast-radius / irreversibility regression.** `rm` reaching `TrashThread`
  by default, `put` reaching a send path, or a live `send`/`label`/`unlabel` verb
  wired into v1 dispatch.
- **R-D. Scope over-grant.** Full `mail.google.com`, hard-delete, or a send scope
  in the v1 constant; or scope wording duplicated outside the single auth.go source.
- **R-E. N+1 doubling.** A thread expansion path firing during ordinary `ls`,
  re-introducing the second N+1 tier the 2-level decision exists to avoid.
- **R-F. Interface over-exposure.** A `gmailClient` interface so wide it re-exports
  the SDK, defeating testability and quarantine.

## Review Notes

- This checklist is intentionally exhaustive on the *boundary contracts* because the
  whole "same experience" guarantee rests on auth/shell/audit/output being reused
  near-verbatim and only the `gmail` backend + command bodies being email-specific.
- Per the Critical Review Policy, the review will record at least one
  concern/trade-off even under an approve decision, each paired with a concrete
  structural proposal, and will issue a structured decision
  (Approve with observations / Approve with minor suggestions / Request revision).
