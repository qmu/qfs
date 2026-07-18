import {
  type SoftStr,
  type Box,
  type Icon,
  box,
  icon,
  pattern,
} from "plgg";
import { type Row } from "plggmatic/Declare/model/Row";
import { type SchedulerMsg } from "plggmatic/Schedule/model/Msg";

/**
 * The checked, normalized flow IR — the shape the reader
 * (`Flow/usecase/read.ts`) produces and the interpreter
 * (`Flow/usecase/interpret.ts`) evaluates. Everything here
 * is inert, JSON-safe DATA — no closures anywhere —
 * because the paused continuation must serialize
 * (`dsl-v1-core.md` §5.4).
 *
 * The expression side generalized from the point-9
 * prototype's ad-hoc node set to a host-application model
 * (`EApp{op, args}`) so all three worked spec §6 flows —
 * including `(sum (map :hours …))` — are expressible. The
 * old prototype constructors survive as thin builders over
 * the new nodes, so the interpreter's existing spec keeps
 * compiling.
 *
 * Restriction (frozen back to `dsl-v1-core.md` §2 by the
 * point-9 prototype): `dispatch` appears only as a DIRECT
 * STEP of a flow body, never nested in an expression — so
 * pure expressions never pause and the paused continuation
 * is a step index + bindings + fuel.
 */

/**
 * A runtime value — the CLOSED, JSON-safe value union of
 * the evaluator. Option/Result are the DSL's no-nil
 * discipline (`dsl-v1-core.md` §3): absence and failure
 * are values folded by match forms, never null, never a
 * throw.
 */
export type FlowValue =
  | Box<"VStr", SoftStr>
  | Box<"VNum", number>
  | Box<
      "VList",
      Readonly<{
        items: ReadonlyArray<FlowValue>;
      }>
    >
  | Box<"VRow", Row>
  | Icon<"VNone">
  | Box<"VSome", Readonly<{ value: FlowValue }>>
  | Box<"VOk", Readonly<{ value: FlowValue }>>
  | Box<"VErr", SoftStr>;

/** Constructs a string value. */
export const vStr = (s: SoftStr): FlowValue =>
  box("VStr")(s);

/** Constructs a number value. */
export const vNum = (n: number): FlowValue =>
  box("VNum")(n);

/** Constructs a list value. */
export const vList = (
  items: ReadonlyArray<FlowValue>,
): FlowValue => box("VList")({ items });

/** Constructs a row value (a scene-read row). */
export const vRow = (r: Row): FlowValue =>
  box("VRow")(r);

/** The absent Option value. */
export const vNone = (): FlowValue =>
  icon("VNone");

/** Constructs a present Option value. */
export const vSome = (v: FlowValue): FlowValue =>
  box("VSome")({ value: v });

/** Constructs a success Result value. */
export const vOk = (v: FlowValue): FlowValue =>
  box("VOk")({ value: v });

/** Constructs a failure Result value (a keyword). */
export const vErr = (kw: SoftStr): FlowValue =>
  box("VErr")(kw);

/** Matchers for folding a {@link FlowValue}. */
export const vStr$ = () => pattern("VStr")();
export const vNum$ = () => pattern("VNum")();
export const vList$ = () => pattern("VList")();
export const vRow$ = () => pattern("VRow")();
export const vNone$ = () => pattern("VNone")();
export const vSome$ = () => pattern("VSome")();
export const vOk$ = () => pattern("VOk")();
export const vErr$ = () => pattern("VErr")();

/**
 * A pure expression. `scene-rows` is the only read of the
 * settled Scene; `EProj`/`EGet` are the no-`fn` keyword
 * projection of `dsl-v1-core.md` §4; `EApp` is a host
 * function application (the ~31-name vocabulary); the two
 * match forms are the exhaustive folds over "rows
 * present?" and Option. No expression dispatches: pure
 * evaluation never pauses.
 */
export type FlowExpr =
  | Box<"EStr", SoftStr>
  | Box<"ENum", number>
  | Box<"EVar", SoftStr>
  | Box<"ESceneRows", SoftStr>
  | Box<
      "EProj",
      Readonly<{ kw: SoftStr; of: FlowExpr }>
    >
  | Box<
      "EGet",
      Readonly<{ kw: SoftStr; of: FlowExpr }>
    >
  | Box<
      "EApp",
      Readonly<{
        op: SoftStr;
        args: ReadonlyArray<FlowExpr>;
      }>
    >
  | Box<"EOk", Readonly<{ of: FlowExpr }>>
  | Box<"EErr", SoftStr>
  | Box<"ESome", Readonly<{ of: FlowExpr }>>
  | Icon<"ENone">
  | Box<
      "EMatchEmpty",
      Readonly<{
        of: FlowExpr;
        whenEmpty: FlowExpr;
        bind: SoftStr;
        whenRest: FlowExpr;
      }>
    >
  | Box<
      "EMatchOption",
      Readonly<{
        of: FlowExpr;
        whenNone: FlowExpr;
        bind: SoftStr;
        whenSome: FlowExpr;
      }>
    >;

