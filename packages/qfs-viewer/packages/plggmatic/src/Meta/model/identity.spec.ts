import {
  test,
  check,
  all,
  toBe,
} from "plgg-test";
import {
  frameworkName,
  cssPrefix,
} from "plggmatic/Meta/model/identity";

test("frameworkName is the one word for this framework", () => {
  return check(frameworkName, toBe("plggmatic"));
});

test("cssPrefix namespaces plggmatic custom properties", () => {
  return all([
    check(cssPrefix, toBe("pm")),
    check(
      `--${cssPrefix}-surface`,
      toBe("--pm-surface"),
    ),
  ]);
});
