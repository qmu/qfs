import {
  test,
  check,
  all,
  toBe,
  toEqual,
} from "plgg-test";
import {
  type SoftStr,
  type Option,
  some,
  fromNullable,
  match,
  matchOption,
} from "plgg";
import {
  cmdNone,
  makeUrl,
} from "plgg-view/client";
import {
  row,
  field,
  fieldOf,
  numValue,
  refValue,
} from "plggmatic/Declare/model/Row";
import {
  sync,
  async,
} from "plggmatic/Declare/model/Source";
import {
  query,
  queryChoice,
} from "plggmatic/Declare/model/Query";
import {
  action,
  immediate,
  confirm,
} from "plggmatic/Declare/model/Action";
import { collection } from "plggmatic/Declare/model/Collection";
import {
  menu,
  menuEntry,
} from "plggmatic/Declare/model/Menu";
import { declare } from "plggmatic/Declare/model/Declaration";
import { actor } from "plggmatic/Declare/model/Adapter";
import {
  type SchedulerMsg,
  openMenu,
  select,
  requestAction,
} from "plggmatic/Schedule/model/Msg";
import { schedule } from "plggmatic/Schedule/usecase/schedule";
import {
  type Tool,
  type ToolInput,
  tool,
  nullary,
  emit,
  enumArg$,
  textArg$,
  nullary$,
  emit$,
  runFlow$,
} from "plggmatic/Catalog/model/tool";
import {
  catalogOf,
  rowCap,
} from "plggmatic/Catalog/usecase/catalog";

// --- fixture: a menu over a queryable/actionable list, a
// big list (over the row cap), a board, and a slow (async,
// forever-loading) list — the Demo-1-shaped acceptance
// surface for the fold.

type Client = Readonly<{
  id: SoftStr;
  label: SoftStr;
  tier: SoftStr;
}>;
const clients: ReadonlyArray<Client> = [
  { id: "c1", label: "Ada", tier: "Gold" },
  { id: "c2", label: "Ben", tier: "Silver" },
  { id: "c3", label: "Cid", tier: "Gold" },
];
const bigRows = Array.from(
  { length: 60 },
  (_unused: unknown, i: number) =>
    row(`b${i}`, `Big ${i}`),
);
const teamRows: ReadonlyArray<
  ReturnType<typeof row>
> = [
  row("t1", "Team A", [
    fieldOf(
      "Lead",
      refValue("clients", "c1", "Ada"),
    ),
  ]),
  row("t2", "Team B", [field("Lead", "none")]),
];

const decl = declare({
  title: "Demo",
  menu: menu([
    menuEntry("Clients", "clients"),
    menuEntry("Big", "big"),
    menuEntry("Teams", "teams"),
    menuEntry("Slow", "slow"),
  ]),
  collections: [
    collection<Client>({
      id: "clients",
      title: "Clients",
      toRow: (c: Client) =>
        row(c.id, c.label, [
          field("Tier", c.tier),
          fieldOf("Score", numValue("10")),
        ]),
      source: sync(() => clients),
      query: query("Find a client", [
        queryChoice("tier", "Tier", "Tier", [
          "Gold",
          "Silver",
        ]),
      ]),
      actions: [
        action({
          id: "new",
          label: "New client",
          verb: "create",
          confirm: immediate(),
          run: () => cmdNone(),
          authorize: (a) =>
            a.roles.includes("admin"),
        }),
        action({
          id: "edit",
          label: "Edit",
          verb: "update",
          confirm: immediate(),
          run: () => cmdNone(),
        }),
        action({
          id: "remove",
          label: "Delete",
          verb: "delete",
          confirm: confirm(
            "Delete this client?",
            true,
          ),
          run: () => cmdNone(),
        }),
      ],
    }),
    collection<ReturnType<typeof row>>({
      id: "big",
      title: "Big",
      toRow: (r) => r,
      source: sync(() => bigRows),
    }),
    collection<ReturnType<typeof row>>({
      id: "teams",
      title: "Teams",
      toRow: (r) => r,
      source: sync(() => teamRows),
      board: true,
    }),
    collection<ReturnType<typeof row>>({
      id: "slow",
      title: "Slow",
      toRow: (r) => r,
      source: async(
        () => new Promise(() => undefined),
      ),
    }),
    // reachable via openMenu (no menu entry needed) — the
    // edge levels the fold's guards handle.
    collection<ReturnType<typeof row>>({
      id: "empty",
      title: "Empty",
      toRow: (r) => r,
      source: sync(() => []),
    }),
    collection<ReturnType<typeof row>>({
      id: "flatboard",
      title: "Flat board",
      toRow: (r) => r,
      source: sync(() => [
        row("f1", "Flat 1"),
        row("f2", "Flat 2"),
      ]),
      board: true,
    }),
    collection<ReturnType<typeof row>>({
      id: "slowboard",
      title: "Slow board",
      toRow: (r) => r,
      source: async(
        () => new Promise(() => undefined),
      ),
      board: true,
    }),
  ],
});

