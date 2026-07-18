import {
  test,
  check,
  all,
  toBe,
  toEqual,
} from "plgg-test";
import {
  type SoftStr,
  type Result,
  isOk,
  isErr,
  match,
} from "plgg";
import { makeUrl } from "plgg-view/client";
import {
  row,
  field,
  fieldOf,
  numValue,
} from "plggmatic/Declare/model/Row";
import { sync } from "plggmatic/Declare/model/Source";
import {
  query,
  queryChoice,
} from "plggmatic/Declare/model/Query";
import { collection } from "plggmatic/Declare/model/Collection";
import {
  menu,
  menuEntry,
} from "plggmatic/Declare/model/Menu";
import { declare } from "plggmatic/Declare/model/Declaration";
import { schedule } from "plggmatic/Schedule/usecase/schedule";
import {
  type FlowScript,
  vStr,
  vNum,
  vList,
  vOk,
  vErr,
  vSome,
} from "plggmatic/Flow/model/script";
import { flowSchema } from "plggmatic/Flow/model/forms";
import {
  codeUnknownForm,
  codeUnknownName,
  codeArityMismatch,
  codeTypeMismatch,
} from "plgg-ir-language";
import { readFlow } from "plggmatic/Flow/usecase/read";
import {
  startFlow,
  resumeFlow,
} from "plggmatic/Flow/usecase/interpret";
import {
  type FlowOutcome,
  flowPaused$,
  flowDone$,
  flowFailed$,
  defaultFuel,
} from "plggmatic/Flow/model/run";

// --- the manifest-shaped schema the reader checks against:
// two collections, timesheets with a numeric `hours` field,
// and a declared `status` choice on both.

const schema = flowSchema([
  {
    id: "clients",
    fields: [],
    choices: ["status"],
  },
  {
    id: "timesheets",
    fields: [{ kw: "hours", numeric: true }],
    choices: [],
  },
]);

// --- a real derived scheduler over the same shape, to
// prove a read flow runs end-to-end through the interpreter.

type Client = Readonly<{
  id: SoftStr;
  label: SoftStr;
  status: SoftStr;
}>;
type Sheet = Readonly<{
  id: SoftStr;
  label: SoftStr;
  hours: number;
}>;
const clients: ReadonlyArray<Client> = [
  { id: "acme", label: "ACME", status: "Active" },
  {
    id: "beacon",
    label: "Beacon",
    status: "Active",
  },
  {
    id: "cobalt",
    label: "Cobalt",
    status: "Idle",
  },
];
const sheets: ReadonlyArray<Sheet> = [
  { id: "w1", label: "Week 1", hours: 40 },
  { id: "w2", label: "Week 2", hours: 32 },
];
const decl = declare({
  title: "Demo",
  menu: menu([
    menuEntry("Clients", "clients"),
    menuEntry("Timesheets", "timesheets"),
  ]),
  collections: [
    collection<Client>({
      id: "clients",
      title: "Clients",
      toRow: (c: Client) =>
        row(c.id, c.label, [
          field("Status", c.status),
        ]),
      source: sync(() => clients),
      query: query("Filter", [
        queryChoice(
          "status",
          "Status",
          "Status",
          ["Active", "Idle"],
        ),
      ]),
    }),
    collection<Sheet>({
      id: "timesheets",
      title: "Timesheets",
      toRow: (s: Sheet) =>
        row(s.id, s.label, [
          fieldOf(
            "hours",
            numValue(String(s.hours)),
          ),
        ]),
      source: sync(() => sheets),
    }),
  ],
});
const sch = schedule(decl);
const url0 = makeUrl("/", "");

/** Drives a read flow through the real scheduler to done. */
const runOutcome = (
  script: FlowScript,
): FlowOutcome => {
  const step = (
    model: ReturnType<typeof sch.init>[0],
    out: FlowOutcome,
  ): FlowOutcome =>
    match(out)(
      [
        flowPaused$(),
        ({ content }): FlowOutcome => {
          const next = sch.update(
            content.msg,
            model,
          )[0];
          return step(
            next,
            resumeFlow(content, sch.scene(next)),
          );
        },
      ],
      [flowDone$(), (): FlowOutcome => out],
      [flowFailed$(), (): FlowOutcome => out],
    );
  const [m0] = sch.init(url0);
  return step(
    m0,
    startFlow(script, sch.scene(m0), defaultFuel),
  );
};

