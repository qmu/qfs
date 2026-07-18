// `GET /resolve/<trail>` (and the bare `/`) — the corpus as accreting columns.
//
// The mission's gate: "following a document link opens it in a new column to
// the right without discarding the previous one, and the URL records the
// traversal so a reload restores the same columns." The address is a path
// under /resolve, prefix-closed at the comma — column i is the resolution of
// the address's prefix i, and a click appends one segment (docs/adr/0007).
//
// It is SERVER-RENDERED, and that is not a limitation — it is what makes the
// claim true. Every column is a function of the URL, so a reload, a bookmark,
// a pasted link, and `curl` all reconstruct the identical screen. A
// client-side column stack would keep its state in memory, where the URL
// could not see it and a reload would lose it; the state would then be
// *reported* in the URL rather than *held* there, and the two would drift the
// first time someone pressed Back. The ONLY client script is the engine's
// appearance bootstrap (the html.dark scheme class) — presentation, holding
// nothing navigable.
//
// The strip itself is the plggmatic ENGINE's (ADR 0002, second amendment):
// engine columns in an engine row, sticky engine column headers, the engine's
// chrome CSS — the measured reference shape (docs/plggmatic-semantics/
// poc-findings.md: depth grows the strip's own scroll width, never the page
// body's). The trail also lowers into the engine's `Scene`
// (domain/usecase/scene.ts), which the engine's own `crumbsOf` folds into the
// breadcrumb rail — one Scene, of which this strip is one skin.
//
// The trick that makes links accrete is `resolveLink`. plgg-md hands every
// link target to that seam before emitting it, so a link to `other.md` inside
// column 2 is rewritten to the URL for "the trail up to column 2, plus
// other.md". The document's own markdown is untouched; the same file rendered
// at a different depth resolves its links differently, because depth is what
// changes.
import {
  isOk,
  fromNullable,
  match,
  matchOption,
  invalidError,
  ok,
  err,
  some,
  none,
  type Option,
  type Result,
  type InvalidError,
  type SoftStr,
  type PromisedResult,
} from "plgg";
import {
  renderMarkdownWithOptions,
  renderOptions,
  plainHighlighter,
} from "plgg-md";
import {
  pageResponse,
  textResponse,
  statusOf,
  type Context,
  type HttpResponse,
  type HttpError,
} from "plgg-server";
import {
  type Html,
  type Flow,
  slot,
  div,
  h1,
  h2,
  p,
  ul,
  li,
  a,
  span,
  nav,
  section,
  form,
  input,
  button,
  table,
  thead,
  tbody,
  tr,
  th,
  td,
  text,
  href,
  attr,
  class_,
  raw,
  renderToString,
} from "plgg-view";
import {
  row,
  column,
  navPane,
  mainPane,
  asidePane,
  colHead,
  breadcrumb,
  crumbsOf,
  menuLevel$,
  listLevel$,
  boardLevel$,
  detailLevel$,
  type Level,
  type MenuLink,
  type Parts,
} from "plggmatic";
import {
  pragmaticTheme,
  schemeCss,
  metricCss,
  chromeCss,
  reducedMotionCss,
  colorVar,
  metricVar,
  basis,
  appearanceInitScript,
} from "plggmatic/style";
import {
  type Index,
  listDocuments,
  getDocument,
  documentCount,
  indexErrors,
} from "#qfs-viewer/domain/model/Index";
import {
  type Trail,
  parseTrail,
  parseResolvePath,
  trailUrl,
  formatTrail,
  openFrom,
  docStop,
  resourceStop,
  qfsStop,
} from "#qfs-viewer/domain/model/Trail";
import {
  type DefaultView,
  asResourceDescribe,
  lowerToDefaultView,
} from "#qfs-viewer/domain/model/Describe";
import {
  parseListQuery,
  defaultLimit,
} from "#qfs-viewer/domain/model/Query";
import {
  listCollection,
  matchDocuments,
} from "#qfs-viewer/domain/usecase/listCollection";
import { tagGroupsOf } from "#qfs-viewer/domain/usecase/tagGroups";
import {
  rootLevel,
  docLevel,
  resourceLevel,
  qfsLevel,
  qfsErrorLevel,
  sceneOf,
} from "#qfs-viewer/domain/usecase/scene";
import {
  type QueryPath,
  type DriverConnections,
  type Declared,
  asQueryPaths,
  asConnectionsByDriver,
  matchDeclared,
  PATHS_QUERY,
  CONNECTIONS_QUERY,
} from "#qfs-viewer/domain/model/Declaration";
import {
  type Config,
  defaultConfig,
} from "#qfs-viewer/domain/model/Config";
import { asResourceTable } from "#qfs-viewer/domain/model/Resource";
import { type ResourceRunner } from "#qfs-viewer/domain/model/Scan";
import { numberedHeading } from "#qfs-viewer/entrypoints/document";
import { type IndexRef } from "#qfs-viewer/domain/usecase/reload";
import { type CollectionLink } from "#qfs-viewer/domain/model/Collection";
import { documentLinks } from "#qfs-viewer/domain/usecase/collection";
import {
  resolveRelativePath,
  documentPathString,
  asDocumentPath,
  type DocumentPath,
} from "#qfs-viewer/domain/model/Vocabulary";

// A link inside a document, rewritten to open its target one column to the
// right of the document it appears in.
//
// A target that is not a document path — an external URL, an anchor, a mailto
// — is left exactly as written. The corpus is other people's markdown, and
// rewriting `https://example.com` into a column would be a bug that ate the
// web.
const columnResolver =
  (
    trail: Trail,
    depth: number,
    from: DocumentPath,
  ) =>
  (target: string): string => {
    // The anchor rides along: `other.md#goal` opens `other.md` in the next
    // column AND lands on the heading, because plgg-md stamps that exact slug
    // as the heading's id. Splitting on the FIRST `#` only — a fragment can
    // legally contain another one.
    const hash = target.indexOf("#");
    const targetPath =
      hash === -1
        ? target
        : target.slice(0, hash);
    const fragment =
      hash === -1 ? "" : target.slice(hash);
    // A bare `#goal` is a link within this very document. It must stay as
    // written: rewriting it to a column URL would reload the whole page to
    // show the document it is already showing.
    if (targetPath === "") {
      return target;
    }
    const resolved = resolveRelativePath(
      from,
      targetPath,
    );
    return resolved.__tag === "Some"
      ? `${trailUrl(openFrom(trail, depth, docStop(resolved.content)))}${fragment}`
      : target;
  };

const optionsAt = (
  trail: Trail,
  depth: number,
  from: DocumentPath,
) => ({
  ...renderOptions(
    plainHighlighter,
    columnResolver(trail, depth, from),
  ),
  decorateHeading: numberedHeading,
});

// One column of the strip: the engine column (its Html) and its `Level` in
// the ONE Scene the whole strip lowers to (domain/usecase/scene.ts). The
// two are built together from the same resolution, so the Scene and the
// screen cannot drift.
type StripColumn = Readonly<{
  html: Html<never>;
  level: Level;
}>;

// The address that closes column `depth` and everything after it — the
// engine's own `back` semantics (the previous depth becomes the deepest),
// used both as the column header's collapse link and as the Level's back.
const backTo = (
  trail: Trail,
  depth: number,
): Option<SoftStr> =>
  some(trailUrl(trail.slice(0, depth)));

