// The generic lowering: any qfs path's describe, as a default column view.
//
// The plan's step 1 (汎用ロワリング): any path qfs can `describe` renders as
// a default view with NO per-resource code — the describe answer names the
// node and its columns, a plain read supplies the rows, and a row that names
// a CONTAINED child path is a click that appends a segment to the trail.
//
// `lowerToDefaultView` is deliberately one named, pure function from
// (describe, rows) to view data. It is ONE deterministic manifest generator,
// and the mission requires the seam to stay explicit: richer generators (the
// markdown collection path's, later an LLM's) sit BESIDE this function and
// feed the same rendering, so the pipeline stays single
// (workaholic:design / sacrificial-architecture; the plan's 配管は一本).
//
// Navigation here is CONTAINMENT ONLY. Row selection (`@selection`) and
// derived reverse edges are strategy-owned open questions (plan.md 開いた問い)
// — the MVP stands on containment segments, which no answer to those
// questions will invalidate, and this module must not invent local answers.
import {
  type SoftStr,
  type Result,
  type Option,
  type InvalidError,
  invalidError,
  ok,
  err,
  some,
  none,
} from "plgg";
import {
  type ResourceColumn,
  type ResourceRow,
  type ResourceTable,
} from "#qfs-viewer/domain/model/Resource";

// A qfs path as this viewer will repeat it into a statement: absolute, and
// spelled from a closed charset. The charset is not cosmetic — a path is
// embedded verbatim in `<path> |> limit N`, and excluding whitespace, quotes
// and pipes is what makes that embedding injection-proof. `@` stays in for
// qfs's ref coordinates (`/git/repo@v1.2/…`).
const QFS_PATH =
  /^\/[A-Za-z0-9_.@-]+(?:\/[A-Za-z0-9_.@-]+)*$/;

/** Whether `raw` is a qfs path this viewer will address. */
export const isQfsPath = (
  raw: SoftStr,
): boolean =>
  QFS_PATH.test(raw) &&
  // `.` and `..` are legal charset but would give one resource many
  // addresses (and a canonical address is the trail's anchor), so they are
  // refused rather than normalized — normalizing would be this repo
  // guessing at qfs's path semantics.
  !raw
    .split("/")
    .some((s) => s === "." || s === "..");

/**
 * Validate an untrusted string (a URL segment, a form field) into a qfs
 * path. The error names the rule, because the person who typed the path is
 * the one who reads it.
 */
export const asQfsPath = (
  raw: SoftStr,
): Result<SoftStr, InvalidError> =>
  isQfsPath(raw)
    ? ok(raw)
    : err(
        invalidError({
          message: `not a qfs path: ${JSON.stringify(raw)} — a qfs path is absolute (/local/…, /sql/…) and uses letters, digits, and ._@- only`,
        }),
      );

/** What `qfs describe` said about a node — the slice this viewer reads. */
export type ResourceDescribe = Readonly<{
  path: SoftStr;
  archetype: SoftStr;
  columns: ReadonlyArray<ResourceColumn>;
}>;

const isRecord = (
  v: unknown,
): v is Readonly<Record<string, unknown>> =>
  typeof v === "object" &&
  v !== null &&
  !Array.isArray(v);

/**
 * Read qfs's describe answer into a {@link ResourceDescribe}.
 *
 * The input is whatever `qfs describe --json` printed, already JSON-parsed —
 * UNTRUSTED, like every program boundary. qfs's own error shape
 * (`{"error":{…}}`, e.g. `unknown_mount` for a path nothing serves) is
 * reported as itself, because "no driver is mounted for /nosuch" is exactly
 * what the person who typed the path needs to read.
 *
 * Describe spells column types as `ty` (`Text`, `Int`) where a run's schema
 * spells `type` (`text`, `int`); both are carried as the strings qfs chose —
 * this viewer displays types, it does not interpret them.
 */
