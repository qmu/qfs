#!/usr/bin/env node
// qfs-viewer launcher: relocate out of node_modules if
// needed, then hand off to the TypeScript CLI (the runtime
// strips types on load). Kept as a plain `.mjs` because it
// runs at the very process entry.
//
// There is no resolver hook here any more. This used to
// `register("./hook.mjs")` to teach Node the `qfs-viewer/*`
// self-alias, which worked on Node and nowhere else: deno's
// `register` is a silent no-op stub -- it accepts a hook path
// that does not exist without throwing -- so deno installed no
// resolver, fell through to `exports`, and could not run the
// product. The alias is `#qfs-viewer/*` in package.json's
// `imports` now, which every runtime resolves natively, so the
// hook has nothing left to do.
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { relocateOutOfNodeModules } from "./relocate.mjs";

// Node 24 refuses to strip types from `.ts` under
// `node_modules`. When this tool is installed from the
// registry (which is the whole point of `npx qfs-viewer`),
// relocate a copy OUTSIDE `node_modules` and re-exec there; a
// no-op on a `file:` link.
relocateOutOfNodeModules(
  import.meta.url,
  "qfs-viewer.mjs",
);

const here = dirname(
  fileURLToPath(import.meta.url),
);
const cli = join(
  here,
  "..",
  "src",
  "entrypoints",
  "cli.ts",
);

await import(cli);
