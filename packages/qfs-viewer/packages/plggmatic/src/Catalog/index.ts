/**
 * The plggmatic Catalog module: the THIRD renderer of the
 * settled {@link import("plggmatic/Schedule/model/Scene").Scene}
 * (beside the two HTML renderers) — a fold to the engine-
 * owned {@link Tool} catalog (mission point 8, 2026-07-13),
 * plus the fuel-bounded {@link runFlow} the standing
 * `run_flow` tool composes, and a thin, disposable WebMCP
 * adapter over the `navigator.modelContext` proposal. The
 * catalog is transport-neutral DATA; the adapter is the
 * only impure edge and is inert when the API is absent.
 * An explicit named barrel (house style).
 */
export {
  type ToolInput,
  type ToolEffect,
  type Tool,
  nullary,
  textArg,
  enumArg,
  emit,
  runFlowEffect,
  tool,
  nullary$,
  textArg$,
  enumArg$,
  emit$,
  runFlow$,
} from "plggmatic/Catalog/model/tool";
export {
  rowCap,
  catalogOf,
} from "plggmatic/Catalog/usecase/catalog";
export {
  type FlowHost,
  type RunOutcome,
  flowHost,
  flowSchemaOf,
  runFlow,
  rejected,
  stalled,
  ran,
  rejected$,
  stalled$,
  ran$,
} from "plggmatic/Catalog/usecase/runFlow";
export {
  type ToolDescriptor,
  type CatalogDiff,
  type ModelContext,
  descriptorOf,
  diffCatalog,
  syncCatalog,
  invokeWith,
  detectModelContext,
  inertModelContext,
} from "plggmatic/Catalog/usecase/adapter";