export const asResourceDescribe = (
  value: unknown,
): Result<ResourceDescribe, InvalidError> => {
  if (!isRecord(value)) {
    return err(
      invalidError({
        message:
          "qfs describe did not return an object",
      }),
    );
  }
  const error: unknown = value["error"];
  if (isRecord(error)) {
    const message: unknown = error["message"];
    const code: unknown = error["code"];
    return err(
      invalidError({
        message: `qfs: ${typeof message === "string" ? message : "unknown error"}${typeof code === "string" ? ` (${code})` : ""}`,
      }),
    );
  }
  const path: unknown = value["path"];
  const archetype: unknown = value["archetype"];
  if (typeof path !== "string") {
    return err(
      invalidError({
        message: "qfs describe returned no path",
      }),
    );
  }
  const rawColumns: unknown = value["columns"];
  const columns: Array<ResourceColumn> = [];
  for (const column of Array.isArray(rawColumns)
    ? rawColumns
    : []) {
    if (
      !isRecord(column) ||
      typeof column["name"] !== "string"
    ) {
      return err(
        invalidError({
          message:
            "qfs describe returned a column with no name",
        }),
      );
    }
    const ty: unknown = column["ty"];
    columns.push({
      name: column["name"],
      type:
        typeof ty === "string" ? ty : "unknown",
    });
  }
  return ok({
    path,
    archetype:
      typeof archetype === "string"
        ? archetype
        : "unknown",
    columns,
  });
};

/** One row of the default view: display cells, and — when the row names a
 * contained child — the child's qfs path, ready to become a trail segment. */
export type DefaultViewRow = Readonly<{
  cells: ReadonlyArray<SoftStr>;
  child: Option<SoftStr>;
}>;

/** The default column view — pure data, renderer-agnostic. */
export type DefaultView = Readonly<{
  path: SoftStr;
  archetype: SoftStr;
  columns: ReadonlyArray<ResourceColumn>;
  rows: ReadonlyArray<DefaultViewRow>;
  truncated: boolean;
}>;

// A cell is for reading, not for shipping: a blob's `content` column is the
// whole file base64-ed, and rendering it unabridged would make one row eat
// the column. The full value stays reachable by navigating to the row's own
// path; this is display truncation, not data truncation, and the marker says
// it happened.
const CELL_MAX = 160;

const cellOf = (value: unknown): SoftStr => {
  if (value === undefined || value === null) {
    return "";
  }
  const s =
    typeof value === "string"
      ? value
      : typeof value === "number" ||
          typeof value === "boolean"
        ? String(value)
        : JSON.stringify(value);
  return s.length > CELL_MAX
    ? `${s.slice(0, CELL_MAX)}…`
    : s;
};

/**
 * The containment rule — the ONE way a generic row becomes a click.
 *
 * A row links when its `path` column names a valid qfs path strictly inside
 * the path being viewed. That convention is read off what qfs actually
 * returns (blob namespaces answer a `path` per entry); a table whose rows
 * carry no such column simply has no links yet — row SELECTION is the
 * /resolve ticket's business, on a grammar strategy still owns.
 */
export const containedChild = (
  parent: SoftStr,
  row: ResourceRow,
): Option<SoftStr> => {
  const child: unknown = row["path"];
  return typeof child === "string" &&
    child.startsWith(`${parent}/`) &&
    isQfsPath(child)
    ? some(child)
    : none();
};

/**
 * Lower (describe, rows) to the default column view.
 *
 * Thin and total: no archetype dispatch, no per-service branch — the same
 * function answers a directory, a database table, and a mail folder, which
 * is the acceptance item's "no per-resource code" made literal.
 */
export const lowerToDefaultView = (
  described: ResourceDescribe,
  table: ResourceTable,
): DefaultView => ({
  path: described.path,
  archetype: described.archetype,
  // The run's own schema orders the cells; describe's column list can be
  // wider than a projected read, and the rows are the thing on screen.
  columns: table.columns,
  rows: table.rows.map((row) => ({
    cells: table.columns.map((c) =>
      cellOf(row[c.name]),
    ),
    child: containedChild(described.path, row),
  })),
  truncated: table.truncated,
});