// The landmark a column takes (docs/adr/0010, D2).
//
// TWO rules, and the order matters:
//
//  1. The DEEPEST column is the page's `main`, whatever its kind. It is what
//     the URL named and what the reader came for, so it is the page's primary
//     content by construction — which is what `main` means.
//  2. Every SHALLOWER column takes the role its LEVEL KIND declares — a menu
//     is `nav`, a list or a detail left behind is `complementary`. This is the
//     reference's rule (multi-column.md:52 — "landmark roles come from the
//     level kind, never hardcoded"), and it is a `match` on the closed union,
//     so a fifth level kind is a compile error here rather than a column that
//     silently picks a landmark.
//
// Rule 1 is a DELIBERATE divergence from the reference, which is kind-driven
// at every position and therefore emits no `main` at all whenever nothing is
// drilled into. Rule 2 is the reference's own rule, which this file used to
// throw away with `deepest ? mainPane : asidePane`.
//
// Together they guarantee EXACTLY ONE `main` at every depth. The old rule did
// not, and the comment that claimed it did was false: the corpus column
// bypassed this function and was unconditionally `navPane`, so the root page
// — the most visited one — rendered ZERO `main` landmarks. Measured, not
// argued: `columns.spec.ts` asserts the count at every depth.
const paneFor = (
  level: Level,
  deepest: boolean,
  parts: Parts,
  children: ReadonlyArray<Html<never>>,
): Html<never> =>
  deepest
    ? mainPane<never>(parts, children)
    : match(level)(
        [
          menuLevel$(),
          (): Html<never> =>
            navPane<never>(parts, children),
        ],
        [
          listLevel$(),
          (): Html<never> =>
            asidePane<never>(parts, children),
        ],
        [
          boardLevel$(),
          (): Html<never> =>
            asidePane<never>(parts, children),
        ],
        [
          detailLevel$(),
          (): Html<never> =>
            asidePane<never>(parts, children),
        ],
      );

// A trail column, rendered as the ENGINE's column: a `pm-col` track of the
// strip, a landmark pane ({@link paneFor}), the sticky `colHead` header (the
// mission's static column header — its title IS the collapse link), and the
// app body under it.
const shellColumn = (
  level: Level,
  deepest: boolean,
  head: Readonly<{
    title: SoftStr;
    close: Option<SoftStr>;
    links: ReadonlyArray<
      Readonly<{ label: SoftStr; href: SoftStr }>
    >;
  }>,
  bodyClass: SoftStr,
  body: ReadonlyArray<Flow<never>>,
): Html<never> =>
  column<never>(
    [basis("32rem")],
    [
      paneFor(
        level,
        deepest,
        [],
        [
          colHead<never>(head),
          div([class_(bodyClass)], body),
        ],
      ),
    ],
  );

// A URL scheme, for telling an external link target from a root-escaping
// relative one. The collection driver already made the call that matters
// (`target_doc` is NULL for both); this only decides whether the target is
// a place a browser can go.
const EXTERNAL_TARGET = /^[a-z][a-z0-9+.-]*:/i;

// One link of the links table, rendered as what it leads to: a strip
// segment when the driver resolved a target document, a plain anchor for an
// external URL, and inert text for a root-escaping relative target — a
// column cannot open what the corpus does not hold, and pretending would
// put a dead click on screen.
const linkItem = (
  trail: Trail,
  depth: number,
  link: CollectionLink,
) => {
  const context =
    link.sectionPath.length === 0
      ? []
      : [
          span(
            [class_("link-section")],
            [
              text(
                `${link.sectionPath.join(" › ")} — `,
              ),
            ],
          ),
        ];
  const resolved = matchOption<
    SoftStr,
    Result<DocumentPath, InvalidError>
  >(
    () =>
      err(
        invalidError({
          message: "no target document",
        }),
      ),
    (target) => asDocumentPath(target),
  )(link.targetDoc);
  return li(
    [],
    [
      ...context,
      isOk(resolved)
        ? a(
            [
              // The sideways walk itself: this link's target document,
              // opened one column to the right of the document whose links
              // table row this is.
              href(
                trailUrl(
                  openFrom(
                    trail,
                    depth,
                    docStop(resolved.content),
                  ),
                ),
              ),
              attr(
                "title",
                `${link.sourceDoc}:${link.line}`,
              ),
            ],
            [text(link.target)],
          )
        : EXTERNAL_TARGET.test(link.target)
          ? a(
              [
                href(link.target),
                attr(
                  "title",
                  `${link.sourceDoc}:${link.line}`,
                ),
              ],
              [text(link.target)],
            )
          : text(link.target),
    ],
  );
};

// The document's rows of the links table, as a section under its body: the
// same links the body's prose carries, but with the section context the
// driver preserved (`source_section_path`) made visible — where in the
// document each link was written, not just that it was.
const linksSection = (
  trail: Trail,
  depth: number,
  fetched: Result<
    ReadonlyArray<CollectionLink>,
    InvalidError
  >,
): ReadonlyArray<Flow<never>> =>
  !isOk(fetched)
    ? [
        section(
          [class_("doc-links")],
          [
            h2([], [text("Links")]),
            p(
              [class_("resource-error")],
              [
                text(
                  fetched.content.content.message,
                ),
              ],
            ),
          ],
        ),
      ]
    : fetched.content.length === 0
      ? []
      : [
          section(
            [class_("doc-links")],
            [
              h2([], [text("Links")]),
              ul(
                [],
                fetched.content.map((link) =>
                  linkItem(trail, depth, link),
                ),
              ),
            ],
          ),
        ];

// One document column. A document the index does not hold still gets a column
// saying so, rather than vanishing: the URL named it, so the screen owes the
// reader an answer about it.
//
// `links` is the collection path speaking (docs/adr/0008): `None` when no
// collection is declared (the legacy corpus has no links table to read),
// `Some` carrying the links table's rows for this document — or qfs's own
// words about why they could not be read.
const documentColumn = (
  index: Index,
  trail: Trail,
  depth: number,
  path: DocumentPath,
  links: Option<
    Result<
      ReadonlyArray<CollectionLink>,
      InvalidError
    >
  >,
): StripColumn => {
  const pathString = documentPathString(path);
  const close = backTo(trail, depth);
  const deepest = depth === trail.length - 1;
  const level = docLevel(pathString, close);
  const doc = getDocument(index, path);
  if (doc.__tag === "None") {
    return {
      level,
      html: shellColumn(
        level,
        deepest,
        {
          title: pathString,
          close,
          links: [],
        },
        "column-body column-missing",
        [
          p(
            [],
            [
              text(
                "This document is not in the corpus. It may have been deleted since the link was made.",
              ),
            ],
          ),
        ],
      ),
    };
  }
  const rendered = renderMarkdownWithOptions(
    optionsAt(trail, depth, path),
  )(doc.content.content.source);
  return {
    level,
    html: shellColumn(
      level,
      deepest,
      {
        title: pathString,
        close,
        // The edit link rides the column header, and carries the trail so
        // saving returns to these columns rather than dropping the reader
        // at the root.
        links: [
          {
            label: "edit",
            href: `/edit/${pathString}?cols=${formatTrail(trail)}`,
          },
        ],
      },
      "column-body column-doc",
      [
        isOk(rendered)
          ? // The body is an `Html<never>` with an open tag, which the
            // pane's Flow children correctly refuse. `raw` is plgg-view's
            // own seam for a trusted content pipeline, and this is one:
            // plgg-md rendered this tree with `rawHtml: false`, so nothing
            // in it came from the document unescaped.
            raw(
              renderToString(
                rendered.content.body,
              ),
            )
          : p(
              [],
              [
                text(
                  `This document could not be rendered: ${rendered.content.content.message}`,
                ),
              ],
            ),
        ...matchOption<
          Result<
            ReadonlyArray<CollectionLink>,
            InvalidError
          >,
          ReadonlyArray<Flow<never>>
        >(
          () => [],
          (fetched) =>
            linksSection(trail, depth, fetched),
        )(links),
      ],
    ),
  };
};

