---
type: Mission
title: A walk extends one trail one column at a time
slug: a-walk-extends-one-trail-one-column-at-a-time
status: active
created_at: 2026-07-21T18:33:32+09:00
author: a@qmu.jp
assignee: a@qmu.jp
strategy: the-viewer-renders-qfs-as-a-walk-over-trails
drive_authorized: true
tickets: []
stories: []
concerns: []
gate_type:
gate_target: blueprint §14b/§14c after the recording tickets land
gate_assert: North star, not a machine check — a reader of docs/blueprint.md finds trail and walk defined as domain terms with their exact distinctions (noun/verb, static/dynamic, result/act, the linearity rule), finds every point the 2026-07-21 design conversation settled written as a ruling rather than an open question, and finds the genuinely-open items still listed as open with their downstream missions named. Verified by reading the shipped blueprint on main, not by any code behavior.
---

# A walk extends one trail one column at a time

## Goal

**This mission is foundation/recording, explicitly not a feature.** The owner chose scope
"(あ): terminology and rulings first" (2026-07-21): a long design conversation about how
qfs-viewer should work converged, and the outcome is to be *recorded* — two domain terms
defined and the design rulings written into blueprint §14b/§14c — not implemented. No viewer
code, no new grammar. Downstream implementation missions are named below but deliberately not
created.

