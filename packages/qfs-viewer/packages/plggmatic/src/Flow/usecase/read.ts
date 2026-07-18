import {
  type SoftStr,
  type Option,
  type Result,
  type Box,
  ok,
  err,
  isErr,
  some,
  none,
  box,
  getOr,
  matchOption,
  fromNullable,
} from "plgg";
import {
  type Sexp,
  type SourceRange,
  parseSexps,
  isSymbolExp,
  isStrExp,
  isNumExp,
  isListExp,
  sexpRange,
} from "plgg-ir-syntax";
import {
  type SemDiagnostic,
  semError,
  fromSyntaxDiagnostic,
  codeUnknownForm,
  codeUnknownName,
  codeArityMismatch,
  codeTypeMismatch,
  codeInvalidForm,
} from "plgg-ir-language";
import {
  type SchedulerMsg,
  openMenu,
  queryInput,
  queryChoiceInput,
  select,
  requestAction,
} from "plggmatic/Schedule/model/Msg";
import {
  type FlowExpr,
  type FlowStep,
  type FlowScript,
  strLit,
  numLit,
  varRef,
  sceneRows,
  mapKw,
  getKw,
  firstOf,
  countOf,
  sumOf,
  okOf,
  errOf,
  someOf,
  noneOf,
  matchEmpty,
  matchOption as matchOptionExpr,
  dispatchStep,
  bindStep,
  flowScript,
} from "plggmatic/Flow/model/script";
import {
  type FlowSchema,
  type CollectionSchema,
  collectionSchema,
  fieldIsNumeric,
  hasChoice,
} from "plggmatic/Flow/model/forms";

/**
 * The static layer's reader: flow DSL text → a checked,
 * normalized {@link FlowScript}, or accumulated positioned
 * diagnostics. Total — never throws.
 *
 * Parsing is `plgg-ir-syntax` (the shared S-expression
 * grammar and its ranged diagnostics) and diagnostics are
 * `plgg-ir-language`'s `SemDiagnostic` vocabulary (its code
 * constants + `fromSyntaxDiagnostic`); type checking runs
 * on a structural {@link FlowType} (the manifest `SemType`
 * vocabulary is reserved for the manifest lowering, not
 * spent on v1's numeric/not distinction). Collection,
 * choice, and field names resolve against the manifest-
 * shaped {@link FlowSchema} (the seeded binding set) — the
 * cross-dialect seam a compiled manifest will feed. (The
 * manifest exports a `Language<Module>`, not a composable
 * `Dialect`, so `compose` onto it is unavailable via the
 * public API and unsound anyway; scope-seeding is the
 * seam — see the ticket's recorded findings.)
 *
 * v1 surface: `(flow <name> <step>* <result-expr>)` where
 * a step is `(dispatch <msg>)` or `(let <name> <expr>)`,
 * and folds are explicit `(match-empty …)` /
 * `(match-option …)` forms (a 1:1-to-`FlowScript` surface
 * over `dsl-v1-core.md` §6's sugar).
 */

/** A structural flow type (containers nest; leaves are str/num/row). */
export type FlowType =
  | Box<"TStr", null>
  | Box<"TNum", null>
  | Box<"TRow", SoftStr>
  | Box<"TList", Readonly<{ el: FlowType }>>
  | Box<"TOption", Readonly<{ el: FlowType }>>
  | Box<"TResult", Readonly<{ el: FlowType }>>;

const tStr: FlowType = box("TStr")(null);
const tNum: FlowType = box("TNum")(null);
const tRow = (c: SoftStr): FlowType =>
  box("TRow")(c);
const tList = (el: FlowType): FlowType =>
  box("TList")({ el });
const tOption = (el: FlowType): FlowType =>
  box("TOption")({ el });
const tResult = (el: FlowType): FlowType =>
  box("TResult")({ el });

/** A checked expression: the lowered node + its type. */
type Typed = Readonly<{
  expr: FlowExpr;
  type: FlowType;
}>;

type Diags = ReadonlyArray<SemDiagnostic>;
type Env = ReadonlyArray<
  Readonly<{ name: SoftStr; type: FlowType }>
>;

const fail = (
  code: SoftStr,
  message: SoftStr,
  range: SourceRange,
): Result<never, Diags> =>
  err([semError(code, message, range)]);

