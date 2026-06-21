# Round 1 Review — Architect

Reviewer: Architect (Neutral / Structural Idealism)
Artifacts reviewed:
- `directions/direction-v1.md` (Planner)
- `designs/design-v1.md` (Constructor)
Coherence-checked against: `models/model-v1.md` (own).

Lens: translation fidelity (business intent → structure → build) and structural
integrity (package boundaries, email→filesystem mapping traceability).

---

## Decision per artifact

- **Direction v1 — Approve with observations.**
- **Design v1 — Approve with minor suggestions.**

Neither warrants a Request revision: both are internally sound and faithful to
the gdrive-ftp lineage. The one genuine cross-artifact conflict (navigation
hierarchy) is adjudicated below with a single canonical recommendation rather
than a revision demand, because it is a modeling choice the team must *pick*,
not a defect either author introduced.

---

## Direction v1 — domain feedback (Approve with observations)

The Direction is faithful to the "same concept, same experience, new backend"
business intent and maps cleanly onto the model's boundary-preservation thesis.
Personas, risk framing (privacy, destructive-op trust, quotas, scope creep), and
the v1 success criteria all translate into structures that already exist in
gdrive-ftp. Translation fidelity from business vision to model is high.

**Observation 1 — "labels behave like directories, threads and messages like
files" (lines 12–16, 83–84) under-specifies the leaf, and that ambiguity is
exactly what the model and design then resolve differently.** The Direction
lists "threads and messages" as co-equal file analogs. A business document is
allowed to be metaphor-level, but because this single phrase is the seed of the
downstream divergence, it should state the *user-visible* promise unambiguously.
*Structural alternative:* amend §1/§4 to commit to one user-facing sentence —
"each email (message) is a file; a conversation (thread) is reachable as a group
but is not a second kind of file in the same listing." This keeps the business
text metaphor-level while removing the fork that the model and design inherit.
It costs one sentence and makes the requirement traceable end-to-end.

**Observation 2 — the "same directory structure and project shape" promise
(lines 86–89) is a structural commitment hiding in a business document.** This
is the load-bearing fidelity constraint, and it is good that it is stated. But
"same directory structure" is true at the *package* level (auth/shell/audit/
output preserved) and deliberately *false* at the *navigation* level (Gmail has
no folder tree). *Structural alternative:* split the promise into "same project
shape and command vocabulary" (a true structural invariant) versus "an honest,
documented mapping where the backend differs" (already gestured at on lines
91–95). Separating the invariant from the mapping prevents a stakeholder from
reading "same directory structure" as "Gmail has folders," which it does not.

---

## Design v1 — domain feedback (Approve with minor suggestions)

The Design is a strong, traceable port. The file-by-file inventory, the
boundary-preservation discipline (auth verbatim, audit near-verbatim, owned DTOs
never marshaling the vendor struct), the least-privilege scope reasoning, the
explicit `put`=draft / `send`=separate-verb safety split, and the delivery plan
all faithfully realize both the business intent and the model's component
taxonomy. The `gmailClient` interface (§3) is a justified, low-cost improvement
over gdrive-ftp's concrete dependency and strengthens the backend-client
boundary without breaking it. From the structural-integrity lens this is
build-ready.

