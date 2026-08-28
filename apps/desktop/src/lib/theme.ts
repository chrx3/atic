/** Tema de interfaz: las paletas disponibles, o la que diga el sistema. */

import {
  derivePalette,
  derivedSide,
  normalizeKnobs,
  PALETTE_TOKENS,
  type Palette,
  type ThemeKnobs,
} from "./themeDerive";

export {
  DEFAULT_KNOBS,
  derivePalette,
  PALETTE_TOKENS,
  seedKnobs,
  type Palette,
  type ThemeKnobs,
} from "./themeDerive";

/**
 * Cada valor tiene un archivo en `src/styles/palettes/atic/` que declara el
 * juego completo de tokens. Agregar un tema es agregar el archivo, importarlo
 * en `app.css` antes de `console.css`, sumarlo acá y a `ui_theme` en Rust.
 */
export const UI_THEMES = [
  "system",
  "light",
  "sepia",
  "mist",
  "graphite",
  "midnight",
  "dark",
  "claude",
  "claude-dark",
  "custom",
] as const;

export type UiTheme = (typeof UI_THEMES)[number];

/** Un tema concreto: lo que termina en `data-theme`. */
export type ResolvedTheme = Exclude<UiTheme, "system">;

/**
 * Los que tienen archivo de paleta propio.
 *
 * `custom` no lo tiene —se escribe token por token en el `style` del root— y
 * por eso tampoco puede ser el base de sí mismo.
 */
export const BASE_THEMES = UI_THEMES.filter(
  (theme): theme is Exclude<ResolvedTheme, "custom"> =>
    theme !== "system" && theme !== "custom",
);

/**
 * De qué lado está la tinta de cada tema.
 *
 * No es decoración: manda el `color-scheme` nativo (scrollbars, controles del
 * sistema), la variante `dark:` de Tailwind, la paleta del xterm y los `--rb-*`
 * del árbol viejo, que solo tiene dos variantes.
 */
const THEME_BASE: Record<Exclude<ResolvedTheme, "custom">, "light" | "dark"> = {
  light: "light",
  sepia: "light",
  mist: "light",
  claude: "light",
  graphite: "dark",
  midnight: "dark",
  dark: "dark",
  "claude-dark": "dark",
};

/**
 * Cache por webview. El overlay de Tauri usa un `data_directory` propio
 * (WebView2 no puede compartir perfil con main), así que este valor NO cruza
 * ventanas. La fuente de verdad es `config.ui_theme`; el evento `ui-theme`
 * avisa a las flotantes. El cache solo evita un destello en la misma ventana.
 */
export const THEME_STORAGE_KEY = "atic-theme";
const STORAGE_KEY = THEME_STORAGE_KEY;

export function normalizeTheme(value: string | null | undefined): UiTheme {
  return UI_THEMES.includes(value as UiTheme) ? (value as UiTheme) : "system";
}

