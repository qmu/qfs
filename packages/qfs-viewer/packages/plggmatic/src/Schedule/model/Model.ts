import {
  type SoftStr,
  type Box,
  type Icon,
  type Option,
  none,
  fromNullable,
  matchOption,
  pipe,
  icon,
  box,
  pattern,
} from "plgg";
import { type Row } from "plggmatic/Declare/model/Row";
import { type Action } from "plggmatic/Declare/model/Action";

/**
 * A collection's load state — the resource slot. `Idle`
 * before a read, `Loading` while a `Cmd` is in flight,
 * `Loaded` with rows, `Failed` with a message. A closed
 * union so a renderer must acknowledge every state (no
 * bare "maybe null rows").
 */
export type Slot =
  | Icon<"Idle">
  | Icon<"Loading">
  | Box<"Loaded", ReadonlyArray<Row>>
  | Box<"Failed", SoftStr>;

/** The idle slot (never read). */
export const idle = (): Slot => icon("Idle");
/** The loading slot (a read is in flight). */
export const loading = (): Slot =>
  icon("Loading");
/** A loaded slot carrying rows. */
export const loadedSlot = (
  rows: ReadonlyArray<Row>,
): Slot => box("Loaded")(rows);
/** A failed slot carrying a message. */
export const failedSlot = (
  error: SoftStr,
): Slot => box("Failed")(error);

/** Matchers for folding a {@link Slot}. */
export const idle$ = () => pattern("Idle")();
export const loading$ = () =>
  pattern("Loading")();
export const loadedSlot$ = () =>
  pattern("Loaded")();
export const failedSlot$ = () =>
  pattern("Failed")();

/**
 * A destructive action parked awaiting confirmation — the
 * scheduler state that makes confirm/cancel modeless and
 * data-driven (not renderer folklore). Carries which
 * collection, the full {@link Action} (run on confirm),
 * the target row id (`None` for a create), and the
 * `prompt`/`destructive` copied off the action's
 * `Confirm` at park time — so the scene reads them
 * directly, with no match over a branch that can never
 * be parked (only a `Confirm` action ever parks).
 */
export type PendingAction = Readonly<{
  collection: SoftStr;
  action: Action;
  target: Option<SoftStr>;
  prompt: SoftStr;
  destructive: boolean;
}>;

/**
 * The scheduled model — everything plgg-view's
 * `Application` needs minus the view, carrying ONLY
 * mode-independent truth (design tenet g): the mount
 * `base`, the chosen `root` collection, the drill-down
 * `path` of selected row ids (root→leaf), the active
 * `query` text with the chosen `queryChoices` values (the
 * declared typed query fields, point 4), the
 * per-collection load `slots`, and the `pending`
 * confirmation. "Which single screen is focused" is
 * derived (`focusedView`), never stored.
 *
 * `slots` is an assoc list (not a `Dict`, whose values
 * must be `Datum`): lookup is a total helper returning
 * `Idle` for an absent key.
 */
export type Model = Readonly<{
  base: SoftStr;
  root: Option<SoftStr>;
  path: ReadonlyArray<SoftStr>;
  query: SoftStr;
  queryChoices: ReadonlyArray<
    readonly [SoftStr, SoftStr]
  >;
  slots: ReadonlyArray<readonly [SoftStr, Slot]>;
  pending: Option<PendingAction>;
}>;

/** The empty model at a mount `base`. */
export const emptyModel = (
  base: SoftStr,
): Model => ({
  base,
  root: none(),
  path: [],
  query: "",
  queryChoices: [],
  slots: [],
  pending: none(),
});

/**
 * The chosen value of a declared query choice, or `""`
 * (no filter) when unset. Total — an assoc lookup folded
 * through `Option`.
 */
export const choiceOf = (
  model: Model,
  choice: SoftStr,
): SoftStr =>
  pipe(
    fromNullable(
      model.queryChoices.find(
        ([id]) => id === choice,
      ),
    ),
    matchOption<
      readonly [SoftStr, SoftStr],
      SoftStr
    >(
      () => "",
      ([, value]) => value,
    ),
  );

/**
 * A collection's slot, or `Idle` if never read. Total —
 * an assoc lookup folded through `Option`, no dead index
 * guard (the pair is destructured in the `Some` branch).
 */
export const slotOf = (
  model: Model,
  collection: SoftStr,
): Slot =>
  pipe(
    fromNullable(
      model.slots.find(
        ([id]) => id === collection,
      ),
    ),
    matchOption<readonly [SoftStr, Slot], Slot>(
      () => idle(),
      ([, slot]) => slot,
    ),
  );

/**
 * Sets a collection's slot, replacing any prior entry.
 * Pure — returns a new slots list.
 */
export const setSlot = (
  model: Model,
  collection: SoftStr,
  slot: Slot,
): Model => ({
  ...model,
  slots: [
    ...model.slots.filter(
      ([id]) => id !== collection,
    ),
    [collection, slot],
  ],
});
