---
created_at: 2026-07-16T01:15:00+09:00
author: a@qmu.jp
type: housekeeping
layer: [Config]
effort: 2h
commit_hash: d1ccf0e
category: Changed
depends_on:
mission: build-insightbrowser-on-the-plgg-family
---

# Resume: commit the two UI fixes in the tree, then finish deno

## DONE 2026-07-16 — all three items, and one of them was wrong

Outcome, in the order this ticket set:

1. **The five files are committed** (`b271e81`), unchanged, with the drafted
   message. The gate was green without deno on PATH, so they stood on their
   own.
2. **deno is DONE and the mission item is checked (12/17)** — `d1ccf0e`. It
   was **not** "one command": this ticket's prediction that the tsconfig fix
   would cover deno was wrong. deno reads `deno.json`, not `tsconfig.json`,
   and its `register()` is a silent no-op stub, so it had no self-alias
   resolver at all. The real cause was one fact declared three times
   (`bin/hook.mjs` / tsconfig `paths` / nothing); it is `#insightbrowser/*` in
   package.json's `imports` now and `hook.mjs` is deleted.
3. **Pagination is DONE** — `05ec65d` — and it uncovered that item 1 was
   **half a fix**. See below, because it is the part worth reading.

### The correction: the facet-count fix in this ticket was wrong

This ticket's bug 2 says the counts were counted over the wrong set "because
the click ANDs with the filter already on". **The click did not AND.** The
facet links REPLACED the filter (`?type=enhancement` rendered
`/?layer=Config`), so counting over the filtered set made the count disagree
with its own link: `Config (1)` linked to a page answering **4**.

Counting over the INDEX — the thing this ticket called the bug — was
CONSISTENT under replace-semantics: the count was exactly what the click
delivered. So the fix took a coherent design and left it half-way to a
different one.

Resolved by finishing the design the counts were already written for (the
developer chose it): links AND, and an applied value links to its own removal.
Verified live — `Config (1)` now links to a page of 1.

**The lesson this ticket already recorded, earned once more:** its own
symptom ("a facet read `enhancement (5)` beside a list of 3") was not a bug.
Under replace-semantics, 5 is exactly what clicking delivers. The number
beside a facet describes what you would GET, not what is in the list you are
looking at.

Two more, found on the way:

- **`src/domain/model/Index.ts` held a literal NUL byte** (the `errorKey`
  separator), which made it `data` to `file(1)` and therefore BINARY to grep.
  It answered "no match" to every search while looking normal on screen. A
  package-wide rename silently skipped it, and the grep that "verified" the
  rename reported zero. Written `\0` now.
- **The npx smoke was silent on the runtime that failed** — `ACTUAL=$(...
  2>/dev/null)` under `sh -eu` discarded the error and exited at the
  assignment, before its own FAIL line. Fixed first, before the thing it
  measures.
- **The facets counted at most 100 documents** (`maxLimit`), which is right
  here and silently wrong on plgg's 1711. `matchDocuments` is unpaged now.

Still open, carried by `20260715225000`: the five items that need a person,
not a keyboard. **CLAUDE.md's 302-means-healthy rule is still wrong** and is
still not fixed — see Considerations.

## READ THIS FIRST — there is UNCOMMITTED, GREEN work in the working tree

**Carry origin:** the 2026-07-15/16 session on `work-20260715-172000`, carried
at 01:15 because the token window was filling. The mission is at **11/17**;
`./scripts/test-insightbrowser.sh` reports **263 passed**.

Five files are modified and NOT committed. They fix two real bugs the developer
found by opening the live site — do not discard them, and do not start anything
else before they are committed:

```
 M packages/insightbrowser/src/domain/model/Query.ts
 M packages/insightbrowser/src/domain/usecase/tagGroups.spec.ts
 M packages/insightbrowser/src/domain/usecase/tagGroups.ts
 M packages/insightbrowser/src/entrypoints/columns.ts
 M packages/insightbrowser/src/entrypoints/mcpTools.ts
```

### What those five files fix

1. **`?cols=…` was being read as a front-matter FILTER.** `cols` was not in
   `Query.ts`'s `RESERVED` list, and the rule is "any unreserved parameter is a
   front-matter filter". So `?cols=docs/adr/index.md` meant "documents whose
   front matter says `cols: docs/adr/index.md`" — which nothing matches.
   **Opening one column emptied the document list to `0 of 32` and took every
   facet down with it.** `cols` is now reserved.

   This shipped and nobody noticed for a whole session, including me clicking
   through `?cols=` repeatedly: the specs tested `?cols=` and facets
   SEPARATELY, so neither saw the interaction. The developer found it in about
   a minute by opening the page.

2. **The facet counts were counted over the wrong set.** `tagGroupsOf` took the
   `Index` and counted the whole corpus, so under a filter a facet read
   `enhancement (5)` beside a list of 3 — promising five documents that
   clicking it could never produce, because the click ANDs with the filter
   already on. It now takes `ReadonlyArray<Document>` and the corpus column
   passes the FILTERED set (not the index, and not the page — counting the page
   would make the numbers shift when you turn it).

   `mcpTools.ts` changed only to pass `allDocuments(index)`: `list_tag_groups`
   takes no filter, so the whole corpus genuinely is its set.

### First actions, in order

1. `./scripts/check-all.sh` — confirm it still exits 0 (it did at carry time:
   263 tests, node + bun).
2. **Commit those five files.** A message is drafted in `## Draft commit`
   below.
3. Then pick up `## Remaining work`.

## Remaining work

