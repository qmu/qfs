---
created_at: 2026-07-11T12:15:26+09:00
author: a@qmu.jp
type: enhancement
layer: [Infrastructure, Domain]
effort:
commit_hash: d868fb3
category: Added
depends_on:
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
---

# Chatwork declared driver with file attach/detach — the API-key-style declared proof

## Overview

Ship a real Chatwork driver **as a declared (qfs-query) driver**, covering rooms, messages, and
**files** (list, download, upload/attach, delete where the API allows). Chatwork is an API-key
REST API (`x-chatworktoken` header) and is already the worked example in the §13 parser tests and
the cookbook integration article (906b702) — this ticket turns the example into a shipped,
installable declaration asset (like `cloudflare.qfs`) and thereby doubles as the mission's
**API-key-style "rewrite drivers by qfs query" proof**: a full service surface, including binary
file transfer, expressed in CREATE DRIVER / TYPE / VIEW / MAP statements with zero new compiled
code (or with the smallest possible compiled assist if declared MAPs cannot yet express multipart
upload — in which case that gap is the recorded finding).

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout (applies to all code work)
- `workaholic:implementation` / `policies/coding-standards.md` — style conventions (applies to all code work)
- `workaholic:design` / `policies/vendor-neutrality.md` — a new external API enters only behind the declared-driver seam, host-confined to `/http/chatwork/`
- `workaholic:design` / `policies/email-sending-restraint.md` — Chatwork message posting is an outbound communication effect; each write MAP needs deliberate justification and the send gate
- `workaholic:safety` / `policies/standard.md` — the API token connects via `SECRET 'vault:…'` reference only; no clause carries a secret value

## Key Files

- `packages/qfs/crates/skill/assets/examples/cloudflare.qfs` - the reference declared-driver definition this asset copies (CREATE DRIVER … AT … AUTH; CREATE TYPE; CREATE VIEW … AS /http/… |> DECODE json |> EXPAND; CREATE MAP INSERT)
- `packages/qfs/crates/qfs/src/declared_driver.rs` - the declared-driver evaluator (loads /sys/drivers rows into RestApiConfig; compiled ∪ declared registry)
- `packages/qfs/crates/qfs/src/declared_eval.rs` - {param} view expansion, host confinement, MAP lowering
- `packages/qfs/crates/parser/src/tests.rs` - §13 declared-driver parse tests already using chatwork as the example
- `docs/cookbook/` - the cookbook article carrying the recipes (gen-skills source)

## Related History

Chatwork appeared as a cookbook integration example and as the parser-test worked example; Cloudflare is the shipped precedent of an API-key declared driver.

- [20260707182610-chatwork-drive-cookbook-integration.md](.workaholic/tickets/archive/work-20260707-180555/20260707182610-chatwork-drive-cookbook-integration.md) - Chatwork declared-driver cookbook example (find room, read latest, cross to Slack/Drive)
- [20260708023259-cloudflare-declared-driver-query-based.md](.workaholic/tickets/archive/work-20260707-181519/20260708023259-cloudflare-declared-driver-query-based.md) - shipped query-based Cloudflare driver, the declared API-key precedent
- [20260704145138-driver-conformance-and-first-conversion.md](.workaholic/tickets/archive/work-20260705-032203/20260704145138-driver-conformance-and-first-conversion.md) - declared-driver conformance ratchet + first compiled→script conversion

## Implementation Steps

1. Author `chatwork.qfs` as a shipped declaration asset: DRIVER (base URL `https://api.chatwork.com/v2`, AUTH HEADER `x-chatworktoken`), TYPEs (`chatwork/room`, `chatwork/message`, `chatwork/file`), VIEWs for rooms / room messages / room files (with `{room_id}` params), MAPs for message post and file endpoints.
2. Probe the declared MAP surface against Chatwork file semantics: download is a two-step (file metadata → `download_url`), upload is multipart `POST /rooms/{id}/files`. Where the declared model cannot express a step (multipart encode, follow-URL fetch), record the exact missing primitive and either (a) add that primitive to the declared evaluator as a small generic capability, or (b) document the gap as the ticket's finding — do not hand-wave it.
3. Install-test hermetically: parse-check the asset (cookbook ratchet), load it into /sys/drivers via the existing install path, DESCRIBE shows the declared views credential-free (MockHttpClient).
4. Add cookbook recipes: connect with `SECRET 'vault:chatwork'`, read latest messages, list files, download a file, post a message with a file.
5. Regenerate docs and skills; bump the four plugin version fields if the taught surface changes (CLAUDE.md rule).