**Suggestion 1 (the central concern) — the §0 mapping table introduces a
THIRD navigation tier (label → thread → message) that neither gdrive-ftp's
structure nor the model supports, and it strains the boundary it claims to
preserve.** Design lines 22–33 map "folder inside a drive → thread inside a
label" and "file inside a folder → message inside a thread," producing a
3-level `cd` path (`cd INBOX`, `cd <thread>`, then messages as leaves). The
model (§2, §3 item 3) deliberately collapses this to exactly two conceptual
levels (label → message) and surfaces the thread as a `threadId` field plus an
`id:` addressable container, *not* as a `cd`-able directory. This is the one
real divergence; it is adjudicated in the dedicated section below. *Structural
alternative (preview):* adopt the model's 2-level canonical model and demote the
thread from a navigation tier to a grouping affordance. Concrete edit to §0/§2:
replace the four-row mapping table's bottom two rows with a single row
("file inside a folder → message under a label; attachments are leaves inside a
message") and move thread to a "grouping view" note.

**Suggestion 2 — `Ref.Kind ∈ {label, thread, message}` (line 48) hard-codes the
3-level model into the cwd stack type, which couples a contested modeling
decision to the most reused structure in the shell.** If the team adopts the
2-level canonical model, `thread` should not be a cwd `Kind` at all (you never
`cd` into it). *Structural alternative:* define `Kind ∈ {label, message}` for
the navigable stack, and carry `threadId` as a *field* on the message entry/Ref
rather than as a stack frame. Keep an internal `thread` notion only inside the
client wrapper for batching (`GetThread`), where it is an implementation detail,
not a navigation level. This keeps the gdrive-ftp `Ref`-stack architecture
(drive → folder → file = two navigable tiers above the leaf) structurally
isomorphic to (label → message), preserving the "same `Ref` stack" claim the
model and §1 both rely on.

**Suggestion 3 — the inventory drops `internal/gmail/model.go` that the model
proposed (model §5) for message-name synthesis, folding helpers into
`client.go` (design line 46).** Message-name synthesis (date-prefix + subject
slug + collision/`id:` strategy) has no gdrive-ftp counterpart and is the single
genuinely new domain concern; burying it in the client wrapper mixes
API-transport code with display-naming policy. *Structural alternative:* keep a
small `internal/gmail/model.go` (or `name.go`) for the owned `Ref`/name-synthesis
logic, so the new-and-only-new domain concept has a named home and its
table-driven tests (`TestMessageName`, collision cases) sit beside it. This is a
file-boundary suggestion, not a behavior change, and it aligns the design with
the model's taxonomy at zero build cost.

**Suggestion 4 — scope wording drift between R1 and §2.** §2 (lines 111–116)
proposes `gmail.modify` + `gmail.compose` as the pragmatic v1 set; R1 (line 214)
says the same; the model (§4) recommended the `readonly`+`modify`+`compose`
union and flagged it for decision. These agree in spirit but state the set
slightly differently (whether `readonly` is named explicitly given `modify`
supersedes it). *Structural alternative:* state once, in the single documented
`auth.go` constant, that the requested set is `gmail.modify` + `gmail.compose`
and add a one-line comment that `modify` subsumes read and cannot hard-delete
(the safety property). One source of truth removes the drift.

---

## Cross-artifact coherence (direction ↔ model ↔ design)

**Strong alignment on the spine.** All three agree on: backend-agnostic plumbing
(auth/shell/audit/output) preserved verbatim-to-near-verbatim; one new backend
package (`internal/gmail`) carrying all email-specific work; `put`=draft and
`send` as a separate, audited, irreversible-by-construction verb; `rm`=trash
(reversible) as a load-bearing safety promise; `id:` addressing promoted to the
canonical handle for synthesized-name items; least-privilege scopes over a single
super-scope. The Direction's success criteria (lines 105–115) trace cleanly to
the model's boundaries and the design's delivery steps. Fidelity from business →
structure → build is high on every axis except one.

**The single coherence fault line: navigation depth.**
- Direction (line 12, 83–84): "threads and messages like files" — *ambiguous*,
  permits both readings.
- Model (§2 line 50, §3 item 3): exactly **two** conceptual levels
  (label → message); thread is a field + `id:` container, opt-in grouping.
- Design (§0 lines 22–33, line 48): **three** levels (label → thread → message),
  thread is a `cd`-able directory and a `Ref.Kind`.

This is the one place where the three artifacts do not yet tell the same story,
and it propagates into the `Ref` type, the path resolver, `pwd` output
(design line 124 prints `/INBOX/<thread-subject>`, model prints `/INBOX` then a
message), Tab-completion, and the SKILL.md path model. It must be resolved before
coding, because the cwd stack type is the most-reused structure and cannot carry
two incompatible shapes.

---

## Adjudication — canonical navigation model

**Positions.**
- *Model (mine):* message = default leaf; **two** navigable tiers (root→label,
  label→message), attachments as leaves inside a message; thread = `threadId`
  field + `id:thread:<id>` addressable container; thread grouping opt-in.
- *Design (Constructor):* **three** navigable tiers (root→label, label→thread,
  thread→message); message is the leaf only after `cd`-ing through a thread;
  attachments as leaves inside a message; `Ref.Kind ∈ {label,thread,message}`.

**Structural coherence of each, judged against gdrive-ftp.**

gdrive-ftp's structure is *virtual root → drive → folder(tree) → file*. The
number of navigable tiers *above the leaf* in gdrive-ftp is not a fixed "3" — it
is "root, then drive, then an arbitrary-depth folder tree, then file." The
load-bearing structural facts are: (a) the leaf is the thing you `get` as
bytes; (b) every tier above the leaf is a genuine *container you can `cd` into
and `ls`*; (c) the `Ref` stack frames correspond one-to-one to real backend
containers (a drive is a real Drive; a folder is a real Drive folder).

- **Design's 3-level model** is *superficially* the closer visual analog to
  "drive → folder → file" (three named rows). But it fails fact (c): a Gmail
  thread is **not** a container that owns messages the way a folder owns files —
  it is a server-side grouping of messages that *also* independently appear under
  labels, carry their own labels, and are the unit search returns. Making the
  thread a mandatory `cd` tier forces every message access through a container
  that is not a real navigational parent, and it makes `Ref.Kind` carry a frame
  (`thread`) whose semantics differ from the other frames. It also produces the
  awkward `pwd` `/INBOX/<thread-subject>` where thread-subject == message-subject
  for single-message threads (the common case), i.e. a redundant tier most of the
  time.

- **Model's 2-level model** satisfies facts (a) and (b) cleanly: label is a real
  navigable container (a query, `label:X`), message is the leaf you `get` as
  `.eml`, attachments are leaves-inside-a-leaf using the existing
  `ls <leaf>/`→children seam (which is also how the design handles attachments).
  It satisfies (c) in the only way Gmail allows — label-as-view is the honest
  analog of drive-as-container — and it does *not* invent a frame whose
  semantics diverge from the rest of the stack. The cost is that the thread, the
  Gmail-native unit, is demoted from navigation to a field + `id:` handle.

**Recommendation: adopt the MODEL's two-level navigation as canonical**
(root → label → message; attachments as leaves inside a message; thread as a
`threadId` field on every row plus an `id:thread:<id>` addressable container and
an opt-in grouping view), and **retain the Design's thread machinery one layer
down** as an *implementation* concern, not a navigation tier.

**Rationale grounded in gdrive-ftp.** gdrive-ftp's `Ref` stack frames are always
real, `cd`-able, `ls`-able containers, and the leaf is always the byte-bearing
object you `get`. The 2-level model keeps that invariant exactly: every stack
frame (label) is a real navigable view, the leaf (message) is the `.eml` you
`get`. The 3-level model breaks the invariant by inserting a frame (thread) that
is not a navigational parent in Gmail's data model and that is usually redundant
(single-message threads). Preserving the invariant is precisely the "same
directory structure / same experience" fidelity promise the Direction makes and
the model is charged with protecting. The Design's per-thread fetch logic
(`GetThread`, `Format("metadata")` batching, §2) is *correct and needed* — but it
belongs inside `internal/gmail` as a batching/grouping mechanism, surfaced via
`threadId` and `id:thread:<id>`, not as a `cd` tier and not as a `Ref.Kind`.

**Concrete convergence edits (for the Constructor's next design version):**
1. §0 table: collapse the bottom two rows to "message under a label = file;
   attachment = leaf inside a message." Add a note: "a thread is a grouping view,
   reachable via `threadId`/`id:thread:<id>`, not a `cd` tier."
2. Line 48: `Ref.Kind ∈ {label, message}` for the navigable stack; `threadId`
   becomes a field on the message entry, not a stack frame.
3. Line 124 (`pwd`): prints `/INBOX` (label) then the message is the leaf;
   `pwd` inside a leaf is not a `/INBOX/<thread>` path.
4. §2 command table `cd` row: "navigate root→label only; messages and threads are
   not `cd` targets" (threads addressable by `id:`, not enterable).
5. Keep `GetThread`/thread batching in the client wrapper unchanged.

This costs the Constructor a small, localized edit (the resolver and `Ref.Kind`
were going to be touched regardless) and yields a single, traceable navigation
model that all three artifacts and the SKILL.md path documentation can share.

---

## Summary of required follow-ups

- Direction: one-sentence leaf commitment (Obs 1); split "same directory
  structure" into invariant-vs-mapping (Obs 2). *Observations, not blocking.*
- Design: adopt the 2-level canonical navigation (Sug 1 + the five convergence
  edits); narrow `Ref.Kind` to `{label,message}` (Sug 2); keep a `model.go` home
  for name synthesis (Sug 3); single scope constant (Sug 4). *Minor, localized.*