const doneOf = (o: FlowOutcome) =>
  match(o)(
    [flowDone$(), ({ content }) => content],
    [flowPaused$(), () => vErr("not-done")],
    [flowFailed$(), () => vErr("not-done")],
  );

const okScript = (
  r: Result<FlowScript, unknown>,
): FlowScript =>
  isOk(r)
    ? r.content
    : (() => {
        throw new Error("expected Ok");
      })();

const errCode = (
  r: Result<
    unknown,
    ReadonlyArray<{ code: SoftStr }>
  >,
): SoftStr =>
  isErr(r) ? (r.content[0]?.code ?? "") : "";

// --- the three worked spec §6 flows read → check green ---

test("flow 1 (search and read) reads and runs to Ok of labels", () => {
  const src = `
    (flow find-active
      (dispatch (open-menu clients))
      (dispatch (query-input "ea"))
      (dispatch (query-choice status "Active"))
      (let rows (scene-rows clients))
      (match-empty rows (err not-found) rs (ok (map label rows))))`;
  const r = readFlow(src, schema);
  return all([
    check(isOk(r), toBe(true)),
    // "ea" matches Beacon only; status Active keeps it
    check(
      doneOf(runOutcome(okScript(r))),
      toEqual(vOk(vList([vStr("Beacon")]))),
    ),
  ]);
});

test("flow 2 (dashboard headline) reads and folds an option row", () => {
  const src = `
    (flow headline
      (dispatch (open-menu clients))
      (let rows (scene-rows clients))
      (match-option (first rows) (err empty) r (ok (get Status r))))`;
  const r = readFlow(src, schema);
  return all([
    check(isOk(r), toBe(true)),
    check(
      doneOf(runOutcome(okScript(r))),
      toEqual(vOk(vSome(vStr("Active")))),
    ),
  ]);
});

test("flow 3 (sum unbilled hours) reads a numeric projection and sums", () => {
  const src = `
    (flow unbilled-total
      (dispatch (open-menu timesheets))
      (ok (sum (map hours (scene-rows timesheets)))))`;
  const r = readFlow(src, schema);
  return all([
    check(isOk(r), toBe(true)),
    check(
      doneOf(runOutcome(okScript(r))),
      toEqual(vOk(vNum(72))),
    ),
  ]);
});

// --- the required positioned rejections ---

test("an unknown collection is rejected", () =>
  check(
    errCode(
      readFlow(
        "(flow f (ok (scene-rows nope)))",
        schema,
      ),
    ),
    toBe(codeUnknownName),
  ));

test("an unknown query choice is rejected", () =>
  check(
    errCode(
      readFlow(
        `(flow f
           (dispatch (open-menu clients))
           (dispatch (query-choice nope "x"))
           (ok (scene-rows clients)))`,
        schema,
      ),
    ),
    toBe(codeUnknownName),
  ));

test("sum over a string projection is a type error", () =>
  check(
    errCode(
      readFlow(
        `(flow f
           (dispatch (open-menu clients))
           (ok (sum (map label (scene-rows clients)))))`,
        schema,
      ),
    ),
    toBe(codeTypeMismatch),
  ));

test("a non-exhaustive fold shape (match-option on a list) is a type error", () =>
  check(
    errCode(
      readFlow(
        `(flow f
           (dispatch (open-menu clients))
           (match-option (scene-rows clients) (err x) r (ok r)))`,
        schema,
      ),
    ),
    toBe(codeTypeMismatch),
  ));

test("an unknown form or function is rejected", () =>
  check(
    errCode(
      readFlow(
        "(flow f (ok (frobnicate (scene-rows clients))))",
        schema,
      ),
    ),
    toBe(codeUnknownForm),
  ));

test("a user-defined fn is rejected (not in the closed v1 set)", () =>
  check(
    errCode(
      readFlow("(flow f (fn x (ok x)))", schema),
    ),
    toBe(codeUnknownForm),
  ));

test("a syntax error surfaces as a positioned diagnostic", () =>
  check(
    isErr(readFlow("(flow f (ok", schema)),
    toBe(true),
  ));

test("arity is checked", () =>
  check(
    errCode(
      readFlow(
        "(flow f (ok (first (scene-rows clients) extra)))",
        schema,
      ),
    ),
    toBe(codeArityMismatch),
  ));

// --- more positioned rejections, covering the reader's
// structural error branches.

