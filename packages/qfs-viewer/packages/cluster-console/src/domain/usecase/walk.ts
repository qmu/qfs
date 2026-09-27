/**
 * The cluster console's column walk, as a pure TEA core
 * on plgg. The design borrows plggmatic's column idea
 * (select a row in one column and the next column opens
 * to its right) without depending on it: a fixed area
 * column (Server pool / Account pool / Session
 * allocation), then the selected area's list, then the
 * drill-down member → directory → session. A 5 s `Tick`
 * asks the injected poll effect for a fresh snapshot; the
 * selection survives every refresh.
 */
import {
  type Box,
  type Icon,
  box,
  icon,
  pattern,
  match,
} from "plgg";
import {
  type Cmd,
  type Sub,
  cmdNone,
  cmdEffect,
  interval,
} from "plgg-view/client";
import {
  type Account,
  type Member,
  type Session,
  type Snapshot,
  emptySnapshot,
} from "../model/Cluster.ts";

/** The poll period. */
export const POLL_MS = 5000;

/** The three top-level areas. */
export type Area =
  | "servers"
  | "accounts"
  | "allocation";

export const areas: ReadonlyArray<
  readonly [Area, string]
> = [
  ["servers", "Server pool"],
  ["accounts", "Account pool"],
  ["allocation", "Session allocation"],
];

/** Where the walk stands. `""` means "nothing chosen". */
export type Selection = Readonly<{
  area: Area;
  /** A member (servers/allocation) or account label. */
  first: string;
  /** A working directory under the member. */
  cwd: string;
  /** A session id under the directory. */
  session: string;
}>;

/** The console model. */
export type Model = Readonly<{
  snapshot: Snapshot;
  selection: Selection;
  error: string;
  polledAt: number;
}>;

/** The console messages. */
export type Msg =
  | Box<"Open", Area>
  | Box<"PickFirst", string>
  | Box<"PickCwd", string>
  | Box<"PickSession", string>
  | Icon<"Tick">
  | Box<
      "Polled",
      Readonly<{ snap: Snapshot; at: number }>
    >
  | Box<"PollFailed", string>;

export const open = (a: Area): Msg =>
  box("Open")(a);
export const pickFirst = (id: string): Msg =>
  box("PickFirst")(id);
export const pickCwd = (cwd: string): Msg =>
  box("PickCwd")(cwd);
export const pickSession = (id: string): Msg =>
  box("PickSession")(id);
export const tick = (): Msg => icon("Tick");
export const polled = (
  snap: Snapshot,
  at: number,
): Msg => box("Polled")({ snap, at });
export const pollFailed = (e: string): Msg =>
  box("PollFailed")(e);

const open$ = () => pattern("Open")();
const pickFirst$ = () => pattern("PickFirst")();
const pickCwd$ = () => pattern("PickCwd")();
const pickSession$ = () =>
  pattern("PickSession")();
const tick$ = () => pattern("Tick")();
const polled$ = () => pattern("Polled")();
const pollFailed$ = () => pattern("PollFailed")();

/** The initial model: the Server pool open, no data. */
export const initialModel: Model = {
  snapshot: emptySnapshot,
  selection: {
    area: "servers",
    first: "",
    cwd: "",
    session: "",
  },
  error: "",
  polledAt: 0,
};

type Step = readonly [Model, Cmd<Msg>];

/** The fetch effect: one snapshot, or a failure `Msg`. */
export type Poll = () => Promise<Msg>;

/** The pure update, closed over the poll effect. */
export const makeUpdate =
  (poll: Poll) =>
  (msg: Msg, model: Model): Step => {
    const sel = model.selection;
    const at = (s: Selection): Step => [
      { ...model, selection: s },
      cmdNone(),
    ];
    return match(msg)(
      [
        open$(),
        ({ content }): Step =>
          at({
            area: content,
            first: "",
            cwd: "",
            session: "",
          }),
      ],
      [
        pickFirst$(),
        ({ content }): Step =>
          at({
            ...sel,
            first: content,
            cwd: "",
            session: "",
          }),
      ],
      [
        pickCwd$(),
        ({ content }): Step =>
          at({
            ...sel,
            cwd: content,
            session: "",
          }),
      ],
      [
        pickSession$(),
        ({ content }): Step =>
          at({ ...sel, session: content }),
      ],
      [
        tick$(),
        (): Step => [model, cmdEffect(poll)],
      ],
      [
        polled$(),
        ({ content }): Step => [
          {
            ...model,
            snapshot: content.snap,
            error: "",
            polledAt: content.at,
          },
          cmdNone(),
        ],
      ],
      [
        pollFailed$(),
        ({ content }): Step => [
          { ...model, error: content },
          cmdNone(),
        ],
      ],
    );
  };

