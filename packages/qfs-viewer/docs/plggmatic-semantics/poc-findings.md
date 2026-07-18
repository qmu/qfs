> Imported from qmu/plggmatic on 2026-07-16 by HQ triage (strategy mission qfs-viewer-mvp-headquarters); original path: none — this file summarizes live PoC findings from the plggmatic MCP-App labs work.

# plggmatic PoC findings — the two live results

Two findings from driving the plggmatic UI inside a real bounded host
frame, recorded here because they shape the qfs-viewer MVP directly.

## 1. The horizontal strip fits a bounded host frame — structurally

The horizontal column strip fits a bounded 420×640 host frame
**structurally**: with the trail 8 columns deep, the strip scrolls
internally and the body width stays constant. The unbounded-growth
model and the bounded box are not in conflict — the strip owns its own
horizontal scrolling, and the frame never needs to grow.

## 2. URL-as-truth fails inside host frames — plgg-view needs a virtual-URL mode

URL-as-truth **fails** inside host frames: `pushState` throws a
`SecurityError` in the sandboxed frame. plgg-view therefore needs a
**virtual-URL mode** — the codecs are kept as-is, links are
intercepted, and the URL becomes an in-memory value rather than the
address bar's. The URL remains the single source of truth as a *value*;
only its storage location changes when there is no address bar to hold
it.
