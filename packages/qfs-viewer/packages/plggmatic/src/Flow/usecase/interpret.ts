import {
  type SoftStr,
  type Box,
  type Icon,
  type Option,
  some,
  none,
  box,
  icon,
  pattern,
  match,
  matchOption,
  fromNullable,
} from "plgg";
import {
  type Row,
  type Field,
  type FieldValue,
  fieldText,
  textValue$,
  numValue$,
  flagValue$,
  momentValue$,
  refValue$,
  mediaValue$,
} from "plggmatic/Declare/model/Row";
import {
  type Scene,
  type Level,
  type RowLink,
  menuLevel$,
  listLevel$,
  boardLevel$,
  detailLevel$,
} from "plggmatic/Schedule/model/Scene";
import {
  type FlowValue,
  type FlowExpr,
  type FlowStep,
  type FlowScript,
  vStr,
  vNum,
  vList,
  vRow,
  vNone,
  vSome,
  vOk,
  vErr,
  vStr$,
  vNum$,
  vList$,
  vRow$,
  vNone$,
  vSome$,
  vOk$,
  vErr$,
  strLit$,
  numLit$,
  varRef$,
  sceneRows$,
  mapKw$,
  getKw$,
  app$,
  okOf$,
  errOf$,
  someOf$,
  noneOf$,
  matchEmpty$,
  matchOption$,
  dispatchStep$,
  bindStep$,
} from "plggmatic/Flow/model/script";
import {
  type FlowDiag,
  type FlowOutcome,
  type FlowEnv,
  type PausedFlow,
  flowPaused,
  flowDone,
  flowFailed,
  emptyEnv,
  bind,
} from "plggmatic/Flow/model/run";

/**
 * The small-step, fuel-metered, pausable evaluator of
 * `dsl-v1-core.md` §1's `step.ts`. The handshake it proves
 * (asserted by its spec):
 *
 * 1. evaluation runs until a dispatch step, then returns a
 *    PAUSED VALUE carrying the yielded Msg — nothing runs;
 * 2. the harness dispatches through the pure `update` and
 *    settles;
 * 3. the paused flow resumes with the settled `Scene`;
 * 4. a paused flow serializes (JSON) and a revived copy
 *    resumes identically;
 * 5. fuel is total: exhaustion is a `Failed` VALUE, and
 *    accounting survives pause/resume (1 fuel = 1
 *    reduction; zero consumed while paused).
 *
 * Everything is total — every error path is a
 * `FlowFailed` value, never a throw.
 */

/** A pure evaluation's outcome (value + fuel left). */
type EvalOut =
  | Box<
      "EvalOk",
      Readonly<{ value: FlowValue; fuel: number }>
    >
  | Box<"EvalFail", FlowDiag>;

const evalOk = (
  value: FlowValue,
  fuel: number,
): EvalOut => box("EvalOk")({ value, fuel });

const evalFail = (
  code: FlowDiag["code"],
  at: number,
): EvalOut => box("EvalFail")({ code, at });

const evalOk$ = () => pattern("EvalOk")();
const evalFail$ = () => pattern("EvalFail")();

/**
 * The settled rows of a collection, read from the Scene
 * — the `scene-rows` observation. Only a `ListLevel`
 * carries rows; a missing collection is `None` (the
 * evaluator turns it into an `unknown-collection` value).
 */
const rowsOf = (
  scene: Scene,
  collection: SoftStr,
): Option<ReadonlyArray<Row>> =>
  fromNullable(
    scene.levels.flatMap((level: Level) =>
      match(level)(
        [
          listLevel$(),
          ({
            content,
          }): ReadonlyArray<
            ReadonlyArray<Row>
          > =>
            content.collection === collection
              ? [
                  content.rows.map(
                    (r: RowLink) => r.row,
                  ),
                ]
              : [],
        ],
        [
          menuLevel$(),
          (): ReadonlyArray<
            ReadonlyArray<Row>
          > => [],
        ],
        [
          boardLevel$(),
          (): ReadonlyArray<
            ReadonlyArray<Row>
          > => [],
        ],
        [
          detailLevel$(),
          (): ReadonlyArray<
            ReadonlyArray<Row>
          > => [],
        ],
      ),
    )[0],
  );

