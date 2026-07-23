import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import { type SoftStr, none, some } from "plgg";
import {
  renderToString,
  slot,
  text,
} from "plgg-view";
import { makeUrl } from "plgg-view/client";
import {
  row,
  field,
  fieldOf,
  refValue,
  numValue,
  flagValue,
  momentValue,
  mediaValue,
} from "plggmatic/Declare/model/Row";
import { sync } from "plggmatic/Declare/model/Source";
import { query } from "plggmatic/Declare/model/Query";
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
  queryInput,
  requestAction,
} from "plggmatic/Schedule/model/Msg";
import { schedule } from "plggmatic/Schedule/usecase/schedule";
import {
  multiColumn,
  multiColumnWith,
} from "plggmatic/Render/usecase/multiColumn";
import { cmdEffect } from "plgg-view/client";
import { loaded } from "plggmatic/Schedule/model/Msg";

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

const secs: ReadonlyArray<Sec> = [
  { id: "a", label: "Alpha" },
];
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
      source: sync(() => secs),
      child: "notes",
      query: query("Filter"),
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
const [m0] = s.init(makeUrl("/app", ""));
const at = (
  ...msgs: ReadonlyArray<
    ReturnType<typeof openMenu>
  >
) =>
  msgs.reduce(
    (m, msg) => s.update(msg, m)[0],
    m0,
  );

test("the root scene renders the menu as a navigation landmark", () => {
  const html = renderToString(
    multiColumn(s.scene(m0)),
  );
  return all([
    check(html.includes("<nav"), toBe(true)),
    check(html.includes("Sections"), toBe(true)),
    check(html.includes("pm-row"), toBe(true)),
  ]);
});

test("opening a collection adds a complementary list column", () => {
  const html = renderToString(
    multiColumn(
      s.scene(at(openMenu("sections"))),
    ),
  );
  return all([
    check(html.includes("<aside"), toBe(true)),
    check(html.includes("Alpha"), toBe(true)),
    // no × close button; the truncating href is the title link
    check(html.includes("pm-close"), toBe(false)),
    check(
      html.includes('href="/app"'),
      toBe(true),
    ),
    // the list column's query box is present
    check(html.includes("pm-query"), toBe(true)),
  ]);
});

test("drilling to a note adds the main detail column with its body and action", () => {
  const html = renderToString(
    multiColumn(
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
    check(html.includes("<main"), toBe(true)),
    check(
      html.includes("the body of one"),
      toBe(true),
    ),
    // the detail carries the destructive action button
    check(html.includes("Delete"), toBe(true)),
  ]);
});

test("a selected row is marked aria-current", () => {
  const html = renderToString(
    multiColumn(
      s.scene(
        at(openMenu("sections"), select(0, "a")),
      ),
    ),
  );
  return check(
    html.includes('aria-current="page"'),
    toBe(true),
  );
});

test("pushed columns carry a truncating close link and a breadcrumb trail", () => {
  const html = renderToString(
    multiColumn(
      s.scene(
        at(openMenu("sections"), select(0, "a")),
      ),
    ),
  );
  return all([
    // the title link resets to this level; no × close button
    check(html.includes("pm-close"), toBe(false)),
    check(
      html.includes('aria-label="Breadcrumb"'),
      toBe(true),
    ),
    check(
      html.includes("pm-crumb-here"),
      toBe(true),
    ),
  ]);
});

test("multiColumnWith can omit the internal breadcrumb", () => {
  const html = renderToString(
    multiColumnWith(
      s.scene(
        at(openMenu("sections"), select(0, "a")),
      ),
      {
        mapMsg: (msg) => msg,
        omitBreadcrumb: true,
      },
    ),
  );
  return check(
    html.includes('aria-label="Breadcrumb"'),
    toBe(false),
  );
});

test("a parked confirmation renders a modal dialog overlay", () => {
  const parked = at(
    openMenu("sections"),
    select(0, "a"),
    select(1, "n1"),
    requestAction("notes", "del", some("n1")),
  );
  const html = renderToString(
    multiColumn(s.scene(parked)),
  );
  return all([
    check(
      html.includes('role="dialog"'),
      toBe(true),
    ),
    check(
      html.includes("Delete note?"),
      toBe(true),
    ),
    check(html.includes(">Cancel<"), toBe(true)),
  ]);
});

