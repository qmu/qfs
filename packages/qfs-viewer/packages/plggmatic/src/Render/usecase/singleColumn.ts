import {
  type SoftStr,
  type Option,
  none,
  some,
  matchOption,
  match,
} from "plgg";
import {
  type Html,
  slot,
  span,
  text,
  attr,
} from "plgg-view";
import {
  column,
  navPane,
  mainPane,
} from "plggmatic/Layout/usecase/combinators";
import { heading } from "plggmatic/Component/usecase/typography";
import { type SchedulerMsg } from "plggmatic/Schedule/model/Msg";
import {
  type Scene,
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
  type Screen,
  currentScreen,
} from "plggmatic/Render/model/screen";
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
  backControl,
} from "plggmatic/Render/usecase/parts";

/**
 * The SINGLE-COLUMN mode renderer (D10) — one operation
 * per screen. A pure projection of the SAME scheduled
 * {@link Scene} the multi-column renderer draws: the
 * current screen is derived (the deepest level), the menu
 * screen is a `navigation` landmark and every other screen
 * a single `main`, and each non-root screen shows a
 * labelled back affordance (a truncating link — the
 * runtime turns the click into the scheduler's navigation,
 * and browser Back pops one screen because traversal
 * pushes history). Shares the interactive pieces with the
 * multi-column renderer through {@link parts}, so the two
 * modes stay in parity. Holds no state, touches no
 * `window`.
 */
export const singleColumn = (
  scene: Scene,
): Html<SchedulerMsg, "div"> =>
  slot(
    [attr("class", `${cssPrefix}-single`)],
    [
      ...confirmOverlay(scene.confirm),
      matchOption<Screen, Html<SchedulerMsg>>(
        () =>
          column(
            [],
            [
              span(
                [
                  attr(
                    "class",
                    `${cssPrefix}-hint`,
                  ),
                ],
                [text("Nothing to show")],
              ),
            ],
          ),
        (screen: Screen) => screenView(screen),
      )(currentScreen(scene)),
    ],
  );

const screenView = (
  screen: Screen,
): Html<SchedulerMsg> =>
  match(screen)(
    [
      menuLevel$(),
      ({ content }): Html<SchedulerMsg> =>
        column(
          [],
          [
            navPane(
              [],
              [
                screenTitle(content.title),
                menuNav(content.entries),
              ],
            ),
          ],
        ),
    ],
    [
      listLevel$(),
      ({ content }): Html<SchedulerMsg> =>
        column(
          [],
          [
            mainPane(
              [],
              [
                ...backControl(content.back),
                screenTitle(content.title),
                ...queryField(content.query),
                ...loadingHint(content.loading),
                ...errorHint(content.error),
                rowList(content.rows),
                ...actionRow(
                  content.collection,
                  none(),
                  content.actions,
                ),
              ],
            ),
          ],
        ),
    ],
    [
      boardLevel$(),
      ({ content }): Html<SchedulerMsg> =>
        column(
          [],
          [
            mainPane(
              [],
              [
                ...backControl(content.back),
                screenTitle(content.title),
                ...loadingHint(content.loading),
                ...errorHint(content.error),
                tileGrid(content.tiles),
              ],
            ),
          ],
        ),
    ],
    [
      detailLevel$(),
      ({ content }): Html<SchedulerMsg> =>
        column(
          [],
          [
            mainPane(
              [],
              [
                ...backControl(content.back),
                screenTitle(content.title),
                ...detailBody(
                  content.collection,
                  content.row,
                  content.fields,
                  content.actions,
                ),
              ],
            ),
          ],
        ),
    ],
  );

const screenTitle = (
  title: SoftStr,
): Html<SchedulerMsg> => heading(2, title);

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
