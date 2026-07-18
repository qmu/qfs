import {
  type SoftStr,
  type Box,
  isErr,
  box,
  pattern,
  match,
  matchOption,
} from "plgg";
import { type SemDiagnostic } from "plgg-ir-language";
import {
  type Row,
  type Field,
  type FieldValue,
  numValue$,
  textValue$,
  flagValue$,
  momentValue$,
  refValue$,
  mediaValue$,
} from "plggmatic/Declare/model/Row";
import { type Collection } from "plggmatic/Declare/model/Collection";
import {
  type Path,
  sync$,
  async$,
  dynamic$,
  adapter$,
} from "plggmatic/Declare/model/Source";
import {
  type Query,
  type QueryChoice,
} from "plggmatic/Declare/model/Query";
import { type Declaration } from "plggmatic/Declare/model/Declaration";
import {
  type Scene,
  type ConfirmPrompt,
} from "plggmatic/Schedule/model/Scene";
import { type SchedulerMsg } from "plggmatic/Schedule/model/Msg";
import { type Model } from "plggmatic/Schedule/model/Model";
import { type Scheduled } from "plggmatic/Schedule/usecase/schedule";
import {
  type FlowSchema,
  type CollectionSchema,
  flowSchema,
} from "plggmatic/Flow/model/forms";
import {
  type FlowOutcome,
  flowPaused$,
  flowDone$,
  flowFailed$,
  defaultFuel,
} from "plggmatic/Flow/model/run";
import { readFlow } from "plggmatic/Flow/usecase/read";
import {
  startFlow,
  resumeFlow,
} from "plggmatic/Flow/usecase/interpret";

/**
 * The pure scheduler seam `run_flow` drives: the current
 * settled {@link Scene}, and a `settle` that folds one
 * {@link SchedulerMsg} into the NEXT seam (the model is
 * threaded inside; the returned `Cmd` is DROPPED — the
 * flow harness is pure, exactly as the scheduler "executes
 * nothing"). {@link flowHost} builds one from a derived
 * program and a model.
 */
export type FlowHost = Readonly<{
  scene: Scene;
  settle: (msg: SchedulerMsg) => FlowHost;
}>;

/** Builds a {@link FlowHost} from a derived program + model. */
export const flowHost = (
  scheduled: Scheduled,
  model: Model,
): FlowHost => ({
  scene: scheduled.scene(model),
  settle: (msg: SchedulerMsg): FlowHost =>
    flowHost(
      scheduled,
      scheduled.update(msg, model)[0],
    ),
});

/**
 * The result of a `run_flow` invocation — a closed union so
 * the adapter reports it as a value:
 * - `Rejected` — the reader/checker refused the script;
 *   the positioned diagnostics carry source ranges.
 * - `Stalled` — the flow ran to completion but left a
 *   destructive action PARKED at its confirmation (no
 *   auto-confirm — mission point 8): the caller must
 *   dispatch the confirm explicitly.
 * - `Ran` — the flow reached a terminal {@link FlowOutcome}
 *   (a value, or a positioned runtime failure).
 */
export type RunOutcome =
  | Box<"Rejected", ReadonlyArray<SemDiagnostic>>
  | Box<"Stalled", ConfirmPrompt>
  | Box<"Ran", FlowOutcome>;

/** Constructs a `Rejected` result. */
export const rejected = (
  diagnostics: ReadonlyArray<SemDiagnostic>,
): RunOutcome => box("Rejected")(diagnostics);

/** Constructs a `Stalled` result. */
export const stalled = (
  confirm: ConfirmPrompt,
): RunOutcome => box("Stalled")(confirm);

/** Constructs a `Ran` result. */
export const ran = (
  outcome: FlowOutcome,
): RunOutcome => box("Ran")(outcome);

/** Matchers for folding a {@link RunOutcome}. */
export const rejected$ = () =>
  pattern("Rejected")();
export const stalled$ = () =>
  pattern("Stalled")();
export const ran$ = () => pattern("Ran")();

/** True when a field value is a numeric cell. */
const isNumeric = (v: FieldValue): boolean =>
  match(v)(
    [numValue$(), (): boolean => true],
    [textValue$(), (): boolean => false],
    [flagValue$(), (): boolean => false],
    [momentValue$(), (): boolean => false],
    [refValue$(), (): boolean => false],
    [mediaValue$(), (): boolean => false],
  );

/**
 * The static field types observed in a collection's rows:
 * one entry per distinct non-empty field label, numeric
 * when EVERY occurrence is a numeric cell (so `sum (map
 * :kw …)` type-checks exactly when the projection is
 * numeric, matching the interpreter's per-row projection).
 */
