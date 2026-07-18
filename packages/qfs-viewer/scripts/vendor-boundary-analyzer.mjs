// Vendor-boundary analyzer — ported from the plgg monorepo
// (plgg/scripts/vendor-boundary-analyzer.mjs, ticket 20260704185201).
//
// Enforces the vendor-isolation policy as a machine-checked rule: third-party
// imports (`node:*`, the tsc compiler API, any bare non-plgg specifier — this
// repo has zero third-party npm RUNTIME deps) may appear ONLY under a
// package's `src/vendors/**` or `src/entrypoints/**` (the anti-corruption
// boundary and the thin program checkpoints). plgg-family packages (`plgg`,
// `plgg-*`, `plggmatic*`, `plggpress*`, and self-aliases) are domain
// vocabulary, importable anywhere; relative imports are in-package.
//
// A package with a violation must appear in the exemption list; an exempted
// package that is actually clean is a STALE exemption (also a failure).
// `qfs-viewer` passes unexempted from day one.
//
// Zero new dependencies: imports the already-present `typescript` package (a
// devDependency of each package) via createRequire and uses its lightweight
// `preProcessFile` import scanner — no full program, no config.
//
// Usage:
//   node scripts/vendor-boundary-analyzer.mjs [--audit] [--self-test]
//     (no flag)   — gate mode: exit 1 on any violation / stale exemption
//     --audit     — print the full per-package audit table, always exit 0
//     --self-test — run the red/green logic proof, exit 1 if any case fails

import {
  readFileSync,
  readdirSync,
  statSync,
  existsSync,
} from "node:fs";
import { join, relative, dirname, sep } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { createRequire } from "node:module";

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(HERE, "..");
const PACKAGES = join(REPO_ROOT, "packages");
const EXEMPTIONS_FILE = join(
  HERE,
  "vendor-boundary-exemptions.txt",
);

// The tsc compiler API. Unlike the plgg monorepo (which resolves it from
// plgg-bundle's node_modules, the one place it is installed), every package
// here carries `typescript` as its own devDependency — so resolve from the
// first package that has it installed.
const resolveTypescript = () => {
  const candidates = readdirSync(PACKAGES).filter((n) =>
    existsSync(
      join(PACKAGES, n, "node_modules", "typescript"),
    ),
  );
  const host = candidates[0];
  if (host === undefined) {
    console.error(
      "vendor-boundary: `typescript` is not installed in any package — run ./scripts/npm-install.sh first.",
    );
    process.exit(1);
  }
  const requireFrom = createRequire(
    pathToFileURL(
      join(PACKAGES, host, "package.json"),
    ),
  );
  return requireFrom("typescript");
};

const ts = resolveTypescript();

// Classify an import specifier relative to the importing package.
//   "relative" — in-package (`./x`, `../y`)
//   "plgg"     — plgg-family domain vocabulary or a self-alias (starts "plgg")
//   "forbidden"— node: builtin, the tsc API, or any other bare third-party
//
// `#...` is a subpath import: the spec resolves it against the importing
// package's own `imports` map and nowhere else, so it is in-package BY
// CONSTRUCTION. That is why this tests the `#` sigil rather than the name
// `qfs-viewer` as it used to: a bare name is a claim (a third-party
// package could be called `qfs-viewer-anything` and get waved through),
// while a `#` specifier cannot leave the package even in principle.
const classify = (spec) =>
  spec.startsWith(".")
    ? "relative"
    : spec.startsWith("#")
      ? "relative"
      : spec.startsWith("plgg")
        ? "plgg"
        : "forbidden";

// A src-relative posix path is a boundary location (may import third-party)
// when it sits under `vendors/` or `entrypoints/`.
const isBoundaryLocation = (srcRelPath) => {
  const p = srcRelPath.split(sep).join("/");
  return (
    p.startsWith("vendors/") ||
    p.startsWith("entrypoints/")
  );
};

