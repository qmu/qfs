// Reading `qfs-viewer.config.json` off the real filesystem.
//
// The third `node:fs` adapter, and the last: read the corpus, write one
// document, read one config. Each is its own seam because each is its own
// authority (see `domain/model/Editor.ts`).
//
// The rules — what a config MAY say, and what is a typo — live in
// `domain/model/Config.ts`. This file finds the bytes and parses JSON; it does
// not decide.
import { readFileSync } from "node:fs";
import {
  type SoftStr,
  type Result,
  type InvalidError,
  invalidError,
  ok,
  err,
} from "plgg";
import {
  type Config,
  asConfig,
  defaultConfig,
} from "#qfs-viewer/domain/model/Config";

/** Where a repository declares itself. */
export const CONFIG_FILE =
  "qfs-viewer.config.json";

/**
 * Load `qfs-viewer.config.json` from `cwd`, or the default.
 *
 * ABSENT IS NOT AN ERROR — it is the normal case, and the product's premise:
 * `npx qfs-viewer` at any repository root, no configuration required. A
 * missing file returns {@link defaultConfig}, which is exactly the behaviour
 * every repository had before configs existed.
 *
 * MALFORMED IS AN ERROR, and a loud one. A config is a thing a person wrote on
 * purpose, so a trailing comma or a mistyped key is a question they want
 * answered — falling back to the default would start the server with their
 * intent silently discarded, and they would find out by noticing the facets
 * were wrong. The caller decides what to do with the failure; this reports it.
 *
 * The `readFileSync` throw is caught and turned into a value, but ONLY to
 * distinguish "no file" from "a file I could not read": an unreadable config
 * that exists (a permission bite) is a real failure and must not be mistaken
 * for an absent one.
 */
export const loadConfig = (
  cwd: SoftStr,
): Result<Config, InvalidError> => {
  const path = `${cwd}/${CONFIG_FILE}`;
  let raw: string;
  try {
    raw = readFileSync(path, "utf8");
  } catch (e: unknown) {
    return e !== null &&
      typeof e === "object" &&
      "code" in e &&
      e.code === "ENOENT"
      ? ok(defaultConfig)
      : err(
          invalidError({
            message: `${CONFIG_FILE} exists but could not be read: ${e instanceof Error ? e.message : String(e)}`,
          }),
        );
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch (e: unknown) {
    return err(
      invalidError({
        message: `${CONFIG_FILE} is not valid JSON: ${e instanceof Error ? e.message : String(e)}`,
      }),
    );
  }
  return asConfig(parsed);
};
