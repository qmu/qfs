// The two axes, asserted on qfs's real answer SHAPE with invented values.
//
// Every fixture here is synthetic. `/sys/paths` and `/sys/connections` return
// the operator's real accounts and real connection names on any machine that
// has connected anything, and none of that belongs in a repository. What is
// copied from the live probe is the SCHEMA (column names and text typing) and
// the driver vocabulary, which are facts about the qfs binary rather than about
// anyone's mailbox.
import {
  test,
  check,
  all,
  toBe,
  toEqual,
} from "plgg-test";
import {
  ok,
  err,
  invalidError,
  matchOption,
  type SoftStr,
} from "plgg";
import {
  asQueryPaths,
  asConnectionsByDriver,
  matchDeclared,
  PATHS_QUERY,
  CONNECTIONS_QUERY,
  type QueryPath,
  type DriverConnections,
  type Declared,
} from "#qfs-viewer/domain/model/Declaration";

// The `/sys/paths` schema, copied from a live `qfs describe /sys/paths`. Every
// column is text-typed, which is why an empty string is how a nullable column
// arrives.
const PATHS_SCHEMA = [
  { name: "path", type: "text" },
  { name: "driver", type: "text" },
  { name: "at", type: "text" },
  { name: "secret_ref", type: "text" },
  { name: "alias_of", type: "text" },
  { name: "host", type: "text" },
  { name: "account", type: "text" },
  { name: "app", type: "text" },
  { name: "created_at", type: "text" },
];

const CONNECTIONS_SCHEMA = [
  { name: "driver", type: "text" },
  { name: "connection", type: "text" },
  { name: "created_at", type: "text" },
];

const pathRow = (
  path: string,
  driver: string,
  account: string,
  aliasOf: string = "",
) => ({
  path,
  driver,
  at: "local",
  secret_ref: "",
  alias_of: aliasOf,
  host: "local",
  account,
  app: "",
  created_at: "2026-07-17T00:00:00Z",
});

const answerOf = (
  schema: ReadonlyArray<{
    name: string;
    type: string;
  }>,
  rows: ReadonlyArray<unknown>,
  truncated: boolean = false,
) =>
  ok({
    schema,
    rows,
    meta: { row_count: rows.length, truncated },
  });

// The shape that decides the whole ticket: ONE connection backing TWO query
// paths, under driver names the connections axis never mentions. This mirrors
// the live measurement (a `google` connection behind `gmail` and `gdrive`
// paths; a `github` connection behind a `ghdecl` path) with invented accounts.
const PATHS_ANSWER = answerOf(PATHS_SCHEMA, [
  pathRow("/team-mail", "gmail", "acme"),
  pathRow("/team-files", "gdrive", "acme"),
  pathRow("/team-code", "ghdecl", "acme-org"),
]);

const CONNECTIONS_ANSWER = answerOf(
  CONNECTIONS_SCHEMA,
  [
    {
      driver: "google",
      connection: "acme",
      created_at: "2026-07-17T00:00:00Z",
    },
    {
      driver: "google",
      connection: "acme-backup",
      created_at: "2026-07-17T00:00:00Z",
    },
    {
      driver: "github",
      connection: "acme-org",
      created_at: "2026-07-17T00:00:00Z",
    },
  ],
);

const itemsOf = <A>(
  declared: Declared<A>,
): ReadonlyArray<A> =>
  matchDeclared<A, ReadonlyArray<A>>({
    declared: (items) => items,
    undeclared: () => [],
    unanswerable: () => [],
  })(declared);

const tagOf = <A>(
  declared: Declared<A>,
): string =>
  matchDeclared<A, string>({
    declared: () => "Declared",
    undeclared: () => "Undeclared",
    unanswerable: () => "Unanswerable",
  })(declared);

// ---------------------------------------------------------------------------
// The questions

test("each axis asks its own question, and asks it of qfs", () =>
  all([
    check(
      PATHS_QUERY,
      toBe("/sys/paths |> limit 200"),
    ),
    check(
      CONNECTIONS_QUERY,
      toBe("/sys/connections |> limit 200"),
    ),
  ]));

// ---------------------------------------------------------------------------
// Axis 1 — the query paths

test("axis 1 reads the paths the operator declared", () => {
  const paths = itemsOf(
    asQueryPaths(PATHS_ANSWER),
  );
  return all([
    check(paths.length, toBe(3)),
    check(
      paths.map((p: QueryPath) => p.path),
      toEqual([
        "/team-mail",
        "/team-files",
        "/team-code",
      ]),
    ),
    // qfs's own order, not a ranking this repository invented
    check(
      paths.map((p: QueryPath) => p.driver),
      toEqual(["gmail", "gdrive", "ghdecl"]),
    ),
  ]);
});

test("an account qfs left empty is absence, not an empty label", () => {
  const paths = itemsOf(
    asQueryPaths(
      answerOf(PATHS_SCHEMA, [
        pathRow("/local-tree", "local", ""),
      ]),
    ),
  );
  return all([
    check(paths.length, toBe(1)),
    check(
      paths[0]?.account.__tag ?? "missing",
      toBe("None"),
    ),
  ]);
});

