/**
 * The plggmatic Layout module: the column-oriented
 * pattern as COMPOSITION. A screen is an ordered
 * {@link row} of {@link column}s, each holding
 * {@link pane} landmarks — small builders that take
 * `(parts, children)` like element builders take
 * `(attributes, children)`. There is no layout config
 * object and no interpreter: sizing (`basis`/`fluid`),
 * scroll, and responsiveness are style parts and the
 * consumer's own stylesheet targeting the `pm-row`/
 * `pm-col`/`pm-pane` hooks. An explicit named barrel
 * (house style); it grows one recorded rule at a time.
 */
export {
  type PaneRole,
  landmarkTag,
} from "plggmatic/Layout/model/pane";
export {
  type Parts,
  row,
  column,
  pane,
  navPane,
  mainPane,
  asidePane,
} from "plggmatic/Layout/usecase/combinators";
