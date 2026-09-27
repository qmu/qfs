import {
  test,
  check,
  all,
  toBe,
  toEqual,
} from "plgg-test";
import { match } from "plgg";
import {
  type Cmd,
  cmdNone$,
  cmdBatch$,
  cmdEffect$,
} from "plgg-view/client";
import {
  asMembers,
  asAccounts,
  asSessions,
  emptySnapshot,
  type Snapshot,
} from "../model/Cluster.ts";
import {
  type Column,
  type Model,
  type Msg,
  initialModel,
  makeUpdate,
  columns,
  areaCount,
  open,
  pickFirst,
  pickCwd,
  pickSession,
  tick,
  polled,
  pollFailed,
  subscriptions,
  usage,
  iso,
  list$,
  detail$,
} from "./walk.ts";

const snap: Snapshot = {
  members: asMembers([
    {
      name: "box-a",
      hostname: "mac",
      status: "online",
      cpu_pct: 12.5,
      mem_used: 2 * 1024 ** 3,
      mem_total: 16 * 1024 ** 3,
      disk_used: 1024 ** 3,
      disk_total: 512 * 1024 ** 3,
      last_seen: 1790000000,
    },
    { name: "box-b", status: "offline" },
    { hostname: "nameless" },
    "junk",
  ]),
  accounts: asAccounts([
    {
      provider: "claude-code",
      label: "work",
      email: "a@example.com",
      plan: "max",
      status: "assigned",
      assigned_member: "box-a",
    },
    {
      provider: "codex",
      label: "cx",
      email: null,
      plan: null,
      status: "available",
      assigned_member: null,
    },
  ]),
  sessions: asSessions([
    {
      member: "box-a",
      id: "s1",
      cwd: "/w/qfs",
      status: "busy",
      last_message: "testing",
      ts: 1790000000,
    },
    {
      member: "box-a",
      id: "s2",
      cwd: "/w/qfs",
      status: "idle",
      last_message: null,
      ts: 0,
    },
    { member: "box-a", id: "s3", cwd: null },
    { member: "box-b" },
  ]),
};

const cmdKind = (c: Cmd<Msg>): string =>
  match(c)(
    [cmdNone$(), (): string => "none"],
    [cmdBatch$(), (): string => "batch"],
    [cmdEffect$(), (): string => "effect"],
  );

const polledMsg = polled(snap, 42);
const update = makeUpdate(() =>
  Promise.resolve(polledMsg),
);

const run = (
  msgs: ReadonlyArray<Msg>,
  from: Model = initialModel,
): Model =>
  msgs.reduce(
    (m, msg) => update(msg, m)[0],
    from,
  );

const shape = (c: Column): string =>
  match(c)(
    [
      list$(),
      ({ content }): string =>
        `list:${content.key}:${content.items.length}:${content.items
          .filter((i) => i.active)
          .map((i) => i.id)
          .join(",")}`,
    ],
    [
      detail$(),
      ({ content }): string =>
        `detail:${content.key}:${content.fields.length}`,
    ],
  );

test("casters drop malformed rows and default cells", () =>
  all([
    check(snap.members.length, toBe(2)),
    check(snap.members[1]?.cpuPct, toBe(0)),
    check(snap.accounts[1]?.email, toBe("")),
    check(snap.sessions.length, toBe(3)),
    check(asMembers({}).length, toBe(0)),
  ]));

test("the server pool walks member -> directory -> session", () => {
  const m = run([polledMsg]);
  const walked = run(
    [
      pickFirst("box-a"),
      pickCwd("/w/qfs"),
      pickSession("s1"),
    ],
    m,
  );
  return all([
    check(
      columns(m).map(shape),
      toEqual(["list:servers:2:"]),
    ),
    check(
      columns(run([pickFirst("box-a")], m)).map(
        shape,
      ),
      toEqual([
        "list:servers:2:box-a",
        "detail:member:7",
        "list:cwds:2:",
      ]),
    ),
    check(
      columns(walked).map(shape),
      toEqual([
        "list:servers:2:box-a",
        "detail:member:7",
        "list:cwds:2:/w/qfs",
        "list:sessions:2:s1",
        "detail:session:6",
      ]),
    ),
    // a refresh keeps the selection
    check(
      columns(run([polledMsg], walked)).length,
      toBe(5),
    ),
    // an unknown session shows no detail
    check(
      columns(run([pickSession("gone")], walked))
        .length,
      toBe(4),
    ),
  ]);
});

test("allocation and account areas", () => {
  const m = run([polledMsg]);
  return all([
    check(
      columns(
        run(
          [
            open("allocation"),
            pickFirst("box-b"),
          ],
          m,
        ),
      ).map(shape),
      toEqual([
        "list:allocation:2:box-b",
        "list:cwds:0:",
      ]),
    ),
    check(
      columns(run([open("accounts")], m)).map(
        shape,
      ),
      toEqual(["list:accounts:2:"]),
    ),
    check(
      columns(
        run(
          [open("accounts"), pickFirst("cx")],
          m,
        ),
      ).map(shape),
      toEqual([
        "list:accounts:2:cx",
        "detail:account:6",
      ]),
    ),
    check(areaCount(snap, "servers"), toBe(2)),
    check(areaCount(snap, "accounts"), toBe(2)),
    check(areaCount(snap, "allocation"), toBe(3)),
    check(
      columns(initialModel).map(shape),
      toEqual(["list:servers:0:"]),
    ),
    check(
      columns({
        ...initialModel,
        snapshot: emptySnapshot,
        selection: {
          area: "accounts",
          first: "",
          cwd: "",
          session: "",
        },
      }).map(shape),
      toEqual(["list:accounts:0:"]),
    ),
  ]);
});

test("tick polls, poll results land, failures are shown", () => {
  const [, tickCmd] = update(
    tick(),
    initialModel,
  );
  const failedModel = run([pollFailed("down")]);
  return all([
    check(cmdKind(tickCmd), toBe("effect")),
    check(
      cmdKind(
        update(open("servers"), initialModel)[1],
      ),
      toBe("none"),
    ),
    check(run([polledMsg]).polledAt, toBe(42)),
    check(failedModel.error, toBe("down")),
    check(
      run([polledMsg], failedModel).error,
      toBe(""),
    ),
    check(
      subscriptions().__tag,
      toBe("SubInterval"),
    ),
  ]);
});

test("formatting helpers", () =>
  all([
    check(
      usage(1024 ** 3, 2 * 1024 ** 3),
      toBe("1.0 / 2.0 GB"),
    ),
    check(iso(0), toBe("—")),
    check(
      iso(1),
      toBe("1970-01-01T00:00:01.000Z"),
    ),
  ]));
