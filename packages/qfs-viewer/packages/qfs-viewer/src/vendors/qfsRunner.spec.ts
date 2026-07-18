// The qfs adapter against a REAL subprocess.
//
// The domain tests run on fake runners; this spec is the counterweight
// (workaholic:implementation / test: test against the real thing). No real
// qfs is required — a fixture executable stands in, because what this
// adapter owes is subprocess mechanics, not qfs semantics: argv passing,
// stdout-as-answer, structured-error-on-nonzero-exit, and the not-JSON
// contract breach. qfs semantics are Resource.spec's and Describe.spec's
// business, against captured real answers.
import {
  test,
  check,
  all,
  toBe,
  toEqual,
  shouldBeOk,
  shouldBeErr,
  andThen,
} from "plgg-test";
import {
  mkdtempSync,
  writeFileSync,
  chmodSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  qfsRunner,
  probeQfs,
} from "#qfs-viewer/vendors/qfsRunner";

// A fixture binary whose behaviour is the script's, run through the real
// spawn path. Cleaned up per test so order cannot matter.
const withFixtureBin = <A>(
  script: string,
  body: (bin: string) => A,
): A => {
  const dir = mkdtempSync(
    join(tmpdir(), "qfs-viewer-runner-"),
  );
  const bin = join(dir, "fake-qfs");
  try {
    writeFileSync(bin, `#!/bin/sh\n${script}\n`);
    chmodSync(bin, 0o755);
    return body(bin);
  } finally {
    rmSync(dir, {
      recursive: true,
      force: true,
    });
  }
};

test("the spawn form runs the configured binary and parses its stdout", () =>
  withFixtureBin(
    `echo '{"schema":[],"rows":[]}'`,
    (bin) =>
      andThen(
        shouldBeOk()(
          qfsRunner({ __tag: "Spawn", bin }).run(
            "/local/x |> limit 1",
          ),
        ),
        (answer) =>
          check(
            answer,
            toEqual({ schema: [], rows: [] }),
          ),
      ),
  ));

// The statement travels as ONE argv element and never touches a shell; the
// fixture echoes its argv back so the contract is observed, not assumed.
test("run and describe pass their subcommand and argument as argv", () =>
  withFixtureBin(
    `printf '{"argv":"%s"}' "$*"`,
    (bin) => {
      const runner = qfsRunner({
        __tag: "Spawn",
        bin,
      });
      return all([
        andThen(
          shouldBeOk()(
            runner.run("/a |> limit 1"),
          ),
          (answer) =>
            check(
              answer,
              toEqual({
                argv: "--json run /a |> limit 1",
              }),
            ),
        ),
        andThen(
          shouldBeOk()(runner.describe("/a/b")),
          (answer) =>
            check(
              answer,
              toEqual({
                argv: "--json describe /a/b",
              }),
            ),
        ),
      ]);
    },
  ));

// qfs reports a parse error or an unknown mount as JSON on stdout AND a
// non-zero exit (observed: exit 3 with {"error":{…}}). The answer is worth
// more than the exception.
test("a non-zero exit still surfaces qfs's structured answer", () =>
  withFixtureBin(
    `echo '{"error":{"code":"unknown_mount","message":"no driver"}}'; exit 3`,
    (bin) =>
      andThen(
        shouldBeOk()(
          qfsRunner({
            __tag: "Spawn",
            bin,
          }).describe("/nosuch"),
        ),
        (answer) =>
          check(
            answer,
            toEqual({
              error: {
                code: "unknown_mount",
                message: "no driver",
              },
            }),
          ),
      ),
  ));

// A missing qfs is the ONE error whose remedy the reader cannot guess, so
// the message owes more than "could not be run": the binary that was looked
// for, the way to get one, and the fact that markdown browsing never needed
// it. Same words as the boot probe's — `unreachableAdvice` is the single
// source (domain/model/Connection.ts), and docs/adr/0009 is why they are the
// only remedy this product can offer.
test("a missing binary is an adapter failure that says what is missing and how to get it", () =>
  andThen(
    shouldBeErr()(
      qfsRunner({
        __tag: "Spawn",
        bin: "/no/such/qfs-binary",
      }).run("/a |> limit 1"),
    ),
    (e) =>
      all([
        check(
          e.content.message.includes(
            "qfs could not be run",
          ),
          toBe(true),
        ),
        check(
          e.content.message.includes(
            "/no/such/qfs-binary",
          ),
          toBe(true),
        ),
        check(
          e.content.message.includes(
            "install.sh",
          ),
          toBe(true),
        ),
        check(
          e.content.message.includes(
            "Markdown browsing does not need qfs",
          ),
          toBe(true),
        ),
      ]),
  ));