test("map without a keyword, get on a non-row, and count on a non-list are type/form errors", () =>
  all([
    check(
      isErr(
        readFlow(
          "(flow f (ok (get Status (scene-rows clients))))",
          schema,
        ),
      ),
      toBe(true),
    ),
    check(
      isErr(
        readFlow(
          "(flow f (ok (count (ok (scene-rows clients)))))",
          schema,
        ),
      ),
      toBe(true),
    ),
    check(
      isErr(
        readFlow(
          "(flow f (ok (map hours (ok (scene-rows clients)))))",
          schema,
        ),
      ),
      toBe(true),
    ),
  ]));

test("scene-rows without a name, and none with an argument, are rejected", () =>
  all([
    check(
      isErr(
        readFlow(
          '(flow f (ok (scene-rows "x")))',
          schema,
        ),
      ),
      toBe(true),
    ),
    check(
      isErr(
        readFlow(
          "(flow f (ok (none extra)))",
          schema,
        ),
      ),
      toBe(true),
    ),
  ]));

test("a malformed top level or empty body is rejected", () =>
  all([
    check(
      isErr(readFlow('"just a string"', schema)),
      toBe(true),
    ),
    check(
      isErr(
        readFlow("(notflow f (ok x))", schema),
      ),
      toBe(true),
    ),
    check(
      isErr(readFlow("(flow f)", schema)),
      toBe(true),
    ),
    check(
      isErr(readFlow("(flow)", schema)),
      toBe(true),
    ),
    check(
      isErr(
        readFlow(
          "(flow f (ok x) (ok x))",
          schema,
        ),
      ),
      toBe(true),
    ),
  ]));

test("a malformed step or dispatch message is rejected", () =>
  all([
    check(
      isErr(
        readFlow(
          "(flow f (nope a) (ok (scene-rows clients)))",
          schema,
        ),
      ),
      toBe(true),
    ),
    check(
      isErr(
        readFlow(
          "(flow f (dispatch (teleport clients)) (ok (scene-rows clients)))",
          schema,
        ),
      ),
      toBe(true),
    ),
    check(
      isErr(
        readFlow(
          '(flow f (dispatch (open-menu "notasym")) (ok (scene-rows clients)))',
          schema,
        ),
      ),
      toBe(true),
    ),
    check(
      isErr(
        readFlow(
          "(flow f (let 5 (scene-rows clients)) (ok x))",
          schema,
        ),
      ),
      toBe(true),
    ),
  ]));

test("select and request-action messages read", () => {
  const src = `
    (flow nav
      (dispatch (open-menu clients))
      (dispatch (select 0 "acme"))
      (dispatch (request-action clients del))
      (ok (scene-rows clients)))`;
  return check(
    isOk(readFlow(src, schema)),
    toBe(true),
  );
});

test("err and some and none forms read and check", () => {
  const src = `
    (flow ctors
      (dispatch (open-menu clients))
      (match-option (first (scene-rows clients))
        (ok (none))
        r
        (ok (some (get label r)))))`;
  return check(
    isOk(readFlow(src, schema)),
    toBe(true),
  );
});

test("keyword/name-position and arity error arms are covered", () =>
  all(
    [
      '(flow f (ok (map "x" (scene-rows clients))))',
      '(flow f (dispatch (open-menu clients)) (let r (first (scene-rows clients))) (ok (get "x" r)))',
      "(flow f (ok (scene-rows 5)))",
      '(flow f (ok (err "x")))',
      "(flow f (ok (err)))",
      "(flow f (ok (sum " +
        "(scene-rows clients) extra)))",
      "(flow f (ok (first)))",
      "(flow f (dispatch (open-menu clients)) (match-empty (scene-rows clients) (err e) 5 (ok x)))",
      "(flow f (dispatch (open-menu clients)) (match-option (first (scene-rows clients)) (err e) 5 (ok x)))",
    ].map((src) =>
      check(
        isErr(readFlow(src, schema)),
        toBe(true),
      ),
    ),
  ));

