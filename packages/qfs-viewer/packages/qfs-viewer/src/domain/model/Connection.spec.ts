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
  asQfsConnection,
  defaultConnection,
} from "#qfs-viewer/domain/model/Connection";

// The zero-config default is the whole of demo leg 1: `npx qfs-viewer` with
// no daemon and no config still reaches qfs, by spawning the one on PATH.
test("the default connection spawns the qfs on PATH", () =>
  check(
    defaultConnection,
    toEqual({ __tag: "Spawn", bin: "qfs" }),
  ));

test("the spawn form takes an explicit binary path", () =>
  andThen(
    shouldBeOk()(
      asQfsConnection({
        form: "spawn",
        bin: "/opt/qfs/bin/qfs",
      }),
    ),
    (c) =>
      check(
        c,
        toEqual({
          __tag: "Spawn",
          bin: "/opt/qfs/bin/qfs",
        }),
      ),
  ));

test("the spawn form's bin defaults to qfs when omitted", () =>
  andThen(
    shouldBeOk()(
      asQfsConnection({ form: "spawn" }),
    ),
    (c) => check(c, toEqual(defaultConnection)),
  ));

// The other two issuance forms are selectable TODAY — that the config
// vocabulary already knows them is what makes the seam swappable rather
// than aspirational. What they answer is the vendor adapter's business.
test("the local-server and remote forms parse with their address", () =>
  all([
    andThen(
      shouldBeOk()(
        asQfsConnection({
          form: "local-server",
          url: "http://localhost:7700",
        }),
      ),
      (c) =>
        check(
          c,
          toEqual({
            __tag: "LocalServer",
            url: "http://localhost:7700",
          }),
        ),
    ),
    andThen(
      shouldBeOk()(
        asQfsConnection({
          form: "remote",
          url: "https://qfs.example.com",
        }),
      ),
      (c) =>
        check(
          c,
          toEqual({
            __tag: "Remote",
            url: "https://qfs.example.com",
          }),
        ),
    ),
  ]));

// Reject, don't repair — a misspelled form falling back to spawn would
// answer the author's question by ignoring it.
test("an unknown form is rejected, not defaulted", () =>
  andThen(
    shouldBeErr()(
      asQfsConnection({ form: "daemon" }),
    ),
    (e) =>
      check(
        e.content.message.includes("qfs.form"),
        toBe(true),
      ),
  ));

test("a server form without its address is rejected", () =>
  all([
    andThen(
      shouldBeErr()(
        asQfsConnection({
          form: "local-server",
        }),
      ),
      (e) =>
        check(
          e.content.message.includes("qfs.url"),
          toBe(true),
        ),
    ),
    andThen(
      shouldBeErr()(
        asQfsConnection({
          form: "remote",
          url: "",
        }),
      ),
      (e) =>
        check(
          e.content.message.includes("qfs.url"),
          toBe(true),
        ),
    ),
  ]));

test("a spawn form with an empty bin is rejected", () =>
  andThen(
    shouldBeErr()(
      asQfsConnection({
        form: "spawn",
        bin: "",
      }),
    ),
    (e) =>
      check(
        e.content.message.includes("qfs.bin"),
        toBe(true),
      ),
  ));

test("a non-object qfs key is rejected", () =>
  andThen(
    shouldBeErr()(asQfsConnection("spawn")),
    (e) =>
      check(
        e.content.message.includes(
          "must be an object",
        ),
        toBe(true),
      ),
  ));
