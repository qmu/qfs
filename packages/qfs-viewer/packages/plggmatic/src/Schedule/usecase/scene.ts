import {
  type SoftStr,
  type Option,
  none,
  some,
  isSome,
  fromNullable,
  matchOption,
  match,
} from "plgg";
import {
  type Row,
  type Field,
  refTarget,
  fieldText,
} from "plggmatic/Declare/model/Row";
import {
  type Action,
  isDestructive,
  permits,
} from "plggmatic/Declare/model/Action";
import { type Actor } from "plggmatic/Declare/model/Adapter";
import {
  type Query,
  type QueryChoice,
  matchesQuery,
  matchesChoice,
} from "plggmatic/Declare/model/Query";
import { type Collection } from "plggmatic/Declare/model/Collection";
import { type Declaration } from "plggmatic/Declare/model/Declaration";
import {
  type Model,
  type Slot,
  type PendingAction,
  slotOf,
  choiceOf,
  idle$,
  loading$,
  loadedSlot$,
  failedSlot$,
} from "plggmatic/Schedule/model/Model";
import {
  type Scene,
  type Level,
  type Tile,
  type ActionButton,
  type DetailField,
  type QueryState,
  type QueryChoiceState,
  menuLevel,
  listLevel,
  boardLevel,
  detailLevel,
} from "plggmatic/Schedule/model/Scene";
import {
  type View,
  listView,
  detailView,
  binding,
  menuView$,
  listView$,
  detailView$,
} from "plggmatic/Schedule/model/View";
import { hrefFor } from "plggmatic/Schedule/usecase/codec";
import {
  focusedView,
  sliceOf,
} from "plggmatic/Schedule/usecase/lower";
import {
  chainCollections,
  ancestorPath,
} from "plggmatic/Schedule/usecase/chain";

/** A slot read down to its display parts (total fold). */
type SlotView = Readonly<{
  rows: ReadonlyArray<Row>;
  loading: boolean;
  error: Option<SoftStr>;
}>;

const readSlot = (slot: Slot): SlotView =>
  match(slot)(
    [
      idle$(),
      (): SlotView => ({
        rows: [],
        loading: false,
        error: none(),
      }),
    ],
    [
      loading$(),
      (): SlotView => ({
        rows: [],
        loading: true,
        error: none(),
      }),
    ],
    [
      loadedSlot$(),
      ({ content }): SlotView => ({
        rows: content,
        loading: false,
        error: none(),
      }),
    ],
    [
      failedSlot$(),
      ({ content }): SlotView => ({
        rows: [],
        loading: false,
        error: some(content),
      }),
    ],
  );

/** Projects an action into a renderer button. */
const projectAction = (
  a: Action,
): ActionButton => ({
  id: a.id,
  label: a.label,
  verb: a.verb,
  destructive: isDestructive(a.confirm),
});

/** The href drilling a row selection at a given level. */
const drillHref = (
  model: Model,
  level: number,
  id: SoftStr,
): SoftStr =>
  hrefFor(model.base, {
    root: model.root,
    path: [
      ...ancestorPath(model.path, level),
      id,
    ],
    query: "",
  });

/** The href truncating the path to `n` selections. */
const truncateHref = (
  model: Model,
  n: number,
): SoftStr =>
  hrefFor(model.base, {
    root: model.root,
    path: ancestorPath(model.path, n),
    query: "",
  });

/** The href returning from an opened root list to menu-only. */
const menuOnlyHref = (model: Model): SoftStr =>
  hrefFor(model.base, {
    root: none(),
    path: [],
    query: "",
  });

/** The declared choices of a collection (empty when none). */
const choicesOf = (
  collection: Collection,
): ReadonlyArray<QueryChoice> =>
  matchOption<Query, ReadonlyArray<QueryChoice>>(
    () => [],
    (q: Query) => q.choices,
  )(collection.query);

