---
created_at: 2026-07-11T12:15:30+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain]
effort:
commit_hash:
category:
depends_on: [20260711121529-live-model-providers-anthropic-openai-google.md]
mission: qfs-capability-tryout-file-handling-transformation-and-platform-hardening
---

# PDF → text → Google Drive in one transform pipeline

## Overview

Prove the Extraction cardinality mode on its flagship case: a single statement that reads a PDF's
bytes (from `/local`, `/drive`, or a `/mail` attachment), runs it through a transform whose INPUT
is one bytes column (derive_mode → Extraction: blob in, structured rows out), and upserts the
extracted text into Google Drive. The mode plumbing shipped with the epic; what's unproven is the
bytes-to-provider leg (PDF documents as model input — each provider has a document/file input
form) and the end-to-end composition with a Drive write. Land the hermetic proof and the taught
recipe; the live run (real PDF, real key, real Drive) is the mission's acceptance evidence.

## Policies

- `workaholic:implementation` / `policies/directory-structure.md` — conventional project layout (applies to all code work)
- `workaholic:implementation` / `policies/coding-standards.md` — style conventions (applies to all code work)
- `workaholic:implementation` / `policies/type-driven-design.md` — the OUTPUT schema (e.g. `{page int, text string}`) is the contract; extraction quality is judged against it, not free text
- `workaholic:design` / `policies/data-sovereignty.md` — the PDF's content transits to the chosen model provider; the recipe must say so plainly (informed egress)

## Key Files

- `packages/qfs/crates/types/src/transform.rs` - derive_mode: single bytes column → Extraction
- `packages/qfs/crates/qfs/src/transform.rs` - executor; where PDF bytes get encoded into the provider's document-input form
- `packages/qfs/crates/driver-gdrive/src/effect.rs` - Drive upload the extracted text lands through
- `packages/qfs/crates/exec/src/lib.rs` - materialize_pipeline_source (read → transform → write in one statement)

## Related History

Extraction mode shipped as designed but has never carried a real document; the cross-service write channel is proven.

- [20260708192731-transform-plan-spine.md](.workaholic/tickets/archive/work-20260709-023822/20260708192731-transform-plan-spine.md) - OUTPUT schema fold exposing extracted rows downstream
- [20260708192732-transform-execution-routing.md](.workaholic/tickets/archive/work-20260709-023822/20260708192732-transform-execution-routing.md) - Extraction as one of the three derived modes

## Implementation Steps

1. Wire document input per provider: Anthropic document content block (base64 PDF), OpenAI file/input_file part, Gemini inlineData — behind the same ModelRequest, chosen by the def's provider.
2. Spec by example: `/local/report.pdf |> transform pdf_text |> upsert into /drive/<folder>` (exact grammar per shipped surface); fix composition gaps at PREVIEW.
3. Hermetic test: fixture PDF bytes → mock provider returning schema rows → assert Drive effect carries the extracted text; size-limit behavior (provider byte caps) fails structured at PREVIEW where knowable.
4. Cookbook recipe with the egress note (which provider sees the document); regenerate docs/skills.

## Quality Gate

**Acceptance criteria**

- The one-statement PDF→text→Drive pipeline commits hermetically: fixture bytes in, OUTPUT-schema rows out, Drive write carrying them.
- Oversized input fails with a structured pre-network error when the cap is known.

**Verification method**

- `cargo test --workspace` green including the new Extraction end-to-end test; `gen-docs --check` / `gen-skills --check` clean.

**Gate**

- Hermetic suite green is the /drive approval gate; the live round (a real PDF through a real provider into real Drive, then read back) runs owner-attended and is recorded on this ticket.

## Considerations

- Provider PDF size caps differ (single-digit MB) — surface the limit in the error, don't discover it at commit (`packages/qfs/crates/qfs/src/transform.rs`)
- Long PDFs may exceed one call; chunking is out of scope here — record it as the follow-up if the live round hits it

## Live Round Evidence

### Round 5 — real PDF × provider key × Drive write, read back (2026-07-13, owner-attended, PASSED with findings)

- **Binary:** qfs 0.0.59 (c30fa0a); provider anthropic / claude-haiku-4-5-20251001 / effort low /
  `secret 'env:ANTHROPIC_API_KEY'` (transform `extractpdf2`, input `(content bytes)`, output
  `(name text, mime_type text, bytes text)`).
- **Source:** a real PDF from the owner's own Drive (342,628 bytes), first copied
  Drive→local through the qfs shell `cp` (a live cross-service copy in passing).
- **The statement (one query, committed):** `/local/<scratch>/llm.pdf |> select {c: content} as s
  |> extend content = s.c |> select content |> transform extractpdf2 |> insert into
  /drive/my/qfs-extract-test` — `CALL transform.extractpdf2 [affected 1] (!)` then
  `INSERT drive [affected 1]`. Preview was model-free; the model ran once at commit.
- **Read-back:** the folder lists one new file with this round's timestamp, **named by the model
  from the PDF's internal Title** (`Fundamental LLM model comparison.pdf`) — direct proof the
  provider received and read the real document. Output bytes: 100 (thin extraction at effort
  low — capability proof, not quality proof). Byte-level content read-back was blocked by the
  addressing defects below; the owner can eyeball the file in the Drive UI.
- **Why the struct bypass:** the taught recipe (`/local/report.pdf |> transform extract`) refuses
  at plan — single-file blob nodes omit `content` from the plan schema
  (`TransformInputMissing` / `UnknownColumn`), while the struct constructor path plans and
  commits. Ticketed 20260713120000 (the hermetic test had faked the source schema).
- **Two more live defects, both ticketed:** Drive UPDATE on a folder path silently drops WHERE
  and renamed the folder itself (20260713120100, restored by a path-addressed rename); the
  documented `/drive/id:<id>` addressing is invalid_path and space-named files are wholly
  unaddressable (20260713120200).
- **Residue:** `/drive/my/qfs-extract-test` with the 100-byte extracted file; transform
  definitions `extractpdf` (unused, input blob) and `extractpdf2` (`remove transform <name>`
  drops them).
