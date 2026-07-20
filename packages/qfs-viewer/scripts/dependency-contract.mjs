// The dependency-contract gate (see scripts/gate-dependencies.sh).
//
// Fails when a package's RUNTIME `dependencies` names anything that is not
// plgg-family. devDependencies are out of scope by design.
//
// plggmatic counts as plgg-family since ADR 0002's second amendment
// (2026-07-17): the ported engine at packages/plggmatic is this package's
// UI engine, so the exclusion that once caught it before the prefix rule
// is gone — deliberately, with the amendment recording why
// (docs/adr/0002-plggmatic-is-a-reference-not-a-dependency.md).
//
// Zero dependencies: node built-ins only.
//
// Usage:
//   node scripts/dependency-contract.mjs [--self-test]

import {
  readFileSync,
  readdirSync,
  existsSync,
} from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(HERE, "..");
const PACKAGES = join(REPO_ROOT, "packages");

const isPlggFamily = (name) => name.startsWith("plgg");

// Classify one runtime dependency name.
//   "ok"       — plgg-family (plggmatic included, per ADR 0002's amendment)
//   "foreign"  — anything else
export const classifyDependency = (name) =>
  isPlggFamily(name) ? "ok" : "foreign";

const auditPackages = () => {
  const pkgs = readdirSync(PACKAGES).filter((n) =>
    existsSync(join(PACKAGES, n, "package.json")),
  );
  return pkgs.map((pkg) => {
    const manifest = JSON.parse(
      readFileSync(
        join(PACKAGES, pkg, "package.json"),
        "utf8",
      ),
    );
    const deps = Object.keys(
      manifest.dependencies ?? {},
    );
    return {
      pkg,
      offenders: deps
        .map((d) => ({
          name: d,
          verdict: classifyDependency(d),
        }))
        .filter((d) => d.verdict !== "ok"),
    };
  });
};

const gate = (audit) =>
  audit.flatMap(({ pkg, offenders }) =>
    offenders.map(
      (o) =>
        `${pkg}: depends on "${o.name}" — not a plgg-family package; this repo takes no other runtime dependency (docs/adr/0001-npm-only-plgg-family-contract.md).`,
    ),
  );

const selfTest = () => {
  const cases = [
    [
      "plgg-family runtime deps are ok",
      classifyDependency("plgg") === "ok" &&
        classifyDependency("plgg-view") === "ok" &&
        classifyDependency("plgg-md") === "ok" &&
        classifyDependency("plggpress") === "ok" &&
        classifyDependency("plgg-cms") === "ok",
    ],
    [
      "plggmatic is accepted (ADR 0002 amendment, 2026-07-17)",
      classifyDependency("plggmatic") === "ok",
    ],
    [
      "a third party is foreign",
      classifyDependency("react") === "foreign" &&
        classifyDependency("chokidar") === "foreign" &&
        classifyDependency("typescript") === "foreign",
    ],
    [
      "gate is RED on a foreign runtime dep",
      gate([
        {
          pkg: "fixture",
          offenders: [
            { name: "react", verdict: "foreign" },
          ],
        },
      ]).length > 0,
    ],
    [
      "gate is GREEN on a clean package",
      gate([{ pkg: "fixture", offenders: [] }])
        .length === 0,
    ],
  ];
  let ok = true;
  for (const [name, pass] of cases) {
    console.log(`  ${pass ? "PASS" : "FAIL"}  ${name}`);
    if (!pass) {
      ok = false;
    }
  }
  return ok;
};

const main = () => {
  if (process.argv[2] === "--self-test") {
    console.log(
      "=== dependency-contract gate self-test ===",
    );
    process.exit(selfTest() ? 0 : 1);
  }
  const audit = auditPackages();
  const failures = gate(audit);
  if (failures.length > 0) {
    console.error(
      "=== dependency-contract gate: FAILED ===",
    );
    for (const f of failures) {
      console.error(`  ${f}`);
    }
    process.exit(1);
  }
  console.log(
    `  ${audit.length} package(s) audited — every runtime dependency is plgg-family.`,
  );
  process.exit(0);
};

main();
