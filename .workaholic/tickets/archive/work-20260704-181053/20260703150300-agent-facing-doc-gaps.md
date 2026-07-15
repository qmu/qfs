---
created_at: 2026-07-03T15:03:00+09:00
author: a@qmu.jp
type: enhancement
layer: [UX]
effort: 1h
commit_hash: e779763
category: Added
depends_on: []
---

# Agent-facing gaps: boolean literals; the result envelope (JSON shapes) per blueprint §14

From the skills-only agent run and the live parity check (2026-07-03, v0.0.17):

1. **Boolean literal predicate unsupported**: `where is_google_doc == true` fails with
   `unsupported predicate: expected a literal, found column 'true'` — the grammar has no bool
   literal (or the pushdown rejects it). The schema advertises Bool columns, so agents will
   write exactly this. (Workaround today: `mime_type LIKE 'application/vnd.google-apps%'`.)
2. **`--json` output shapes are undocumented**: `date` is epoch milliseconds, a blob `content`
   is a JSON array of byte values (a 758KB file becomes ~2.5MB of JSON), and a destructive
   PREVIEW exits 4 with a `commit_required` error alongside the preview. None of this is in the
   cookbooks/skills; an agent has to discover it. Document the shapes (and consider base64 for
   `content` in --json as a follow-up decision).
3. **Preview does not validate**: two recipes previewed `affected 1` and failed only at commit
   (see 20260703150000/20260703150100). Once those are fixed, add the honest note that PREVIEW
   is a plan projection, not an apply dry-run, wherever the cookbook says "safely watch what it
   would do".

## Absorbed scope (2026-07-04): the §14 result envelope

Blueprint §14 (the console face, approved) upgrades item 2 from "document the shapes" to
**implement the stable, schema-carrying result envelope**: the `--json` / HTTP result becomes
rows + the typed schema (§5 types when declared) + honest execution metadata (affected counts,
truncation/limit flags). It is also the **server↔console pairing contract** the plgg console
decodes (`cast` on the envelope — the §5 plgg bridge). Design the shape once, implement it on
the one-shot `--json` path and the endpoint face, then document it in the cookbooks/skills
(item 2's original ask). Whether the envelope joins the §12 versioned surface is decided here,
with the first console as the evidence. The base64-for-blob-content decision folds in.

## Settled envelope design (2026-07-04, owner-approved; recorded in blueprint §14)

```json
{
  "schema": [ {"name":"date","type":"timestamp"}, {"name":"content","type":"bytes"} ],
  "rows":   [ {"date":1751600000000, "content":"aGVsbG8gcWZzCg=="} ],
  "meta":   {"row_count":1, "truncated":false, "limit":null, "offset":null, "affected":null}
}
```

- `rows` stays an array of **objects** (agent-native; the console's plgg `cast` decodes objects;
  one shape, no negotiated variants).
- `schema` is **always present**, in column order (fixes JSON's lost column order): name + §5
  type token when known, `"unknown"` honestly otherwise. Rendered from `qfs_types::Schema` —
  one source of truth.
- `meta` carries honest execution fact: `row_count`; `truncated` + the bound that cut
  (`limit`/`offset` — the exact vocabulary endpoint paging reuses, ticket 20260704152639);
  `affected` non-null only when effects ran.
- Encodings are schema-discoverable and documented: `timestamp` = epoch-ms UTC (**kept**);
  `bytes` = **base64** (owner-approved hard break from the byte-array rendering; goldens
  re-blessed).
- **One serializer, three faces**: `qfs run --json`, the HTTP endpoint face, and the MCP result
  payload all emit this envelope — implement once behind the `Renderer`/`RowSet` seam
  (`crates/exec/src/{dto.rs,output.rs}`).
- `{"error":…}` and `{"preview":…,"committed":…}` shapes are **unchanged**; exit codes get
  documented in the cookbook.
- §12 versioned-surface membership stays **deferred** per blueprint §14 ("decided once the first
  console consumes it") — mark the envelope "stabilizing" in the docs.

## Key files

- `packages/qfs/crates/parser/` (bool literal), `docs/cookbook/*.md` + `docs/guide/cli.md`
  (--json shapes, exit codes), gen-skills regeneration.
- `packages/qfs/crates/exec/src/{dto.rs,output.rs}` — the `RowSet`/`Renderer` seam the envelope
  lands behind (one serializer for all three faces).

## Quality Gate

- `where <bool-col> == true` parses and pushes down (cookbook ratchet covers it).
- The result envelope is one documented shape (rows + typed schema + affected/truncation
  metadata) emitted by both `qfs run --json` and the endpoint face, proven by hermetic tests;
  the versioned-surface decision is recorded in blueprint §14 when it lands.
- The cookbooks state the envelope, date/content encodings, and exit codes; skills regenerated;
  `gen-docs --check` / `gen-skills --check` in sync.