/** The symbol name of a node, if it is a symbol (total). */
const symName = (
  s: Sexp | undefined,
): Option<SoftStr> =>
  s !== undefined && isSymbolExp(s)
    ? some(s.content.name)
    : none();

/** The source range of a node, or an empty range. */
const rangeOf = (
  s: Sexp | undefined,
): SourceRange =>
  s === undefined ? emptyRange() : sexpRange(s);

/** The head symbol name of a list's items. */
const headName = (
  items: ReadonlyArray<Sexp>,
): Option<SoftStr> =>
  matchOption<Sexp, Option<SoftStr>>(
    () => none(),
    (h: Sexp) => symName(h),
  )(fromNullable(items[0]));

/** The items of a node, if it is a list (total). */
const listItems = (
  s: Sexp | undefined,
): Option<ReadonlyArray<Sexp>> =>
  s !== undefined && isListExp(s)
    ? some(s.content.items)
    : none();

/** Looks a bound name up in the env. */
const lookupEnv = (
  env: Env,
  name: SoftStr,
): Option<FlowType> =>
  matchOption<
    Readonly<{ name: SoftStr; type: FlowType }>,
    Option<FlowType>
  >(
    () => none(),
    (b) => some(b.type),
  )(
    fromNullable(
      env.find((b) => b.name === name),
    ),
  );

/**
 * A name in a name/keyword position — a plain symbol.
 * plgg-ir-syntax has no `:`-prefixed keyword literals (it
 * rejects `:`), so a field keyword is an ordinary symbol
 * distinguished by position, not lexeme; `asName` and
 * `asKeyword` are the same extraction, named for intent.
 */
const asName = (
  s: Sexp | undefined,
): Option<SoftStr> => symName(s);
const asKeyword = asName;

/**
 * A collection's schema, or an empty one. Total — a `TRow`
 * only arises from a schema-verified `scene-rows`, so the
 * default is unreachable in practice; it keeps map/get free
 * of a dead failure branch.
 */
const csOf = (
  schema: FlowSchema,
  id: SoftStr,
): CollectionSchema =>
  getOr<CollectionSchema>({
    id,
    fields: [],
    choices: [],
  })(collectionSchema(schema, id));

/** The value type of a keyword field in a collection. */
const kwType = (
  cs: CollectionSchema,
  kw: SoftStr,
): FlowType =>
  fieldIsNumeric(cs, kw) ? tNum : tStr;

/** The collection a `TRow` names. */
const rowColl = (t: FlowType): SoftStr =>
  t.__tag === "TRow" ? t.content : "";

/**
 * Checks + lowers one pure expression against the schema
 * and the binding env. Never dispatches. Returns the
 * first blocking diagnostic.
 */
const checkExpr = (
  schema: FlowSchema,
  env: Env,
  s: Sexp,
): Result<Typed, Diags> => {
  if (isStrExp(s))
    return ok({
      expr: strLit(s.content.value),
      type: tStr,
    });
  if (isNumExp(s))
    return ok({
      expr: numLit(s.content.value),
      type: tNum,
    });
  if (isSymbolExp(s)) {
    const name = s.content.name;
    return matchOption<
      FlowType,
      Result<Typed, Diags>
    >(
      () =>
        fail(
          codeUnknownName,
          `unbound name '${name}'`,
          sexpRange(s),
        ),
      (t: FlowType) =>
        ok({ expr: varRef(name), type: t }),
    )(lookupEnv(env, name));
  }
  return matchOption<
    ReadonlyArray<Sexp>,
    Result<Typed, Diags>
  >(
    () =>
      fail(
        codeInvalidForm,
        "expected an expression",
        sexpRange(s),
      ),
    (items: ReadonlyArray<Sexp>) =>
      matchOption<SoftStr, Result<Typed, Diags>>(
        () =>
          fail(
            codeInvalidForm,
            "expected a form head",
            sexpRange(s),
          ),
        (h: SoftStr) =>
          checkForm(
            schema,
            env,
            s,
            h,
            items.slice(1),
          ),
      )(headName(items)),
  )(listItems(s));
};

