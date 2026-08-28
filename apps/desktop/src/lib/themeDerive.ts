/**
 * El tema personalizado: cuatro perillas, veinticuatro tokens.
 *
 * La paleta es plana y completa —esa es la regla del sistema—, así que un
 * editor honesto tendría 24 controles y la mitad de las combinaciones serían
 * ilegibles. Acá el usuario mueve papel, tinta, temperatura y acento, y el
 * resto se deriva conservando la ESTRUCTURA del tema base: cuánto sube cada
 * superficie sobre el papel, dónde cae el texto secundario entre el papel y la
 * tinta, qué alfa tienen los hairlines. Cambia el color, no el diseño.
 *
 * Todo el color se mezcla en OKLab y no en sRGB: mover la luminosidad de un
 * gris cálido en sRGB lo vuelve rosa, y una mezcla al 50% entre dos tonos del
 * mismo matiz se hunde. Es la única forma de que "papel más claro" signifique
 * eso y nada más.
 *
 * El módulo es puro y sin DOM a propósito: los literales de las paletas base
 * los lee `theme.ts` del CSS —única fuente de verdad— y los pasa acá.
 */

/** El juego completo. Mismo orden que las paletas en `styles/palettes/atic/`. */
export const PALETTE_TOKENS = [
  "bg",
  "surface",
  "surface-2",
  "elevated",
  "text",
  "muted",
  "faint",
  "line",
  "line-strong",
  "accent",
  "on-accent",
  "rec",
  "ok",
  "ok-soft",
  "warn",
  "warn-soft",
  "danger",
  "danger-soft",
  "info",
  "info-soft",
  "mic",
  "sys",
  "skin",
  "agent-accent",
] as const;

export type PaletteToken = (typeof PALETTE_TOKENS)[number];
export type Palette = Record<PaletteToken, string>;

/** Lo que el usuario mueve. Se persiste esto, no los colores resultantes. */
export type ThemeKnobs = {
  /** Tema del que se hereda la estructura y los colores de estado. */
  base: string;
  /** Luminosidad del papel, relativa a la del base. -100..100 */
  paper: number;
  /** Cuánto se aleja la tinta del papel. 0..100 */
  ink: number;
  /** Frío ↔ cálido. -100..100 */
  warmth: number;
  /** Acento, en hexadecimal. */
  accent: string;
};

export const DEFAULT_KNOBS: ThemeKnobs = {
  base: "dark",
  paper: 0,
  ink: 100,
  warmth: 0,
  accent: "#e8e8e0",
};

/** Lo que el slider de papel puede mover la luminosidad, en unidades OKLab. */
const PAPER_RANGE = 0.42;

/** Croma máximo que agrega la temperatura. Más que esto y el papel se tiñe. */
const WARMTH_CHROMA = 0.032;

/** Matiz cálido (ámbar) y frío (azul) de la temperatura, en grados OKLCh. */
const WARM_HUE = 74;
const COOL_HUE = 250;

/** Contraste mínimo contra el papel. La tinta cumple AA; el resto, jerarquía. */
const MIN_CONTRAST = { text: 4.5, muted: 3.2, faint: 2.2 };

// ── Color ───────────────────────────────────────────────────────────────────

type Rgb = { r: number; g: number; b: number };
type Oklch = { l: number; c: number; h: number };

const clamp = (n: number, min: number, max: number) =>
  n < min ? min : n > max ? max : n;

/** Acepta `#abc`, `#aabbcc` y el `rgb(r, g, b)` que devuelve `getComputedStyle`. */
export function parseColor(value: string): Rgb | null {
  const raw = value.trim();
  const hex = raw.startsWith("#") ? raw.slice(1) : null;
  if (hex && (hex.length === 3 || hex.length === 6)) {
    const wide = hex.length === 3 ? [...hex].map((c) => c + c).join("") : hex;
    const n = Number.parseInt(wide, 16);
    if (Number.isNaN(n)) return null;
    return { r: (n >> 16) & 255, g: (n >> 8) & 255, b: n & 255 };
  }
  const nums = raw.match(/-?\d*\.?\d+/g);
  if (raw.startsWith("rgb") && nums && nums.length >= 3) {
    return { r: Number(nums[0]), g: Number(nums[1]), b: Number(nums[2]) };
  }
  return null;
}

function toHex({ r, g, b }: Rgb): string {
  const part = (n: number) =>
    Math.round(clamp(n, 0, 255))
      .toString(16)
      .padStart(2, "0");
  return `#${part(r)}${part(g)}${part(b)}`;
}

const toLinear = (c: number) =>
  c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;

const toGamma = (c: number) =>
  c <= 0.0031308 ? c * 12.92 : 1.055 * c ** (1 / 2.4) - 0.055;