test("the query input reflects the model's query text", () => {
  const html = renderToString(
    multiColumn(
      s.scene(
        at(
          openMenu("sections"),
          queryInput("Alp"),
        ),
      ),
    ),
  );
  return check(
    html.includes('value="Alp"'),
    toBe(true),
  );
});

test("multiColumnWith accepts app-owned header links and extra columns", () => {
  const html = renderToString(
    multiColumnWith(
      s.scene(at(openMenu("sections"))),
      {
        mapMsg: (msg) => msg,
        headerLinks: [
          {
            collection: "sections",
            label: "Add section",
            href: "/app?c=sections&add=section",
          },
          {
            collection: "sections",
            label: "Import",
            href: "/app?c=sections&import=1",
            active: true,
          },
        ],
        afterMenu: [
          {
            key: "section-submenu",
            title: "Section",
            close: none(),
            body: [
              slot([], [text("Section submenu")]),
            ],
          },
        ],
        extraColumns: [
          {
            key: "section-form",
            title: "Add section",
            close: none(),
            body: [
              slot(
                [],
                [text("App-owned form body")],
              ),
            ],
          },
        ],
      },
    ),
  );
  return all([
    check(
      html.includes("pm-list-action"),
      toBe(true),
    ),
    check(
      html.includes("Section submenu"),
      toBe(true),
    ),
    check(
      html.includes(">Add section<"),
      toBe(true),
    ),
    check(html.includes(">Import<"), toBe(true)),
    check(
      html.includes("App-owned form body"),
      toBe(true),
    ),
  ]);
});

// --- typed field values (mission point 3): a detail with
// every FieldValue kind, and the Reference rendered as a
// link to the target's CANONICAL address (the jump).

const typedDecl = declare({
  title: "Typed",
  menu: menu([
    menuEntry("Projects", "projects"),
    menuEntry("Clients", "clients"),
  ]),
  collections: [
    collection<
      Readonly<{ id: SoftStr; name: SoftStr }>
    >({
      id: "projects",
      title: "Projects",
      toRow: (p) =>
        row(p.id, p.name, [
          fieldOf(
            "Client",
            refValue("clients", "acme", "ACME"),
          ),
          fieldOf(
            "Budget",
            numValue("8.4", "M¥"),
          ),
          fieldOf("Active", flagValue(true)),
          fieldOf(
            "Since",
            momentValue("2026-04-01"),
          ),
          fieldOf(
            "Logo",
            mediaValue("/logo.png", "logo"),
          ),
        ]),
      source: sync(() => [
        { id: "p1", name: "Storefront" },
      ]),
    }),
    collection<
      Readonly<{ id: SoftStr; name: SoftStr }>
    >({
      id: "clients",
      title: "Clients",
      toRow: (c) => row(c.id, c.name),
      source: sync(() => [
        { id: "acme", name: "ACME" },
      ]),
    }),
  ],
});

const ts = schedule(typedDecl);
const [tm0] = ts.init(makeUrl("/app", ""));
const typedDetail = [
  openMenu("projects"),
  select(0, "p1"),
].reduce((m, msg) => ts.update(msg, m)[0], tm0);

test("a Reference field renders as a link to the target's canonical address", () => {
  const html = renderToString(
    multiColumn(ts.scene(typedDetail)),
  );
  return all([
    check(
      html.includes(
        'href="/app?c=clients&amp;p=acme"',
      ) ||
        html.includes(
          'href="/app?c=clients&p=acme"',
        ),
      toBe(true),
    ),
    check(
      html.includes("pm-field-ref"),
      toBe(true),
    ),
    check(html.includes(">ACME<"), toBe(true)),
  ]);
});

test("typed field kinds carry their class hooks and display text", () => {
  const html = renderToString(
    multiColumn(ts.scene(typedDetail)),
  );
  return all([
    check(
      html.includes("pm-field-num"),
      toBe(true),
    ),
    check(html.includes("8.4 M¥"), toBe(true)),
    check(
      html.includes("pm-field-flag"),
      toBe(true),
    ),
    check(
      html.includes("pm-field-moment"),
      toBe(true),
    ),
    check(
      html.includes("2026-04-01"),
      toBe(true),
    ),
    check(
      html.includes("pm-field-media"),
      toBe(true),
    ),
    check(
      html.includes('src="/logo.png"'),
      toBe(true),
    ),
  ]);
});
