---
created_at: 2026-07-16T02:50:07+09:00
author: a@qmu.jp
type: enhancement
layer: [UX, Infrastructure]
effort: 4h
commit_hash:
category: Added
depends_on:
mission: build-insightbrowser-on-the-plgg-family
---

# Browser AI over WebMCP against the live corpus

## This item was called impossible. It is not, and the check was in the wrong place

The mission's WebMCP criterion has been carried as blocked since 2026-07-15,
on this reasoning (`20260715225000`):

> The npm `webmcp` package exists but is NOT qmu's (maintainer
> `melvincarvalho`, no repository), so ADR 0001 rules it out. … there is
> nothing in this environment to implement against, and building one would be
> inventing an interface rather than adopting it.

**Every fact in that paragraph is true and the conclusion is still wrong**,
because it answers a question nobody asked. WebMCP is not a package. It is a
**browser platform API**: the page calls `navigator.modelContext.registerTool`
and installs NOTHING. ADR 0001 governs *dependencies*, and this adds **zero**
— so the ADR does not reach it. The registry was the constrained thing; the
browser was the constraining one.

That is the fourth time on this branch that "impossible" was a diagnosis that
had not finished, and this one survived a session BECAUSE it looked
well-evidenced: an npm check, a maintainer name, an ADR citation. The missing
step was cheaper than any of them — asking what WebMCP *is*.

## MEASURED, not assumed (2026-07-16)

Everything below was run, not recalled. My knowledge cuts off January 2026 and
WebMCP landed after it, which is exactly why the earlier session could not
reason its way to this and had to look.

**The spec exists.** WebMCP is a **Draft Community Group Report** of the W3C
**Web Machine Learning Community Group**, published **2026-07-10**
(<https://webmachinelearning.github.io/webmcp/>). It is **NOT a W3C Standard
and NOT on the W3C Standards Track** — build accordingly, and see the risk
below.

**The browser here implements it, unflagged:**

```
~/.cache/ms-playwright/chromium-1228  Chromium 149.0.7827.0  -> navigator.modelContext EXISTS
~/.cache/ms-playwright/chromium-1217  Chromium 147.0.7727.0  -> absent
```

Probed with `chrome --headless --dump-dom` on a local file, no flags, no
origin-trial token:

```
ctor:       "ModelContext"
protoKeys:  ["ontoolchange", "getTools", "registerTool", "constructor"]
registerTool: "function"
```

**So there is a target AND a way to verify against it.** 147 is a free
negative control: the same page must still work there, with the API absent.

### The one trap: the spec and the shipped API DISAGREE today

| | says |
| --- | --- |
| the spec (2026-07-10) | `document.modelContext` |
| Chromium 149 (measured) | `navigator.modelContext` — `document.modelContext` is `undefined` |

Do not pick one from a blog post; both readings are current. **Feature-detect
both** (`document.modelContext ?? navigator.modelContext`) and write down the
date, because this WILL move again — it moved within six days of the draft.

## The real decision: this product ships NO client JavaScript

`entrypoints/columns.ts:7` states the stance, and it is not incidental:

> It is SERVER-RENDERED, with no client JavaScript at all, and that is not a
> limitation — it is what makes the claim true. Every column is a function of
> the URL, so a reload, a bookmark, a pasted link, and `curl` all reconstruct
> the identical screen.

WebMCP needs client JS to call `registerTool`. **Write ADR 0007** (0001-0006
exist) and decide it explicitly rather than by drifting into it.

The proposed shape, to be argued in the ADR rather than assumed here:

- The claim that matters is **"every column is a function of the URL"**, not
  "zero bytes of JS". Progressive enhancement keeps it: SSR output stays
  byte-identical, the script is **additive and non-rendering**, and with JS off
  (or on Chromium 147) the page is exactly what it is today.
- What would BREAK the claim is client-side *rendering* or client-held state —
  which is what `clientEntry` was built for and is precisely what this must not
  use it for. A tool that mutates the page would put state where the URL cannot
  see it.
- So the rule to write down: **the WebMCP script may READ the corpus and
  navigate; it may not render.**

## Key files

- `src/entrypoints/mcpTools.ts` — the tools ALREADY EXIST over `IndexRef`:
  `list_documents`, `get_document`, `list_tag_groups`, `corpus_health`. This
  is the registry to mirror, not to reinvent. Note it is deliberately
  **read-only** (the file says why): an MCP tool that writes before principals
  exist is an unauthenticated write to someone's working tree. **WebMCP tools
  inherit the user's browser session**, which makes that reasoning stronger,
  not weaker.
- `src/entrypoints/serve.ts` / `api.ts` — where a `/webmcp.js` route hangs.
- `src/entrypoints/columns.ts` — the SSR pages that would carry the entry.

### The seam, corrected

`20260715225000` says "`plgg-server`'s `clientEntry` is the seam". Half right,
and the half that is wrong will cost an hour: **`plgg-server` exports no
`clientEntry`** (checked: 115 exports, none of them that). It is an optional
field on the html-document options —

```ts
// plgg-server/dist/View/usecase/htmlDocument.d.ts
export type HtmlDocumentOptions<Msg> = Readonly<{
  title: SoftStr;
  root: Html<Msg>;
  /** When present, a `<script type="module">` boots client-side rendering. */
  clientEntry?: SoftStr;
}>;
```

— which emits `<script type="module" src="…">`. The body is served by
**`javascriptResponse(body, status, headers)`**, which IS a real export. So the
seam is those two together, and neither needs a new dependency.

## Policies

- `workaholic:planning` / `policies/verify-before-building.md` — **the policy
  this ticket exists because of.** The item was ruled out by an npm check that
  answered a question nobody asked; one probe of the actual browser inverted
  the conclusion. Every claim below carries the command that produced it, and
  the implementing session must keep it that way: the spec moves, and a
  recalled fact about a July web platform from a January-cutoff model is not a
  fact.
- `workaholic:planning` / `policies/ai-native-future.md` — this is the mission's
  AI-native surface itself: a corpus reachable by an agent through a declared
  tool contract rather than by scraping a page. It governs what we expose and
  why.
- `workaholic:planning` / `policies/terminology.md` — the WebMCP tool names MUST
  be the MCP tool names (`list_documents`, `get_document`, `list_tag_groups`,
  `corpus_health`). One concept, one word, across TS types, REST paths, MCP,
  and now the browser. A second vocabulary for the same tools would be the
  cheapest possible mistake to avoid.
- `workaholic:implementation` / `policies/directory-structure.md` — universal.
  The client script and its route are `entrypoints/` work; nothing new at top
  level.
- `workaholic:implementation` / `policies/coding-standards.md` — universal. No
  `any`/`as`/`ts-ignore`, and note this code is read back by a *browser* — the
  feature-detect must receive `unknown` at the boundary rather than assert a
  shape the platform may have changed.
- `workaholic:implementation` / `policies/anti-corruption-structure.md` — the
  WebMCP surface is a FOURTH entry point over the same index, and must stay a
  thin shell calling the same public procedures. Its `execute` reaches `/api/*`
  precisely so no query logic is re-implemented client-side.
- `workaholic:implementation` / `policies/objective-documentation.md` — ADR 0007
  is mandatory here, not optional: "does this product ship client JS" is a
  decision with real alternatives and a stated existing position to overturn or
  refine. Record the reasoning, not the outcome.
- `workaholic:design` / `policies/access-control.md` — WebMCP tools inherit the
  user's authenticated browser session. RBAC (principals in
  `qfs-viewer.config.json`, OPEN when none declared) is the only thing
  between a page's tools and a working tree. Read-only stays read-only.
