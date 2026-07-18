import {
  test,
  check,
  all,
  toBe,
  okThen,
  shouldBeOk,
  shouldBeErr,
  andThen,
} from "plgg-test";
import { isOk } from "plgg";
import {
  asDocumentPath,
  isDocumentPath,
  documentPathString,
  asDocumentSlug,
  documentSlugString,
  asHeadingAnchor,
  asRoute,
  isRoute,
  routeString,
  resolveRelativePath,
} from "#qfs-viewer/domain/model/Vocabulary";

test("asDocumentPath accepts a relative markdown path and unwraps it", () =>
  check(
    asDocumentPath("docs/adr/0001-npm-only.md"),
    okThen((p) =>
      toBe("docs/adr/0001-npm-only.md")(
        documentPathString(p),
      ),
    ),
  ));

test("asDocumentPath rejects what is not a repo-relative markdown path", () =>
  all([
    // absolute
    check(
      asDocumentPath("/etc/passwd.md"),
      shouldBeErr(),
    ),
    // not markdown
    check(
      asDocumentPath("docs/logo.png"),
      shouldBeErr(),
    ),
    // traverses out of the root
    check(
      asDocumentPath("docs/../../secret.md"),
      shouldBeErr(),
    ),
    // empty segment
    check(
      asDocumentPath("docs//a.md"),
      shouldBeErr(),
    ),
    // empty string
    check(asDocumentPath(""), shouldBeErr()),
    // not a string at all
    check(asDocumentPath(42), shouldBeErr()),
  ]));

test("asDocumentPath accepts a leading-dots filename that does not traverse", () =>
  // "..foo.md" is a legitimate name; only a ".." SEGMENT
  // traverses. A substring test would wrongly reject this.
  check(
    asDocumentPath("docs/..foo.md"),
    shouldBeOk(),
  ));

test("asDocumentPath accepts an uppercase extension — the corpus is other people's files", () =>
  // Must agree with isDocumentFile in domain/model/Scan.ts. When these
  // disagreed, a README.MD was walked and then rejected here, so it
  // disappeared into the scan's collected errors instead of being indexed.
  all([
    check(
      asDocumentPath("docs/README.MD"),
      shouldBeOk(),
    ),
    check(
      asDocumentPath("docs/Mixed.Md"),
      shouldBeOk(),
    ),
  ]));

test("isDocumentPath guards the brand, not the shape", () =>
  all([
    andThen(
      shouldBeOk()(asDocumentPath("a.md")),
      (p) => toBe(true)(isDocumentPath(p)),
    ),
    // a bare string that would PASS the predicate is still
    // not branded — the brand is the point.
    check(isDocumentPath("a.md"), toBe(false)),
  ]));

test("asDocumentSlug accepts a hyphenated lowercase slug", () =>
  check(
    asDocumentSlug("0001-npm-only"),
    okThen((s) =>
      toBe("0001-npm-only")(
        documentSlugString(s),
      ),
    ),
  ));

test("asDocumentSlug rejects anything but single-hyphenated lowercase", () =>
  all([
    check(
      asDocumentSlug("NpmOnly"),
      shouldBeErr(),
    ),
    check(asDocumentSlug("a--b"), shouldBeErr()),
    check(asDocumentSlug("-a"), shouldBeErr()),
    check(asDocumentSlug("a-"), shouldBeErr()),
    check(asDocumentSlug(""), shouldBeErr()),
  ]));

test("asHeadingAnchor shares the slug grammar so a citation round-trips", () =>
  all([
    check(asHeadingAnchor("goal"), shouldBeOk()),
    check(
      asHeadingAnchor("Goal Section"),
      shouldBeErr(),
    ),
  ]));

test("asRoute accepts the root and a nested route", () =>
  all([
    check(
      asRoute("/"),
      okThen((r) => toBe("/")(routeString(r))),
    ),
    check(
      asRoute("/docs/adr/0001-npm-only"),
      shouldBeOk(),
    ),
  ]));