/** Dispatches a checked list form by its head. */
const checkForm = (
  schema: FlowSchema,
  env: Env,
  whole: Sexp,
  head: SoftStr,
  args: ReadonlyArray<Sexp>,
): Result<Typed, Diags> => {
  const range = sexpRange(whole);
  switch (head) {
    case "scene-rows":
      return checkSceneRows(schema, args, range);
    case "map":
      return checkMap(schema, env, args, range);
    case "get":
      return checkGet(schema, env, args, range);
    case "first":
      return checkFirst(schema, env, args, range);
    case "count":
      return checkCount(schema, env, args, range);
    case "sum":
      return checkSum(schema, env, args, range);
    case "ok":
      return checkWrap(
        schema,
        env,
        args,
        range,
        okOf,
        tResult,
      );
    case "some":
      return checkWrap(
        schema,
        env,
        args,
        range,
        someOf,
        tOption,
      );
    case "err":
      return checkErr(args, range);
    case "none":
      return args.length === 0
        ? ok({
            expr: noneOf(),
            type: tOption(tStr),
          })
        : fail(
            codeArityMismatch,
            "none takes no arguments",
            range,
          );
    case "match-empty":
      return checkMatchEmpty(
        schema,
        env,
        args,
        range,
      );
    case "match-option":
      return checkMatchOption(
        schema,
        env,
        args,
        range,
      );
    default:
      return fail(
        codeUnknownForm,
        `unknown form or function '${head}'`,
        range,
      );
  }
};

/** Runs `then` only when arity holds. */
const withArity = (
  args: ReadonlyArray<Sexp>,
  n: number,
  head: SoftStr,
  range: SourceRange,
  then: () => Result<Typed, Diags>,
): Result<Typed, Diags> =>
  args.length === n
    ? then()
    : fail(
        codeArityMismatch,
        `${head} takes ${n} argument(s), got ${args.length}`,
        range,
      );

const checkSceneRows = (
  schema: FlowSchema,
  args: ReadonlyArray<Sexp>,
  range: SourceRange,
): Result<Typed, Diags> =>
  withArity(args, 1, "scene-rows", range, () =>
    matchOption<SoftStr, Result<Typed, Diags>>(
      () =>
        fail(
          codeInvalidForm,
          "scene-rows needs a collection name",
          range,
        ),
      (coll: SoftStr) =>
        matchOption<
          CollectionSchema,
          Result<Typed, Diags>
        >(
          () =>
            fail(
              codeUnknownName,
              `unknown collection '${coll}'`,
              range,
            ),
          () =>
            ok({
              expr: sceneRows(coll),
              type: tList(tRow(coll)),
            }),
        )(collectionSchema(schema, coll)),
    )(asName(args[0] as Sexp)),
  );

const checkMap = (
  schema: FlowSchema,
  env: Env,
  args: ReadonlyArray<Sexp>,
  range: SourceRange,
): Result<Typed, Diags> =>
  withArity(args, 2, "map", range, () =>
    matchOption<SoftStr, Result<Typed, Diags>>(
      () =>
        fail(
          codeInvalidForm,
          "map needs a :keyword first",
          range,
        ),
      (kw: SoftStr) => {
        const of = checkExpr(
          schema,
          env,
          args[1] as Sexp,
        );
        if (isErr(of)) return of;
        const t = of.content.type;
        return t.__tag === "TList" &&
          t.content.el.__tag === "TRow"
          ? ok({
              expr: mapKw(kw, of.content.expr),
              type: tList(
                kwType(
                  csOf(
                    schema,
                    rowColl(t.content.el),
                  ),
                  kw,
                ),
              ),
            })
          : fail(
              codeTypeMismatch,
              "map expects a list of rows",
              range,
            );
      },
    )(asKeyword(args[0] as Sexp)),
  );

const checkGet = (
  schema: FlowSchema,
  env: Env,
  args: ReadonlyArray<Sexp>,
  range: SourceRange,
): Result<Typed, Diags> =>
  withArity(args, 2, "get", range, () =>
    matchOption<SoftStr, Result<Typed, Diags>>(
      () =>
        fail(
          codeInvalidForm,
          "get needs a :keyword first",
          range,
        ),
      (kw: SoftStr) => {
        const of = checkExpr(
          schema,
          env,
          args[1] as Sexp,
        );
        if (isErr(of)) return of;
        const t = of.content.type;
        return t.__tag === "TRow"
          ? ok({
              expr: getKw(kw, of.content.expr),
              type: tOption(
                kwType(
                  csOf(schema, t.content),
                  kw,
                ),
              ),
            })
          : fail(
              codeTypeMismatch,
              "get expects a row",
              range,
            );
      },
    )(asKeyword(args[0] as Sexp)),
  );

