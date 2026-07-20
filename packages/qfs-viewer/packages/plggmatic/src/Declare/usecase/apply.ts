import {
  type SoftStr,
  type Option,
  matchResult,
} from "plgg";
import {
  type Cmd,
  cmdEffect,
} from "plgg-view/client";
import {
  type SchedulerMsg,
  loaded,
  failed,
} from "plggmatic/Schedule/model/Msg";
import {
  type HostAdapter,
  type Effect,
  type Actor,
  type ApplyOk,
} from "plggmatic/Declare/model/Adapter";

/**
 * Builds an action `run` from a host adapter's `apply` — the
 * apply HAND-OFF (point 7). `build` turns the dispatch
 * target (the selected row, `None` for a create) into the
 * checked {@link Effect}; the adapter applies it against the
 * actor and the deferred Result folds INSIDE the `cmdEffect`
 * thunk to a `Loaded` (the refreshed rows) or `Failed` (the
 * error on the action-result path, named by `collection`)
 * message. Nothing throws; an apply `Err` surfaces as a
 * value exactly like a read `Err`.
 *
 * The `actor` here is the one the host also passes to
 * `schedule` — the adapter's OPTIONAL re-enforcement seam
 * (defense in depth). The engine's authorize gate over the
 * runtime actor remains the primary boundary; an
 * unauthorized action never reaches this `run` at all.
 */
export const applyVia =
  (
    adapter: HostAdapter,
    actor: Actor,
    collection: SoftStr,
    build: (target: Option<SoftStr>) => Effect,
  ) =>
  (target: Option<SoftStr>): Cmd<SchedulerMsg> =>
    cmdEffect(() =>
      adapter.apply(build(target), actor).then(
        matchResult<ApplyOk, Error, SchedulerMsg>(
          (e: Error): SchedulerMsg =>
            failed(collection, e.message),
          (ok: ApplyOk): SchedulerMsg =>
            loaded(ok.collection, ok.rows),
        ),
      ),
    );