test("output that is not JSON is qfs breaking its contract, and is said", () =>
  withFixtureBin(`echo 'plain text'`, (bin) =>
    andThen(
      shouldBeErr()(
        qfsRunner({ __tag: "Spawn", bin }).run(
          "/a |> limit 1",
        ),
      ),
      (e) =>
        check(
          e.content.message.includes("not JSON"),
          toBe(true),
        ),
    ),
  ));

// The boot probe. It is what makes "starts and says exactly what is missing"
// true BEFORE anyone clicks a qfs path — and the version it reports on the
// happy path is the only record of which qfs a session was talking to.
test("the probe reports the configured binary's version line", () =>
  withFixtureBin(
    `echo 'qfs 0.0.71'; echo 'commit:  f9387de'`,
    (bin) =>
      andThen(
        shouldBeOk()(
          probeQfs({ __tag: "Spawn", bin }),
        ),
        (version) =>
          // The FIRST line only: qfs prints its commit under the version,
          // and a log field is one fact.
          check(version, toBe("qfs 0.0.71")),
      ),
  ));

test("the probe asks for --version and nothing else", () =>
  withFixtureBin(`printf '%s' "$*"`, (bin) =>
    andThen(
      shouldBeOk()(
        probeQfs({ __tag: "Spawn", bin }),
      ),
      (argv) => check(argv, toBe("--version")),
    ),
  ));

// The probe's failure IS the boot message, so it carries the advice rather
// than a status: `serve.ts` prints exactly this and adds nothing.
test("an unreachable binary probes to the advice, not a bare error", () =>
  andThen(
    shouldBeErr()(
      probeQfs({
        __tag: "Spawn",
        bin: "/no/such/qfs-binary",
      }),
    ),
    (e) =>
      all([
        check(
          e.content.message.includes(
            "/no/such/qfs-binary",
          ),
          toBe(true),
        ),
        check(
          e.content.message.includes(
            "install.sh",
          ),
          toBe(true),
        ),
      ]),
  ));

// A non-zero `--version` is an unreachable qfs too: something is there and
// it cannot answer the cheapest question qfs answers.
test("a binary that cannot answer --version probes as unreachable", () =>
  withFixtureBin(`exit 1`, (bin) =>
    andThen(
      shouldBeErr()(
        probeQfs({ __tag: "Spawn", bin }),
      ),
      (e) =>
        check(
          e.content.message.includes(
            "qfs could not be run",
          ),
          toBe(true),
        ),
    ),
  ));

// ① and ③ dial nothing yet, so "reachable" is not a question about them —
// the probe reports what the runner would answer every query with, instead
// of inventing a health check for an interface that does not exist.
test("the skeleton forms probe to their own not-implemented message", () =>
  andThen(
    shouldBeErr()(
      probeQfs({
        __tag: "Remote",
        url: "https://qfs.example.com",
      }),
    ),
    (e) =>
      check(
        e.content.message.includes(
          "remote issuance form",
        ),
        toBe(true),
      ),
  ));

// The skeleton forms exist to be SELECTABLE, and to answer with what works
// today rather than a connection refused three layers down.
test("the local-server and remote forms answer with the typed skeleton error", () => {
  const local = qfsRunner({
    __tag: "LocalServer",
    url: "http://localhost:7700",
  });
  const remote = qfsRunner({
    __tag: "Remote",
    url: "https://qfs.example.com",
  });
  return all([
    andThen(
      shouldBeErr()(local.run("/a |> limit 1")),
      (e) =>
        all([
          check(
            e.content.message.includes(
              "local-server issuance form",
            ),
            toBe(true),
          ),
          check(
            e.content.message.includes(
              "http://localhost:7700",
            ),
            toBe(true),
          ),
        ]),
    ),
    andThen(
      shouldBeErr()(remote.describe("/a")),
      (e) =>
        check(
          e.content.message.includes(
            "remote issuance form",
          ),
          toBe(true),
        ),
    ),
  ]);
});
