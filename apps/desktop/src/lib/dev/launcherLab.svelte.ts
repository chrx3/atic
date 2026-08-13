/**
 * Perillas en vivo del launcher + piel líquida del overlay.
 * Solo para iterar números; cuando queden bien se copian a constantes / CSS.
 *
 * Persistencia: `localStorage` clave `atic-launcher-lab`.
 * Abrir/cerrar: Ctrl+Alt+F (dev). Esc o × cierra el panel.
 */

import { GOO_GROW } from "$lib/GooFilter.svelte";
import { BLEND, CELL } from "$liquid/constants";
import { sminReach } from "$liquid/sdf";

const STORAGE_KEY = "atic-launcher-lab";
/** Flag entre ventanas (main ↔ overlay), como `atic-liquid-lab`. */
export const LAUNCHER_LAB_OPEN_KEY = "atic-launcher-lab-open";

export type LauncherLabValues = {
  /** BLEND del Skin (REACH = sminReach(blend)). */
  blend: number;
  /** Celda de muestreo del Skin. */
  cell: number;
  /** Hueco barra→grupo de favs (px). */
  favGap: number;
  /** Hueco entre dots de favs (px). Idle > REACH corta cuello. */
  dotGap: number;
  /** Morph open del float launcher (ms). */
  openDur: number;
  /** Morph close del float launcher (ms). */
  closeDur: number;
  /** Ancho stadium compacto (px). */
  barW: number;
  /** Alto stadium / ancla compacto (px). */
  barH: number;
  /** `--goo-grow` en px (filtro SVG; overlay SDF lo ignora). */
  gooGrow: number;
};

export const LAUNCHER_LAB_DEFAULTS: LauncherLabValues = {
  blend: BLEND,
  cell: CELL,
  /** Idle: 15 > REACH (12) → sin cuello; al acercarse (emerge) sí fusionan. */
  favGap: 15,
  dotGap: 15,
  openDur: 100,
  closeDur: 90,
  barW: 292,
  barH: 40,
  gooGrow: GOO_GROW,
};

function load(): LauncherLabValues {
  if (typeof localStorage === "undefined") return { ...LAUNCHER_LAB_DEFAULTS };
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return { ...LAUNCHER_LAB_DEFAULTS };
    const parsed = JSON.parse(raw) as Partial<LauncherLabValues>;
    return { ...LAUNCHER_LAB_DEFAULTS, ...parsed };
  } catch {
    return { ...LAUNCHER_LAB_DEFAULTS };
  }
}

function clamp(n: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, n));
}

class LauncherLab {
  open = $state(false);
  blend = $state(LAUNCHER_LAB_DEFAULTS.blend);
  cell = $state(LAUNCHER_LAB_DEFAULTS.cell);
  favGap = $state(LAUNCHER_LAB_DEFAULTS.favGap);
  dotGap = $state(LAUNCHER_LAB_DEFAULTS.dotGap);
  openDur = $state(LAUNCHER_LAB_DEFAULTS.openDur);
  closeDur = $state(LAUNCHER_LAB_DEFAULTS.closeDur);
  barW = $state(LAUNCHER_LAB_DEFAULTS.barW);
  barH = $state(LAUNCHER_LAB_DEFAULTS.barH);
  gooGrow = $state(LAUNCHER_LAB_DEFAULTS.gooGrow);

  constructor() {
    this.apply(load());
    this.open = false;
    if (typeof localStorage !== "undefined") {
      localStorage.removeItem(LAUNCHER_LAB_OPEN_KEY);
    }
  }

  get reach(): number {
    return Math.round(sminReach(this.blend) * 10) / 10;
  }

  apply(v: Partial<LauncherLabValues>): void {
    // Techos seguros: blend/cell extremos remeshan el Skin a costo cuadrático
    // y dejan el overlay “pegado” al abrir el launcher con el lab activo.
    if (v.blend != null) this.blend = clamp(v.blend, 0, 90);
    if (v.cell != null) this.cell = clamp(v.cell, 3, 12);
    if (v.favGap != null) this.favGap = clamp(v.favGap, 0, 80);
    if (v.dotGap != null) this.dotGap = clamp(v.dotGap, 0, 80);
    if (v.openDur != null) this.openDur = clamp(v.openDur, 40, 700);
    if (v.closeDur != null) this.closeDur = clamp(v.closeDur, 40, 300);
    if (v.barW != null) this.barW = clamp(v.barW, 240, 560);
    if (v.barH != null) this.barH = clamp(v.barH, 36, 72);
    if (v.gooGrow != null) this.gooGrow = clamp(v.gooGrow, 0, 8);
  }

  snapshot(): LauncherLabValues {
    return {
      blend: this.blend,
      cell: this.cell,
      favGap: this.favGap,
      dotGap: this.dotGap,
      openDur: this.openDur,
      closeDur: this.closeDur,
      barW: this.barW,
      barH: this.barH,
      gooGrow: this.gooGrow,
    };
  }

  persist(): void {
    if (typeof localStorage === "undefined") return;
    localStorage.setItem(STORAGE_KEY, JSON.stringify(this.snapshot()));
  }

  reset(): void {
    this.apply(LAUNCHER_LAB_DEFAULTS);
    this.persist();
  }

  toggle(): void {
    this.open = !this.open;
    this.#syncOpenFlag();
  }

  close(): void {
    this.open = false;
    this.#syncOpenFlag();
  }

  /** JSON listo para pegar al chat / al código. */
  asJson(): string {
    const v = this.snapshot();
    return JSON.stringify({ ...v, reach: this.reach }, null, 2);
  }

  #syncOpenFlag(): void {
    if (typeof localStorage === "undefined") return;
    if (this.open) localStorage.setItem(LAUNCHER_LAB_OPEN_KEY, "1");
    else localStorage.removeItem(LAUNCHER_LAB_OPEN_KEY);
  }
}

export const launcherLab = new LauncherLab();

export function commitLauncherLab(): void {
  launcherLab.persist();
}
