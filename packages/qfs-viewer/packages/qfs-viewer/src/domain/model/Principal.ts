// Who is asking, and what they may do.
//
// DECLARED, NOT STORED. The mission's Goal rules a database out of the local
// surface — "no build step, no database, no central configuration" — so
// principals live in `qfs-viewer.config.json` beside the taxonomy. That is
// not a shortcut around plgg-auth: plgg-auth is an OIDC identity PROVIDER, and
// it brings plgg-sql and plgg-db-migration with it. Being an identity provider
// is a different product from reading one, and a knowledge browser that made
// you run a migration before it would show you a README would have lost the
// argument before it started.
//
// OPEN BY DEFAULT, and this is the decision worth defending. A config with no
// `principals` leaves every surface unauthenticated — which is exactly right
// for the product's headline case: `npx qfs-viewer` at your own repository
// root, on your own machine, reading your own files. Demanding a token there
// would be security theatre performed for an audience of one. Access control
// turns on when a repository DECLARES principals, which is the moment it stops
// being one developer's local tool.
import {
  type SoftStr,
  type Option,
  type Result,
  type InvalidError,
  invalidError,
  ok,
  err,
  some,
  none,
} from "plgg";

/**
 * What a principal may do. A closed set of two, ordered: an editor can do
 * everything a reader can.
 *
 * Not a permission bag. A bag invites `{read: true, edit: false, admin: ...}`
 * and then a question about which combination means what; two named roles
 * answer the only question this product actually has.
 */
export type Role = "reader" | "editor";

/**
 * Someone who may use this server — a person or a bot; the model does not
 * distinguish them, because the corpus does not care and an API key is an API
 * key.
 *
 * `key` is the bearer token presented as `Authorization: Bearer <key>`.
 */
export type Principal = Readonly<{
  name: SoftStr;
  key: SoftStr;
  role: Role;
}>;

/**
 * What a request is allowed to do.
 *
 * `Anonymous` is NOT "denied" — it is the no-principals-declared case, where
 * everyone may do everything. Modelling it as its own thing rather than as a
 * `None` principal keeps the two questions apart: "is this server access
 * controlled" and "who is this".
 */
export type Access =
  | Readonly<{ __tag: "Open" }>
  | Readonly<{
      __tag: "Granted";
      principal: Principal;
    }>
  | Readonly<{
      __tag: "Denied";
      reason: SoftStr;
    }>;

const isRecord = (
  v: unknown,
): v is Readonly<Record<string, unknown>> =>
  typeof v === "object" &&
  v !== null &&
  !Array.isArray(v);

const asRole = (
  value: unknown,
  at: number,
): Result<Role, InvalidError> =>
  value === "reader" || value === "editor"
    ? ok(value)
    : err(
        invalidError({
          message: `principals[${at}].role must be 'reader' or 'editor', got ${JSON.stringify(value)}`,
        }),
      );

/** Validate one declared principal. */
export const asPrincipal = (
  value: unknown,
  at: number,
): Result<Principal, InvalidError> => {
  if (!isRecord(value)) {
    return err(
      invalidError({
        message: `principals[${at}] must be an object`,
      }),
    );
  }
  const name: unknown = value["name"];
  if (typeof name !== "string" || name === "") {
    return err(
      invalidError({
        message: `principals[${at}].name must be a non-empty string`,
      }),
    );
  }
  const key: unknown = value["key"];
  if (typeof key !== "string") {
    return err(
      invalidError({
        message: `principals[${at}].key must be a string`,
      }),
    );
  }
  // A short key is worse than no key: it looks like access control while
  // being guessable, so the reader believes they are protected. Refuse it at
  // the boundary rather than letting someone find out later.
  if (key.length < 16) {
    return err(
      invalidError({
        message: `principals[${at}].key must be at least 16 characters — a short key is not access control, it is the appearance of it`,
      }),
    );
  }
  const role = asRole(value["role"], at);
  return role.__tag === "Err"
    ? err(role.content)
    : ok({ name, key, role: role.content });
};

// Constant-time-ish comparison. Not a defence against a serious timing attack
// — that needs `crypto.timingSafeEqual`, which is `node:crypto` and therefore
// a vendor, and this file is domain. It is the cheap half: compare every byte
// rather than bailing on the first mismatch, so the loop's duration does not
// advertise how much of the key was right.
const keyEquals = (
  a: SoftStr,
  b: SoftStr,
): boolean => {
  if (a.length !== b.length) {
    return false;
  }
  let diff = 0;
  for (let i = 0; i < a.length; i = i + 1) {
    diff =
      diff | (a.charCodeAt(i) ^ b.charCodeAt(i));
  }
  return diff === 0;
};

/**
 * Resolve a request's credential into an {@link Access}.
 *
 * No declared principals means {@link Access} `Open` — see the header. Once
 * there ARE principals, a missing or unknown key is `Denied`: a server that
 * declares who may use it does not also serve people who did not say.
 */
export const accessFor = (
  principals: ReadonlyArray<Principal>,
  presented: Option<SoftStr>,
): Access => {
  if (principals.length === 0) {
    return { __tag: "Open" };
  }
  if (presented.__tag === "None") {
    return {
      __tag: "Denied",
      reason:
        "this corpus declares principals; present one as `Authorization: Bearer <key>`",
    };
  }
  const found = principals.find((p) =>
    keyEquals(p.key, presented.content),
  );
  return found === undefined
    ? {
        __tag: "Denied",
        // Deliberately not "unknown key" vs "wrong key": both are the same
        // answer, and distinguishing them tells a prober which half to work on.
        reason: "not a principal of this corpus",
      }
    : { __tag: "Granted", principal: found };
};

/** Whether an {@link Access} may read the corpus. */
export const mayRead = (
  access: Access,
): boolean => access.__tag !== "Denied";

/**
 * Whether an {@link Access} may change a document.
 *
 * `Open` may edit: an undeclared corpus is the local developer's own tree, and
 * the editor is the mission's "editable in place". A reader may not — that is
 * the whole point of having two roles.
 */
export const mayEdit = (
  access: Access,
): boolean =>
  access.__tag === "Open" ||
  (access.__tag === "Granted" &&
    access.principal.role === "editor");

/** The bearer token in an `Authorization` header, if it carries one. */
export const bearerOf = (
  header: Option<SoftStr>,
): Option<SoftStr> => {
  if (header.__tag === "None") {
    return none();
  }
  const match = /^Bearer +(.+)$/i.exec(
    header.content.trim(),
  );
  const token = match?.[1];
  return token === undefined || token === ""
    ? none()
    : some(token);
};
