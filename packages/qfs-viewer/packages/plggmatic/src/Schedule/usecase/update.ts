import {
  type SoftStr,
  type Option,
  none,
  some,
  fromNullable,
  getOr,
  pipe,
  match,
  matchOption,
  matchResult,
} from "plgg";
import {
  type Cmd,
  cmdNone,
  cmdBatch,
  cmdEffect,
} from "plgg-view/client";
import { type Row } from "plggmatic/Declare/model/Row";
import {
  type Collection,
  collectionById,
  actionById,
} from "plggmatic/Declare/model/Collection";
import {
  type Action,
  permits,
  immediate$,
  confirm$,
} from "plggmatic/Declare/model/Action";
import {
  type Actor,
  type HostAdapter,
  type Registry,
  scope,
  resolveAdapter,
} from "plggmatic/Declare/model/Adapter";
import {
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
  type Model,
  type PendingAction,
  emptyModel,
  loading,
  loadedSlot,
  failedSlot,
  setSlot,
} from "plggmatic/Schedule/model/Model";
import {
  type SchedulerMsg,
  loaded,
  failed,
  navigate$,
  urlChanged$,
  openMenu$,
  select$,
  queryInput$,
  queryChoiceInput$,
  requestAction$,
  confirmAction$,
  cancelAction$,
  loaded$,
  failed$,
} from "plggmatic/Schedule/model/Msg";
import {
  type UrlSlice,
  parseUrl,
} from "plggmatic/Schedule/usecase/codec";
import { sliceOf } from "plggmatic/Schedule/usecase/lower";
import {
  chainCollections,
  ancestorPath,
} from "plggmatic/Schedule/usecase/chain";

type Step = readonly [Model, Cmd<SchedulerMsg>];

/**
 * The host-bound engine context threaded through a derived
 * program: the adapter {@link Registry} an `Adapter` source
 * reads through, and the host-supplied {@link Actor} (if
 * any) the authorize gate evaluates. Empty/`None` for a
 * self-contained declaration (every legacy program), so the
 * option is additive.
 */
export type EngineOptions = Readonly<{
  registry: Registry;
  actor: Option<Actor>;
}>;

/** The no-host engine context (legacy default). */
export const noEngine: EngineOptions = {
  registry: [],
  actor: none(),
};

/**
 * Reads one collection into its slot against an ancestor
 * `path`. `Sync` resolves immediately into `Loaded`;
 * `Async` parks `Loading` and returns a `cmdEffect` the
 * runtime runs after paint — the deferred read folded to
 * a `Loaded`/`Failed` message INSIDE the thunk, so the
 * effect always resolves to a message and `update` never
 * awaits (effects are data — design tenet b). `Adapter`
 * resolves a registered host adapter and runs its `read`
 * exactly like `Async` (`Err` → the `Failed` slot); an
 * unresolved name lands a `Failed` slot rather than
 * throwing (the startup reconciliation reports the cause).
 */
const readInto = (
  registry: Registry,
  model: Model,
  collection: Collection,
  path: ReadonlyArray<SoftStr>,
): Step =>
  match(collection.source)(
    [
      sync$(),
      ({ content }): Step => [
        setSlot(
          model,
          collection.id,
          loadedSlot(content(path)),
        ),
        cmdNone(),
      ],
    ],
    [
      async$(),
      ({ content }): Step => [
        setSlot(model, collection.id, loading()),
        cmdEffect(() =>
          content(path).then(
            matchResult<
              ReadonlyArray<Row>,
              Error,
              SchedulerMsg
            >(
              (e: Error) =>
                failed(collection.id, e.message),
              (rows: ReadonlyArray<Row>) =>
                loaded(collection.id, rows),
            ),
          ),
        ),
      ],
    ],
    // Dynamic: consumer-owned rows. The re-read PRESERVES
    // the existing slot (the consumer set it from data its
    // Model owns via `withRows`) rather than reading a
    // fixed thunk — so a runtime-created record survives
    // navigation without a module-global store, and
    // `update` stays pure.
    [dynamic$(), (): Step => [model, cmdNone()]],
    [
      adapter$(),
      ({ content }): Step =>
        matchOption<HostAdapter, Step>(
          () => [
            setSlot(
              model,
              collection.id,
              failedSlot(
                "unresolved host adapter",
              ),
            ),
            cmdNone(),
          ],
          (ad: HostAdapter): Step => [
            setSlot(
              model,
              collection.id,
              loading(),
            ),
            cmdEffect(() =>
              ad
                .read(scope(collection.id, path))
                .then(
                  matchResult<
                    ReadonlyArray<Row>,
                    Error,
                    SchedulerMsg
                  >(
                    (e: Error) =>
                      failed(
                        collection.id,
                        e.message,
                      ),
                    (rows: ReadonlyArray<Row>) =>
                      loaded(collection.id, rows),
                  ),
                ),
            ),
          ],
        )(resolveAdapter(registry, content)),
    ],
  );