/** Builds a `ListLevel` for a collection at a flow depth. */
const buildListLevel = (
  model: Model,
  actor: Option<Actor>,
  collection: Collection,
  index: number,
  activeListIndex: number,
): Level => {
  const view = readSlot(
    slotOf(model, collection.id),
  );
  const isActive = index === activeListIndex;
  // the active list filters by the keyword AND every
  // chosen declared choice — the closed evaluation pair
  // (substring / equality) of the point-4 decision.
  const filtered =
    isActive && isSome(collection.query)
      ? view.rows.filter(
          (r: Row) =>
            matchesQuery(model.query, r.label) &&
            choicesOf(collection).every(
              (c: QueryChoice) =>
                matchesChoice(
                  choiceOf(model, c.id),
                  c.field,
                  r,
                ),
            ),
        )
      : view.rows;
  const selectedId = fromNullable(
    model.path[index],
  );
  return listLevel({
    collection: collection.id,
    title: collection.title,
    back:
      index === 0
        ? some(menuOnlyHref(model))
        : some(truncateHref(model, index - 1)),
    query: isActive
      ? matchOption<Query, Option<QueryState>>(
          () => none(),
          (q: Query) =>
            some({
              placeholder: q.placeholder,
              text: model.query,
              choices: q.choices.map(
                (
                  c: QueryChoice,
                ): QueryChoiceState => ({
                  id: c.id,
                  label: c.label,
                  options: c.options,
                  value: choiceOf(model, c.id),
                }),
              ),
            }),
        )(collection.query)
      : none(),
    rows: filtered.map((r: Row) => ({
      row: r,
      href: drillHref(model, index, r.id),
      active: matchOption<SoftStr, boolean>(
        () => false,
        (sid: SoftStr) => sid === r.id,
      )(selectedId),
    })),
    loading: view.loading,
    error: view.error,
    // create actions the actor may run (subject `None` — a
    // create has no target yet); an unauthorized action
    // projects no button (the legality fold, point 7).
    actions: collection.actions
      .filter((a: Action) => a.verb === "create")
      .filter((a: Action) =>
        permits(a, actor, none()),
      )
      .map((a: Action) => projectAction(a)),
  });
};

/** A reference target's shape (collection + row id). */
type RefTarget = Readonly<{
  collection: SoftStr;
  id: SoftStr;
}>;

/**
 * The canonical address a board tile's reference jumps
 * to: the target ROW's detail when an id is given (the
 * same address a `DetailField.href` resolves), or the
 * target collection's LIST when the id is empty — a tile
 * referencing the section itself jumps to the section
 * (the point-5 grounding: a dashboard tile summarizes a
 * section, so its jump lands on that section's list).
 */
const tileJumpHref = (
  model: Model,
  t: RefTarget,
): SoftStr =>
  t.id === ""
    ? hrefFor(
        model.base,
        sliceOf(listView(t.collection), []),
      )
    : hrefFor(
        model.base,
        sliceOf(detailView(t.collection), [
          binding(t.collection, t.id),
        ]),
      );

/** The first `Reference` target among a row's fields. */
const firstRef = (
  fields: ReadonlyArray<Field>,
): Option<RefTarget> =>
  fromNullable(
    fields.flatMap((f: Field) =>
      matchOption<
        RefTarget,
        ReadonlyArray<RefTarget>
      >(
        () => [],
        (t: RefTarget) => [t],
      )(refTarget(f.value)),
    )[0],
  );

/**
 * One board tile from a row: the label as the headline,
 * the FIRST field as the caption, and the first
 * `Reference` field's target resolved to the tile's jump
 * href (`None` when the row carries no reference — a
 * tile with no interaction at all).
 */
const tileOf =
  (model: Model) =>
  (r: Row): Tile => ({
    label: r.label,
    caption: matchOption<Field, SoftStr>(
      () => "",
      (f: Field) => fieldText(f.value),
    )(fromNullable(r.fields[0])),
    href: matchOption<RefTarget, Option<SoftStr>>(
      () => none(),
      (t: RefTarget) =>
        some(tileJumpHref(model, t)),
    )(firstRef(r.fields)),
  });

/**
 * Builds a `BoardLevel` for a board collection at a flow
 * depth — projected INSTEAD of a `ListLevel` when the
 * collection opts into board presentation. No query (the
 * keyword/choice pair does not apply to boards in v1)
 * and no drill: each row becomes a tile whose only
 * interaction is its jump.
 */
const buildBoardLevel = (
  model: Model,
  collection: Collection,
  index: number,
): Level => {
  const view = readSlot(
    slotOf(model, collection.id),
  );
  return boardLevel({
    collection: collection.id,
    title: collection.title,
    back:
      index === 0
        ? some(menuOnlyHref(model))
        : some(truncateHref(model, index - 1)),
    tiles: view.rows.map(tileOf(model)),
    loading: view.loading,
    error: view.error,
  });
};

/**
 * The `DetailLevel`, if the focused view is a detail —
 * i.e. the derived {@link focusedView} settled on the
 * deepest childless selection (a `Some` child means the
 * selection drilled into a list instead, so no detail).
 * A BOARD collection never shows a detail: its rows do
 * not drill (a tile's only interaction is its jump), so
 * a selection under a board — a hand-typed URL or a
 * programmatic `Select` — is structurally inert rather
 * than a bogus "detail of a tile".
 */
const buildDetail = (
  model: Model,
  actor: Option<Actor>,
  focused: View,
  chain: ReadonlyArray<Collection>,
): ReadonlyArray<Level> =>
  match(focused)(
    [menuView$(), (): ReadonlyArray<Level> => []],
    [listView$(), (): ReadonlyArray<Level> => []],
    [
      detailView$(),
      ({ content }): ReadonlyArray<Level> =>
        matchOption<
          Collection,
          ReadonlyArray<Level>
        >(
          () => [],
          (c: Collection) =>
            c.board
              ? []
              : detailFor(model, actor, c),
        )(
          fromNullable(
            chain.find(
              (c: Collection) => c.id === content,
            ),
          ),
        ),
    ],
  );

