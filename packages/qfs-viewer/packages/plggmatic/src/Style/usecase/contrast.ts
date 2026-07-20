import {
  type HexColor,
  hex,
} from "plggmatic/Style/model/hexColor";

/**
 * The WCAG 2.x relative-luminance / contrast-ratio math,
 * extracted so both the phase-1 gate (`contrast.spec.ts`)
 * and an override author can run the SAME AA audit over a
 * palette. `contrastRatio` returns the 1..21 ratio; the
 * caller compares against 4.5 (normal text) or 3.0
 * (non-text) — the caster does NOT reject a low-contrast
 * override (that is the app's deliberate choice), this is
 * an advisory audit.
 */
const channel = (b: number): number => {
  const s = b / 255;
  return s <= 0.03928
    ? s / 12.92
    : Math.pow((s + 0.055) / 1.055, 2.4);
};

const luminance = (h: string): number => {
  const r = parseInt(h.slice(1, 3), 16);
  const g = parseInt(h.slice(3, 5), 16);
  const b = parseInt(h.slice(5, 7), 16);
  return (
    0.2126 * channel(r) +
    0.7152 * channel(g) +
    0.0722 * channel(b)
  );
};

export const contrastRatio = (
  a: HexColor,
  b: HexColor,
): number => {
  const la = luminance(hex(a)) + 0.05;
  const lb = luminance(hex(b)) + 0.05;
  return la > lb ? la / lb : lb / la;
};
