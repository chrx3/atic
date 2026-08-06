/**
 * Perillas en vivo del picker (rueda + cards). Solo para encontrar números;
 * cuando queden bien se copian a `ToolRail` / `constants.ts`.
 *
 * Persistencia: `localStorage` clave `atic-picker-lab`. Abrir/cerrar el panel:
 * Ctrl+Alt+P en la ventana principal (dev).
 */

const STORAGE_KEY = "atic-picker-lab";

export type PickerLabValues = {
  blend: number;
  cell: number;
  cardFloat: number;
  /** Extra sobre el pitch mínimo (hot/2 + cold/2). */
  pitchPad: number;
  /** Fracción del alto usada para repartir ±2 slots. */
  heightFill: number;
  hotX: number;
  hotXExpanded: number;
  stepDeg: number;
  stepDegExpanded: number;
  cardHotW: number;
  cardHotH: number;
  cardColdW: number;
  cardColdH: number;
};

/** Valores afinados a mano en el picker lab (2026-08). */
export const PICKER_LAB_DEFAULTS: PickerLabValues = {
  blend: 120,
  cell: 16,
  cardFloat: 99,
  pitchPad: 120,
  heightFill: 0.87,
  hotX: 40,
  hotXExpanded: 89,
  stepDeg: 45,
  stepDegExpanded: 20,
  cardHotW: 420,
  cardHotH: 154,
  cardColdW: 248,
  cardColdH: 64,
};

function load(): PickerLabValues {
  if (typeof localStorage === "undefined") return { ...PICKER_LAB_DEFAULTS };
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return { ...PICKER_LAB_DEFAULTS };
    return { ...PICKER_LAB_DEFAULTS, ...JSON.parse(raw) };
  } catch {
    return { ...PICKER_LAB_DEFAULTS };
  }
}

function clamp(n: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, n));
}

class PickerLab {
  open = $state(false);
  blend = $state(PICKER_LAB_DEFAULTS.blend);
  cell = $state(PICKER_LAB_DEFAULTS.cell);
  cardFloat = $state(PICKER_LAB_DEFAULTS.cardFloat);
  pitchPad = $state(PICKER_LAB_DEFAULTS.pitchPad);
  heightFill = $state(PICKER_LAB_DEFAULTS.heightFill);
  hotX = $state(PICKER_LAB_DEFAULTS.hotX);
  hotXExpanded = $state(PICKER_LAB_DEFAULTS.hotXExpanded);
  stepDeg = $state(PICKER_LAB_DEFAULTS.stepDeg);
  stepDegExpanded = $state(PICKER_LAB_DEFAULTS.stepDegExpanded);
  cardHotW = $state(PICKER_LAB_DEFAULTS.cardHotW);
  cardHotH = $state(PICKER_LAB_DEFAULTS.cardHotH);
  cardColdW = $state(PICKER_LAB_DEFAULTS.cardColdW);
  cardColdH = $state(PICKER_LAB_DEFAULTS.cardColdH);

  constructor() {
    // El panel SIEMPRE arranca cerrado: persistir `open` dejaba la UI atrapada.
    this.apply(load());
    this.open = false;
    if (typeof localStorage !== "undefined") {
      localStorage.removeItem("atic-picker-lab-open");
    }
  }

  apply(v: Partial<PickerLabValues>): void {
    if (v.blend != null) this.blend = clamp(v.blend, 0, 120);
    if (v.cell != null) this.cell = clamp(v.cell, 2, 16);
    if (v.cardFloat != null) this.cardFloat = clamp(v.cardFloat, 0, 200);
    if (v.pitchPad != null) this.pitchPad = clamp(v.pitchPad, 0, 120);
    if (v.heightFill != null) this.heightFill = clamp(v.heightFill, 0.5, 1);
    if (v.hotX != null) this.hotX = clamp(v.hotX, 40, 160);
    if (v.hotXExpanded != null) this.hotXExpanded = clamp(v.hotXExpanded, 40, 180);
    if (v.stepDeg != null) this.stepDeg = clamp(v.stepDeg, 8, 45);
    if (v.stepDegExpanded != null) {
      this.stepDegExpanded = clamp(v.stepDegExpanded, 8, 40);
    }
    if (v.cardHotW != null) this.cardHotW = clamp(v.cardHotW, 180, 420);
    if (v.cardHotH != null) this.cardHotH = clamp(v.cardHotH, 80, 220);
    if (v.cardColdW != null) this.cardColdW = clamp(v.cardColdW, 140, 360);
    if (v.cardColdH != null) this.cardColdH = clamp(v.cardColdH, 40, 120);
  }

  snapshot(): PickerLabValues {
    return {
      blend: this.blend,
      cell: this.cell,
      cardFloat: this.cardFloat,
      pitchPad: this.pitchPad,
      heightFill: this.heightFill,
      hotX: this.hotX,
      hotXExpanded: this.hotXExpanded,
      stepDeg: this.stepDeg,
      stepDegExpanded: this.stepDegExpanded,
      cardHotW: this.cardHotW,
      cardHotH: this.cardHotH,
      cardColdW: this.cardColdW,
      cardColdH: this.cardColdH,
    };
  }

  persist(): void {
    if (typeof localStorage === "undefined") return;
    localStorage.setItem(STORAGE_KEY, JSON.stringify(this.snapshot()));
  }

  reset(): void {
    this.apply(PICKER_LAB_DEFAULTS);
    this.persist();
  }

  toggle(): void {
    this.open = !this.open;
  }

  close(): void {
    this.open = false;
  }

  /** Snippet listo para pegar en el código. */
  asCode(): string {
    const v = this.snapshot();
    return [
      `blend: ${v.blend}`,
      `cell: ${v.cell}`,
      `cardFloat: ${v.cardFloat}`,
      `pitchPad: ${v.pitchPad}`,
      `heightFill: ${v.heightFill}`,
      `hotX: ${v.hotX} / expanded ${v.hotXExpanded}`,
      `stepDeg: ${v.stepDeg} / expanded ${v.stepDegExpanded}`,
      `cardHot: ${v.cardHotW}×${v.cardHotH}`,
      `cardCold: ${v.cardColdW}×${v.cardColdH}`,
    ].join("\n");
  }
}

export const pickerLab = new PickerLab();

/** Persistir al soltar un slider (el panel lo llama). */
export function commitPickerLab(): void {
  pickerLab.persist();
}
