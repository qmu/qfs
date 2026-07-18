import {
  type SoftStr,
  type Box,
  type Option,
  none,
  some,
  fromNullable,
  match,
  matchOption,
  box,
  pattern,
} from "plgg";

/**
 * A field's typed value — the CLOSED union the typed-field
 * decision fixed (mission changelog, 2026-07-12, point 3).
 * A detail cell stops being a bare string: renderers fold
 * this union exhaustively (a new kind is a compile error
 * at every renderer) and WebMCP field schemas derive from
 * the same vocabulary. The members mirror the manifest
 * field types the lowering will map onto: string → `Text`,
 * numeric/money → `Num`, boolean → `Flag`, date/time →
 * `Moment`, relation/domain-id → `Reference`, media →
 * `Media`.
 *
 * `Reference` is the decided cross-link jump realized at
 * the field seam: it names a collection and a row id (a
 * `Binding`, structurally), and the scene resolves it to
 * the target's CANONICAL address — activation is a jump,
 * never a walk.
 */
export type FieldValue =
  | Box<"Text", SoftStr>
  | Box<
      "Num",
      Readonly<{
        value: SoftStr;
        unit: Option<SoftStr>;
      }>
    >
  | Box<"Flag", boolean>
  | Box<"Moment", SoftStr>
  | Box<
      "Reference",
      Readonly<{
        collection: SoftStr;
        id: SoftStr;
        label: SoftStr;
      }>
    >
  | Box<
      "Media",
      Readonly<{ src: SoftStr; alt: SoftStr }>
    >;

/** A plain text value. */
export const textValue = (
  value: SoftStr,
): FieldValue => box("Text")(value);

/** A numeric value (pre-formatted) with an optional unit. */
export const numValue = (
  value: SoftStr,
  unit?: SoftStr,
): FieldValue =>
  box("Num")({
    value,
    unit: fromNullable(unit),
  });

/** A yes/no value. */
export const flagValue = (
  value: boolean,
): FieldValue => box("Flag")(value);

/** A date/time value (ISO string). */
export const momentValue = (
  value: SoftStr,
): FieldValue => box("Moment")(value);

/**
 * A reference to another collection's row — activation
 * jumps to the target's canonical address.
 */
export const refValue = (
  collection: SoftStr,
  id: SoftStr,
  label: SoftStr,
): FieldValue =>
  box("Reference")({ collection, id, label });

/** A media value (image source + alternative text). */
export const mediaValue = (
  src: SoftStr,
  alt: SoftStr,
): FieldValue => box("Media")({ src, alt });

/** Matchers for folding a {@link FieldValue}. */
export const textValue$ = () => pattern("Text")();
export const numValue$ = () => pattern("Num")();
export const flagValue$ = () => pattern("Flag")();
export const momentValue$ = () =>
  pattern("Moment")();
export const refValue$ = () =>
  pattern("Reference")();
export const mediaValue$ = () =>
  pattern("Media")();

/**
 * One labelled cell of a {@link Row}'s detail. `label`
 * may be empty (a body paragraph has a value but no
 * caption) — the emptiness is presentation. The value is
 * a typed {@link FieldValue}; the plain-string
 * {@link field} constructor lowers to `Text`, so the
 * historical string shape keeps working.
 */
export type Field = Readonly<{
  label: SoftStr;
  value: FieldValue;
}>;

/**
 * The presentation-neutral projection of a collection
 * item — the ONLY item shape the scheduled model and the
 * renderer seam ever see (a typed `T` lives only at the
 * {@link Collection} boundary, captured by its `toRow`).
 * `label` is what a list shows; `fields` is what a detail
 * shows; `id` is the item identity used as the URL and
 * selection key. Mirrors the oracle's discipline: the
 * model holds ids, never the domain objects.
 */
export type Row = Readonly<{
  id: SoftStr;
  label: SoftStr;
  fields: ReadonlyArray<Field>;
}>;

/**
 * Constructs a plain-text {@link Field} — the historical
 * string shape, lowered to a `Text` value.
 */
export const field = (
  label: SoftStr,
  value: SoftStr,
): Field => ({ label, value: textValue(value) });

/** Constructs a {@link Field} with a typed value. */
export const fieldOf = (
  label: SoftStr,
  value: FieldValue,
): Field => ({ label, value });

/** Constructs a {@link Row}. */
export const row = (
  id: SoftStr,
  label: SoftStr,
  fields: ReadonlyArray<Field> = [],
): Row => ({ id, label, fields });

/**
 * The reference target of a value, if it is one — the
 * seam the scene uses to resolve a `Reference` into the
 * href of its target's canonical address. `None` for
 * every non-reference kind.
 */
export const refTarget = (
  value: FieldValue,
): Option<
  Readonly<{ collection: SoftStr; id: SoftStr }>
> =>
  match(value)(
    [
      textValue$(),
      (): Option<RefTargetOf> => none(),
    ],
    [
      numValue$(),
      (): Option<RefTargetOf> => none(),
    ],
    [
      flagValue$(),
      (): Option<RefTargetOf> => none(),
    ],
    [
      momentValue$(),
      (): Option<RefTargetOf> => none(),
    ],
    [
      refValue$(),
      ({ content }): Option<RefTargetOf> =>
        some({
          collection: content.collection,
          id: content.id,
        }),
    ],
    [
      mediaValue$(),
      (): Option<RefTargetOf> => none(),
    ],
  );

type RefTargetOf = Readonly<{
  collection: SoftStr;
  id: SoftStr;
}>;

/**
 * A {@link FieldValue}'s display text — the one total
 * string projection shared by renderers and tests
 * (references show their label; media its alt text).
 */
export const fieldText = (
  value: FieldValue,
): SoftStr =>
  match(value)(
    [
      textValue$(),
      ({ content }): SoftStr => content,
    ],
    [
      numValue$(),
      ({ content }): SoftStr =>
        matchOption<SoftStr, SoftStr>(
          () => content.value,
          (unit: SoftStr) =>
            `${content.value} ${unit}`,
        )(content.unit),
    ],
    [
      flagValue$(),
      ({ content }): SoftStr =>
        content ? "✓" : "—",
    ],
    [
      momentValue$(),
      ({ content }): SoftStr => content,
    ],
    [
      refValue$(),
      ({ content }): SoftStr => content.label,
    ],
    [
      mediaValue$(),
      ({ content }): SoftStr => content.alt,
    ],
  );
