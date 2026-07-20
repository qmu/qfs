import {
  type SoftStr,
  type Option,
  none,
  some,
  fromNullable,
  matchOption,
  match,
  pipe,
} from "plgg";
import { type Collection } from "plggmatic/Declare/model/Collection";
import { type Declaration } from "plggmatic/Declare/model/Declaration";
import {
  type View,
  type Binding,
  menuView,
  listView,
  detailView,
  menuView$,
  listView$,
  detailView$,
} from "plggmatic/Schedule/model/View";
import { type UrlSlice } from "plggmatic/Schedule/usecase/codec";
import { chainCollections } from "plggmatic/Schedule/usecase/chain";

/**
 * The legacy lowering — where the declaration vocabulary
 * meets the (view, typed params) core (mission decision,
 * 2026-07-12). The stored model keeps the legacy slice
 * encoding (`root`/`path`/`query`), and this module is
 * the total bridge both ways: {@link focusedView} derives
 * WHICH view a slice addresses (replacing the
 * "`path.length` is the screen" folklore), and
 * {@link sliceOf} projects a navigation target back into
 * the slice the codec and scheduler already speak. Both
 * are TOTAL: dangling roots, dangling children, and
 * overlong paths degrade to addressable-but-unresolvable
 * views, never a throw — the same standard URLs get.
 */

/**
 * The view a `root`/`path` slice addresses, derived
 * against the declaration's drill chain:
 *
 * - no root — the menu;
 * - fewer selections than the chain — the list the next
 *   selection would come from;
 * - the whole chain selected — the last collection's
 *   detail (childless) or its declared-but-unresolved
 *   child list (dangling `child`);
 * - junk beyond the chain — the deepest addressable list
 *   (parity with the scene's clamped active level).
 */
export const focusedView =
  (declaration: Declaration) =>
  (
    root: Option<SoftStr>,
    path: ReadonlyArray<SoftStr>,
  ): View =>
    matchOption<SoftStr, View>(
      () => menuView(),
      (r: SoftStr) =>
        viewAt(
          chainCollections(declaration, some(r)),
          r,
          path.length,
        ),
    )(root);

const viewAt = (
  chain: ReadonlyArray<Collection>,
  root: SoftStr,
  depth: number,
): View =>
  pipe(
    fromNullable(chain[depth]),
    matchOption<Collection, View>(
      () => beyondChain(chain, root, depth),
      (c: Collection) => listView(c.id),
    ),
  );

/** The view addressed at or past the chain's end. */
const beyondChain = (
  chain: ReadonlyArray<Collection>,
  root: SoftStr,
  depth: number,
): View =>
  pipe(
    fromNullable(chain[chain.length - 1]),
    matchOption<Collection, View>(
      // dangling root: the addressed-but-unresolvable list
      () => listView(root),
      (last: Collection) =>
        depth === chain.length
          ? matchOption<SoftStr, View>(
              () => detailView(last.id),
              (childId: SoftStr) =>
                listView(childId),
            )(last.child)
          : // junk beyond the chain: clamp to the deepest
            // resolvable list, matching the scene
            listView(last.id),
    ),
  );

/**
 * Projects a navigation target — a {@link View} plus its
 * ancestor {@link Binding}s — into the slice encoding the
 * codec serializes and the scheduler stores. Declaration-
 * free and total: the root is the first binding's
 * collection (or the target's own collection when no
 * bindings address it), the path is the bindings' row
 * ids, and a navigation always resets the query. A target
 * whose bindings do not add up degrades to the slice they
 * spell, exactly as a hand-typed URL would.
 */
export const sliceOf = (
  to: View,
  bindings: ReadonlyArray<Binding>,
): UrlSlice =>
  match(to)(
    [
      menuView$(),
      (): UrlSlice => ({
        root: none(),
        path: [],
        query: "",
      }),
    ],
    [
      listView$(),
      ({ content }): UrlSlice =>
        towards(content, bindings),
    ],
    [
      detailView$(),
      ({ content }): UrlSlice =>
        towards(content, bindings),
    ],
  );

const towards = (
  collection: SoftStr,
  bindings: ReadonlyArray<Binding>,
): UrlSlice => ({
  root: some(
    pipe(
      fromNullable(bindings[0]),
      matchOption<Binding, SoftStr>(
        () => collection,
        (b: Binding) => b.collection,
      ),
    ),
  ),
  path: bindings.map((b: Binding) => b.id),
  query: "",
});