### 1. deno — the item is one command from being verifiable (NOT blocked any more)

**`deno 2.9.2` IS NOW INSTALLED** at `~/.deno/bin/deno`. The developer
explicitly authorised the installer via an AskUserQuestion. Ticket
`20260715225000` says deno is absent and blocked — **that is now false**, and
its line 41 and 114-119 need correcting when you touch it.

The verification was **interrupted mid-run** (the developer stopped the tool
call to go look at the site) and has never completed. It is one command:

```sh
export PATH="$HOME/.deno/bin:$PATH"
./scripts/check-all.sh          # the smoke now runs every runtime it finds
```

The smoke prints `PASS: node`, `PASS: bun` and — if the tsconfig fix covers
deno as predicted — `PASS: deno`. **If it passes, the mission's node/bun/deno
item is met: check it in `mission.md` (12/17).** If it fails, the failure is
the news; do not check the item.

Expect one possible snag: `insightbrowser mcp` loads `plgg-content` →
`node:sqlite`, which deno may not have. The smoke only runs `--version` and
`--help`, and those are lazy since `10ffdaa`, so it should not bite there.

### 2. Pagination — a KNOWN, UNFIXED bug the developer saw

`/` reads `20 of 32 document(s)`. `defaultLimit` is 20 and **the corpus column
offers no way to reach the other 12**. The count is honest and the documents
are unreachable, which is worse than either alone.

`listCollection` already takes `limit`/`offset` and `ListResult` already
carries `totalCount` — so this is a rendering gap, not a model one. The
smallest honest fix is prev/next links in the corpus column that carry the
current `cols` and any facet filters through (`withCols` in `columns.ts`
already does exactly that for facet links — reuse it).

Watch the same trap as bug 1: a paging link must NOT drop `cols` or the facets,
and a spec that tests paging alone will not catch it. Test paging WITH a column
open and a facet applied.

### 3. Everything else needs a person, not a keyboard

`20260715225000` carries them with the evidence for each (each blocker is a
command that was run, not a claim): `OPENAI_API_KEY` is unset, Cloudflare and
AWS both answer no-credentials, WebMCP has nothing in this environment to
implement against, and the plgg/qfs docs sites are those repositories' adoption
work. **Correct its deno rows** when you next open it.

## Draft commit

For the five files in the tree:

```
Stop reading `?cols=` as a filter, and count facets over the filtered set

Two bugs the developer found by opening the live site, in about a minute,
after a session of green gates.

`cols` was not in Query.ts's RESERVED list, and the rule there is that any
unreserved parameter is a front-matter filter. So `?cols=docs/adr/index.md`
meant "documents whose front matter says cols: docs/adr/index.md", which
nothing matches: opening a single column emptied the document list to `0 of
32` and took every facet with it. I had been clicking `?cols=` all session
without noticing the list behind it was empty. The specs tested `?cols=` and
facets separately, so neither saw the interaction.

The unreserved-means-filter rule is what makes `?type=bugfix` work with no
wiring, and this is its cost: every parameter a surface adds must be declared
reserved or it silently becomes a filter for a key no document has. The list
is one place so that cost is payable in one place.

The facet counts were also counted over the wrong set. `tagGroupsOf` took the
Index, so under a filter a facet read `enhancement (5)` beside a list of 3 --
promising five documents the click could never produce, because it ANDs with
the filter already on. It takes documents now, and the corpus column passes
the FILTERED set: not the index (the bug), and not the page (a subtler version
of it -- the numbers would shift when you turned the page).

Verified live at localhost:4100: `?cols=docs/adr/index.md` is 20 of 32 again,
and `?cols=…&type=enhancement` is 5 of 32.

263 tests pass.
```

## Considerations

- **The dev workload is RUNNING** on port 4100 (`podman compose -f
  workloads/development/compose.yaml`), bind-mounting this worktree, published
  at `insight-browser.qmu.dev` behind Cloudflare Access. It reads TS at boot,
  so **restart it after a source change** — `podman compose -f
  workloads/development/compose.yaml restart` — a bind mount does not reload
  the code, only the corpus.
- **CLAUDE.md's health rule is WRONG and is not yet fixed.** It says a 302 from
  `insight-browser.qmu.dev` is healthy and a 502 means the workload is down.
  Measured this session: the workload was completely stopped and the tunnel
  still answered **302**, because that is Cloudflare Access's login redirect,
  returned before the tunnel reaches the origin. A 302 proves Access is in
  front, nothing more. Fix the rule or someone will read a dead workload as
  healthy.
- **ADR 0005's first retirement date is 2026-07-22 21:09 JST** — the
  `NPM_CONFIG_MIN_RELEASE_AGE=0` override in `scripts/smoke-npx.sh`. It has
  moved once already (adopting plgg-md 0.0.3 restarted the clock); the ADR says
  when to stop extending rather than keep bumping it.
- **A `/request` was filed to plgg tonight** —
  `20260716000445-plgg-mcp-exports-drag-in-plgg-content.md`, uncommitted there
  by design. It asks plgg-mcp to split its `exports` so the protocol core does
  not drag in `plgg-content`/`node:sqlite`, which is what makes
  `insightbrowser mcp` node-only.
- **The branch story is written** (`.workaholic/stories/work-20260715-172000.md`)
  and the branch has no PR. It is one `/report` from one.
- **The lesson this session kept teaching**: I called four things impossible and
  was wrong four times, every time by examining the thing that was stuck rather
  than the thing holding it. And both bugs above were found by a person opening
  a page, not by 263 tests.