// Test code is analyzed separately from the production boundary. The gate
// governs PRODUCTION code structure; test files legitimately import real
// vendors under the "test against the real engine" practice (a temp-dir
// `node:fs`), which is the anti-corruption layer's tests connecting to the
// vendor — NOT domain purity.
const isTestCode = (srcRelPath) => {
  const p = srcRelPath.split(sep).join("/");
  return (
    p.endsWith(".spec.ts") ||
    p.endsWith(".spec.tsx") ||
    p.startsWith("testkit/") ||
    p.includes("/testkit/")
  );
};

// Every `.ts`/`.tsx` file under a directory (recursive).
const walkTs = (dir) => {
  const out = [];
  const visit = (d) => {
    for (const name of readdirSync(d)) {
      const full = join(d, name);
      const st = statSync(full);
      if (st.isDirectory()) {
        visit(full);
      } else if (
        name.endsWith(".ts") ||
        name.endsWith(".tsx")
      ) {
        out.push(full);
      }
    }
  };
  visit(dir);
  return out;
};

// Scan one file for forbidden imports outside the boundary locations.
const scanFile = (pkgSrc, file) => {
  const text = readFileSync(file, "utf8");
  const info = ts.preProcessFile(text, true, true);
  const srcRel = relative(pkgSrc, file);
  if (isBoundaryLocation(srcRel) || isTestCode(srcRel)) {
    return [];
  }
  const violations = [];
  for (const imp of info.importedFiles) {
    if (classify(imp.fileName) === "forbidden") {
      const line = text
        .slice(0, imp.pos)
        .split("\n").length;
      violations.push({
        file: relative(REPO_ROOT, file),
        line,
        spec: imp.fileName,
      });
    }
  }
  return violations;
};

// The per-package audit: violations + whether a src/domain/ layout exists.
const auditPackages = () => {
  const pkgs = readdirSync(PACKAGES).filter((n) => {
    const s = join(PACKAGES, n, "src");
    return existsSync(s) && statSync(s).isDirectory();
  });
  return pkgs.map((pkg) => {
    const src = join(PACKAGES, pkg, "src");
    const violations = walkTs(src).flatMap((f) =>
      scanFile(src, f),
    );
    return {
      pkg,
      hasDomainLayout: existsSync(join(src, "domain")),
      violations,
    };
  });
};