const checkFirst = (
  schema: FlowSchema,
  env: Env,
  args: ReadonlyArray<Sexp>,
  range: SourceRange,
): Result<Typed, Diags> =>
  withArity(args, 1, "first", range, () => {
    const of = checkExpr(
      schema,
      env,
      args[0] as Sexp,
    );
    if (isErr(of)) return of;
    const t = of.content.type;
    return t.__tag === "TList"
      ? ok({
          expr: firstOf(of.content.expr),
          type: tOption(t.content.el),
        })
      : fail(
          codeTypeMismatch,
          "first expects a list",
          range,
        );
  });

const checkCount = (
  schema: FlowSchema,
  env: Env,
  args: ReadonlyArray<Sexp>,
  range: SourceRange,
): Result<Typed, Diags> =>
  withArity(args, 1, "count", range, () => {
    const of = checkExpr(
      schema,
      env,
      args[0] as Sexp,
    );
    if (isErr(of)) return of;
    return of.content.type.__tag === "TList"
      ? ok({
          expr: countOf(of.content.expr),
          type: tNum,
        })
      : fail(
          codeTypeMismatch,
          "count expects a list",
          range,
        );
  });

const checkSum = (
  schema: FlowSchema,
  env: Env,
  args: ReadonlyArray<Sexp>,
  range: SourceRange,
): Result<Typed, Diags> =>
  withArity(args, 1, "sum", range, () => {
    const of = checkExpr(
      schema,
      env,
      args[0] as Sexp,
    );
    if (isErr(of)) return of;
    const t = of.content.type;
    return t.__tag === "TList" &&
      t.content.el.__tag === "TNum"
      ? ok({
          expr: sumOf(of.content.expr),
          type: tNum,
        })
      : fail(
          codeTypeMismatch,
          "sum expects a list of numbers",
          range,
        );
  });

const checkWrap = (
  schema: FlowSchema,
  env: Env,
  args: ReadonlyArray<Sexp>,
  range: SourceRange,
  wrap: (e: FlowExpr) => FlowExpr,
  ty: (t: FlowType) => FlowType,
): Result<Typed, Diags> =>
  withArity(args, 1, "wrap", range, () => {
    const of = checkExpr(
      schema,
      env,
      args[0] as Sexp,
    );
    if (isErr(of)) return of;
    return ok({
      expr: wrap(of.content.expr),
      type: ty(of.content.type),
    });
  });

const checkErr = (
  args: ReadonlyArray<Sexp>,
  range: SourceRange,
): Result<Typed, Diags> =>
  withArity(args, 1, "err", range, () =>
    matchOption<SoftStr, Result<Typed, Diags>>(
      () =>
        fail(
          codeInvalidForm,
          "err needs a :keyword",
          range,
        ),
      (kw: SoftStr) =>
        ok({
          expr: errOf(kw),
          type: tResult(tStr),
        }),
    )(asKeyword(args[0] as Sexp)),
  );

/** match-empty of whenEmpty bind whenRest */
const checkMatchEmpty = (
  schema: FlowSchema,
  env: Env,
  args: ReadonlyArray<Sexp>,
  range: SourceRange,
): Result<Typed, Diags> =>
  withArity(args, 4, "match-empty", range, () => {
    const of = checkExpr(
      schema,
      env,
      args[0] as Sexp,
    );
    if (isErr(of)) return of;
    const t = of.content.type;
    if (t.__tag !== "TList")
      return fail(
        codeTypeMismatch,
        "match-empty expects a list",
        range,
      );
    return matchOption<
      SoftStr,
      Result<Typed, Diags>
    >(
      () =>
        fail(
          codeInvalidForm,
          "match-empty needs a bind name",
          range,
        ),
      (bindName: SoftStr) => {
        const empty = checkExpr(
          schema,
          env,
          args[1] as Sexp,
        );
        if (isErr(empty)) return empty;
        const rest = checkExpr(
          schema,
          [{ name: bindName, type: t }, ...env],
          args[3] as Sexp,
        );
        if (isErr(rest)) return rest;
        return ok({
          expr: matchEmpty({
            of: of.content.expr,
            whenEmpty: empty.content.expr,
            bind: bindName,
            whenRest: rest.content.expr,
          }),
          type: rest.content.type,
        });
      },
    )(asName(args[2] as Sexp));
  });