/** True if a field value is a numeric cell. */
const isNumField = (v: FieldValue): boolean =>
  match(v)(
    [textValue$(), (): boolean => false],
    [numValue$(), (): boolean => true],
    [flagValue$(), (): boolean => false],
    [momentValue$(), (): boolean => false],
    [refValue$(), (): boolean => false],
    [mediaValue$(), (): boolean => false],
  );

/**
 * One row field projected to a value (`dsl-v1-core.md`
 * §4 keyword projection): `label`/`id` read the row's own
 * identity as strings; any other keyword reads the field
 * whose label matches, as a NUMBER when that field is a
 * numeric cell (so `sum (map :hours …)` works) and a
 * string otherwise. Total — a missing field is `""`.
 */
const projectKw = (
  kw: SoftStr,
  r: Row,
): FlowValue =>
  kw === "label"
    ? vStr(r.label)
    : kw === "id"
      ? vStr(r.id)
      : matchOption<Field, FlowValue>(
          () => vStr(""),
          (f: Field) =>
            isNumField(f.value)
              ? vNum(Number(fieldText(f.value)))
              : vStr(fieldText(f.value)),
        )(
          fromNullable(
            r.fields.find(
              (f: Field) => f.label === kw,
            ),
          ),
        );

/** Keyword lookup as an Option value (`get`). */
const lookupKw = (
  kw: SoftStr,
  r: Row,
): FlowValue =>
  kw === "label" ||
  kw === "id" ||
  r.fields.some((f: Field) => f.label === kw)
    ? vSome(projectKw(kw, r))
    : vNone();

/** A value's list items, if it is a list (total). */
type MaybeItems = Option<
  ReadonlyArray<FlowValue>
>;
const listItems = (v: FlowValue): MaybeItems =>
  match(v)(
    [vStr$(), (): MaybeItems => none()],
    [vNum$(), (): MaybeItems => none()],
    [
      vList$(),
      ({ content }): MaybeItems =>
        some(content.items),
    ],
    [vRow$(), (): MaybeItems => none()],
    [vNone$(), (): MaybeItems => none()],
    [vSome$(), (): MaybeItems => none()],
    [vOk$(), (): MaybeItems => none()],
    [vErr$(), (): MaybeItems => none()],
  );

/** A value's row, if it is one (total). */
type MaybeRow = Option<Row>;
const rowItem = (v: FlowValue): MaybeRow =>
  match(v)(
    [vStr$(), (): MaybeRow => none()],
    [vNum$(), (): MaybeRow => none()],
    [vList$(), (): MaybeRow => none()],
    [
      vRow$(),
      ({ content }): MaybeRow => some(content),
    ],
    [vNone$(), (): MaybeRow => none()],
    [vSome$(), (): MaybeRow => none()],
    [vOk$(), (): MaybeRow => none()],
    [vErr$(), (): MaybeRow => none()],
  );

/** How a value folds as an Option — or not one. */
type OptShape =
  | Box<"IsSome", FlowValue>
  | Icon<"IsNone">
  | Icon<"NotOption">;
const optShape = (v: FlowValue): OptShape =>
  match(v)(
    [vStr$(), (): OptShape => icon("NotOption")],
    [vNum$(), (): OptShape => icon("NotOption")],
    [vList$(), (): OptShape => icon("NotOption")],
    [vRow$(), (): OptShape => icon("NotOption")],
    [vNone$(), (): OptShape => icon("IsNone")],
    [
      vSome$(),
      ({ content }): OptShape =>
        box("IsSome")(content.value),
    ],
    [vOk$(), (): OptShape => icon("NotOption")],
    [vErr$(), (): OptShape => icon("NotOption")],
  );