/** Poll every {@link POLL_MS}. */
export const subscriptions = (): Sub<Msg> =>
  interval("cluster-poll", POLL_MS, tick);

// --- projection: the model as columns ------------------

/** One selectable line of a list column. */
export type Item = Readonly<{
  id: string;
  label: string;
  detail: string;
  tone: string;
  active: boolean;
  msg: Msg;
}>;

/** A labelled value of a detail column. */
export type Field = readonly [string, string];

/** A column: a list of items or a detail card. */
export type Column =
  | Box<
      "List",
      Readonly<{
        key: string;
        title: string;
        items: ReadonlyArray<Item>;
        empty: string;
      }>
    >
  | Box<
      "Detail",
      Readonly<{
        key: string;
        title: string;
        fields: ReadonlyArray<Field>;
      }>
    >;

export const list$ = () => pattern("List")();
export const detail$ = () => pattern("Detail")();

const listCol = (
  key: string,
  title: string,
  items: ReadonlyArray<Item>,
  empty: string,
): Column =>
  box("List")({ key, title, items, empty });

const detailCol = (
  key: string,
  title: string,
  fields: ReadonlyArray<Field>,
): Column =>
  box("Detail")({ key, title, fields });

const GIB = 1024 ** 3;

/** `used / total GB`. */
export const usage = (
  used: number,
  total: number,
): string =>
  `${(used / GIB).toFixed(1)} / ${(total / GIB).toFixed(1)} GB`;

/** Epoch seconds as an ISO instant (`—` for 0). */
export const iso = (secs: number): string =>
  secs > 0
    ? new Date(secs * 1000).toISOString()
    : "—";

const orDash = (s: string): string =>
  s === "" ? "—" : s;

/** The label a session's cwd is grouped under. */
export const cwdOf = (s: Session): string =>
  s.cwd === "" ? "(no directory)" : s.cwd;

const sessionsOf = (
  snap: Snapshot,
  member: string,
): ReadonlyArray<Session> =>
  snap.sessions.filter(
    (s) => s.member === member,
  );

const memberItem = (
  m: Member,
  sel: Selection,
): Item => ({
  id: m.name,
  label: `${m.name} · ${orDash(m.hostname)}`,
  detail: `cpu ${m.cpuPct.toFixed(0)}% · mem ${usage(m.memUsed, m.memTotal)} · disk ${usage(m.diskUsed, m.diskTotal)} · seen ${iso(m.lastSeen).slice(11, 19)}`,
  tone: m.status,
  active: sel.first === m.name,
  msg: pickFirst(m.name),
});

const allocationItem = (
  snap: Snapshot,
  m: Member,
  sel: Selection,
): Item => {
  const n = sessionsOf(snap, m.name).length;
  return {
    id: m.name,
    label: m.name,
    detail: `${n} session${n === 1 ? "" : "s"}`,
    tone: m.status,
    active: sel.first === m.name,
    msg: pickFirst(m.name),
  };
};

const accountItem = (
  a: Account,
  sel: Selection,
): Item => ({
  id: a.label,
  label: `${a.label} · ${a.provider}`,
  detail: `${orDash(a.plan)} · ${a.assignedMember === "" ? "unassigned" : `→ ${a.assignedMember}`}`,
  tone: a.status,
  active: sel.first === a.label,
  msg: pickFirst(a.label),
});

/** The directory items under the selected member. */
const cwdItems = (
  snap: Snapshot,
  sel: Selection,
): ReadonlyArray<Item> => {
  const all = sessionsOf(snap, sel.first).map(
    cwdOf,
  );
  return [...new Set(all)].sort().map((cwd) => {
    const n = all.filter((c) => c === cwd).length;
    return {
      id: cwd,
      label: cwd,
      detail: `${n} session${n === 1 ? "" : "s"}`,
      tone: "",
      active: sel.cwd === cwd,
      msg: pickCwd(cwd),
    };
  });
};