// The runner a server gets when nobody handed it one.
//
// The capability is the argument, exactly as the editor's writer is: an `api`
// built with no runner CANNOT reach qfs, and a declared resource on such a
// server says so rather than half-working. A hosted read-only deployment gets
// this by construction rather than by remembering to configure it off.
const refusingRunner: ResourceRunner = {
  run: () =>
    err(
      invalidError({
        message:
          "this server was built without a qfs runner, so it cannot read resources",
      }),
    ),
  describe: () =>
    err(
      invalidError({
        message:
          "this server was built without a qfs runner, so it cannot describe paths",
      }),
    ),
};

// A qfs resource, rendered as what it is: a table.
//
// NOT markdown. qfs answers `{schema, rows, meta}` — columns with types and
// rows with values — and rendering that as prose would throw the schema away
// and pretend the result was authored. A real `<table>` with a real `<thead>`
// keeps the structure that assistive tech, the MCP surface and a reader all
// navigate by, which is the same reason headings here are `h1`-`h6` and not
// styled divs.
//
// Fetched per request, never indexed. A live table's whole value is being
// live; caching it would make it a stale copy of something qfs already holds
// (docs/adr/0003), and the index's guarantees are about markdown on disk.
const resourceColumn = (
  runner: ResourceRunner,
  config: Config,
  trail: Trail,
  depth: number,
  name: string,
): StripColumn => {
  const close = backTo(trail, depth);
  const deepest = depth === trail.length - 1;
  const declared = config.resources.find(
    (r) => r.name === name,
  );
  if (declared === undefined) {
    const level = resourceLevel(
      name,
      close,
      some(
        "This repository declares no resource by that name.",
      ),
    );
    return {
      level,
      html: shellColumn(
        level,
        deepest,
        { title: name, close, links: [] },
        "column-body column-missing",
        [
          p(
            [],
            [
              text(
                "This repository declares no resource by that name. A resource is browsable only when qfs-viewer.config.json says so.",
              ),
            ],
          ),
        ],
      ),
    };
  }
  const answer = runner.run(declared.query);
  const parsed = isOk(answer)
    ? asResourceTable(answer.content)
    : answer;
  if (!isOk(parsed)) {
    // qfs's own words, not "could not load". A parse error in the declared
    // statement is exactly what the person who wrote it needs to read.
    const level = resourceLevel(
      declared.label,
      close,
      some(parsed.content.content.message),
    );
    return {
      level,
      html: shellColumn(
        level,
        deepest,
        {
          title: declared.label,
          close,
          links: [],
        },
        "column-body column-resource",
        [
          p(
            [class_("resource-error")],
            [
              text(
                parsed.content.content.message,
              ),
            ],
          ),
          p(
            [class_("resource-query")],
            [text(declared.query)],
          ),
        ],
      ),
    };
  }
  const t = parsed.content;
  const level = resourceLevel(
    declared.label,
    close,
    none(),
  );
  return {
    level,
    html: shellColumn(
      level,
      deepest,
      {
        title: declared.label,
        close,
        links: [],
      },
      "column-body column-resource",
      [
        p(
          [class_("resource-query")],
          [text(declared.query)],
        ),
        ...(t.truncated
          ? [
              p(
                [class_("resource-error")],
                [
                  text(
                    "qfs truncated this result — you are not seeing every row.",
                  ),
                ],
              ),
            ]
          : []),
        table(
          [],
          [
            thead(
              [],
              [
                tr(
                  [],
                  t.columns.map((c) =>
                    th(
                      [],
                      [
                        text(
                          `${c.name} (${c.type})`,
                        ),
                      ],
                    ),
                  ),
                ),
              ],
            ),
            tbody(
              [],
              t.rows.map((row) =>
                tr(
                  [],
                  t.columns.map((c) =>
                    td(
                      [],
                      [
                        // qfs types its columns, so a row holds numbers and
                        // booleans as themselves. Rendering is the only place
                        // that needs text, so the conversion happens here and
                        // nowhere earlier.
                        text(
                          row[c.name] ===
                            undefined ||
                            row[c.name] === null
                            ? ""
                            : String(row[c.name]),
                        ),
                      ],
                    ),
                  ),
                ),
              ),
            ),
          ],
        ),
      ],
    ),
  };
};

/**
 * How many rows a generic qfs column reads.
 *
 * The limit is stated in the statement rather than assumed, so qfs's own
 * `truncated` flag is what tells the reader when there is more — the same
 * honesty rule the declared-resource column follows.
 */
const QFS_ROW_LIMIT = 200;

// The generic qfs column: any path qfs can describe, as the default view.
//
// NO per-resource code — this is the mission's 汎用ロワリング made literal.
// `describe` names the node, one read (`<path> |> limit N`) supplies the
// rows, and `lowerToDefaultView` (the ONE deterministic generator,
// domain/model/Describe.ts) turns the pair into view data this function
// merely folds. A row that names a contained child is a link that appends a
// `qfs:` segment to the trail — containment only; row selection rides the
// /resolve ticket on a grammar strategy still owns.
//
// Fetched per request, never indexed, like the declared-resource column and
// for the same reason (docs/adr/0003). Errors are qfs's own words: "no
// driver is mounted for /nosuch" is the answer the person who typed the
// path needs.
const qfsColumn = (
  runner: ResourceRunner,
  trail: Trail,
  depth: number,
  path: SoftStr,
): StripColumn => {
  const close = backTo(trail, depth);
  const deepest = depth === trail.length - 1;
  const answered = runner.describe(path);
  const described = isOk(answered)
    ? asResourceDescribe(answered.content)
    : answered;
  if (!isOk(described)) {
    const level = qfsErrorLevel(
      path,
      close,
      described.content.content.message,
    );
    return {
      level,
      html: shellColumn(
        level,
        deepest,
        { title: path, close, links: [] },
        "column-body column-missing",
        [
          p(
            [class_("resource-error")],
            [
              text(
                described.content.content.message,
              ),
            ],
          ),
        ],
      ),
    };
  }
  const read = runner.run(
    `${path} |> limit ${QFS_ROW_LIMIT}`,
  );
  const parsed = isOk(read)
    ? asResourceTable(read.content)
    : read;
  if (!isOk(parsed)) {
    const level = qfsErrorLevel(
      path,
      close,
      parsed.content.content.message,
    );
    return {
      level,
      html: shellColumn(
        level,
        deepest,
        { title: path, close, links: [] },
        "column-body column-resource",
        [
          p(
            [class_("resource-query")],
            [text(described.content.archetype)],
          ),
          p(
            [class_("resource-error")],
            [
              text(
                parsed.content.content.message,
              ),
            ],
          ),
        ],
      ),
    };
  }
  const view: DefaultView = lowerToDefaultView(
    described.content,
    parsed.content,
  );
  // The describe lowering feeding the engine's Scene: the level's rows
  // are exactly the containment links (domain/usecase/scene.ts).
  const level = qfsLevel(view, close, (child) =>
    trailUrl(
      openFrom(trail, depth, qfsStop(child)),
    ),
  );
  return {
    level,
    html: shellColumn(
      level,
      deepest,
      { title: view.path, close, links: [] },
      "column-body column-resource",
      [
        p(
          [class_("resource-query")],
          [text(view.archetype)],
        ),
        ...(view.truncated
          ? [
              p(
                [class_("resource-error")],
                [
                  text(
                    "qfs truncated this result — you are not seeing every row.",
                  ),
                ],
              ),
            ]
          : []),
        table(
          [],
          [
            thead(
              [],
              [
                tr(
                  [],
                  view.columns.map((c) =>
                    th(
                      [],
                      [
                        text(
                          `${c.name} (${c.type})`,
                        ),
                      ],
                    ),
                  ),
                ),
              ],
            ),
            tbody(
              [],
              view.rows.map((row) =>
                tr(
                  [],
                  row.cells.map((cell, i) =>
                    td(
                      [],
                      [
                        // The FIRST cell carries the row's containment link
                        // when it has one: one obvious click target per row,
                        // and the rest of the row stays plain data.
                        ...(i === 0 &&
                        row.child.__tag === "Some"
                          ? [
                              a(
                                [
                                  href(
                                    trailUrl(
                                      openFrom(
                                        trail,
                                        depth,
                                        qfsStop(
                                          row
                                            .child
                                            .content,
                                        ),
                                      ),
                                    ),
                                  ),
                                ],
                                [text(cell)],
                              ),
                            ]
                          : [text(cell)]),
                      ],
                    ),
                  ),
                ),
              ),
            ),
          ],
        ),
      ],
    ),
  };
};

