---
created_at: 2026-06-22T12:37:01+09:00
author: a@qmu.jp
type: enhancement
layer: [UX, Domain, Infrastructure]
effort:
commit_hash:
category:
depends_on: [20260622123701-unify-gmail-gdrive-ftp.md]
---

# Pipe-composable byte streaming + cross-backend `cp` (Drive file → Gmail draft attachment)

## Overview

The headline reason for merging the two tools (see `depends_on`): move a **Google Drive file** into a **Gmail draft** as an attachment without the user downloading and re-uploading by hand. The design is **pipe-composable** (Unix-style), with a convenience verb on top.

**Core primitive — a `-` stdin/stdout streaming convention** so operations compose like Unix pipes:
- `get <remote> -` streams the object's bytes to **stdout**.
- `attach <draft> --name <n> [--type <t>] -` reads attachment bytes from **stdin**, appends to the draft, never sends.

So Drive → draft attachment is literally a pipe, and it composes with **any** tool (not just Drive→Gmail):
```
gftp get /drive/Reports/q3.pdf -  |  gftp attach /mail/id:draft:r-8423 --name q3.pdf --type application/pdf -
curl -s https://host/report.csv   |  gftp attach /mail/id:draft:r-8423 --name report.csv -
gftp get /mail/id:att:18f1a2b:AX - |  gftp put /drive/Backups/ --name from-mail.pdf -   # reverse direction
```

