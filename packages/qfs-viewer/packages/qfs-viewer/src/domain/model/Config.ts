// `qfs-viewer.config.json` — what a repository declares about itself.
//
// OPTIONAL, and that is the product's premise rather than a convenience: `npx
// qfs-viewer` at a repository root must work with no build step and no
// central configuration. A repository with no config gets the discovered
// behaviour, which is what every repository got before this file existed. The
// config REFINES; it is never required to make the browser work.
//
// JSON, not TypeScript. plggpress loads a `site.config.ts` through a dynamic
// import, which is right for plggpress — its consumers are TypeScript projects
// with a toolchain. This runs at ANY repository root, including ones with no
// TypeScript, no bundler and no `node_modules`, so a config that must be
// compiled to be read would exclude most of the corpora this exists to browse.
// `JSON.parse` is the stdlib and reads everywhere.
//
// The mission asks this file to drive "layout and classification". It does
// both, and nothing else yet: `title` is layout, `tagGroups`/`hide` are
// classification. Fields are added when something needs them — this package
// has already deleted one round of exported-but-uncallable API.
import {
  type SoftStr,
  type Option,
  type Result,
  type InvalidError,
  invalidError,
  ok,
  err,
  some,
  none,
} from "plgg";
import {
  type Principal,
  asPrincipal,
} from "#qfs-viewer/domain/model/Principal";
import {
  type ResourceConfig,
  asResourceConfig,
} from "#qfs-viewer/domain/model/Resource";
import {
  type QfsConnection,
  asQfsConnection,
  defaultConnection,
} from "#qfs-viewer/domain/model/Connection";
import { asCollectionName } from "#qfs-viewer/domain/model/Collection";

/**
 * One declared dimension.
 *
 * `label` is what the facet heading reads; absent, the key speaks for itself
 * (`layer` is a fine heading). `variations`, when present, FIXES the order and
 * the membership of that dimension's values — a declared taxonomy, where the
 * discovered one is whatever the corpus happens to hold today.
 */
export type TagGroupConfig = Readonly<{
  key: SoftStr;
  label: Option<SoftStr>;
  variations: Option<ReadonlyArray<SoftStr>>;
}>;

/** What `qfs-viewer.config.json` may say. */
export type Config = Readonly<{
  /** The name at the top of the corpus column. */
  title: Option<SoftStr>;
  /**
   * The dimensions to show, in this order, before any discovered ones.
   * Empty means "no opinion" — every dimension is discovered.
   */
  tagGroups: ReadonlyArray<TagGroupConfig>;
  /**
   * Front-matter keys that are NOT dimensions. Replaces the built-in list
   * when present, so a corpus whose `title` really is a tag can say so.
   */
  hide: Option<ReadonlyArray<SoftStr>>;
  /**
   * Who may use this server, and how.
   *
   * EMPTY MEANS OPEN, which is the product's headline case: `npx
   * qfs-viewer` at your own repository root needs no token, and demanding
   * one would be theatre performed for an audience of one. Declaring a
   * principal is the moment a corpus stops being one developer's local tree,
   * and that is when access control turns on.
   */
  principals: ReadonlyArray<Principal>;
  /**
   * qfs resources to surface beside the markdown.
   *
   * DECLARED, never discovered. qfs reaches mail, databases and cloud
   * accounts; enumerating it onto a page would make a knowledge browser an
   * exfiltration tool its own repository never asked for. A resource appears
   * because someone wrote it down, and their statement is the whole of what
   * may run.
   */
  resources: ReadonlyArray<ResourceConfig>;
  /**
   * How this viewer reaches qfs — one of the plan's three issuance forms.
   * Absent means "spawn the qfs on PATH per query" (form ②), which is what
   * lets `npx qfs-viewer` work with no daemon and no config. See
   * `domain/model/Connection.ts`.
   */
  qfs: QfsConnection;
  /**
   * THE switch between the two corpus sources (docs/adr/0008). Present, it
   * names the qfs markdown tree (`CONNECT /markdown/<name> TO markdown AT
   * '<root>'`) this corpus is served FROM: the in-process scanner and
   * watcher are never constructed, and enumeration plus front-matter
   * interpretation are qfs's alone. Absent, the legacy scan serves — until
   * its recorded retirement date (2026-07-31, docs/adr/0008), after which
   * the scan path is deleted rather than kept as a parallel truth.
   */
  collection: Option<SoftStr>;
}>;

/** The config a repository with no config file gets. */
export const defaultConfig: Config = {
  title: none(),
  tagGroups: [],
  hide: none(),
  principals: [],
  resources: [],
  qfs: defaultConnection,
  collection: none(),
};

const isRecord = (
  v: unknown,
): v is Readonly<Record<string, unknown>> =>
  typeof v === "object" &&
  v !== null &&
  !Array.isArray(v);

const stringsOf = (
  value: unknown,
  field: SoftStr,
): Result<
  ReadonlyArray<SoftStr>,
  InvalidError
> =>
  !Array.isArray(value)
    ? err(
        invalidError({
          message: `${field} must be an array of strings`,
        }),
      )
    : value.every((v) => typeof v === "string")
      ? ok(value)
      : err(
          invalidError({
            message: `${field} must contain only strings`,
          }),
        );

