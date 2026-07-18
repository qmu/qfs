import {
  test,
  check,
  all,
  toBe,
  toEqual,
} from "plgg-test";
import {
  type SoftStr,
  ok,
  match,
  matchOption,
  fromNullable,
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
  flagValue,
  momentValue,
  refValue,
  mediaValue,
} from "plggmatic/Declare/model/Row";
import {
  sync,
  async,
  dynamic,
  adapter,
} from "plggmatic/Declare/model/Source";
import {
  query,
  queryChoice,
} from "plggmatic/Declare/model/Query";
import {
  action,
  confirm,
} from "plggmatic/Declare/model/Action";
import { collection } from "plggmatic/Declare/model/Collection";
import {
  menu,
  menuEntry,
} from "plggmatic/Declare/model/Menu";
import { declare } from "plggmatic/Declare/model/Declaration";
import { schedule } from "plggmatic/Schedule/usecase/schedule";
import {
  flowPaused$,
  flowDone$,
  flowFailed$,
} from "plggmatic/Flow/model/run";
import {
  type FlowValue,
  vStr,
  vList,
  vOk,
  vErr,
} from "plggmatic/Flow/model/script";
import {
  type CollectionSchema,
  collectionSchema,
  fieldIsNumeric,
  hasChoice,
} from "plggmatic/Flow/model/forms";
import {
  type Tool,
  emit$,
  runFlow$,
} from "plggmatic/Catalog/model/tool";
import { catalogOf } from "plggmatic/Catalog/usecase/catalog";
import {
  type RunOutcome,
  flowHost,
  flowSchemaOf,
  runFlow,
  ran$,
  rejected$,
  stalled$,
} from "plggmatic/Catalog/usecase/runFlow";

// --- the interpret.spec fixture: a sync, queryable
// sections collection with a numeric `hours` field.

type Sec = Readonly<{
  id: SoftStr;
  label: SoftStr;
  status: SoftStr;
  hours: number;
}>;
const secs: ReadonlyArray<Sec> = [
  {
    id: "a",
    label: "Alpha",
    status: "Active",
    hours: 3,
  },
  {
    id: "b",
    label: "Albion",
    status: "Idle",
    hours: 5,
  },
  {
    id: "c",
    label: "Beta",
    status: "Active",
    hours: 4,
  },
];
const decl = declare({
  title: "Demo",
  menu: menu([menuEntry("Sections", "sections")]),
  collections: [
    collection<Sec>({
      id: "sections",
      title: "Sections",
      toRow: (x: Sec) =>
        row(x.id, x.label, [
          field("Status", x.status),
          fieldOf(
            "hours",
            numValue(String(x.hours)),
          ),
        ]),
      source: sync(() => secs),
      query: query("Filter", [
        queryChoice(
          "status",
          "Status",
          "Status",
          ["Active", "Idle"],
        ),
      ]),
    }),
  ],
});
const s = schedule(decl);
const schema = flowSchemaOf(decl);
const url0 = makeUrl("/", "");
const host = () => flowHost(s, s.init(url0)[0]);

const findActive = `
  (flow find-active
    (dispatch (open-menu sections))
    (dispatch (query-input "Al"))
    (dispatch (query-choice status "Active"))
    (let rows (scene-rows sections))
    (match-empty rows (err not-found) rs (ok (map label rs))))`;

/** The RunOutcome kind (its box tag, folded). */
const kind = (r: RunOutcome): SoftStr =>
  match(r)(
    [ran$(), (): SoftStr => "ran"],
    [rejected$(), (): SoftStr => "rejected"],
    [stalled$(), (): SoftStr => "stalled"],
  );

