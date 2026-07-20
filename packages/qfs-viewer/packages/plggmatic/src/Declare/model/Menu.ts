import { type SoftStr } from "plgg";

/**
 * One labelled entry of the root {@link Menu}: a label
 * and the id of the collection it opens (the root of a
 * flow). A menu is navigation INTO the flow, so an entry
 * references a collection by id, not a view — the
 * scheduler resolves the collection.
 */
export type MenuEntry = Readonly<{
  label: SoftStr;
  collection: SoftStr;
}>;

/** The root navigation entries of a declaration. */
export type Menu = Readonly<{
  entries: ReadonlyArray<MenuEntry>;
}>;

/** Constructs a {@link MenuEntry}. */
export const menuEntry = (
  label: SoftStr,
  collection: SoftStr,
): MenuEntry => ({ label, collection });

/** Constructs a {@link Menu}. */
export const menu = (
  entries: ReadonlyArray<MenuEntry>,
): Menu => ({ entries });
