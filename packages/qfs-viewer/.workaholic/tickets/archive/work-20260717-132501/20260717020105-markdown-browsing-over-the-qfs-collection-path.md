---
created_at: 2026-07-17T02:01:05+09:00
author: a@qmu.jp
type: enhancement
layer: [Domain, UX]
effort: 4h
commit_hash:
category: Changed
depends_on: [20260717020104-strip-ui-on-the-plggmatic-engine.md]
mission: qfs-viewer-mvp
---

# Markdown browsing over qfs's マークダウン収集パス, retiring the in-process indexer

## Overview

Mission acceptance item 3 (demo leg 2). Pointed at a repository, the viewer
consumes qfs's markdown collection path — the `documents` / `links` tables
(the latter carrying `source_section_path`) that the qfs-side mission
`markdown-trees-are-queryable-as-documents-and-links-tables` provides — and
walks the strip sideways: document → its links → target document. The old
in-process markdown indexer (scanner, front-matter index, watcher) retires
INTO that path: it must not survive as a parallel source of truth beside
qfs's collection — that drift is exactly what qfs-mandatory exists to kill.

## Blocked on

The qfs-side mission landing the `documents`/`links` tables in a released
qfs binary. Until then this ticket must not start; the describe-generic
view already makes the raw files browsable, which is the honest interim.

## Policies

- `workaholic:design` / `policies/data-sovereignty.md` — collection and
  interpretation live in one place (a qfs path); the viewer reads, it does
  not re-collect.
- `workaholic:implementation` / `policies/anti-corruption-structure.md` —
  the documents/links tables arrive through the same `ResourceRunner` seam;
  no markdown-specific transport appears.
- `workaholic:operation` / `policies/continuous-deployment.md` — the
  retirement lands in slices that keep `npx qfs-viewer serve` working at
  every commit (index-backed and qfs-backed reads may coexist briefly, but
  only behind one switch with a recorded retirement date).

## Completed (2026-07-17, branch work-20260717-132501)

The "Blocked on" condition cleared: qfs merged the collection path (PR #6,
`/markdown/<name>/{documents,links}`), and the same day's PR #7 canonicalized
the **hosts** realm only — `/markdown` is not host-realm-only, so qfs main's
generated `docs/drivers.md` teaches the bare `/markdown/<name>/…` as
canonical, and that is what this viewer speaks. Verified against qfs main
rather than assumed; `docs/adr/0008` records the check.

- **The switch**: `qfs-viewer.config.json`'s new `collection` key names the
  qfs markdown tree. Declared, the corpus is enumerated and interpreted by
  qfs alone; absent, the legacy scan serves unchanged. One switch, both
  composition roots (`serve`, `mcp`), **retirement date recorded:
  2026-07-31** (docs/adr/0008) — the coexistence the operation policy
  allows, with the written end it demands.
- **The retirement is structural, not nominal.** On the collection arm the
  scanner, the fence parser and the watcher are *never constructed*. The
  spec proves it by construction: the collection reads run against a
  filesystem whose `readDirectory`/`isDirectory` **throw**, so any walk
  explodes the test. The parallel-truth proof is its twin — a file whose
  fence says `type: from-the-fence` while the table says
  `type: from-the-table` facets on the TABLE.
- `domain/model/Collection.ts` (statements + typed reading of both tables)
  and `domain/usecase/collection.ts` (the corpus, read per request). Both
  arrive through the PR #5 `ResourceRunner` seam — no markdown-specific
  transport, per anti-corruption-structure.
- **Front matter is now folded plain data** (`Document.FrontMatter`), the
  shape both producers share: the scan folds through plgg-md's `foldYaml` at
  the read boundary, the table's `frontmatter` JSON column already is it.
  `YamlMap` no longer leaks into facets, filters, REST or MCP.
- **The sideways walk**: a document column renders its `links` table rows —
  `source_section_path` shown as each link's section context, an internal
  target as a next-column strip segment, an external one as a plain anchor,
  a root-escaping one as inert text (a column cannot open what the corpus
  does not hold).
- Bodies are read point-wise by the path the TABLE named, through the same
  `FileSystem` seam the editor writes through — the collection path serves
  no body column. Not a second collector: no enumeration, no fence parse,
  no pruning rule.
- No reader-controlled value ever reaches a statement: both statements are
  constants of a charset-validated tree name (`asCollectionName`), and the
  per-document narrowing happens on the answer.
- Verification: 359 unit tests pass (`Collection.ts` and `collection.ts` at
  100% coverage); `./scripts/check-all.sh` **exits 0**.
- **Live run at the qmu/strategy root** (qfs 0.0.75, isolated store via
  `XDG_CONFIG_HOME`, `--read-only`, port 4137 so the developer's 4100 was
  never touched; the temporary config was removed on every exit path):
  `CONNECT /markdown/strategy TO markdown AT '…/strategy'` → the tables
  answer **12 documents / 72 links**; the viewer boots
  `{"event":"collection.attached"}` → `{"documents":12,"errors":0}`;
  `/api/health` `{"documentCount":12,"errorCount":0}`; `/` lists all 12;
  `/resolve/CLAUDE.md` renders its Links section
  (`strategy › HQ デスク — ` → `href="/resolve/CLAUDE.md,.workaholic/hq-desk-rules.md"`);
  that address renders **3 columns**; `/api/errors` empty; and **zero
  `scan.*` events** — the scanner never walked.

## Quality Gate

- Acceptance: with a qfs that serves the collection path, `/` browses
  documents and follows links column-by-column with the in-process scanner
  deleted (or verifiably inert); front-matter facets read from the
  `documents` table.
- Verification: unit specs against a fake runner serving documents/links
  fixtures; a live run at the qmu/strategy root.
- Gate: `./scripts/check-all.sh` exits 0.

**Met.** The scanner is verifiably inert on the collection arm (proven by a
throwing filesystem, not by inspection) and dated for deletion rather than
left as a parallel truth; facets read the `documents` table's frontmatter
column; the fake-runner specs and the live strategy-root run are recorded
above; the gate exits 0.
