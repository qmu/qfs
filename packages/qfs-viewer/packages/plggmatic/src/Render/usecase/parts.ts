import {
  type SoftStr,
  type Option,
  some,
  none,
  matchOption,
  match,
} from "plgg";
import {
  type Html,
  type Phrasing,
  slot,
  span,
  input,
  button,
  select,
  option,
  a,
  img,
  text,
  attr,
  href,
  onClick,
  onInput,
  onChange,
  ul as ulEl,
  li as liElement,
} from "plgg-view";
import { style_ } from "plggmatic/styleEntry";
import { type NavItem } from "plggmatic/Component/model/navItem";
import { navTree } from "plggmatic/Component/usecase/navTree";
import { focusRing } from "plggmatic/Component/model/interaction";
import { cssPrefix } from "plggmatic/Meta/model/identity";
import { confirmDialog } from "plggmatic/Component/usecase/confirmDialog";
import {
  type SchedulerMsg,
  queryInput,
  queryChoiceInput,
  requestAction,
  confirmAction,
  cancelAction,
} from "plggmatic/Schedule/model/Msg";
import {
  type ConfirmPrompt,
  type ActionButton,
  type QueryState,
  type QueryChoiceState,
  type RowLink,
  type MenuLink,
  type Tile,
  type DetailField,
} from "plggmatic/Schedule/model/Scene";
import {
  type FieldValue,
  fieldText,
  textValue$,
  numValue$,
  flagValue$,
  momentValue$,
  refValue$,
  mediaValue$,
} from "plggmatic/Declare/model/Row";

/**
 * The rendering pieces BOTH mode renderers (multi-column,
 * single-column) share — so the two projections of the
 * same {@link Scene} can never drift (mode parity is the
 * proof obligation of ticket 11): the confirmation
 * overlay, the action buttons, the query box, the list
 * rows, the menu nav, and the detail fields. Navigation
 * is links (the runtime turns an in-app `<a>` click into
 * `onUrlChange`); the query, actions, and confirmation
 * dispatch scheduler `Msg`s.
 */

/**
 * The confirmation, if parked — rendered by ticket 12's
 * framework `confirmDialog` (a real `role="dialog"` /
 * `aria-modal` modal with a backdrop), so both renderers
 * get the accessible dialog. Backdrop-click and Cancel
 * dispatch `cancelAction`, Confirm dispatches
 * `confirmAction`.
 */
export const confirmOverlay = (
  confirm: Option<ConfirmPrompt>,
): ReadonlyArray<Html<SchedulerMsg>> =>
  matchOption<
    ConfirmPrompt,
    ReadonlyArray<Html<SchedulerMsg>>
  >(
    () => [],
    (c: ConfirmPrompt) => [
      confirmDialog<SchedulerMsg>({
        title: c.destructive
          ? "Confirm deletion"
          : "Please confirm",
        body: c.prompt,
        confirmLabel: "Confirm",
        cancelLabel: "Cancel",
        destructive: c.destructive,
        onConfirm: confirmAction(),
        onCancel: cancelAction(),
      }),
    ],
  )(confirm);

/** The action buttons for a collection (empty when none). */
export const actionRow = (
  collection: SoftStr,
  target: Option<SoftStr>,
  actions: ReadonlyArray<ActionButton>,
): ReadonlyArray<Html<SchedulerMsg>> =>
  actions.length === 0
    ? []
    : [
        slot(
          [attr("class", `${cssPrefix}-actions`)],
          actions.map((ab: ActionButton) =>
            button(
              [
                style_(
                  `${cssPrefix}-btn`,
                  focusRing,
                ),
                onClick(
                  requestAction(
                    collection,
                    ab.id,
                    target,
                  ),
                ),
              ],
              [text(ab.label)],
            ),
          ),
        ),
      ];

/**
 * One declared query choice as a controlled dropdown —
 * the empty option ("Any") clears the filter; a change
 * dispatches `queryChoiceInput`.
 */
const choiceSelect = (
  c: QueryChoiceState,
): Html<SchedulerMsg> =>
  select(
    [
      attr("class", `${cssPrefix}-query-choice`),
      attr("aria-label", c.label),
      onChange((v: SoftStr) =>
        queryChoiceInput(c.id, v),
      ),
    ],
    [
      option(
        [
          attr("value", ""),
          ...(c.value === ""
            ? [attr("selected", "")]
            : []),
        ],
        [text("Any")],
      ),
      ...c.options.map((o: SoftStr) =>
        option(
          [
            attr("value", o),
            ...(o === c.value
              ? [attr("selected", "")]
              : []),
          ],
          [text(o)],
        ),
      ),
    ],
  );

