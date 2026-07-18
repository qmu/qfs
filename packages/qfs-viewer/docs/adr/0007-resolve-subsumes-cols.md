# 0007 — `/resolve/<trail>` is the canonical address, and it subsumes `?cols=`

**Status:** Accepted (2026-07-17)
**Ticket:** 20260717020103-resolve-addresses-with-prefix-closure.md
**Mission:** qfs-viewer-mvp

## Decision

The column view's truth is a **path under `/resolve`**:

```
/resolve/docs/plan.md                          one document column
/resolve/docs/plan.md,docs/overview.md         …plus the document opened from it
/resolve/qfs:/local/repo,qfs:/local/repo/docs  a qfs containment walk
```

- The address is `/resolve/` + the trail's one serialization
  (`formatTrail`: comma-joined, percent-escaped stops with slashes kept
  readable). **Column i is the resolution of the address's prefix i** — cut
  the address at any comma and what remains is a valid address naming
  exactly the first i columns (接頭辞閉包). A click appends one segment.
- `?cols=` — the first spelling of the same idea — is **subsumed, not kept
  beside**: nothing emits it any more, and a request to `/` still carrying
  it is answered `308` to the canonical `/resolve` address (data filters
  carried along). One serialization stays in circulation.
- The empty trail's one spelling stays `/`; a `/resolve` address whose
  every segment dropped out redirects there.
- **Display state (folding, sort, highlights) has no slot in the grammar.**
  The address is path-only — `trailUrl` emits no query component, and the
  codec reads stops and nothing else. Query parameters on a `/resolve`
  address belong to the corpus column's *data* filters (facets, paging),
  which change what column 0 lists, never which columns the address names.

## Reasoning

The mission (and the plan it defers to, `qmu/strategy` `docs/plan.md`) says
the view's truth is a path under `/resolve`, because a path is the thing a
person can copy, send, bookmark and paste into a fresh session — and because
the resolve path is what later gives the virtual-URL mode (host frames,
where `pushState` throws) its serialization principle.

The choice this ADR actually records is **subsume vs. redirect-to-`?cols=`**,
which the ticket left open:

- Redirecting `/resolve` → `?cols=` would have made `/resolve` a vanity
  entrance: after one click the address shape changes, and "a click is a
  segment appended to the address" — the acceptance item — would be false.
- Keeping both as peers is the forbidden outcome: two driftable
  serializations of one idea (`workaholic:design` /
  `sacrificial-architecture`). So `/resolve` is the address of record,
  `?cols=` is read exactly once per aged bookmark, to walk it forward.

**Why the comma stays the separator** (rather than the plan's
slash-per-step examples): the plan's trail grammar — `@selection` composite
keys, derived reverse edges (`~projects`), even the name `/resolve` itself —
is an open question strategy owns (plan.md 開いた問い), and this repository
must not pre-empt it with local answers. The MVP's stops (document, declared
resource, qfs containment path) are not containment-ordered among
themselves, so a slash-granular prefix would not be closed: `/resolve/docs`
is not a valid trail of `/resolve/docs/plan.md`. At the comma, closure is
exact, the codec is the ONE that already existed, and a document path or a
qfs path stays legible (slashes stay slashes). If strategy later freezes a
slash-segment grammar, it lands as a codec change in `Trail.ts` behind the
same `trailUrl`/`parseResolvePath` seam, and this ADR gets amended.

**Why the trail is parsed from the raw pathname**, not the router's capture:
the router percent-decodes each captured part, and a decode before the split
lets `%2C` in a document name forge the separator. `parseResolvePath` takes
`c.req.path` verbatim so the codec performs the only decode. (This is also
why the address moved OUT of the query string cleanly: `searchParams`
pre-decodes query values, which the old `?cols=` reader then decoded again.)

## Alternatives considered

- **Slash-granular prefixes** (`/resolve/sql/crm/projects` → three columns).
  Rejected for now: prefix closure fails for document stops and inter-stop
  boundaries become ambiguous (a qfs segment can end `.md`); and the
  step-vs-path-segment granularity is part of the grammar strategy owns.
- **`/resolve` renders AND `/?cols=` renders** (no redirect). Rejected: two
  live spellings of every screen is exactly the drift the policy forbids.
- **303 for the legacy walk.** Rejected in favour of 308: the address moved
  permanently; 303 says "see another resource", which is the `/qfs` form's
  situation, not this one. Caching a 308 is harmless here because `noStore`
  stamps every response (docs/adr/0003).

## Consequences

- Every link the view emits — document links, containment rows, facet chips,
  pager, resource list — goes through `trailUrl` and is a `/resolve` address.
- The edit form and the qfs path form still carry the trail as a `cols`
  query VALUE (same `formatTrail` serialization) on their own screens; those
  are parameters of the editor/translator, not view addresses, and their
  redirects land on `/resolve`.
- A corpus directory literally named `resolve/` is shadowed by the route:
  its documents stay indexed, listed and readable via the trail address
  (`/resolve/resolve/x.md`), but not at the bare `/resolve/x.md` document
  page. Accepted as the cost of a short, typable prefix.
- Row SELECTION (`@…`) is still absent on purpose; when strategy freezes the
  grammar, it arrives as new stop kinds in the same codec.