/** The flow value of a Done run (a sentinel otherwise). */
const doneValue = (r: RunOutcome): FlowValue =>
  match(r)(
    [
      ran$(),
      ({ content }): FlowValue =>
        match(content)(
          [
            flowDone$(),
            ({ content: v }): FlowValue => v,
          ],
          [
            flowPaused$(),
            (): FlowValue => vErr("paused"),
          ],
          [
            flowFailed$(),
            (): FlowValue => vErr("failed"),
          ],
        ),
    ],
    [
      rejected$(),
      (): FlowValue => vErr("rejected"),
    ],
    [
      stalled$(),
      (): FlowValue => vErr("stalled"),
    ],
  );

/** The failure code of a Failed run (empty otherwise). */
const failCode = (r: RunOutcome): SoftStr =>
  match(r)(
    [
      ran$(),
      ({ content }): SoftStr =>
        match(content)(
          [
            flowFailed$(),
            ({ content: d }): SoftStr => d.code,
          ],
          [flowDone$(), (): SoftStr => ""],
          [flowPaused$(), (): SoftStr => ""],
        ),
    ],
    [rejected$(), (): SoftStr => ""],
    [stalled$(), (): SoftStr => ""],
  );

test("run_flow reads → checks → runs a spec §6 flow end-to-end to a value", () => {
  const out = runFlow(findActive, schema, host());
  return all([
    check(kind(out), toBe("ran")),
    // keyword "Al" + choice Active leave exactly Alpha
    check(
      doneValue(out),
      toEqual(vOk(vList([vStr("Alpha")]))),
    ),
  ]);
});

test("run_flow returns positioned diagnostics for a rejected script", () => {
  const out = runFlow(
    "(flow bad (frobnicate sections))",
    schema,
    host(),
  );
  return all([
    check(kind(out), toBe("rejected")),
    check(
      match(out)(
        [
          rejected$(),
          ({ content }): boolean =>
            content.length >= 1 &&
            content[0] !== undefined &&
            content[0].range.start.offset >= 0,
        ],
        [ran$(), (): boolean => false],
        [stalled$(), (): boolean => false],
      ),
      toBe(true),
    ),
  ]);
});

test("fuel exhaustion is a Failed outcome (never a throw)", () =>
  check(
    failCode(
      runFlow(findActive, schema, host(), 2),
    ),
    toBe("fuel-exhausted"),
  ));

// --- the stall path: a destructive action a flow dispatches
// parks a confirmation; run_flow stops there (no auto-
// confirm — mission point 8).

const withDelete = declare({
  title: "Docs",
  menu: menu([menuEntry("Docs", "docs")]),
  collections: [
    collection<ReturnType<typeof row>>({
      id: "docs",
      title: "Docs",
      toRow: (r) => r,
      source: sync(() => [row("d1", "Doc 1")]),
      actions: [
        action({
          id: "remove",
          label: "Delete",
          verb: "delete",
          confirm: confirm(
            "Delete this doc?",
            true,
          ),
          run: () => cmdNone(),
        }),
      ],
    }),
  ],
});

test("a destructive flow without an explicit confirm stalls at the parked confirmation", () => {
  const sd = schedule(withDelete);
  const out = runFlow(
    `(flow del
       (dispatch (open-menu docs))
       (dispatch (request-action docs remove))
       (ok (none)))`,
    flowSchemaOf(withDelete),
    flowHost(sd, sd.init(url0)[0]),
  );
  return all([
    check(kind(out), toBe("stalled")),
    check(
      match(out)(
        [
          stalled$(),
          ({ content }): SoftStr =>
            content.prompt,
        ],
        [ran$(), (): SoftStr => ""],
        [rejected$(), (): SoftStr => ""],
      ),
      toBe("Delete this doc?"),
    ),
    check(
      match(out)(
        [
          stalled$(),
          ({ content }): boolean =>
            content.destructive,
        ],
        [ran$(), (): boolean => false],
        [rejected$(), (): boolean => false],
      ),
      toBe(true),
    ),
  ]);
});

