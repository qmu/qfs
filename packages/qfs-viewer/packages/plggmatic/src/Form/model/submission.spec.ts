import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import {
  idleSubmission,
  pendingSubmission,
  isPending,
} from "plggmatic/Form/model/submission";

test("isPending distinguishes the two states", () =>
  all([
    check(
      isPending(idleSubmission()),
      toBe(false),
    ),
    check(
      isPending(pendingSubmission()),
      toBe(true),
    ),
  ]));
