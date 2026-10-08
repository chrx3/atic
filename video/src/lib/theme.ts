import { Easing } from "remotion";

/** Paleta oscura primaria de Atic (styles/palettes/atic/dark.css). */
export const C = {
  skin: "#1a1a18",
  bg: "#121211",
  surface: "#1a1a18",
  surface2: "#1e1e1b",
  elevated: "#262622",
  text: "#f0f0ea",
  muted: "#a8a89e",
  faint: "#8f8f86",
  line: "rgb(240 240 234 / 10%)",
  lineStrong: "rgb(240 240 234 / 18%)",
  accent: "#e8e8e0",
  onAccent: "#121211",
  rec: "#e85a52",
  ok: "#6faf88",
  warn: "#d4a84b",
  info: "#8fa9b8",
} as const;

export const FONT_SANS =
  '"Aptos", "Avenir Next", "Helvetica Neue", "Segoe UI Variable Text", "Segoe UI Variable", sans-serif';
export const FONT_MONO =
  '"Cascadia Mono", "SFMono-Regular", "Roboto Mono", monospace';

export const SHADOW_GOO = "drop-shadow(0 10px 22px rgb(0 0 0 / 38%))";

/** Curvas reales de app.css. */
export const EASE = {
  island: Easing.bezier(0.33, 1.38, 0.46, 1),
  liquid: Easing.bezier(0.5, 0, 0.2, 1),
  smoothOut: Easing.bezier(0.22, 1, 0.36, 1),
};

export const ISLAND = { openMs: 240, staggerMs: 26 } as const;

/** Geometría de la pill (pillStage.ts). */
export const PILL = {
  islandLong: 124,
  islandThick: 40,
  islandTool: 44,
  islandGap: 2,
  islandMark: 32,
} as const;

export const clamp01 = (n: number) => Math.min(1, Math.max(0, n));
export const lerp = (a: number, b: number, t: number) => a + (b - a) * t;
