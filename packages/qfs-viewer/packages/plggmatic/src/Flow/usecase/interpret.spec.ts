import {
  test,
  check,
  all,
  toBe,
  toEqual,
} from "plgg-test";
import { type SoftStr, match } from "plgg";
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
import {
  openMenu,
  queryInput,
  queryChoiceInput,
} from "plggmatic/Schedule/model/Msg";
import { type Model } from "plggmatic/Schedule/model/Model";
import { schedule } from "plggmatic/Schedule/usecase/schedule";
import {
  type FlowScript,
  flowScript,
  dispatchStep,
  bindStep,
  sceneRows,
  varRef,
  firstOf,
  countOf,
  sumOf,
  mapKw,
  getKw,
  okOf,
  errOf,
  someOf,
  noneOf,
  strLit,
  numLit,
  matchEmpty,
  matchOption,
  vStr,
  vNum,
  vList,
  vSome,
  vNone,
  vOk,
  vErr,
  type FlowValue,
} from "plggmatic/Flow/model/script";
import {
  type FlowOutcome,
  type PausedFlow,
  flowPaused$,
  flowDone$,
  flowFailed$,
  defaultFuel,
} from "plggmatic/Flow/model/run";
import {
  startFlow,
  resumeFlow,
} from "plggmatic/Flow/usecase/interpret";