// An alias IS a query path — but listing it as though it were an independent
// connection would overstate what is connected, which is the same class of
// defect as inventing a menu.
test("an alias is carried as an alias, not as a second connection", () => {
  const paths = itemsOf(
    asQueryPaths(
      answerOf(PATHS_SCHEMA, [
        pathRow(
          "/mail-shortcut",
          "gmail",
          "acme",
          "/team-mail",
        ),
      ]),
    ),
  );
  return all([
    check(paths.length, toBe(1)),
    check(
      paths.flatMap((p: QueryPath) =>
        matchOption<
          SoftStr,
          ReadonlyArray<SoftStr>
        >(
          () => [],
          (aliasOf) => [aliasOf],
        )(p.aliasOf),
      ),
      toEqual(["/team-mail"]),
    ),
  ]);
});

// ---------------------------------------------------------------------------
// Axis 2 — the admin view

test("axis 2 groups connections under the driver that has them", () => {
  const drivers = itemsOf(
    asConnectionsByDriver(CONNECTIONS_ANSWER),
  );
  return all([
    check(drivers.length, toBe(2)),
    check(
      drivers.map(
        (d: DriverConnections) => d.driver,
      ),
      toEqual(["google", "github"]),
    ),
    check(
      drivers.map(
        (d: DriverConnections) => d.connections,
      ),
      toEqual([
        ["acme", "acme-backup"],
        ["acme-org"],
      ]),
    ),
  ]);
});

// THE measurement that settles the ticket. The two axes do not share a
// vocabulary: one `google` connection backs `gmail` and `gdrive` paths. There
// is no 1:1 map to fuse the axes along — so fusing them is not merely against
// the developer's correction, it is not expressible.
test("the two axes do not share a driver vocabulary", () => {
  const pathDrivers = new Set(
    itemsOf(asQueryPaths(PATHS_ANSWER)).map(
      (p: QueryPath) => p.driver,
    ),
  );
  const connectionDrivers = new Set(
    itemsOf(
      asConnectionsByDriver(CONNECTIONS_ANSWER),
    ).map((d: DriverConnections) => d.driver),
  );
  return all([
    // the paths axis knows `gmail`; the connections axis has never heard of it
    check(pathDrivers.has("gmail"), toBe(true)),
    check(
      connectionDrivers.has("gmail"),
      toBe(false),
    ),
    // and the reverse: `google` is a connection, never a path's driver
    check(
      connectionDrivers.has("google"),
      toBe(true),
    ),
    check(pathDrivers.has("google"), toBe(false)),
  ]);
});

// ---------------------------------------------------------------------------
// The three states, kept apart

// The defect this union exists to prevent: a broken qfs reported as a bare
// machine. "Cannot say" is not "declares nothing".
test("a runner that cannot answer is Unanswerable, never Undeclared", () =>
  all([
    check(
      tagOf(
        asQueryPaths(
          err(
            invalidError({
              message:
                "this server was built without a qfs runner",
            }),
          ),
        ),
      ),
      toBe("Unanswerable"),
    ),
    check(
      tagOf(
        asConnectionsByDriver(
          err(
            invalidError({
              message: "qfs could not be run",
            }),
          ),
        ),
      ),
      toBe("Unanswerable"),
    ),
  ]));

test("Unanswerable carries qfs's own words, not a substitute", () =>
  check(
    matchDeclared<QueryPath, string>({
      declared: () => "",
      undeclared: () => "",
      unanswerable: (reason) => reason,
    })(
      asQueryPaths(
        err(
          invalidError({
            message:
              "no driver is mounted for /nosuch",
          }),
        ),
      ),
    ),
    toBe("no driver is mounted for /nosuch"),
  ));

test("a machine with nothing connected is Undeclared", () =>
  all([
    check(
      tagOf(
        asQueryPaths(answerOf(PATHS_SCHEMA, [])),
      ),
      toBe("Undeclared"),
    ),
    check(
      tagOf(
        asConnectionsByDriver(
          answerOf(CONNECTIONS_SCHEMA, []),
        ),
      ),
      toBe("Undeclared"),
    ),
  ]));

test("qfs stopping early is carried, so the reader can be told", () =>
  check(
    matchDeclared<QueryPath, boolean>({
      declared: (_items, truncated) => truncated,
      undeclared: () => false,
      unanswerable: () => false,
    })(
      asQueryPaths(
        answerOf(
          PATHS_SCHEMA,
          [
            pathRow(
              "/team-mail",
              "gmail",
              "acme",
            ),
          ],
          true,
        ),
      ),
    ),
    toBe(true),
  ));

// Not a skipped row — the WHOLE answer fails closed. Skipping is exactly
// `catalog.rs`'s best-effort `continue`, which is how qfs's own generated
// catalog silently lost `/cf`. A column that quietly drops a row tells the
// reader a shorter truth and never says it did.
test("a row qfs answered without a path fails the answer, rather than vanishing", () => {
  const declared = asQueryPaths(
    answerOf(PATHS_SCHEMA, [
      pathRow("/team-mail", "gmail", "acme"),
      pathRow("", "gdrive", "acme"),
    ]),
  );
  return all([
    check(tagOf(declared), toBe("Unanswerable")),
    // and it says so, rather than showing the one row it liked
    check(itemsOf(declared).length, toBe(0)),
  ]);
});

test("qfs's own error shape is reported as itself", () =>
  check(
    matchDeclared<QueryPath, string>({
      declared: () => "",
      undeclared: () => "",
      unanswerable: (reason) => reason,
    })(
      asQueryPaths(
        ok({
          error: {
            code: "parse_error",
            message: "unexpected token",
          },
        }),
      ),
    ),
    toBe("qfs: unexpected token (parse_error)"),
  ));