const asTagGroupConfig = (
  value: unknown,
  at: number,
): Result<TagGroupConfig, InvalidError> => {
  if (!isRecord(value)) {
    return err(
      invalidError({
        message: `tagGroups[${at}] must be an object`,
      }),
    );
  }
  const key: unknown = value["key"];
  if (typeof key !== "string" || key === "") {
    return err(
      invalidError({
        message: `tagGroups[${at}].key must be a non-empty string`,
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
        message: `tagGroups[${at}].label must be a string`,
      }),
    );
  }
  const rawVariations: unknown =
    value["variations"];
  if (rawVariations === undefined) {
    return ok({
      key,
      label:
        rawLabel === undefined
          ? none()
          : some(rawLabel),
      variations: none(),
    });
  }
  const variations = stringsOf(
    rawVariations,
    `tagGroups[${at}].variations`,
  );
  return variations.__tag === "Err"
    ? err(variations.content)
    : ok({
        key,
        label:
          rawLabel === undefined
            ? none()
            : some(rawLabel),
        variations: some(variations.content),
      });
};

/**
 * Validate an untrusted parsed JSON value into a {@link Config}.
 *
 * REJECTS rather than repairs. A config is a thing a person wrote on purpose,
 * so a typo in it is a question they want answered, not a field to quietly
 * drop — the same reason `parseListQuery` refuses a bad `limit` instead of
 * defaulting it. The one exception is the FILE's absence, which is not an
 * error at all and never reaches here.
 *
 * Unknown top-level keys are allowed through in silence, deliberately: this is
 * a config format that will grow, and a repository that pins an older
 * qfs-viewer should not fail to start because it mentions a field the
 * older build has not learned yet.
 */
export const asConfig = (
  value: unknown,
): Result<Config, InvalidError> => {
  if (!isRecord(value)) {
    return err(
      invalidError({
        message:
          "qfs-viewer.config.json must contain a JSON object",
      }),
    );
  }
  const rawTitle: unknown = value["title"];
  if (
    rawTitle !== undefined &&
    typeof rawTitle !== "string"
  ) {
    return err(
      invalidError({
        message: "title must be a string",
      }),
    );
  }
  const rawHide: unknown = value["hide"];
  if (rawHide !== undefined) {
    const hide = stringsOf(rawHide, "hide");
    if (hide.__tag === "Err") {
      return err(hide.content);
    }
  }
  const rawPrincipals: unknown =
    value["principals"];
  if (
    rawPrincipals !== undefined &&
    !Array.isArray(rawPrincipals)
  ) {
    return err(
      invalidError({
        message: "principals must be an array",
      }),
    );
  }
  const principals: Array<Principal> = [];
  for (const [at, raw] of (
    rawPrincipals ?? []
  ).entries()) {
    const parsed = asPrincipal(raw, at);
    if (parsed.__tag === "Err") {
      return err(parsed.content);
    }
    principals.push(parsed.content);
  }
  const rawResources: unknown =
    value["resources"];
  if (
    rawResources !== undefined &&
    !Array.isArray(rawResources)
  ) {
    return err(
      invalidError({
        message: "resources must be an array",
      }),
    );
  }
  const resources: Array<ResourceConfig> = [];
  for (const [at, raw] of (
    rawResources ?? []
  ).entries()) {
    const parsed = asResourceConfig(raw, at);
    if (parsed.__tag === "Err") {
      return err(parsed.content);
    }
    resources.push(parsed.content);
  }
  const rawQfs: unknown = value["qfs"];
  const qfs =
    rawQfs === undefined
      ? undefined
      : asQfsConnection(rawQfs);
  if (qfs !== undefined && qfs.__tag === "Err") {
    return err(qfs.content);
  }
  const rawCollection: unknown =
    value["collection"];
  const collection =
    rawCollection === undefined
      ? undefined
      : asCollectionName(rawCollection);
  if (
    collection !== undefined &&
    collection.__tag === "Err"
  ) {
    return err(collection.content);
  }
  const rawGroups: unknown = value["tagGroups"];
  if (
    rawGroups !== undefined &&
    !Array.isArray(rawGroups)
  ) {
    return err(
      invalidError({
        message: "tagGroups must be an array",
      }),
    );
  }
  const groups: Array<TagGroupConfig> = [];
  for (const [at, raw] of (
    rawGroups ?? []
  ).entries()) {
    const parsed = asTagGroupConfig(raw, at);
    if (parsed.__tag === "Err") {
      return err(parsed.content);
    }
    groups.push(parsed.content);
  }
  const hideResult =
    rawHide === undefined
      ? undefined
      : stringsOf(rawHide, "hide");
  return ok({
    title:
      rawTitle === undefined
        ? none()
        : some(rawTitle),
    tagGroups: groups,
    principals,
    resources,
    qfs:
      qfs === undefined
        ? defaultConnection
        : qfs.content,
    collection:
      collection === undefined
        ? none()
        : some(collection.content),
    hide:
      hideResult === undefined ||
      hideResult.__tag === "Err"
        ? none()
        : some(hideResult.content),
  });
};
