import {
  type SoftStr,
  type Box,
  type Option,
  none,
  some,
  isNone,
  matchOption,
  box,
  match,
  pattern,
} from "plgg";
import { type Declaration } from "plggmatic/Declare/model/Declaration";
import { type Collection } from "plggmatic/Declare/model/Collection";
import { type Registry } from "plggmatic/Declare/model/Adapter";
import {
  type Source,
  sync$,
  async$,
  dynamic$,
  adapter$,
} from "plggmatic/Declare/model/Source";

/**
 * A startup reconciliation finding — the binding check that
 * runs before the first render (point-7 naming decision):
 *
 * - `UnknownAdapter` — a NAMED adapter source the host never
 *   registered (a positioned ERROR, keyed by collection).
 * - `NoDefaultAdapter` — an UNNAMED (default) adapter source
 *   meeting a registry that is not exactly one, so there is
 *   no sole default to resolve (a positioned ERROR).
 * - `UnusedAdapter` — a registration no collection reads
 *   through (a WARNING).
 *
 * v1 has no cross-adapter include/projection to check: each
 * collection reads through exactly ONE adapter (its source's
 * name), so "one read within one adapter" holds structurally
 * — a cross-backend read is the host's business inside a
 * single adapter's `read`.
 */
export type Diagnostic =
  | Box<
      "UnknownAdapter",
      Readonly<{
        collection: SoftStr;
        name: SoftStr;
      }>
    >
  | Box<
      "NoDefaultAdapter",
      Readonly<{
        collection: SoftStr;
        registered: number;
      }>
    >
  | Box<
      "UnusedAdapter",
      Readonly<{ name: SoftStr }>
    >;

/** An unknown named-adapter error. */
export const unknownAdapter = (
  collection: SoftStr,
  name: SoftStr,
): Diagnostic =>
  box("UnknownAdapter")({ collection, name });

/** A missing-default error for an unnamed adapter source. */
export const noDefaultAdapter = (
  collection: SoftStr,
  registered: number,
): Diagnostic =>
  box("NoDefaultAdapter")({
    collection,
    registered,
  });

/** An unused-registration warning. */
export const unusedAdapter = (
  name: SoftStr,
): Diagnostic => box("UnusedAdapter")({ name });

/** Matchers for folding a {@link Diagnostic}. */
export const unknownAdapter$ = () =>
  pattern("UnknownAdapter")();
export const noDefaultAdapter$ = () =>
  pattern("NoDefaultAdapter")();
export const unusedAdapter$ = () =>
  pattern("UnusedAdapter")();

/** A diagnostic's severity — errors block, warnings inform. */
export const diagnosticSeverity = (
  d: Diagnostic,
): "error" | "warning" =>
  match(d)(
    [
      unknownAdapter$(),
      (): "error" | "warning" => "error",
    ],
    [
      noDefaultAdapter$(),
      (): "error" | "warning" => "error",
    ],
    [
      unusedAdapter$(),
      (): "error" | "warning" => "warning",
    ],
  );

/**
 * The adapter name a source binds to, if it is an adapter
 * source: `Some(name)` where `name` is itself `Some(named)`
 * or `None` (the default). A non-adapter source is `None`.
 */
const bindingOf = (
  source: Source,
): Option<Option<SoftStr>> =>
  match(source)(
    [
      sync$(),
      (): Option<Option<SoftStr>> => none(),
    ],
    [
      async$(),
      (): Option<Option<SoftStr>> => none(),
    ],
    [
      dynamic$(),
      (): Option<Option<SoftStr>> => none(),
    ],
    [
      adapter$(),
      ({ content }): Option<Option<SoftStr>> =>
        some(content),
    ],
  );

/** One collection's adapter binding (collection id + name). */
type Binding = Readonly<{
  collection: SoftStr;
  name: Option<SoftStr>;
}>;

/**
 * Reconciles a declaration's adapter sources against the
 * host {@link Registry} — the total startup check. Returns
 * every {@link Diagnostic} (errors and warnings); an empty
 * array means the bindings are sound. Pure: reads no data,
 * runs no adapter.
 */
export const reconcile = (
  declaration: Declaration,
  registry: Registry,
): ReadonlyArray<Diagnostic> => {
  const bindings: ReadonlyArray<Binding> =
    declaration.collections.flatMap(
      (c: Collection) =>
        matchOption<
          Option<SoftStr>,
          ReadonlyArray<Binding>
        >(
          () => [],
          (name: Option<SoftStr>) => [
            { collection: c.id, name },
          ],
        )(bindingOf(c.source)),
    );
  const bindErrors: ReadonlyArray<Diagnostic> =
    bindings.flatMap((b: Binding) =>
      matchOption<
        SoftStr,
        ReadonlyArray<Diagnostic>
      >(
        () =>
          registry.length === 1
            ? []
            : [
                noDefaultAdapter(
                  b.collection,
                  registry.length,
                ),
              ],
        (nm: SoftStr) =>
          registry.some((r) => r.name === nm)
            ? []
            : [unknownAdapter(b.collection, nm)],
      )(b.name),
    );
  const usedNames: ReadonlyArray<SoftStr> =
    bindings.flatMap((b: Binding) =>
      matchOption<
        SoftStr,
        ReadonlyArray<SoftStr>
      >(
        () => [],
        (nm: SoftStr) => [nm],
      )(b.name),
    );
  const defaultUsed =
    registry.length === 1 &&
    bindings.some((b: Binding) => isNone(b.name));
  const unusedWarnings: ReadonlyArray<Diagnostic> =
    registry
      .filter(
        (r) =>
          !usedNames.includes(r.name) &&
          !(registry.length === 1 && defaultUsed),
      )
      .map((r) => unusedAdapter(r.name));
  return [...bindErrors, ...unusedWarnings];
};
