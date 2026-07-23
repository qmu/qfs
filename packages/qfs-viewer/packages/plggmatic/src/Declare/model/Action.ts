import {
  type SoftStr,
  type Box,
  type Icon,
  type Option,
  fromNullable,
  matchOption,
  box,
  icon,
  pattern,
  match,
} from "plgg";
import { type Cmd } from "plgg-view/client";
import { type SchedulerMsg } from "plggmatic/Schedule/model/Msg";
import { type Actor } from "plggmatic/Declare/model/Adapter";

/**
 * The mutation an {@link Action} performs, as a closed
 * union so a renderer can style create/update/delete
 * distinctly and the scheduler can reason about intent
 * without a free string.
 */
export type Verb = "create" | "update" | "delete";

/**
 * Confirmation semantics as DATA — destructive intent is
 * explicit, and confirm/cancel is scheduler state, not
 * renderer folklore. `Immediate` runs the action at once;
 * `Confirm` parks a pending-confirmation in the model
 * (with the prompt and whether it is destructive) until a
 * `ConfirmAction`/`CancelAction` message resolves it.
 */
export type Confirm =
  | Icon<"Immediate">
  | Box<
      "Confirm",
      Readonly<{
        prompt: SoftStr;
        destructive: boolean;
      }>
    >;

/** No confirmation — run the action immediately. */
export const immediate = (): Confirm =>
  icon("Immediate");

/** Park a confirmation before running the action. */
export const confirm = (
  prompt: SoftStr,
  destructive: boolean = false,
): Confirm =>
  box("Confirm")({ prompt, destructive });

/** Matchers for folding a {@link Confirm}. */
export const immediate$ = () =>
  pattern("Immediate")();
export const confirm$ = () =>
  pattern("Confirm")();

/**
 * An action's authorization policy: a total predicate over
 * the host-supplied {@link Actor} and the subject (the
 * target row id, `None` for a create). The ENGINE evaluates
 * it BEFORE dispatching — an unauthorized action never runs
 * and the Scene hides its button (point-7 authorization).
 * A pure decision as data; the adapter MAY re-enforce at
 * the data layer (defense in depth, not required).
 */
export type Authorize = (
  actor: Actor,
  subject: Option<SoftStr>,
) => boolean;

/**
 * A create/update/delete verb on a collection, mapped to
 * a `Cmd` factory. `run` receives the acting row's id
 * (`None` for a create, which has no target yet) and
 * returns a `Cmd` — pure data the plgg-view runtime
 * executes; the scheduler only returns it, never runs it.
 * Fold the mutation's `Result` to a `Loaded`/`Failed`
 * message inside the `cmdEffect` thunk so the effect
 * always resolves to a message. `authorize` is the optional
 * legality gate (`None` = always permitted): the engine
 * checks it against the actor before dispatching `run`.
 */
export type Action = Readonly<{
  id: SoftStr;
  label: SoftStr;
  verb: Verb;
  confirm: Confirm;
  run: (
    target: Option<SoftStr>,
  ) => Cmd<SchedulerMsg>;
  authorize: Option<Authorize>;
}>;

/** Constructs an {@link Action}. */
export const action = (a: {
  id: SoftStr;
  label: SoftStr;
  verb: Verb;
  confirm: Confirm;
  run: (
    target: Option<SoftStr>,
  ) => Cmd<SchedulerMsg>;
  authorize?: Authorize;
}): Action => ({
  id: a.id,
  label: a.label,
  verb: a.verb,
  confirm: a.confirm,
  run: a.run,
  authorize: fromNullable(a.authorize),
});

/**
 * Whether the actor may run `action` on `subject` — the
 * engine's legality gate, total. An action with no declared
 * `authorize` is always permitted (absence of a policy is
 * not a denial in v1; deny-by-default arrives with the
 * manifest's static authorize checks). A declared policy
 * REQUIRES an actor: a policy with no supplied actor
 * (`None`) is denied — the engine cannot prove
 * authorization without one.
 */
export const permits = (
  action: Action,
  actor: Option<Actor>,
  subject: Option<SoftStr>,
): boolean =>
  matchOption<Authorize, boolean>(
    () => true,
    (fn: Authorize) =>
      matchOption<Actor, boolean>(
        () => false,
        (ac: Actor) => fn(ac, subject),
      )(actor),
  )(action.authorize);

/** Whether an action's confirmation is destructive. */
export const isDestructive = (
  c: Confirm,
): boolean =>
  match(c)(
    [immediate$(), (): boolean => false],
    [
      confirm$(),
      ({ content }): boolean =>
        content.destructive,
    ],
  );
