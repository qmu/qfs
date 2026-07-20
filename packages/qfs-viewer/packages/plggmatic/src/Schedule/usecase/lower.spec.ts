import {
  test,
  check,
  all,
  toBe,
  toEqual,
} from "plgg-test";
import {
  type SoftStr,
  some,
  none,
  isNone,
  getOr,
  match,
} from "plgg";
import { makeUrl } from "plgg-view/client";
import { row } from "plggmatic/Declare/model/Row";
import { sync } from "plggmatic/Declare/model/Source";
import { collection } from "plggmatic/Declare/model/Collection";
import {
  menu,
  menuEntry,
} from "plggmatic/Declare/model/Menu";
import { declare } from "plggmatic/Declare/model/Declaration";
import { type Model } from "plggmatic/Schedule/model/Model";
import {
  type View,
  menuView,
  listView,
  detailView,
  menuView$,
  listView$,
  detailView$,
  binding,
} from "plggmatic/Schedule/model/View";
import {
  type SchedulerMsg,
  navigate,
  openMenu,
  select,
  queryInput,
} from "plggmatic/Schedule/model/Msg";
import {
  focusedView,
  sliceOf,
} from "plggmatic/Schedule/usecase/lower";
import { schedule } from "plggmatic/Schedule/usecase/schedule";

// --- fixture: sections → notes (childless leaf), plus a
// collection whose child dangles, and a dangling root.

const decl = declare({
  title: "Demo",
  menu: menu([
    menuEntry("Sections", "sections"),
    menuEntry("Ghost", "ghost"),
  ]),
  collections: [
    collection<
      Readonly<{ id: SoftStr; label: SoftStr }>
    >({
      id: "sections",
      title: "Sections",
      toRow: (s) => row(s.id, s.label),
      source: sync(() => [
        { id: "a", label: "Alpha" },
      ]),
      child: "notes",
    }),
    collection<
      Readonly<{ id: SoftStr; label: SoftStr }>
    >({
      id: "notes",
      title: "Notes",
      toRow: (n) => row(n.id, n.label),
      source: sync(() => [
        { id: "n1", label: "One" },
      ]),
    }),
    collection<
      Readonly<{ id: SoftStr; label: SoftStr }>
    >({
      id: "loose",
      title: "Loose",
      toRow: (x) => row(x.id, x.label),
      source: sync(() => []),
      child: "nowhere",
    }),
  ],
});

const focused = focusedView(decl);
const s = schedule(decl);
const [m0] = s.init(makeUrl("/app", ""));
const step = (
  msg: SchedulerMsg,
  model: Model,
): Model => s.update(msg, model)[0];

/** The view's tag, for shape assertions. */
const tagOf = (v: View): SoftStr =>
  match(v)(
    [menuView$(), (): SoftStr => "menu"],
    [
      listView$(),
      ({ content }): SoftStr => `list:${content}`,
    ],
    [
      detailView$(),
      ({ content }): SoftStr =>
        `detail:${content}`,
    ],
  );

test("focusedView derives menu, list, and detail along the drill", () =>
  all([
    check(
      tagOf(focused(none(), [])),
      toBe("menu"),
    ),
    check(
      tagOf(focused(some("sections"), [])),
      toBe("list:sections"),
    ),
    check(
      tagOf(focused(some("sections"), ["a"])),
      toBe("list:notes"),
    ),
    check(
      tagOf(
        focused(some("sections"), ["a", "n1"]),
      ),
      toBe("detail:notes"),
    ),
  ]));

test("focusedView degrades totally on junk addresses", () =>
  all([
    // dangling root: addressed but unresolvable list
    check(
      tagOf(focused(some("ghost"), [])),
      toBe("list:ghost"),
    ),
    check(
      tagOf(focused(some("ghost"), ["x"])),
      toBe("list:ghost"),
    ),
    // dangling child: the declared-but-unresolved list
    check(
      tagOf(focused(some("loose"), ["x"])),
      toBe("list:nowhere"),
    ),
    // junk beyond the chain clamps to the deepest list
    check(
      tagOf(
        focused(some("sections"), [
          "a",
          "n1",
          "zzz",
        ]),
      ),
      toBe("list:notes"),
    ),
  ]));

test("sliceOf projects a target back into the slice encoding", () =>
  all([
    check(
      isNone(sliceOf(menuView(), []).root),
      toBe(true),
    ),
    check(
      getOr("")(
        sliceOf(listView("sections"), []).root,
      ),
      toBe("sections"),
    ),
    check(
      sliceOf(detailView("notes"), [
        binding("sections", "a"),
        binding("notes", "n1"),
      ]).path,
      toEqual(["a", "n1"]),
    ),
    check(
      getOr("")(
        sliceOf(detailView("notes"), [
          binding("sections", "a"),
          binding("notes", "n1"),
        ]).root,
      ),
      toBe("sections"),
    ),
    // no bindings: the target's own collection is the root
    check(
      getOr("")(
        sliceOf(detailView("notes"), []).root,
      ),
      toBe("notes"),
    ),
    // a navigation always resets the query
    check(
      sliceOf(listView("sections"), []).query,
      toBe(""),
    ),
  ]));

test("navigate to a root list is openMenu", () => {
  const viaNavigate = step(
    navigate(listView("sections"), []),
    m0,
  );
  const viaMenu = step(openMenu("sections"), m0);
  return all([
    check(
      getOr("")(viaNavigate.root),
      toBe(getOr("")(viaMenu.root)),
    ),
    check(
      viaNavigate.path,
      toEqual(viaMenu.path),
    ),
  ]);
});

test("navigate with bindings is the select chain", () => {
  const viaNavigate = step(
    navigate(detailView("notes"), [
      binding("sections", "a"),
      binding("notes", "n1"),
    ]),
    m0,
  );
  const viaSelects = step(
    select(1, "n1"),
    step(
      select(0, "a"),
      step(openMenu("sections"), m0),
    ),
  );
  return all([
    check(
      viaNavigate.path,
      toEqual(viaSelects.path),
    ),
    check(
      getOr("")(viaNavigate.root),
      toBe(getOr("")(viaSelects.root)),
    ),
    check(
      s.toUrl(viaNavigate).search,
      toBe(s.toUrl(viaSelects).search),
    ),
  ]);
});

test("navigate resets the active query", () => {
  const queried = step(
    queryInput("Alp"),
    step(openMenu("sections"), m0),
  );
  const moved = step(
    navigate(listView("sections"), []),
    queried,
  );
  return all([
    check(queried.query, toBe("Alp")),
    check(moved.query, toBe("")),
  ]);
});

test("the focused view round-trips through sliceOf", () => {
  const to: View = detailView("notes");
  const bindings = [
    binding("sections", "a"),
    binding("notes", "n1"),
  ];
  const slice = sliceOf(to, bindings);
  return check(
    tagOf(focused(slice.root, slice.path)),
    toBe("detail:notes"),
  );
});
