import {
  type SoftStr,
  type Option,
  fromNullable,
  matchOption,
} from "plgg";

/**
 * The manifest-SHAPED schema the flow reader checks a flow
 * against. A `Collection`'s `toRow` is a function that
 * carries no static field types, so field types come from
 * a schema the manifest supplies (point-3 decision) rather
 * than from the opaque TypeScript `Declaration`. The bridge
 * from a real `Declaration` (collection + declared-choice
 * names, plus manifest field types) lands with the host-
 * adapter ticket; a spec fixture builds a {@link FlowSchema}
 * directly.
 *
 * A field's static type is v1-simple: numeric or not (the
 * only distinction the flow type-checker needs — `sum`
 * over numbers vs strings). Richer field types arrive with
 * the manifest lowering; the plgg-ir-language `SemType`
 * vocabulary is reserved for that, not spent on a boolean.
 */

/** One collection's static schema. */
export type CollectionSchema = Readonly<{
  id: SoftStr;
  fields: ReadonlyArray<
    Readonly<{ kw: SoftStr; numeric: boolean }>
  >;
  choices: ReadonlyArray<SoftStr>;
}>;

/** The schema a flow is checked against. */
export type FlowSchema = Readonly<{
  collections: ReadonlyArray<CollectionSchema>;
}>;

/** Constructs a {@link FlowSchema}. */
export const flowSchema = (
  collections: ReadonlyArray<CollectionSchema>,
): FlowSchema => ({ collections });

/** Finds a collection's schema by id. */
export const collectionSchema = (
  schema: FlowSchema,
  id: SoftStr,
): Option<CollectionSchema> =>
  fromNullable(
    schema.collections.find((c) => c.id === id),
  );

/**
 * Whether a keyword projects to a number. `label`/`id`
 * are the row's own string identity; an undeclared field
 * defaults to string (the conservative `fieldText`
 * projection).
 */
export const fieldIsNumeric = (
  cs: CollectionSchema,
  kw: SoftStr,
): boolean =>
  kw === "label" || kw === "id"
    ? false
    : matchOption<
        Readonly<{
          kw: SoftStr;
          numeric: boolean;
        }>,
        boolean
      >(
        () => false,
        (f) => f.numeric,
      )(
        fromNullable(
          cs.fields.find((f) => f.kw === kw),
        ),
      );

/** True if the collection declares the choice id. */
export const hasChoice = (
  cs: CollectionSchema,
  choiceId: SoftStr,
): boolean =>
  cs.choices.some((c) => c === choiceId);
