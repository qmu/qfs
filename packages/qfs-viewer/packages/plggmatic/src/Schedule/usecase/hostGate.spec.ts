import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import {
  type SoftStr,
  type Option,
  type Result,
  ok,
  err,
  some,
  none,
  isSome,
  isNone,
  matchOption,
  match,
} from "plgg";
import {
  type Cmd,
  cmdNone,
  cmdEffect,
  cmdNone$,
  cmdBatch$,
  cmdEffect$,
} from "plgg-view/client";
import {
  type Row,
  row as makeRow,
} from "plggmatic/Declare/model/Row";
import {
  sync,
  adapter,
} from "plggmatic/Declare/model/Source";
import { collection } from "plggmatic/Declare/model/Collection";
import {
  type Declaration,
  declare,
} from "plggmatic/Declare/model/Declaration";
import {
  menu,
  menuEntry,
} from "plggmatic/Declare/model/Menu";
import { query } from "plggmatic/Declare/model/Query";
import {
  type Authorize,
  action,
  immediate,
  confirm,
} from "plggmatic/Declare/model/Action";
import {
  actor,
  named,
  hostAdapter,
} from "plggmatic/Declare/model/Adapter";
import {
  type SchedulerMsg,
  openMenu,
  select,
  requestAction,
  confirmAction,
  loaded,
} from "plggmatic/Schedule/model/Msg";
import {
  type Model,
  type Slot,
  emptyModel,
  slotOf,
  idle$,
  loading$,
  loadedSlot$,
  failedSlot$,
} from "plggmatic/Schedule/model/Model";
import {
  type Scene,
  type Level,
  menuLevel$,
  listLevel$,
  boardLevel$,
  detailLevel$,
} from "plggmatic/Schedule/model/Scene";
import { schedule } from "plggmatic/Schedule/usecase/schedule";

const hasRole =
  (role: SoftStr): Authorize =>
  (a) =>
    a.roles.includes(role);

// A sync-backed declaration so rows load immediately and a
// detail level can resolve its selected row (the authorize
// gate is independent of the source kind).
const decl: Declaration = declare({
  title: "CRM",
  menu: menu([menuEntry("Clients", "clients")]),
  collections: [
    collection<Row>({
      id: "clients",
      title: "Clients",
      toRow: (r: Row) => r,
      source: sync(() => [
        makeRow("acme", "ACME"),
        makeRow("beta", "Beta"),
      ]),
      query: query("Filter"),
      actions: [
        action({
          id: "new",
          label: "New",
          verb: "create",
          confirm: immediate(),
          run: () => cmdNone(),
          authorize: hasRole("editor"),
        }),
        action({
          id: "del",
          label: "Delete",
          verb: "delete",
          confirm: confirm("Delete?", true),
          run: () => cmdNone(),
          authorize: hasRole("admin"),
        }),
      ],
    }),
  ],
});

const editor = schedule(decl, {
  actor: actor("u", ["editor"]),
});
const admin = schedule(decl, {
  actor: actor("a", ["admin", "editor"]),
});
const anon = schedule(decl);

const step = (
  s: typeof editor,
  msg: SchedulerMsg,
  m: Model,
): Model => s.update(msg, m)[0];

const listActionIds = (
  scene: Scene,
): ReadonlyArray<SoftStr> =>
  scene.levels.flatMap((l: Level) =>
    match(l)(
      [
        menuLevel$(),
        (): ReadonlyArray<SoftStr> => [],
      ],
      [
        listLevel$(),
        ({ content }): ReadonlyArray<SoftStr> =>
          content.actions.map((a) => a.id),
      ],
      [
        boardLevel$(),
        (): ReadonlyArray<SoftStr> => [],
      ],
      [
        detailLevel$(),
        (): ReadonlyArray<SoftStr> => [],
      ],
    ),
  );

const detailActionIds = (
  scene: Scene,
): ReadonlyArray<SoftStr> =>
  scene.levels.flatMap((l: Level) =>
    match(l)(
      [
        menuLevel$(),
        (): ReadonlyArray<SoftStr> => [],
      ],
      [
        listLevel$(),
        (): ReadonlyArray<SoftStr> => [],
      ],
      [
        boardLevel$(),
        (): ReadonlyArray<SoftStr> => [],
      ],
      [
        detailLevel$(),
        ({ content }): ReadonlyArray<SoftStr> =>
          content.actions.map((a) => a.id),
      ],
    ),
  );

