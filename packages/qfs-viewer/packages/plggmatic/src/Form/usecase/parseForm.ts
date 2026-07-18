import {
  type SoftStr,
  type Datum,
  type Result,
  type Option,
  type InvalidError,
  ok,
  err,
  some,
  none,
  fromNullable,
  matchOption,
  matchResult,
  pipe,
  plggErrorMessage,
} from "plgg";

/**
 * One field's parse contract: its name and a caster that
 * turns the raw draft into a typed value or an
 * `InvalidError`. The caster IS the validation — "parse,
 * don't validate" (`asStr`'s `Result<A, InvalidError>`
 * shape); there is no second validate-then-convert layer.
 */
export type FieldSpec = Readonly<{
  name: SoftStr;
  cast: (
    value: unknown,
  ) => Result<Datum, InvalidError>;
}>;

/** Per-field parse errors — `[name, message]` pairs. */
export type FormErrors = ReadonlyArray<
  readonly [SoftStr, SoftStr]
>;

/** The parsed payload — field name → typed value. */
export type Payload = Readonly<
  Record<string, Datum>
>;

type Acc = Readonly<{
  payload: Record<string, Datum>;
  errors: ReadonlyArray<
    readonly [SoftStr, SoftStr]
  >;
}>;

/**
 * Parses every field's draft through its caster in ONE
 * pass, collecting ALL failures (not fail-fast, unlike a
 * `cast` pipeline — a form shows every field's error at
 * once). `Ok` with the typed payload when all fields
 * parse; `Err` with per-field `[name, message]` errors
 * when any fails. Total and DOM-free.
 */
export const parseForm = (
  specs: ReadonlyArray<FieldSpec>,
  draftOf: (name: SoftStr) => SoftStr,
): Result<Payload, FormErrors> => {
  const acc = specs.reduce<Acc>(
    (a: Acc, spec: FieldSpec) =>
      matchResult<Datum, InvalidError, Acc>(
        (e: InvalidError) => ({
          payload: a.payload,
          errors: [
            ...a.errors,
            [spec.name, plggErrorMessage(e)],
          ],
        }),
        (v: Datum) => ({
          payload: {
            ...a.payload,
            [spec.name]: v,
          },
          errors: a.errors,
        }),
      )(spec.cast(draftOf(spec.name))),
    { payload: {}, errors: [] },
  );
  return acc.errors.length === 0
    ? ok(acc.payload)
    : err(acc.errors);
};

/** The error message for a field name, if any. */
export const errorFor = (
  errors: FormErrors,
  name: SoftStr,
): Option<SoftStr> =>
  pipe(
    fromNullable(
      errors.find(([n]) => n === name),
    ),
    matchOption<
      readonly [SoftStr, SoftStr],
      Option<SoftStr>
    >(
      () => none(),
      ([, msg]: readonly [SoftStr, SoftStr]) =>
        some(msg),
    ),
  );
