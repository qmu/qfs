import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import {
  isNone,
  isSome,
  matchOption,
} from "plgg";
import {
  type FieldValue,
  field,
  fieldOf,
  row,
  textValue,
  numValue,
  flagValue,
  momentValue,
  refValue,
  mediaValue,
  fieldText,
  refTarget,
} from "plggmatic/Declare/model/Row";

test("field lowers the historical string shape to a Text value", () => {
  const f = field("Status", "Active");
  return all([
    check(f.label, toBe("Status")),
    check(fieldText(f.value), toBe("Active")),
    check(isNone(refTarget(f.value)), toBe(true)),
  ]);
});

test("fieldText projects every kind to its display text", () =>
  all([
    check(fieldText(textValue("x")), toBe("x")),
    check(
      fieldText(numValue("8.4")),
      toBe("8.4"),
    ),
    check(
      fieldText(numValue("8.4", "M¥")),
      toBe("8.4 M¥"),
    ),
    check(fieldText(flagValue(true)), toBe("✓")),
    check(fieldText(flagValue(false)), toBe("—")),
    check(
      fieldText(momentValue("2026-07-12")),
      toBe("2026-07-12"),
    ),
    check(
      fieldText(
        refValue("clients", "acme", "ACME"),
      ),
      toBe("ACME"),
    ),
    check(
      fieldText(mediaValue("/x.png", "logo")),
      toBe("logo"),
    ),
  ]));

test("refTarget yields the binding for a reference and None otherwise", () => {
  const ref: FieldValue = refValue(
    "clients",
    "acme",
    "ACME",
  );
  const target = refTarget(ref);
  return all([
    check(isSome(target), toBe(true)),
    check(
      matchOption<
        Readonly<{
          collection: string;
          id: string;
        }>,
        string
      >(
        () => "",
        (t) => `${t.collection}/${t.id}`,
      )(target),
      toBe("clients/acme"),
    ),
    check(
      isNone(refTarget(momentValue("2026"))),
      toBe(true),
    ),
    check(
      isNone(
        refTarget(mediaValue("/x.png", "x")),
      ),
      toBe(true),
    ),
    check(
      isNone(refTarget(flagValue(true))),
      toBe(true),
    ),
    check(
      isNone(refTarget(numValue("1"))),
      toBe(true),
    ),
  ]);
});

test("fieldOf carries a typed value and row defaults its fields", () => {
  const f = fieldOf(
    "Budget",
    numValue("8.4", "M¥"),
  );
  const r = row("p1", "Storefront");
  return all([
    check(f.label, toBe("Budget")),
    check(fieldText(f.value), toBe("8.4 M¥")),
    check(r.fields.length, toBe(0)),
  ]);
});
