import type { Shot } from "../short/shots";
import { AGENTES_HERO_MS, AGENTES_SEGMENTS, AgentesMoment } from "./AgentesMoment";
import { CLIPBOARD_HERO_MS, CLIPBOARD_SEGMENTS, ClipboardMoment } from "./ClipboardMoment";
import { COLOR_HERO_MS, COLOR_SEGMENTS, ColorMoment } from "./ColorMoment";
import { FLIPBOARD_SEGMENTS, FlipboardMoment } from "./FlipboardMoment";
import { HOOK_SEGMENTS, HookMoment } from "./HookMoment";
import { LAUNCHER_HERO_MS, LAUNCHER_SEGMENTS, LauncherMoment } from "./LauncherMoment";
import { PayoffDeskMoment } from "./PayoffDeskMoment";
import { PIZARRA_HERO_MS, PIZARRA_SEGMENTS, PizarraMoment } from "./PizarraMoment";
import { STRIP_SEGMENTS, StripMoment } from "./StripMoment";
import { TEXTOS_HERO_MS, TEXTOS_SEGMENTS, TextosMoment } from "./TextosMoment";

/**
 * «Un bug en producción, sin salir de lo que hacías». Mismo esqueleto de 18
 * compases (1.6 s a 150 BPM) que el corto anterior, con contenido nuevo.
 */
export const STORY_SHOTS: Shot[] = [
  {
    id: "hook",
    kind: "scene",
    bar: 0,
    bars: 1,
    scene: HookMoment,
    segments: HOOK_SEGMENTS,
    es: "ALGO SE ROMPIÓ",
    en: "Something broke",
    move: "hook",
  },
  {
    id: "strip",
    kind: "scene",
    bar: 1,
    bars: 1,
    scene: StripMoment,
    segments: STRIP_SEGMENTS,
    es: "TODO EN UNA PILL",
    en: "Everything in one pill",
    move: "orbit",
  },
  {
    id: "clipboard",
    kind: "scene",
    bar: 2,
    bars: 1,
    scene: ClipboardMoment,
    segments: CLIPBOARD_SEGMENTS,
    es: "COPIA EL ERROR",
    en: "Copy the error",
    keys: ["Ctrl", "Shift", "V"],
    move: "push",
  },
  {
    id: "pizarra",
    kind: "scene",
    bar: 3,
    bars: 1,
    scene: PizarraMoment,
    segments: PIZARRA_SEGMENTS,
    es: "MÁRCALO",
    en: "Mark it up",
    keys: ["Ctrl", "Shift", "X"],
    move: "swing",
  },
  {
    id: "color",
    kind: "scene",
    bar: 4,
    bars: 1,
    scene: ColorMoment,
    segments: COLOR_SEGMENTS,
    es: "ELIGE EL COLOR",
    en: "Pick the color",
    keys: ["Ctrl", "Shift", "C"],
    move: "rise",
  },
  {
    id: "textos",
    kind: "scene",
    bar: 5,
    bars: 1,
    scene: TextosMoment,
    segments: TEXTOS_SEGMENTS,
    es: "RESPONDE AL TIRO",
    en: "Reply instantly",
    keys: ["Ctrl", "Shift", "S"],
    move: "push",
  },
  {
    id: "flipboard",
    kind: "scene",
    bar: 6,
    bars: 2,
    scene: FlipboardMoment,
    segments: FLIPBOARD_SEGMENTS,
    es: "ANOTA AL REVERSO",
    en: "Note it on the back",
    keys: ["Ctrl", "Shift", "B"],
    move: "swing",
  },
  {
    id: "agentes",
    kind: "scene",
    bar: 9,
    bars: 2,
    scene: AgentesMoment,
    segments: AGENTES_SEGMENTS,
    es: "QUE TU AGENTE LO ARREGLE",
    en: "Let your agent fix it",
    keys: ["Ctrl", "Shift", "A"],
    move: "rise",
  },
  {
    id: "apps",
    kind: "scene",
    bar: 8,
    bars: 1,
    scene: LauncherMoment,
    segments: LAUNCHER_SEGMENTS,
    es: "ABRE EL CORREO",
    en: "Open your mail",
    keys: ["Ctrl", "Space"],
    move: "push",
  },
  {
    id: "montage",
    kind: "montage",
    bar: 11,
    bars: 2,
    es: "UN ATAJO PARA TODO",
    en: "One shortcut for everything",
    move: "push",
  },
  {
    id: "payoff",
    kind: "payoff",
    bar: 13,
    bars: 1,
    scene: PayoffDeskMoment,
    es: "SIN SALIR DE\nLO QUE HACÍAS",
    en: "Without leaving what you were doing",
    move: "settle",
  },
  {
    id: "cta",
    kind: "cta",
    bar: 14,
    bars: 2,
    es: "PRUÉBALA",
    en: "Try it",
    move: "pull",
  },
];

export const STORY_MONTAGE_CARDS = [
  { scene: ClipboardMoment, atMs: CLIPBOARD_HERO_MS },
  { scene: PizarraMoment, atMs: PIZARRA_HERO_MS },
  { scene: ColorMoment, atMs: COLOR_HERO_MS },
  { scene: TextosMoment, atMs: TEXTOS_HERO_MS },
  { scene: FlipboardMoment, atMs: 5400 },
  { scene: AgentesMoment, atMs: AGENTES_HERO_MS },
  { scene: LauncherMoment, atMs: LAUNCHER_HERO_MS },
];

export const STORY_MONTAGE_KEYS: string[][] = [
  ["Ctrl", "Shift", "V"],
  ["Ctrl", "Shift", "X"],
  ["Ctrl", "Shift", "C"],
  ["Ctrl", "Shift", "S"],
  ["Ctrl", "Shift", "B"],
  ["Ctrl", "Shift", "A"],
  ["Ctrl", "Space"],
];