/** The query controls (empty when the list has no query). */
export const queryField = (
  q: Option<QueryState>,
): ReadonlyArray<Html<SchedulerMsg>> =>
  matchOption<
    QueryState,
    ReadonlyArray<Html<SchedulerMsg>>
  >(
    () => [],
    (state: QueryState) => [
      input(
        [
          attr("type", "search"),
          attr("class", `${cssPrefix}-query`),
          attr("value", state.text),
          attr("placeholder", state.placeholder),
          onInput((v: SoftStr) => queryInput(v)),
        ],
        [],
      ),
      ...state.choices.map(choiceSelect),
    ],
  )(q);

/** One list row as a drilling link (`aria-current` when active). */
export const rowItem = (
  r: RowLink,
): Html<SchedulerMsg, "li"> =>
  liElement(
    [attr("class", `${cssPrefix}-list-item`)],
    [
      a(
        [
          href(r.href),
          ...(r.active
            ? [attr("aria-current", "page")]
            : []),
          style_(
            `${cssPrefix}-row-link`,
            focusRing,
          ),
        ],
        [text(r.row.label)],
      ),
    ],
  );

/** A row list. */
export const rowList = (
  rows: ReadonlyArray<RowLink>,
): Html<SchedulerMsg, "ul"> =>
  ulEl(
    [style_(`${cssPrefix}-list`)],
    rows.map(rowItem),
  );

/** A tile's body: the label headline over the caption. */
const tileBody = (
  t: Tile,
): ReadonlyArray<Phrasing<SchedulerMsg>> => [
  span(
    [attr("class", `${cssPrefix}-tile-label`)],
    [text(t.label)],
  ),
  span(
    [attr("class", `${cssPrefix}-tile-caption`)],
    [text(t.caption)],
  ),
];

/**
 * One board tile. With a jump the whole tile is a link
 * (the tile's ONLY interaction); without one it is inert
 * — a board never drills, so no row-selection dispatch
 * exists here.
 */
const tileItem = (
  t: Tile,
): Html<SchedulerMsg, "li"> =>
  liElement(
    [attr("class", `${cssPrefix}-tile`)],
    [
      matchOption<
        SoftStr,
        Phrasing<SchedulerMsg>
      >(
        () =>
          span(
            [
              attr(
                "class",
                `${cssPrefix}-tile-body`,
              ),
            ],
            tileBody(t),
          ),
        (to: SoftStr) =>
          a(
            [
              href(to),
              style_(
                `${cssPrefix}-tile-link`,
                focusRing,
              ),
            ],
            tileBody(t),
          ),
      )(t.href),
    ],
  );

/**
 * A board's tile grid — the shared `BoardLevel` fold of
 * BOTH mode renderers (the parity obligation), as the
 * `rowList`/`detailFields` parts are for the other level
 * kinds.
 */
export const tileGrid = (
  tiles: ReadonlyArray<Tile>,
): Html<SchedulerMsg, "ul"> =>
  ulEl(
    [style_(`${cssPrefix}-board`)],
    tiles.map(tileItem),
  );

/** The menu entries as a `navTree`, active one marked. */
export const menuNav = (
  entries: ReadonlyArray<MenuLink>,
): Html<SchedulerMsg> =>
  navTree(
    entries.map((e: MenuLink): NavItem => ({
      label: e.label,
      href: some(e.href),
      children: [],
    })),
    matchOption<MenuLink, SoftStr>(
      () => "",
      (e: MenuLink) => e.href,
    )(activeEntry(entries)),
  );

const activeEntry = (
  entries: ReadonlyArray<MenuLink>,
): Option<MenuLink> => {
  const hit = entries.find(
    (e: MenuLink) => e.active,
  );
  return hit === undefined ? none() : some(hit);
};

/** A loading hint line (empty when not loading). */
export const loadingHint = (
  loading: boolean,
): ReadonlyArray<Html<SchedulerMsg>> =>
  loading
    ? [
        span(
          [attr("class", `${cssPrefix}-hint`)],
          [text("Loading…")],
        ),
      ]
    : [];

