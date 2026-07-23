import {
  type SoftStr,
  type Box,
  box,
  pattern,
} from "plgg";
import { type SchedulerMsg } from "plggmatic/Schedule/model/Msg";
import {
  type FlowScript,
  type FlowValue,
} from "plggmatic/Flow/model/script";

/**
 * The interpreter's outcome vocabulary — the run-state
 * union of `dsl-v1-core.md` §1 (`run.ts`), prototype
 * scale. Everything here is inert, JSON-safe data:
 * failure is a value (never a throw — fuel semantics
 * §5.3), and the PAUSED state is the serializable
 * continuation the whole point-9 prototype exists to
 * prove (§5.4).
 */

/** Why a flow failed — a closed diagnostic code set. */
export type FlowDiagCode =
  | "fuel-exhausted"
  | "unknown-collection"
  | "unbound-name"
  | "type-mismatch"
  | "unknown-host";

/**
 * A failure diagnostic: the code and the step index the
 * reduction was working on (the prototype's stand-in for
 * the source range the checked IR will carry).
 */
export type FlowDiag = Readonly<{
  code: FlowDiagCode;
  at: number;
}>;

/**
 * A paused flow — the DEFUNCTIONALIZED continuation:
 * the script itself, the index of the next step, the
 * bindings computed so far, the REMAINING fuel (§5.4:
 * fuel serializes with the continuation and none is
 * consumed while paused), and the yielded Msg the
 * harness must dispatch before resuming.
 */
export type PausedFlow = Readonly<{
  script: FlowScript;
  next: number;
  env: Readonly<Record<string, FlowValue>>;
  fuel: number;
  msg: SchedulerMsg;
}>;

/**
 * The step outcome: paused on a dispatch, done with the
 * flow's value, or failed with a diagnostic.
 */
export type FlowOutcome =
  | Box<"FlowPaused", PausedFlow>
  | Box<"FlowDone", FlowValue>
  | Box<"FlowFailed", FlowDiag>;

/** Constructs a paused outcome. */
export const flowPaused = (
  p: PausedFlow,
): FlowOutcome => box("FlowPaused")(p);

/** Constructs a done outcome. */
export const flowDone = (
  value: FlowValue,
): FlowOutcome => box("FlowDone")(value);

/** Constructs a failed outcome. */
export const flowFailed = (
  code: FlowDiagCode,
  at: number,
): FlowOutcome => box("FlowFailed")({ code, at });

/** Matchers for folding a {@link FlowOutcome}. */
export const flowPaused$ = () =>
  pattern("FlowPaused")();
export const flowDone$ = () =>
  pattern("FlowDone")();
export const flowFailed$ = () =>
  pattern("FlowFailed")();

/** The spec-set default fuel budget (§5.2). */
export const defaultFuel = 10000;

/** A binding environment (JSON-safe). */
export type FlowEnv = Readonly<
  Record<string, FlowValue>
>;

/** The empty environment. */
export const emptyEnv = (): FlowEnv => ({});

/** Extends an environment with one binding. */
export const bind = (
  env: FlowEnv,
  name: SoftStr,
  value: FlowValue,
): FlowEnv => ({ ...env, [name]: value });
