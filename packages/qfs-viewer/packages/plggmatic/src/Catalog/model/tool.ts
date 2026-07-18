import {
  type SoftStr,
  type Box,
  type Icon,
  box,
  icon,
  pattern,
} from "plgg";
import { type SchedulerMsg } from "plggmatic/Schedule/model/Msg";

/**
 * A tool's argument schema — the CLOSED engine union the
 * catalog fold emits, kept transport-neutral: the JSON-
 * Schema serialization a WebMCP (or any) adapter needs is
 * the ADAPTER's job, derived from this data. Every v1 tool
 * takes at most one argument, so the union is a single
 * argument slot:
 *
 * - `Nullary` — no argument (a create/row action, whose
 *   target is fixed by the scene position).
 * - `TextArg` — one free-text argument (the keyword filter,
 *   the `run_flow` script source).
 * - `EnumArg` — one argument drawn from a CLOSED option set
 *   (a row-selection id, a declared query choice, a menu
 *   section). An EMPTY option set withholds the choices
 *   deliberately (the row-cap guard) — never a free string.
 */
export type ToolInput =
  | Icon<"Nullary">
  | Box<
      "TextArg",
      Readonly<{
        name: SoftStr;
        description: SoftStr;
      }>
    >
  | Box<
      "EnumArg",
      Readonly<{
        name: SoftStr;
        description: SoftStr;
        options: ReadonlyArray<SoftStr>;
      }>
    >;

/** A no-argument input. */
export const nullary = (): ToolInput =>
  icon("Nullary");

/** A single free-text argument. */
export const textArg = (
  name: SoftStr,
  description: SoftStr,
): ToolInput =>
  box("TextArg")({ name, description });

/** A single argument drawn from a closed option set. */
export const enumArg = (
  name: SoftStr,
  description: SoftStr,
  options: ReadonlyArray<SoftStr>,
): ToolInput =>
  box("EnumArg")({ name, description, options });

/** Matchers for folding a {@link ToolInput}. */
export const nullary$ = () =>
  pattern("Nullary")();
export const textArg$ = () =>
  pattern("TextArg")();
export const enumArg$ = () =>
  pattern("EnumArg")();

/**
 * What invoking a tool does — a CLOSED union so the adapter
 * folds it exhaustively:
 *
 * - `Emit` — lower the (validated) argument onto a single
 *   {@link SchedulerMsg} the host dispatches, EXACTLY as
 *   the human path does (the same `select`/`queryInput`/
 *   `requestAction`/`urlChanged` constructors the renderers
 *   wire). A `Nullary` tool ignores the passed `""`.
 * - `RunFlow` — the standing `run_flow` tool: the argument
 *   is a flow DSL source the adapter reads, checks, and
 *   runs against the current scene (see `runFlow`).
 */
export type ToolEffect =
  | Box<"Emit", (arg: SoftStr) => SchedulerMsg>
  | Icon<"RunFlow">;

/** Lowers the validated argument onto a scheduler message. */
export const emit = (
  lower: (arg: SoftStr) => SchedulerMsg,
): ToolEffect => box("Emit")(lower);

/** The `run_flow` effect (the argument is a flow source). */
export const runFlowEffect = (): ToolEffect =>
  icon("RunFlow");

/** Matchers for folding a {@link ToolEffect}. */
export const emit$ = () => pattern("Emit")();
export const runFlow$ = () =>
  pattern("RunFlow")();

/**
 * One engine-owned tool: a name (the diff/registration
 * key), a human/agent-readable description, its argument
 * schema, and the lowering of a validated argument onto an
 * effect. Pure DATA — a renderer-parallel fold from the
 * settled {@link import("plggmatic/Schedule/model/Scene").Scene}
 * produces the catalog; the WebMCP adapter is a thin,
 * disposable skin over this.
 */
export type Tool = Readonly<{
  name: SoftStr;
  description: SoftStr;
  input: ToolInput;
  effect: ToolEffect;
}>;

/** Constructs a {@link Tool}. */
export const tool = (t: {
  name: SoftStr;
  description: SoftStr;
  input: ToolInput;
  effect: ToolEffect;
}): Tool => t;
