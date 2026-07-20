/**
 * The plggmatic Form module (ticket 12): the headless
 * form machinery the control components render against —
 * the caster-parse fold ("parse, don't validate": the
 * only validation is the cast that produces the typed
 * payload), the control-kind and submission-state unions,
 * and the form assembly. DOM-free where it can be; the
 * views compose plgg-view element builders.
 */
export {
  type ControlKind,
  controlKinds,
} from "plggmatic/Form/model/control";
export {
  type SubmissionState,
  idleSubmission,
  pendingSubmission,
  isPending,
} from "plggmatic/Form/model/submission";
export {
  type FieldSpec,
  type FormErrors,
  type Payload,
  parseForm,
  errorFor,
} from "plggmatic/Form/usecase/parseForm";
export {
  type FormViewProps,
  formView,
} from "plggmatic/Form/usecase/formView";
