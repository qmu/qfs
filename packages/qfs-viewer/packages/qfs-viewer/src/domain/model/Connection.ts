// How this viewer reaches qfs — the connection as a value.
//
// The plan (qmu/strategy docs/plan.md) names three issuance forms for a
// qfs-query: ① a locally running qfs server, ② an on-demand command
// invocation per query, ③ a remote qfs on another machine. Which one a
// deployment uses is the DEPLOYMENT's business, so the choice is
// configuration, not code — and being able to choose at all is the viewer's
// half of user sovereignty (the freedom to pick your own infrastructure).
//
// The MVP implements ② and only SHAPES ① and ③: both are selectable and both
// answer every query with a typed "not implemented" error. That is
// deliberate rather than lazy — the seam is proven swappable by construction
// BEFORE a second form exists, so when one arrives it replaces one fold arm
// and no caller. A form that "does not exist yet" would instead be a
// hard-wired spawn that someone later has to un-wire.
import {
  type SoftStr,
  type Result,
  type InvalidError,
  invalidError,
  ok,
  err,
} from "plgg";

/**
 * Form ② as a type of its own: the binary is named, and that name is the
 * whole of the acquisition story (docs/adr/0009 — qfs is FOUND, never
 * bundled or fetched).
 */
export type SpawnConnection = Readonly<{
  __tag: "Spawn";
  bin: SoftStr;
}>;

/**
 * The connection, as a closed union — one variant per issuance form.
 *
 * `Spawn` (form ②) is the default: each query is one invocation of the qfs
 * binary, so `npx qfs-viewer` needs no daemon running first. `LocalServer`
 * (①) and `Remote` (③) carry the address they will someday dial; today they
 * are skeletons the vendor adapter answers with a typed error.
 */
export type QfsConnection =
  | SpawnConnection
  | Readonly<{
      __tag: "LocalServer";
      url: SoftStr;
    }>
  | Readonly<{ __tag: "Remote"; url: SoftStr }>;

/**
 * The connection a repository with no `qfs` config gets: spawn the `qfs` on
 * PATH per query. The zero-config default is what demo leg 1 requires.
 */
export const defaultConnection: QfsConnection = {
  __tag: "Spawn",
  bin: "qfs",
};

/**
 * How to get a qfs.
 *
 * The MVP FINDS the binary on PATH; it does not bundle one and does not
 * fetch one (docs/adr/0009 — the alternatives, esbuild-style per-platform
 * npm binary packages or a postinstall download, are real and were
 * declined). So the product cannot install qfs, and the one thing it owes
 * someone without it is the exact command that does — here, in the domain,
 * because "how a user acquires the substrate" is a decision, not a detail
 * of the spawn adapter.
 *
 * Kept verbatim in step with qfs's own README (`packages/qfs/install.sh`,
 * sha256-verified, installs to `~/.local/bin`). A wrong command is worse
 * than none: it sends someone to a 404 with our name on it.
 */
export const QFS_INSTALL_COMMAND: SoftStr =
  "curl -fsSL https://raw.githubusercontent.com/qmu/qfs/main/packages/qfs/install.sh | sh";

/**
 * What to say when the configured qfs cannot be run.
 *
 * Three facts, in the order someone needs them: WHAT is missing (this exact
 * binary name, and the OS's own reason — an ENOENT and a permission bite are
 * different problems), HOW to get it, and WHAT still works meanwhile. The
 * last one is not a courtesy: markdown browsing needs no qfs at all, so a
 * message that read like a fatal error would send a reader off to install
 * something before opening the viewer they already have running.
 *
 * Pure, and here rather than in the adapter, so the words are testable
 * without a subprocess and are the SAME words at boot (the probe) and at
 * request time (a failed query) — one missing binary, one explanation.
 */
export const unreachableAdvice = (
  connection: SpawnConnection,
  reason: SoftStr,
): SoftStr =>
  [
    `qfs could not be run: ${reason}.`,
    `This build reaches qfs by spawning "${connection.bin}" per query, and expects to find it on PATH — it neither bundles nor downloads one.`,
    `Install it with: ${QFS_INSTALL_COMMAND}`,
    `Already have one elsewhere? Name it: {"qfs": {"form": "spawn", "bin": "/path/to/qfs"}} in qfs-viewer.config.json.`,
    "Markdown browsing does not need qfs — only qfs paths do.",
  ].join(" ");

const isRecord = (
  v: unknown,
): v is Readonly<Record<string, unknown>> =>
  typeof v === "object" &&
  v !== null &&
  !Array.isArray(v);

/**
 * Validate the config's `qfs` key into a {@link QfsConnection}.
 *
 * REJECTS rather than repairs, matching `asConfig`'s stance: a config is a
 * thing a person wrote on purpose, and a misspelled form silently falling
 * back to spawn would answer their question by ignoring it.
 */
export const asQfsConnection = (
  value: unknown,
): Result<QfsConnection, InvalidError> => {
  if (!isRecord(value)) {
    return err(
      invalidError({
        message: "qfs must be an object",
      }),
    );
  }
  const form: unknown = value["form"];
  if (form === "spawn") {
    const bin: unknown = value["bin"];
    if (
      bin !== undefined &&
      (typeof bin !== "string" || bin === "")
    ) {
      return err(
        invalidError({
          message:
            "qfs.bin must be a non-empty string",
        }),
      );
    }
    return ok({
      __tag: "Spawn",
      bin: bin === undefined ? "qfs" : bin,
    });
  }
  if (
    form === "local-server" ||
    form === "remote"
  ) {
    const url: unknown = value["url"];
    if (typeof url !== "string" || url === "") {
      return err(
        invalidError({
          message: `qfs.url is required for the ${form} form`,
        }),
      );
    }
    return ok(
      form === "local-server"
        ? { __tag: "LocalServer", url }
        : { __tag: "Remote", url },
    );
  }
  return err(
    invalidError({
      message: `qfs.form must be one of "spawn", "local-server", "remote", got ${JSON.stringify(form)}`,
    }),
  );
};
