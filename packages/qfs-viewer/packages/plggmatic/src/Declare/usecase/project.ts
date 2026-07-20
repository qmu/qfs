import {
  type SoftStr,
  type Box,
  type Icon,
  type Option,
  none,
  getOr,
  fromNullable,
  matchOption,
  match,
  icon,
  box,
  pipe,
  pattern,
} from "plgg";
import {
  type Row,
  type Field,
  type FieldValue,
  fieldOf,
  textValue,
  numValue,
  momentValue,
  flagValue,
  row as makeRow,
} from "plggmatic/Declare/model/Row";

/**
 * A host RECORD — the untyped data an adapter read yields
 * before projection: field keys mapped to their string
 * values (the serialization every backend shares). The
 * keyword projection lowers it to a typed {@link Row}.
 */
export type HostRecord = ReadonlyArray<
  readonly [SoftStr, SoftStr]
>;

/**
 * How a projected field lowers its raw string onto the
 * closed {@link FieldValue} union — the DATA form of the
 * per-field kind (the point-6 keyword-projection idea, the
 * declaration-friendly alternative to a `toRow` function).
 * v1 covers the scalar kinds; `Reference`/`Media` (which
 * need several keys) stay in function-`toRow` territory.
 */
export type Projected =
  | Icon<"AsText">
  | Box<"AsNum", Option<SoftStr>>
  | Icon<"AsMoment">
  | Icon<"AsFlag">;

/** Project a field as plain text. */
export const asText = (): Projected =>
  icon("AsText");
/** Project a field as a number with an optional unit. */
export const asNum = (
  unit?: SoftStr,
): Projected => box("AsNum")(fromNullable(unit));
/** Project a field as a date/time (ISO string). */
export const asMoment = (): Projected =>
  icon("AsMoment");
/** Project a field as a yes/no flag. */
export const asFlag = (): Projected =>
  icon("AsFlag");

const asText$ = () => pattern("AsText")();
const asNum$ = () => pattern("AsNum")();
const asMoment$ = () => pattern("AsMoment")();
const asFlag$ = () => pattern("AsFlag")();

/** The raw strings a flag reads as `true` (else `false`). */
const truthy: ReadonlyArray<SoftStr> = [
  "true",
  "1",
  "yes",
  "✓",
];

/** Lowers a raw string onto a {@link FieldValue} by kind. */
const lower = (
  as: Projected,
  raw: SoftStr,
): FieldValue =>
  match(as)(
    [asText$(), (): FieldValue => textValue(raw)],
    [
      asNum$(),
      ({ content }): FieldValue =>
        matchOption<SoftStr, FieldValue>(
          () => numValue(raw),
          (unit: SoftStr) => numValue(raw, unit),
        )(content),
    ],
    [
      asMoment$(),
      (): FieldValue => momentValue(raw),
    ],
    [
      asFlag$(),
      (): FieldValue =>
        flagValue(
          truthy.includes(raw.toLowerCase()),
        ),
    ],
  );

/**
 * One projected field: the row field `label`, the record
 * `key` it reads, and how it lowers (`as`). A missing key
 * omits the field (totality) — never a crash, never a bare
 * empty cell.
 */
export type FieldProjection = Readonly<{
  label: SoftStr;
  key: SoftStr;
  as: Projected;
}>;

/**
 * A keyword projection: which record keys supply the row's
 * `id` and `label`, and the ordered field projections. The
 * data alternative to a `toRow` function — a manifest/DSL
 * declaration carries this instead of a closure.
 */
export type Projection = Readonly<{
  id: SoftStr;
  label: SoftStr;
  fields: ReadonlyArray<FieldProjection>;
}>;

/** Constructs a {@link FieldProjection}. */
export const projectField = (
  label: SoftStr,
  key: SoftStr,
  as: Projected,
): FieldProjection => ({ label, key, as });

/** Constructs a {@link Projection}. */
export const projection = (p: {
  id: SoftStr;
  label: SoftStr;
  fields?: ReadonlyArray<FieldProjection>;
}): Projection => ({
  id: p.id,
  label: p.label,
  fields: pipe(
    fromNullable(p.fields),
    getOr<ReadonlyArray<FieldProjection>>([]),
  ),
});

/** A record's value for a key, if present. */
const valueOf = (
  record: HostRecord,
  key: SoftStr,
): Option<SoftStr> =>
  matchOption<
    readonly [SoftStr, SoftStr],
    Option<SoftStr>
  >(
    () => none(),
    ([, value]) => fromNullable(value),
  )(
    fromNullable(record.find(([k]) => k === key)),
  );

/**
 * Lowers a keyword {@link Projection} to a `toRow`-shaped
 * function `(record) => Row` — the data alternative to a
 * `toRow` closure, usable as any source's projection. Total:
 * an absent `id`/`label` key reads as `""`, and an absent
 * field key OMITS that field rather than throwing (the
 * missing-field totality the gate requires).
 */
export const projectRow =
  (spec: Projection) =>
  (record: HostRecord): Row =>
    makeRow(
      pipe(
        valueOf(record, spec.id),
        getOr<SoftStr>(""),
      ),
      pipe(
        valueOf(record, spec.label),
        getOr<SoftStr>(""),
      ),
      spec.fields.flatMap((fp: FieldProjection) =>
        matchOption<
          SoftStr,
          ReadonlyArray<Field>
        >(
          () => [],
          (raw: SoftStr) => [
            fieldOf(fp.label, lower(fp.as, raw)),
          ],
        )(valueOf(record, fp.key)),
      ),
    );
