/**
 * The kind of form control a {@link Field} renders — a
 * closed union so a renderer's `match` is exhaustive and
 * a new control kind cannot be added without every
 * interpreter site acknowledging it.
 */
export type ControlKind =
  "text" | "textarea" | "select" | "checkbox";

export const controlKinds: ReadonlyArray<ControlKind> =
  ["text", "textarea", "select", "checkbox"];
