import {
  type SoftStr,
  type Box,
  type Option,
  type Result,
  box,
  fromNullable,
  pattern,
} from "plgg";
import { type Row } from "plggmatic/Declare/model/Row";

/**
 * The ancestor selections a source reads against, root→
 * parent — so a child collection (notes) filters by its
 * parent selection (a section id) WITHOUT the model
 * storing the parent object. Empty at the root.
 */
export type Path = ReadonlyArray<SoftStr>;

/**
 * A typed data-access seam over items `T`. Four shapes:
 * `Sync` is an in-memory read, `Async` a deferred
 * (`proc`/Promise-folded-to-`Result`) read the scheduler
 * runs as a `Cmd` (design tenet e — swapping `Sync` for
 * `Async` never rewrites the app), and `Dynamic` is a
 * consumer-OWNED collection whose rows live in the model's
 * slot rather than being read from a fixed thunk. `Dynamic`
 * carries no read: the scheduler PRESERVES its slot on
 * re-read, and the consumer supplies/updates the rows from
 * data ITS Model owns via `Scheduled.withRows` — so a
 * record created at runtime lives in the Model and
 * `update` stays a pure `(msg, model) → [model, cmd]`
 * instead of forcing a module-global store (ticket
 * 20260708192518). `Sync`/`Async` are unchanged; a third
 * variant never rewrites an existing consumer.
 */
export type TypedSource<T> =
  | Box<"Sync", (path: Path) => ReadonlyArray<T>>
  | Box<
      "Async",
      (
        path: Path,
      ) => Promise<
        Result<ReadonlyArray<T>, Error>
      >
    >
  | Box<"Dynamic", null>
  | Box<"Adapter", Option<SoftStr>>;

/**
 * The erased, `Row`-valued source the scheduler consumes
 * (a {@link TypedSource} with its items already projected
 * through the collection's `toRow`). Same two variants,
 * so the sync/async symmetry survives erasure.
 */
export type Source =
  | Box<
      "Sync",
      (path: Path) => ReadonlyArray<Row>
    >
  | Box<
      "Async",
      (
        path: Path,
      ) => Promise<
        Result<ReadonlyArray<Row>, Error>
      >
    >
  | Box<"Dynamic", null>
  | Box<"Adapter", Option<SoftStr>>;

/** A synchronous typed source. */
export const sync = <T>(
  read: (path: Path) => ReadonlyArray<T>,
): TypedSource<T> => box("Sync")(read);

/** A deferred typed source (folded to a `Cmd`). */
export const async = <T>(
  read: (
    path: Path,
  ) => Promise<Result<ReadonlyArray<T>, Error>>,
): TypedSource<T> => box("Async")(read);

/**
 * A consumer-owned (Model-driven) source: the collection's
 * rows live in the scheduler slot, seeded/updated by the
 * consumer from data ITS Model owns via
 * `Scheduled.withRows`, and PRESERVED across navigation.
 * Carries no read thunk (hence `null`) — the point is that
 * no fixed thunk closes over external state, so the
 * consumer's `update` stays pure.
 */
export const dynamic = <T>(): TypedSource<T> =>
  box("Dynamic")(null);

/**
 * A host-adapter source (point 7): the collection's rows
 * come from a registered {@link HostAdapter}'s `read`, run
 * as a deferred `Cmd` exactly like `Async`. The optional
 * `name` selects a NAMED adapter; omitting it reads the
 * DEFAULT (the sole registration) — the common
 * single-backend case names nothing. The read carries no
 * thunk here (the adapter lives in the registry, resolved
 * at read time); an unresolved name is reported by the
 * startup reconciliation and, defensively, lands a Failed
 * slot rather than throwing.
 */
export const adapter = <T>(
  name?: SoftStr,
): TypedSource<T> =>
  box("Adapter")(fromNullable(name));

/** Matchers for folding a {@link Source}. */
export const sync$ = () => pattern("Sync")();
export const async$ = () => pattern("Async")();
export const dynamic$ = () =>
  pattern("Dynamic")();
export const adapter$ = () =>
  pattern("Adapter")();
