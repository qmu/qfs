import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import {
  type SoftStr,
  type Datum,
  type Result,
  type InvalidError,
  ok,
  err,
  invalidError,
  isOk,
  isErr,
  isSome,
  isNone,
  matchResult,
} from "plgg";
import {
  type FormErrors,
  type Payload,
  parseForm,
  errorFor,
} from "plggmatic/Form/usecase/parseForm";

// a caster: a non-empty string, else an InvalidError
const asFilled = (
  v: unknown,
): Result<Datum, InvalidError> =>
  typeof v === "string" && v.length > 0
    ? ok(v)
    : err(invalidError({ message: "Required" }));

const specs = [
  { name: "title", cast: asFilled },
  { name: "body", cast: asFilled },
];

test("all-valid parses to the typed payload in one pass", () => {
  const drafts = (n: SoftStr): SoftStr =>
    n === "title" ? "Hello" : "World";
  const result = parseForm(specs, drafts);
  return all([
    check(isOk(result), toBe(true)),
    check(
      matchResult<Payload, FormErrors, SoftStr>(
        () => "err",
        (p: Payload) =>
          `${p["title"]}/${p["body"]}`,
      )(result),
      toBe("Hello/World"),
    ),
  ]);
});

test("any-invalid collects per-field errors, no payload", () => {
  const drafts = (n: SoftStr): SoftStr =>
    n === "title" ? "Hello" : "";
  const result = parseForm(specs, drafts);
  return all([
    check(isErr(result), toBe(true)),
    check(
      matchResult<Payload, FormErrors, number>(
        (e: FormErrors) => e.length,
        () => -1,
      )(result),
      toBe(1),
    ),
  ]);
});

test("errorFor finds a field's message and absent ones are None", () => {
  const errors: FormErrors = [
    ["body", "Required"],
  ];
  return all([
    check(
      isSome(errorFor(errors, "body")),
      toBe(true),
    ),
    check(
      isNone(errorFor(errors, "title")),
      toBe(true),
    ),
  ]);
});
