import { type SoftStr } from "plgg";
import { type Menu } from "plggmatic/Declare/model/Menu";
import { type Collection } from "plggmatic/Declare/model/Collection";

/**
 * The root of a plggmatic declaration — everything
 * `schedule(...)` needs to derive a TEA program: a title,
 * the root {@link Menu}, and the {@link Collection}s
 * keyed by id. The flow graph is implicit: the menu names
 * the roots, and each collection's `child` names the next
 * level, so no separate flow type is needed. MODE-
 * AGNOSTIC (D10): nothing here names a column, pane,
 * drawer, or screen — renderers project the derived level
 * stack into a display.
 */
export type Declaration = Readonly<{
  title: SoftStr;
  menu: Menu;
  collections: ReadonlyArray<Collection>;
}>;

/**
 * Constructs a {@link Declaration}. An identity function
 * with a name — it performs nothing (constructing a
 * declaration reads no data and runs no effect); it is
 * the vocabulary's entry point the scheduler consumes.
 */
export const declare = (
  d: Declaration,
): Declaration => d;