/**
 * Reads every collection revealed by the current
 * `root`/`path` — the chain up to the deepest selected
 * level (index ≤ `path.length`) — folding each read into
 * the model and batching its command. Reading fresh on
 * every navigation keeps a child list correct when its
 * parent selection changes (the notes reload for the new
 * section) without a slot-invalidation dance.
 */
const ensureChain =
  (
    declaration: Declaration,
    registry: Registry,
  ) =>
  (model: Model): Step => {
    const chain = chainCollections(
      declaration,
      model.root,
    );
    const [next, cmds] = chain
      .map((collection, index) => ({
        collection,
        index,
      }))
      .filter((e) => e.index <= model.path.length)
      .reduce<
        readonly [
          Model,
          ReadonlyArray<Cmd<SchedulerMsg>>,
        ]
      >(
        ([m, acc], e) => {
          const [m2, cmd] = readInto(
            registry,
            m,
            e.collection,
            ancestorPath(model.path, e.index),
          );
          return [m2, [...acc, cmd]];
        },
        [model, []],
      );
    return [next, cmdBatch(cmds)];
  };

/**
 * Chosen choice values normalized against the
 * declaration: only ids some collection DECLARES survive,
 * in declaration order, so equal states serialize
 * byte-equal (canonical) and junk URL params drop
 * silently — the same totality every slice field gets.
 */
const normalizeChoices = (
  declaration: Declaration,
  entries: ReadonlyArray<
    readonly [SoftStr, SoftStr]
  >,
): ReadonlyArray<readonly [SoftStr, SoftStr]> =>
  declaration.collections
    .flatMap((c) =>
      matchOption<
        Query,
        ReadonlyArray<QueryChoice>
      >(
        () => [],
        (q: Query) => q.choices,
      )(c.query),
    )
    .flatMap((choice) =>
      matchOption<
        readonly [SoftStr, SoftStr],
        ReadonlyArray<readonly [SoftStr, SoftStr]>
      >(
        () => [],
        ([, value]) => {
          const chosen: readonly [
            SoftStr,
            SoftStr,
          ] = [choice.id, value];
          return value === "" ? [] : [chosen];
        },
      )(
        fromNullable(
          entries.find(
            ([id]) => id === choice.id,
          ),
        ),
      ),
    );

/**
 * The one navigation funnel: settle on a slice, drop any
 * parked confirmation, and re-read the revealed chain.
 * Every navigation message — `Navigate` (the core),
 * `UrlChanged`, `OpenMenu`, `Select` — lowers to a slice
 * and lands here, so navigation semantics exist exactly
 * once (mission decision, 2026-07-12).
 */
const goto =
  (
    declaration: Declaration,
    registry: Registry,
  ) =>
  (model: Model, slice: UrlSlice): Step =>
    ensureChain(
      declaration,
      registry,
    )({
      ...model,
      root: slice.root,
      path: slice.path,
      query: slice.query,
      queryChoices: normalizeChoices(
        declaration,
        pipe(
          fromNullable(slice.choices),
          getOr<
            ReadonlyArray<
              readonly [SoftStr, SoftStr]
            >
          >([]),
        ),
      ),
      pending: none(),
    });

/**
 * Runs an action's `Cmd` factory against a target, or a
 * no-op when it cannot be resolved — the shared tail of
 * the immediate and confirmed action paths.
 */
const runAction = (
  action: Action,
  target: Option<SoftStr>,
): Cmd<SchedulerMsg> => action.run(target);

/**
 * Resolves a requested action and either runs it
 * immediately or parks a confirmation, per its
 * `Confirm` data. An unknown collection/action is a
 * no-op (total).
 */