const sessionItems = (
  snap: Snapshot,
  sel: Selection,
): ReadonlyArray<Item> =>
  sessionsOf(snap, sel.first)
    .filter((s) => cwdOf(s) === sel.cwd)
    .map((s) => ({
      id: s.id,
      label: orDash(s.lastMessage),
      detail: `${s.id} · ${iso(s.ts)}`,
      tone: s.status,
      active: sel.session === s.id,
      msg: pickSession(s.id),
    }));

const memberFields = (
  m: Member,
): ReadonlyArray<Field> => [
  ["Name", m.name],
  ["Hostname", orDash(m.hostname)],
  ["Status", m.status],
  ["CPU", `${m.cpuPct.toFixed(1)} %`],
  ["Memory", usage(m.memUsed, m.memTotal)],
  ["Disk", usage(m.diskUsed, m.diskTotal)],
  ["Last seen", iso(m.lastSeen)],
];

const accountFields = (
  a: Account,
): ReadonlyArray<Field> => [
  ["Provider", a.provider],
  ["Label", a.label],
  ["Email", orDash(a.email)],
  ["Plan", orDash(a.plan)],
  ["Status", a.status],
  ["Assigned member", orDash(a.assignedMember)],
];

const sessionFields = (
  s: Session,
): ReadonlyArray<Field> => [
  ["Session", s.id],
  ["Member", s.member],
  ["Directory", cwdOf(s)],
  ["Status", orDash(s.status)],
  ["Last message", orDash(s.lastMessage)],
  ["Reported", iso(s.ts)],
];

/** The member → directory → session drill columns. */
const drill = (
  snap: Snapshot,
  sel: Selection,
): ReadonlyArray<Column> => {
  if (sel.first === "") {
    return [];
  }
  const dirs = listCol(
    "cwds",
    `${sel.first} · directories`,
    cwdItems(snap, sel),
    "no sessions reported",
  );
  if (sel.cwd === "") {
    return [dirs];
  }
  const sessions = listCol(
    "sessions",
    sel.cwd,
    sessionItems(snap, sel),
    "no sessions",
  );
  const chosen = sessionsOf(snap, sel.first).find(
    (s) => s.id === sel.session,
  );
  return chosen === undefined
    ? [dirs, sessions]
    : [
        dirs,
        sessions,
        detailCol(
          "session",
          "Session",
          sessionFields(chosen),
        ),
      ];
};

/**
 * The columns right of the area column, for the current
 * selection over the current snapshot.
 */
export const columns = (
  model: Model,
): ReadonlyArray<Column> => {
  const snap = model.snapshot;
  const sel = model.selection;
  switch (sel.area) {
    case "servers": {
      const m = snap.members.find(
        (x) => x.name === sel.first,
      );
      return [
        listCol(
          "servers",
          "Server pool",
          snap.members.map((x) =>
            memberItem(x, sel),
          ),
          "no members have joined",
        ),
        ...(m === undefined
          ? []
          : [
              detailCol(
                "member",
                m.name,
                memberFields(m),
              ),
            ]),
        ...drill(snap, sel),
      ];
    }
    case "allocation":
      return [
        listCol(
          "allocation",
          "Session allocation",
          snap.members.map((x) =>
            allocationItem(snap, x, sel),
          ),
          "no members have joined",
        ),
        ...drill(snap, sel),
      ];
    case "accounts": {
      const a = snap.accounts.find(
        (x) => x.label === sel.first,
      );
      return [
        listCol(
          "accounts",
          "Account pool",
          snap.accounts.map((x) =>
            accountItem(x, sel),
          ),
          "no accounts (qfs cluster account add)",
        ),
        ...(a === undefined
          ? []
          : [
              detailCol(
                "account",
                a.label,
                accountFields(a),
              ),
            ]),
      ];
    }
  }
};

/** The area column's counts, per area. */
export const areaCount = (
  snap: Snapshot,
  area: Area,
): number =>
  area === "servers"
    ? snap.members.length
    : area === "accounts"
      ? snap.accounts.length
      : snap.sessions.length;