test("flowSchemaOf observes numeric fields and declared choices from a sync source", () => {
  const cs = matchOption<
    CollectionSchema,
    CollectionSchema
  >(
    () => ({ id: "", fields: [], choices: [] }),
    (x: CollectionSchema) => x,
  )(collectionSchema(schema, "sections"));
  return all([
    check(
      fieldIsNumeric(cs, "hours"),
      toBe(true),
    ),
    check(
      fieldIsNumeric(cs, "Status"),
      toBe(false),
    ),
    check(hasChoice(cs, "status"), toBe(true)),
    check(hasChoice(cs, "nope"), toBe(false)),
  ]);
});

test("flowSchemaOf covers non-sync sources and every field kind", () => {
  const mixed = declare({
    title: "Mixed",
    menu: menu([menuEntry("Kinds", "kinds")]),
    collections: [
      collection<ReturnType<typeof row>>({
        id: "kinds",
        title: "Kinds",
        toRow: (r) => r,
        source: sync(() => [
          row("k1", "K1", [
            fieldOf("f", flagValue(true)),
            fieldOf(
              "m",
              momentValue("2026-01-01"),
            ),
            fieldOf(
              "r",
              refValue("kinds", "k1", "K1"),
            ),
            fieldOf(
              "g",
              mediaValue("/a.png", "alt"),
            ),
            fieldOf("n", numValue("3")),
            field("t", "text"),
          ]),
        ]),
      }),
      collection<ReturnType<typeof row>>({
        id: "remote",
        title: "Remote",
        toRow: (r) => r,
        source: async(() =>
          Promise.resolve(ok([])),
        ),
        query: query("f", [
          queryChoice("q", "Q", "Q", ["x"]),
        ]),
      }),
      collection<ReturnType<typeof row>>({
        id: "dyn",
        title: "Dyn",
        toRow: (r) => r,
        source: dynamic(),
      }),
      collection<ReturnType<typeof row>>({
        id: "ad",
        title: "Ad",
        toRow: (r) => r,
        source: adapter("h"),
      }),
    ],
  });
  const sc = flowSchemaOf(mixed);
  const kinds = matchOption<
    CollectionSchema,
    CollectionSchema
  >(
    () => ({ id: "", fields: [], choices: [] }),
    (x: CollectionSchema) => x,
  )(collectionSchema(sc, "kinds"));
  const present = (id: SoftStr): boolean =>
    matchOption<CollectionSchema, boolean>(
      () => false,
      () => true,
    )(collectionSchema(sc, id));
  const remote = matchOption<
    CollectionSchema,
    CollectionSchema
  >(
    () => ({ id: "", fields: [], choices: [] }),
    (x: CollectionSchema) => x,
  )(collectionSchema(sc, "remote"));
  return all([
    // only the numeric cell is numeric; every other kind is not
    check(fieldIsNumeric(kinds, "n"), toBe(true)),
    check(
      fieldIsNumeric(kinds, "f"),
      toBe(false),
    ),
    check(
      fieldIsNumeric(kinds, "m"),
      toBe(false),
    ),
    check(
      fieldIsNumeric(kinds, "r"),
      toBe(false),
    ),
    check(
      fieldIsNumeric(kinds, "g"),
      toBe(false),
    ),
    check(
      fieldIsNumeric(kinds, "t"),
      toBe(false),
    ),
    // a non-sync source contributes no observed fields, but its
    // declared choice survives, and it stays a known collection
    check(remote.fields.length, toBe(0)),
    check(hasChoice(remote, "q"), toBe(true)),
    check(present("dyn"), toBe(true)),
    check(present("ad"), toBe(true)),
  ]);
});

test("the run_flow tool is sourced from the catalog (a RunFlow marker)", () =>
  check(
    matchOption<Tool, boolean>(
      () => false,
      (t: Tool) =>
        match(t.effect)(
          [runFlow$(), (): boolean => true],
          [emit$(), (): boolean => false],
        ),
    )(
      fromNullable(
        catalogOf(host().scene).find(
          (t: Tool) => t.name === "run_flow",
        ),
      ),
    ),
    toBe(true),
  ));
