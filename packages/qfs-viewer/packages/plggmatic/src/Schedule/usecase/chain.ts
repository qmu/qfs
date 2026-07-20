import {
  type SoftStr,
  type Option,
  matchOption,
} from "plgg";
import {
  type Collection,
  collectionById,
} from "plggmatic/Declare/model/Collection";
import { type Declaration } from "plggmatic/Declare/model/Declaration";

/**
 * The ordered chain of collections a root opens, by
 * following each collection's `child`: `[root, child,
 * grandchild, …]`. TOTAL — a `None` root or a dangling
 * child id ends the chain rather than throwing, so an
 * unknown reference degrades to a shorter flow, not a
 * crash. The scheduler and the scene share this to map a
 * flow depth (a path index) to its collection.
 */
export const chainCollections = (
  declaration: Declaration,
  root: Option<SoftStr>,
): ReadonlyArray<Collection> =>
  matchOption<SoftStr, ReadonlyArray<Collection>>(
    () => [],
    (id: SoftStr) => followChain(declaration, id),
  )(root);

const followChain = (
  declaration: Declaration,
  id: SoftStr,
): ReadonlyArray<Collection> =>
  matchOption<
    Collection,
    ReadonlyArray<Collection>
  >(
    () => [],
    (c: Collection) => [
      c,
      ...matchOption<
        SoftStr,
        ReadonlyArray<Collection>
      >(
        () => [],
        (childId: SoftStr) =>
          followChain(declaration, childId),
      )(c.child),
    ],
  )(collectionById(declaration.collections, id));

/** The first `n` selection ids — a collection's ancestor context. */
export const ancestorPath = (
  path: ReadonlyArray<SoftStr>,
  n: number,
): ReadonlyArray<SoftStr> => path.slice(0, n);
