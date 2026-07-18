import { test, check, toBe } from "plgg-test";
import {
  type SoftStr,
  type Option,
  ok,
  err,
  some,
  match,
} from "plgg";
import {
  type Cmd,
  cmdNone$,
  cmdBatch$,
  cmdEffect$,
} from "plgg-view/client";
import {
  type Row,
  row as makeRow,
} from "plggmatic/Declare/model/Row";
import { dynamic } from "plggmatic/Declare/model/Source";
import { collection } from "plggmatic/Declare/model/Collection";
import { declare } from "plggmatic/Declare/model/Declaration";
import {
  menu,
  menuEntry,
} from "plggmatic/Declare/model/Menu";
import {
  type Effect,
  actor,
  effect,
  hostAdapter,
} from "plggmatic/Declare/model/Adapter";
import { type SchedulerMsg } from "plggmatic/Schedule/model/Msg";
import {
  emptyModel,
  slotOf,
  loadedSlot$,
  failedSlot$,
} from "plggmatic/Schedule/model/Model";
import { schedule } from "plggmatic/Schedule/usecase/schedule";
import { applyVia } from "plggmatic/Declare/usecase/apply";

const s = schedule(
  declare({
    title: "T",
    menu: menu([menuEntry("Tasks", "tasks")]),
    collections: [
      collection<Row>({
        id: "tasks",
        title: "Tasks",
        toRow: (r: Row) => r,
        source: dynamic(),
      }),
    ],
  }),
);

const build = (target: Option<SoftStr>): Effect =>
  effect({
    collection: "tasks",
    verb: "delete",
    target,
  });

/** Runs a single-effect Cmd to the message it resolves to. */
const runEffect = (
  cmd: Cmd<SchedulerMsg>,
): Promise<SchedulerMsg> =>
  match(cmd)(
    [
      cmdNone$(),
      (): Promise<SchedulerMsg> =>
        Promise.reject(new Error("no effect")),
    ],
    [
      cmdBatch$(),
      (): Promise<SchedulerMsg> =>
        Promise.reject(new Error("a batch")),
    ],
    [
      cmdEffect$(),
      ({ content }): Promise<SchedulerMsg> =>
        content(),
    ],
  );

test("applyVia folds apply Ok to a Loaded slot on the action path", async () => {
  const ad = hostAdapter({
    read: () => Promise.resolve(ok([])),
    apply: () =>
      Promise.resolve(
        ok({
          collection: "tasks",
          rows: [makeRow("t2", "kept")],
        }),
      ),
  });
  const cmd = applyVia(
    ad,
    actor("u"),
    "tasks",
    build,
  )(some("t1"));
  const msg = await runEffect(cmd);
  const [model] = s.update(msg, emptyModel("/"));
  return check(
    match(slotOf(model, "tasks"))(
      [
        loadedSlot$(),
        ({ content }): number => content.length,
      ],
      [failedSlot$(), (): number => -1],
    ),
    toBe(1),
  );
});

test("applyVia folds apply Err to a Failed slot (failure as a value)", async () => {
  const ad = hostAdapter({
    read: () => Promise.resolve(ok([])),
    apply: () =>
      Promise.resolve(err(new Error("boom"))),
  });
  const cmd = applyVia(
    ad,
    actor("u"),
    "tasks",
    build,
  )(some("t1"));
  const msg = await runEffect(cmd);
  const [model] = s.update(msg, emptyModel("/"));
  return check(
    match(slotOf(model, "tasks"))(
      [
        failedSlot$(),
        ({ content }): SoftStr => content,
      ],
      [loadedSlot$(), (): SoftStr => ""],
    ),
    toBe("boom"),
  );
});