/** A failure hint line (empty when no error). */
export const errorHint = (
  error: Option<SoftStr>,
): ReadonlyArray<Html<SchedulerMsg>> =>
  matchOption<
    SoftStr,
    ReadonlyArray<Html<SchedulerMsg>>
  >(
    () => [],
    (e: SoftStr) => [
      span(
        [attr("class", `${cssPrefix}-error`)],
        [text(`Failed: ${e}`)],
      ),
    ],
  )(error);

/**
 * One field value rendered by its kind — the exhaustive
 * fold over {@link FieldValue} (a new kind is a compile
 * error HERE, for both mode renderers at once). `Text`
 * keeps its historical bare-text markup; the typed kinds
 * carry a `-field-<kind>` class hook; a `Reference`
 * renders as a link to the target's canonical address
 * (the href the scene resolved) — the cross-link jump.
 */
const valueNode = (
  value: FieldValue,
  refHref: Option<SoftStr>,
): Phrasing<SchedulerMsg> =>
  match(value)(
    [
      textValue$(),
      ({ content }): Phrasing<SchedulerMsg> =>
        text(content),
    ],
    [
      numValue$(),
      (): Phrasing<SchedulerMsg> =>
        span(
          [
            attr(
              "class",
              `${cssPrefix}-field-num`,
            ),
          ],
          [text(fieldText(value))],
        ),
    ],
    [
      flagValue$(),
      (): Phrasing<SchedulerMsg> =>
        span(
          [
            attr(
              "class",
              `${cssPrefix}-field-flag`,
            ),
          ],
          [text(fieldText(value))],
        ),
    ],
    [
      momentValue$(),
      (): Phrasing<SchedulerMsg> =>
        span(
          [
            attr(
              "class",
              `${cssPrefix}-field-moment`,
            ),
          ],
          [text(fieldText(value))],
        ),
    ],
    [
      refValue$(),
      ({ content }): Phrasing<SchedulerMsg> =>
        matchOption<
          SoftStr,
          Phrasing<SchedulerMsg>
        >(
          // total fallback: an unresolved reference
          // renders its label as plain text
          () => text(content.label),
          (to: SoftStr) =>
            a(
              [
                href(to),
                style_(
                  `${cssPrefix}-field-ref`,
                  focusRing,
                ),
              ],
              [text(content.label)],
            ),
        )(refHref),
    ],
    [
      mediaValue$(),
      ({ content }): Phrasing<SchedulerMsg> =>
        img(
          [
            attr("src", content.src),
            attr("alt", content.alt),
            attr(
              "class",
              `${cssPrefix}-field-media`,
            ),
          ],
          [],
        ),
    ],
  );

/**
 * The detail fields block. A field's `label` is rendered
 * as a caption above its value when non-empty; an empty
 * label (a body paragraph) renders the value alone — the
 * emptiness is presentation. Values render by their
 * {@link FieldValue} kind via {@link valueNode}.
 */
export const detailFields = (
  fields: ReadonlyArray<DetailField>,
): Html<SchedulerMsg, "div"> =>
  slot(
    [attr("class", `${cssPrefix}-fields`)],
    fields.map((f: DetailField) =>
      slot(
        [attr("class", `${cssPrefix}-field`)],
        f.label === ""
          ? [valueNode(f.value, f.href)]
          : [
              span(
                [
                  attr(
                    "class",
                    `${cssPrefix}-field-label`,
                  ),
                ],
                [text(f.label)],
              ),
              span(
                [
                  attr(
                    "class",
                    `${cssPrefix}-field-value`,
                  ),
                ],
                [valueNode(f.value, f.href)],
              ),
            ],
      ),
    ),
  );

/** A labelled back affordance (a truncating link), if any. */
export const backControl = (
  back: Option<SoftStr>,
): ReadonlyArray<Html<SchedulerMsg>> =>
  matchOption<
    SoftStr,
    ReadonlyArray<Html<SchedulerMsg>>
  >(
    () => [],
    (to: SoftStr) => [
      a(
        [
          href(to),
          attr("aria-label", "Back"),
          style_(`${cssPrefix}-back`, focusRing),
        ],
        [text("← Back")],
      ),
    ],
  )(back);