/** match-option of whenNone bind whenSome */
const checkMatchOption = (
  schema: FlowSchema,
  env: Env,
  args: ReadonlyArray<Sexp>,
  range: SourceRange,
): Result<Typed, Diags> =>
  withArity(
    args,
    4,
    "match-option",
    range,
    () => {
      const of = checkExpr(
        schema,
        env,
        args[0] as Sexp,
      );
      if (isErr(of)) return of;
      const t = of.content.type;
      if (t.__tag !== "TOption")
        return fail(
          codeTypeMismatch,
          "match-option expects an option",
          range,
        );
      return matchOption<
        SoftStr,
        Result<Typed, Diags>
      >(
        () =>
          fail(
            codeInvalidForm,
            "match-option needs a bind name",
            range,
          ),
        (bindName: SoftStr) => {
          const noneB = checkExpr(
            schema,
            env,
            args[1] as Sexp,
          );
          if (isErr(noneB)) return noneB;
          const someB = checkExpr(
            schema,
            [
              {
                name: bindName,
                type: t.content.el,
              },
              ...env,
            ],
            args[3] as Sexp,
          );
          if (isErr(someB)) return someB;
          return ok({
            expr: matchOptionExpr({
              of: of.content.expr,
              whenNone: noneB.content.expr,
              bind: bindName,
              whenSome: someB.content.expr,
            }),
            type: someB.content.type,
          });
        },
      )(asName(args[2] as Sexp));
    },
  );

/** Lowers a `(dispatch <msg>)` inner form to a Msg. */
const readMsg = (
  schema: FlowSchema,
  s: Sexp | undefined,
): Result<SchedulerMsg, Diags> =>
  matchOption<
    ReadonlyArray<Sexp>,
    Result<SchedulerMsg, Diags>
  >(
    () =>
      fail(
        codeInvalidForm,
        "dispatch needs a message form",
        rangeOf(s),
      ),
    (items: ReadonlyArray<Sexp>) => {
      const range = rangeOf(s);
      const a = items.slice(1);
      return matchOption<
        SoftStr,
        Result<SchedulerMsg, Diags>
      >(
        () =>
          fail(
            codeInvalidForm,
            "expected a message head",
            range,
          ),
        (h: SoftStr) => {
          switch (h) {
            case "open-menu":
              return sym1(a, range, openMenu);
            case "query-input":
              return str1(a, range, queryInput);
            case "query-choice":
              return choiceMsg(schema, a, range);
            case "select":
              return numStr(a, range, select);
            case "request-action":
              return symSym(a, range, (c, act) =>
                requestAction(c, act, none()),
              );
            default:
              return fail(
                codeUnknownForm,
                `unknown message '${h}'`,
                range,
              );
          }
        },
      )(headName(items));
    },
  )(listItems(s));

/** The string value at position `i`, if it is a string. */
const atStr = (
  a: ReadonlyArray<Sexp>,
  i: number,
): Option<SoftStr> =>
  matchOption<Sexp, Option<SoftStr>>(
    () => none(),
    (n: Sexp) =>
      isStrExp(n)
        ? some(n.content.value)
        : none(),
  )(fromNullable(a[i]));

/** The number value at position `i`, if it is a number. */
const atNum = (
  a: ReadonlyArray<Sexp>,
  i: number,
): Option<number> =>
  matchOption<Sexp, Option<number>>(
    () => none(),
    (n: Sexp) =>
      isNumExp(n)
        ? some(n.content.value)
        : none(),
  )(fromNullable(a[i]));

/** A one-symbol message (`open-menu`). */
const sym1 = (
  a: ReadonlyArray<Sexp>,
  range: SourceRange,
  f: (id: SoftStr) => SchedulerMsg,
): Result<SchedulerMsg, Diags> =>
  matchOption<
    SoftStr,
    Result<SchedulerMsg, Diags>
  >(
    () => badMsg(range, "expected a name"),
    (id: SoftStr) => ok(f(id)),
  )(asName(a[0]));

