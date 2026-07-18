// Shared launcher preamble for the run-from-source plgg CLIs.
//
// These tools run their `src/**/*.ts` entry directly and rely on Node stripping
// types on load. Node refuses to strip types for `.ts` files under
// `node_modules`, so a registry-installed tool cannot run in place. This helper
// copies the package to a per-(version, install-location) dir OUTSIDE
// `node_modules` (with the tool's own deps reachable via a `node_modules`
// symlink) and re-execs the copy there. On a monorepo `file:` link the package
// realpath is already outside `node_modules`, so it is a no-op.
//
// Plain `.mjs`, Node built-ins only — it runs at process entry, before any
// resolver hook is registered.
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  realpathSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import {
  dirname,
  join,
  sep,
} from "node:path";
import { fileURLToPath } from "node:url";

const isUnderNodeModules = (p) =>
  p.split(sep).includes("node_modules");

// Relocate the calling launcher's package out of node_modules and re-exec, or
// return (no-op) when already outside it. Exits the process on re-exec.
export const relocateOutOfNodeModules = (
  launcherUrl,
  launcherBinName,
) => {
  const binDir = dirname(
    fileURLToPath(launcherUrl),
  );
  const pkgRoot = realpathSync(
    join(binDir, ".."),
  );
  if (!isUnderNodeModules(pkgRoot)) {
    return;
  }

  const pkg = JSON.parse(
    readFileSync(
      join(pkgRoot, "package.json"),
      "utf8",
    ),
  );
  // The node_modules that CONTAINS this package holds its deps (npm hoists
  // there). Key the relocation dir by that location too — two installs of the
  // same version from different trees (e.g. a publish smoke's scratch install
  // and a real consumer) must NOT share a copy, or one inherits the other's
  // stale (possibly deleted) deps symlink.
  const depsNodeModules = dirname(pkgRoot);
  const tag = createHash("sha1")
    .update(depsNodeModules)
    .digest("hex")
    .slice(0, 12);
  const dest = join(
    tmpdir(),
    `plgg-relocate-${pkg.name}-${pkg.version}-${tag}`,
  );
  const ready = join(
    dest,
    ".plgg-relocate-ready",
  );
  const link = join(dest, "node_modules");

  if (!existsSync(ready)) {
    rmSync(dest, {
      recursive: true,
      force: true,
    });
    mkdirSync(dest, { recursive: true });
    for (const dir of ["src", "bin"]) {
      const from = join(pkgRoot, dir);
      if (existsSync(from)) {
        cpSync(from, join(dest, dir), {
          recursive: true,
        });
      }
    }
    // `package.json` travels with the copy, and it is load-bearing rather
    // than tidy: it carries the `#qfs-viewer/*` -> `./src/*.ts` `imports`
    // map, which is how EVERY runtime resolves this package's inward imports.
    // Without it here, the first inward import fails ("Cannot find module
    // '#qfs-viewer/entrypoints/serve'") -- a `#` specifier is resolved
    // against the nearest package.json, so the copy needs its own.
    //
    // `tsconfig.json` travels too, for typecheck-in-place and editors. It no
    // longer carries the alias: that lived here as `paths` as well, and the
    // duplicate is exactly what let deno break unnoticed.
    for (const file of [
      "package.json",
      "tsconfig.json",
    ]) {
      const from = join(pkgRoot, file);
      if (existsSync(from)) {
        cpSync(from, join(dest, file));
      }
    }
    writeFileSync(ready, depsNodeModules + "\n");
  }
  // (Re)create the deps symlink every run so it always points at the CURRENT
  // node_modules: a cached copy from a prior run may hold a symlink to a tree
  // that has since been removed (a publish smoke's scratch install) or moved.
  rmSync(link, { force: true });
  try {
    symlinkSync(depsNodeModules, link, "dir");
  } catch {
    // A concurrent run created it first; its target is identical.
  }

  const child = spawnSync(
    process.execPath,
    [
      join(dest, "bin", launcherBinName),
      ...process.argv.slice(2),
    ],
    { stdio: "inherit" },
  );
  process.exit(child.status ?? 1);
};
