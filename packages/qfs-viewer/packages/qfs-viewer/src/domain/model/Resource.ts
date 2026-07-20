// A qfs resource: something in the corpus that is not markdown.
//
// The mission: "Other qfs resources browsable alongside markdown." The design
// question that decides everything downstream is what such a thing IS in an
// index whose entire model is `Document`, and the answer — read off what qfs
// actually returns rather than guessed — is that it is NOT a document.
//
//   $ qfs run '/local/… |> select name, size, is_dir'
//   {"schema":[{"name":"name","type":"text"},…],
//    "rows":[{"name":"CLAUDE.md","size":2363,"is_dir":false},…],
//    "meta":{"row_count":6,"truncated":false,…}}
//
// That is a TABLE. It has columns with types and rows with values. A markdown
// document is a path and a body. Projecting one into the other would mean
// either rendering the table to markdown text and pretending it was authored
// (losing the schema, and lying about where it came from), or widening
// `Document` until it means "anything", at which point it means nothing.
//
// So `Resource` is its own archetype, and the two live SIDE BY SIDE — which is
// what "alongside" asked for in the first place. The index stays markdown-only
// and keeps its guarantees; a resource is fetched per request, because a live
// table's whole value is being live and caching it would make it a stale copy
// of a thing qfs already holds (docs/adr/0003).
import {
  type SoftStr,
  type Result,
  type InvalidError,
  invalidError,
  ok,
  err,
} from "plgg";

/** One column of a resource's schema, as qfs describes it. */
export type ResourceColumn = Readonly<{
  name: SoftStr;
  type: SoftStr;
}>;

/**
 * A row, as qfs returns it: values keyed by column name.
 *
 * `unknown`, not `SoftStr`. qfs types its columns (`text`, `int`, `bool`,
 * `timestamp`, `bytes`) and returns JSON that matches, so a row holds numbers
 * and booleans as themselves. Flattening them to strings at the boundary would
 * throw away the types qfs went to the trouble of reporting, and the renderer
 * is the only thing that needs text.
 */
export type ResourceRow = Readonly<
  Record<string, unknown>
>;

/** What a qfs query answered. */
export type ResourceTable = Readonly<{
  columns: ReadonlyArray<ResourceColumn>;
  rows: ReadonlyArray<ResourceRow>;
  /** True when qfs stopped early — the reader must be told. */
  truncated: boolean;
}>;

/**
 * A resource a repository has chosen to surface, declared in
 * `qfs-viewer.config.json`.
 *
 * DECLARED, never discovered. qfs is a control plane over mail, databases,
 * cloud accounts and more; enumerating it and putting everything on screen
 * would turn a knowledge browser into an exfiltration tool that its own
 * repository never asked for. A resource appears because someone wrote it
 * down, and the query they wrote is the whole of what it may run.
 */
export type ResourceConfig = Readonly<{
  /** What the reader sees in the list. */
  label: SoftStr;
  /** The slug that addresses it in a URL. */
  name: SoftStr;
  /** The qfs statement to run, verbatim. */
  query: SoftStr;
}>;

// A resource is addressed by name in a URL, so the name has to survive being
// one — and has to be distinguishable from a document path at a glance, which
// is why it is a slug rather than anything with a slash or a dot in it.
const NAME = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

const isRecord = (
  v: unknown,
): v is Readonly<Record<string, unknown>> =>
  typeof v === "object" &&
  v !== null &&
  !Array.isArray(v);

/**
 * Validate one declared resource.
 *
 * The query is NOT parsed or checked here, and that is deliberate: qfs owns
 * that grammar, it reports a structured `parse_error` for a bad statement, and
 * a second opinion in this repository would be a worse copy that drifts. What
 * IS enforced is that the statement exists and the name can be a URL.
 */
export const asResourceConfig = (
  value: unknown,
  at: number,
): Result<ResourceConfig, InvalidError> => {
  if (!isRecord(value)) {
    return err(
      invalidError({
        message: `resources[${at}] must be an object`,
      }),
    );
  }
  const name: unknown = value["name"];
  if (
    typeof name !== "string" ||
    !NAME.test(name)
  ) {
    return err(
      invalidError({
        message: `resources[${at}].name must be a slug (lowercase alphanumerics separated by single hyphens), got ${JSON.stringify(name)}`,
      }),
    );
  }
  const query: unknown = value["query"];
  if (typeof query !== "string" || query === "") {
    return err(
      invalidError({
        message: `resources[${at}].query must be a non-empty qfs statement`,
      }),
    );
  }
  const rawLabel: unknown = value["label"];
  if (
    rawLabel !== undefined &&
    typeof rawLabel !== "string"
  ) {
    return err(
      invalidError({
        message: `resources[${at}].label must be a string`,
      }),
    );
  }
  return ok({
    name,
    query,
    label:
      rawLabel === undefined ? name : rawLabel,
  });
};

/**
 * Read qfs's answer into a {@link ResourceTable}.
 *
 * The input is whatever `qfs run` printed, already JSON-parsed. It is
 * UNTRUSTED in the same sense front matter is: qfs is another program, its
 * output is a boundary, and a shape this does not recognise is a typed failure
 * rather than a crash.
 *
 * qfs's own error shape is understood and reported as itself —
 * `{"error":{"code":"parse_error","message":…}}` — because "the query you
 * declared has a syntax error" is exactly what the person who wrote it needs
 * to read, and flattening it to "could not load" would hide the answer.
 */
export const asResourceTable = (
  value: unknown,
): Result<ResourceTable, InvalidError> => {
  if (!isRecord(value)) {
    return err(
      invalidError({
        message: "qfs did not return an object",
      }),
    );
  }
  const error: unknown = value["error"];
  if (isRecord(error)) {
    const message: unknown = error["message"];
    const code: unknown = error["code"];
    return err(
      invalidError({
        message: `qfs: ${typeof message === "string" ? message : "unknown error"}${typeof code === "string" ? ` (${code})` : ""}`,
      }),
    );
  }
  const schema: unknown = value["schema"];
  const rows: unknown = value["rows"];
  if (!Array.isArray(schema)) {
    return err(
      invalidError({
        message:
          "qfs returned no schema — this query does not answer with rows",
      }),
    );
  }
  if (!Array.isArray(rows)) {
    return err(
      invalidError({
        message: "qfs returned no rows array",
      }),
    );
  }
  const columns: Array<ResourceColumn> = [];
  for (const column of schema) {
    if (
      !isRecord(column) ||
      typeof column["name"] !== "string"
    ) {
      return err(
        invalidError({
          message:
            "qfs returned a schema entry with no name",
        }),
      );
    }
    const type: unknown = column["type"];
    columns.push({
      name: column["name"],
      type:
        typeof type === "string"
          ? type
          : "unknown",
    });
  }
  const meta: unknown = value["meta"];
  return ok({
    columns,
    rows: rows.filter((r) => isRecord(r)),
    truncated:
      isRecord(meta) &&
      meta["truncated"] === true,
  });
};