const isSome$ = () => pattern("IsSome")();
const isNone$ = () => pattern("IsNone")();
const notOption$ = () => pattern("NotOption")();

/** Chains an evaluation into its continuation. */
const onEval = (
  out: EvalOut,
  then: (
    value: FlowValue,
    fuel: number,
  ) => EvalOut,
): EvalOut =>
  match(out)(
    [
      evalOk$(),
      ({ content }): EvalOut =>
        then(content.value, content.fuel),
    ],
    [
      evalFail$(),
      ({ content }): EvalOut =>
        box("EvalFail")(content),
    ],
  );

/** Chains an evaluation that must yield a list. */
const onList = (
  out: EvalOut,
  at: number,
  then: (
    items: ReadonlyArray<FlowValue>,
    fuel: number,
  ) => EvalOut,
): EvalOut =>
  onEval(out, (value, fuel) =>
    matchOption<
      ReadonlyArray<FlowValue>,
      EvalOut
    >(
      () => evalFail("type-mismatch", at),
      (items: ReadonlyArray<FlowValue>) =>
        then(items, fuel),
    )(listItems(value)),
  );

/** Evaluates each element to a number, or fails. */
const numbersOf = (
  items: ReadonlyArray<FlowValue>,
): Option<ReadonlyArray<number>> => {
  const nums = items.flatMap((item: FlowValue) =>
    match(item)(
      [vStr$(), (): ReadonlyArray<number> => []],
      [
        vNum$(),
        ({ content }): ReadonlyArray<number> => [
          content,
        ],
      ],
      [vList$(), (): ReadonlyArray<number> => []],
      [vRow$(), (): ReadonlyArray<number> => []],
      [vNone$(), (): ReadonlyArray<number> => []],
      [vSome$(), (): ReadonlyArray<number> => []],
      [vOk$(), (): ReadonlyArray<number> => []],
      [vErr$(), (): ReadonlyArray<number> => []],
    ),
  );
  return nums.length === items.length
    ? some(nums)
    : none();
};

/**
 * Applies a host function to already-evaluated args.
 * `first`/`count`/`sum` are the ones the worked spec §6
 * flows exercise; an unknown op or a type-wrong arg is a
 * total failure value (the reader already type-checked,
 * so these are defence-in-depth).
 */
const applyHost = (
  op: SoftStr,
  args: ReadonlyArray<FlowValue>,
  at: number,
  fuel: number,
): EvalOut => {
  const one = fromNullable(args[0]);
  return matchOption<FlowValue, EvalOut>(
    () => evalFail("type-mismatch", at),
    (arg: FlowValue) =>
      matchOption<
        ReadonlyArray<FlowValue>,
        EvalOut
      >(
        () => evalFail("type-mismatch", at),
        (items: ReadonlyArray<FlowValue>) =>
          op === "first"
            ? matchOption<FlowValue, EvalOut>(
                () => evalOk(vNone(), fuel),
                (head: FlowValue) =>
                  evalOk(vSome(head), fuel),
              )(fromNullable(items[0]))
            : op === "count"
              ? evalOk(vNum(items.length), fuel)
              : op === "sum"
                ? matchOption<
                    ReadonlyArray<number>,
                    EvalOut
                  >(
                    () =>
                      evalFail(
                        "type-mismatch",
                        at,
                      ),
                    (ns: ReadonlyArray<number>) =>
                      evalOk(
                        vNum(
                          ns.reduce(
                            (a, b) => a + b,
                            0,
                          ),
                        ),
                        fuel,
                      ),
                  )(numbersOf(items))
                : evalFail("unknown-host", at),
      )(listItems(arg)),
  )(one);
};

/**
 * Evaluates one PURE expression against the settled scene
 * and the bindings, charging 1 fuel per reduction. Never
 * pauses (dispatch is a step, not an expression) and
 * never throws.
 */
