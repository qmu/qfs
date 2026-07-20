import {
  test,
  check,
  all,
  toBe,
  shouldBeOk,
  shouldBeErr,
  andThen,
} from "plgg-test";
import { some, none } from "plgg";
import {
  type Principal,
  asPrincipal,
  accessFor,
  mayRead,
  mayEdit,
  bearerOf,
} from "#qfs-viewer/domain/model/Principal";

const reader: Principal = {
  name: "ci-bot",
  key: "reader-fixture-not-a-real-key",
  role: "reader",
};
const editor: Principal = {
  name: "alice",
  key: "editor-fixture-not-a-real-key",
  role: "editor",
};

// The product's headline case: `npx qfs-viewer` at your own repository
// root, on your own machine, reading your own files. Demanding a token there
// would be theatre performed for an audience of one.
test("a corpus that declares no principals is open to everyone", () =>
  all([
    check(
      accessFor([], none()).__tag,
      toBe("Open"),
    ),
    check(
      accessFor([], some("anything")).__tag,
      toBe("Open"),
    ),
    check(
      mayRead(accessFor([], none())),
      toBe(true),
    ),
    check(
      mayEdit(accessFor([], none())),
      toBe(true),
    ),
  ]));

// Declaring a principal is the moment a corpus stops being one developer's
// local tree. A server that says who may use it does not also serve people who
// did not say.
test("once principals exist, an unauthenticated request is denied", () => {
  const access = accessFor([reader], none());
  return all([
    check(access.__tag, toBe("Denied")),
    check(mayRead(access), toBe(false)),
    check(mayEdit(access), toBe(false)),
  ]);
});

test("an unknown key is denied", () =>
  check(
    accessFor(
      [reader],
      some("not-a-real-key-at-all"),
    ).__tag,
    toBe("Denied"),
  ));

test("a known key is granted its principal", () => {
  const access = accessFor(
    [reader, editor],
    some(editor.key),
  );
  return all([
    check(access.__tag, toBe("Granted")),
    check(
      access.__tag === "Granted" &&
        access.principal.name,
      toBe("alice"),
    ),
  ]);
});

// The two roles, which is the whole point of having roles.
test("a reader may read and may not edit", () => {
  const access = accessFor(
    [reader],
    some(reader.key),
  );
  return all([
    check(mayRead(access), toBe(true)),
    check(mayEdit(access), toBe(false)),
  ]);
});

test("an editor may do both", () => {
  const access = accessFor(
    [editor],
    some(editor.key),
  );
  return all([
    check(mayRead(access), toBe(true)),
    check(mayEdit(access), toBe(true)),
  ]);
});

// Distinguishing "unknown key" from "wrong key" would tell a prober which half
// to work on.
test("a denial says the same thing however it was reached", () =>
  check(
    accessFor(
      [reader],
      some("wrong-key-xxxxxxxxxxxx"),
    ).__tag === "Denied" &&
      accessFor(
        [reader],
        some("other-wrong-key-yyyyyy"),
      ).__tag === "Denied",
    toBe(true),
  ));

test("a bearer token is read out of the Authorization header", () =>
  all([
    check(
      bearerOf(some("Bearer abc123")).__tag ===
        "Some" &&
        bearerOf(some("Bearer abc123")).content,
      toBe("abc123"),
    ),
    // the scheme is case-insensitive per RFC 7235, and clients differ
    check(
      bearerOf(some("bearer abc123")).__tag,
      toBe("Some"),
    ),
    check(
      bearerOf(some("  Bearer   abc123  "))
        .__tag === "Some" &&
        bearerOf(some("  Bearer   abc123  "))
          .content,
      toBe("abc123"),
    ),
  ]));

test("anything that is not a bearer token reads as absent", () =>
  all([
    check(bearerOf(none()).__tag, toBe("None")),
    check(bearerOf(some("")).__tag, toBe("None")),
    check(
      bearerOf(some("Bearer")).__tag,
      toBe("None"),
    ),
    check(
      bearerOf(some("Bearer ")).__tag,
      toBe("None"),
    ),
    check(
      bearerOf(some("Basic dXNlcjpwYXNz")).__tag,
      toBe("None"),
    ),
  ]));

test("a declared principal is validated at the boundary", () =>
  andThen(
    shouldBeOk()(
      asPrincipal(
        {
          name: "alice",
          key: "editor-fixture-not-a-real-key",
          role: "editor",
        },
        0,
      ),
    ),
    (p) =>
      all([
        check(p.name, toBe("alice")),
        check(p.role, toBe("editor")),
      ]),
  ));

// A short key is worse than no key: it looks like access control while being
// guessable, so the reader believes they are protected.
test("a short key is refused, because it is the appearance of security", () =>
  andThen(
    shouldBeErr()(
      asPrincipal(
        {
          name: "x",
          key: "short",
          role: "reader",
        },
        0,
      ),
    ),
    (e) =>
      toBe(true)(
        e.content.message.includes(
          "16 characters",
        ),
      ),
  ));

test("a malformed principal is refused, naming its index", () =>
  all([
    check(asPrincipal("nope", 0), shouldBeErr()),
    check(
      asPrincipal(
        {
          key: "0123456789abcdef",
          role: "reader",
        },
        0,
      ),
      shouldBeErr(),
    ),
    check(
      asPrincipal(
        { name: "x", role: "reader" },
        0,
      ),
      shouldBeErr(),
    ),
    check(
      asPrincipal(
        {
          name: "x",
          key: "0123456789abcdef",
          role: "admin",
        },
        0,
      ),
      shouldBeErr(),
    ),
    check(
      asPrincipal(
        {
          name: "",
          key: "0123456789abcdef",
          role: "reader",
        },
        0,
      ),
      shouldBeErr(),
    ),
  ]));

test("a role error names the index so a long list can be fixed", () =>
  andThen(
    shouldBeErr()(
      asPrincipal(
        {
          name: "x",
          key: "0123456789abcdef",
          role: "wat",
        },
        3,
      ),
    ),
    (e) =>
      toBe(true)(
        e.content.message.includes(
          "principals[3].role",
        ),
      ),
  ));

// The key comparison walks every byte rather than bailing on the first
// mismatch, so its duration does not advertise how much of the key was right.
// This asserts CORRECTNESS, not the timing property — a unit test cannot see
// that, and pretending otherwise would be worse than saying so.
test("key comparison is exact: a prefix is not a match", () =>
  all([
    check(
      accessFor(
        [reader],
        some("reader-fixture-not-a-real-ke"),
      ).__tag,
      toBe("Denied"),
    ),
    check(
      accessFor(
        [reader],
        some("reader-fixture-not-a-real-keyy"),
      ).__tag,
      toBe("Denied"),
    ),
    check(
      accessFor([reader], some(reader.key)).__tag,
      toBe("Granted"),
    ),
  ]));