test("the Scene hides a create action the actor may not run", () => {
  const m0 = emptyModel("/");
  const atList = (
    s: typeof editor,
  ): ReadonlyArray<SoftStr> =>
    listActionIds(
      s.scene(step(s, openMenu("clients"), m0)),
    );
  return all([
    // editor may create → the button shows
    check(atList(editor).join(","), toBe("new")),
    // no actor + a declared policy → denied → no button
    check(atList(anon).length, toBe(0)),
  ]);
});

test("the Scene hides a delete action for a non-admin at the detail", () => {
  const m0 = emptyModel("/");
  const detailOf = (
    s: typeof editor,
  ): ReadonlyArray<SoftStr> => {
    const m1 = step(s, openMenu("clients"), m0);
    const m2 = step(s, select(0, "acme"), m1);
    return detailActionIds(s.scene(m2));
  };
  return all([
    // editor is not admin → delete hidden
    check(detailOf(editor).length, toBe(0)),
    // admin → delete shows
    check(detailOf(admin).join(","), toBe("del")),
  ]);
});

test("the engine gate blocks an unauthorized action from dispatching", () => {
  const m0 = emptyModel("/");
  const m2e = step(
    editor,
    select(0, "acme"),
    step(editor, openMenu("clients"), m0),
  );
  // editor requests delete (admin-only) → total no-op: no
  // parked confirmation, nothing dispatched.
  const blocked = step(
    editor,
    requestAction("clients", "del", some("acme")),
    m2e,
  );
  // admin requests the same → the confirmation parks.
  const m2a = step(
    admin,
    select(0, "acme"),
    step(admin, openMenu("clients"), m0),
  );
  const parked = step(
    admin,
    requestAction("clients", "del", some("acme")),
    m2a,
  );
  return all([
    check(isNone(blocked.pending), toBe(true)),
    check(isSome(parked.pending), toBe(true)),
  ]);
});

// --- Adapter source: read path + reconciliation ----------

const itemsDecl: Declaration = declare({
  title: "Items",
  menu: menu([menuEntry("Items", "items")]),
  collections: [
    collection<Row>({
      id: "items",
      title: "Items",
      toRow: (r: Row) => r,
      source: adapter<Row>(),
    }),
  ],
});

const stubApply = () =>
  Promise.resolve(
    ok({ collection: "items", rows: [] }),
  );

const scheduleWith = (
  read: () => Promise<
    Result<ReadonlyArray<Row>, Error>
  >,
) =>
  schedule(itemsDecl, {
    adapters: [
      named(
        "db",
        hostAdapter({ read, apply: stubApply }),
      ),
    ],
  });

/** The first `cmdEffect` thunk reachable in a Cmd tree. */
const firstEffect = (
  cmd: Cmd<SchedulerMsg>,
): Option<() => Promise<SchedulerMsg>> =>
  match(cmd)(
    [
      cmdNone$(),
      (): Option<() => Promise<SchedulerMsg>> =>
        none(),
    ],
    [
      cmdEffect$(),
      ({
        content,
      }): Option<() => Promise<SchedulerMsg>> =>
        some(content),
    ],
    [
      cmdBatch$(),
      ({
        content,
      }): Option<() => Promise<SchedulerMsg>> =>
        content.reduce<
          Option<() => Promise<SchedulerMsg>>
        >(
          (acc, c) =>
            isSome(acc) ? acc : firstEffect(c),
          none(),
        ),
    ],
  );

/** Opens the adapter collection and resolves its settled slot. */
const openItems = (
  read: () => Promise<
    Result<ReadonlyArray<Row>, Error>
  >,
): Promise<Slot> => {
  const s = scheduleWith(read);
  const [m1, cmd] = s.update(
    openMenu("items"),
    emptyModel("/"),
  );
  return matchOption<
    () => Promise<SchedulerMsg>,
    Promise<Slot>
  >(
    () => Promise.resolve(slotOf(m1, "items")),
    (thunk) =>
      thunk().then((msg) =>
        slotOf(s.update(msg, m1)[0], "items"),
      ),
  )(firstEffect(cmd));
};

