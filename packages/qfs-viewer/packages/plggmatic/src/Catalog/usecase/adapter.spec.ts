import {
  test,
  check,
  all,
  toBe,
  toEqual,
} from "plgg-test";
import { type SoftStr, matchOption } from "plgg";
import {
  type SchedulerMsg,
  openMenu,
  queryInput,
} from "plggmatic/Schedule/model/Msg";
import {
  tool,
  nullary,
  textArg,
  enumArg,
  emit,
  runFlowEffect,
} from "plggmatic/Catalog/model/tool";
import {
  type ModelContext,
  descriptorOf,
  diffCatalog,
  syncCatalog,
  invokeWith,
  detectModelContext,
  inertModelContext,
} from "plggmatic/Catalog/usecase/adapter";

const a = tool({
  name: "a",
  description: "A",
  input: nullary(),
  effect: emit(() => openMenu("x")),
});
const b = tool({
  name: "b",
  description: "B",
  input: enumArg("id", "d", ["1", "2"]),
  effect: emit((v: SoftStr) => queryInput(v)),
});
const bChanged = tool({
  name: "b",
  description: "B (relabelled)",
  input: enumArg("id", "d", ["1", "2"]),
  effect: emit((v: SoftStr) => queryInput(v)),
});
const rf = tool({
  name: "run_flow",
  description: "run",
  input: textArg("flow", "src"),
  effect: runFlowEffect(),
});

const noopInvoker = invokeWith(
  () => undefined,
  () => undefined,
);
const names = (
  ts: ReadonlyArray<Readonly<{ name: SoftStr }>>,
): ReadonlyArray<SoftStr> =>
  ts.map(
    (t: Readonly<{ name: SoftStr }>) => t.name,
  );

test("descriptorOf keeps name/description/input and drops the effect", () => {
  const d = descriptorOf(b);
  return all([
    check(d.name, toBe("b")),
    check(d.description, toBe("B")),
    // the input schema survives structurally (it is pure data)
    check(d.input, toEqual(b.input)),
  ]);
});

test("diffCatalog adds every tool against an empty registry", () => {
  const diff = diffCatalog([], [a, b]);
  return all([
    check(names(diff.added), toEqual(["a", "b"])),
    check(diff.removed.length, toBe(0)),
    check(diff.changed.length, toBe(0)),
  ]);
});

test("diffCatalog is empty when nothing changed (minimality)", () => {
  const diff = diffCatalog(
    [descriptorOf(a), descriptorOf(b)],
    [a, b],
  );
  return all([
    check(diff.added.length, toBe(0)),
    check(diff.removed.length, toBe(0)),
    check(diff.changed.length, toBe(0)),
  ]);
});

test("diffCatalog reports only the changed tool under a stable name", () => {
  const diff = diffCatalog(
    [descriptorOf(a), descriptorOf(b)],
    [a, bChanged],
  );
  return all([
    check(names(diff.changed), toEqual(["b"])),
    check(diff.added.length, toBe(0)),
    check(diff.removed.length, toBe(0)),
  ]);
});

test("diffCatalog reports a dropped tool as removed", () => {
  const diff = diffCatalog(
    [descriptorOf(a), descriptorOf(b)],
    [a],
  );
  return all([
    check(diff.removed, toEqual(["b"])),
    check(diff.added.length, toBe(0)),
    check(diff.changed.length, toBe(0)),
  ]);
});

test("syncCatalog registers the initial catalog and returns its descriptors", () => {
  const log: Array<SoftStr> = [];
  const host: ModelContext = {
    registerTool: (d) => {
      log.push(`+${d.name}`);
    },
    unregisterTool: (n) => {
      log.push(`-${n}`);
    },
  };
  const descriptors = syncCatalog(
    host,
    noopInvoker,
    [],
    [a, b],
  );
  return all([
    check(log, toEqual(["+a", "+b"])),
    check(
      names(descriptors),
      toEqual(["a", "b"]),
    ),
  ]);
});

test("syncCatalog re-registers ONLY the changed tool across two settles", () => {
  const log: Array<SoftStr> = [];
  const host: ModelContext = {
    registerTool: (d) => {
      log.push(`+${d.name}`);
    },
    unregisterTool: (n) => {
      log.push(`-${n}`);
    },
  };
  const first = syncCatalog(
    host,
    noopInvoker,
    [],
    [a, b],
  );
  const before = log.length;
  syncCatalog(host, noopInvoker, first, [
    a,
    bChanged,
  ]);
  return check(
    log.slice(before),
    toEqual(["-b", "+b"]),
  );
});

test("invokeWith dispatches an Emit tool's message and hands run_flow to the runner", () => {
  const dispatched: Array<SoftStr> = [];
  const runners: Array<SoftStr> = [];
  const invoker = invokeWith(
    (m: SchedulerMsg) => {
      dispatched.push(m.__tag);
    },
    (src: SoftStr) => {
      runners.push(src);
    },
  );
  invoker(a)("");
  invoker(rf)("(flow x (none))");
  return all([
    check(dispatched, toEqual(["OpenMenu"])),
    check(runners, toEqual(["(flow x (none))"])),
  ]);
});

test("detectModelContext is None when the API or its methods are absent", () =>
  all([
    check(
      isNone(detectModelContext(undefined)),
      toBe(true),
    ),
    check(
      isNone(detectModelContext(null)),
      toBe(true),
    ),
    check(
      isNone(detectModelContext({})),
      toBe(true),
    ),
    check(
      isNone(
        detectModelContext({ modelContext: {} }),
      ),
      toBe(true),
    ),
    check(
      isNone(
        detectModelContext({
          modelContext: {
            registerTool: 1,
            unregisterTool: 2,
          },
        }),
      ),
      toBe(true),
    ),
  ]));

test("detectModelContext forwards to a present host's methods", () => {
  const calls: Array<SoftStr> = [];
  const nav = {
    modelContext: {
      registerTool: (
        _d: unknown,
        _invoke: unknown,
      ) => {
        calls.push("reg");
      },
      unregisterTool: (_n: unknown) => {
        calls.push("unreg");
      },
    },
  };
  return all([
    check(
      isNone(detectModelContext(nav)),
      toBe(false),
    ),
    check(
      run(detectModelContext(nav), calls),
      toEqual(["reg", "unreg"]),
    ),
  ]);
});

test("the inert host makes syncCatalog a total no-op that still tracks descriptors", () =>
  check(
    names(
      syncCatalog(
        inertModelContext,
        noopInvoker,
        [],
        [a, b, rf],
      ),
    ),
    toEqual(["a", "b", "run_flow"]),
  ));

// --- local helpers -------------------------------------

/** Whether a detected {@link ModelContext} option is None. */
const isNone = (
  o: ReturnType<typeof detectModelContext>,
): boolean =>
  matchOption<ModelContext, boolean>(
    () => true,
    () => false,
  )(o);

/** Drives a detected host (register + unregister) and returns the call log. */
const run = (
  o: ReturnType<typeof detectModelContext>,
  calls: Array<SoftStr>,
): ReadonlyArray<SoftStr> => {
  matchOption<ModelContext, void>(
    () => undefined,
    (mc: ModelContext) => {
      mc.registerTool(
        descriptorOf(a),
        () => undefined,
      );
      mc.unregisterTool("a");
    },
  )(o);
  return calls;
};
