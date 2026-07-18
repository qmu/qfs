import {
  type Icon,
  icon,
  pattern,
  match,
} from "plgg";

/**
 * A form's submission state — a closed union so pending
 * cannot be confused with idle and a renderer must handle
 * both. `Pending` disables every control and the submit
 * button and withholds hover/press feedback; the action's
 * completion `Msg` folds the `Result` back to `Idle`. The
 * scheduler (ticket 09) owns the action LIFECYCLE; this
 * is the local view state a form control reads to disable
 * itself, not a second action state machine.
 */
export type SubmissionState =
  Icon<"Idle"> | Icon<"Pending">;

/** The idle submission state. */
export const idleSubmission =
  (): SubmissionState => icon("Idle");

/** The pending submission state (in flight). */
export const pendingSubmission =
  (): SubmissionState => icon("Pending");

/** Matchers for folding a {@link SubmissionState}. */
export const idleSubmission$ = () =>
  pattern("Idle")();
export const pendingSubmission$ = () =>
  pattern("Pending")();

/** Whether the form is currently submitting. */
export const isPending = (
  s: SubmissionState,
): boolean =>
  match(s)(
    [idleSubmission$(), (): boolean => false],
    [pendingSubmission$(), (): boolean => true],
  );
