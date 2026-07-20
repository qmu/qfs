import { type SoftStr, type Option } from "plgg";

/**
 * One node of a {@link navTree}. `href` is an `Option`
 * (not a nullable string): a `Some` renders a link, a
 * `None` renders a plain section-header label — so a
 * group heading and a leaf link are one recursive type,
 * and "a heading with no link" is expressed in the type,
 * not by an empty string. `children` recurses; a leaf
 * has `[]`.
 */
export type NavItem = Readonly<{
  label: SoftStr;
  href: Option<SoftStr>;
  children: ReadonlyArray<NavItem>;
}>;
