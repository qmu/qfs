import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import {
  type SoftStr,
  none,
  isNone,
  matchOption,
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
import { openMenu } from "plggmatic/Schedule/model/Msg";
import { schedule } from "plggmatic/Schedule/usecase/schedule";
import {
  type Scene,
  type Level,
  menuLevel$,
  listLevel$,
  boardLevel$,
  detailLevel$,
} from "plggmatic/Schedule/model/Scene";
import {
  type Screen,
  currentScreen,
} from "plggmatic/Render/model/screen";

type Sec = Readonly<{
  id: SoftStr;
  label: SoftStr;
}>;
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
    }),
  ],
});
const s = schedule(decl);
const [m0] = s.init(makeUrl("/", ""));

// which Screen kind the deepest level is, or "none"
const kindOf = (scene: Scene): SoftStr =>
  matchOption<Screen, SoftStr>(
    () => "none",
    (screen: Level) =>
      match(screen)(
        [menuLevel$(), () => "menu"],
        [listLevel$(), () => "list"],
        [boardLevel$(), () => "board"],
        [detailLevel$(), () => "detail"],
      ),
  )(currentScreen(scene));

test("the current screen is the menu when nothing is open", () =>
  check(kindOf(s.scene(m0)), toBe("menu")));

test("opening a collection makes the list the current screen", () => {
  const opened = s.update(
    openMenu("sections"),
    m0,
  )[0];
  return check(
    kindOf(s.scene(opened)),
    toBe("list"),
  );
});

test("a structurally empty scene has no current screen", () => {
  const empty: Scene = {
    title: "x",
    levels: [],
    confirm: none(),
  };
  return all([
    check(kindOf(empty), toBe("none")),
    check(
      isNone(currentScreen(empty)),
      toBe(true),
    ),
  ]);
});