## Quality Gate

**Acceptance criteria**

- `chatwork.qfs` parses clean under the cookbook ratchet and installs into /sys/drivers as ordinary previewed writes.
- DESCRIBE over the connected `/chatwork` mount lists rooms/messages/files views without credentials.
- File download and upload are either fully expressed in the declaration, or the missing declared-MAP primitive is precisely recorded (statement shape + evaluator change needed).

**Verification method**

- `cargo test --workspace` green (parser §13 tests, cookbook ratchet, declared evaluator tests); `gen-docs --check` / `gen-skills --check` clean.

**Gate**

- Hermetic suite green is the /drive approval gate. The live round (real Chatwork token: list rooms, download a real file, upload a test file) runs owner-attended and is recorded afterwards.

## Considerations

- If multipart upload forces a compiled assist, keep it a generic declared-driver capability (e.g. `ENCODE multipart`) rather than Chatwork-specific code (`packages/qfs/crates/qfs/src/declared_eval.rs`)
- Compiled drivers win name collisions in the two-source registry — the asset name `chatwork` must not shadow anything compiled (`packages/qfs/crates/qfs/src/declared_driver.rs`)
- This ticket satisfies two mission acceptance items (Chatwork file handling; API-key-style declared rewrite) — archive notes should say so explicitly

## Live Round Evidence

### Round 1 — declared read half (2026-07-12, owner-attended, PASSED)

- **Binary:** qfs 0.0.59 (commit c30fa0a, aarch64 musl), installed from the published GitHub
  Release via `install.sh` (sha256 verified) — the release round-trip itself proven in passing.
- **Re-install:** all 10 statements of the shipped `chatwork.qfs` (driver, 3 types, 3 read views,
  message map, file blob view, multipart files map) each previewed then committed — one reversible
  `/sys/drivers` INSERT per statement, zero network. The pre-existing 6-row stale install remains
  alongside (append semantics); the newest-wins TYPE lookup (PR #34) heals the stale-type case.
- **Live read (owner-approved):** `/chatwork/rooms |> select room_id, name, type, role |> order by
  name` returned **91 rows, every row carrying real values** — the value-less-row defect is gone.
  Typed schema resolved: `room_id int, name text, type text, role text`.
- **Drift note:** the OF contract declares `room_id text PRIMARY KEY`; the live read reported
  `room_id` as `int`. Conformance reconciled it without refusal — acceptable, but worth a look if
  the declared OF/live reconciliation is ever tightened.
- **Operating pattern:** owner unlocked the vault (`qfs auth`, shared 8h session) in a real
  terminal; assistant ran install PREVIEW/COMMIT (local-only) and the live read after an explicit
  owner approval. No third-party row content is reproduced here (count + shape only).
- **Acceptance:** ticks the mission's API-key-style declared rewrite **read half**. The write half
  (file attach/detach via FOLLOW blob view + ENCODE multipart map) is round 10.

### Round 10 — Chatwork file upload + download via the generic primitives (2026-07-13, owner-attended, PASSED)

- **Binary:** qfs 0.0.59 (c30fa0a); target the owner's own マイチャット (room 25496268, type
  `my` — self-visible only), upload explicitly owner-approved.
- **Upload (`ENCODE multipart` map):** `/local/<scratch>/qfs-round10.txt |> select {c: content,
  n: name} as s |> extend file = s.c, filename = s.n, message = 'qfs live round 10 upload'
  |> select file, filename, message |> insert into /chatwork/rooms/25496268/files` — previewed
  (irreversible-gated), committed `affected 1`. (The struct bypass again — ticket
  20260713120000's plan-schema gap applies to any blob source.)
- **Listing read-back:** `file_id 2108337622, filename qfs-round10.txt, filesize 76` — exact.
- **Download (`FOLLOW` blob view):** `/chatwork/rooms/25496268/files/2108337622/blob` returned
  one `content` bytes row whose base64 **matches the source file byte-for-byte** (76 bytes) —
  the two-step metadata→`download_url` cross-host GET worked with no credential forwarded.
- **Ticks acceptance:** Chatwork file handling via the declared driver (upload + download; the
  Chatwork API offers no file delete, so detach is N/A by service design), and — together with
  round 1's read half — the API-key-style declared driver is proven **end-to-end**.
- **Residue:** one 76-byte test file in the owner's マイチャット.