// The ONE composition of a screen address: the trail as the path (trailUrl,
// docs/adr/0007), and the corpus column's DATA query — facets, paging — as
// the query string. The two never mix: the trail is never a parameter, and
// no parameter can reach the trail codec, so nothing presentational has a
// slot to hide in.
const addressOf = (
  trail: Trail,
  params: ReadonlyArray<
    readonly [string, string]
  >,
): string => {
  const base = trailUrl(trail);
  const q = params
    .map(
      ([k, v]) =>
        `${encodeURIComponent(k)}=${encodeURIComponent(v)}`,
    )
    .join("&");
  return q === "" ? base : `${base}?${q}`;
};

// Everything the request's query carries EXCEPT the legacy trail parameter:
// what a redirect to the canonical address must keep (the filter state is
// data), and what it must shed (the trail now lives in the path).
const paramsSansCols = (
  params: Readonly<Record<string, string>>,
): ReadonlyArray<readonly [string, string]> =>
  Object.entries(params).filter(
    ([k]) => k !== "cols",
  );

// One axis of the root column, rendered — and the three-state fold that keeps
// "qfs could not be asked" from being read as "nothing is connected".
//
// The three states render DISTINGUISHABLY, which is the whole point of the
// union (domain/model/Declaration.ts):
//
//  - Declared    -> the heading and the list.
//  - Undeclared  -> NOTHING. Not an empty menu, not a disabled one. qfs
//                   answered and named nothing, so there is no feature here to
//                   show. An absent feature is not an empty state.
//  - Unanswerable-> the heading and qfs's OWN WORDS. Silence here would report
//                   a broken qfs as a bare machine (workaholic:design /
//                   self-explanatory-ui: an error says what happened).
const axisSection =
  <A>(
    className: SoftStr,
    heading: SoftStr,
    truncatedWords: SoftStr,
    itemsBody: (
      items: ReadonlyArray<A>,
    ) => ReadonlyArray<Flow<never>>,
  ) =>
  (
    declared: Declared<A>,
  ): ReadonlyArray<Flow<never>> =>
    matchDeclared<A, ReadonlyArray<Flow<never>>>({
      declared: (items, truncated) => [
        section(
          [class_(className)],
          [
            h2([], [text(heading)]),
            ...(truncated
              ? [
                  p(
                    [class_("resource-error")],
                    [text(truncatedWords)],
                  ),
                ]
              : []),
            ...itemsBody(items),
          ],
        ),
      ],
      undeclared: () => [],
      unanswerable: (reason) => [
        section(
          [class_(className)],
          [
            h2([], [text(heading)]),
            p(
              [class_("resource-error")],
              [text(reason)],
            ),
          ],
        ),
      ],
    })(declared);

// AXIS 1 — the query paths: "the path you actually query", a function of what
// the operator CONNECTed. These NAVIGATE: each is a link that opens the path
// as a qfs column, through the `qfsStop` machinery that already exists. No new
// navigation concept, and no list of prefixes in this repository — qfs is
// asked (`PATHS_QUERY`) and qfs answers.
//
// NOT "よく使うパスへのリンク", which the developer scoped as a future. That
// would be a ranking derived from watching the reader, which nobody declared.
// This is the declared registry of paths that exist.
const pathsSection = (
  trail: Trail,
  declared: Declared<QueryPath>,
): ReadonlyArray<Flow<never>> =>
  axisSection<QueryPath>(
    "qfs-paths",
    "Paths",
    "qfs truncated this result — you are not seeing every declared path.",
    (items) => [
      ul(
        [],
        items.map((qp) =>
          li(
            [],
            [
              a(
                [
                  href(
                    trailUrl(
                      openFrom(
                        trail,
                        -1,
                        qfsStop(qp.path),
                      ),
                    ),
                  ),
                ],
                [text(qp.path)],
              ),
              span(
                [class_("path-driver")],
                [text(` ${qp.driver}`)],
              ),
              ...matchOption<
                SoftStr,
                ReadonlyArray<Flow<never>>
              >(
                () => [],
                (account) => [
                  span(
                    [class_("path-account")],
                    [text(` ${account}`)],
                  ),
                ],
              )(qp.account),
              // An alias is a path you can query, so it belongs here — but
              // shown as an alias, because listing it as an independent
              // connection would overstate what is connected.
              ...matchOption<
                SoftStr,
                ReadonlyArray<Flow<never>>
              >(
                () => [],
                (aliasOf) => [
                  span(
                    [class_("path-alias")],
                    [
                      text(
                        ` → alias of ${aliasOf}`,
                      ),
                    ],
                  ),
                ],
              )(qp.aliasOf),
            ],
          ),
        ),
      ),
    ],
  )(declared);

