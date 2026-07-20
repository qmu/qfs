/**
 * plggmatic — the Pragmatic design system on the plgg
 * family, and the home of its UI engine (absorbed back
 * from the retired `plgg-ui` package; the engine depends
 * only on `plgg` and `plgg-view`). The root barrel is
 * the RUNTIME surface: an explicit named-export list of
 * layout combinators, components, forms, the declarative
 * vocabulary, the scheduler, and the renderers. The THEME
 * surface (token utilities, scheme-aware color atoms, CSS
 * emitters, and the `themeToggle*` family) lives on the
 * `plggmatic/style` subpath (see `src/styleEntry.ts`) so
 * their Tailwind-style names (`p`, `text`, …) never
 * collide with the Html element builders here, and so the
 * subpath boundary equals the runtime/theme surface
 * boundary a consumer imports across.
 *
 * The `themeToggle*` family — routed onto the `/style`
 * subpath — is ALSO re-exported HERE on the root, because
 * plggmatic's historical root surface (where consumers
 * still import `themeToggle*`) carries it.
 */
export {
  frameworkName,
  cssPrefix,
} from "plggmatic/Meta/model/identity";
export {
  type PaneRole,
  type Parts,
  landmarkTag,
  row,
  column,
  pane,
  navPane,
  mainPane,
  asidePane,
} from "plggmatic/Layout";
// The `themeToggle*` family (component + class/CSS/static
// helpers) is part of the THEME surface, so it is routed
// through `plggmatic/style` (see `src/styleEntry.ts`); its
// source stays physically in `Component/`, only the export
// is routed. Everything else here is the runtime surface.
export {
  type InteractionState,
  type ButtonProps,
  type TextLinkProps,
  type HeadingLevel,
  type NavItem,
  focusRing,
  hoverDim,
  pressDim,
  button,
  textLink,
  heading,
  prose,
  navTree,
  type ColHeadProps,
  type Crumb,
  colHead,
  breadcrumb,
  type TextInputProps,
  type TextAreaProps,
  type SelectOption,
  type SelectProps,
  type CheckboxProps,
  type ConfirmDialogProps,
  type Tone,
  type ToastProps,
  textInput,
  textArea,
  selectInput,
  checkbox,
  confirmDialog,
  tones,
  toast,
  toaster,
} from "plggmatic/Component";

// --- Form: caster-parsed forms + submission (ticket 12) -
export {
  type ControlKind,
  type SubmissionState,
  type FieldSpec,
  type FormErrors,
  type Payload,
  type FormViewProps,
  controlKinds,
  idleSubmission,
  pendingSubmission,
  isPending,
  parseForm,
  errorFor,
  formView,
} from "plggmatic/Form";

// --- Render: screen-mode renderers (ticket 10/11) -----
export {
  type Mode,
  type Screen,
  modes,
  toggleMode,
  currentScreen,
  type HeaderLink,
  type ExtraColumn,
  type MultiColumnOptions,
  multiColumn,
  multiColumnWith,
  crumbsOf,
  singleColumn,
  renderMode,
} from "plggmatic/Render";

// --- Declarative vocabulary (ticket 09) ---------------
// The framework half: a mode-agnostic declaration (D10)
// from which `schedule` derives a TEA program. Renderers
// (tickets 10/11) consume the derived `Scene`; no type
// here names a column, pane, drawer, or screen.
export {
  type Field,
  type FieldValue,
  type Row,
  field,
  fieldOf,
  textValue,
  numValue,
  flagValue,
  momentValue,
  refValue,
  mediaValue,
  textValue$,
  numValue$,
  flagValue$,
  momentValue$,
  refValue$,
  mediaValue$,
  fieldText,
  refTarget,
  // aliased: `row` is the Layout pane combinator above;
  // the Row data constructor takes the `make` prefix.
  row as makeRow,
} from "plggmatic/Declare/model/Row";
export {
  type Path,
  type TypedSource,
  type Source,
  sync,
  async,
  dynamic,
  adapter,
} from "plggmatic/Declare/model/Source";
// The host capability seam (point 7): one adapter with
// read/apply verbs, the host-supplied actor, a checked
// effect, and the named/default registry the reconciliation
// checks. `toRow` left this surface for keyword projection.
export {
  type Actor,
  type Scope,
  type Effect,
  type ApplyOk,
  type HostAdapter,
  type NamedAdapter,
  type Registry,
  actor,
  scope,
  effect,
  hostAdapter,
  named,
  resolveAdapter,
} from "plggmatic/Declare/model/Adapter";
// Startup reconciliation of adapter bindings + the keyword
// projection (the data alternative to `toRow`) + the apply
// hand-off (adapter.apply → an action-result message).
export {
  type Diagnostic,
  reconcile,
  diagnosticSeverity,
  unknownAdapter$,
  noDefaultAdapter$,
  unusedAdapter$,
} from "plggmatic/Declare/usecase/reconcile";
export {
  type HostRecord,
  type Projected,
  type FieldProjection,
  type Projection,
  asText,
  asNum,
  asMoment,
  asFlag,
  projectField,
  projection,
  projectRow,
} from "plggmatic/Declare/usecase/project";
export { applyVia } from "plggmatic/Declare/usecase/apply";
export {
  type Query,
  type QueryChoice,
  query,
  queryChoice,
  matchesQuery,
  matchesChoice,
} from "plggmatic/Declare/model/Query";
export {
  type Verb,
  type Confirm,
  type Action,
  type Authorize,
  immediate,
  confirm,
  action,
  isDestructive,
  permits,
} from "plggmatic/Declare/model/Action";
export {
  type Collection,
  collection,
  collectionById,
  actionById,
} from "plggmatic/Declare/model/Collection";
export {
  type MenuEntry,
  type Menu,
  menuEntry,
  menu,
} from "plggmatic/Declare/model/Menu";
export {
  type Declaration,
  declare,
} from "plggmatic/Declare/model/Declaration";

