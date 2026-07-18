// What qfs declares about itself — the root column's two axes, READ and never
// held.
//
// The developer, at the 2026-07-17 design discussion:
//
// > 実際に github に接続して querying する際のパスと、driver 一覧があって、
// > そのドライバーにどんなコネクションがあるか、というのは別のはずです
//
// TWO AXES, and they are not two views of one list:
//
//   axis 1 — the QUERY PATHS: the path you actually query (/github/qmu/…).
//            `/sys/paths`, a function of what the operator CONNECTed.
//   axis 2 — the ADMIN VIEW: which drivers exist, and what connections each
//            driver has. `/sys/connections`.
//
// **They do not even share a vocabulary**, which is the measurement that
// settles it. Read live: `/sys/paths` names drivers like `gmail` and `gdrive`
// where `/sys/connections` names `google`; `/sys/paths` names `ghdecl` where
// `/sys/connections` names `github`. ONE connection backs SEVERAL query paths.
// There is no 1:1 map to fuse the axes along, so fusing them is not merely
// against the correction — it is not expressible. qfs says the same thing in
// its own source (`catalog.rs:110`): the catalog is a pure function of the
// BINARY, "never of the operator's live CONNECT-ed/declared mounts". So the
// two stay separate here, and separate in the Scene: axis 1 navigates, axis 2
// does not.
//
// NOTHING here is a driver/mount/prefix list. The only literals are the two
// paths that ASK the questions. Naming the question is not holding the answer
// — and holding the answer is what the mission's "qfs is mandatory" clause
// exists to prevent, because a second list in this repository would drift from
// the first.
//
// The rows this reads carry the OPERATOR'S REAL ACCOUNTS. Nothing from a live
// `/sys` response belongs in source, tests, or fixtures — every test here
// builds its own rows.
import {
  type SoftStr,
  type Option,
  type Result,
  type InvalidError,
  isOk,
  invalidError,
  ok,
  err,
  some,
  none,
} from "plgg";
import {
  type ResourceRow,
  type ResourceTable,
  asResourceTable,
} from "#qfs-viewer/domain/model/Resource";

/**
 * How many rows an axis reads.
 *
 * Stated in the statement rather than assumed, so qfs's own `truncated` flag
 * is what tells the reader when there is more — the same honesty rule every
 * other column in this viewer follows.
 */
const AXIS_ROW_LIMIT = 200;

/** Axis 1's question. The path you actually query. */
export const PATHS_QUERY: SoftStr = `/sys/paths |> limit ${AXIS_ROW_LIMIT}`;

/** Axis 2's question. Which drivers exist, and what connections each has. */
export const CONNECTIONS_QUERY: SoftStr = `/sys/connections |> limit ${AXIS_ROW_LIMIT}`;

/**
 * Axis 1: one path the operator declared, and can query.
 *
 * `alias_of` is carried rather than dropped. An alias IS a path you can query,
 * so it belongs on this axis — but listing it as though it were an independent
 * connection would overstate what is connected, which is the same class of
 * defect as inventing a menu. Show the alias, and show that it is one.
 *
 * `account` is `Option` because qfs leaves it empty for drivers that have no
 * account to name — absence is a real answer here, not a missing field.
 */
export type QueryPath = Readonly<{
  path: SoftStr;
  driver: SoftStr;
  account: Option<SoftStr>;
  aliasOf: Option<SoftStr>;
}>;

/** Axis 2: one driver, and the connections the operator gave it. */
export type DriverConnections = Readonly<{
  driver: SoftStr;
  connections: ReadonlyArray<SoftStr>;
}>;

/**
 * What qfs answered about an axis.
 *
 * THREE states, and collapsing any two is the defect this union exists to
 * prevent:
 *
 *  - `Declared` — qfs named things. Render them.
 *  - `Undeclared` — qfs answered, and named nothing. Render NOTHING: not an
 *    empty menu, not a disabled one. An absent feature is not an empty state.
 *  - `Unanswerable` — qfs could not be asked at all (no runner, no binary, a
 *    malformed answer). This is **not** "declares nothing", and saying it was
 *    would report a broken qfs as a bare machine. It renders qfs's own words
 *    (workaholic:design / self-explanatory-ui: an error says what happened).
 *
 * A closed union, folded only through {@link matchDeclared} — so a fourth
 * state is a compile error rather than a column that silently picks a
 * rendering.
 */
export type Declared<A> =
  | Readonly<{
      __tag: "Declared";
      items: ReadonlyArray<A>;
      /** qfs stopped early — the reader must be told. */
      truncated: boolean;
    }>
  | Readonly<{ __tag: "Undeclared" }>
  | Readonly<{
      __tag: "Unanswerable";
      reason: SoftStr;
    }>;

/**
 * Fold a {@link Declared}. The ONLY way to read one.
 *
 * The `never` binding is the guarantee the ticket asks for: add a variant to
 * {@link Declared} and this stops compiling, at every call site at once.
 */
export const matchDeclared =
  <A, B>(
    handlers: Readonly<{
      declared: (
        items: ReadonlyArray<A>,
        truncated: boolean,
      ) => B;
      undeclared: () => B;
      unanswerable: (reason: SoftStr) => B;
    }>,
  ) =>
  (declared: Declared<A>): B => {
    if (declared.__tag === "Declared") {
      return handlers.declared(
        declared.items,
        declared.truncated,
      );
    }
    if (declared.__tag === "Undeclared") {
      return handlers.undeclared();
    }
    if (declared.__tag === "Unanswerable") {
      return handlers.unanswerable(
        declared.reason,
      );
    }
    const exhaustive: never = declared;
    return exhaustive;
  };