const evalExpr = (
  scene: Scene,
  env: FlowEnv,
  at: number,
  expr: FlowExpr,
  fuel: number,
): EvalOut => {
  if (fuel <= 0)
    return evalFail("fuel-exhausted", at);
  const f = fuel - 1;
  return match(expr)(
    [
      strLit$(),
      ({ content }): EvalOut =>
        evalOk(vStr(content), f),
    ],
    [
      numLit$(),
      ({ content }): EvalOut =>
        evalOk(vNum(content), f),
    ],
    [
      varRef$(),
      ({ content }): EvalOut =>
        matchOption<FlowValue, EvalOut>(
          () => evalFail("unbound-name", at),
          (v: FlowValue) => evalOk(v, f),
        )(fromNullable(env[content])),
    ],
    [
      sceneRows$(),
      ({ content }): EvalOut =>
        matchOption<ReadonlyArray<Row>, EvalOut>(
          () =>
            evalFail("unknown-collection", at),
          (rows: ReadonlyArray<Row>) =>
            evalOk(vList(rows.map(vRow)), f),
        )(rowsOf(scene, content)),
    ],
    [
      mapKw$(),
      ({ content }): EvalOut =>
        onList(
          evalExpr(scene, env, at, content.of, f),
          at,
          (items, left) =>
            matchOption<
              ReadonlyArray<Row>,
              EvalOut
            >(
              () => evalFail("type-mismatch", at),
              (rows: ReadonlyArray<Row>) =>
                evalOk(
                  vList(
                    rows.map((r: Row) =>
                      projectKw(content.kw, r),
                    ),
                  ),
                  left,
                ),
            )(allRows(items)),
        ),
    ],
    [
      getKw$(),
      ({ content }): EvalOut =>
        onEval(
          evalExpr(scene, env, at, content.of, f),
          (value, left) =>
            matchOption<Row, EvalOut>(
              () => evalFail("type-mismatch", at),
              (r: Row) =>
                evalOk(
                  lookupKw(content.kw, r),
                  left,
                ),
            )(rowItem(value)),
        ),
    ],
    [
      app$(),
      ({ content }): EvalOut =>
        evalArgs(
          scene,
          env,
          at,
          content.args,
          f,
          [],
          (vals, left) =>
            applyHost(content.op, vals, at, left),
        ),
    ],
    [
      okOf$(),
      ({ content }): EvalOut =>
        onEval(
          evalExpr(scene, env, at, content.of, f),
          (value, left) =>
            evalOk(vOk(value), left),
        ),
    ],
    [
      errOf$(),
      ({ content }): EvalOut =>
        evalOk(vErr(content), f),
    ],
    [
      someOf$(),
      ({ content }): EvalOut =>
        onEval(
          evalExpr(scene, env, at, content.of, f),
          (value, left) =>
            evalOk(vSome(value), left),
        ),
    ],
    [
      noneOf$(),
      (): EvalOut => evalOk(vNone(), f),
    ],
    [
      matchEmpty$(),
      ({ content }): EvalOut =>
        onList(
          evalExpr(scene, env, at, content.of, f),
          at,
          (items, left) =>
            items.length === 0
              ? evalExpr(
                  scene,
                  env,
                  at,
                  content.whenEmpty,
                  left,
                )
              : evalExpr(
                  scene,
                  bind(
                    env,
                    content.bind,
                    vList(items),
                  ),
                  at,
                  content.whenRest,
                  left,
                ),
        ),
    ],
    [
      matchOption$(),
      ({ content }): EvalOut =>
        onEval(
          evalExpr(scene, env, at, content.of, f),
          (value, left) =>
            match(optShape(value))(
              [
                isSome$(),
                ({ content: inner }): EvalOut =>
                  evalExpr(
                    scene,
                    bind(
                      env,
                      content.bind,
                      inner,
                    ),
                    at,
                    content.whenSome,
                    left,
                  ),
              ],
              [
                isNone$(),
                (): EvalOut =>
                  evalExpr(
                    scene,
                    env,
                    at,
                    content.whenNone,
                    left,
                  ),
              ],
              [
                notOption$(),
                (): EvalOut =>
                  evalFail("type-mismatch", at),
              ],
            ),
        ),
    ],
  );
};

