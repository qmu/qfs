import {
  type SoftStr,
  type Option,
  some,
  none,
  fromNullable,
  match,
  matchOption,
} from "plgg";
import { makeUrl } from "plgg-view/client";
import { type Row } from "plggmatic/Declare/model/Row";
import {
  type Scene,
  type Level,
  type MenuLink,
  type RowLink,
  type ActionButton,
  type QueryState,
  type QueryChoiceState,
  type Tile,
  menuLevel$,
  listLevel$,
  boardLevel$,
  detailLevel$,
} from "plggmatic/Schedule/model/Scene";
import {
  type SchedulerMsg,
  select,
  queryInput,
  queryChoiceInput,
  requestAction,
  urlChanged,
} from "plggmatic/Schedule/model/Msg";
import {
  type Tool,
  tool,
  nullary,
  textArg,
  enumArg,
  emit,
  runFlowEffect,
} from "plggmatic/Catalog/model/tool";

/**
 * The fold from the settled {@link Scene} to the engine-
 * owned tool catalog — the THIRD renderer, beside the two
 * HTML renderers: the SAME closed {@link Level} union, the
 * SAME exhaustive `match`, so a new level kind is a compile
 * error here too (mission point 8, 2026-07-13). A tool
 * lowers its validated argument onto the very
 * {@link SchedulerMsg} the renderers' human path dispatches
 * (navigation is `select`/`urlChanged`; the query, actions,
 * and flow are their own messages) — no parallel dispatch.
 *
 * What each level contributes:
 * - `MenuLevel` → one `open_menu` tool whose section enum is
 *   the menu labels; choosing one navigates to that section.
 * - `ListLevel` → a `select` tool (its id enum is the
 *   VISIBLE rows, capped), the keyword and declared-choice
 *   FILTER tools, and the create-action tools the legality
 *   projection already left on the scene. A still-loading
 *   list contributes NOTHING.
 * - `BoardLevel` → one `jump` tool over the tiles that carry
 *   a jump; a loading board contributes nothing.
 * - `DetailLevel` → the row-action tools (update/delete) the
 *   legality projection left, targeted at the loaded row.
 * Plus the standing `run_flow` tool at every scene.
 */

/**
 * The visible-row cap: a `select` tool enumerates at most
 * this many row ids. Over the cap the choices are WITHHELD
 * (an empty enum) and the description directs the agent to
 * filter first — never a free-string id fallback.
 */
export const rowCap = 50;

/**
 * Reconstructs the {@link import("plgg-view/client").Url}
 * the runtime would see for a scene href — the exact human
 * path (a link click navigates the browser, whose location
 * the runtime folds to `UrlChanged`). A scene href is
 * `base + "?search"`, so it splits at the first `?`.
 */
const hrefToUrl = (href: SoftStr) => {
  const at = href.indexOf("?");
  return at < 0
    ? makeUrl(href, "")
    : makeUrl(href.slice(0, at), href.slice(at));
};

/** The href of a labelled navigation target (falls back to the first). */
const hrefByLabel = (
  pairs: ReadonlyArray<
    readonly [SoftStr, SoftStr]
  >,
  label: SoftStr,
  fallback: SoftStr,
): SoftStr =>
  matchOption<
    readonly [SoftStr, SoftStr],
    SoftStr
  >(
    () => fallback,
    ([, href]) => href,
  )(
    fromNullable(
      pairs.find(([l]) => l === label),
    ),
  );

/** The `open_menu` tool for the menu entries (none when empty). */
const menuTools = (
  entries: ReadonlyArray<MenuLink>,
): ReadonlyArray<Tool> => {
  const pairs = entries.map(
    (
      e: MenuLink,
    ): readonly [SoftStr, SoftStr] => [
      e.label,
      e.href,
    ],
  );
  const first = pairs[0];
  return first === undefined
    ? []
    : [
        tool({
          name: "open_menu",
          description:
            "Open a top-level section from the menu.",
          input: enumArg(
            "section",
            "the section to open",
            pairs.map(([label]) => label),
          ),
          effect: emit((label: SoftStr) =>
            urlChanged(
              hrefToUrl(
                hrefByLabel(
                  pairs,
                  label,
                  first[1],
                ),
              ),
            ),
          ),
        }),
      ];
};

/** One list level's content (the `ListLevel` box payload). */
type ListContent = Readonly<{
  collection: SoftStr;
  title: SoftStr;
  back: Option<SoftStr>;
  query: Option<QueryState>;
  rows: ReadonlyArray<RowLink>;
  loading: boolean;
  error: Option<SoftStr>;
  actions: ReadonlyArray<ActionButton>;
}>;

/** The `select` tool for a list's visible rows (capped). */
const selectTool = (
  c: ListContent,
  depth: number,
): ReadonlyArray<Tool> => {
  const ids = c.rows.map(
    (r: RowLink) => r.row.id,
  );
  if (ids.length === 0) return [];
  const overCap = ids.length > rowCap;
  return [
    tool({
      name: `select_${c.collection}`,
      description: overCap
        ? `${ids.length} rows exceed the ${rowCap}-row cap — narrow ${c.collection} with the filter tools first, then row choices appear.`
        : `Open one of the ${ids.length} visible ${c.collection} rows by id.`,
      input: enumArg(
        "id",
        overCap
          ? "withheld until the list is filtered below the cap"
          : "the id of the row to open",
        overCap ? [] : ids,
      ),
      effect: emit((id: SoftStr) =>
        select(depth, id),
      ),
    }),
  ];
};