const admin = schedule(decl, {
  actor: actor("u", ["admin"]),
});
const anon = schedule(decl);
const url0 = makeUrl("/", "");

/** The catalog at a scene reached by dispatching `msgs`. */
const catalogAfter = (
  s: typeof admin,
  msgs: ReadonlyArray<SchedulerMsg>,
): ReadonlyArray<Tool> => {
  const model = msgs.reduce(
    (m, msg) => s.update(msg, m)[0],
    s.init(url0)[0],
  );
  return catalogOf(s.scene(model));
};

const byName = (
  cat: ReadonlyArray<Tool>,
  name: SoftStr,
): Option<Tool> =>
  fromNullable(
    cat.find((t: Tool) => t.name === name),
  );

/** A sentinel tool a `need` miss returns (fails assertions loudly). */
const missingTool: Tool = tool({
  name: "__missing__",
  description: "__missing__",
  input: nullary(),
  effect: emit(() => openMenu("__missing__")),
});

/** The option set of an EnumArg input (empty otherwise). */
const enumOptions = (
  input: ToolInput,
): ReadonlyArray<SoftStr> =>
  match(input)(
    [
      nullary$(),
      (): ReadonlyArray<SoftStr> => [],
    ],
    [
      textArg$(),
      (): ReadonlyArray<SoftStr> => [],
    ],
    [
      enumArg$(),
      ({ content }): ReadonlyArray<SoftStr> =>
        content.options,
    ],
  );

/** The message an Emit tool lowers `arg` to (a sentinel otherwise). */
const emitMsg = (
  t: Tool,
  arg: SoftStr,
): SchedulerMsg =>
  match(t.effect)(
    [
      emit$(),
      ({ content }): SchedulerMsg => content(arg),
    ],
    [
      runFlow$(),
      (): SchedulerMsg => openMenu("__none__"),
    ],
  );

const has = (
  cat: ReadonlyArray<Tool>,
  name: SoftStr,
): boolean =>
  cat.some((t: Tool) => t.name === name);

const need = (
  cat: ReadonlyArray<Tool>,
  name: SoftStr,
): Tool =>
  matchOption<Tool, Tool>(
    () => missingTool,
    (t: Tool) => t,
  )(byName(cat, name));

test("the menu scene yields open_menu (section enum) and the standing run_flow", () => {
  const cat = catalogAfter(admin, []);
  return all([
    check(
      enumOptions(need(cat, "open_menu").input),
      toEqual([
        "Clients",
        "Big",
        "Teams",
        "Slow",
      ]),
    ),
    check(has(cat, "run_flow"), toBe(true)),
    // nothing else at the bare menu
    check(cat.length, toBe(2)),
  ]);
});

test("open_menu navigates to the chosen section (the human href path)", () => {
  const cat = catalogAfter(admin, []);
  return check(
    emitMsg(need(cat, "open_menu"), "Big").__tag,
    toBe("UrlChanged"),
  );
});

test("a list scene yields select (row-id enum equal to the rendered rows), filter, and the create tool", () => {
  const cat = catalogAfter(admin, [
    openMenu("clients"),
  ]);
  return all([
    check(
      enumOptions(
        need(cat, "select_clients").input,
      ),
      toEqual(["c1", "c2", "c3"]),
    ),
    check(has(cat, "filter_clients"), toBe(true)),
    check(
      enumOptions(
        need(cat, "filter_clients_tier").input,
      ),
      toEqual(["Gold", "Silver"]),
    ),
    check(has(cat, "clients_new"), toBe(true)),
    check(has(cat, "run_flow"), toBe(true)),
  ]);
});

test("select lowers to select(depth,id) at the list's flow depth", () =>
  check(
    emitMsg(
      need(
        catalogAfter(admin, [
          openMenu("clients"),
        ]),
        "select_clients",
      ),
      "c2",
    ),
    toEqual(select(0, "c2")),
  ));