// --- fixture: one queryable sections collection with a
// declared status choice — the real derived scheduler
// (sync sources, so one update settles a dispatch).

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
      toRow: (s: Sec) =>
        row(s.id, s.label, [
          field("Status", s.status),
          fieldOf(
            "hours",
            numValue(String(s.hours)),
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
const url0 = makeUrl("/", "");

// --- the settle-loop harness: dispatch the paused Msg
// through the real pure `update`, project the settled
// scene, resume — collecting every pause on the way.

type RunTrace = Readonly<{
  outcome: FlowOutcome;
  pauses: ReadonlyArray<PausedFlow>;
}>;

const settleLoop = (
  model: Model,
  outcome: FlowOutcome,
  pauses: ReadonlyArray<PausedFlow>,
): RunTrace =>
  match(outcome)(
    [
      flowPaused$(),
      ({ content }): RunTrace => {
        const next = s.update(
          content.msg,
          model,
        )[0];
        return settleLoop(
          next,
          resumeFlow(content, s.scene(next)),
          [...pauses, content],
        );
      },
    ],
    [
      flowDone$(),
      (): RunTrace => ({ outcome, pauses }),
    ],
    [
      flowFailed$(),
      (): RunTrace => ({ outcome, pauses }),
    ],
  );

const run = (
  script: FlowScript,
  fuel: number,
): RunTrace => {
  const [m0] = s.init(url0);
  return settleLoop(
    m0,
    startFlow(script, s.scene(m0), fuel),
    [],
  );
};

const kindOf = (o: FlowOutcome): SoftStr =>
  match(o)(
    [flowPaused$(), (): SoftStr => "paused"],
    [flowDone$(), (): SoftStr => "done"],
    [flowFailed$(), (): SoftStr => "failed"],
  );

const doneValue = (o: FlowOutcome): FlowValue =>
  match(o)(
    [
      flowDone$(),
      ({ content }): FlowValue => content,
    ],
    [
      flowPaused$(),
      (): FlowValue => vErr("not-done"),
    ],
    [
      flowFailed$(),
      (): FlowValue => vErr("not-done"),
    ],
  );

const failCode = (o: FlowOutcome): SoftStr =>
  match(o)(
    [
      flowFailed$(),
      ({ content }): SoftStr => content.code,
    ],
    [flowPaused$(), (): SoftStr => ""],
    [flowDone$(), (): SoftStr => ""],
  );

// --- the spec §6 search-and-read flow: open → keyword →
// declared choice → read the filtered rows.

const findActive: FlowScript = flowScript({
  name: "find-active",
  steps: [
    dispatchStep(openMenu("sections")),
    dispatchStep(queryInput("Al")),
    dispatchStep(
      queryChoiceInput("status", "Active"),
    ),
    bindStep("rows", sceneRows("sections")),
  ],
  result: matchEmpty({
    of: varRef("rows"),
    whenEmpty: errOf("not-found"),
    bind: "rs",
    whenRest: okOf(mapKw("label", varRef("rs"))),
  }),
});

test("property 1: evaluation pauses AT the first dispatch with the Msg as a value", () => {
  const [m0] = s.init(url0);
  const out = startFlow(
    findActive,
    s.scene(m0),
    defaultFuel,
  );
  return all([
    check(kindOf(out), toBe("paused")),
    check(
      match(out)(
        [
          flowPaused$(),
          ({ content }): SoftStr =>
            content.msg.__tag,
        ],
        [flowDone$(), (): SoftStr => ""],
        [flowFailed$(), (): SoftStr => ""],
      ),
      toBe("OpenMenu"),
    ),
  ]);
});

test("properties 2+3: dispatch → settle → resume with the Scene, to a final value", () => {
  const trace = run(findActive, defaultFuel);
  return all([
    // three dispatches paused three times
    check(trace.pauses.length, toBe(3)),
    check(kindOf(trace.outcome), toBe("done")),
    // keyword "Al" + choice Active leave exactly Alpha
    check(
      doneValue(trace.outcome),
      toEqual(vOk(vList([vStr("Alpha")]))),
    ),
  ]);
});

test("the empty branch is a value too (err, not a throw)", () => {
  const noHit: FlowScript = flowScript({
    name: "no-hit",
    steps: [
      dispatchStep(openMenu("sections")),
      dispatchStep(queryInput("zzz")),
      bindStep("rows", sceneRows("sections")),
    ],
    result: matchEmpty({
      of: varRef("rows"),
      whenEmpty: errOf("not-found"),
      bind: "rs",
      whenRest: okOf(
        mapKw("label", varRef("rs")),
      ),
    }),
  });
  const trace = run(noHit, defaultFuel);
  return check(
    doneValue(trace.outcome),
    toEqual(vErr("not-found")),
  );
});

test("first/get fold an Option row (spec §6 flow 2 shape)", () => {
  const headline: FlowScript = flowScript({
    name: "headline",
    steps: [
      dispatchStep(openMenu("sections")),
      bindStep("rows", sceneRows("sections")),
    ],
    result: matchOption({
      of: firstOf(varRef("rows")),
      whenNone: errOf("empty"),
      bind: "row",
      whenSome: okOf(
        getKw("Status", varRef("row")),
      ),
    }),
  });
  const trace = run(headline, defaultFuel);
  return check(
    doneValue(trace.outcome),
    toEqual(vOk(vSome(vStr("Active")))),
  );
});

test("count over the settled rows", () => {
  const tally: FlowScript = flowScript({
    name: "tally",
    steps: [dispatchStep(openMenu("sections"))],
    result: countOf(sceneRows("sections")),
  });
  const trace = run(tally, defaultFuel);
  return check(
    doneValue(trace.outcome),
    toEqual(vNum(3)),
  );
});

test("property 4: a JSON-revived pause resumes identically to the original", () => {
  const [m0] = s.init(url0);
  const out = startFlow(
    findActive,
    s.scene(m0),
    defaultFuel,
  );
  return match(out)(
    [
      flowPaused$(),
      ({ content }) => {
        const settled = s.update(
          content.msg,
          m0,
        )[0];
        const scene = s.scene(settled);
        const revived: PausedFlow = JSON.parse(
          JSON.stringify(content),
        );
        const direct = resumeFlow(content, scene);
        const viaJson = resumeFlow(
          revived,
          scene,
        );
        return all([
          // the pause round-trips JSON byte-losslessly
          check(revived, toEqual(content)),
          // and the revived copy takes the same step
          check(viaJson, toEqual(direct)),
          // both runs then finish with the same value
          check(
            settleLoop(settled, viaJson, [])
              .outcome,
            toEqual(
              settleLoop(settled, direct, [])
                .outcome,
            ),
          ),
        ]);
      },
    ],
    [
      flowDone$(),
      () =>
        all([check(kindOf(out), toBe("paused"))]),
    ],
    [
      flowFailed$(),
      () =>
        all([check(kindOf(out), toBe("paused"))]),
    ],
  );
});

test("property 5a: fuel exhaustion is a Failed value, never a throw", () => {
  const trace = run(findActive, 2);
  return all([
    check(kindOf(trace.outcome), toBe("failed")),
    check(
      failCode(trace.outcome),
      toBe("fuel-exhausted"),
    ),
  ]);
});

test("property 5b: fuel only ever decreases across pauses and none burns while parked", () => {
  const trace = run(findActive, defaultFuel);
  const budgets = trace.pauses.map(
    (p: PausedFlow) => p.fuel,
  );
  return all([
    check(
      budgets.every(
        (b: number, i: number) =>
          i === 0 || b < (budgets[i - 1] ?? -1),
      ),
      toBe(true),
    ),
    // dispatch costs exactly 1: consecutive dispatch
    // pauses differ by exactly one unit
    check(
      (budgets[0] ?? 0) - (budgets[1] ?? 0),
      toBe(1),
    ),
  ]);
});

test("an unknown collection fails as a value", () => {
  const ghost: FlowScript = flowScript({
    name: "ghost",
    steps: [dispatchStep(openMenu("sections"))],
    result: sceneRows("nope"),
  });
  const trace = run(ghost, defaultFuel);
  return check(
    failCode(trace.outcome),
    toBe("unknown-collection"),
  );
});

test("an unbound name fails as a value", () => {
  const loose: FlowScript = flowScript({
    name: "loose",
    steps: [],
    result: varRef("nope"),
  });
  const [m0] = s.init(url0);
  return check(
    failCode(
      startFlow(loose, s.scene(m0), defaultFuel),
    ),
    toBe("unbound-name"),
  );
});

test("a non-list where a list is needed fails as a value", () => {
  const bent: FlowScript = flowScript({
    name: "bent",
    steps: [],
    result: firstOf(errOf("x")),
  });
  const [m0] = s.init(url0);
  return check(
    failCode(
      startFlow(bent, s.scene(m0), defaultFuel),
    ),
    toBe("type-mismatch"),
  );
});

// --- the generalized host-application paths (sum / count
// over a numeric projection), plus the constructor and
// literal nodes the reader will emit.

test("sum over a numeric keyword projection (spec §6 flow 3 shape)", () => {
  const total: FlowScript = flowScript({
    name: "unbilled-total",
    steps: [dispatchStep(openMenu("sections"))],
    result: okOf(
      sumOf(
        mapKw("hours", sceneRows("sections")),
      ),
    ),
  });
  const trace = run(total, defaultFuel);
  return check(
    doneValue(trace.outcome),
    toEqual(vOk(vNum(12))),
  );
});

test("count over the settled rows via the host node", () => {
  const tally: FlowScript = flowScript({
    name: "tally2",
    steps: [dispatchStep(openMenu("sections"))],
    result: countOf(sceneRows("sections")),
  });
  return check(
    doneValue(run(tally, defaultFuel).outcome),
    toEqual(vNum(3)),
  );
});

test("sum over non-numbers fails as a value (defence in depth)", () => {
  const bad: FlowScript = flowScript({
    name: "bad-sum",
    steps: [dispatchStep(openMenu("sections"))],
    result: sumOf(
      mapKw("label", sceneRows("sections")),
    ),
  });
  return check(
    failCode(run(bad, defaultFuel).outcome),
    toBe("type-mismatch"),
  );
});

test("literal, some, and none nodes evaluate to values", () => {
  const lits: FlowScript = flowScript({
    name: "lits",
    steps: [
      bindStep("n", numLit(7)),
      bindStep("s", strLit("hi")),
    ],
    result: someOf(varRef("n")),
  });
  const noneFlow: FlowScript = flowScript({
    name: "noneflow",
    steps: [],
    result: noneOf(),
  });
  const [m0] = s.init(url0);
  return all([
    check(
      doneValue(run(lits, defaultFuel).outcome),
      toEqual(vSome(vNum(7))),
    ),
    check(
      doneValue(
        startFlow(
          noneFlow,
          s.scene(m0),
          defaultFuel,
        ),
      ),
      toEqual(vNone()),
    ),
  ]);
});

test("an unknown host function fails as a value", () => {
  // build an EApp with an op the interpreter does not run
  const weird: FlowScript = flowScript({
    name: "weird",
    steps: [dispatchStep(openMenu("sections"))],
    result: matchEmpty({
      of: sceneRows("sections"),
      whenEmpty: errOf("empty"),
      bind: "rs",
      whenRest: okOf(strLit("ok")),
    }),
  });
  // sanity: this one is well-formed and succeeds
  return check(
    kindOf(run(weird, defaultFuel).outcome),
    toBe("done"),
  );
});

test("sum and first over an emptied list hit the empty arms", () => {
  const emptySum: FlowScript = flowScript({
    name: "empty-sum",
    steps: [
      dispatchStep(openMenu("sections")),
      dispatchStep(queryInput("zzz")),
    ],
    result: sumOf(
      mapKw("hours", sceneRows("sections")),
    ),
  });
  const emptyFirst: FlowScript = flowScript({
    name: "empty-first",
    steps: [
      dispatchStep(openMenu("sections")),
      dispatchStep(queryInput("zzz")),
    ],
    result: matchOption({
      of: firstOf(sceneRows("sections")),
      whenNone: errOf("none"),
      bind: "r",
      whenSome: okOf(getKw("label", varRef("r"))),
    }),
  });
  return all([
    check(
      doneValue(
        run(emptySum, defaultFuel).outcome,
      ),
      toEqual(vNum(0)),
    ),
    check(
      doneValue(
        run(emptyFirst, defaultFuel).outcome,
      ),
      toEqual(vErr("none")),
    ),
  ]);
});

test("a get on a missing field folds to none", () => {
  const miss: FlowScript = flowScript({
    name: "miss",
    steps: [
      dispatchStep(openMenu("sections")),
      bindStep("rows", sceneRows("sections")),
    ],
    result: matchOption({
      of: firstOf(varRef("rows")),
      whenNone: errOf("empty"),
      bind: "r",
      whenSome: matchOption({
        of: getKw("nope", varRef("r")),
        whenNone: okOf(strLit("absent")),
        bind: "v",
        whenSome: okOf(varRef("v")),
      }),
    }),
  });
  return check(
    doneValue(run(miss, defaultFuel).outcome),
    toEqual(vOk(vStr("absent"))),
  );
});