const fieldsOfRows = (
  rows: ReadonlyArray<Row>,
): ReadonlyArray<
  Readonly<{ kw: SoftStr; numeric: boolean }>
> => {
  const labels = rows
    .flatMap((r: Row) =>
      r.fields.map((f: Field) => f.label),
    )
    .filter(
      (
        l: SoftStr,
        i: number,
        a: ReadonlyArray<SoftStr>,
      ) => l !== "" && a.indexOf(l) === i,
    );
  return labels.map((kw: SoftStr) => {
    const values = rows.flatMap((r: Row) =>
      r.fields
        .filter((f: Field) => f.label === kw)
        .map((f: Field) => f.value),
    );
    return {
      kw,
      numeric:
        values.length > 0 &&
        values.every(isNumeric),
    };
  });
};

/**
 * A collection's static schema. Field types are OBSERVED
 * from a `Sync` source's rows (the only source a pure
 * derivation can read); an `Async`/`Adapter`/`Dynamic`
 * collection contributes its id and declared choices but
 * no field types (they default to string until the
 * manifest lowering supplies them). This is the
 * declaration→schema bridge `readFlow` anticipated.
 */
const schemaOfCollection = (
  c: Collection,
): CollectionSchema => ({
  id: c.id,
  fields: match(c.source)(
    [
      sync$(),
      ({
        content,
      }): ReadonlyArray<
        Readonly<{
          kw: SoftStr;
          numeric: boolean;
        }>
      > => fieldsOfRows(content([] as Path)),
    ],
    [
      async$(),
      (): ReadonlyArray<
        Readonly<{
          kw: SoftStr;
          numeric: boolean;
        }>
      > => [],
    ],
    [
      dynamic$(),
      (): ReadonlyArray<
        Readonly<{
          kw: SoftStr;
          numeric: boolean;
        }>
      > => [],
    ],
    [
      adapter$(),
      (): ReadonlyArray<
        Readonly<{
          kw: SoftStr;
          numeric: boolean;
        }>
      > => [],
    ],
  ),
  choices: matchOption<
    Query,
    ReadonlyArray<SoftStr>
  >(
    () => [],
    (q: Query) =>
      q.choices.map((ch: QueryChoice) => ch.id),
  )(c.query),
});

/**
 * Derives the {@link FlowSchema} a flow is checked against
 * from a declaration — every collection's id, its observed
 * field types, and its declared choice ids. The cross-
 * dialect binding set `readFlow` resolves names against.
 */
export const flowSchemaOf = (
  declaration: Declaration,
): FlowSchema =>
  flowSchema(
    declaration.collections.map(
      schemaOfCollection,
    ),
  );

/** Reports the flow's terminal state, stalling on a parked confirm. */
const stallOr = (
  host: FlowHost,
  done: RunOutcome,
): RunOutcome =>
  matchOption<ConfirmPrompt, RunOutcome>(
    () => done,
    (c: ConfirmPrompt) => stalled(c),
  )(host.scene.confirm);

/**
 * Drives the pause→settle→resume loop (the interpreter's
 * handshake): a dispatch pauses with its Msg, the host
 * settles it into the next scene, and the flow resumes —
 * until a terminal outcome. A `Done` flow that left a
 * destructive action parked reports `Stalled`; a `Failed`
 * flow reports its positioned diagnostic.
 */
const drive = (
  host: FlowHost,
  outcome: FlowOutcome,
): RunOutcome =>
  match(outcome)(
    [
      flowPaused$(),
      ({ content }): RunOutcome => {
        const next = host.settle(content.msg);
        return drive(
          next,
          resumeFlow(content, next.scene),
        );
      },
    ],
    [
      flowDone$(),
      (): RunOutcome =>
        stallOr(host, ran(outcome)),
    ],
    [
      flowFailed$(),
      (): RunOutcome => ran(outcome),
    ],
  );

/**
 * `run_flow`: reads + checks a flow DSL source against the
 * schema, then runs it fuel-bounded through the pure host
 * seam — the standing catalog tool the point-8 design
 * names. Total: a rejected script is a `Rejected` value
 * (positioned diagnostics), a parked destructive action is
 * a `Stalled` value, and every runtime error is a `Failed`
 * outcome inside `Ran` — never a throw.
 */
export const runFlow = (
  source: SoftStr,
  schema: FlowSchema,
  host: FlowHost,
  fuel: number = defaultFuel,
): RunOutcome => {
  const read = readFlow(source, schema);
  return isErr(read)
    ? rejected(read.content)
    : drive(
        host,
        startFlow(read.content, host.scene, fuel),
      );
};
