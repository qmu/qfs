// The trail: which documents are open, in the order they were opened.
//
// This is the mission's navigable state, and it lives in the URL — nowhere
// else. Its canonical address is a PATH under `/resolve` (docs/adr/0007):
// `/resolve/docs/a.md,docs/b.md` says: the corpus list, then `a.md`, then
// `b.md` opened from a link inside `a.md`. Reload it and the same three
// columns come back, because the address was the only thing holding them
// (`workaholic:design` / `modeless-design`).
//
// The address is PREFIX-CLOSED at the comma: cut it at any separator and
// what remains is a valid address whose columns are exactly the first
// columns of the longer one — column i is the resolution of the address's
// prefix i, and a click appends one segment. `?cols=` was the first
// spelling of the same idea; `/resolve` subsumes it, and the legacy query
// is read only to redirect it to the canonical address, so there is ONE
// serialization, not two that drift (`workaholic:design` /
// `sacrificial-architecture`; the decision is recorded in docs/adr/0007).
//
// That is the whole reason this is a value and not a session: a server that
// remembered your columns would answer differently to two people at the same
// address, and the address would stop being the thing you can send someone.
// plggmatic proposed the model — columns as projected depth, the whole state
// in the URL — and since ADR 0002's second amendment the strip renders
// through the plggmatic engine itself; the trail stays this viewer's own
// vocabulary, lowered into the engine's Scene at the entrypoint.
import {
  type SoftStr,
  type Option,
  matchOption,
  isOk,
  some,
  tryCatch,
} from "plgg";
import {
  type DocumentPath,
  asDocumentPath,
  documentPathString,
} from "#qfs-viewer/domain/model/Vocabulary";
import { isQfsPath } from "#qfs-viewer/domain/model/Describe";

/**
 * One thing a column can show.
 *
 * A tagged union rather than a bare path, because a document and a qfs
 * resource are genuinely different things — one is markdown on disk with an
 * index behind it, the other is a live table qfs answers per request. The
 * mission asks for them "alongside", and alongside means BOTH are in the
 * trail, not that one pretends to be the other.
 *
 * `Qfs` is the generic third kind: any path qfs can describe, lowered to
 * the default column view with no declaration and no per-resource code
 * (the mission's describe generic browsing). `Resource` remains the
 * DECLARED, curated entry a repository chose to surface with its own query;
 * `Qfs` is the address a reader walked to.
 */
export type Stop =
  | Readonly<{ __tag: "Doc"; path: DocumentPath }>
  | Readonly<{
      __tag: "Resource";
      name: SoftStr;
    }>
  | Readonly<{ __tag: "Qfs"; path: SoftStr }>;

/**
 * What is open, leftmost first. Column 0 is always the corpus list, so
 * `trail[0]` is the FIRST opened column.
 */
export type Trail = ReadonlyArray<Stop>;

/** A resource stop's prefix in the URL. */
const RESOURCE_PREFIX = "res:";

/** A qfs stop's prefix in the URL. */
const QFS_PREFIX = "qfs:";

/** Builds a document stop. */
export const docStop = (
  path: DocumentPath,
): Stop => ({ __tag: "Doc", path });

/** Builds a resource stop. */
export const resourceStop = (
  name: SoftStr,
): Stop => ({ __tag: "Resource", name });

/** Builds a qfs stop. The path must already have passed `asQfsPath`. */
export const qfsStop = (path: SoftStr): Stop => ({
  __tag: "Qfs",
  path,
});

// A path is escaped so the separator cannot be forged, then its slashes are
// put back so the URL stays readable.
//
// `encodeURIComponent` escapes `,` to `%2C` and `%` to `%25` — which is what
// makes splitting on `,` sound, because a real comma in a filename can no
// longer look like a separator. It also escapes `/` to `%2F`, which is correct
// but unreadable, and this mission asks for a traversal that is legible in the
// URL. Slashes carry no ambiguity here (the separator is a comma), so they go
// back. `decodeURIComponent` reverses all of it, `/` included, so the
// round-trip holds.
//
// A resource is prefixed `res:` so the two kinds cannot be confused in a URL.
// A DocumentPath must end `.md` and may not be absolute, so `res:users` could
// never BE one — but relying on that would mean the reader has to know the
// rule to read the URL, and the mission asks for a legible traversal.
const encodeSegment = (stop: Stop): string =>
  stop.__tag === "Doc"
    ? encodeURIComponent(
        documentPathString(stop.path),
      ).replaceAll("%2F", "/")
    : stop.__tag === "Resource"
      ? `${RESOURCE_PREFIX}${encodeURIComponent(stop.name)}`
      : // A qfs path's charset holds no comma and no percent (`asQfsPath`
        // enforces it), so it reads back verbatim — but it goes through the
        // same escape as everything else so the separator rule has one
        // implementation, not one per kind.
        `${QFS_PREFIX}${encodeURIComponent(stop.path).replaceAll("%2F", "/")}`;

/**
 * Render a trail as the comma-joined segment list — the part of the
 * `/resolve` address after the prefix.
 *
 * This is the ONE serialization of a trail. It also rides, verbatim, as the
 * value the edit form and the qfs path form carry to remember the columns to
 * return to — a parameter of THOSE screens, not a second address grammar.
 *
 * An empty trail renders as the empty string; `trailUrl` renders the bare
 * `/` in that case, so the root is the canonical "nothing open".
 */
