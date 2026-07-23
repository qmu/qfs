import {
  type SoftStr,
  type Option,
  some,
  none,
  getOr,
  matchOption,
  match,
  pipe,
  fromNullable,
} from "plgg";
import {
  type Html,
  slot,
  span,
  a,
  href,
  text,
  attr,
  key,
  fadeIn,
  mapHtml,
} from "plgg-view";
import {
  style_,
  basis,
  fluid,
} from "plggmatic/styleEntry";
import {
  row,
  column,
  navPane,
  mainPane,
  asidePane,
} from "plggmatic/Layout/usecase/combinators";
import {
  type Crumb,
  breadcrumb,
} from "plggmatic/Component/usecase/breadcrumb";
import { colHead } from "plggmatic/Component/usecase/colHead";
import { type SchedulerMsg } from "plggmatic/Schedule/model/Msg";
import {
  type Scene,
  type Level,
  type ActionButton,
  menuLevel$,
  listLevel$,
  boardLevel$,
  detailLevel$,
  type DetailField,
} from "plggmatic/Schedule/model/Scene";
import { type Row } from "plggmatic/Declare/model/Row";
import { cssPrefix } from "plggmatic/Meta/model/identity";
import {
  confirmOverlay,
  actionRow,
  queryField,
  rowList,
  tileGrid,
  menuNav,
  loadingHint,
  errorHint,
  detailFields,
} from "plggmatic/Render/usecase/parts";

export type HeaderLink = Readonly<{
  collection: SoftStr;
  label: SoftStr;
  href: SoftStr;
  active?: boolean;
}>;

export type ExtraColumn<Msg> = Readonly<{
  key: SoftStr;
  title: SoftStr;
  close: Option<SoftStr>;
  body: ReadonlyArray<Html<Msg>>;
}>;

export type MultiColumnOptions<Msg> = Readonly<{
  mapMsg: (msg: SchedulerMsg) => Msg;
  headerLinks?: ReadonlyArray<HeaderLink>;
  extraColumns?: ReadonlyArray<ExtraColumn<Msg>>;
  /**
   * When true, the internal breadcrumb is not rendered —
   * the consumer renders its own, e.g. in an app navbar.
   */
  omitBreadcrumb?: boolean;
  /**
   * App-owned columns inserted right AFTER the menu column
   * and before the collection's list/detail columns — e.g.
   * a section sub-menu. Rendered in order; empty when the
   * consumer provides none.
   */
  afterMenu?: ReadonlyArray<ExtraColumn<Msg>>;
}>;

/**
 * The MULTI-COLUMN mode renderer (D10) — a pure
 * projection of ticket 09's scheduled {@link Scene} into
 * panes expanding rightward. The base renderer is kept
 * lightweight; consumers that need app-specific actions or
 * forms can use {@link multiColumnWith} to add header links
 * and arbitrary extra columns without teaching the
 * scheduler those domain concepts.
 */
export const multiColumn = (
  scene: Scene,
): Html<SchedulerMsg, "div"> =>
  multiColumnWith<SchedulerMsg>(scene, {
    mapMsg: (msg: SchedulerMsg) => msg,
  });

export const multiColumnWith = <Msg>(
  scene: Scene,
  options: MultiColumnOptions<Msg>,
): Html<Msg, "div"> =>
  slot(
    [attr("class", `${cssPrefix}-scheduler`)],
    [
      ...(options.omitBreadcrumb === true
        ? []
        : [breadcrumb<Msg>(crumbsOf(scene))]),
      ...confirmOverlay(scene.confirm).map(
        mapScheduler(options),
      ),
      row<Msg>(
        [],
        [
          ...scene.levels
            .slice(0, 1)
            .map((level: Level) =>
              columnFor(level, options),
            ),
          ...(options.afterMenu ?? []).map(
            extraColumn,
          ),
          ...scene.levels
            .slice(1)
            .map((level: Level) =>
              columnFor(level, options),
            ),
          ...(options.extraColumns ?? []).map(
            extraColumn,
          ),
        ],
      ),
    ],
  );

