import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import { match } from "plgg";
import {
  tool,
  nullary,
  textArg,
  enumArg,
  emit,
  runFlowEffect,
  nullary$,
  textArg$,
  enumArg$,
  emit$,
  runFlow$,
} from "plggmatic/Catalog/model/tool";
import {
  openMenu,
  queryInput,
} from "plggmatic/Schedule/model/Msg";

test("input constructors fold exhaustively by kind", () =>
  all([
    check(
      match(nullary())(
        [nullary$(), (): string => "nullary"],
        [textArg$(), (): string => "text"],
        [enumArg$(), (): string => "enum"],
      ),
      toBe("nullary"),
    ),
    check(
      match(textArg("k", "d"))(
        [nullary$(), (): string => "nullary"],
        [
          textArg$(),
          ({ content }): string => content.name,
        ],
        [enumArg$(), (): string => "enum"],
      ),
      toBe("k"),
    ),
    check(
      match(enumArg("k", "d", ["a", "b"]))(
        [nullary$(), (): number => -1],
        [textArg$(), (): number => -1],
        [
          enumArg$(),
          ({ content }): number =>
            content.options.length,
        ],
      ),
      toBe(2),
    ),
  ]));

test("an Emit effect lowers its argument onto a message", () => {
  const t = tool({
    name: "filter_x",
    description: "filter",
    input: textArg("keyword", "type"),
    effect: emit((v: string) => queryInput(v)),
  });
  return check(
    match(t.effect)(
      [
        emit$(),
        ({ content }): string =>
          content("hi").__tag,
      ],
      [runFlow$(), (): string => "run"],
    ),
    toBe("QueryInput"),
  );
});

test("a RunFlow effect is the standing marker", () => {
  const t = tool({
    name: "run_flow",
    description: "run",
    input: textArg("flow", "src"),
    effect: runFlowEffect(),
  });
  return check(
    match(t.effect)(
      [emit$(), (): string => "emit"],
      [runFlow$(), (): string => "run"],
    ),
    toBe("run"),
  );
});

test("an Emit nullary tool ignores the passed argument", () => {
  const t = tool({
    name: "open_menu",
    description: "open",
    input: nullary(),
    effect: emit(() => openMenu("clients")),
  });
  return check(
    match(t.effect)(
      [
        emit$(),
        ({ content }): string =>
          content("ignored").__tag,
      ],
      [runFlow$(), (): string => "run"],
    ),
    toBe("OpenMenu"),
  );
});