function rgbToOklch({ r, g, b }: Rgb): Oklch {
  const lr = toLinear(r / 255);
  const lg = toLinear(g / 255);
  const lb = toLinear(b / 255);

  const l = Math.cbrt(0.4122214708 * lr + 0.5363325363 * lg + 0.0514459929 * lb);
  const m = Math.cbrt(0.2119034982 * lr + 0.6806995451 * lg + 0.1073969566 * lb);
  const s = Math.cbrt(0.0883024619 * lr + 0.2817188376 * lg + 0.6299787005 * lb);

  const okL = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s;
  const okA = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
  const okB = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;

  const c = Math.hypot(okA, okB);
  const h = c < 1e-6 ? 0 : ((Math.atan2(okB, okA) * 180) / Math.PI + 360) % 360;
  return { l: okL, c, h };
}

function oklchToRgb({ l, c, h }: Oklch): Rgb {
  const rad = (h * Math.PI) / 180;
  const okA = c * Math.cos(rad);
  const okB = c * Math.sin(rad);

  const l_ = (l + 0.3963377774 * okA + 0.2158037573 * okB) ** 3;
  const m_ = (l - 0.1055613458 * okA - 0.0638541728 * okB) ** 3;
  const s_ = (l - 0.0894841775 * okA - 1.291485548 * okB) ** 3;

  return {
    r: 255 * toGamma(4.0767416621 * l_ - 3.3077115913 * m_ + 0.2309699292 * s_),
    g: 255 * toGamma(-1.2684380046 * l_ + 2.6097574011 * m_ - 0.3413193965 * s_),
    b: 255 * toGamma(-0.0041960863 * l_ - 0.7034186147 * m_ + 1.707614701 * s_),
  };
}

const hex = (color: Oklch) => toHex(oklchToRgb(color));

function luminance({ r, g, b }: Rgb): number {
  return (
    0.2126 * toLinear(r / 255) + 0.7152 * toLinear(g / 255) + 0.0722 * toLinear(b / 255)
  );
}

/** Contraste WCAG entre dos colores opacos. 1 = iguales, 21 = negro sobre blanco. */
export function contrast(a: string, b: string): number {
  const ca = parseColor(a);
  const cb = parseColor(b);
  if (!ca || !cb) return 1;
  const la = luminance(ca);
  const lb = luminance(cb);
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}

/**
 * El papel con el matiz de un color de estado encima.
 *
 * No es una mezcla: mezclar el papel claro con un verde oscuro lo ENSUCIA en
 * vez de teñirlo. Se toma el matiz del estado, una fracción de su croma, y se
 * deja la luminosidad del papel apenas movida —igual que un `*-soft` escrito a
 * mano en las paletas.
 */
function tint(paper: Oklch, color: string, lift: number): string {
  const state = parseColor(color);
  if (!state) return hex(paper);
  const { c, h } = rgbToOklch(state);
  return hex({ l: clamp(paper.l + lift, 0, 1), c: c * 0.4, h });
}

const lightnessOf = (color: string) => {
  const rgb = parseColor(color);
  return rgb ? rgbToOklch(rgb).l : 0;
};

/**
 * Aleja la tinta del papel hasta que se lea.
 *
 * Es la red que hace que el editor no pueda producir un tema roto: por más que
 * el usuario baje el contraste a cero, el texto sigue cumpliendo AA. Si el lado
 * pedido no da (tinta clara sobre papel casi blanco), se prueba el otro.
 */
function legible(want: Oklch, paper: string, min: number): Oklch {
  const away = want.l >= lightnessOf(paper) ? 1 : -1;
  for (const dir of [away, -away]) {
    for (let l = want.l; l >= 0 && l <= 1; l += dir * 0.01) {
      const candidate = { ...want, l };
      if (contrast(hex(candidate), paper) >= min) return candidate;
    }
  }
  return want;
}

// ── Derivación ──────────────────────────────────────────────────────────────

export function normalizeKnobs(raw: unknown): ThemeKnobs {
  const value = (raw ?? {}) as Partial<ThemeKnobs>;
  const num = (n: unknown, min: number, max: number, fallback: number) =>
    typeof n === "number" && Number.isFinite(n) ? clamp(Math.round(n), min, max) : fallback;
  const accent =
    typeof value.accent === "string" && parseColor(value.accent)
      ? toHex(parseColor(value.accent)!)
      : DEFAULT_KNOBS.accent;
  return {
    base: typeof value.base === "string" && value.base ? value.base : DEFAULT_KNOBS.base,
    paper: num(value.paper, -100, 100, DEFAULT_KNOBS.paper),
    ink: num(value.ink, 0, 100, DEFAULT_KNOBS.ink),
    warmth: num(value.warmth, -100, 100, DEFAULT_KNOBS.warmth),
    accent,
  };
}

/**
 * Las perillas que reproducen un tema tal cual está.
 *
 * Es lo que se carga al elegir un base: el editor abre mostrando ese tema y
 * cada slider en su posición actual, así el primer movimiento se siente como
 * una corrección y no como empezar de cero.
 */