- `workaholic:design` / `policies/interaction-design-standard.md` — the `UX`
  layer. The tools are an interface for an agent, and the same legibility rules
  apply: a tool's description is its affordance.

## Implementation Steps

1. **ADR 0007 first** — the client-JS stance above. Everything else is
   downstream of it, and it is the only irreversible decision here.
2. Serve `/webmcp.js` via `javascriptResponse`, and reference it from the
   corpus page via `clientEntry`. Nothing else changes in the SSR output —
   assert that.
3. Register the four existing tools, feature-detecting `document.` then
   `navigator.`. Each tool's `execute` calls the REST surface (`/api/*`) that
   already serves the same indexed model, so there is one model and no second
   implementation of the query.
4. `annotations: { readOnlyHint: true }` on every tool — true today by
   construction, and the honest signal while the surface stays read-only.

## Quality Gate

### Acceptance Criteria

- ADR 0007 exists and states the client-JS position, with the alternatives it
  rejected.
- On Chromium **149**, the live corpus page registers **four** tools, and their
  names are exactly the MCP ones (`list_documents`, `get_document`,
  `list_tag_groups`, `corpus_health`).
- `execute` on `list_tag_groups` returns the SAME data the REST surface returns
  for the same corpus — not a second answer computed client-side.
- Every registered tool carries `annotations.readOnlyHint === true`.
- On Chromium **147** (API absent) the page still renders and is usable.
- The SSR HTML for `/` is unchanged **except** for the single added
  `<script type="module">` tag.

### Verification Method

```sh
# the API is there / not there — the two controls
~/.cache/ms-playwright/chromium-1228/chrome-linux/chrome \
  --headless --disable-gpu --no-sandbox --dump-dom http://localhost:4100/   # 149: tools register
~/.cache/ms-playwright/chromium-1217/chrome-linux/chrome \
  --headless --disable-gpu --no-sandbox --dump-dom http://localhost:4100/   # 147: page still fine

# the stance, mechanically: the ONLY diff is the script tag
curl -sf localhost:4100/ > /tmp/after.html
diff <(git show HEAD:...) /tmp/after.html   # expect exactly one added <script> line

# the tools answer with the model's own data, not a copy
curl -sf localhost:4100/api/tag-groups      # compare against list_tag_groups' execute
```

The MCP playwright plugin does **not** work here — it wants a Chrome at
`/opt/google/chrome/chrome` which is not installed. Drive the bundled binary
directly, as every measurement in this ticket was.

### Gate

- `./scripts/check-all.sh` exits 0 (268 tests at time of writing).
- Both Chromium probes above behave as stated — **149 registering is not
  enough; 147 must still serve the page.** A pass on 149 alone proves the
  feature and hides the regression.

## Considerations

- **Do NOT add a dependency for this.** The entire point is that the platform
  provides it. A polyfill or an SDK would re-open ADR 0001 for no gain and
  would be the thing that was correctly rejected in the first place.
- **The spec is a Draft CG Report, not a Standard.** It is legitimate to build
  against and legitimate to have it move under us; the ADR should say what we
  do when it does. Record the probe date and the Chromium version next to any
  claim — this ticket's central lesson is that a January-cutoff model cannot
  reason about a July web platform and must look instead.
- **Verification requires a browser that implements it.** We have one. If the
  playwright Chromium is ever pruned to a single version, the negative control
  goes with it — the MCP playwright plugin does NOT help here: it wants a
  Chrome at `/opt/google/chrome/chrome` that is not installed, so drive the
  bundled binary directly (`--headless --dump-dom`), which is how everything
  above was measured.
- **Session inheritance is the security surface.** WebMCP tools run inside the
  user's authenticated session, so RBAC (already built: principals in
  `qfs-viewer.config.json`, OPEN when none declared) is what stands between
  a page's tools and someone's tree. Read-only stays read-only until that is
  reasoned about deliberately.
