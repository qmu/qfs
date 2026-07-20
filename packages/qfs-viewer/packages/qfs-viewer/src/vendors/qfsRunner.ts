// Reaching qfs, behind the domain's `ResourceRunner` seam.
//
// The fourth and last `node:*` adapter. The plan names three issuance forms
// for a qfs-query — ① a locally running qfs server, ② an on-demand command
// invocation per query, ③ a remote qfs — and which one a deployment uses is
// configuration (`domain/model/Connection.ts`), not code. This adapter folds
// that closed union: form ② is implemented (it shells out to the qfs binary;
// `qfs --json run|describe` IS its published interface, there is no library
// to link and ADR 0001 would forbid one anyway unless it were plgg-family),
// and forms ① and ③ are SKELETONS that answer every query with a typed
// error naming themselves. The skeleton is the point: the seam is proven
// swappable by construction before a second form exists.
//
// What it does NOT do: interpret. The statement is the caller's, verbatim;
// the shape of the answer is `domain/model/Resource.ts`'s and
// `domain/model/Describe.ts`'s to judge. This file finds the bytes.
import { execFileSync } from "node:child_process";
import {
  type SoftStr,
  type Result,
  type InvalidError,
  invalidError,
  ok,
  err,
} from "plgg";
import { type ResourceRunner } from "#qfs-viewer/domain/model/Scan";
import {
  type QfsConnection,
  type SpawnConnection,
  defaultConnection,
  unreachableAdvice,
} from "#qfs-viewer/domain/model/Connection";

/**
 * How long a resource may take before the page gives up on it.
 *
 * Finite by policy (`workaholic:implementation` / `observability`: finite
 * timeouts at every IO edge). Measured 2026-07-15 at 7.2s for a trivial
 * local select (qfs registered drivers and probed mounts per invocation);
 * re-measured 2026-07-17 at ~50ms on qfs 0.0.71 — the upstream startup cost
 * is gone. The budget stays at 20s for the sake of real network mounts
 * behind a query, not local startup.
 */
const TIMEOUT_MS = 20000;

/**
 * How long the boot probe waits for `qfs --version`.
 *
 * Far tighter than a query's budget, and deliberately: the probe runs on the
 * startup path, where the cost of waiting is the product not starting. A qfs
 * that cannot say its own version in two seconds is not one this viewer
 * should hold the boot for — the answer would be "unreachable" either way,
 * and the browsing that needs no qfs is being delayed meanwhile.
 */
const PROBE_TIMEOUT_MS = 2000;

/**
 * One spawned invocation: `<bin> --json <run|describe> <argument>`.
 *
 * `execFileSync`, never `exec`: the statement goes to qfs as ONE argv
 * element and never touches a shell, so a query containing a quote or a `;`
 * is a query and not an injection.
 *
 * qfs writes its answer to stdout and its logs to stderr, and it exits
 * non-zero on some failures while reporting others as a JSON `error` object
 * on stdout — so a throw here is not necessarily a failure to report, and
 * stdout is read either way.
 */
const spawnQuery = (
  connection: SpawnConnection,
  args: ReadonlyArray<SoftStr>,
): Result<unknown, InvalidError> => {
  let out: string;
  try {
    out = execFileSync(
      connection.bin,
      [...args],
      {
        encoding: "utf8",
        timeout: TIMEOUT_MS,
        // stderr is qfs's log stream and carries warnings (a Cloudflare
        // mount it could not reach, say) that are not this query's problem.
        // Inheriting it would splice them into the server's own log; piping
        // it keeps them out of both.
        stdio: ["ignore", "pipe", "pipe"],
      },
    );
  } catch (e: unknown) {
    // A non-zero exit STILL carries qfs's structured error on stdout — that
    // is how it reports a parse error or an unknown mount — so the output is
    // worth more than the exception. Only a genuinely empty answer is an
    // adapter-level failure.
    const stdout: unknown =
      e !== null &&
      typeof e === "object" &&
      "stdout" in e
        ? e.stdout
        : undefined;
    if (
      typeof stdout === "string" &&
      stdout.trim() !== ""
    ) {
      out = stdout;
    } else {
      // NOT a bare "qfs could not be run: ENOENT". A query that fails
      // because there is no qfs on this machine is the one error whose
      // remedy the reader cannot guess, and the words for it are the
      // domain's (`unreachableAdvice`) — the same ones the boot probe
      // prints, so learning it twice teaches the same thing once.
      return err(
        invalidError({
          message: unreachableAdvice(
            connection,
            e instanceof Error
              ? e.message
              : String(e),
          ),
        }),
      );
    }
  }
  // qfs prints one JSON object; its logs go to stderr, which is piped away.
  // A line that is not JSON is qfs breaking its own contract, and saying so
  // beats a bare SyntaxError from a JSON parser three frames up.
  try {
    return ok(JSON.parse(out));
  } catch {
    return err(
      invalidError({
        message: `qfs printed something that is not JSON: ${out.slice(0, 120)}`,
      }),
    );
  }
};

