import { type SoftStr } from "plgg";

/**
 * The landmark role a pane plays in the page. A closed
 * union (not a free string), mapped to a semantic HTML
 * element by an exhaustive `Record` — so every pane
 * renders as a real landmark and a new role cannot be
 * added without giving it an element. This is the seed
 * set the known consumers need (a `navigation` sidebar,
 * a `main` content column, a `complementary` rail/list);
 * more roles arrive one at a time with a consumer.
 */
export type PaneRole =
  "navigation" | "main" | "complementary";

/**
 * The semantic element each {@link PaneRole} renders as.
 * Exhaustive: a role missing here is a `tsc` error, so
 * the accessibility skeleton (nav/main/aside landmarks)
 * can never silently degrade to a bare `div`.
 */
const LANDMARK: Record<PaneRole, SoftStr> = {
  navigation: "nav",
  main: "main",
  complementary: "aside",
};

/** The landmark tag for a role. */
export const landmarkTag = (
  role: PaneRole,
): SoftStr => LANDMARK[role];
