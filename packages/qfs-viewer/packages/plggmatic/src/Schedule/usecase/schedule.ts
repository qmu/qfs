import {
  type SoftStr,
  getOr,
  fromNullable,
  pipe,
} from "plgg";
import {
  type Cmd,
  type Url,
} from "plgg-view/client";
import { type Row } from "plggmatic/Declare/model/Row";
import { type Declaration } from "plggmatic/Declare/model/Declaration";
import {
  type Registry,
  type Actor,
} from "plggmatic/Declare/model/Adapter";
import {
  type Diagnostic,
  reconcile,
} from "plggmatic/Declare/usecase/reconcile";
import {
  type Model,
  loadedSlot,
  setSlot,
} from "plggmatic/Schedule/model/Model";
import {
  type SchedulerMsg,
  urlChanged,
} from "plggmatic/Schedule/model/Msg";
import { type Scene } from "plggmatic/Schedule/model/Scene";
import {
  type EngineOptions,
  makeUpdate,
  makeInit,
} from "plggmatic/Schedule/usecase/update";
import {
  parseUrl,
  toUrl,
} from "plggmatic/Schedule/usecase/codec";
import { makeScene } from "plggmatic/Schedule/usecase/scene";

/**
 * The derived TEA program of a declaration — everything
 * plgg-view's `Application<Model, SchedulerMsg>` needs
 * except `view`, plus a typed `scene` projector renderers
 * consume. Complete a runnable program by supplying a
 * renderer `r: (Scene) => Html<SchedulerMsg>`:
 * `{ ...scheduled, view: (m) => r(scheduled.scene(m)) }`.
 */
export type Scheduled = Readonly<{
  init: (
    url: Url,
  ) => readonly [Model, Cmd<SchedulerMsg>];
  update: (
    msg: SchedulerMsg,
    model: Model,
  ) => readonly [Model, Cmd<SchedulerMsg>];
  onUrlChange: (url: Url) => SchedulerMsg;
  toUrl: (model: Model) => Url;
  historyMode: (
    prev: Model,
    next: Model,
  ) => "push" | "replace" | "none";
  scene: (model: Model) => Scene;
  /**
   * Refresh a `Dynamic` collection's slot from rows the
   * CONSUMER's Model owns — pure `(model, id, rows) →
   * model`. A consumer holds its records in its own Model
   * and calls this (at init, and after a create/update/
   * delete) to project them into the scheduler slot; the
   * `Dynamic` source then PRESERVES that slot across
   * navigation. This is the seam that lets a consumer's
   * `update` stay pure instead of a module-global store
   * (ticket 20260708192518).
   */
  withRows: (
    model: Model,
    collectionId: SoftStr,
    rows: ReadonlyArray<Row>,
  ) => Model;
  /**
   * The startup reconciliation findings for this program's
   * adapter bindings against the supplied registry (point
   * 7): unknown/absent-default adapter ERRORS and unused
   * registration WARNINGS. Empty for a self-contained
   * declaration. The host inspects these before the first
   * render — the binding check the naming decision requires.
   */
  diagnostics: ReadonlyArray<Diagnostic>;
}>;

/**
 * The host bindings a program is derived against (point 7),
 * all optional: the adapter registry `Adapter` sources read
 * through, and the actor the authorize gate evaluates.
 * Omitting them yields the legacy self-contained program.
 */
export type Host = Readonly<{
  adapters?: Registry;
  actor?: Actor;
}>;

/** The URL-visible position of a model, as one string. */
const positionOf = (model: Model): string =>
  `${pipe(model.root, getOr(""))}|${model.path.join("/")}`;

/**
 * Derives the scheduled program from a declaration
 * (design ticket 09). Pure and mode-agnostic: `update`
 * executes nothing (async reads and action verbs are
 * returned as `Cmd` data), the URL codec is total both
 * ways, and `scene` is the only renderer seam — no
 * declaration type or derived value names a column, pane,
 * drawer, or screen (D10).
 *
 * `historyMode` marks a real navigation (`root`/`path`
 * change) as `push` so back/forward traverses it, and a
 * query-only change as `replace` (typing does not spam
 * history) — the nuqs discipline the oracle used.
 */
export const schedule = (
  declaration: Declaration,
  host: Host = {},
): Scheduled => {
  const engine: EngineOptions = {
    registry: pipe(
      fromNullable(host.adapters),
      getOr<Registry>([]),
    ),
    actor: fromNullable(host.actor),
  };
  const update = makeUpdate(declaration, engine);
  const init = makeInit(declaration, engine);
  const scene = makeScene(
    declaration,
    engine.actor,
  );
  return {
    init: (url: Url) => {
      const slice = parseUrl(url);
      return init(url.path)(slice);
    },
    update,
    onUrlChange: (url: Url): SchedulerMsg =>
      urlChanged(url),
    toUrl,
    historyMode: (prev: Model, next: Model) =>
      positionOf(prev) !== positionOf(next)
        ? "push"
        : "replace",
    scene,
    withRows: (
      model: Model,
      collectionId: SoftStr,
      rows: ReadonlyArray<Row>,
    ): Model =>
      setSlot(
        model,
        collectionId,
        loadedSlot(rows),
      ),
    diagnostics: reconcile(
      declaration,
      engine.registry,
    ),
  };
};