/** The keyword-filter tool for an active list (none otherwise). */
const keywordTool = (
  c: ListContent,
): ReadonlyArray<Tool> =>
  matchOption<QueryState, ReadonlyArray<Tool>>(
    () => [],
    (q: QueryState) => [
      tool({
        name: `filter_${c.collection}`,
        description: `Filter ${c.collection} by keyword (substring over the row label).`,
        input: textArg("keyword", q.placeholder),
        effect: emit((text: SoftStr) =>
          queryInput(text),
        ),
      }),
    ],
  )(c.query);

/** The declared-choice filter tools for an active list. */
const choiceTools = (
  c: ListContent,
): ReadonlyArray<Tool> =>
  matchOption<QueryState, ReadonlyArray<Tool>>(
    () => [],
    (q: QueryState) =>
      q.choices.map((ch: QueryChoiceState) =>
        tool({
          name: `filter_${c.collection}_${ch.id}`,
          description: `Filter ${c.collection} by ${ch.label}.`,
          input: enumArg(
            ch.id,
            ch.label,
            ch.options,
          ),
          effect: emit((value: SoftStr) =>
            queryChoiceInput(ch.id, value),
          ),
        }),
      ),
  )(c.query);

/**
 * An action button lowered to a tool: a nullary tool whose
 * invocation requests the action on `target` — exactly the
 * renderer's `requestAction`. The buttons are ALREADY the
 * legality projection (an action the actor may not run left
 * no button, so it yields no tool — point 7).
 */
const actionTool =
  (
    collection: SoftStr,
    target: Option<SoftStr>,
  ) =>
  (ab: ActionButton): Tool =>
    tool({
      name: `${collection}_${ab.id}`,
      description: matchOption<SoftStr, SoftStr>(
        () =>
          `${ab.label} — create in ${collection}.`,
        (id: SoftStr) =>
          `${ab.label} — on ${collection} ${id}.`,
      )(target),
      input: nullary(),
      effect: emit(() =>
        requestAction(collection, ab.id, target),
      ),
    });

/** A list level's tools (empty while the list is loading). */
const listTools = (
  c: ListContent,
  depth: number,
): ReadonlyArray<Tool> =>
  c.loading
    ? []
    : [
        ...selectTool(c, depth),
        ...keywordTool(c),
        ...choiceTools(c),
        ...c.actions.map(
          actionTool(c.collection, none()),
        ),
      ];

/** One board level's content (the `BoardLevel` box payload). */
type BoardContent = Readonly<{
  collection: SoftStr;
  title: SoftStr;
  back: Option<SoftStr>;
  tiles: ReadonlyArray<Tile>;
  loading: boolean;
  error: Option<SoftStr>;
}>;

/** A board level's `jump` tool over its jumpable tiles. */
const boardTools = (
  c: BoardContent,
): ReadonlyArray<Tool> => {
  if (c.loading) return [];
  const pairs = c.tiles.flatMap(
    (
      t: Tile,
    ): ReadonlyArray<
      readonly [SoftStr, SoftStr]
    > =>
      matchOption<
        SoftStr,
        ReadonlyArray<readonly [SoftStr, SoftStr]>
      >(
        () => [],
        (href: SoftStr) => [[t.label, href]],
      )(t.href),
  );
  const first = pairs[0];
  return first === undefined
    ? []
    : [
        tool({
          name: `jump_${c.collection}`,
          description: `Jump to a ${c.collection} tile's target.`,
          input: enumArg(
            "tile",
            "the tile to jump to",
            pairs.map(([label]) => label),
          ),
          effect: emit((label: SoftStr) =>
            urlChanged(
              hrefToUrl(
                hrefByLabel(
                  pairs,
                  label,
                  first[1],
                ),
              ),
            ),
          ),
        }),
      ];
};

/** One detail level's content (the `DetailLevel` box payload). */
type DetailContent = Readonly<{
  collection: SoftStr;
  title: SoftStr;
  back: Option<SoftStr>;
  row: Option<Row>;
  fields: ReadonlyArray<unknown>;
  actions: ReadonlyArray<ActionButton>;
}>;

/**
 * A detail level's row-action tools — targeted at the
 * LOADED row (a detail whose row is not yet loaded exposes
 * no id, so it yields no action tools).
 */
const detailTools = (
  c: DetailContent,
): ReadonlyArray<Tool> =>
  matchOption<Row, ReadonlyArray<Tool>>(
    () => [],
    (r: Row) =>
      c.actions.map(
        actionTool(c.collection, some(r.id)),
      ),
  )(c.row);

/** The standing `run_flow` tool (present at every scene). */
const runFlowTool: Tool = tool({
  name: "run_flow",
  description:
    "Read, check, and run a flow-DSL script against the current screen; results and positioned diagnostics come back as values.",
  input: textArg(
    "flow",
    "an s-expression (flow …) script",
  ),
  effect: runFlowEffect(),
});

/**
 * The full tool catalog for a settled scene. The menu is
 * always at level 0; a list/board follows at flow depth
 * `position - 1` (the chain index `select` and the scene
 * addressing use). Loading collections contribute nothing;
 * `run_flow` is always available.
 */
export const catalogOf = (
  scene: Scene,
): ReadonlyArray<Tool> => [
  ...scene.levels.flatMap(
    (
      level: Level,
      position: number,
    ): ReadonlyArray<Tool> =>
      match(level)(
        [
          menuLevel$(),
          ({ content }): ReadonlyArray<Tool> =>
            menuTools(content.entries),
        ],
        [
          listLevel$(),
          ({ content }): ReadonlyArray<Tool> =>
            listTools(content, position - 1),
        ],
        [
          boardLevel$(),
          ({ content }): ReadonlyArray<Tool> =>
            boardTools(content),
        ],
        [
          detailLevel$(),
          ({ content }): ReadonlyArray<Tool> =>
            detailTools(content),
        ],
      ),
  ),
  runFlowTool,
];