const titleOf = (level: Level): SoftStr =>
  match(level)(
    [
      menuLevel$(),
      ({ content }): SoftStr => content.title,
    ],
    [
      listLevel$(),
      ({ content }): SoftStr => content.title,
    ],
    [
      boardLevel$(),
      ({ content }): SoftStr => content.title,
    ],
    [
      detailLevel$(),
      ({ content }): SoftStr => content.title,
    ],
  );

const backOf = (level: Level): Option<SoftStr> =>
  match(level)(
    [menuLevel$(), (): Option<SoftStr> => none()],
    [
      listLevel$(),
      ({ content }): Option<SoftStr> =>
        content.back,
    ],
    [
      boardLevel$(),
      ({ content }): Option<SoftStr> =>
        content.back,
    ],
    [
      detailLevel$(),
      ({ content }): Option<SoftStr> =>
        content.back,
    ],
  );

/**
 * One crumb per level; each crumb links to the URL that
 * makes ITS level the deepest — obtained as the NEXT
 * level's `back` (which truncates to exactly there).
 */
export const crumbsOf = (
  scene: Scene,
): ReadonlyArray<Crumb> =>
  scene.levels.map(
    (level: Level, i: number): Crumb => ({
      label: titleOf(level),
      to: pipe(
        fromNullable(scene.levels[i + 1]),
        matchOption<Level, Option<SoftStr>>(
          () => none(),
          (next: Level) => backOf(next),
        ),
      ),
    }),
  );

const detailBody = (
  collection: SoftStr,
  detailRow: Option<Row>,
  fields: ReadonlyArray<DetailField>,
  actions: ReadonlyArray<ActionButton>,
): ReadonlyArray<Html<SchedulerMsg>> =>
  matchOption<
    Row,
    ReadonlyArray<Html<SchedulerMsg>>
  >(
    () => [
      span(
        [attr("class", `${cssPrefix}-hint`)],
        [text("Not found")],
      ),
    ],
    (r: Row) => [
      detailFields(fields),
      ...actionRow(
        collection,
        some(r.id),
        actions,
      ),
    ],
  )(detailRow);

const headerLinks = (
  collection: SoftStr,
  options: MultiColumnOptions<unknown>,
): ReadonlyArray<
  Readonly<{
    label: SoftStr;
    href: SoftStr;
    active: boolean;
  }>
> =>
  (options.headerLinks ?? [])
    .filter(
      (link: HeaderLink) =>
        link.collection === collection,
    )
    .map((link: HeaderLink) => ({
      label: link.label,
      href: link.href,
      active: link.active ?? false,
    }));

/**
 * The collection's app-owned header links, rendered as a
 * button row ABOVE the list's query box (not inside the
 * column header) — the consumer's per-list actions (e.g.
 * "Add client") sit over the filter, styled by the app.
 * Empty when the collection declares no links.
 */
const listActions = <Msg>(
  collection: SoftStr,
  options: MultiColumnOptions<Msg>,
): ReadonlyArray<Html<Msg>> => {
  const links = headerLinks(collection, options);
  return links.length === 0
    ? []
    : [
        slot(
          [
            attr(
              "class",
              `${cssPrefix}-list-actions`,
            ),
          ],
          links.map(
            (
              link: Readonly<{
                label: SoftStr;
                href: SoftStr;
                active: boolean;
              }>,
            ) =>
              a(
                [
                  href(link.href),
                  attr(
                    "class",
                    `${cssPrefix}-list-action`,
                  ),
                  ...(link.active
                    ? [
                        attr(
                          "aria-current",
                          "page",
                        ),
                      ]
                    : []),
                ],
                [text(link.label)],
              ),
          ),
        ),
      ];
};

