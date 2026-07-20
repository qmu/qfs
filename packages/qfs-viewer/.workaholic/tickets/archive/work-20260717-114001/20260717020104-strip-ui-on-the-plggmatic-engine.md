---
created_at: 2026-07-17T02:01:04+09:00
author: a@qmu.jp
type: enhancement
layer: [UX, Domain, Config]
effort: 4h
commit_hash:
category: Changed
depends_on: [20260717020103-resolve-addresses-with-prefix-closure.md]
mission: qfs-viewer-mvp
---

# The horizontal strip UI on the ported plggmatic engine (with the ADR 0002 amendment)

## Overview

Mission acceptance item 1 (demo legs 2–3). Replace the hand-built column UI
with the plggmatic engine's column strip — column strip, recursive trail,
static column headers, depth never consuming the viewport (the measured
reference shape, `docs/plggmatic-semantics/poc-findings.md`). The default
describe lowering (already landed as view data) becomes a Declaration → Scene
feed: one deterministic manifest generator on the ONE pipeline that richer
manifests and, later, an LLM will feed.

## The blocking decision this ticket owns

`packages/qfs-viewer` may not name plggmatic today (ADR 0002 + the
dependency-contract gate), and there is a REAL packaging wall behind the
paper one, found 2026-07-17: the registry's `plggmatic` (0.1.0, published
2026-07-04) predates the engine port — it is the old facade — while the
ported engine in `packages/plggmatic` is unpublished. A `file:` sibling
dependency breaks the npx smoke (the packed tarball cannot resolve
`file:../plggmatic` in a consumer's tree), and executing the bin from `src`
means consumers resolve the dependency from the registry, not from this
repo. So the honest sequence is:

1. Publish the ported engine from this repository (a version above 0.1.0;
   needs the developer's npm credentials — route via HQ/developer), wait out
   or lift min-release-age per ADR 0005's rules.
2. Amend ADR 0002: plggmatic becomes this package's UI engine per the plan
   and the mission — an explicit amendment, not a silent gate change.
3. Update `scripts/dependency-contract.mjs` + `gate-dependencies.sh` to
   accept `plggmatic` as a plgg-family runtime dep (registry `^version`).
4. Re-render the trail's columns as the engine's strip; the hand-built
   column renderer retires.

## Policies

- `workaholic:design` / `policies/sacrificial-architecture.md` — the UI is
  one skin over one Scene; the hand-built renderer was the sacrificial
  first skin and this ticket is its planned death.
- `workaholic:implementation` / `policies/objective-documentation.md` — the
  ADR amendment and the gate change are one decision recorded with its
  reasoning, never a silent reversal (ADR 0002 names this exact risk).
- `workaholic:design` / `policies/modeless-design.md` — the strip renders
  the same address; mode (hand-built vs engine) changes nothing the URL
  holds.

## Progress (2026-07-17, branch work-20260717-094501)

Steps 2 and 3 of the sequence LANDED ahead of the publish, deliberately —
they are publish-independent and make the eventual dependency flip a
two-line change:

- ADR 0002 carries the second amendment: plggmatic becomes this package's
  UI engine, with the full publish-first sequence recorded (an explicit
  amendment, not a silent gate change).
- `scripts/dependency-contract.mjs` + `gate-dependencies.sh` accept
  `plggmatic` as a plgg-family runtime dep; every other non-plgg dep stays
  rejected, self-tested both ways each run. CLAUDE.md / README / ADR index
  updated to match.
- The packaging wall re-verified harder than found: the registry
  `plggmatic@0.1.0` tarball contains NO code at all — `package.json` and
  `README.md` only; the `dist/` its manifest points at is absent.

Step 1 (publish the ported engine above 0.1.0) is BLOCKED in the work
environment: `npm whoami` answers 401 — the developer's npm credentials are
required, and publishing is an irreversible external operation reserved for
the developer. Routed via HQ per this ticket's own instruction.

Step 4 (declare the registry `^version`, re-render the trail's columns as
the engine strip, retire the hand-built renderer) waits on step 1 — any
premature `file:`/unpublished declaration turns the npx smoke red, which is
the check keeping the sequence honest. Ticket 20260717020103 has landed
(merged 2026-07-17, PR #7): `/resolve/<trail>` is now the canonical
address, so the strip re-render targets `/resolve` directly.

## Completed (2026-07-17, branch work-20260717-114001)

Step 1 landed: the developer published **plggmatic@0.2.0** (the registry
tarball verified real — full `dist`, typed `.`/`./style` exports). Steps
2–4 followed on this branch:

- `packages/qfs-viewer` declares `plggmatic: ^0.2.0` from the registry.
- `/` and `/resolve` render the ENGINE strip: engine `row`/`column`/panes,
  the engine's sticky `colHead` on every column (title = the collapse
  link, the edit link rides the header), the engine's
  scheme/metric/chrome CSS with the `html.dark` appearance bootstrap, and
  the breadcrumb rail folded by the engine's `crumbsOf` from the ONE
  Scene the trail lowers to (`domain/usecase/scene.ts` — corpus →
  MenuLevel, document → DetailLevel, describe default view → ListLevel
  whose rows are exactly the containment links).
- The hand-built renderer retired: the `.columns`/`.column` shell, its h2
  headers, and `domain/model/Palette.ts` + spec are deleted; the engine
  theme is the one color vocabulary.
- PoC depth measurement re-run against the served page (headless
  chromium): 9 columns deep — body scrollWidth stays 1280 at 1280×800
  while `.pm-row` scrolls 4457px internally; in the PoC's 420×640 frame,
  body 420 vs row 4877. Depth never consumes the viewport; `pm-colhead`
  computes `position: sticky`.
- Verification: `scene.spec.ts` (the Scene lowering, incl. the engine
  `crumbsOf` round-trip) + a strip spec pinning the engine shell;
  `./scripts/check-all.sh` exits 0 — the npx smoke resolves the PUBLISHED
  engine from the registry in a scratch consumer under node, bun, and
  deno (scoped ADR-0005 override until 0.2.0 clears the floor
  2026-07-24).

## Quality Gate

- Acceptance: `/` and `/resolve` render the engine strip (static headers,
  internal horizontal scroll, 8+ columns leaving body width constant);
  ADR 0002 carries the amendment; the dependency gate accepts plggmatic and
  still rejects every other non-plgg dep.
- Verification: unit specs over the Scene lowering; the PoC's depth
  measurement re-run against the served page; the npx smoke proves a
  registry consumer resolves the published engine.
- Gate: `./scripts/check-all.sh` exits 0.