// AXIS 2 — the admin view: which drivers exist, and what connections each
// driver has.
//
// NO LINKS, and that is the two axes staying apart rather than a gap someone
// should fill. A connection is NOT a path: one `google` connection backs both
// a `gmail` path and a `gdrive` path (Declaration.ts's header records the live
// measurement), so a link from a connection could only guess which path it
// meant. There is no 1:1 map to link along.
//
// It also confers nothing. `/sys/connections` is select-only — every write
// verb reads `false` — so this reads what the operator configured on their own
// machine, under their own OS login, which is qfs's whole authentication model
// ("one operator per OS user, no password"). That is why an "admin view" here
// does not violate workaholic:design / admin-isolation: there is no privileged
// operation to isolate and no role check standing in for one. The moment this
// axis gains a write verb, that reasoning expires.
const connectionsSection = (
  declared: Declared<DriverConnections>,
): ReadonlyArray<Flow<never>> =>
  axisSection<DriverConnections>(
    "qfs-connections",
    "Drivers",
    "qfs truncated this result — you are not seeing every connection.",
    (items) => [
      ul(
        [],
        items.map((d) =>
          li(
            [],
            [
              span(
                [class_("driver-name")],
                [text(d.driver)],
              ),
              span(
                [class_("driver-connections")],
                [
                  text(
                    ` ${d.connections.join(", ")}`,
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    ],
  )(declared);

// Axis 1's paths as the root `MenuLevel`'s entries: they are openable, so the
// Scene holds them. Axis 2 contributes NOTHING here — it is a view, not
// navigation, and giving it entries would fuse the axes in the Scene even
// while the screen kept them apart.
const pathEntries = (
  trail: Trail,
  declared: Declared<QueryPath>,
): ReadonlyArray<MenuLink> =>
  matchDeclared<
    QueryPath,
    ReadonlyArray<MenuLink>
  >({
    declared: (items) =>
      items.map((qp) => ({
        label: qp.path,
        href: trailUrl(
          openFrom(trail, -1, qfsStop(qp.path)),
        ),
        active: false,
      })),
    undeclared: () => [],
    unanswerable: () => [],
  })(declared);

// The leftmost column: the corpus, faceted — plus the two axes qfs declares
// about itself.
//
// The corpus STAYS, and ungated. `Connection.ts` ships the rule this rests on:
// "Markdown browsing does not need qfs — only qfs paths do." A root reachable
// only through a qfs-derived column would break `npx qfs-viewer` on a machine
// with no qfs — the product's headline case — and would be the same structural
// error as gating it behind a sign-in that does not exist (the ticket's
// falsified premise). What changed is that the column is no longer ONLY the
// corpus: it now also answers the developer's two axes, derived rather than
// held.
//
// Every facet link carries the CURRENT trail, so narrowing the list does not
// close the columns you already opened — the facet is a filter on column 0,
// not a navigation away from everything.
const rootColumn = (
  index: Index,
  trail: Trail,
  params: Readonly<Record<string, string>>,
  config: Config,
  runner: ResourceRunner,
): StripColumn => {
  const title = matchOption<SoftStr, SoftStr>(
    () => "qfs-viewer",
    (t) => t,
  )(config.title);
  // The two axes, each asked its own question of qfs. TWO invocations per root
  // render, and the root column renders on every page — a real cost, and a
  // deliberate one: ADR 0003 forbids caching because a live registry's whole
  // value is being live, and a cached copy of what the operator connected
  // would be a stale second answer beside qfs's own.
  //
  // Asked SEPARATELY, never derived from one another. The paths axis is not
  // computable from the connections axis or the reverse — they do not share a
  // driver vocabulary (Declaration.ts).
  const paths = asQueryPaths(
    runner.run(PATHS_QUERY),
  );
  const drivers = asConnectionsByDriver(
    runner.run(CONNECTIONS_QUERY),
  );
  const query = parseListQuery(params);
  // `matched` is every document under the current filter; `documents` is
  // the page of it. The facets count `matched` — not the index, not the
  // page:
  //
  //   - counting the INDEX was a bug you could read straight off the
  //     screen: a facet said `enhancement (5)` beside a list of 3,
  //     promising five documents that clicking it could never produce,
  //     because the click ANDs with the filter already on.
  //   - counting the PAGE would be a subtler version of the same lie —
  //     the numbers would shift when you turned the page.
  const matched = isOk(query)
    ? matchDocuments(index, query.content)
    : listDocuments(index);
  const documents = isOk(query)
    ? listCollection(index, query.content)
        .contents
    : listDocuments(index);
  const groups = tagGroupsOf(matched, config);
  const activeTags = isOk(query)
    ? query.content.tags
    : [];
  const limit = isOk(query)
    ? query.content.limit
    : defaultLimit;
  const offset = isOk(query)
    ? query.content.offset
    : 0;
  // EVERY link in this column is the CURRENT state plus one change, never the
  // change alone. It used to be the change alone, and that is what made the
  // facet counts lie: a count is counted over the filtered set, so it is only
  // true if the click KEEPS that filter. With `?type=enhancement` on, the
  // `layer` facet read `Config (1)` and linked to `/?layer=Config` — which
  // drops the type and answers 4. The count and the link have to mean the same
  // query, or one of them is lying and it hardly matters which.
  //
  // A `null` override REMOVES a parameter: that is how a facet is un-applied,
  // and without it AND-ing would be a one-way ratchet into the empty set.
  // `cols` is never carried through here — the trail owns it.
  const urlWith = (
    overrides: Readonly<
      Record<string, string | null>
    >,
  ): string =>
    addressOf(
      trail,
      Object.entries({
        ...params,
        ...overrides,
      }).filter(
        (entry): entry is [string, string] =>
          entry[0] !== "cols" &&
          entry[1] !== null,
      ),
    );
  const isActive = (
    key: SoftStr,
    value: SoftStr,
  ): boolean =>
    activeTags.some(
      (t) => t.key === key && t.value === value,
    );
  // The page's place in the matched set. `first`/`last` are 1-based and
  // inclusive because they are read by a person, not sliced by a machine.
  const total = matched.length;
  const corpus = documentCount(index);
  const first =
    documents.length === 0 ? 0 : offset + 1;
  const last = offset + documents.length;
  const hasPrev = offset > 0;
  const hasNext = last < total;
  const prevOffset = Math.max(0, offset - limit);
  const nextOffset = offset + limit;
  // The corpus size is named only when a filter is on, because that is the
  // only time it is a different number from the total and so the only time it
  // says anything.
  const countLabel =
    total === 0
      ? `no documents (${corpus} in corpus)`
      : total === corpus
        ? `${first}–${last} of ${total} document(s)`
        : `${first}–${last} of ${total} document(s) (${corpus} in corpus)`;
  // What the root level of the Scene holds: the openable things — the page
  // of documents, the declared resources, and the query paths qfs declares
  // (axis 1) — as the engine's menu entries. The facets, the pager, the
  // error report and the CONNECTIONS axis are the root BODY's content, not
  // entries of it: axis 2 navigates nowhere, so it is not in the menu.
  const entries: ReadonlyArray<MenuLink> = [
    ...documents.map((d) => ({
      label: documentPathString(d.content.path),
      href: trailUrl(
        openFrom(
          trail,
          -1,
          docStop(d.content.path),
        ),
      ),
      active: false,
    })),
    ...config.resources.map((r) => ({
      label: r.label,
      href: trailUrl(
        openFrom(trail, -1, resourceStop(r.name)),
      ),
      active: false,
    })),
    ...pathEntries(trail, paths),
  ];
  const body: ReadonlyArray<Flow<never>> = [
    // The page's h1 — the corpus masthead. The colHead above repeats the
    // title as the column's sticky header, but a page still owes
    // assistive tech exactly one top-level heading, and this is it.
    h1([], [text(title)]),
    // What is on screen, and what it is a page OF. Both numbers, because
    // either alone misleads: `20 of 33` did not say the other 13 existed to
    // be reached, and a bare `1-20` does not say when to stop.
    p([class_("count")], [text(countLabel)]),
    // The facets: a tag group is a dimension, and each of its variations is
    // one link. This is the "non-tree" half of the mission — the corpus
    // navigated by what a document IS, not by where it sits.
    //
    // An APPLIED value links to its own removal rather than to itself. A
    // link that re-applies what is already on is a link that does nothing,
    // and without a way off, drilling in would be one-way.
    ...groups.map((group) =>
      section(
        [class_("facet")],
        [
          h2([], [text(group.label)]),
          ul(
            [],
            group.values.map((value) =>
              li(
                [],
                [
                  a(
                    [
                      class_(
                        isActive(group.key, value)
                          ? "facet-value active"
                          : "facet-value",
                      ),
                      // The chip truncates at 18rem, so the untruncated
                      // value has to survive somewhere: a `depends_on`
                      // filename is unreadable at any width, and an
                      // ellipsis with no way back is worse than a wrap.
                      //
                      // `attr`, not plgg-view's `title` — that one is the
                      // `<title>` ELEMENT, and reaching for it by name is a
                      // compile error rather than a page that silently grew
                      // a head tag inside a link. The closed element/
                      // attribute types earning their keep.
                      attr(
                        "title",
                        `${group.key}: ${value}`,
                      ),
                      href(
                        isActive(group.key, value)
                          ? urlWith({
                              [group.key]: null,
                              offset: null,
                            })
                          : urlWith({
                              [group.key]: value,
                              // Narrowing changes the set, so page 3 of the
                              // old set is meaningless in the new one.
                              offset: null,
                            }),
                      ),
                    ],
                    [
                      text(
                        isActive(group.key, value)
                          ? `× ${value} (${group.counts[value] ?? 0})`
                          : `${value} (${group.counts[value] ?? 0})`,
                      ),
                    ],
                  ),
                ],
              ),
            ),
          ),
        ],
      ),
    ),
    // The pager. `listCollection` has always taken limit/offset and
    // `ListResult` has always carried the total, so the documents past the
    // first page were reachable by the model and not by the screen — the
    // count said `20 of 33` and offered nothing to click. That is worse than
    // an honest omission, because the number tells you they are there.
    //
    // Both links carry the whole current state (`cols` and every facet),
    // which is the trap this column already fell into once: a pager that
    // drops the filter pages a different corpus than the one on screen.
    ...(hasPrev || hasNext
      ? [
          nav(
            [class_("pager")],
            [
              ...(hasPrev
                ? [
                    a(
                      [
                        class_("prev"),
                        href(
                          urlWith({
                            offset:
                              prevOffset === 0
                                ? null
                                : String(
                                    prevOffset,
                                  ),
                          }),
                        ),
                      ],
                      [text("← previous")],
                    ),
                  ]
                : []),
              ...(hasNext
                ? [
                    a(
                      [
                        class_("next"),
                        href(
                          urlWith({
                            offset:
                              String(nextOffset),
                          }),
                        ),
                      ],
                      [text("next →")],
                    ),
                  ]
                : []),
            ],
          ),
        ]
      : []),
    // Resources, listed BESIDE the documents — which is what "alongside"
    // asked for. A separate section because they are a separate kind of
    // thing: one is markdown this server indexed, the other is a live table
    // qfs answers when you ask. Merging the two lists would say they were
    // the same, and the reader would learn otherwise by clicking.
    ...(config.resources.length === 0
      ? []
      : [
          section(
            [class_("resources")],
            [
              h2([], [text("Resources")]),
              ul(
                [],
                config.resources.map((r) =>
                  li(
                    [],
                    [
                      a(
                        [
                          href(
                            trailUrl(
                              openFrom(
                                trail,
                                -1,
                                resourceStop(
                                  r.name,
                                ),
                              ),
                            ),
                          ),
                        ],
                        [text(r.label)],
                      ),
                    ],
                  ),
                ),
              ),
            ],
          ),
        ]),
    // The door into generic browsing: any path qfs can describe, entered
    // by hand. A GET form, because the resulting screen must BE an
    // address — the form redirects to the trail URL with the new segment
    // appended (`GET /qfs`), and from there everything is a link like
    // everywhere else. Declared resources above remain the curated list;
    // this is the undeclared rest, reachable because the mission says
    // every connected resource is at least browsable.
    section(
      [class_("qfs-browse")],
      [
        h2([], [text("qfs")]),
        form(
          [
            attr("method", "get"),
            attr("action", "/qfs"),
          ],
          [
            input(
              [
                attr("type", "hidden"),
                attr("name", "cols"),
                attr("value", formatTrail(trail)),
              ],
              [],
            ),
            input(
              [
                attr("type", "text"),
                attr("name", "path"),
                attr(
                  "placeholder",
                  "/local/… — a qfs path",
                ),
              ],
              [],
            ),
            button(
              [attr("type", "submit")],
              [text("browse")],
            ),
          ],
        ),
      ],
    ),
    section(
      [class_("documents")],
      [
        h2([], [text("Documents")]),
        ul(
          [],
          documents.map((d) =>
            li(
              [],
              [
                a(
                  [
                    href(
                      trailUrl(
                        openFrom(
                          trail,
                          -1,
                          docStop(d.content.path),
                        ),
                      ),
                    ),
                  ],
                  [
                    text(
                      documentPathString(
                        d.content.path,
                      ),
                    ),
                  ],
                ),
              ],
            ),
          ),
        ),
      ],
    ),
    // The two axes qfs declares about itself, kept APART and in the
    // developer's own order: the paths you query, then the drivers and what
    // connections each has.
    //
    // BELOW the documents, deliberately. The corpus is what the reader came
    // for, and this column already has a queued ticket against it for burying
    // that (20260716130216 — "the corpus column buries what you came for").
    // Two more sections above the document list would compound a known
    // problem; the ordering of what was already here is left exactly as it
    // was, so that ticket still finds the layout it was written against.
    ...pathsSection(trail, paths),
    ...connectionsSection(drivers),
    // The corpus's failures go LAST, and that placement is the fix rather
    // than a detail. This section rendered BEFORE the document list, so the
    // page announced `1-8 of 8 document(s)`, showed seven facet groups and an
    // error, and put the eight documents below all of it — off-screen at
    // 1400x900. The reader had to scroll past what went wrong to reach what
    // went right.
    //
    // It still cannot be dropped: a document that silently never indexed is
    // exactly the bug a reader needs told about, and the first cut of this
    // column omitted it (the spec caught that). `errorCount` is routinely
    // non-zero and is not a fault (see CLAUDE.md), so this states what is
    // wrong without implying the corpus is broken. Last is not hidden.
    ...(indexErrors(index).length === 0
      ? []
      : [
          section(
            [class_("errors")],
            [
              h2(
                [],
                [
                  text(
                    `${indexErrors(index).length} document(s) with unreadable front matter`,
                  ),
                ],
              ),
              ul(
                [],
                indexErrors(index).map((e) =>
                  li(
                    [],
                    [
                      text(
                        `${e.content.path} — ${e.content.message}`,
                      ),
                    ],
                  ),
                ),
              ),
            ],
          ),
        ]),
  ];
  const level = rootLevel(title, entries);
  return {
    level,
    // The corpus takes its landmark from the SAME rule as every other column
    // ({@link paneFor}) rather than a hardcoded `navPane`. At the root it is
    // the only column, so it is what the reader came for and it is the page's
    // `main`; once anything is open it steps back to the `nav` its MenuLevel
    // kind declares. Hardcoding `navPane` here is what made the root page
    // render zero `main` landmarks while the comment two hundred lines up
    // claimed the deepest column was always `main`.
    html: column<never>(
      ["corpus-col", basis("22rem")],
      [
        paneFor(
          level,
          trail.length === 0,
          [],
          [
            colHead<never>({
              title,
              close: none(),
              links: [],
            }),
            div(
              [
                class_(
                  "column-body column-corpus",
                ),
              ],
              body,
            ),
          ],
        ),
      ],
    ),
  };
};

// The sheet. Inline, because ADR 0001 forbids a CSS toolchain as much as any
// other dependency — and a tool that runs at a repository root with no build
// step cannot have one.
//
// The strip's chrome is the ENGINE's: `schemeCss` binds every color meaning
// as a `--pm-*` variable (dark is the `html.dark` class the appearance
// bootstrap sets, not a second sheet), `metricCss` binds the dimension
// tokens, and `chromeCss` paints the column strip itself — the sticky
// `pm-colhead` headers, per-column vertical scroll above the snap
// breakpoint, and the below-snap one-column-per-swipe scroll-snap strip.
// The app rules come AFTER the engine's, because the engine's row contract
// deliberately leaves them to the consumer: who owns horizontal overflow,
// what pads a column body, which surfaces the corpus gets.
//
// The hand-built palette (`domain/model/Palette.ts`) retired with the
// hand-built strip: the engine's theme is the one vocabulary now, so
// `ink("blurple")` stays a compile error exactly as before.
//
// The layout numbers stay literal on purpose: a metric scale is a token
// vocabulary nothing here has earned yet (workaholic:design /
// sacrificial-architecture) — except the rail, which is the ENGINE's own
// metric, because chromeCss sizes the strip to 100vh minus exactly it.
//
// NO BACKTICKS ANYWHERE BELOW. This is a template literal: one backtick ends it
// early and `tsc` reports the syntax error rules further down, where the cause
// is not. That has now happened twice, the second time in the comment warning
// about the first.
const theme = pragmaticTheme;
const ink = colorVar(theme);
const metric = metricVar(theme);
const APP_STYLE = `
  body { margin: 0; font: 14px/1.5 system-ui, sans-serif; background: ${ink("surface")}; color: ${ink("text")}; }
  a { color: ${ink("text")}; }
  /* The strip OWNS horizontal scrolling at every width: depth grows the
     strip's own scroll width, never the page body's — the measured
     reference shape (docs/plggmatic-semantics/poc-findings.md). The engine
     row leaves overflow to the consumer above the snap breakpoint; this is
     that consumer part, and it must come after chromeCss to win the
     cascade. */
  .pm-row { overflow-x: auto; }
  /* The rail above the strip: the breadcrumb trail the engine folds out of
     the Scene. Its height is the engine's own rail metric — chromeCss
     sizes the strip to 100vh minus exactly this. */
  .strip-rail { box-sizing: border-box; height: ${metric("rail")}; display: flex; align-items: center; padding: 0 0.75rem; background: ${ink("surface-2")}; border-bottom: 1px solid ${ink("border")}; }
  .corpus-col { background: ${ink("surface-2")}; }
  .column-body { padding: 1rem 1.5rem; }
  .facet ul, .documents ul { list-style: none; padding: 0; margin: 0 0 1rem; }
  .facet li { display: inline-block; margin: 0 .4rem .3rem 0; }
  .facet h2 { font: 11px/1.4 ui-monospace, monospace; color: ${ink("muted")}; text-transform: uppercase; letter-spacing: .04em; margin: 0 0 .4rem; }
  /* A facet is a control, so it has to look like one: a target you can hit and
     a state you can read. These were bare links with no rule at all — the
     behaviour (AND-ing, removal) shipped and the affordance did not, which is
     how you get a control nobody can see is a control.
     A value can be arbitrarily long (depends_on carried whole filenames), and an
     un-truncated chip wraps and eats the column; the full value lives in the
     title attribute, so the ellipsis costs nothing a hover cannot recover. */
  .facet-value { display: inline-block; max-width: 18rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; vertical-align: bottom; padding: .1rem .4rem; border: 1px solid ${ink("border")}; border-radius: 3px; background: ${ink("surface")}; color: ${ink("text")}; text-decoration: none; }
  .facet-value:hover { border-color: ${ink("primary-border")}; }
  /* APPLIED, and it must be obvious which: this one link REMOVES the filter
     instead of adding one, and a reader who cannot tell them apart cannot tell
     what the list in front of them is. primary is the role that means "the
     thing you are doing", which is exactly what an applied filter is. */
  .facet-value.active { background: ${ink("primary-base")}; border-color: ${ink("primary-base")}; color: ${ink("surface")}; }
  /* The corpus. Paths are long and monospace, so they wrap rather than
     truncate: a path is the document's IDENTITY here (there is no title —
     Query.ts says why), and half an identity is not one. */
  .documents li { font: 12px/1.6 ui-monospace, monospace; overflow-wrap: anywhere; margin: 0 0 .2rem; }
  .documents a { text-decoration: none; }
  .documents a:hover { text-decoration: underline; }
  /* The pager. The count says 37 documents exist and this is the only thing
     that makes the other 17 reachable, so it is not decoration — it is the
     difference between an honest number and a promise the page cannot keep.
     space-between puts prev left and next right even when only one exists. */
  .pager { display: flex; justify-content: space-between; gap: 1rem; margin: 0 0 1rem; }
  .pager a { padding: .2rem .5rem; border: 1px solid ${ink("border")}; border-radius: 3px; background: ${ink("surface")}; color: ${ink("text")}; text-decoration: none; font-size: 12px; }
  .pager a:hover { border-color: ${ink("primary-border")}; }
  .pager .next { margin-left: auto; }
  .resources ul { list-style: none; padding: 0; margin: 0 0 1rem; }
  /* The links table's section under a document body (collection mode only).
     Monospace like the document list, because a target is a path and a path
     is an identity; the section context is muted because it locates the
     link rather than being one. */
  .doc-links { border-top: 1px solid ${ink("border")}; margin-top: 1rem; padding-top: .75rem; }
  .doc-links h2 { font: 11px/1.4 ui-monospace, monospace; color: ${ink("muted")}; text-transform: uppercase; letter-spacing: .04em; margin: 0 0 .4rem; }
  .doc-links ul { list-style: none; padding: 0; margin: 0; }
  .doc-links li { font: 12px/1.6 ui-monospace, monospace; overflow-wrap: anywhere; margin: 0 0 .2rem; }
  .doc-links .link-section { color: ${ink("muted")}; }
  /* The qfs door. A form is a control like the facets are: a visible target,
     monospace because a path is an identity. The input grows with the
     column, the button stays a button. */
  .qfs-browse form { display: flex; gap: .4rem; margin: 0 0 1rem; }
  .qfs-browse h2 { font: 11px/1.4 ui-monospace, monospace; color: ${ink("muted")}; text-transform: uppercase; letter-spacing: .04em; margin: 0 0 .4rem; }
  .qfs-browse input[type="text"] { flex: 1; min-width: 0; font: 12px/1.5 ui-monospace, monospace; padding: .2rem .4rem; border: 1px solid ${ink("border")}; border-radius: 3px; background: ${ink("surface")}; color: ${ink("text")}; }
  .qfs-browse button { font-size: 12px; padding: .2rem .5rem; border: 1px solid ${ink("border")}; border-radius: 3px; background: ${ink("surface")}; color: ${ink("text")}; }
  .qfs-browse button:hover { border-color: ${ink("primary-border")}; }
  /* The two axes qfs declares. Monospace like the document list, because a
     path is an identity and half an identity is not one. Both headings take
     the same section-label rule as the facets, so a reader learns one
     vocabulary rather than one per section (workaholic:design /
     interaction-design-standard). */
  .qfs-paths h2, .qfs-connections h2 { font: 11px/1.4 ui-monospace, monospace; color: ${ink("muted")}; text-transform: uppercase; letter-spacing: .04em; margin: 0 0 .4rem; }
  .qfs-paths ul, .qfs-connections ul { list-style: none; padding: 0; margin: 0 0 1rem; }
  .qfs-paths li, .qfs-connections li { font: 12px/1.6 ui-monospace, monospace; overflow-wrap: anywhere; margin: 0 0 .2rem; }
  .qfs-paths a { text-decoration: none; }
  .qfs-paths a:hover { text-decoration: underline; }
  /* The driver and the account LOCATE a path rather than being it, so they are
     muted — the same rule the links table's section context follows. The path
     is the link; these are not. */
  .path-driver, .path-account, .path-alias, .driver-connections { color: ${ink("muted")}; }
  /* Axis 2 has no links, and must not look like it does: a driver name is a
     label, so it reads as one. A control that does nothing is worse than
     absent (workaholic:design / no-dark-patterns), and a name styled like a
     link is a control that does nothing. */
  .driver-name { color: ${ink("text")}; }
  .column-resource table { border-collapse: collapse; font: 12px/1.5 ui-monospace, monospace; }
  .column-resource th, .column-resource td { border: 1px solid ${ink("border")}; padding: .25rem .5rem; text-align: left; }
  .column-resource th { background: ${ink("surface-2")}; }
  .resource-query { font: 11px/1.4 ui-monospace, monospace; color: ${ink("muted")}; overflow-wrap: anywhere; }
  .resource-error { color: ${ink("danger-text")}; }
  .count { color: ${ink("muted")}; }
  /* The corpus's failures. danger is a ROLE, so the dark scheme re-inks it
     without this rule knowing a scheme exists. */
  .errors { border-left: 2px solid ${ink("danger-border")}; background: ${ink("danger-surface")}; padding: .5rem .75rem; }
  .errors h2 { font-size: 13px; margin: 0 0 .4rem; color: ${ink("danger-text")}; }
  .errors ul { list-style: none; padding: 0; margin: 0; }
  .errors li { font: 12px/1.5 ui-monospace, monospace; color: ${ink("danger-text")}; overflow-wrap: anywhere; }
  pre { overflow-x: auto; background: ${ink("surface-2")}; padding: .6rem; }
`;
const STYLE = [
  schemeCss(theme),
  metricCss(theme),
  chromeCss(theme),
  reducedMotionCss,
  APP_STYLE,
].join("\n");

// The one lowering from trail to columns. Both routes below render through
// this — /resolve and the bare root are two doors into ONE renderer, never
// two renderers (`workaholic:design` / `sacrificial-architecture`). The
// renderer is the ENGINE's strip: engine columns in an engine row, and the
// whole trail lowered to ONE Scene whose crumbs the engine folds.
const columnsPage = (
  index: Index,
  trail: Trail,
  params: Readonly<Record<string, string>>,
  config: Config,
  runner: ResourceRunner,
): HttpResponse => {
  const title = matchOption<SoftStr, SoftStr>(
    () => "qfs-viewer",
    (t) => t,
  )(config.title);
  const root = rootColumn(
    index,
    trail,
    params,
    config,
    runner,
  );
  // One column per stop, and the branch is the whole point: a document, a
  // declared qfs resource, and a walked qfs path are different things, so
  // each gets its own column rather than one pretending to be another.
  const opened: ReadonlyArray<StripColumn> =
    trail.map((stop, depth) =>
      stop.__tag === "Doc"
        ? documentColumn(
            index,
            trail,
            depth,
            stop.path,
            // The links table speaks only where a collection is declared —
            // the legacy corpus has no such table, and asking qfs for one
            // that was never bound would put a spurious error on every
            // document column.
            matchOption<
              SoftStr,
              Option<
                Result<
                  ReadonlyArray<CollectionLink>,
                  InvalidError
                >
              >
            >(
              () => none(),
              (name) =>
                some(
                  documentLinks(
                    runner,
                    name,
                    documentPathString(stop.path),
                  ),
                ),
            )(config.collection),
          )
        : stop.__tag === "Resource"
          ? resourceColumn(
              runner,
              config,
              trail,
              depth,
              stop.name,
            )
          : qfsColumn(
              runner,
              trail,
              depth,
              stop.path,
            ),
    );
  const scene = sceneOf(title, [
    root.level,
    ...opened.map((c) => c.level),
  ]);
  return pageResponse({
    title,
    // `slot`, not `div`: the engine's row is an open-tagged Html, which
    // the closed Flow children of `div` refuse — slot is plgg-view's own
    // container for exactly that composition (the engine uses it too).
    root: slot(
      [attr("class", "strip")],
      [
        // `HtmlDocumentOptions` has no `head`; plgg-server folds the
        // root's own css atoms into one. This sheet is not per-element
        // styling, so it rides in through the same `raw` seam — it is
        // ours, not the corpus's.
        raw(`<style>${STYLE}</style>`),
        // The engine's appearance bootstrap: html.dark from storage or
        // the media query, before first paint. Presentation only — the
        // address holds everything navigable, and this holds nothing.
        raw(
          `<script>${appearanceInitScript}</script>`,
        ),
        div(
          [class_("strip-rail")],
          [breadcrumb<never>(crumbsOf(scene))],
        ),
        row<never>(
          [],
          [
            root.html,
            ...opened.map((c) => c.html),
          ],
        ),
      ],
    ),
  });
};

// 308, not 303: the legacy spelling MOVED, permanently — a redirect after a
// form POST (the 303s elsewhere) says "see another resource", which is not
// this. The `noStore` middleware still stamps the response, so no cache can
// pin the old address to one answer (docs/adr/0003).
const movedTo = (
  location: string,
): HttpResponse =>
  textResponse("", statusOf(308), { location });

/**
 * `GET /resolve/<trail>` — the canonical address: the corpus list plus one
 * column per address prefix (docs/adr/0007).
 *
 * The whole screen is a function of the address and the index. Nothing is
 * remembered between requests, so two people at the same address see the same
 * thing and a reload is not a navigation.
 *
 * The trail is parsed from the RAW path, never from the router's
 * pre-decoded capture — see `parseResolvePath` for why. An address whose
 * every segment dropped out is the empty trail, and the empty trail's
 * canonical spelling is `/`, so it redirects rather than rendering a second
 * address for the same screen.
 */
export const resolveHandler =
  (
    ref: IndexRef,
    config: Config = defaultConfig,
    runner: ResourceRunner = refusingRunner,
  ) =>
  (
    c: Context,
  ): PromisedResult<HttpResponse, HttpError> => {
    const trail = parseResolvePath(c.req.path);
    if (trail.length === 0) {
      return Promise.resolve(
        ok(
          movedTo(
            addressOf(
              [],
              paramsSansCols(c.req.query),
            ),
          ),
        ),
      );
    }
    // Read the index ONCE: every column is rendered from this one value, so
    // a reload mid-request cannot show column 1 from the old corpus and
    // column 2 from the new.
    return Promise.resolve(
      ok(
        columnsPage(
          ref.current(),
          trail,
          c.req.query,
          config,
          runner,
        ),
      ),
    );
  };

/**
 * `GET /` — the corpus list with nothing open.
 *
 * A request still carrying the legacy `?cols=` trail is answered with a
 * permanent redirect to the canonical `/resolve` address, filters and all —
 * the old bookmarks keep working, and exactly one serialization of the
 * trail remains in circulation (docs/adr/0007).
 */
export const columnsHandler =
  (
    ref: IndexRef,
    config: Config = defaultConfig,
    runner: ResourceRunner = refusingRunner,
  ) =>
  (
    c: Context,
  ): PromisedResult<HttpResponse, HttpError> => {
    const legacy = fromNullable(
      c.req.query["cols"],
    );
    if (legacy.__tag === "Some") {
      return Promise.resolve(
        ok(
          movedTo(
            addressOf(
              parseTrail(legacy),
              paramsSansCols(c.req.query),
            ),
          ),
        ),
      );
    }
    return Promise.resolve(
      ok(
        columnsPage(
          ref.current(),
          [],
          c.req.query,
          config,
          runner,
        ),
      ),
    );
  };
