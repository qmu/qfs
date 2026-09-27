/**
 * The three collections a qfs cluster host serves on its
 * loopback JSON API (`GET /api/cluster/{members,accounts,
 * sessions}`), and total casters from the `unknown` a
 * `fetch().json()` yields. A malformed row is dropped,
 * never trusted; a non-array body is an empty list.
 */

/** A `/cluster/members` row. */
export type Member = Readonly<{
  name: string;
  hostname: string;
  status: string;
  cpuPct: number;
  memUsed: number;
  memTotal: number;
  diskUsed: number;
  diskTotal: number;
  lastSeen: number;
}>;

/** A `/cluster/accounts` row (metadata only). */
export type Account = Readonly<{
  provider: string;
  label: string;
  email: string;
  plan: string;
  status: string;
  assignedMember: string;
}>;

/** A `/cluster/sessions` row. */
export type Session = Readonly<{
  member: string;
  id: string;
  cwd: string;
  status: string;
  lastMessage: string;
  ts: number;
}>;

/** One poll of the host: all three collections. */
export type Snapshot = Readonly<{
  members: ReadonlyArray<Member>;
  accounts: ReadonlyArray<Account>;
  sessions: ReadonlyArray<Session>;
}>;

/** The empty snapshot (before the first poll). */
export const emptySnapshot: Snapshot = {
  members: [],
  accounts: [],
  sessions: [],
};

type Obj = Readonly<Record<string, unknown>>;

const isObj = (v: unknown): v is Obj =>
  typeof v === "object" &&
  v !== null &&
  !Array.isArray(v);

/** A string cell; absent/null/non-string is `""`. */
const str = (o: Obj, key: string): string => {
  const v = o[key];
  return typeof v === "string" ? v : "";
};

/** A numeric cell; absent/non-finite is `0`. */
const num = (o: Obj, key: string): number => {
  const v = o[key];
  return typeof v === "number" &&
    Number.isFinite(v)
    ? v
    : 0;
};

const rowsOf = <T>(
  body: unknown,
  cast: (o: Obj) => T | undefined,
): ReadonlyArray<T> =>
  Array.isArray(body)
    ? body.flatMap((v: unknown) => {
        const t = isObj(v) ? cast(v) : undefined;
        return t === undefined ? [] : [t];
      })
    : [];

/** Cast a members body. */
export const asMembers = (
  body: unknown,
): ReadonlyArray<Member> =>
  rowsOf(body, (o) =>
    str(o, "name") === ""
      ? undefined
      : {
          name: str(o, "name"),
          hostname: str(o, "hostname"),
          status: str(o, "status"),
          cpuPct: num(o, "cpu_pct"),
          memUsed: num(o, "mem_used"),
          memTotal: num(o, "mem_total"),
          diskUsed: num(o, "disk_used"),
          diskTotal: num(o, "disk_total"),
          lastSeen: num(o, "last_seen"),
        },
  );

/** Cast an accounts body. */
export const asAccounts = (
  body: unknown,
): ReadonlyArray<Account> =>
  rowsOf(body, (o) =>
    str(o, "label") === ""
      ? undefined
      : {
          provider: str(o, "provider"),
          label: str(o, "label"),
          email: str(o, "email"),
          plan: str(o, "plan"),
          status: str(o, "status"),
          assignedMember: str(
            o,
            "assigned_member",
          ),
        },
  );

/** Cast a sessions body. */
export const asSessions = (
  body: unknown,
): ReadonlyArray<Session> =>
  rowsOf(body, (o) =>
    str(o, "id") === "" || str(o, "member") === ""
      ? undefined
      : {
          member: str(o, "member"),
          id: str(o, "id"),
          cwd: str(o, "cwd"),
          status: str(o, "status"),
          lastMessage: str(o, "last_message"),
          ts: num(o, "ts"),
        },
  );