// --- Scheduler (ticket 09) ----------------------------
export {
  type Slot,
  type PendingAction,
  type Model as ScheduledModel,
  emptyModel,
  slotOf,
  choiceOf,
} from "plggmatic/Schedule/model/Model";
// The (view, typed params) core: the View vocabulary,
// the Navigate message it addresses, and the legacy
// lowering both ways (mission decision, 2026-07-12).
export {
  type View,
  type Binding,
  menuView,
  listView,
  detailView,
  menuView$,
  listView$,
  detailView$,
  binding,
} from "plggmatic/Schedule/model/View";
export {
  focusedView,
  sliceOf,
} from "plggmatic/Schedule/usecase/lower";
export {
  type SchedulerMsg,
  navigate,
  urlChanged,
  openMenu,
  select,
  queryInput,
  queryChoiceInput,
  requestAction,
  confirmAction,
  cancelAction,
  loaded,
  failed,
} from "plggmatic/Schedule/model/Msg";
export {
  type ConfirmPrompt,
  type ActionButton,
  type RowLink,
  type MenuLink,
  type QueryState,
  type QueryChoiceState,
  type DetailField,
  type Tile,
  type Level,
  type Scene,
  menuLevel$,
  listLevel$,
  boardLevel$,
  detailLevel$,
} from "plggmatic/Schedule/model/Scene";
export {
  type UrlSlice,
  parseUrl,
  toUrl as sceneToUrl,
} from "plggmatic/Schedule/usecase/codec";
export {
  type Scheduled,
  type Host,
  schedule,
} from "plggmatic/Schedule/usecase/schedule";

// The Flow tier: the DSL's checked script IR + reader
// (static layer) and the fuel-metered pausable interpreter
// (prototype). `readFlow` turns flow DSL text + a schema
// into a checked `FlowScript`; the interpreter drives it
// against the settled Scene (mission points 6/9).
export {
  type FlowScript,
  type FlowStep,
  type FlowExpr,
  type FlowValue,
} from "plggmatic/Flow/model/script";
export {
  type FlowOutcome,
  type PausedFlow,
  type FlowDiag,
  defaultFuel,
} from "plggmatic/Flow/model/run";
export {
  type FlowSchema,
  type CollectionSchema,
  flowSchema,
} from "plggmatic/Flow/model/forms";
export { readFlow } from "plggmatic/Flow/usecase/read";
export {
  startFlow,
  resumeFlow,
} from "plggmatic/Flow/usecase/interpret";

// The Catalog tier: the third renderer (Scene → the engine-
// owned Tool catalog), the fuel-bounded run_flow the
// standing tool composes, and the thin, disposable WebMCP
// adapter over navigator.modelContext (mission point 8).
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
  rowCap,
  catalogOf,
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
  type ToolDescriptor,
  type CatalogDiff,
  type ModelContext,
  descriptorOf,
  diffCatalog,
  syncCatalog,
  invokeWith,
  detectModelContext,
  inertModelContext,
} from "plggmatic/Catalog";

// --- plggmatic's historical root surface --------------
// Routed from the SPECIFIC themeToggle module (as the
// `/style` subpath does) so the root barrel does not
// close an import cycle through `styleEntry`.
export {
  type ThemeToggleProps,
  themeToggle,
  staticThemeToggle,
  themeToggleClass,
  themeToggleCss,
} from "plggmatic/Component/usecase/themeToggle";
// The Pragmatic brand substance plggmatic owns (A3): the
// branded default Theme + palette-override API.
export {
  pragmaticTheme,
  pragmaticThemeWithPalette,
} from "plggmatic/brand";
