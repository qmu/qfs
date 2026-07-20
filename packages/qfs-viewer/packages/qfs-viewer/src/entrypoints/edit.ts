// `GET /edit/<path>` and `POST /edit/<path>` — editing a document in place.
//
// The mission: "documents are browsable *and editable* in place — no build
// step, no database, no central configuration."
//
// A plain HTML form with NO client JavaScript, like everything else here. A
// textarea and a submit button is the whole editor, and that is not a
// placeholder for a real one — it means the edit surface works in a terminal
// browser, over `curl`, and with a screen reader, and that it cannot get out
// of sync with the URL because it has no state of its own to lose.
//
// POST/REDIRECT/GET: the save answers 303 rather than rendering. A rendered
// POST response means a reload re-submits the form, which would silently
// overwrite a newer edit with an older one — the classic double-submit, which
// on a knowledge base is data loss rather than a duplicate order.
import {
  isOk,
  ok,
  err,
  fromNullable,
  matchOption,
  type PromisedResult,
} from "plgg";
import {
  pageResponse,
  textResponse,
  parseForm,
  notFound,
  statusOf,
  type Context,
  type HttpResponse,
  type HttpError,
} from "plgg-server";
import {
  div,
  h1,
  p,
  a,
  form,
  textarea,
  button,
  text,
  href,
  attr,
  class_,
  raw,
} from "plgg-view";
import {
  type FileWriter,
  validateEdit,
} from "#qfs-viewer/domain/model/Editor";
import { getDocument } from "#qfs-viewer/domain/model/Index";
import {
  type IndexRef,
  applyChange,
} from "#qfs-viewer/domain/usecase/reload";
import { type FileSystem } from "#qfs-viewer/domain/model/Scan";
import {
  type Trail,
  parseTrail,
  trailUrl,
  formatTrail,
} from "#qfs-viewer/domain/model/Trail";
import {
  asDocumentPath,
  documentPathString,
  type DocumentPath,
} from "#qfs-viewer/domain/model/Vocabulary";

const STYLE = `
  body { margin: 0; font: 14px/1.5 system-ui, sans-serif; }
  .edit { max-width: 60rem; margin: 0 auto; padding: 1.5rem; }
  .edit textarea { width: 100%; min-height: 70vh; font: 13px/1.6 ui-monospace, monospace; padding: .8rem; }
  .edit .path { font: 12px/1.4 ui-monospace, monospace; color: #666; }
  .edit .actions { display: flex; gap: 1rem; align-items: center; margin-top: .8rem; }
  .edit .error { color: #a33; }
`;

// The edit form posts to its own URL and carries the trail through, so saving
// returns you to the columns you were reading rather than to the root.
const editPage = (
  path: DocumentPath,
  source: string,
  trail: Trail,
  message: string,
): HttpResponse => {
  const pathText = documentPathString(path);
  const back =
    trail.length === 0 ? "/" : trailUrl(trail);
  const action =
    trail.length === 0
      ? `/edit/${pathText}`
      : `/edit/${pathText}?cols=${formatTrail(trail)}`;
  return pageResponse({
    title: `edit ${pathText}`,
    root: div(
      [class_("edit")],
      [
        raw(`<style>${STYLE}</style>`),
        h1([], [text("Edit")]),
        p([class_("path")], [text(pathText)]),
        ...(message === ""
          ? []
          : [
              p(
                [class_("error")],
                [text(message)],
              ),
            ]),
        form(
          [
            attr("method", "post"),
            attr("action", action),
          ],
          [
            textarea(
              [
                attr("name", "source"),
                attr("spellcheck", "false"),
              ],
              [text(source)],
            ),
            div(
              [class_("actions")],
              [
                button(
                  [attr("type", "submit")],
                  [text("Save")],
                ),
                a([href(back)], [text("Cancel")]),
              ],
            ),
          ],
        ),
      ],
    ),
  });
};

const trailOf = (c: Context): Trail =>
  parseTrail(fromNullable(c.req.query["cols"]));

const pathOf = (
  c: Context,
): DocumentPath | undefined => {
  const raw = c.req.params["path"];
  const parsed = asDocumentPath(
    raw === undefined ? "" : raw,
  );
  return isOk(parsed)
    ? parsed.content
    : undefined;
};

/** `GET /edit/<path>` — the document, in a textarea. */
export const editFormHandler =
  (ref: IndexRef) =>
  (
    c: Context,
  ): PromisedResult<HttpResponse, HttpError> => {
    const path = pathOf(c);
    if (path === undefined) {
      return Promise.resolve(
        err(notFound(c.req.path)),
      );
    }
    const doc = getDocument(ref.current(), path);
    return Promise.resolve(
      doc.__tag === "None"
        ? err(notFound(c.req.path))
        : ok(
            editPage(
              path,
              doc.content.content.source,
              trailOf(c),
              "",
            ),
          ),
    );
  };

/**
 * `POST /edit/<path>` — write it back, and update the index NOW.
 *
 * The index is updated HERE rather than left to the watcher, and that closes a
 * real race. The watcher debounces 50ms before swapping; the 303 sends the
 * browser straight back, so the follow-up GET would arrive first and render
 * the OLD source — the author would watch their save appear not to happen, and
 * reload, and see it. Applying it here means the redirect lands on a corpus
 * that already knows.
 *
 * The watcher still fires afterwards and re-reads the same bytes. That is
 * harmless — the second value equals the first — and it is worth the
 * redundancy: the editor is not the only thing that writes to a working tree,
 * so the watcher must stay the general answer.
 */
export const editSaveHandler =
  (
    ref: IndexRef,
    fs: FileSystem,
    writer: FileWriter,
  ) =>
  async (
    c: Context,
  ): PromisedResult<HttpResponse, HttpError> => {
    const path = pathOf(c);
    if (path === undefined) {
      return err(notFound(c.req.path));
    }
    const doc = getDocument(ref.current(), path);
    if (doc.__tag === "None") {
      return err(notFound(c.req.path));
    }
    const fields = parseForm(c.req.body);
    const submitted = fromNullable(
      fields["source"],
    );
    const source = matchOption<string, string>(
      () => "",
      (v) => v,
    )(submitted);
    const validated = validateEdit(source);
    if (!isOk(validated)) {
      // Re-render the form with the text the author typed still in it. Losing
      // someone's writing to tell them it was invalid would be its own bug.
      return ok(
        editPage(
          path,
          source,
          trailOf(c),
          validated.content.content.message,
        ),
      );
    }
    writer.writeFile(path, validated.content);
    ref.swap(
      applyChange(
        fs,
        ref.current(),
        documentPathString(path),
        "changed",
      ),
    );
    const trail = trailOf(c);
    return ok(
      textResponse("", statusOf(303), {
        location:
          trail.length === 0
            ? `/${documentPathString(path)}`
            : trailUrl(trail),
      }),
    );
  };
