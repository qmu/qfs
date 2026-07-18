/**
 * The framework's fixed identity values. `frameworkName`
 * is the one word for this framework everywhere (docs,
 * data attributes, error messages); `cssPrefix` is the
 * namespace every plggmatic CSS custom property carries
 * (`--pm-...`), keeping framework variables from
 * colliding with an app's own.
 */
export const frameworkName = "plggmatic";

/**
 * Prefix for CSS custom properties owned by plggmatic
 * (e.g. `--pm-surface`). Consumed by the color-scheme
 * emitter.
 */
export const cssPrefix = "pm";
