import {
  type SoftStr,
  type Option,
  getOr,
  fromNullable,
  match,
  box,
  mapResult,
  pipe,
} from "plgg";
import { type Row } from "plggmatic/Declare/model/Row";
import {
  type Path,
  type Source,
  type TypedSource,
  sync$,
  async$,
  dynamic$,
  adapter$,
} from "plggmatic/Declare/model/Source";
import { type Query } from "plggmatic/Declare/model/Query";
import { type Action } from "plggmatic/Declare/model/Action";

/**
 * A typed data resource in the declaration: an identity,
 * a title, a `Source`, and its place in the flow graph —
 * an optional `child` collection a row drills into, an
 * optional `Query` filter, and the create/update/delete
 * `Action`s available on it. The `Source` here is the
 * ERASED (`Row`-valued) source; a typed `T` never leaks
 * past {@link collection}'s `toRow`. `board` opts the
 * collection into board (tile) presentation: its rows
 * project as tiles whose only interaction is a jump —
 * they neither drill nor show a detail (the point-5
 * decision, 2026-07-12).
 */
export type Collection = Readonly<{
  id: SoftStr;
  title: SoftStr;
  source: Source;
  child: Option<SoftStr>;
  query: Option<Query>;
  actions: ReadonlyArray<Action>;
  board: boolean;
}>;

/**
 * Erases a typed source to a `Row`-valued one by mapping
 * every item through `toRow` — the single seam where a
 * concrete `T` becomes the scheduler's `Row`. `Sync`
 * maps the array; `Async` maps inside the resolved
 * `Result` (the deferred read is left deferred).
 */
const erase = <T>(
  source: TypedSource<T>,
  toRow: (item: T) => Row,
): Source =>
  match(source)(
    [
      sync$(),
      ({ content }): Source =>
        box("Sync")((path: Path) =>
          content(path).map(toRow),
        ),
    ],
    [
      async$(),
      ({ content }): Source =>
        box("Async")((path: Path) =>
          content(path).then(
            mapResult((items: ReadonlyArray<T>) =>
              items.map(toRow),
            ),
          ),
        ),
    ],
    // Dynamic carries no read — the consumer owns the rows
    // (via `Scheduled.withRows`), so `toRow` is not applied
    // here; the erased marker is identical to the typed one.
    [
      dynamic$(),
      (): Source => box("Dynamic")(null),
    ],
    // Adapter carries no thunk — the host adapter's `read`
    // already yields `Row`s (the host projects, via keyword
    // projection or its own mapping), so `toRow` is not
    // applied here; the erased marker keeps the adapter name.
    [
      adapter$(),
      ({ content }): Source =>
        box("Adapter")(content),
    ],
  );

/**
 * Declares a {@link Collection}. The ONLY place a typed
 * item `T` appears: `toRow` captures the list/detail
 * projection and erases the source to `Row`. `child`,
 * `query`, `actions`, and `board` are optional (absence
 * is `None` / `[]` / `false`, not `undefined` past this
 * boundary) — every existing declaration is untouched.
 */
export const collection = <T>(c: {
  id: SoftStr;
  title: SoftStr;
  toRow: (item: T) => Row;
  source: TypedSource<T>;
  child?: SoftStr;
  query?: Query;
  actions?: ReadonlyArray<Action>;
  board?: boolean;
}): Collection => ({
  id: c.id,
  title: c.title,
  source: erase(c.source, c.toRow),
  child: fromNullable(c.child),
  query: fromNullable(c.query),
  actions: pipe(
    fromNullable(c.actions),
    getOr<ReadonlyArray<Action>>([]),
  ),
  board: pipe(
    fromNullable(c.board),
    getOr(false),
  ),
});

/**
 * Finds a collection by id in a declaration's list.
 * Total: an unknown id yields `None`, never throws — the
 * scheduler treats a dangling child/menu reference as an
 * empty level rather than a crash.
 */
export const collectionById = (
  collections: ReadonlyArray<Collection>,
  id: SoftStr,
): Option<Collection> =>
  fromNullable(
    collections.find((c) => c.id === id),
  );

/** The action of a collection by id. */
export const actionById = (
  c: Collection,
  id: SoftStr,
): Option<Action> =>
  fromNullable(
    c.actions.find((a) => a.id === id),
  );