// The skeleton forms answer, they do not vanish: a deployment that selected
// ① or ③ gets told exactly what is not there yet and what works today,
// instead of a connection refused three layers down.
const notImplementedMessage = (
  form: SoftStr,
  url: SoftStr,
): SoftStr =>
  `the ${form} issuance form (${url}) is not implemented yet — this build reaches qfs by on-demand spawn only; set {"qfs": {"form": "spawn"}} or remove the qfs block from qfs-viewer.config.json`;

// Which form a non-spawn connection named, in the config's own spelling —
// so the message quotes back what was written rather than the tag.
const skeletonForm = (
  connection: Exclude<
    QfsConnection,
    SpawnConnection
  >,
): SoftStr =>
  connection.__tag === "LocalServer"
    ? "local-server"
    : "remote";

const notImplemented = (
  form: SoftStr,
  url: SoftStr,
): ResourceRunner => {
  const answer = (): Result<
    unknown,
    InvalidError
  > =>
    err(
      invalidError({
        message: notImplementedMessage(form, url),
      }),
    );
  return { run: answer, describe: answer };
};

/**
 * Is the configured qfs actually there? — asked ONCE, at boot.
 *
 * `<bin> --version` and nothing else: the cheapest question qfs answers
 * (~50ms on 0.0.71), pure on its side, and it needs no mount, no
 * credential, and no vault unlock — so a machine with a locked vault
 * reports its qfs as present, which it is.
 *
 * The Ok carries qfs's own version line, because the log is the only place
 * anyone can later read WHICH qfs this viewer was talking to. The Err
 * carries {@link unreachableAdvice} — words for a person, not a code —
 * because the caller's whole job is to print it.
 *
 * A non-spawn form is not probed: ① and ③ are skeletons that dial nothing,
 * so "reachable" is not yet a question about them, and answering it would
 * be inventing an interface the fold below still refuses. They report the
 * same not-implemented message they answer every query with.
 */
export const probeQfs = (
  connection: QfsConnection = defaultConnection,
): Result<SoftStr, InvalidError> => {
  if (connection.__tag !== "Spawn") {
    return err(
      invalidError({
        message: notImplementedMessage(
          skeletonForm(connection),
          connection.url,
        ),
      }),
    );
  }
  try {
    return ok(
      execFileSync(
        connection.bin,
        ["--version"],
        {
          encoding: "utf8",
          timeout: PROBE_TIMEOUT_MS,
          stdio: ["ignore", "pipe", "pipe"],
        },
      )
        .split("\n")[0]
        ?.trim() ?? "",
    );
  } catch (e: unknown) {
    return err(
      invalidError({
        message: unreachableAdvice(
          connection,
          e instanceof Error
            ? e.message
            : String(e),
        ),
      }),
    );
  }
};

/**
 * A {@link ResourceRunner} over the configured {@link QfsConnection}.
 *
 * The fold is the whole adapter: `Spawn` reaches the binary per query;
 * `LocalServer` and `Remote` are the shaped-but-skeleton forms. When one of
 * them becomes real it replaces its arm here and nothing above this file
 * moves — that is the acceptance item's "swappable by configuration".
 */
export const qfsRunner = (
  connection: QfsConnection = defaultConnection,
): ResourceRunner => {
  if (connection.__tag !== "Spawn") {
    return notImplemented(
      skeletonForm(connection),
      connection.url,
    );
  }
  return {
    run: (statement: SoftStr) =>
      spawnQuery(connection, [
        "--json",
        "run",
        statement,
      ]),
    describe: (path: SoftStr) =>
      spawnQuery(connection, [
        "--json",
        "describe",
        path,
      ]),
  };
};