**Convenience verb `cp` (sugar over the primitive)** for the common case and for use **inside** the interactive prompt (where OS pipes don't exist between two prompt commands):
```
cp /drive/<path-or-id:>  /mail/id:draft:<id>
```
`cp` == an in-process `get … - | attach … -`, but it additionally **infers** the filename/content-type from the Drive file, **auto-exports** Google Docs, and **pre-checks** the size — metadata a blind pipe would need passed via `--name/--type`. Both `cp` and the pipe ride the **same internal byte-mover**, so they stay consistent. The result is always a **draft mutation only — never sends** (send stays the explicit, separate, audited verb). v1 only needs Drive → draft-attachment for `cp`, but the `-` primitive generalizes to any source/sink.

## Key Files

- `internal/shell/commands.go` — add `cmdCp`. Mirror `putAttach`'s body (the v1.1 attach path is the exact template): resolve target draft via `parseDraftIDArg`, then `GetDraftRaw` → `AppendAttachment` → `UpdateDraft` (single update, so a mid-flight failure can't corrupt the draft). Source bytes come from Drive instead of `os.Open`.
- `internal/gdrive/client.go` — `Download(ctx, fileID, w io.Writer)` and `Export(ctx, fileID, mime, w)` both already take an `io.Writer`; stream into a buffer/`io.Pipe` — **no client change needed**. `GetByID`/`FindOne` resolve the source; `IsGoogleDoc`/`ExportFormat` decide export-vs-raw and the exported filename/extension.
- `internal/gmail/model.go` — `AppendAttachment(raw []byte, att MIMEAttachment) []byte` and `MIMEAttachment{Filename, ContentType, Content []byte}` are the pure handoff seam: Drive bytes → `Content`, Drive (or exported) filename → `Filename`. Reuse as-is.
- `internal/gmail/client.go` — `GetDraftRaw`/`UpdateDraft` are the target-draft methods.
- `internal/shell/shell.go` — `cmdCp` needs BOTH live backend clients (the first command that spans both); wire through the unified shell. Add `cp` to `argKind`/Tab-completion: arg1 = a `/drive` remote path, arg2 = a draft id (no path completion, like `send`/`put`'s draft arg).
- `internal/audit/audit.go` — audit the transfer recording **source Drive ID + target draft ID** (reuse `OpDraft` or add `OpAttach`).
- `README.md`, `plugins/gftp/skills/gftp/SKILL.md` — document `cp /drive/... /mail/id:draft:<id>`; note Google-Docs export behavior and that it never sends.

## Related History

- `gmail-ftp` v1.1 ticket (`20260621191543`) shipped `put <file> <draft>` (attach), `compose`, `send`, and the pure multipart-MIME builder — the direct, reusable precursor. This ticket sources the attachment bytes from Drive instead of local disk.
- Trip safety bar (Amendment 1): `put`/`compose` never send; `send` is the sole irreversible verb. `cp` must terminate at a draft, audited, never send.

## Implementation Steps

0. **`-` stdin/stdout streaming primitive (do this first — it's the core):**
   - Teach `get <remote> -` to stream bytes to **stdout** (works for any readable object: a Drive file, a Gmail message `.eml`, a Gmail attachment `id:att:`). Reuse each backend's existing `io.Writer`-based download/export — point the writer at `os.Stdout`.
   - Add `attach <draft> --name <n> [--type <t>] -` that reads attachment bytes from **stdin**, then `GetDraftRaw → AppendAttachment → UpdateDraft` (never sends). `--name` required when reading stdin (no source filename to infer); `--type` optional (default `application/octet-stream` or guessed from `--name`).
   - Both `-` ends stream (use `io.Pipe`/buffered copy); enforce the Gmail size ceiling on the sink side.
   - This makes the tool pipe-composable with arbitrary external tools (`curl`, `gpg`, etc.) and is the primitive `cp` is sugar over.
1. **Resolve Drive source** exactly as gdrive `cmdGet`: `resolveFile(path)` (or `GetByID` for `id:`) → `*drive.File`. If `IsGoogleDoc(f)`, pick `ExportFormat(f.MimeType)` for `(mime, ext)` and the attachment filename becomes `f.Name+ext`; else filename = `f.Name`, content-type = the Drive `mimeType` (or `contentTypeForName`).
2. **Stream bytes** into a buffer (binary → `Download`; native Google doc → `Export`) — both accept `io.Writer`. For large files use `io.Pipe` to avoid buffering the whole file in RAM, and **guard against Gmail's ~25 MB message-size ceiling** with a clear pre-`UpdateDraft` error.
3. **Resolve target draft** via `parseDraftIDArg(arg)` → `draftID`.
4. **Attach** (verbatim from `putAttach`): `threadID, raw, _ := GetDraftRaw(ctx, draftID)`; `newRaw := AppendAttachment(raw, MIMEAttachment{Filename, ContentType, Content: bytes})`; `UpdateDraft(ctx, draftID, newRaw)`. One update call — no partial-corruption window.
5. **Audit** the transfer (source Drive ID + target draft ID); never send.
6. **Wire dispatch + completion** for `cp`; clear errors when roots don't match the supported direction (Drive→draft) in v1.
7. **Docs** README/SKILL.
8. **Quality gate:** `go build/vet/gofmt/test` clean. Tests (fake Drive + fake Gmail clients, no live creds): cp attaches Drive bytes to a draft and NEVER sends; Google-Doc source is exported (filename gets the export extension); oversize source errors before `UpdateDraft`; a missing draft 404s cleanly; a missing Drive source 404s cleanly.

## Considerations

- **Partial-failure / recovery (operation/operational-planning, observability):** the transfer is two dependent external calls (Drive fetch, Gmail attach). On attach failure, don't leave orphans; report cleanly and make a retry idempotent (re-running resolves to the same draft; avoid duplicate-attachment build-up — consider detecting an already-present identical part, or document that a retry appends again). Wrap each leg with a finite timeout + bounded retries; record both legs in the audit log so a half-done transfer is reconstructable.
- **Google Docs have no raw bytes** — they MUST be exported (`.docx/.xlsx/.pptx/...`) before attaching; the attachment filename/content-type come from `ExportFormat`, not the raw `mimeType`. Missing this attaches an empty/garbage file.
- **Size ceiling:** Gmail caps total message size (~25 MB via API). Fail fast with an actionable error before `UpdateDraft`; a future option could insert a Drive share link in the body instead of bytes (out of scope for v1).
- **Reversible-by-default:** `cp` is a draft mutation; the user still runs `send` explicitly to deliver. Never bundle attach+send.
- **Direction scope:** v1 supports Drive → draft only. Keep the `cp <src> <draft>` shape general so Drive→local / local→Drive / Gmail-attachment→Drive could be added later, but error clearly on unsupported directions now.
- **In-prompt vs out-of-prompt composition (design/modeless):** the `-` pipe primitive is the **out-of-prompt** composition path (one-shot / agent / scripts, joined by OS pipes). **Inside** the interactive `gftp>` prompt there is no OS pipe between two prompt commands, so `cp` is the in-prompt path. Both must call the **same internal byte-mover** so behavior is identical; do not let `cp` and `get -|attach -` diverge. (A built-in prompt `|` operator is out of scope.)
- **Pipe loses source metadata:** a blind `get - | attach -` has no Drive filename/content-type/size — hence `--name`/`--type` on `attach` and the size check on the sink. `cp` infers all three from the Drive file; keep that asymmetry documented so users know when to pass flags.
- **Testability:** the whole path must be exercisable with fake backend interfaces and the pure MIME builder — no live credentials — matching the inherited table-driven bar. Test the `-` ends with in-memory stdin/stdout (inject readers/writers), not real pipes.