// A cell qfs may legitimately leave empty. An empty string is absence, not a
// value: qfs's `/sys` tables are text-typed throughout, so "" is how a nullable
// column arrives.
const optionalAt = (
  row: ResourceRow,
  name: SoftStr,
): Option<SoftStr> => {
  const value: unknown = row[name];
  return typeof value === "string" && value !== ""
    ? some(value)
    : none();
};

// A cell the row MUST have to mean anything.
//
// Fails the WHOLE answer rather than skipping the row, and that is deliberate:
// skipping is exactly `catalog.rs`'s best-effort `continue`, which is how qfs's
// own generated catalog silently lost `/cf` — a driver the code registered on
// purpose "so the catalogue surfaces them". A column that quietly drops a row
// tells the reader a shorter truth and never says it did. This repository does
// not reproduce that defect locally.
const requiredAt = (
  row: ResourceRow,
  name: SoftStr,
  what: SoftStr,
): Result<SoftStr, InvalidError> => {
  const found = optionalAt(row, name);
  return found.__tag === "Some"
    ? ok(found.content)
    : err(
        invalidError({
          message: `qfs answered ${what} with a row carrying no ${name} — refusing to show a partial list rather than silently dropping it`,
        }),
      );
};

const rowsOf = <A>(
  table: ResourceTable,
  ofRow: (
    row: ResourceRow,
  ) => Result<A, InvalidError>,
): Result<ReadonlyArray<A>, InvalidError> => {
  const items: Array<A> = [];
  for (const row of table.rows) {
    const item = ofRow(row);
    if (!isOk(item)) {
      return item;
    }
    items.push(item.content);
  }
  return ok(items);
};

// The one lowering from "what the runner said" to a {@link Declared}. Pure:
// the input is whatever `qfs run` printed, already JSON-parsed, so every state
// below is reachable in a test without a qfs.
const declaredFrom = <A>(
  answer: Result<unknown, InvalidError>,
  ofRow: (
    row: ResourceRow,
  ) => Result<A, InvalidError>,
): Declared<A> => {
  const parsed = isOk(answer)
    ? asResourceTable(answer.content)
    : answer;
  if (!isOk(parsed)) {
    return {
      __tag: "Unanswerable",
      reason: parsed.content.content.message,
    };
  }
  const items = rowsOf(parsed.content, ofRow);
  if (!isOk(items)) {
    return {
      __tag: "Unanswerable",
      reason: items.content.content.message,
    };
  }
  return items.content.length === 0
    ? { __tag: "Undeclared" }
    : {
        __tag: "Declared",
        items: items.content,
        truncated: parsed.content.truncated,
      };
};

const queryPathOf = (
  row: ResourceRow,
): Result<QueryPath, InvalidError> => {
  const path = requiredAt(
    row,
    "path",
    "/sys/paths",
  );
  if (!isOk(path)) {
    return path;
  }
  const driver = requiredAt(
    row,
    "driver",
    "/sys/paths",
  );
  if (!isOk(driver)) {
    return driver;
  }
  return ok({
    path: path.content,
    driver: driver.content,
    account: optionalAt(row, "account"),
    aliasOf: optionalAt(row, "alias_of"),
  });
};

/**
 * Axis 1 — the query paths, as qfs declares them.
 *
 * Takes the runner's raw answer to {@link PATHS_QUERY}.
 */
export const asQueryPaths = (
  answer: Result<unknown, InvalidError>,
): Declared<QueryPath> =>
  declaredFrom(answer, queryPathOf);

type ConnectionRow = Readonly<{
  driver: SoftStr;
  connection: SoftStr;
}>;

const connectionOf = (
  row: ResourceRow,
): Result<ConnectionRow, InvalidError> => {
  const driver = requiredAt(
    row,
    "driver",
    "/sys/connections",
  );
  if (!isOk(driver)) {
    return driver;
  }
  const connection = requiredAt(
    row,
    "connection",
    "/sys/connections",
  );
  if (!isOk(connection)) {
    return connection;
  }
  return ok({
    driver: driver.content,
    connection: connection.content,
  });
};

// qfs's own row order is preserved, and the drivers appear in the order it
// first mentioned them. Re-sorting would be this repository inventing a
// ranking qfs did not declare — the same refusal the whole ticket rests on.
const byDriver = (
  rows: ReadonlyArray<ConnectionRow>,
): ReadonlyArray<DriverConnections> => {
  const order: Array<SoftStr> = [];
  const grouped = new Map<
    SoftStr,
    Array<SoftStr>
  >();
  for (const row of rows) {
    const seen = grouped.get(row.driver);
    if (seen === undefined) {
      order.push(row.driver);
      grouped.set(row.driver, [row.connection]);
    } else {
      seen.push(row.connection);
    }
  }
  return order.map((driver) => ({
    driver,
    connections: grouped.get(driver) ?? [],
  }));
};

/**
 * Axis 2 — which drivers exist, and what connections each driver has.
 *
 * Takes the runner's raw answer to {@link CONNECTIONS_QUERY}, and groups it
 * the way the developer's question is shaped: driver first, then its
 * connections. The grouping is the whole of the transformation — no path is
 * derived from it, because a connection is not a path (see this file's header:
 * one `google` connection backs both `gmail` and `gdrive`).
 */
export const asConnectionsByDriver = (
  answer: Result<unknown, InvalidError>,
): Declared<DriverConnections> =>
  matchDeclared<
    ConnectionRow,
    Declared<DriverConnections>
  >({
    declared: (rows, truncated) => ({
      __tag: "Declared",
      items: byDriver(rows),
      truncated,
    }),
    undeclared: () => ({ __tag: "Undeclared" }),
    unanswerable: (reason) => ({
      __tag: "Unanswerable",
      reason,
    }),
  })(declaredFrom(answer, connectionOf));
