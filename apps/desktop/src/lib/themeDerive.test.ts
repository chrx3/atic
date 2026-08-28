import { describe, expect, it } from "vitest";
import {
  contrast,
  DEFAULT_KNOBS,
  derivePalette,
  derivedSide,
  normalizeKnobs,
  PALETTE_TOKENS,
  parseColor,
  seedKnobs,
  type Palette,
  type ThemeKnobs,
} from "./themeDerive";

/**
 * Fixtures, no copias de la paleta: en la app los literales se leen del CSS.
 * Acá solo hacen falta valores con la forma correcta —oscuro con tinta clara,
 * claro con tinta oscura— para probar la derivación.
 */
const DARK: Palette = {
  bg: "#121211",
  surface: "#1a1a18",
  "surface-2": "#1e1e1b",
  elevated: "#262622",
  text: "#f0f0ea",
  muted: "#a8a89e",
  faint: "#74746c",
  line: "rgb(240 240 234 / 10%)",
  "line-strong": "rgb(240 240 234 / 18%)",
  accent: "#e8e8e0",
  "on-accent": "#121211",
  rec: "#e85a52",
  ok: "#6faf88",
  "ok-soft": "#1c2a22",
  warn: "#d4a84b",
  "warn-soft": "#2b2416",
  danger: "#e85a52",
  "danger-soft": "#2d1a19",
  info: "#8fa9b8",
  "info-soft": "#1a2328",
  mic: "#6faf88",
  sys: "#8fa9b8",
  skin: "#1a1a18",
  "agent-accent": "#a8a89e",
};

const LIGHT: Palette = {
  ...DARK,
  bg: "#ecece6",
  surface: "#f7f7f2",
  "surface-2": "#efefe8",
  elevated: "#ffffff",
  text: "#171714",
  muted: "#5f5f58",
  faint: "#8f8f86",
  accent: "#1a1a17",
  "on-accent": "#f7f7f2",
  rec: "#d6453d",
  ok: "#3f7355",
  warn: "#946718",
  danger: "#d6453d",
  info: "#47708a",
  mic: "#3f7355",
  sys: "#47708a",
  skin: "#f7f7f2",
};

const CANON = { light: LIGHT, dark: DARK };
const knobs = (patch: Partial<ThemeKnobs> = {}): ThemeKnobs =>
  normalizeKnobs({ ...DEFAULT_KNOBS, ...patch });

const lum = (color: string) => {
  const rgb = parseColor(color)!;
  return 0.2126 * rgb.r + 0.7152 * rgb.g + 0.0722 * rgb.b;
};

describe("parseColor", () => {
  it("lee las tres formas que llegan del CSS", () => {
    expect(parseColor("#fff")).toEqual({ r: 255, g: 255, b: 255 });
    expect(parseColor("#121211")).toEqual({ r: 18, g: 18, b: 17 });
    expect(parseColor("rgb(18, 18, 17)")).toEqual({ r: 18, g: 18, b: 17 });
  });

  it("descarta lo que no es un color", () => {
    expect(parseColor("")).toBeNull();
    expect(parseColor("papel")).toBeNull();
  });
});

describe("normalizeKnobs", () => {
  it("recorta los rangos", () => {
    const k = normalizeKnobs({ base: "dark", paper: 999, ink: -5, warmth: -999 });
    expect(k.paper).toBe(100);
    expect(k.ink).toBe(0);
    expect(k.warmth).toBe(-100);
  });

  it("cae al default con basura", () => {
    expect(normalizeKnobs(null)).toEqual(DEFAULT_KNOBS);
    expect(normalizeKnobs({ accent: "azul" }).accent).toBe(DEFAULT_KNOBS.accent);
  });
});

describe("seedKnobs", () => {
  it("reproduce el tema tal cual está", () => {
    const seeded = seedKnobs("dark", DARK);
    expect(seeded.base).toBe("dark");
    expect(seeded.paper).toBe(0);
    expect(seeded.ink).toBe(100);
    expect(seeded.accent).toBe("#e8e8e0");
  });

  it("con papel cálido da temperatura positiva", () => {
    expect(seedKnobs("sepia", { ...LIGHT, bg: "#ded4c1" }).warmth).toBeGreaterThan(0);
  });
});

describe("derivePalette", () => {
  it("devuelve el juego completo de tokens", () => {
    const palette = derivePalette(DARK, CANON, knobs());
    expect(Object.keys(palette).sort()).toEqual([...PALETTE_TOKENS].sort());
    for (const token of PALETTE_TOKENS) expect(palette[token]).toBeTruthy();
  });

  it("el papel sube y baja con su perilla", () => {
    const base = derivePalette(DARK, CANON, knobs());
    const claro = derivePalette(DARK, CANON, knobs({ paper: 60 }));
    const oscuro = derivePalette(DARK, CANON, knobs({ paper: -60 }));
    expect(lum(claro.bg)).toBeGreaterThan(lum(base.bg));
    expect(lum(oscuro.bg)).toBeLessThan(lum(base.bg));
  });

  it("mantiene el escalonado de superficies del tema base", () => {
    const palette = derivePalette(DARK, CANON, knobs({ paper: 40 }));
    expect(lum(palette.surface)).toBeGreaterThan(lum(palette.bg));
    expect(lum(palette["elevated"])).toBeGreaterThan(lum(palette.surface));
  });

  /** La promesa del editor: ninguna combinación de perillas queda ilegible. */
  it("la tinta cumple AA aunque se pida sin contraste", () => {
    for (const ink of [0, 25, 50, 100]) {
      for (const paper of [-100, -40, 0, 40, 100]) {
        for (const base of [DARK, LIGHT]) {
          const palette = derivePalette(base, CANON, knobs({ ink, paper }));
          expect(contrast(palette.text, palette.bg)).toBeGreaterThanOrEqual(4.4);
          expect(contrast(palette.muted, palette.bg)).toBeGreaterThanOrEqual(3.1);
        }
      }
    }
  });

  it("la temperatura tiñe el papel para los dos lados", () => {
    const calido = parseColor(derivePalette(DARK, CANON, knobs({ warmth: 100 })).bg)!;
    const frio = parseColor(derivePalette(DARK, CANON, knobs({ warmth: -100 })).bg)!;
    expect(calido.r).toBeGreaterThan(calido.b);
    expect(frio.b).toBeGreaterThan(frio.r);
  });

  it("respeta el acento y le pone encima la tinta que más contrasta", () => {
    const palette = derivePalette(DARK, CANON, knobs({ accent: "#c15f3c" }));
    expect(palette.accent).toBe("#c15f3c");
    expect(contrast(palette["on-accent"], palette.accent)).toBeGreaterThan(
      contrast(
        palette["on-accent"] === palette.bg ? palette.text : palette.bg,
        palette.accent,
      ),
    );
  });

  it("los colores de estado siguen al lado donde cae el papel", () => {
    const claro = knobs({ paper: 100 });
    expect(derivedSide(DARK, claro)).toBe("light");
    expect(derivePalette(DARK, CANON, claro).ok).toBe(LIGHT.ok);
    expect(derivePalette(DARK, CANON, knobs()).ok).toBe(DARK.ok);
  });

  it("los fondos tenues de estado son el papel teñido", () => {
    const palette = derivePalette(DARK, CANON, knobs());
    // Tan tenue como los `*-soft` escritos a mano: se nota la banda, no el color.
    expect(contrast(palette["ok-soft"], palette.bg)).toBeLessThan(1.6);
    expect(contrast(palette["ok-soft"], palette.text)).toBeGreaterThan(4.5);
  });
});
