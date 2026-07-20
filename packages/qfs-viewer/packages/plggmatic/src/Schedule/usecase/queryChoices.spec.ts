import {
  test,
  check,
  all,
  toBe,
  toEqual,
} from "plgg-test";
import { type SoftStr } from "plgg";
import { renderToString } from "plgg-view";
import { makeUrl } from "plgg-view/client";
import {
  row,
  field,
} from "plggmatic/Declare/model/Row";
import { sync } from "plggmatic/Declare/model/Source";
import {
  query,
  queryChoice,
  matchesChoice,
} from "plggmatic/Declare/model/Query";
import { collection } from "plggmatic/Declare/model/Collection";
import {
  menu,
  menuEntry,
} from "plggmatic/Declare/model/Menu";
import { declare } from "plggmatic/Declare/model/Declaration";
import {
  type Model,
  choiceOf,
} from "plggmatic/Schedule/model/Model";
import {
  type SchedulerMsg,
  openMenu,
  queryInput,
  queryChoiceInput,
} from "plggmatic/Schedule/model/Msg";
import { schedule } from "plggmatic/Schedule/usecase/schedule";
import { multiColumn } from "plggmatic/Render/usecase/multiColumn";

// --- fixture: clients filterable by keyword + a declared
// status choice (the reference search-form shape).

const decl = declare({
  title: "Demo",
  menu: menu([menuEntry("Clients", "clients")]),
  collections: [
    collection<
      Readonly<{
        id: SoftStr;
        name: SoftStr;
        status: SoftStr;
      }>
    >({
      id: "clients",
      title: "Clients",
      toRow: (c) =>
        row(c.id, c.name, [
          field("Status", c.status),
        ]),
      source: sync(() => [
        {
          id: "acme",
          name: "ACME",
          status: "Prime",
        },
        {
          id: "beacon",
          name: "Beacon",
          status: "Active",
        },
        {
          id: "delta",
          name: "Delta",
          status: "Active",
        },
      ]),
      query: query("Filter clients", [
        queryChoice(
          "status",
          "Status",
          "Status",
          ["Prospect", "Active", "Prime"],
        ),
      ]),
    }),
  ],
});

const s = schedule(decl);
const step = (
  msg: SchedulerMsg,
  model: Model,
): Model => s.update(msg, model)[0];
const [m0] = s.init(makeUrl("/app", ""));
const opened = step(openMenu("clients"), m0);

const rowsShown = (model: Model): number => {
  const html = renderToString(
    multiColumn(s.scene(model)),
  );
  return (html.match(/pm-list-item/g) ?? [])
    .length;
};

test("matchesChoice is the closed equality over a row field", () => {
  const r = row("x", "X", [
    field("Status", "Active"),
  ]);
  return all([
    check(
      matchesChoice("", "Status", r),
      toBe(true),
    ),
    check(
      matchesChoice("Active", "Status", r),
      toBe(true),
    ),
    check(
      matchesChoice("Prime", "Status", r),
      toBe(false),
    ),
    check(
      matchesChoice("Active", "Ghost", r),
      toBe(false),
    ),
  ]);
});

test("a chosen choice filters the active list and joins the URL", () => {
  const chosen = step(
    queryChoiceInput("status", "Active"),
    opened,
  );
  return all([
    check(rowsShown(opened), toBe(3)),
    check(rowsShown(chosen), toBe(2)),
    check(
      choiceOf(chosen, "status"),
      toBe("Active"),
    ),
    check(
      s.toUrl(chosen).search,
      toBe("?c=clients&status=Active"),
    ),
  ]);
});

test("keyword and choice combine, and clearing restores", () => {
  const both = step(
    queryInput("bea"),
    step(
      queryChoiceInput("status", "Active"),
      opened,
    ),
  );
  const keywordCleared = step(
    queryInput(""),
    both,
  );
  const allCleared = step(
    queryChoiceInput("status", ""),
    keywordCleared,
  );
  return all([
    check(rowsShown(both), toBe(1)),
    check(rowsShown(keywordCleared), toBe(2)),
    check(rowsShown(allCleared), toBe(3)),
    check(
      choiceOf(allCleared, "status"),
      toBe(""),
    ),
  ]);
});

test("a deep link carries choices and unknown params drop", () => {
  const [m] = s.init(
    makeUrl(
      "/app",
      "?c=clients&status=Prime&junk=zzz",
    ),
  );
  return all([
    check(choiceOf(m, "status"), toBe("Prime")),
    check(choiceOf(m, "junk"), toBe("")),
    check(rowsShown(m), toBe(1)),
    // canonical: the reflected URL keeps only the
    // declared choice
    check(
      s.toUrl(m).search,
      toBe("?c=clients&status=Prime"),
    ),
  ]);
});

test("navigation resets choices; a choice change is a replace", () => {
  const chosen = step(
    queryChoiceInput("status", "Active"),
    opened,
  );
  const renavigated = step(
    openMenu("clients"),
    chosen,
  );
  return all([
    check(renavigated.queryChoices, toEqual([])),
    check(
      s.historyMode(opened, chosen),
      toBe("replace"),
    ),
  ]);
});

test("the choice renders as a labelled select with its options", () => {
  const chosen = step(
    queryChoiceInput("status", "Active"),
    opened,
  );
  const html = renderToString(
    multiColumn(s.scene(chosen)),
  );
  return all([
    check(
      html.includes("pm-query-choice"),
      toBe(true),
    ),
    check(
      html.includes('aria-label="Status"'),
      toBe(true),
    ),
    check(html.includes(">Any<"), toBe(true)),
    check(
      html.includes(
        '<option value="Active" selected>',
      ) ||
        html.includes(
          '<option value="Active" selected="">',
        ),
      toBe(true),
    ),
  ]);
});