test("an adapter read parks Loading before its effect runs", () => {
  const s = scheduleWith(() =>
    Promise.resolve(ok([makeRow("i1", "One")])),
  );
  const [m1] = s.update(
    openMenu("items"),
    emptyModel("/"),
  );
  return check(
    match(slotOf(m1, "items"))(
      [idle$(), (): boolean => false],
      [loading$(), (): boolean => true],
      [loadedSlot$(), (): boolean => false],
      [failedSlot$(), (): boolean => false],
    ),
    toBe(true),
  );
});

test("an adapter read Ok settles a Loaded slot", async () =>
  check(
    match(
      await openItems(() =>
        Promise.resolve(
          ok([makeRow("i1", "One")]),
        ),
      ),
    )(
      [
        loadedSlot$(),
        ({ content }): number => content.length,
      ],
      [failedSlot$(), (): number => -1],
      [loading$(), (): number => -2],
    ),
    toBe(1),
  ));

test("an adapter read Err settles a Failed slot (failure as a value)", async () =>
  check(
    match(
      await openItems(() =>
        Promise.resolve(err(new Error("down"))),
      ),
    )(
      [
        failedSlot$(),
        ({ content }): SoftStr => content,
      ],
      [loadedSlot$(), (): SoftStr => ""],
      [loading$(), (): SoftStr => ""],
    ),
    toBe("down"),
  ));

test("schedule surfaces reconciliation diagnostics for adapter bindings", () =>
  all([
    // no registry → the unnamed default cannot resolve
    check(
      schedule(itemsDecl).diagnostics.length,
      toBe(1),
    ),
    // one registration → the default resolves, no findings
    check(
      scheduleWith(() => Promise.resolve(ok([])))
        .diagnostics.length,
      toBe(0),
    ),
  ]));

test("a named source with no matching registration settles a Failed slot, not a throw", () => {
  const ghost: Declaration = declare({
    title: "Items",
    menu: menu([menuEntry("Items", "items")]),
    collections: [
      collection<Row>({
        id: "items",
        title: "Items",
        toRow: (r: Row) => r,
        source: adapter<Row>("ghost"),
      }),
    ],
  });
  const s = schedule(ghost, {
    adapters: [
      named(
        "db",
        hostAdapter({
          read: () => Promise.resolve(ok([])),
          apply: stubApply,
        }),
      ),
    ],
  });
  const [m1] = s.update(
    openMenu("items"),
    emptyModel("/"),
  );
  return check(
    match(slotOf(m1, "items"))(
      [idle$(), (): SoftStr => ""],
      [loading$(), (): SoftStr => ""],
      [loadedSlot$(), (): SoftStr => ""],
      [
        failedSlot$(),
        ({ content }): SoftStr => content,
      ],
    ),
    toBe("unresolved host adapter"),
  );
});

test("the confirm-time re-check blocks a policy that flips to deny", () => {
  let calls = 0;
  const flip: Authorize = () => {
    calls = calls + 1;
    // permitted at request, denied at the confirm re-check
    return calls <= 1;
  };
  const d: Declaration = declare({
    title: "F",
    menu: menu([menuEntry("C", "c")]),
    collections: [
      collection<Row>({
        id: "c",
        title: "C",
        toRow: (r: Row) => r,
        source: sync(() => [makeRow("x", "X")]),
        actions: [
          action({
            id: "del",
            label: "Del",
            verb: "delete",
            confirm: confirm("?", true),
            run: () =>
              cmdEffect(() =>
                Promise.resolve(loaded("c", [])),
              ),
            authorize: flip,
          }),
        ],
      }),
    ],
  });
  const s = schedule(d, {
    actor: actor("u", ["any"]),
  });
  const m0 = emptyModel("/");
  const m2 = step(
    s,
    select(0, "x"),
    step(s, openMenu("c"), m0),
  );
  const parked = step(
    s,
    requestAction("c", "del", some("x")),
    m2,
  );
  const [after, cmd] = s.update(
    confirmAction(),
    parked,
  );
  return all([
    check(isSome(parked.pending), toBe(true)),
    check(isNone(after.pending), toBe(true)),
    // denied at confirm → nothing dispatched (cmdNone)
    check(
      match(cmd)(
        [cmdNone$(), (): boolean => true],
        [cmdBatch$(), (): boolean => false],
        [cmdEffect$(), (): boolean => false],
      ),
      toBe(true),
    ),
  ]);
});