export function systemPrefersDark(): boolean {
  if (typeof window === "undefined" || !window.matchMedia) return false;
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

export function resolveTheme(theme: UiTheme): ResolvedTheme {
  if (theme === "system") return systemPrefersDark() ? "dark" : "light";
  return theme;
}

/** Las perillas del personalizado, cacheadas igual que el tema. */
export const KNOBS_STORAGE_KEY = "atic-theme-custom";

let knobs: ThemeKnobs = normalizeKnobs(null);
let knobsLoaded = false;

/** El lado del último personalizado aplicado: depende del papel, no del base. */
let customSide: "light" | "dark" = "dark";

/** Claro u oscuro, para lo que no distingue entre paletas del mismo lado. */
export function themeBase(theme: string | null | undefined): "light" | "dark" {
  const resolved = resolveTheme(normalizeTheme(theme));
  return resolved === "custom" ? customSide : THEME_BASE[resolved];
}

/**
 * Los literales de una paleta, leídos del CSS.
 *
 * El personalizado se deriva de un tema base y esos valores viven en
 * `styles/palettes/atic/*.css`: duplicarlos en TS sería garantizar que algún
 * día digan cosas distintas. Se lee de un nodo suelto con el `data-theme`
 * puesto —el mismo truco que la muestra del selector— y se descarta.
 */
export function readPalette(theme: string): Palette {
  const probe = document.createElement("div");
  probe.dataset.theme = theme;
  probe.style.display = "none";
  document.body.append(probe);
  try {
    const styles = getComputedStyle(probe);
    const palette = {} as Palette;
    for (const token of PALETTE_TOKENS) {
      palette[token] = styles.getPropertyValue(`--${token}`).trim();
    }
    return palette;
  } finally {
    probe.remove();
  }
}

function paintCustom(root: HTMLElement): "light" | "dark" {
  const base = readPalette(customKnobs().base);
  const palette = derivePalette(
    base,
    { light: readPalette("light"), dark: readPalette("dark") },
    knobs,
  );
  for (const token of PALETTE_TOKENS) {
    root.style.setProperty(`--${token}`, palette[token]);
  }
  return derivedSide(base, knobs);
}

function clearCustom(root: HTMLElement): void {
  for (const token of PALETTE_TOKENS) root.style.removeProperty(`--${token}`);
}

export function applyTheme(theme: UiTheme): ResolvedTheme {
  const resolved = resolveTheme(theme);
  const root = document.documentElement;

  // Los tokens en línea le ganan a cualquier hoja: hay que sacarlos al volver
  // a un tema con paleta propia, o el personalizado se queda pegado.
  if (resolved === "custom") {
    customSide = paintCustom(root);
  } else {
    clearCustom(root);
  }

  const base = themeBase(resolved);
  root.dataset.theme = resolved;
  root.dataset.themeBase = base;
  root.style.colorScheme = base;
  try {
    localStorage.setItem(STORAGE_KEY, theme);
  } catch {
    // ignore
  }
  return resolved;
}

/**
 * Las perillas en curso.
 *
 * La fuente de verdad es `config.ui_theme_custom`; hasta que la config llegue
 * —o en el overlay, que nace antes— vale el cache del webview.
 */
export function customKnobs(): ThemeKnobs {
  if (!knobsLoaded) {
    knobs = readCachedKnobs();
    knobsLoaded = true;
  }
  return knobs;
}

/**
 * Cambia las perillas y repinta si el personalizado está puesto.
 *
 * Se llama en cada `input` del editor: pintar es escribir 24 propiedades en el
 * root, así que va a 60 fps sin problema. Persistir es otra cosa y lo hace el
 * editor al soltar.
 */
export function setCustomKnobs(next: unknown): ThemeKnobs {
  knobs = normalizeKnobs(next);
  knobsLoaded = true;
  try {
    localStorage.setItem(KNOBS_STORAGE_KEY, JSON.stringify(knobs));
  } catch {
    // ignore
  }
  if (typeof document !== "undefined" && document.documentElement.dataset.theme === "custom") {
    applyTheme("custom");
  }
  return knobs;
}

/** Aplica el tema persistido en config. Es el que ven todos los webviews. */
export function applyConfigTheme(
  uiTheme: string | null | undefined,
  custom?: unknown,
): ResolvedTheme {
  if (custom !== undefined) {
    knobs = normalizeKnobs(custom);
    knobsLoaded = true;
  }
  return applyTheme(normalizeTheme(uiTheme));
}

export function readCachedTheme(): UiTheme {
  try {
    return normalizeTheme(localStorage.getItem(STORAGE_KEY));
  } catch {
    return "system";
  }
}

/** El cache de perillas del webview. Evita el destello antes de leer config. */
export function readCachedKnobs(): ThemeKnobs {
  try {
    const raw = localStorage.getItem(KNOBS_STORAGE_KEY);
    return normalizeKnobs(raw ? JSON.parse(raw) : null);
  } catch {
    return normalizeKnobs(null);
  }
}

/**
 * El botón sol/luna de la barra: claro → oscuro → sistema.
 *
 * Recorre solo los tres de siempre a propósito. Es un atajo para el cambio
 * frecuente, no el selector: las paletas intermedias se eligen en Ajustes, y
 * pasar por siete con un botón sería peor que abrirlas.
 */
export function cycleTheme(theme: UiTheme): UiTheme {
  if (theme === "light") return "dark";
  if (theme === "dark") return "system";
  return "light";
}

export function themeLabel(theme: UiTheme): string {
  switch (theme) {
    case "light":
      return "Claro";
    case "sepia":
      return "Sepia";
    case "mist":
      return "Niebla";
    case "graphite":
      return "Grafito";
    case "midnight":
      return "Nocturno";
    case "dark":
      return "Oscuro";
    case "claude":
      return "Claude";
    case "claude-dark":
      return "Claude oscuro";
    case "custom":
      return "Personalizado";
    default:
      return "Sistema";
  }
}
