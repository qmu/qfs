import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import { fieldText } from "plggmatic/Declare/model/Row";
import {
  type HostRecord,
  projection,
  projectField,
  projectRow,
  asText,
  asNum,
  asMoment,
  asFlag,
} from "plggmatic/Declare/usecase/project";

const spec = projection({
  id: "id",
  label: "name",
  fields: [
    projectField("Budget", "budget", asNum("M¥")),
    projectField("When", "due", asMoment()),
    projectField("Done", "done", asFlag()),
    projectField("Note", "note", asText()),
  ],
});

const texts = (
  rec: HostRecord,
): ReadonlyArray<string> =>
  projectRow(spec)(rec).fields.map((f) =>
    fieldText(f.value),
  );

test("projectRow lowers each field onto its FieldValue kind", () => {
  const r = projectRow(spec)([
    ["id", "p1"],
    ["name", "Storefront"],
    ["budget", "8.4"],
    ["due", "2026-07-14"],
    ["done", "yes"],
    ["note", "hi"],
  ]);
  return all([
    check(r.id, toBe("p1")),
    check(r.label, toBe("Storefront")),
    check(r.fields.length, toBe(4)),
    check(
      r.fields
        .map((f) => fieldText(f.value))
        .join("|"),
      toBe("8.4 M¥|2026-07-14|✓|hi"),
    ),
  ]);
});

test("a missing field key omits that field (totality)", () => {
  const r = projectRow(spec)([
    ["id", "p2"],
    ["name", "No budget"],
    ["done", "1"],
    ["note", "kept"],
  ]);
  return all([
    // budget and due are absent → omitted, not empty cells
    check(r.fields.length, toBe(2)),
    check(
      r.fields.map((f) => f.label).join(","),
      toBe("Done,Note"),
    ),
  ]);
});

test("a missing id / label key reads as empty (never a crash)", () => {
  const r = projectRow(spec)([["budget", "1"]]);
  return all([
    check(r.id, toBe("")),
    check(r.label, toBe("")),
    check(r.fields.length, toBe(1)),
  ]);
});

test("flag lowers truthy strings to yes and everything else to no", () =>
  all([
    check(
      texts([
        ["id", "x"],
        ["name", "x"],
        ["done", "true"],
      ]).join(""),
      toBe("✓"),
    ),
    check(
      texts([
        ["id", "x"],
        ["name", "x"],
        ["done", "✓"],
      ]).join(""),
      toBe("✓"),
    ),
    check(
      texts([
        ["id", "x"],
        ["name", "x"],
        ["done", "no"],
      ]).join(""),
      toBe("—"),
    ),
  ]));