const readExemptions = () => {
  if (!existsSync(EXEMPTIONS_FILE)) {
    return new Set();
  }
  return new Set(
    readFileSync(EXEMPTIONS_FILE, "utf8")
      .split("\n")
      .map((l) => l.replace(/#.*$/, "").trim())
      .filter((l) => l.length > 0),
  );
};

// Gate: fail on an unexempted package with violations, or an exempted package
// that is actually clean (stale).
const gate = (audit, exemptions) => {
  const failures = [];
  for (const { pkg, violations } of audit) {
    const exempted = exemptions.has(pkg);
    if (violations.length > 0 && !exempted) {
      failures.push(
        `${pkg}: ${violations.length} boundary violation(s) — third-party import outside vendors/entrypoints:`,
      );
      for (const v of violations) {
        failures.push(
          `    ${v.file}:${v.line}  imports "${v.spec}"`,
        );
      }
    } else if (violations.length === 0 && exempted) {
      failures.push(
        `${pkg}: STALE exemption — the package is clean; remove it from vendor-boundary-exemptions.txt.`,
      );
    }
  }
  const known = new Set(audit.map((a) => a.pkg));
  for (const e of [...exemptions]) {
    if (!known.has(e)) {
      failures.push(
        `exemption "${e}" names no package under packages/ — remove the stale line.`,
      );
    }
  }
  return { failures };
};

// ── self-test: prove the gate logic red on a violation + a stale exemption,
// green on a clean unexempted package — without mutating the real tree. ──
const selfTest = () => {
  const cases = [];
  cases.push([
    "domain node: import is a violation",
    classify("node:fs") === "forbidden" &&
      !isBoundaryLocation("domain/usecase/x.ts"),
  ]);
  cases.push([
    "node: import under vendors/ is allowed",
    isBoundaryLocation("vendors/fs.ts"),
  ]);
  cases.push([
    "node: import under entrypoints/ is allowed",
    isBoundaryLocation("entrypoints/cli.ts"),
  ]);
  cases.push([
    "domain spec + testkit are test code (excluded)",
    isTestCode("domain/model/x.spec.ts") &&
      isTestCode("testkit/tree.ts") &&
      !isTestCode("domain/usecase/x.ts"),
  ]);
  cases.push([
    "plgg-family is allowed anywhere",
    classify("plgg") === "plgg" &&
      classify("plgg-view") === "plgg" &&
      classify("plgg-md") === "plgg" &&
      classify("plggpress/framework") === "plgg",
  ]);
  cases.push([
    "the `#` self-alias is in-package",
    classify(
      "#qfs-viewer/domain/model/Vocabulary",
    ) === "relative",
  ]);
  cases.push([
    "a bare name is NOT waved through on its prefix",
    classify("qfs-viewer-sdk") === "forbidden" &&
      classify("qfs-viewer") === "forbidden",
  ]);
  cases.push([
    "relative is allowed anywhere",
    classify("./x") === "relative" &&
      classify("../y/z") === "relative",
  ]);
  cases.push([
    "typescript + bare third-party are forbidden",
    classify("typescript") === "forbidden" &&
      classify("fs") === "forbidden" &&
      classify("some-sdk") === "forbidden",
  ]);
  const red = gate(
    [
      {
        pkg: "fixture",
        hasDomainLayout: true,
        violations: [
          { file: "x", line: 1, spec: "node:fs" },
        ],
      },
    ],
    new Set(),
  );
  cases.push([
    "gate is RED on an unexempted violation",
    red.failures.length > 0,
  ]);
  const stale = gate(
    [
      {
        pkg: "fixture",
        hasDomainLayout: true,
        violations: [],
      },
    ],
    new Set(["fixture"]),
  );
  cases.push([
    "gate is RED on a stale exemption",
    stale.failures.some((f) => f.includes("STALE")),
  ]);
  const green = gate(
    [
      {
        pkg: "fixture",
        hasDomainLayout: true,
        violations: [],
      },
    ],
    new Set(),
  );
  cases.push([
    "gate is GREEN on a clean unexempted package",
    green.failures.length === 0,
  ]);
  const exemptedDirty = gate(
    [
      {
        pkg: "fixture",
        hasDomainLayout: false,
        violations: [
          { file: "x", line: 1, spec: "node:fs" },
        ],
      },
    ],
    new Set(["fixture"]),
  );
  cases.push([
    "gate is GREEN on an exempted package with violations",
    exemptedDirty.failures.length === 0,
  ]);

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
  const mode = process.argv[2] ?? "--gate";
  if (mode === "--self-test") {
    console.log(
      "=== vendor-boundary gate self-test ===",
    );
    process.exit(selfTest() ? 0 : 1);
  }
  const audit = auditPackages();
  if (mode === "--audit") {
    for (const {
      pkg,
      hasDomainLayout,
      violations,
    } of audit) {
      const layout = hasDomainLayout
        ? "domain/"
        : "legacy";
      const specs = [
        ...new Set(violations.map((v) => v.spec)),
      ];
      console.log(
        `  ${pkg.padEnd(20)} ${layout.padEnd(8)} ${violations.length} violation(s)${specs.length > 0 ? ` — ${specs.join(", ")}` : ""}`,
      );
    }
    process.exit(0);
  }
  const { failures } = gate(audit, readExemptions());
  if (failures.length > 0) {
    console.error(
      "=== vendor-boundary gate: FAILED ===",
    );
    for (const f of failures) {
      console.error(`  ${f}`);
    }
    process.exit(1);
  }
  console.log(
    `  ${audit.length} package(s) audited — no boundary violations, no stale exemptions.`,
  );
  process.exit(0);
};

main();