export const formatTrail = (
  trail: Trail,
): SoftStr => trail.map(encodeSegment).join(",");

// A resource name is a slug, so anything else under the prefix is a URL
// someone hand-edited. It drops out like any other bad segment.
const RESOURCE_NAME =
  /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

const parseSegment = (
  segment: string,
): Stop | undefined => {
  // The address is untrusted input, so a malformed escape (`%zz`, a
  // truncated `%2`) must drop the segment like any other bad segment —
  // decodeURIComponent THROWS on those, and an address must never be able
  // to throw its way past skip-and-continue into a 500.
  const decodedOr = tryCatch((s: string) =>
    decodeURIComponent(s),
  )(segment);
  if (!isOk(decodedOr)) {
    return undefined;
  }
  const decoded = decodedOr.content;
  if (decoded.startsWith(RESOURCE_PREFIX)) {
    const name = decoded.slice(
      RESOURCE_PREFIX.length,
    );
    return RESOURCE_NAME.test(name)
      ? resourceStop(name)
      : undefined;
  }
  // A qfs path under the prefix goes through the same gate the /qfs route
  // uses — a hand-edited segment that fails it drops out like any other bad
  // segment, and nothing that fails `asQfsPath` can reach a statement.
  if (decoded.startsWith(QFS_PREFIX)) {
    const path = decoded.slice(QFS_PREFIX.length);
    return isQfsPath(path)
      ? qfsStop(path)
      : undefined;
  }
  const path = asDocumentPath(decoded);
  return isOk(path)
    ? docStop(path.content)
    : undefined;
};

/** The path prefix every non-empty trail address lives under. */
export const RESOLVE_PREFIX = "/resolve";

/**
 * The canonical address of a trail — what a link's `href` is set to, and
 * what a reader copies to reproduce the columns in a fresh session.
 *
 * A PATH, never a query string: the address determines data, and query
 * parameters are left to the corpus column's data filters. Display state
 * (folding, sort, highlights) has no slot in this grammar at all — the
 * codec below reads stops and nothing else, which is what "provably absent
 * from the address" cashes out to.
 *
 * `/` for the empty trail rather than `/resolve/`: the address a person
 * lands on first should be the address they can type.
 */
export const trailUrl = (
  trail: Trail,
): SoftStr =>
  trail.length === 0
    ? "/"
    : `${RESOLVE_PREFIX}/${formatTrail(trail)}`;

/**
 * Read a trail out of the `cols` query value.
 *
 * SKIP-AND-CONTINUE, not reject: the query string is untrusted input that a
 * person can hand-edit and a stale link can carry. A segment that is not a
 * valid document path drops out and the rest of the trail still opens —
 * losing one column beats answering 400 to someone whose bookmark aged. The
 * columns that survive are still exactly the ones the URL named, so nothing is
 * invented to fill the gap.
 *
 * A path that parses but names no document is NOT this function's problem: it
 * is a valid path, and whether the corpus holds it is a question for the
 * index, which answers it per column.
 */
export const parseTrail = (
  raw: Option<SoftStr>,
): Trail =>
  matchOption<SoftStr, Trail>(
    () => [],
    (value) =>
      value === ""
        ? []
        : value
            .split(",")
            .map(parseSegment)
            .filter(
              (stop): stop is Stop =>
                stop !== undefined,
            ),
  )(raw);

/**
 * Read a trail out of a raw request path under {@link RESOLVE_PREFIX}.
 *
 * Takes the RAW pathname, not the router's wildcard capture: the router
 * percent-decodes each part before this codec could split, and a `%2C` in a
 * document name would come back as a real comma — a forged separator. The
 * raw path keeps exactly one decode, and it is `parseSegment`'s.
 *
 * The same skip-and-continue stance as {@link parseTrail}, for the same
 * reason: the address is hand-editable. A path that is not under the prefix
 * at all is the empty trail — the caller routed here, so answering columns
 * for an address this codec cannot read would be inventing state.
 */
export const parseResolvePath = (
  path: SoftStr,
): Trail =>
  path.startsWith(`${RESOLVE_PREFIX}/`)
    ? parseTrail(
        some(
          path.slice(RESOLVE_PREFIX.length + 1),
        ),
      )
    : [];

/**
 * Open `path` from the column at `depth`, returning a NEW trail.
 *
 * Everything to the RIGHT of `depth` is dropped. That is what makes the
 * columns a trail rather than a pile: following a link from the second column
 * means the third column is now about that link, and whatever used to be the
 * third and fourth columns is no longer how you got here. The column you
 * clicked FROM survives — which is the mission's gate: "opens it in a new
 * column to the right without discarding the previous one".
 *
 * `depth` is the index in the TRAIL, not the screen: `depth = -1` is the
 * corpus list (column 0 on screen), so opening from it yields a
 * single-document trail.
 */
export const openFrom = (
  trail: Trail,
  depth: number,
  stop: Stop,
): Trail => [...trail.slice(0, depth + 1), stop];