test("malformed dispatch message shapes are covered", () =>
  all(
    [
      "(flow f (dispatch foo) (ok (scene-rows clients)))",
      "(flow f (dispatch (open-menu)) (ok (scene-rows clients)))",
      "(flow f (dispatch (query-input status)) (ok (scene-rows clients)))",
      "(flow f (dispatch (query-choice status 5)) (ok (scene-rows clients)))",
      '(flow f (dispatch (query-choice "x" "y")) (ok (scene-rows clients)))',
      '(flow f (dispatch (select "x" "y")) (ok (scene-rows clients)))',
      "(flow f (dispatch (request-action clients 5)) (ok (scene-rows clients)))",
      "(flow f (let r) (ok r))",
      "(flow f (let) (ok x))",
      "(flow f 5 (ok (scene-rows clients)))",
    ].map((src) =>
      check(
        isErr(readFlow(src, schema)),
        toBe(true),
      ),
    ),
  ));

test("id and label keyword projections read and run", () => {
  const src = `
    (flow ids
      (dispatch (open-menu clients))
      (ok (map id (scene-rows clients))))`;
  const labelGet = `
    (flow lbl
      (dispatch (open-menu clients))
      (match-option (first (scene-rows clients))
        (err e) r (ok (get label r))))`;
  return all([
    check(
      doneOf(
        runOutcome(
          okScript(readFlow(src, schema)),
        ),
      ),
      toEqual(
        vOk(
          vList([
            vStr("acme"),
            vStr("beacon"),
            vStr("cobalt"),
          ]),
        ),
      ),
    ),
    check(
      doneOf(
        runOutcome(
          okScript(readFlow(labelGet, schema)),
        ),
      ),
      toEqual(vOk(vSome(vStr("ACME")))),
    ),
  ]);
});

test("short/empty dispatch message arg lists are covered", () =>
  all(
    [
      "(flow f (dispatch (query-choice)) (ok (scene-rows clients)))",
      "(flow f (dispatch (query-choice status)) (ok (scene-rows clients)))",
      "(flow f (dispatch (select 0)) (ok (scene-rows clients)))",
      "(flow f (dispatch (request-action clients)) (ok (scene-rows clients)))",
      "(flow f (dispatch ()) (ok (scene-rows clients)))",
      "(flow f (dispatch (query-input)) (ok (scene-rows clients)))",
    ].map((src) =>
      check(
        isErr(readFlow(src, schema)),
        toBe(true),
      ),
    ),
  ));

test("count, and get on a numeric field, read and run", () => {
  const cnt = `
    (flow c
      (dispatch (open-menu clients))
      (ok (count (scene-rows clients))))`;
  const getNum = `
    (flow g
      (dispatch (open-menu timesheets))
      (match-option (first (scene-rows timesheets))
        (err e) r (ok (get hours r))))`;
  return all([
    check(
      doneOf(
        runOutcome(
          okScript(readFlow(cnt, schema)),
        ),
      ),
      toEqual(vOk(vNum(3))),
    ),
    check(
      doneOf(
        runOutcome(
          okScript(readFlow(getNum, schema)),
        ),
      ),
      toEqual(vOk(vSome(vNum(40)))),
    ),
  ]);
});

test("remaining message-shape branches are covered", () =>
  all(
    [
      "(flow f (dispatch (select 0 5)) (ok (scene-rows clients)))",
      "(flow f (dispatch (request-action 5 del)) (ok (scene-rows clients)))",
      '(flow f (dispatch (query-choice 5 "x")) (ok (scene-rows clients)))',
    ].map((src) =>
      check(
        isErr(readFlow(src, schema)),
        toBe(true),
      ),
    ),
  ));

test("literal expressions (number, string) read and run", () => {
  const n = "(flow f (ok 5))";
  const s = '(flow f (ok "hi"))';
  return all([
    check(
      doneOf(
        runOutcome(okScript(readFlow(n, schema))),
      ),
      toEqual(vOk(vNum(5))),
    ),
    check(
      doneOf(
        runOutcome(okScript(readFlow(s, schema))),
      ),
      toEqual(vOk(vStr("hi"))),
    ),
  ]);
});

test("a non-expression atom and a non-symbol form head are rejected", () =>
  all([
    check(
      isErr(
        readFlow("(flow f (ok true))", schema),
      ),
      toBe(true),
    ),
    check(
      isErr(
        readFlow("(flow f (ok (())))", schema),
      ),
      toBe(true),
    ),
  ]));

test("a bare (dispatch) with no message is rejected", () =>
  check(
    isErr(
      readFlow(
        "(flow f (dispatch) (ok (scene-rows clients)))",
        schema,
      ),
    ),
    toBe(true),
  ));