/**
 * Projects a row's fields for the renderer, resolving
 * each `Reference` value to the href of its target's
 * CANONICAL address — a jump (`sliceOf` over the target's
 * own detail view), never a walk from here. Built in the
 * scene so renderers stay model-free.
 */
const projectFields = (
  model: Model,
  fields: ReadonlyArray<Field>,
): ReadonlyArray<DetailField> =>
  fields.map((f: Field) => ({
    label: f.label,
    value: f.value,
    href: matchOption<
      Readonly<{
        collection: SoftStr;
        id: SoftStr;
      }>,
      Option<SoftStr>
    >(
      () => none(),
      (t): Option<SoftStr> =>
        some(
          hrefFor(
            model.base,
            sliceOf(detailView(t.collection), [
              binding(t.collection, t.id),
            ]),
          ),
        ),
    )(refTarget(f.value)),
  }));

const detailFor = (
  model: Model,
  actor: Option<Actor>,
  c: Collection,
): ReadonlyArray<Level> =>
  matchOption<SoftStr, ReadonlyArray<Level>>(
    () => [],
    (rowId: SoftStr) => {
      const found = fromNullable(
        readSlot(slotOf(model, c.id)).rows.find(
          (r: Row) => r.id === rowId,
        ),
      );
      return [
        detailLevel({
          collection: c.id,
          // the item's OWN label — what you're viewing —
          // so the detail identifies the selection (the
          // oracle's reader showed the note title, not the
          // collection); falls back to the collection
          // title when the row is not yet loaded / gone.
          title: matchOption<Row, SoftStr>(
            () => c.title,
            (r: Row) => r.label,
          )(found),
          back: some(
            truncateHref(
              model,
              model.path.length - 1,
            ),
          ),
          row: found,
          fields: matchOption<
            Row,
            ReadonlyArray<DetailField>
          >(
            () => [],
            (r: Row) =>
              projectFields(model, r.fields),
          )(found),
          // update/delete actions the actor may run on
          // THIS row (subject = the selected row id); an
          // unauthorized action projects no button.
          actions: c.actions
            .filter(
              (a: Action) => a.verb !== "create",
            )
            .filter((a: Action) =>
              permits(a, actor, some(rowId)),
            )
            .map((a: Action) => projectAction(a)),
        }),
      ];
    },
  )(
    fromNullable(
      model.path[model.path.length - 1],
    ),
  );

/**
 * Derives the {@link Scene} — the typed, mode-agnostic
 * renderer seam — from a model. The `MenuLevel`, then a
 * `ListLevel` per revealed flow depth (root list plus
 * every drilled-into child list), then a `DetailLevel`
 * when the deepest childless selection shows an item, and
 * any pending confirmation. Renderers 10/11 draw this
 * without re-deriving from the model or the declaration.
 */
export const makeScene =
  (
    declaration: Declaration,
    actor: Option<Actor>,
  ) =>
  (model: Model): Scene => {
    const chain = chainCollections(
      declaration,
      model.root,
    );
    const focused = focusedView(declaration)(
      model.root,
      model.path,
    );
    // The active list is the focused list itself, clamped
    // to the deepest resolvable chain entry when the
    // focused view addresses past it (dangling child /
    // junk path) or sits behind a detail.
    const activeListIndex = match(focused)(
      [menuView$(), (): number => -1],
      [
        listView$(),
        ({ content }): number => {
          const at = chain.findIndex(
            (c: Collection) => c.id === content,
          );
          return at >= 0 ? at : chain.length - 1;
        },
      ],
      [
        detailView$(),
        (): number => chain.length - 1,
      ],
    );
    return {
      title: declaration.title,
      levels: [
        menuLevel(
          declaration.title,
          declaration.menu.entries.map((e) => ({
            label: e.label,
            href: hrefFor(model.base, {
              root: some(e.collection),
              path: [],
              query: "",
            }),
            active: matchOption<SoftStr, boolean>(
              () => false,
              (r: SoftStr) => r === e.collection,
            )(model.root),
          })),
        ),
        ...chain
          .map((collection, index) => ({
            collection,
            index,
          }))
          .filter(
            (e) => e.index <= model.path.length,
          )
          .map((e) =>
            e.collection.board
              ? buildBoardLevel(
                  model,
                  e.collection,
                  e.index,
                )
              : buildListLevel(
                  model,
                  actor,
                  e.collection,
                  e.index,
                  activeListIndex,
                ),
          ),
        ...buildDetail(
          model,
          actor,
          focused,
          chain,
        ),
      ],
      confirm: matchOption<
        PendingAction,
        Option<
          Readonly<{
            prompt: SoftStr;
            destructive: boolean;
          }>
        >
      >(
        () => none(),
        (p: PendingAction) =>
          some({
            prompt: p.prompt,
            destructive: p.destructive,
          }),
      )(model.pending),
    };
  };
