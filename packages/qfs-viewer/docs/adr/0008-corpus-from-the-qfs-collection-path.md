# 0008 — The corpus is served from qfs's markdown collection path; the in-process indexer retires

**Status:** Accepted (2026-07-17)
**Ticket:** 20260717020105-markdown-browsing-over-the-qfs-collection-path.md
**Mission:** qfs-viewer-mvp (acceptance item 3, demo leg 2)

## Decision

When `qfs-viewer.config.json` declares a collection —

```json
{ "collection": "strategy" }
```

— the corpus is read from **qfs's markdown collection path**: the two
relational tables a `CONNECT /markdown/<name> TO markdown AT '<root>'`
binding resolves,

```
/markdown/<name>/documents    path, title, frontmatter (Json | NULL)
/markdown/<name>/links        source_doc, source_section_path, target,
                              target_doc (| NULL), line
```

read **per request** through the same `ResourceRunner` seam every other qfs
answer travels. On this arm the in-process indexer — the tree walk, the
fence parsing, the watcher — is **never constructed**: enumeration and
front-matter interpretation are qfs's alone, and every document column
additionally shows the `links` table's rows for that document (the sideways
walk: document → its links → target document), with `source_section_path`
rendered as each link's section context.

`collection` **is the one switch** between the two corpus sources, on both
composition roots (`serve`, `mcp`). Absent, the legacy scan-and-watch
serves exactly as before.

**Retirement date: 2026-07-31.** The legacy scan arm (walk + fence parse +
watcher) is kept verbatim until then so `npx qfs-viewer serve` keeps
working at a repository with no qfs binding, and is then **deleted**, not
kept as a fallback — from that date a corpus is either a declared
collection or the describe-generic browsing, and no second enumerator
exists to drift from qfs's.

## Reasoning

- **One source of truth (`workaholic:design` / data-sovereignty).** The
  ticket's own words: the old indexer "must not survive as a parallel
  source of truth beside qfs's collection — that drift is exactly what
  qfs-mandatory exists to kill." Two enumerators (a walk and a table) and
  two front-matter readers (plgg-md's subset and qfs's parser) WILL
  disagree — about pruning, about dotfiles, about a fence the subset
  declines — and every disagreement is a document that exists on one
  surface and not another.
- **Per request, not snapshot-and-watch (ADR 0003).** The index was never a
  cache only because the watcher kept it honest. A qfs-backed snapshot
  would need a second invalidation channel qfs does not offer — so the
  honest shape is the one every other qfs surface already has: read when
  asked. The qfs markdown driver is itself a read-through tree walk, so a
  fresh read sees an edit immediately; the editor's post-save swap becomes
  a no-op by construction.
- **The path spelling is verified, not assumed.** qfs #7 (2026-07-17)
  canonicalized the hosts realm — `/hosts/<host>/claude`, bare `/claude`
  retired — the same day this landed. `/markdown` is **not** host-realm-only:
  qfs main's generated `docs/drivers.md` teaches `/markdown/<name>/…` bare,
  so that is what this viewer speaks. If qfs ever retires the bare
  spelling, its structured `retired_path` error names the canonical form on
  the corpus column itself.
- **Bodies come off the disk, and that is not a second collector.** The
  collection path serves no body column — it enumerates and interprets, it
  does not transport bytes. Rendering therefore reads the file the TABLE
  named: a point read of a validated `DocumentPath`, through the same
  `FileSystem` seam the editor writes through — no directory enumeration,
  no fence parse, no pruning rule, nothing that could hold an opinion about
  what the corpus is.
- **No reader-controlled value ever enters a statement.** Both statements
  are constants of the validated tree name (`asCollectionName`, one
  path-segment charset); per-document narrowing (the links of ONE document)
  happens on the answer, so there is no quoting rule to get wrong.
- **Why a switch at all, and why dated.** The operation policy
  (`continuous-deployment`) demands every commit keep `npx qfs-viewer
  serve` working; a repository without a qfs binding still needs a corpus
  today. Coexistence is allowed "only behind one switch with a recorded
  retirement date" — this document is that record, and the date is chosen
  one week out, matching ADR 0005's stance that a time-boxed bridge needs a
  written end.