test("asRoute rejects what is not a leading-slashed, non-traversing path", () =>
  all([
    // no leading slash
    check(asRoute("docs/adr"), shouldBeErr()),
    // trailing slash (the root "/" is the sole exception)
    check(asRoute("/docs/"), shouldBeErr()),
    // traverses
    check(asRoute("/docs/../etc"), shouldBeErr()),
    // empty segment
    check(asRoute("/docs//adr"), shouldBeErr()),
    // not a string at all — a URL is user input, so the
    // caster takes unknown
    check(asRoute(42), shouldBeErr()),
  ]));

test("isRoute guards the brand, not the shape", () =>
  all([
    andThen(shouldBeOk()(asRoute("/docs")), (r) =>
      toBe(true)(isRoute(r)),
    ),
    check(isRoute("/docs"), toBe(false)),
  ]));

// The bug this exists to prevent, pinned. `docs/adr/index.md` writes
// `](0001-npm-only.md)`, which means its NEIGHBOUR — not a file at the
// repository root. Resolving it as root-relative sent every ADR-index link to
// a nonexistent document, and each one opened a column saying "not in the
// corpus". Found by driving the real corpus, not by a test.
const from = (raw: string) => {
  const r = asDocumentPath(raw);
  if (!isOk(r)) {
    throw new Error(`fixture: ${raw}`);
  }
  return r.content;
};

const resolved = (
  base: string,
  target: string,
): string | undefined => {
  const o = resolveRelativePath(
    from(base),
    target,
  );
  return o.__tag === "Some"
    ? documentPathString(o.content)
    : undefined;
};

test("a bare neighbour resolves against the document's own directory", () =>
  all([
    check(
      resolved(
        "docs/adr/index.md",
        "0001-npm-only.md",
      ),
      toBe("docs/adr/0001-npm-only.md"),
    ),
    check(
      resolved(
        "docs/adr/index.md",
        "./0001-npm-only.md",
      ),
      toBe("docs/adr/0001-npm-only.md"),
    ),
  ]));

test("a root-level document resolves its neighbours at the root", () =>
  check(
    resolved("README.md", "CLAUDE.md"),
    toBe("CLAUDE.md"),
  ));

test("a subdirectory target resolves under the document's directory", () =>
  check(
    resolved("docs/index.md", "adr/0001-x.md"),
    toBe("docs/adr/0001-x.md"),
  ));

test("a .. climbs one directory", () =>
  all([
    check(
      resolved(
        "docs/adr/index.md",
        "../guide.md",
      ),
      toBe("docs/guide.md"),
    ),
    check(
      resolved(
        "docs/adr/index.md",
        "../../README.md",
      ),
      toBe("README.md"),
    ),
  ]));

// Climbing above the root is a broken link in the document, not a link to the
// root. Clamping it would answer with some unrelated document and call that
// success.
test("a link that climbs above the root resolves to nothing", () =>
  check(
    resolved(
      "docs/adr/index.md",
      "../../../etc/passwd.md",
    ),
    toBe(undefined),
  ));

test("a leading slash means root-relative", () =>
  check(
    resolved("docs/adr/index.md", "/README.md"),
    toBe("README.md"),
  ));

// Rewriting these into columns would be a bug that ate the web.
test("anything that is not a document in this corpus resolves to nothing", () =>
  all([
    check(
      resolved(
        "docs/a.md",
        "https://example.com/x.md",
      ),
      toBe(undefined),
    ),
    check(
      resolved("docs/a.md", "mailto:a@qmu.jp"),
      toBe(undefined),
    ),
    check(
      resolved("docs/a.md", "../assets/logo.png"),
      toBe(undefined),
    ),
    check(
      resolved("docs/a.md", ""),
      toBe(undefined),
    ),
  ]));

// A scheme is checked first: `https://host/y.md` would otherwise normalize
// into the plausible-looking `https:/host/y.md`.
test("an https target is not mistaken for a path because it ends in .md", () =>
  check(
    resolved(
      "docs/a.md",
      "https://plgg.dev/guide.md",
    ),
    toBe(undefined),
  ));