export function seedKnobs(base: string, palette: Palette): ThemeKnobs {
  const paper = rgbToOklch(parseColor(palette.bg) ?? { r: 0, g: 0, b: 0 });
  const warmSide = paper.h > 340 || paper.h < 160 ? 1 : -1;
  return {
    base,
    paper: 0,
    ink: 100,
    warmth: Math.round(warmSide * clamp((paper.c / WARMTH_CHROMA) * 100, 0, 100)),
    accent: toHex(parseColor(palette.accent) ?? { r: 0, g: 0, b: 0 }),
  };
}

/** El lado en el que cae el resultado. Puede no ser el del tema base. */
export function derivedSide(base: Palette, knobs: ThemeKnobs): "light" | "dark" {
  return paperOf(base, knobs).l > 0.5 ? "light" : "dark";
}

function paperOf(base: Palette, knobs: ThemeKnobs): Oklch {
  const bg = rgbToOklch(parseColor(base.bg) ?? { r: 0, g: 0, b: 0 });
  const warmth = knobs.warmth / 100;
  return {
    l: clamp(bg.l + (knobs.paper / 100) * PAPER_RANGE, 0.05, 0.97),
    c: Math.abs(warmth) * WARMTH_CHROMA,
    h: warmth >= 0 ? WARM_HUE : COOL_HUE,
  };
}

/**
 * Los 24 tokens del tema personalizado.
 *
 * `canon` son las paletas claro y oscuro de la app: de ahí salen los colores de
 * estado cuando el papel cruza de lado —un verde de tema oscuro sobre papel
 * claro no se ve—, que es el único caso donde el base deja de servir.
 */
export function derivePalette(
  base: Palette,
  canon: { light: Palette; dark: Palette },
  knobs: ThemeKnobs,
): Palette {
  const paper = paperOf(base, knobs);
  const bg = hex(paper);
  const side = paper.l > 0.5 ? "light" : "dark";
  const states = canon[side];

  const baseBgL = lightnessOf(base.bg);
  const baseTextL = lightnessOf(base.text);
  const inkSpan = baseTextL - baseBgL;

  /** Sube (o baja) una superficie lo mismo que en el tema base. */
  const step = (token: PaletteToken) => {
    const delta = lightnessOf(base[token]) - baseBgL;
    const target = paper.l + delta;
    const l = target > 0.985 || target < 0.015 ? paper.l - delta : target;
    return hex({ ...paper, l: clamp(l, 0, 1) });
  };

  /** Tinta y sus dos escalones, respetando la proporción del tema base. */
  const inkAt = (ratio: number, min: number) => {
    const l = clamp(paper.l + inkSpan * (knobs.ink / 100) * ratio, 0, 1);
    // La tinta hereda algo del matiz del papel: un gris neutro sobre papel
    // cálido se ve azulado por contraste simultáneo.
    return hex(legible({ l, c: paper.c * 0.4, h: paper.h }, bg, min));
  };

  const text = inkAt(1, MIN_CONTRAST.text);
  const muted = inkAt(
    inkSpan === 0 ? 0.7 : (lightnessOf(base.muted) - baseBgL) / inkSpan,
    MIN_CONTRAST.muted,
  );
  const faint = inkAt(
    inkSpan === 0 ? 0.45 : (lightnessOf(base.faint) - baseBgL) / inkSpan,
    MIN_CONTRAST.faint,
  );

  const ink = parseColor(text) ?? { r: 0, g: 0, b: 0 };
  const hairline = (alpha: number) => `rgb(${ink.r} ${ink.g} ${ink.b} / ${alpha}%)`;

  const accent = toHex(parseColor(knobs.accent) ?? { r: 0, g: 0, b: 0 });
  // La tinta de encima del acento es la que más contraste saque: con un acento
  // oscuro gana el papel, con uno claro gana el texto.
  const onAccent =
    contrast(accent, bg) >= contrast(accent, text) ? bg : text;

  /** Los fondos tenues de estado son el papel teñido, no un color propio. */
  const soft = (color: string) => tint(paper, color, side === "light" ? 0.01 : 0.035);

  return {
    bg,
    surface: step("surface"),
    "surface-2": step("surface-2"),
    elevated: step("elevated"),

    text,
    muted,
    faint,

    line: hairline(side === "light" ? 11 : 10),
    "line-strong": hairline(side === "light" ? 20 : 18),

    accent,
    "on-accent": onAccent,

    rec: states.rec,
    ok: states.ok,
    "ok-soft": soft(states.ok),
    warn: states.warn,
    "warn-soft": soft(states.warn),
    danger: states.danger,
    "danger-soft": soft(states.danger),
    info: states.info,
    "info-soft": soft(states.info),

    mic: states.mic,
    sys: states.sys,

    skin: step("surface"),
    "agent-accent": muted,
  };
}
