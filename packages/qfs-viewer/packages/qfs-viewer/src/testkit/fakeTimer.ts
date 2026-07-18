// A timer whose clock the test advances by hand.
//
// Test infrastructure (`testkit/`, excluded from the production vendor
// boundary). Debounce tests that sleep on a real clock are slow and flaky —
// they trade determinism for nothing. Here `run()` fires whatever is pending,
// so the debounce window's BEHAVIOUR (coalescing, last-write-wins, cancel-on-
// new-change) is asserted in microseconds and never races.
import { type Timer } from "#qfs-viewer/domain/usecase/reload";

/**
 * A {@link Timer} plus a `run()` that fires the pending callback, if any.
 *
 * `cancel()` then `schedule()` is exactly what the debouncer does on each
 * event, so a burst leaves exactly one pending callback — which is the
 * property the coalescing test measures.
 */
export type FakeTimer = Timer &
  Readonly<{
    /** Fire the pending callback, if any. */
    run: () => void;
    /** Whether a callback is currently scheduled. */
    isPending: () => boolean;
  }>;

/** Builds a {@link FakeTimer}. */
export const fakeTimer = (): FakeTimer => {
  let pending: (() => void) | undefined =
    undefined;
  return {
    schedule: (fn) => {
      pending = fn;
    },
    cancel: () => {
      pending = undefined;
    },
    run: () => {
      const fn = pending;
      pending = undefined;
      if (fn !== undefined) {
        fn();
      }
    },
    isPending: () => pending !== undefined,
  };
};
