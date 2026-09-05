/** Color: HEX / RGB / HSV / HSL. Sin dependencias; el overlay lo usa en caliente. */

export type Rgb = { r: number; g: number; b: number };
export type Hsv = { h: number; s: number; v: number };
export type Hsl = { h: number; s: number; l: number };
export type ColorFormat = "hex" | "rgb" | "hsl";

export function clamp01(n: number): number {
  if (n < 0) return 0;
  if (n > 1) return 1;
  return n;
}

export function clampByte(n: number): number {
  return Math.min(255, Math.max(0, Math.round(n)));
}

export function rgbToHex({ r, g, b }: Rgb): string {
  return `#${[r, g, b].map((c) => clampByte(c).toString(16).padStart(2, "0")).join("")}`.toUpperCase();
}

export function parseHex(raw: string): Rgb | null {
  const value = raw.trim().replace(/^#/, "");
  if (!/^[0-9a-fA-F]{6}$/.test(value)) return null;
  return {
    r: Number.parseInt(value.slice(0, 2), 16),
    g: Number.parseInt(value.slice(2, 4), 16),
    b: Number.parseInt(value.slice(4, 6), 16),
  };
}

export function rgbToHsv({ r, g, b }: Rgb): Hsv {
  const rr = r / 255;
  const gg = g / 255;
  const bb = b / 255;
  const max = Math.max(rr, gg, bb);
  const min = Math.min(rr, gg, bb);
  const d = max - min;
  let h = 0;
  if (d !== 0) {
    if (max === rr) h = ((gg - bb) / d) % 6;
    else if (max === gg) h = (bb - rr) / d + 2;
    else h = (rr - gg) / d + 4;
    h *= 60;
    if (h < 0) h += 360;
  }
  return { h, s: max === 0 ? 0 : d / max, v: max };
}

export function hsvToRgb({ h, s, v }: Hsv): Rgb {
  const sat = clamp01(s);
  const val = clamp01(v);
  const hue = ((h % 360) + 360) % 360;
  const c = val * sat;
  const x = c * (1 - Math.abs(((hue / 60) % 2) - 1));
  const m = val - c;
  let rr = 0;
  let gg = 0;
  let bb = 0;
  if (hue < 60) {
    rr = c;
    gg = x;
  } else if (hue < 120) {
    rr = x;
    gg = c;
  } else if (hue < 180) {
    gg = c;
    bb = x;
  } else if (hue < 240) {
    gg = x;
    bb = c;
  } else if (hue < 300) {
    rr = x;
    bb = c;
  } else {
    rr = c;
    bb = x;
  }
  return {
    r: clampByte((rr + m) * 255),
    g: clampByte((gg + m) * 255),
    b: clampByte((bb + m) * 255),
  };
}

export function rgbToHsl({ r, g, b }: Rgb): Hsl {
  const rr = r / 255;
  const gg = g / 255;
  const bb = b / 255;
  const max = Math.max(rr, gg, bb);
  const min = Math.min(rr, gg, bb);
  const l = (max + min) / 2;
  const d = max - min;
  if (d === 0) return { h: 0, s: 0, l };
  const s = d / (1 - Math.abs(2 * l - 1));
  let h: number;
  if (max === rr) h = ((gg - bb) / d) % 6;
  else if (max === gg) h = (bb - rr) / d + 2;
  else h = (rr - gg) / d + 4;
  h *= 60;
  if (h < 0) h += 360;
  return { h, s, l };
}

/** HSL a RGB. `h` en grados, `s` y `l` en 0..1. Inversa de `rgbToHsl`. */
export function hslToRgb({ h, s, l }: Hsl): Rgb {
  const sat = clamp01(s);
  const lig = clamp01(l);
  const c = (1 - Math.abs(2 * lig - 1)) * sat;
  const hh = (((h % 360) + 360) % 360) / 60;
  const x = c * (1 - Math.abs((hh % 2) - 1));
  const [r, g, b] =
    hh < 1
      ? [c, x, 0]
      : hh < 2
        ? [x, c, 0]
        : hh < 3
          ? [0, c, x]
          : hh < 4
            ? [0, x, c]
            : hh < 5
              ? [x, 0, c]
              : [c, 0, x];
  const m = lig - c / 2;
  return {
    r: Math.round((r + m) * 255),
    g: Math.round((g + m) * 255),
    b: Math.round((b + m) * 255),
  };
}

/** Lo más largo que puede ser un color escrito. Corta el texto suelto. */
const COLOR_MAX_LEN = 32;

/** Un canal: número suelto o porcentaje. `max` es a cuánto equivale el 100%. */
function channel(raw: string, max: number): number | null {
  const text = raw.trim();
  const percent = text.endsWith("%");
  const n = Number.parseFloat(percent ? text.slice(0, -1) : text);
  if (!Number.isFinite(n)) return null;
  const value = percent ? (n / 100) * max : n;
  return value < 0 || value > max ? null : value;
}

/**
 * Los tres campos de `rgb(...)` / `hsl(...)`, en coma o en espacio.
 *
 * El alfa se descarta y se acepta en sus dos sintaxis: cuarto campo de
 * `rgba(r, g, b, a)` y `/ a` de la forma moderna. La muestra va opaca, que es
 * lo que deja ver el color de verdad sobre cualquier fondo.
 */
function colorArgs(body: string): string[] | null {
  const cut = body.split("/")[0] ?? "";
  const parts = (cut.includes(",") ? cut.split(",") : cut.trim().split(/\s+/))
    .map((part) => part.trim())
    .filter((part) => part.length > 0);
  return parts.length === 3 || parts.length === 4 ? parts.slice(0, 3) : null;
}

/**
 * El color que representa un texto, o `null` si no representa ninguno.
 *
 * Acepta hex de 3, 4, 6 y 8 dígitos —el alfa se ignora, la muestra va opaca— y
 * las formas funcionales `rgb`/`rgba`/`hsl`/`hsla`, con coma o con espacio.
 *
 * Exige que el texto sea **solo** el color: un párrafo que menciona un hex no
 * es un color, y pintarle una muestra convertiría el historial en una fila de
 * cuadrados de colores casuales.
 */
export function parseCssColor(raw: string): Rgb | null {
  const value = raw.trim();
  if (value.length === 0 || value.length > COLOR_MAX_LEN) return null;

  const hex = value.replace(/^#/, "");
  if (/^[0-9a-fA-F]+$/.test(hex)) {
    // Sin `#`, solo se acepta el largo canónico: "123456" es más veces un
    // número que un color, pero con almohadilla la intención es explícita.
    if (!value.startsWith("#") && hex.length !== 6) return null;
    if (hex.length === 3 || hex.length === 4) {
      return parseHex(
        hex
          .slice(0, 3)
          .split("")
          .map((c) => c + c)
          .join(""),
      );
    }
    if (hex.length === 6 || hex.length === 8) return parseHex(hex.slice(0, 6));
    return null;
  }

  const match = /^(rgba?|hsla?)\s*\(([^()]*)\)$/i.exec(value);
  if (!match) return null;
  const args = colorArgs(match[2] ?? "");
  if (!args) return null;
  if (match[1].toLowerCase().startsWith("rgb")) {
    const [r, g, b] = args.map((arg) => channel(arg, 255));
    if (r === null || g === null || b === null) return null;
    return { r: Math.round(r), g: Math.round(g), b: Math.round(b) };
  }
  const h = Number.parseFloat(args[0] ?? "");
  const s = channel(args[1] ?? "", 1);
  const l = channel(args[2] ?? "", 1);
  if (!Number.isFinite(h) || s === null || l === null) return null;
  return hslToRgb({ h, s, l });
}

export function formatRgb({ r, g, b }: Rgb): string {
  return `rgb(${clampByte(r)}, ${clampByte(g)}, ${clampByte(b)})`;
}

export function formatHsl(rgb: Rgb): string {
  const { h, s, l } = rgbToHsl(rgb);
  return `hsl(${Math.round(h)}, ${Math.round(s * 100)}%, ${Math.round(l * 100)}%)`;
}

export function formatColor(rgb: Rgb, format: ColorFormat): string {
  if (format === "rgb") return formatRgb(rgb);
  if (format === "hsl") return formatHsl(rgb);
  return rgbToHex(rgb);
}

/** Tinta para texto sobre un swatch: blanco o negro según luminancia. */
export function inkOn(rgb: Rgb): "#111" | "#fff" {
  const linear = (c: number) => {
    const srgb = clampByte(c) / 255;
    return srgb <= 0.04045 ? srgb / 12.92 : ((srgb + 0.055) / 1.055) ** 2.4;
  };
  const y = 0.2126 * linear(rgb.r) + 0.7152 * linear(rgb.g) + 0.0722 * linear(rgb.b);
  return (y + 0.05) / (linear(17) + 0.05) >= 1.05 / (y + 0.05) ? "#111" : "#fff";
}

/** Los 12 matices de la rosa cromática clásica, saturación y valor al máximo. */
export const ROSE_HUES: readonly number[] = [
  0, 30, 60, 90, 120, 150, 180, 210, 240, 270, 300, 330,
];

export function roseSwatch(hue: number): Rgb {
  return hsvToRgb({ h: hue, s: 1, v: 1 });
}

const RECENT_KEY = "atic-color-recent";
const RECENT_MAX = 8;

export function loadRecentColors(): string[] {
  if (typeof localStorage === "undefined") return [];
  try {
    const raw = localStorage.getItem(RECENT_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw) as unknown;
    if (!Array.isArray(parsed)) return [];
    return normalizeRecentColors(parsed);
  } catch {
    return [];
  }
}

export function normalizeRecentColors(values: unknown[]): string[] {
  const colors = values.flatMap((value) => {
    const parsed = typeof value === "string" ? parseHex(value) : null;
    return parsed ? [rgbToHex(parsed)] : [];
  });
  return [...new Set(colors)].slice(0, RECENT_MAX);
}

export function pushRecentColor(hex: string, sessionRecent: string[] = []): string[] {
  const next = normalizeRecentColors([hex, ...sessionRecent, ...loadRecentColors()]);
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify(next));
  } catch {
    // Quota / modo privado: el historial de sesión igual sirve.
  }
  return next;
}