**The prior map this mission tidies — an unmerged blueprint section.** §14c ("The viewer,
reconsidered — the design space (open; nothing settled)") was written earlier in the same
conversation and sits on branch `work-20260721-031401` at commit `d0218aa` — NOT on main. It
is the pre-convergence map of the design space, checked out read-only at
`.worktrees/viewer-reconsideration/`. This mission does not re-derive it; it folds it in and
tidies it (see ## Scope for the fold procedure). The shipped §14b (the address strip, the
trail paragraph, prefix closure) is the base the terms attach to.

### The two domain terms to define (the core deliverable)

- **trail** — a NOUN, STATIC, a RESULT. One written path within the path concept: the
  canonical/backbone address (containment-only) plus segments beyond bare containment —
  selection (`@A`), declared-relation (`/client`), derived-reverse (`~projects`). A trail is
  "where you have walked," recorded. §14b already carries the term; this mission pins it
  precisely against its new counterpart.
- **walk** — a VERB, DYNAMIC, an ACT. The act of extending a trail one step — one column — at
  a time; **the walk produces the trail** (the trail is the walk's trace). Ruled property: **a
  walk is always linear** — a walk traverses exactly ONE trail, never a graph; the
  non-linearity of a DAG never appears in a walk. Sharpened definition (ruling 8 below):
  *"choose one of the steps the current trail admits, and extend"* — for reads, the admitted
  steps are describe's declared relations and keys; for writes, the next input type dependent
  on the values bound so far. One definition covers both.
- **The containment chain to pin alongside:** address/path (canonical backbone) ⊆ trail
  (backbone + relation segments); walk = the act that builds/traverses a trail.

### The rulings this mission records (design settled 2026-07-21; wording and placement are ticket work)

1. **The column-oriented layout is a design pattern that DISPLAYS post-execution semantics** —
   not an isomorphic representation of qfs query semantics. It is simply "display the
   semantics after a query is exercised, in columns." The owner considers this simple and
   settled; it must not be over-abstracted. An earlier "higher-abstracted container /
   design-pattern" framing was explicitly RETRACTED by the owner — that line is dropped from
   §14c wherever it appears.
2. **Linear-vs-graph is dissolved by placement, not by choosing one.** The strip stays linear
   (columns are a linear walk); a DAG — the non-linear define-time structure, e.g. a
   React-Flow-like pipeline editor — lives INSIDE a single column, not across columns.
   Non-linearity is confined to a column's interior; the column sequence stays a one-way
   linear walk. One row can hold: a stored-procedure menu (enumerate) → a DAG editor column
   (define, non-linear, wide) → a preview column → a result column (extension). "Getting into
   the query semantics and out to the exercised result, in the same single row."
3. **definition (intension) vs application (extension) is welded by path = query = set.** A
   path is simultaneously a query (intension; describe = schema/keys/relations) and resolves
   to a set (extension; read = rows); every prefix carries both. A graph view foregrounds
   intension and a strip foregrounds extension, but both render ONE object at two aspects —
   not two artifacts. Caveat recorded with it: the unity is cleanest for reads; writes/effects
   reintroduce a distinct preview/commit aspect (§7).
4. **100% parity between what the query language can express and what the viewer can
   configure is DELIBERATELY GIVEN UP.** The viewer is a faithful representation of the
   subset it covers, not a lossy projection of the whole. Fidelity is the CONTENT's
   responsibility (e.g. the DAG inside a column), not the container's.
5. **The AI-letter concept rides the SAME column UI.** Its qfs mapping is ruled: the letter
   ENVELOPE encloses both bounded context data AND interactivity; it is CONFINED inward — the
   recipient can reference and manipulate ONLY the enclosed context, never reach the sender's
   live world (the SAME confinement principle as declared-driver host-confinement, applied to
   the letter's data scope) — and has a SINGLE typed egress outward: the recipient's reply
   returns as a TYPED value to the sender's inbox (a typed INSERT). The browser-side
   realtime-API AI agent is the "instructor" driving the same column strip via tool calls;
   who drives (human vs AI) is NOT the design axis — a human ultimately instructs either way,
   and the UI must stand on its own without AI.
6. **Interactivity is DERIVED FROM THE TYPE, not a second attribute.** An enum type → choice
   buttons; a struct type → a form; free text → conversation/entry. Reply is a typed INSERT
   into the sender's inbox. The letter's kind fixes the target reply type; the input MODALITY
   (tap/form/voice/free-conversation) is free but must land on that fixed type — free input is
   distilled to the typed target and confirmed before egress ("enter freely, confirm typed,
   exit").
7. **Filling a form is ALSO a walk** (owner chose column-by-column). A struct input is a trail
   of per-field input columns; a partially-filled struct is a valid intermediate value (the
   prefix-closure analogue); ONLY the completed, type-satisfying value COMMITs — the effect
   fires only at the terminal column, matching "no I/O until COMMIT." Read-navigation and
   typed-input thereby become ONE operation vocabulary: "extend the trail one column at a
   time."
8. **condition-split is a distinct, ruled concept — NOT fan-out.** The next step depends on
   the value bound so far (e.g. "reject ⇒ a reason column grows; approve ⇒ it does not").
   This branches the PATH, not the DATA-FLOW; it does NOT make a walk non-linear — the walked
   trail is always one line; only which line is open depends on the trail's contents. It is a
   DECLARED, checkable rule (not existential search), consistent with "declare and reject,
   never guess." This ruling is what sharpens walk's definition to "choose one of the steps
   the current trail admits, and extend."

## Scope

**Done when** every acceptance item below is ticked: the terms are defined in the blueprint,
the rulings are recorded, §14c's open-space map is tidied (settled → ruled; retracted line
dropped; genuinely-open items kept open with downstream missions named), and the result is on
main through the normal topic-branch → PR → /report → /ship cycle.

**How the unmerged §14c is handled — fold, not a separate ship.** The mission's branch folds
commit `d0218aa` (branch `work-20260721-031401`) in as its base — cherry-pick preferred, so
the map's authorship survives — and the tidy edits land on top, one PR carrying map + tidy to
main together. Fallback: if that commit reaches main first (another session ships it), rebase
onto main and tidy in place. Either way §14c is NOT re-derived from scratch, and the
`.worktrees/viewer-reconsideration/` worktree is never written to — it belongs to another
session.

**Where the definitions live.** qfs's design corpus is docs/blueprint.md (owner direction
2026-07-18; there is no separate design-terms file in this repo, verified). The trail/walk
definitions extend §14b's existing trail paragraph — trail already lives there — with walk
defined beside it, and §14c cross-references the pair. The AI-letter rulings (5–8) are
recorded as a subsection of §14c (or a §14c pointer to a sibling subsection), framed as
design rulings, not implementation.

**Out of scope** (deliberately):

- **Any viewer code.** packages/qfs-viewer is untouched.
- **Any grammar.** No ASK, no split, no predicate-segment spelling is implemented or even
  finally spelled; candidate spellings stay candidates.
- **Creating the downstream missions.** They are NAMED in the blueprint text as the open
  items' owners, but not created:
  - the **enumerate-root plumbing** (§14b's own named follow-up) — the qfs-core seam that
    lets a walk drill rightward;
  - the **request-principal seam / "empty home" root** — a fresh, initially-empty personal
    namespace that fills as one connects/declares, vs the union-of-all-drivers root (§14b's
    "viewer's first column — open");
  - an **ASK type INTO path grammar** — a human-supplied-value INSERT whose UI is derived
    from the type — a CANDIDATE spelling, explicitly UNSETTLED; the predicate/merge-column
    spelling likewise;
  - a first-class **split** primitive (fan-out as named-node + references) and the
    **in-column DAG editor** (the define surface);
  - the **qfs-viewer minimal-walk IMPLEMENTATION mission** (scope "(い)") — deferred by the
    owner until a downstream C-layer item lands.
- **Re-deriving §14c.** The existing map is the input; only its tidying is in scope.

## Experience

- A reader of docs/blueprint.md on main finds **trail** and **walk** defined as domain terms
  in §14b with the exact distinctions: noun vs verb, static vs dynamic, result vs act; "the
  walk produces the trail"; "a walk is always linear — one trail, never a graph"; the
  containment chain address ⊆ trail; and the sharpened operational definition "choose one of
  the steps the current trail admits, and extend" covering reads (describe's declared
  relations/keys) and writes (the next input type dependent on prior values) with one
  sentence. §14c uses the terms instead of paraphrasing them.
- §14c no longer reads "(open; nothing settled)". Its settled points read as rulings: the
  column layout as a display pattern over post-execution semantics (stated simply, with the
  retracted higher-abstraction framing gone); linearity dissolved by placement (DAG inside a
  column, strip linear across columns); the intension/extension weld with its write-edge
  caveat; parity deliberately given up with fidelity assigned to the content. The
  consolidated-open-questions list shrinks accordingly.
- The AI-letter rulings are readable as design rulings: envelope enclosing context +
  interactivity; inward confinement (explicitly the same principle as declared-driver
  host-confinement); single typed egress (reply = typed INSERT into the sender's inbox);
  interactivity derived from the type; modality-free-but-typed entry ("enter freely, confirm
  typed, exit"); form-filling as a walk with commit only at the terminal column;
  condition-split as a declared path-branching rule that keeps every walk linear.
- Every genuinely-open item still reads as open, and each names the downstream mission that
  owns it — so a later session picks up exactly where the recording ends, and nothing settled
  gets re-litigated from a stale map.

## Acceptance

- [ ] **The unmerged §14c map reaches main through this mission, folded not re-derived.** (#20260722122100-fold-the-unmerged-14c-map-into-the-mission-branch.md)
      Commit d0218aa (branch work-20260721-031401) is folded into the mission branch
      (cherry-pick preferred; rebase-and-tidy if it merged first), the viewer-reconsideration
      worktree is never written, and the tidied §14b/§14c ship to main via the normal
      /report → /ship cycle with the patch version bumped per CLAUDE.md.
- [ ] **trail and walk are defined in §14b and cross-referenced from §14c.** (#20260722122200-define-trail-and-walk-in-14b.md) The definitions
      carry all pinned distinctions: trail = noun/static/result (one written path within the
      path concept: canonical backbone + selection/relation/reverse segments; "where you have
      walked, recorded"); walk = verb/dynamic/act (extending a trail one column at a time; the
      walk produces the trail; always linear — exactly one trail, never a graph; a DAG's
      non-linearity never appears in a walk); address ⊆ trail; and the sharpened "choose one
      of the steps the current trail admits, and extend" definition covering reads and writes.
- [ ] **§14c's settled points move from open to ruled.** (#20260722122300-rewrite-14c-settled-points-as-rulings.md) Rulings 1–4 of ## Goal are written
      as rulings in §14c: the column layout as a display pattern for post-execution semantics
      (kept simple), placement-dissolution of linear-vs-graph (DAG confined to a column's
      interior), the path=query=set intension/extension weld (write-edge caveat retained as
      open), and the deliberate surrender of 100% viewer/language parity (fidelity is the
      content's responsibility). The retracted "higher-abstracted container/design-pattern"
      framing is removed from the section.
- [ ] **The AI-letter rulings are recorded in the blueprint as design rulings.** (#20260722122400-record-the-ai-letter-rulings.md) Rulings 5–8
      of ## Goal appear (a §14c subsection or a section §14c points to): envelope =
      context + interactivity, inward confinement named as the same principle as
      declared-driver host-confinement, single typed egress (reply = typed INSERT into the
      sender's inbox), type-derived interactivity with free modality landing on the fixed
      type, form-filling as a walk (partial struct = valid intermediate value; effect only at
      the terminal column — no I/O until COMMIT), condition-split as a declared, checkable
      path-branch that keeps walks linear, and "who drives is not the design axis."
- [ ] **The remaining open items stay open, each naming its downstream mission.** (#20260722122500-consolidate-14c-open-list-and-hold-non-goals.md) §14c's
      consolidated open list keeps (at least): the ASK-grammar / predicate- and merge-column
      spellings (candidates, unsettled), the split primitive + in-column DAG editor, the
      request-principal seam / empty-home root, the enumerate-root plumbing, the per-viewport
      projections, and the intension/extension write edge — with the named-but-not-created
      downstream missions of ## Scope attached, and none of them created by this mission.
- [ ] **The non-goals held.** (#20260722122500-consolidate-14c-open-list-and-hold-non-goals.md) No grammar shipped, no viewer code changed, no ASK/split
      implemented; the diff on the mission branch touches design/knowledge documents only
      (blueprint, mission bookkeeping), verified by reading the shipped PR's file list.

## Changelog

- 2026-07-21 — Mission placed by the design session (owner scope choice "(あ): terminology
  and rulings first" — record the converged viewer design, do not implement). Grounded in the
  shipped §14b and the unmerged §14c map (commit d0218aa, branch work-20260721-031401,
  read-only at .worktrees/viewer-reconsideration). The trail/walk definitions and rulings 1–8
  in ## Goal are the design conversation's converged judgment, recorded for the driving
  session to inscribe into the blueprint. No tickets cut yet — a claiming session
  interrogates and cuts its own; drive_authorized deliberately left empty (no per-ticket
  interrogation has happened).
- 2026-07-22 — ticket added — 20260722122100-fold-the-unmerged-14c-map-into-the-mission-branch.md
- 2026-07-22 — ticket added — 20260722122200-define-trail-and-walk-in-14b.md
- 2026-07-22 — ticket added — 20260722122300-rewrite-14c-settled-points-as-rulings.md
- 2026-07-22 — ticket added — 20260722122400-record-the-ai-letter-rulings.md
- 2026-07-22 — ticket added — 20260722122500-consolidate-14c-open-list-and-hold-non-goals.md
- 2026-07-22 — strategy created and linked — the-viewer-renders-qfs-as-a-walk-over-trails
- 2026-07-22 — mission replanned for the overnight run - five docs-only recording tickets cut from the six acceptance criteria (fold 14c, define trail/walk, rule the settled points, record the AI-letter rulings, consolidate the open list and hold the non-goals); per-ticket judgment pre-answered (documentation-only, verified by reading the shipped blueprint; downstream missions named not created); drive_authorized stamped — mission.md
