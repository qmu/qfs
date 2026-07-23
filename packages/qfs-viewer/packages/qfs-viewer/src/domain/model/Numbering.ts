// Heading numbers: how an outline position is written for a reader.
//
// COUNTING IS NOT HERE, and that is the point. Ticket …004236 step 5 asked for
// "a per-document counter on the `makeSluggers` pattern — a 6-slot level stack
// where `next(level)` increments the slot at `level`". That code is not in this
// repository and must not be: plgg-md's `decorateHeading` seam hands the
// position over already computed, as an `Ordinal` (`[3, 2, 1]`).
//
// Upstream did it that way on purpose, and its reasoning is the same trap the
// ticket flagged. `MarkdownDoc` builds `headings` in a SEPARATE traversal from
// `body`; a counter held HERE would advance only in the body traversal, so a
// number in a table of contents would silently disagree with the number on the
// heading it points at — surfacing far away, as a citation bug. Counting is a
// deterministic function of the heading sequence, so plgg-md runs it in both
// traversals and both agree by construction. A decorator that holds no state
// cannot drift.
//
// So the ticket's "one counter run" acceptance criterion is met by having no
// counter at all. What is left is formatting, which IS ours: counting is
// plgg-md's, presentation is the site's.
import { type SoftStr } from "plgg";

/**
 * A heading's outline position, outermost first. `[3, 2, 1]` is the first H3
 * under the second H2 under the third H1.
 *
 * Structurally plgg-md's `Ordinal`, restated here rather than imported so the
 * domain states its own vocabulary — and so the meaning of a **zero** can be
 * written down, because that is the case a reader will hit and wonder about.
 */
export type Ordinal = ReadonlyArray<number>;

/**
 * Render an {@link Ordinal} as the prefix a reader sees: `3-2-1.`
 *
 * ZEROES ARE KEPT, and this is the decision worth understanding. plgg-md marks
 * a SKIPPED level with a zero: an `h1` followed directly by an `h3` is
 * `[1, 0, 1]`, and a document that opens at `h2` is `[0, 1]`. Dropping the
 * zeroes to print a tidier `1-1.` would be actively wrong — a real `h2` under
 * the first `h1` is `[1, 1]`, which is *also* `1-1.`, so two different
 * positions would print the same number and a citation could not tell them
 * apart. `1-0-1.` is unambiguous, and it shows the gap rather than hiding it:
 * the ticket asks for "a defined, tested number — not a crash and not a silent
 * gap", and the zero is precisely how the gap stays visible.
 *
 * An empty ordinal renders as the empty string. It cannot arise from a real
 * heading — every heading has a level, so its ordinal has at least one slot —
 * but this is a total function over its input type rather than a partial one
 * with an assertion.
 */
export const formatOrdinal = (
  ordinal: Ordinal,
): SoftStr =>
  ordinal.length === 0
    ? ""
    : `${ordinal.join("-")}.`;
