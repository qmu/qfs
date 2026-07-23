import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import {
  type Option,
  ok,
  some,
  none,
  isSome,
  isNone,
  getOr,
  matchOption,
  pipe,
} from "plgg";
import {
  type HostAdapter,
  type Registry,
  actor,
  scope,
  effect,
  hostAdapter,
  named,
  resolveAdapter,
} from "plggmatic/Declare/model/Adapter";

const stub = (): HostAdapter =>
  hostAdapter({
    read: () => Promise.resolve(ok([])),
    apply: () =>
      Promise.resolve(
        ok({ collection: "x", rows: [] }),
      ),
  });

const isThe = (
  target: HostAdapter,
  resolved: Option<HostAdapter>,
): boolean =>
  matchOption<HostAdapter, boolean>(
    () => false,
    (a: HostAdapter) => a === target,
  )(resolved);

test("a single registration resolves as the default (unnamed source)", () => {
  const a = stub();
  const reg: Registry = [named("only", a)];
  return check(
    isThe(a, resolveAdapter(reg, none())),
    toBe(true),
  );
});

test("a named source resolves the matching registration", () => {
  const a = stub();
  const b = stub();
  const reg: Registry = [
    named("crm", a),
    named("billing", b),
  ];
  return all([
    check(
      isThe(
        b,
        resolveAdapter(reg, some("billing")),
      ),
      toBe(true),
    ),
    check(
      isThe(a, resolveAdapter(reg, some("crm"))),
      toBe(true),
    ),
  ]);
});

test("an unknown name resolves to None", () =>
  check(
    isNone(
      resolveAdapter(
        [named("crm", stub())],
        some("nope"),
      ),
    ),
    toBe(true),
  ));

test("an unnamed source resolves to None when the registry is not exactly one", () =>
  all([
    check(
      isNone(resolveAdapter([], none())),
      toBe(true),
    ),
    check(
      isNone(
        resolveAdapter(
          [
            named("a", stub()),
            named("b", stub()),
          ],
          none(),
        ),
      ),
      toBe(true),
    ),
  ]));

test("actor / scope / effect constructors carry their data", () => {
  const ac = actor("u1", ["admin"]);
  const sc = scope("clients", ["acme"]);
  const ef = effect({
    collection: "clients",
    verb: "delete",
    target: some("acme"),
  });
  return all([
    check(ac.id, toBe("u1")),
    check(ac.roles.length, toBe(1)),
    check(actor("u2").roles.length, toBe(0)),
    check(sc.collection, toBe("clients")),
    check(sc.path.length, toBe(1)),
    check(ef.verb, toBe("delete")),
    check(ef.payload.length, toBe(0)),
    check(
      pipe(ef.target, getOr("")),
      toBe("acme"),
    ),
    check(isSome(ef.target), toBe(true)),
  ]);
});
