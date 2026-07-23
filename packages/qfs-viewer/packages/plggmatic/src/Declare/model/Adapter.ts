import {
  type SoftStr,
  type Option,
  type Result,
  some,
  none,
  getOr,
  fromNullable,
  matchOption,
  pipe,
} from "plgg";
import { type Row } from "plggmatic/Declare/model/Row";
import { type Path } from "plggmatic/Declare/model/Source";
import { type Verb } from "plggmatic/Declare/model/Action";

/**
 * The host-supplied ACTOR, passed as DATA at program start
 * (never named in the declaration): an identity and its
 * roles. The engine evaluates an action's declared
 * `authorize` over (actor, subject) BEFORE dispatching, so
 * an unauthorized action never reaches the adapter (the
 * point-7 authorization decision, 2026-07-13).
 */
export type Actor = Readonly<{
  id: SoftStr;
  roles: ReadonlyArray<SoftStr>;
}>;

/** Constructs an {@link Actor} (roles default empty). */
export const actor = (
  id: SoftStr,
  roles: ReadonlyArray<SoftStr> = [],
): Actor => ({ id, roles });

/**
 * A query-scoped READ descriptor: the collection and the
 * ancestor `path` a child reads against (the same Path the
 * legacy Source reads). The Sync/Async split does not
 * appear here — the adapter read is uniformly deferred (a
 * `Promise<Result>`), the concrete backend's concern.
 */
export type Scope = Readonly<{
  collection: SoftStr;
  path: Path;
}>;

/** Constructs a {@link Scope}. */
export const scope = (
  collection: SoftStr,
  path: Path,
): Scope => ({ collection, path });

/**
 * A checked mutation the host APPLIES: the collection, the
 * create/update/delete verb, the target row (`None` for a
 * create), and the projected field payload (key→value).
 * Constructing an effect here is the apply HAND-OFF; the
 * checked-effect construction from a compiled manifest
 * belongs to the manifest lowering (out of scope for this
 * layer).
 */
export type Effect = Readonly<{
  collection: SoftStr;
  verb: Verb;
  target: Option<SoftStr>;
  payload: ReadonlyArray<
    readonly [SoftStr, SoftStr]
  >;
}>;

/** Constructs an {@link Effect} (payload defaults empty). */
export const effect = (e: {
  collection: SoftStr;
  verb: Verb;
  target: Option<SoftStr>;
  payload?: ReadonlyArray<
    readonly [SoftStr, SoftStr]
  >;
}): Effect => ({
  collection: e.collection,
  verb: e.verb,
  target: e.target,
  payload: pipe(
    fromNullable(e.payload),
    getOr<
      ReadonlyArray<readonly [SoftStr, SoftStr]>
    >([]),
  ),
});

/**
 * The successful outcome of `apply`: the collection whose
 * rows changed and its refreshed rows, so the scheduler
 * folds Ok → a `Loaded` message on the action-result path,
 * mirroring a read.
 */
export type ApplyOk = Readonly<{
  collection: SoftStr;
  rows: ReadonlyArray<Row>;
}>;

/**
 * The ONE host capability (point 7): `read` — a
 * query-scoped, deferred read whose `Err` lands in the
 * collection's Failed slot (today's async path) — and
 * `apply` — a checked effect against an actor, its `Err`
 * surfacing on the action-result path. Nothing throws;
 * timeouts are the adapter's own concern. `toRow` is NOT
 * here: it left the capability surface for keyword
 * projection (see `Declare/usecase/project`).
 */
export type HostAdapter = Readonly<{
  read: (
    scope: Scope,
  ) => Promise<Result<ReadonlyArray<Row>, Error>>;
  apply: (
    effect: Effect,
    actor: Actor,
  ) => Promise<Result<ApplyOk, Error>>;
}>;

/** Constructs a {@link HostAdapter}. */
export const hostAdapter = (a: {
  read: (
    scope: Scope,
  ) => Promise<Result<ReadonlyArray<Row>, Error>>;
  apply: (
    effect: Effect,
    actor: Actor,
  ) => Promise<Result<ApplyOk, Error>>;
}): HostAdapter => ({
  read: a.read,
  apply: a.apply,
});

/**
 * A host adapter registered under a NAME. One registration
 * becomes the DEFAULT (declarations name no adapter);
 * several require the non-default collections to name
 * theirs (the point-7 naming decision).
 */
export type NamedAdapter = Readonly<{
  name: SoftStr;
  adapter: HostAdapter;
}>;

/** The host's registered adapters (empty when none). */
export type Registry =
  ReadonlyArray<NamedAdapter>;

/** Registers an adapter under a name. */
export const named = (
  name: SoftStr,
  adapter: HostAdapter,
): NamedAdapter => ({ name, adapter });

/** The adapter of a named registration, if present. */
const pick = (
  na: Option<NamedAdapter>,
): Option<HostAdapter> =>
  matchOption<NamedAdapter, Option<HostAdapter>>(
    () => none(),
    (n: NamedAdapter) => some(n.adapter),
  )(na);

/**
 * Resolves the adapter a source reads through. A named
 * source names it; an unnamed source (`None`) resolves to
 * the DEFAULT — the sole registration. Total: `None` when
 * the name is unknown, or when an unnamed source meets a
 * registry that is not exactly one (0 or ≥2) — the
 * reconciliation pass reports which; this lookup never
 * throws.
 */
export const resolveAdapter = (
  registry: Registry,
  name: Option<SoftStr>,
): Option<HostAdapter> =>
  matchOption<SoftStr, Option<HostAdapter>>(
    () =>
      registry.length === 1
        ? pick(fromNullable(registry[0]))
        : none(),
    (nm: SoftStr) =>
      pick(
        fromNullable(
          registry.find((r) => r.name === nm),
        ),
      ),
  )(name);