test("the keyword filter and a declared choice lower to their own messages", () => {
  const cat = catalogAfter(admin, [
    openMenu("clients"),
  ]);
  return all([
    check(
      emitMsg(need(cat, "filter_clients"), "Ad")
        .__tag,
      toBe("QueryInput"),
    ),
    check(
      emitMsg(
        need(cat, "filter_clients_tier"),
        "Gold",
      ).__tag,
      toBe("QueryChoiceInput"),
    ),
  ]);
});

test("an action the actor may not run yields no tool (the legality projection)", () => {
  const withAdmin = catalogAfter(admin, [
    openMenu("clients"),
  ]);
  const withoutActor = catalogAfter(anon, [
    openMenu("clients"),
  ]);
  return all([
    // create is authorize-gated on the admin role
    check(
      has(withAdmin, "clients_new"),
      toBe(true),
    ),
    check(
      has(withoutActor, "clients_new"),
      toBe(false),
    ),
    // the un-gated create still appears via anon? no —
    // clients_new is the only create; edit/remove are
    // detail-level. anon keeps select + filters.
    check(
      has(withoutActor, "select_clients"),
      toBe(true),
    ),
  ]);
});

test("a still-loading collection contributes no tools", () => {
  const cat = catalogAfter(admin, [
    openMenu("slow"),
  ]);
  return all([
    check(has(cat, "select_slow"), toBe(false)),
    check(has(cat, "filter_slow"), toBe(false)),
    // only the menu tool and run_flow survive
    check(cat.length, toBe(2)),
  ]);
});

test("over the row cap the select choices are withheld (empty enum, filter guidance) — never a free string", () => {
  const cat = catalogAfter(admin, [
    openMenu("big"),
  ]);
  const sel = need(cat, "select_big");
  return all([
    // more than the cap of rows exist
    check(bigRows.length > rowCap, toBe(true)),
    // withheld: still an enum, but empty
    check(enumOptions(sel.input), toEqual([])),
    check(
      match(sel.input)(
        [nullary$(), (): string => "nullary"],
        [textArg$(), (): string => "text"],
        [enumArg$(), (): string => "enum"],
      ),
      toBe("enum"),
    ),
    check(
      sel.description.includes("filter"),
      toBe(true),
    ),
  ]);
});

test("a board level yields a jump tool over its jumpable tiles", () => {
  const cat = catalogAfter(admin, [
    openMenu("teams"),
  ]);
  return all([
    // only the tile carrying a reference jumps
    check(
      enumOptions(need(cat, "jump_teams").input),
      toEqual(["Team A"]),
    ),
    check(
      emitMsg(need(cat, "jump_teams"), "Team A")
        .__tag,
      toBe("UrlChanged"),
    ),
    // a board neither selects nor filters
    check(has(cat, "select_teams"), toBe(false)),
  ]);
});

test("a detail level yields the row-action tools targeted at the loaded row", () => {
  const cat = catalogAfter(admin, [
    openMenu("clients"),
    select(0, "c1"),
  ]);
  return all([
    check(has(cat, "clients_edit"), toBe(true)),
    check(has(cat, "clients_remove"), toBe(true)),
    check(
      emitMsg(need(cat, "clients_edit"), ""),
      toEqual(
        requestAction(
          "clients",
          "edit",
          some("c1"),
        ),
      ),
    ),
    check(
      emitMsg(need(cat, "clients_remove"), ""),
      toEqual(
        requestAction(
          "clients",
          "remove",
          some("c1"),
        ),
      ),
    ),
  ]);
});

test("a detail whose row is not loaded yields no action tools", () =>
  check(
    has(
      catalogAfter(admin, [
        openMenu("clients"),
        select(0, "ghost"),
      ]),
      "clients_edit",
    ),
    toBe(false),
  ));

test("a loading board contributes no tools", () => {
  const cat = catalogAfter(admin, [
    openMenu("slowboard"),
  ]);
  return all([
    check(
      has(cat, "jump_slowboard"),
      toBe(false),
    ),
    check(cat.length, toBe(2)),
  ]);
});

test("a board with no jumpable tiles yields no jump tool", () =>
  check(
    has(
      catalogAfter(admin, [
        openMenu("flatboard"),
      ]),
      "jump_flatboard",
    ),
    toBe(false),
  ));

test("a list with no visible rows yields no select tool", () =>
  check(
    has(
      catalogAfter(admin, [openMenu("empty")]),
      "select_empty",
    ),
    toBe(false),
  ));

test("byName is total (a miss is None)", () =>
  check(
    matchOption<Tool, boolean>(
      () => true,
      () => false,
    )(byName(catalogAfter(admin, []), "nope")),
    toBe(true),
  ));
