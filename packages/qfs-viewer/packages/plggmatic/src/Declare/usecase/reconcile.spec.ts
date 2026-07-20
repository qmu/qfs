import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import { type SoftStr, ok, match } from "plgg";
import { type Row } from "plggmatic/Declare/model/Row";
import { adapter } from "plggmatic/Declare/model/Source";
import { collection } from "plggmatic/Declare/model/Collection";
import {
  type Declaration,
  declare,
} from "plggmatic/Declare/model/Declaration";
import {
  menu,
  menuEntry,
} from "plggmatic/Declare/model/Menu";
import {
  type Registry,
  hostAdapter,
  named,
} from "plggmatic/Declare/model/Adapter";
import {
  type Diagnostic,
  reconcile,
  diagnosticSeverity,
  unknownAdapter$,
  noDefaultAdapter$,
  unusedAdapter$,
} from "plggmatic/Declare/usecase/reconcile";

const stubReg = (
  ...names: ReadonlyArray<SoftStr>
): Registry =>
  names.map((n) =>
    named(
      n,
      hostAdapter({
        read: () => Promise.resolve(ok([])),
        apply: () =>
          Promise.resolve(
            ok({ collection: n, rows: [] }),
          ),
      }),
    ),
  );

const decl = (
  id: SoftStr,
  source: ReturnType<typeof adapter<Row>>,
): Declaration =>
  declare({
    title: "T",
    menu: menu([menuEntry("E", id)]),
    collections: [
      collection<Row>({
        id,
        title: id,
        toRow: (r: Row) => r,
        source,
      }),
    ],
  });

const tag = (d: Diagnostic): SoftStr =>
  match(d)(
    [unknownAdapter$(), (): SoftStr => "unknown"],
    [
      noDefaultAdapter$(),
      (): SoftStr => "no-default",
    ],
    [unusedAdapter$(), (): SoftStr => "unused"],
  );

test("a named source no host registered is an unknown-adapter error", () => {
  const ds = reconcile(
    decl("clients", adapter<Row>("crm")),
    stubReg(),
  );
  return all([
    check(ds.length, toBe(1)),
    check(ds.map(tag).join(","), toBe("unknown")),
    check(
      ds.map(diagnosticSeverity).join(","),
      toBe("error"),
    ),
  ]);
});

test("an unnamed source with no sole default is a no-default error", () => {
  const ds = reconcile(
    decl("clients", adapter<Row>()),
    stubReg(),
  );
  return all([
    check(ds.length, toBe(1)),
    check(
      ds.map(tag).join(","),
      toBe("no-default"),
    ),
    check(
      ds.map(diagnosticSeverity).join(","),
      toBe("error"),
    ),
  ]);
});

test("a registration no collection reads through is an unused warning", () => {
  // one collection uses the default (the sole USED name is
  // none); a second registration is therefore unused. Two
  // registrations means the unnamed source is a no-default
  // error too, so filter to the unused warning.
  const ds = reconcile(
    decl("clients", adapter<Row>("crm")),
    stubReg("crm", "ghost"),
  );
  const unused = ds.filter(
    (d) => tag(d) === "unused",
  );
  return all([
    check(unused.length, toBe(1)),
    check(
      unused.map(diagnosticSeverity).join(","),
      toBe("warning"),
    ),
  ]);
});

test("a sound single-default binding yields no diagnostics", () =>
  check(
    reconcile(
      decl("clients", adapter<Row>()),
      stubReg("only"),
    ).length,
    toBe(0),
  ));