/** A one-string message (`query-input`). */
const str1 = (
  a: ReadonlyArray<Sexp>,
  range: SourceRange,
  f: (v: SoftStr) => SchedulerMsg,
): Result<SchedulerMsg, Diags> =>
  matchOption<
    SoftStr,
    Result<SchedulerMsg, Diags>
  >(
    () => badMsg(range, "expected a string"),
    (v: SoftStr) => ok(f(v)),
  )(atStr(a, 0));

/** query-choice: a schema-validated choice id + a string. */
const choiceMsg = (
  schema: FlowSchema,
  a: ReadonlyArray<Sexp>,
  range: SourceRange,
): Result<SchedulerMsg, Diags> =>
  bothO(
    asName(a[0]),
    atStr(a, 1),
    () =>
      badMsg(
        range,
        "query-choice expects a choice id and a string",
      ),
    (id: SoftStr, value: SoftStr) =>
      schema.collections.some((cs) =>
        hasChoice(cs, id),
      )
        ? ok(queryChoiceInput(id, value))
        : fail(
            codeUnknownName,
            `unknown query choice '${id}'`,
            range,
          ),
  );

/** A two-symbol message (`request-action`). */
const symSym = (
  a: ReadonlyArray<Sexp>,
  range: SourceRange,
  f: (x: SoftStr, y: SoftStr) => SchedulerMsg,
): Result<SchedulerMsg, Diags> =>
  bothO(
    asName(a[0]),
    asName(a[1]),
    () => badMsg(range, "expected two names"),
    (x: SoftStr, y: SoftStr) => ok(f(x, y)),
  );

/** A number-then-string message (`select`). */
const numStr = (
  a: ReadonlyArray<Sexp>,
  range: SourceRange,
  f: (n: number, v: SoftStr) => SchedulerMsg,
): Result<SchedulerMsg, Diags> =>
  bothO(
    atNum(a, 0),
    atStr(a, 1),
    () =>
      badMsg(
        range,
        "expected a number and a string",
      ),
    (n: number, v: SoftStr) => ok(f(n, v)),
  );

/** A dispatch-shape diagnostic. */
const badMsg = (
  range: SourceRange,
  message: SoftStr,
): Result<SchedulerMsg, Diags> =>
  fail(codeInvalidForm, message, range);

/** Folds two Options together (both present, or none). */
const bothO = <A, B>(
  a: Option<A>,
  b: Option<B>,
  onNone: () => Result<SchedulerMsg, Diags>,
  onBoth: (
    a: A,
    b: B,
  ) => Result<SchedulerMsg, Diags>,
): Result<SchedulerMsg, Diags> =>
  matchOption<A, Result<SchedulerMsg, Diags>>(
    onNone,
    (av: A) =>
      matchOption<B, Result<SchedulerMsg, Diags>>(
        onNone,
        (bv: B) => onBoth(av, bv),
      )(b),
  )(a);

/** Reads one step form, returning it and the grown env. */
const readStep = (
  schema: FlowSchema,
  env: Env,
  s: Sexp,
): Result<
  Readonly<{ step: FlowStep; env: Env }>,
  Diags
> =>
  matchOption<
    ReadonlyArray<Sexp>,
    Result<
      Readonly<{ step: FlowStep; env: Env }>,
      Diags
    >
  >(
    () =>
      fail(
        codeInvalidForm,
        "a step must be (dispatch …) or (let …)",
        sexpRange(s),
      ),
    (items: ReadonlyArray<Sexp>) => {
      const range = sexpRange(s);
      return matchOption<
        SoftStr,
        Result<
          Readonly<{
            step: FlowStep;
            env: Env;
          }>,
          Diags
        >
      >(
        () =>
          fail(
            codeInvalidForm,
            "a step needs a head",
            range,
          ),
        (h: SoftStr) => {
          if (h === "dispatch") {
            const m = readMsg(schema, items[1]);
            return isErr(m)
              ? err(m.content)
              : ok({
                  step: dispatchStep(m.content),
                  env,
                });
          }
          if (h === "let")
            return readLet(
              schema,
              env,
              items,
              range,
            );
          return fail(
            codeUnknownForm,
            `a step must be dispatch or let, got '${h}'`,
            range,
          );
        },
      )(headName(items));
    },
  )(listItems(s));