/** A string literal. */
export const strLit = (s: SoftStr): FlowExpr =>
  box("EStr")(s);

/** A number literal. */
export const numLit = (n: number): FlowExpr =>
  box("ENum")(n);

/** References a bound name. */
export const varRef = (name: SoftStr): FlowExpr =>
  box("EVar")(name);

/** Reads a collection's settled rows. */
export const sceneRows = (
  collection: SoftStr,
): FlowExpr => box("ESceneRows")(collection);

/** Keyword projection over a list of rows. */
export const mapKw = (
  kw: SoftStr,
  of: FlowExpr,
): FlowExpr => box("EProj")({ kw, of });

/** Keyword lookup on one row, as an Option. */
export const getKw = (
  kw: SoftStr,
  of: FlowExpr,
): FlowExpr => box("EGet")({ kw, of });

/** A host-function application. */
export const app = (
  op: SoftStr,
  args: ReadonlyArray<FlowExpr>,
): FlowExpr => box("EApp")({ op, args });

/** The first element, as an Option (host `first`). */
export const firstOf = (of: FlowExpr): FlowExpr =>
  app("first", [of]);

/** The element count of a list (host `count`). */
export const countOf = (of: FlowExpr): FlowExpr =>
  app("count", [of]);

/** The numeric sum of a list (host `sum`). */
export const sumOf = (of: FlowExpr): FlowExpr =>
  app("sum", [of]);

/** Wraps a value in Ok. */
export const okOf = (of: FlowExpr): FlowExpr =>
  box("EOk")({ of });

/** An Err carrying a keyword. */
export const errOf = (kw: SoftStr): FlowExpr =>
  box("EErr")(kw);

/** Wraps a value in Some. */
export const someOf = (of: FlowExpr): FlowExpr =>
  box("ESome")({ of });

/** The absent Option. */
export const noneOf = (): FlowExpr =>
  icon("ENone");

/** Folds a list: empty vs bound non-empty. */
export const matchEmpty = (m: {
  of: FlowExpr;
  whenEmpty: FlowExpr;
  bind: SoftStr;
  whenRest: FlowExpr;
}): FlowExpr => box("EMatchEmpty")(m);

/** Folds an Option: none vs bound some. */
export const matchOption = (m: {
  of: FlowExpr;
  whenNone: FlowExpr;
  bind: SoftStr;
  whenSome: FlowExpr;
}): FlowExpr => box("EMatchOption")(m);

/** Matchers for folding a {@link FlowExpr}. */
export const strLit$ = () => pattern("EStr")();
export const numLit$ = () => pattern("ENum")();
export const varRef$ = () => pattern("EVar")();
export const sceneRows$ = () =>
  pattern("ESceneRows")();
export const mapKw$ = () => pattern("EProj")();
export const getKw$ = () => pattern("EGet")();
export const app$ = () => pattern("EApp")();
export const okOf$ = () => pattern("EOk")();
export const errOf$ = () => pattern("EErr")();
export const someOf$ = () => pattern("ESome")();
export const noneOf$ = () => pattern("ENone")();
export const matchEmpty$ = () =>
  pattern("EMatchEmpty")();
export const matchOption$ = () =>
  pattern("EMatchOption")();

/**
 * One step of a flow body: dispatch a Msg (and PAUSE
 * until the scheduler settles), or bind a pure
 * expression's value to a name for later steps.
 */
export type FlowStep =
  | Box<"DispatchStep", SchedulerMsg>
  | Box<
      "BindStep",
      Readonly<{ name: SoftStr; expr: FlowExpr }>
    >;

/** A dispatch step (the flow's only world-touching口). */
export const dispatchStep = (
  msg: SchedulerMsg,
): FlowStep => box("DispatchStep")(msg);

/** A pure binding step. */
export const bindStep = (
  name: SoftStr,
  expr: FlowExpr,
): FlowStep => box("BindStep")({ name, expr });

/** Matchers for folding a {@link FlowStep}. */
export const dispatchStep$ = () =>
  pattern("DispatchStep")();
export const bindStep$ = () =>
  pattern("BindStep")();

/**
 * A whole flow: named steps run in order, then `result`
 * evaluates to the flow's value. The unit `run_flow`
 * (point 8) accepts, and the shape a paused continuation
 * references by step index.
 */
export type FlowScript = Readonly<{
  name: SoftStr;
  steps: ReadonlyArray<FlowStep>;
  result: FlowExpr;
}>;

/** Constructs a {@link FlowScript}. */
export const flowScript = (s: {
  name: SoftStr;
  steps: ReadonlyArray<FlowStep>;
  result: FlowExpr;
}): FlowScript => s;
