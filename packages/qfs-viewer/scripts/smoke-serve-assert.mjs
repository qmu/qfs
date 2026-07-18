// The serve half of the npx smoke: drive a RUNNING packed bin over HTTP.
//
// Split out of `smoke-npx.sh` because this is where the assertions have to
// read a body and a log, and a shell doing that in `grep` says "no match"
// where this can say which of six claims failed and what it saw instead.
//
// It is a CLIENT: it never starts or stops anything. `smoke-npx.sh` owns the
// process (and killing it on every exit path); this asks the server questions
// and judges the answers.
//
// Node built-ins only, and no dependency: it runs from `scripts/`, outside
// every package, exactly like `dependency-contract.mjs` and
// `vendor-boundary-analyzer.mjs`. `fetch` is global on Node 24 — which the
// smoke already requires, since it reads the expected version with `node -p`.
//
//   node scripts/smoke-serve-assert.mjs <port> <serve-log> <doc-path>
import { readFileSync } from "node:fs";

const [port, logPath, docPath] =
  process.argv.slice(2);

const fail = (claim, saw) => {
  process.stderr.write(
    `  FAIL: ${claim}\n${
      saw === undefined
        ? ""
        : `${String(saw)
            .split("\n")
            .map((l) => `      ${l}`)
            .join("\n")}\n`
    }`,
  );
  process.exit(1);
};

const base = `http://127.0.0.1:${port}`;

// How long the server may take to answer at all. It scans the tree before it
// listens, and the scratch corpus is three files — but a loaded CI box still
// has to start a runtime, so the budget is generous and the POLL is what
// makes it cheap: a fast boot costs 50ms here, not the budget.
const BOOT_BUDGET_MS = 30000;
const POLL_MS = 50;

const sleep = (ms) =>
  new Promise((r) => setTimeout(r, ms));

const readLog = () => {
  try {
    return readFileSync(logPath, "utf8");
  } catch {
    return "";
  }
};

// Wait for the port to answer, not for a fixed sleep: a `sleep 2` is both
// slower than the server and, on the day it is slower than the sleep, a
// flake that blames the assertion.
const awaitHealth = async () => {
  const deadline = Date.now() + BOOT_BUDGET_MS;
  for (;;) {
    try {
      const res = await fetch(
        `${base}/api/health`,
      );
      if (res.ok) {
        return await res.json();
      }
    } catch {
      // Not listening yet — the only expected error here.
    }
    if (Date.now() > deadline) {
      // The server's OWN output is the evidence. Without it this read
      // "the server did not start", which is the one thing already known.
      fail(
        `the server never answered ${base}/api/health within ${BOOT_BUDGET_MS}ms — its output was:`,
        readLog(),
      );
    }
    await sleep(POLL_MS);
  }
};

const get = async (path) => {
  const res = await fetch(`${base}${path}`);
  if (!res.ok) {
    fail(
      `GET ${path} answered ${res.status}, expected 200`,
    );
  }
  return await res.text();
};

const mustContain = (
  body,
  needle,
  claim,
  where,
) => {
  if (!body.includes(needle)) {
    fail(
      `${claim} — ${where} does not contain ${JSON.stringify(needle)}`,
    );
  }
};

const health = await awaitHealth();

// The corpus is real: the scratch tree's markdown was found by the PACKED
// bin, from a copy of itself relocated out of node_modules. A viewer serving
// zero documents would satisfy every markup assertion below while being
// useless, so the count is checked first.
if (
  typeof health.documentCount !== "number" ||
  health.documentCount < 1
) {
  fail(
    "/api/health reports no documents — the packed bin served nothing",
    JSON.stringify(health),
  );
}

// THE STRIP, from the packed tarball. These four classes are the plggmatic
// ENGINE's (`plggmatic@^0.2.0`, resolved from the registry by the scratch
// install): the row that owns horizontal scrolling, the column track, the
// sticky column header, and the breadcrumb rail the engine folds out of the
// Scene. This is the assertion the ticket exists for — the UI replacement
// (PR #9) rendered the strip through the engine, and the way that silently
// dies is the engine failing to resolve for a REAL consumer, where nothing
// but a packed-and-installed run can see it.
const root = await get("/");
for (const marker of [
  "pm-row",
  "pm-col",
  "pm-colhead",
  "pm-crumbs",
]) {
  mustContain(
    root,
    marker,
    "the served root is not the engine strip",
    "GET /",
  );
}

// The corpus column lists the scratch document, and its link is a /resolve
// address (docs/adr/0007) — so the strip is not merely present, it is
// populated by this tree.
mustContain(
  root,
  `/resolve/${docPath}`,
  "the corpus column does not link the scratch document",
  "GET /",
);

// A column opens, addressed. This is the product's headline claim reached
// through the bin rather than through the unit suite, which never packs
// anything.
const column = await get(`/resolve/${docPath}`);
mustContain(
  column,
  "SMOKE-DOCUMENT-BODY",
  "the addressed column does not render the document",
  `GET /resolve/${docPath}`,
);
mustContain(
  column,
  "pm-col",
  "the addressed column is not an engine column",
  `GET /resolve/${docPath}`,
);

// NO QFS, AND IT SAYS SO — at the surface a reader is actually looking at.
// The scratch config points `qfs.bin` at a path that cannot exist, so this
// column's describe fails; what it must NOT do is 500, and what it must do
// is print the remedy (docs/adr/0009).
const qfsColumn = await get("/resolve/qfs:/local");
for (const needle of [
  "qfs could not be run",
  "install.sh",
]) {
  mustContain(
    qfsColumn,
    needle,
    "an unreachable qfs does not tell the reader what is missing and how to get it",
    "GET /resolve/qfs:/local",
  );
}

// …and said it ONCE at boot, before anyone clicked anything. The log is
// structured (one JSON object per line), so the event name is matched as
// the field it is, not as prose.
const log = readLog();
for (const needle of [
  '"event":"qfs.unreachable"',
  "install.sh",
  '"event":"serve.listening"',
]) {
  mustContain(
    log,
    needle,
    "the boot log does not report the missing qfs",
    "the server's stdout",
  );
}

// STILL SERVING. "Rather than crashing" is the acceptance's own wording, and
// a viewer that answered every request above and then died on the qfs one
// would have passed everything so far. Asked last, and asked of the same
// process.
const after = await fetch(`${base}/api/health`);
if (!after.ok) {
  fail(
    `the server stopped serving after the qfs column: /api/health answered ${after.status}`,
    readLog(),
  );
}

process.stdout.write(
  `    serve: ${health.documentCount} document(s), engine strip, /resolve column, qfs advice — still serving\n`,
);
