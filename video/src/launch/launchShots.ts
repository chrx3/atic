import type { ComponentType } from "react";
import { AGENTES_SEGMENTS, AgentesMoment } from "../story/AgentesMoment";
import { CAPTURAS_SEGMENTS, CapturasMoment } from "../story/CapturasMoment";
import { CLIPBOARD_SEGMENTS, ClipboardMoment } from "../story/ClipboardMoment";
import { COLOR_SEGMENTS, ColorMoment } from "../story/ColorMoment";
import { FLIPBOARD_SEGMENTS, FlipboardMoment } from "../story/FlipboardMoment";
import { HOOK_SEGMENTS, HookMoment } from "../story/HookMoment";
import { PIZARRA_SEGMENTS, PizarraMoment } from "../story/PizarraMoment";
import { STRIP_SEGMENTS, StripMoment } from "../story/StripMoment";
import { TEXTOS_SEGMENTS, TextosMoment } from "../story/TextosMoment";

/**
 * Video de lanzamiento 1:1 a 128 BPM con el tema «Tilt Tomorrow» desde el inicio (0 s).
 * Los cortes caen en tiempos o medios tiempos. El drop del tema llega a los 30.0 s
 * (tiempo 64): conviene que coincida con el arranque de una herramienta.
 */
export const LAUNCH_BPM = 128;
export const LAUNCH_BEAT_MS = 60000 / LAUNCH_BPM;
/** Cuánto más lento que el ritmo «natural» de cada escena se reproduce (1.05 = 5 % más despacio). */
const SLOW = 1.05;

export type LaunchShot =
  | { kind: "slide"; beat: number; beats: number; es: string; en: string; keys?: string[] }
  | {
      kind: "ui";
      beat: number;
      beats: number;
      scene: ComponentType;
      segments: [number, number][];
      chip?: { label: string; keys?: string[] };
      /** Zoom de entrada (más grande = más «golpe»). */
      punch?: number;
    }
  | { kind: "more"; beat: number; beats: number }
  | { kind: "end"; beat: number; beats: number };

const SLIDE = 2;

const totalMs = (segs: [number, number][]) => segs.reduce((n, [x, y]) => n + (y - x), 0);
/** Tiempos (múltiplos de 0.5) que necesita una escena para verse a su velocidad natural. */
const beatsFor = (segs: [number, number][]) => Math.max(3, Math.ceil(((totalMs(segs) * SLOW) / LAUNCH_BEAT_MS) * 2) / 2);

type Draft =
  | { kind: "slide"; beats: number; es: string; en: string; keys?: string[] }
  | { kind: "ui"; beats: number; scene: ComponentType; segments: [number, number][]; chip?: { label: string; keys?: string[] }; punch?: number }
  | { kind: "more"; beats: number }
  | { kind: "end"; beats: number };

const tool = (
  es: string,
  en: string,
  keys: string[],
  label: string,
  scene: ComponentType,
  segments: [number, number][],
): Draft[] => [
  { kind: "slide", beats: SLIDE, es, en, keys },
  { kind: "ui", beats: beatsFor(segments), scene, segments, chip: { label, keys } },
];

const DRAFT: Draft[] = [
  { kind: "slide", beats: 2, es: "¿Tantas ventanas\nabiertas?", en: "Too many windows open?" },
  { kind: "ui", beats: 3.5, scene: HookMoment, segments: HOOK_SEGMENTS, punch: 1.06 },
  { kind: "slide", beats: 2, es: "Una pill.\nUn atajo por herramienta.", en: "One pill. One shortcut per tool." },
  { kind: "ui", beats: beatsFor(STRIP_SEGMENTS), scene: StripMoment, segments: STRIP_SEGMENTS },
  ...tool("Todo lo que copias", "Everything you copy", ["Ctrl", "Shift", "V"], "Clipboard", ClipboardMoment, CLIPBOARD_SEGMENTS),
  ...tool("Marca lo que veas", "Draw on anything", ["Ctrl", "Shift", "X"], "Pizarra", PizarraMoment, PIZARRA_SEGMENTS),
  ...tool("Captura y listo", "Capture in a snap", ["Ctrl", "Shift", "4"], "Capturas", CapturasMoment, CAPTURAS_SEGMENTS),
  ...tool("Cualquier píxel", "Any pixel", ["Ctrl", "Shift", "C"], "Color", ColorMoment, COLOR_SEGMENTS),
  ...tool("Tus textos, al tiro", "Your snippets, instantly", ["Ctrl", "Shift", "S"], "Textos", TextosMoment, TEXTOS_SEGMENTS),
  ...tool("Da vuelta la ventana", "Flip the window", ["Ctrl", "Shift", "B"], "Flipboard", FlipboardMoment, FLIPBOARD_SEGMENTS),
  ...tool("Tus agentes,\nsin salir de lo que hacías", "Your agents, without leaving what you were doing", ["Ctrl", "Shift", "A"], "Agentes", AgentesMoment, AGENTES_SEGMENTS),
  { kind: "more", beats: 5 },
  { kind: "end", beats: 5.5 },
];

/** Reparte los tiempos en orden: cada plano empieza donde acaba el anterior. */
export const LAUNCH_SHOTS: LaunchShot[] = (() => {
  let beat = 0;
  return DRAFT.map((d) => {
    const shot = { ...d, beat } as LaunchShot;
    beat += d.beats;
    return shot;
  });
})();

export const LAUNCH_BEATS = LAUNCH_SHOTS.reduce((n, s) => Math.max(n, s.beat + s.beats), 0);
export const LAUNCH_FRAMES = Math.round((LAUNCH_BEATS * LAUNCH_BEAT_MS * 60) / 1000);