const mapScheduler =
  <Msg>(options: MultiColumnOptions<Msg>) =>
  (node: Html<SchedulerMsg>): Html<Msg> =>
    mapHtml(options.mapMsg)(node);

const columnFor = <Msg>(
  level: Level,
  options: MultiColumnOptions<Msg>,
): Html<Msg> =>
  match(level)(
    [
      menuLevel$(),
      ({ content }): Html<Msg> =>
        column(
          [basis("220px")],
          [
            navPane(
              [],
              [
                colHead<Msg>({
                  title: content.title,
                  close: none(),
                  links: [],
                }),
                slot(
                  [
                    style_(
                      `${cssPrefix}-menu-body`,
                    ),
                  ],
                  [
                    mapScheduler(options)(
                      menuNav(content.entries),
                    ),
                  ],
                ),
              ],
            ),
          ],
        ),
    ],
    [
      listLevel$(),
      ({ content }): Html<Msg> =>
        column(
          [basis("300px")],
          [
            asidePane(
              [],
              [
                slot(
                  [
                    key(
                      `list-${content.collection}`,
                    ),
                    fadeIn(150),
                  ],
                  [
                    colHead<Msg>({
                      title: content.title,
                      close: content.back,
                      links: [],
                    }),
                    ...listActions(
                      content.collection,
                      options,
                    ),
                    ...queryField(
                      content.query,
                    ).map(mapScheduler(options)),
                    ...loadingHint(
                      content.loading,
                    ).map(mapScheduler(options)),
                    ...errorHint(
                      content.error,
                    ).map(mapScheduler(options)),
                    mapScheduler(options)(
                      rowList(content.rows),
                    ),
                    ...actionRow(
                      content.collection,
                      none(),
                      content.actions,
                    ).map(mapScheduler(options)),
                  ],
                ),
              ],
            ),
          ],
        ),
    ],
    [
      boardLevel$(),
      ({ content }): Html<Msg> =>
        column(
          [fluid],
          [
            mainPane(
              [],
              [
                slot(
                  [
                    key(
                      `board-${content.collection}`,
                    ),
                    fadeIn(150),
                  ],
                  [
                    colHead<Msg>({
                      title: content.title,
                      close: content.back,
                      links: [],
                    }),
                    ...loadingHint(
                      content.loading,
                    ).map(mapScheduler(options)),
                    ...errorHint(
                      content.error,
                    ).map(mapScheduler(options)),
                    mapScheduler(options)(
                      tileGrid(content.tiles),
                    ),
                  ],
                ),
              ],
            ),
          ],
        ),
    ],
    [
      detailLevel$(),
      ({ content }): Html<Msg> =>
        column(
          [fluid],
          [
            mainPane(
              [],
              [
                slot(
                  [
                    key(
                      `detail-${getOr("_")(
                        detailKey(content.row),
                      )}`,
                    ),
                    fadeIn(150),
                  ],
                  [
                    colHead<Msg>({
                      title: content.title,
                      close: content.back,
                      links: [],
                    }),
                    ...detailBody(
                      content.collection,
                      content.row,
                      content.fields,
                      content.actions,
                    ).map(mapScheduler(options)),
                  ],
                ),
              ],
            ),
          ],
        ),
    ],
  );

const extraColumn = <Msg>(
  extra: ExtraColumn<Msg>,
): Html<Msg> =>
  column(
    [fluid],
    [
      mainPane(
        [],
        [
          slot(
            [key(extra.key), fadeIn(150)],
            [
              colHead<Msg>({
                title: extra.title,
                close: extra.close,
                links: [],
              }),
              ...extra.body,
            ],
          ),
        ],
      ),
    ],
  );

const detailKey = (
  detailRow: Option<Row>,
): Option<SoftStr> =>
  matchOption<Row, Option<SoftStr>>(
    () => none(),
    (r: Row) => some(r.id),
  )(detailRow);