const requestActionStep = (
  declaration: Declaration,
  actor: Option<Actor>,
  model: Model,
  collection: SoftStr,
  actionId: SoftStr,
  target: Option<SoftStr>,
): Step =>
  matchOption<Collection, Step>(
    () => [model, cmdNone()],
    (c: Collection) =>
      matchOption<Action, Step>(
        () => [model, cmdNone()],
        (a: Action): Step =>
          // the engine's authorize gate: an unauthorized
          // action never parks a confirmation and never
          // dispatches — it is a total no-op (point 7).
          !permits(a, actor, target)
            ? [model, cmdNone()]
            : match(a.confirm)(
                [
                  immediate$(),
                  (): Step => [
                    model,
                    runAction(a, target),
                  ],
                ],
                [
                  confirm$(),
                  ({ content }): Step => [
                    {
                      ...model,
                      pending: some({
                        collection,
                        action: a,
                        target,
                        prompt: content.prompt,
                        destructive:
                          content.destructive,
                      }),
                    },
                    cmdNone(),
                  ],
                ],
              ),
      )(actionById(c, actionId)),
  )(
    collectionById(
      declaration.collections,
      collection,
    ),
  );

/**
 * Derives the pure, pair-shaped `update` from a
 * declaration. Exhaustive `match` over every `Msg`;
 * touches no `window`/`document` and executes no effect —
 * async reads and action verbs are RETURNED as `Cmd`
 * data. Navigation messages set the `root`/`path`/`query`
 * slice then re-read the revealed chain.
 */
export const makeUpdate =
  (
    declaration: Declaration,
    options: EngineOptions,
  ) =>
  (msg: SchedulerMsg, model: Model): Step =>
    match(msg)(
      [
        navigate$(),
        ({ content }): Step =>
          goto(declaration, options.registry)(
            model,
            sliceOf(content.to, content.with),
          ),
      ],
      [
        urlChanged$(),
        ({ content }): Step =>
          goto(declaration, options.registry)(
            model,
            parseUrl(content),
          ),
      ],
      [
        openMenu$(),
        ({ content }): Step =>
          goto(declaration, options.registry)(
            model,
            {
              root: some(content),
              path: [],
              query: "",
            },
          ),
      ],
      [
        select$(),
        ({ content }): Step =>
          goto(declaration, options.registry)(
            model,
            {
              root: model.root,
              path: [
                ...ancestorPath(
                  model.path,
                  content.level,
                ),
                content.id,
              ],
              query: "",
            },
          ),
      ],
      [
        queryInput$(),
        ({ content }): Step => [
          { ...model, query: content },
          cmdNone(),
        ],
      ],
      [
        queryChoiceInput$(),
        ({ content }): Step => [
          {
            ...model,
            queryChoices: normalizeChoices(
              declaration,
              [
                ...model.queryChoices.filter(
                  ([id]) => id !== content.choice,
                ),
                [content.choice, content.value],
              ],
            ),
          },
          cmdNone(),
        ],
      ],
      [
        requestAction$(),
        ({ content }): Step =>
          requestActionStep(
            declaration,
            options.actor,
            model,
            content.collection,
            content.action,
            content.target,
          ),
      ],
      [
        confirmAction$(),
        (): Step =>
          matchOption<PendingAction, Step>(
            () => [model, cmdNone()],
            (p: PendingAction): Step =>
              // re-check the gate at confirm time — the
              // parked action still must be authorized for
              // the current actor before it dispatches
              // (defense in depth over the request-time gate).
              permits(
                p.action,
                options.actor,
                p.target,
              )
                ? [
                    { ...model, pending: none() },
                    runAction(p.action, p.target),
                  ]
                : [
                    { ...model, pending: none() },
                    cmdNone(),
                  ],
          )(model.pending),
      ],
      [
        cancelAction$(),
        (): Step => [
          { ...model, pending: none() },
          cmdNone(),
        ],
      ],
      [
        loaded$(),
        ({ content }): Step => [
          setSlot(
            model,
            content.collection,
            loadedSlot(content.rows),
          ),
          cmdNone(),
        ],
      ],
      [
        failed$(),
        ({ content }): Step => [
          setSlot(
            model,
            content.collection,
            failedSlot(content.error),
          ),
          cmdNone(),
        ],
      ],
    );

/**
 * The initial `[Model, Cmd]` for an entry URL — the empty
 * model seeded from the URL's slice, then the revealed
 * chain read. `init` for the derived program.
 */
export const makeInit =
  (
    declaration: Declaration,
    options: EngineOptions,
  ) =>
  (base: SoftStr) =>
  (slice: UrlSlice): Step =>
    goto(declaration, options.registry)(
      emptyModel(base),
      slice,
    );
