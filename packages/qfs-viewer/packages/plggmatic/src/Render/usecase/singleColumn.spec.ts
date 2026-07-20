import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import { type SoftStr, some, none } from "plgg";
import { renderToString } from "plgg-view";
import { makeUrl } from "plgg-view/client";
import { cmdEffect } from "plgg-view/client";
import {
  row,
  field,
} from "plggmatic/Declare/model/Row";
import { sync } from "plggmatic/Declare/model/Source";
import {
  action,
  confirm,
} from "plggmatic/Declare/model/Action";
import { collection } from "plggmatic/Declare/model/Collection";
import {
  menu,
  menuEntry,
} from "plggmatic/Declare/model/Menu";
import { declare } from "plggmatic/Declare/model/Declaration";
import {
  openMenu,
  select,
  requestAction,
  loaded,
} from "plggmatic/Schedule/model/Msg";
import { schedule } from "plggmatic/Schedule/usecase/schedule";
import { type Scene } from "plggmatic/Schedule/model/Scene";
import { singleColumn } from "plggmatic/Render/usecase/singleColumn";

type Sec = Readonly<{
  id: SoftStr;
  label: SoftStr;
}>;
type Nt = Readonly<{
  id: SoftStr;
  sec: SoftStr;
  title: SoftStr;
  body: SoftStr;
}>;
const notes: ReadonlyArray<Nt> = [
  {
    id: "n1",
    sec: "a",
    title: "One",
    body: "the body of one",
  },
];
const decl = declare({
  title: "Demo",
  menu: menu([menuEntry("Sections", "sections")]),
  collections: [
    collection<Sec>({
      id: "sections",
      title: "Sections",
      toRow: (s: Sec) => row(s.id, s.label),
      source: sync(() => [
        { id: "a", label: "Alpha" },
      ]),
      child: "notes",
    }),
    collection<Nt>({
      id: "notes",
      title: "Notes",
      toRow: (n: Nt) =>
        row(n.id, n.title, [field("", n.body)]),
      source: sync((path) =>
        notes.filter(
          (n: Nt) => n.sec === path[0],
        ),
      ),
      actions: [
        action({
          id: "del",
          label: "Delete",
          verb: "delete",
          confirm: confirm("Delete note?", true),
          run: () =>
            cmdEffect(() =>
              Promise.resolve(
                loaded("notes", []),
              ),
            ),
        }),
      ],
    }),
  ],
});
const s = schedule(decl);
const [m0] = s.init(makeUrl("/", ""));
const at = (
  ...msgs: ReadonlyArray<
    ReturnType<typeof openMenu>
  >
) =>
  msgs.reduce(
    (m, msg) => s.update(msg, m)[0],
    m0,
  );

test("the menu screen is a navigation landmark", () => {
  const html = renderToString(
    singleColumn(s.scene(m0)),
  );
  return all([
    check(html.includes("<nav"), toBe(true)),
    check(html.includes("<main"), toBe(false)),
    check(html.includes("Sections"), toBe(true)),
  ]);
});

test("a list screen is a single main with a back control", () => {
  const html = renderToString(
    singleColumn(
      s.scene(
        at(openMenu("sections"), select(0, "a")),
      ),
    ),
  );
  return all([
    check(html.includes("<main"), toBe(true)),
    // the deepest screen is the notes list, not the
    // sections column (single operation per screen)
    check(html.includes("One"), toBe(true)),
    check(
      html.includes('aria-label="Back"'),
      toBe(true),
    ),
  ]);
});

test("a detail screen shows the item body and its action", () => {
  const html = renderToString(
    singleColumn(
      s.scene(
        at(
          openMenu("sections"),
          select(0, "a"),
          select(1, "n1"),
        ),
      ),
    ),
  );
  return all([
    check(
      html.includes("the body of one"),
      toBe(true),
    ),
    check(html.includes("Delete"), toBe(true)),
    check(
      html.includes('aria-label="Back"'),
      toBe(true),
    ),
  ]);
});

test("a parked confirmation renders the overlay in single-column too", () => {
  const html = renderToString(
    singleColumn(
      s.scene(
        at(
          openMenu("sections"),
          select(0, "a"),
          select(1, "n1"),
          requestAction(
            "notes",
            "del",
            some("n1"),
          ),
        ),
      ),
    ),
  );
  return check(
    html.includes('role="dialog"'),
    toBe(true),
  );
});

test("a structurally empty scene degrades to a hint, no crash", () => {
  const empty: Scene = {
    title: "x",
    levels: [],
    confirm: none(),
  };
  return check(
    renderToString(singleColumn(empty)).includes(
      "Nothing to show",
    ),
    toBe(true),
  );
});