const readLet = (
  schema: FlowSchema,
  env: Env,
  items: ReadonlyArray<Sexp>,
  range: SourceRange,
): Result<
  Readonly<{ step: FlowStep; env: Env }>,
  Diags
> =>
  matchOption<
    SoftStr,
    Result<
      Readonly<{ step: FlowStep; env: Env }>,
      Diags
    >
  >(
    () =>
      fail(
        codeInvalidForm,
        "let needs a name",
        range,
      ),
    (nm: SoftStr) =>
      matchOption<
        Sexp,
        Result<
          Readonly<{ step: FlowStep; env: Env }>,
          Diags
        >
      >(
        () =>
          fail(
            codeInvalidForm,
            "let needs a value expression",
            range,
          ),
        (valForm: Sexp) => {
          const val = checkExpr(
            schema,
            env,
            valForm,
          );
          if (isErr(val)) return err(val.content);
          return ok({
            step: bindStep(nm, val.content.expr),
            env: [
              {
                name: nm,
                type: val.content.type,
              },
              ...env,
            ],
          });
        },
      )(fromNullable(items[2])),
  )(asName(items[1]));

/**
 * Reads a flow's steps + result: leading `(dispatch …)`
 * / `(let …)` steps, then the final result expression.
 */
const readBody = (
  schema: FlowSchema,
  forms: ReadonlyArray<Sexp>,
  range: SourceRange,
): Result<
  Readonly<{
    steps: ReadonlyArray<FlowStep>;
    result: FlowExpr;
  }>,
  Diags
> => {
  if (forms.length === 0)
    return fail(
      codeInvalidForm,
      "a flow needs a result expression",
      range,
    );
  const stepForms = forms.slice(0, -1);
  const resultForm = forms[
    forms.length - 1
  ] as Sexp;
  const steps: FlowStep[] = [];
  let env: Env = [];
  for (const sf of stepForms) {
    const parsed = readStep(schema, env, sf);
    if (isErr(parsed)) return err(parsed.content);
    steps.push(parsed.content.step);
    env = parsed.content.env;
  }
  const result = checkExpr(
    schema,
    env,
    resultForm,
  );
  if (isErr(result)) return err(result.content);
  return ok({
    steps,
    result: result.content.expr,
  });
};

/**
 * Reads flow DSL text into a checked {@link FlowScript}.
 * Expects exactly one top-level `(flow <name> …)` form.
 */
export const readFlow = (
  source: SoftStr,
  schema: FlowSchema,
): Result<FlowScript, Diags> => {
  const parsed = parseSexps(source);
  if (isErr(parsed))
    return err(
      parsed.content.map(fromSyntaxDiagnostic),
    );
  const forms = parsed.content;
  if (forms.length !== 1)
    return err([
      semError(
        codeInvalidForm,
        "expected exactly one (flow …) form",
        emptyRange(),
      ),
    ]);
  const top = forms[0] as Sexp;
  return matchOption<
    ReadonlyArray<Sexp>,
    Result<FlowScript, Diags>
  >(
    () =>
      fail(
        codeInvalidForm,
        "top level must be a (flow …) form",
        sexpRange(top),
      ),
    (items: ReadonlyArray<Sexp>) => {
      const range = sexpRange(top);
      const isFlow = matchOption<
        SoftStr,
        boolean
      >(
        () => false,
        (n) => n === "flow",
      )(headName(items));
      if (!isFlow)
        return fail(
          codeInvalidForm,
          "top level must be a (flow …) form",
          range,
        );
      return matchOption<
        SoftStr,
        Result<FlowScript, Diags>
      >(
        () =>
          fail(
            codeInvalidForm,
            "a flow needs a name",
            range,
          ),
        (nm: SoftStr) => {
          const body = readBody(
            schema,
            items.slice(2),
            range,
          );
          if (isErr(body))
            return err(body.content);
          return ok(
            flowScript({
              name: nm,
              steps: body.content.steps,
              result: body.content.result,
            }),
          );
        },
      )(asName(items[1]));
    },
  )(listItems(top));
};

const emptyRange = (): SourceRange => ({
  start: { offset: 0, line: 1, column: 1 },
  end: { offset: 0, line: 1, column: 1 },
});