/** Evaluates an argument list left-to-right, threading fuel. */
const evalArgs = (
  scene: Scene,
  env: FlowEnv,
  at: number,
  args: ReadonlyArray<FlowExpr>,
  fuel: number,
  acc: ReadonlyArray<FlowValue>,
  then: (
    vals: ReadonlyArray<FlowValue>,
    fuel: number,
  ) => EvalOut,
): EvalOut =>
  matchOption<FlowExpr, EvalOut>(
    () => then(acc, fuel),
    (head: FlowExpr) =>
      onEval(
        evalExpr(scene, env, at, head, fuel),
        (value, left) =>
          evalArgs(
            scene,
            env,
            at,
            args.slice(1),
            left,
            [...acc, value],
            then,
          ),
      ),
  )(fromNullable(args[0]));

/** Every item as a Row, or None if any is not one. */
const allRows = (
  items: ReadonlyArray<FlowValue>,
): Option<ReadonlyArray<Row>> => {
  const rows = items.flatMap((item: FlowValue) =>
    matchOption<Row, ReadonlyArray<Row>>(
      () => [],
      (r: Row) => [r],
    )(rowItem(item)),
  );
  return rows.length === items.length
    ? some(rows)
    : none();
};

/**
 * Runs steps from `index` until a dispatch pauses, the
 * result finishes, or fuel/an error fails. Stepping onto
 * a dispatch costs 1 fuel; nothing is charged while
 * parked.
 */
const advance = (
  script: FlowScript,
  scene: Scene,
  env: FlowEnv,
  index: number,
  fuel: number,
): FlowOutcome =>
  matchOption<FlowStep, FlowOutcome>(
    () =>
      match(
        evalExpr(
          scene,
          env,
          script.steps.length,
          script.result,
          fuel,
        ),
      )(
        [
          evalOk$(),
          ({ content }): FlowOutcome =>
            flowDone(content.value),
        ],
        [
          evalFail$(),
          ({ content }): FlowOutcome =>
            flowFailed(content.code, content.at),
        ],
      ),
    (step: FlowStep) =>
      fuel <= 0
        ? flowFailed("fuel-exhausted", index)
        : match(step)(
            [
              dispatchStep$(),
              ({ content }): FlowOutcome =>
                flowPaused({
                  script,
                  next: index + 1,
                  env,
                  fuel: fuel - 1,
                  msg: content,
                }),
            ],
            [
              bindStep$(),
              ({ content }): FlowOutcome =>
                match(
                  evalExpr(
                    scene,
                    env,
                    index,
                    content.expr,
                    fuel - 1,
                  ),
                )(
                  [
                    evalOk$(),
                    ({
                      content: out,
                    }): FlowOutcome =>
                      advance(
                        script,
                        scene,
                        bind(
                          env,
                          content.name,
                          out.value,
                        ),
                        index + 1,
                        out.fuel,
                      ),
                  ],
                  [
                    evalFail$(),
                    ({
                      content: diag,
                    }): FlowOutcome =>
                      flowFailed(
                        diag.code,
                        diag.at,
                      ),
                  ],
                ),
            ],
          ),
  )(fromNullable(script.steps[index]));

/**
 * Starts a flow against the current settled scene with a
 * fuel budget. Returns paused-on-first-dispatch, done, or
 * failed — never throws.
 */
export const startFlow = (
  script: FlowScript,
  scene: Scene,
  fuel: number,
): FlowOutcome =>
  advance(script, scene, emptyEnv(), 0, fuel);

/**
 * Resumes a paused flow with the scene the scheduler
 * settled on after dispatching the paused Msg. The paused
 * value is pure data — a JSON-revived copy resumes
 * identically.
 */
export const resumeFlow = (
  paused: PausedFlow,
  scene: Scene,
): FlowOutcome =>
  advance(
    paused.script,
    scene,
    paused.env,
    paused.next,
    paused.fuel,
  );
