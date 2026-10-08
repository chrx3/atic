import type { ComponentType } from "react";
import { AgentesScene } from "../scenes/AgentesScene";
import { ClipboardScene } from "../scenes/ClipboardScene";
import { ColorScene } from "../scenes/ColorScene";
import { FlipboardScene } from "../scenes/FlipboardScene";
import { LauncherScene } from "../scenes/LauncherScene";
import { NotchIntro } from "../scenes/NotchIntro";
import { PizarraScene } from "../scenes/PizarraScene";
import { TextosScene } from "../scenes/TextosScene";

/** 150 BPM: un tiempo = 0.4 s, un compás de 4 tiempos = 1.6 s. */
export const BPM = 150;
export const BEAT_MS = 60000 / BPM;
export const BAR_MS = BEAT_MS * 4;
export const TOTAL_SECONDS = 28.8;
export const FPS = 60;
export const TOTAL_FRAMES = Math.round(TOTAL_SECONDS * FPS);

export type ShotKind = "scene" | "montage" | "payoff" | "cta";

export type Shot = {
  id: string;
  kind: ShotKind;
  /** Inicio y duración en compases (1 compás = 1.6 s). */
  bar: number;
  bars: number;
  scene?: ComponentType;
  /** Tramos [desde, hasta] (ms) de la escena original, con cortes secos entre ellos. */
  segments?: [number, number][];
  /** Texto: línea principal (ES), línea secundaria (EN) y atajo. */
  es: string;
  en: string;
  keys?: string[];
  /** Tipo de movimiento de cámara 3D. */
  move: "hook" | "orbit" | "swing" | "rise" | "push" | "pull" | "settle";
};

export const SHOTS: Shot[] = [
  {
    id: "hook",
    kind: "scene",
    bar: 0,
    bars: 1,
    scene: NotchIntro,
    segments: [[0, 1200]],
    es: "ATIC",
    en: "Your desktop toolbox",
    move: "hook",
  },
  {
    id: "strip",
    kind: "scene",
    bar: 1,
    bars: 1,
    scene: NotchIntro,
    segments: [[850, 3300]],
    es: "TODO EN UNA PILL",
    en: "Everything in one pill",
    move: "orbit",
  },
  {
    id: "apps",
    kind: "scene",
    bar: 2,
    bars: 1,
    scene: LauncherScene,
    segments: [
      [650, 1750],
      [3400, 3900],
    ],
    es: "APPS",
    en: "Launch anything",
    keys: ["Ctrl", "Space"],
    move: "push",
  },
  {
    id: "clipboard",
    kind: "scene",
    bar: 3,
    bars: 1,
    scene: ClipboardScene,
    segments: [
      [700, 1600],
      [1780, 2050],
      [2450, 3150],
      [4300, 4650],
    ],
    es: "CLIPBOARD",
    en: "Everything you copy",
    keys: ["Ctrl", "Shift", "V"],
    move: "swing",
  },
  {
    id: "textos",
    kind: "scene",
    bar: 4,
    bars: 1,
    scene: TextosScene,
    segments: [
      [900, 1750],
      [1940, 2100],
      [2400, 2900],
      [3300, 3700],
    ],
    es: "TEXTOS",
    en: "Snippets, ready to paste",
    keys: ["Ctrl", "Shift", "S"],
    move: "rise",
  },
  {
    id: "flipboard",
    kind: "scene",
    bar: 5,
    bars: 2,
    scene: FlipboardScene,
    segments: [
      [600, 1600],
      [2200, 2900],
      [3350, 3990],
      [4200, 5450],
    ],
    es: "FLIPBOARD",
    en: "Flip a window. Write on the back.",
    keys: ["Ctrl", "Shift", "B"],
    move: "swing",
  },
  {
    id: "pizarra",
    kind: "scene",
    bar: 7,
    bars: 1,
    scene: PizarraScene,
    segments: [
      [1200, 2150],
      [2600, 3120],
      [3330, 3830],
    ],
    es: "PIZARRA",
    en: "Draw right on your screen",
    keys: ["Ctrl", "Shift", "X"],
    move: "pull",
  },
  {
    id: "color",
    kind: "scene",
    bar: 8,
    bars: 1,
    scene: ColorScene,
    segments: [
      [1250, 2050],
      [3400, 3700],
      [4100, 4450],
    ],
    es: "COLOR",
    en: "Pick any pixel",
    keys: ["Ctrl", "Shift", "C"],
    move: "push",
  },
  {
    id: "agentes",
    kind: "scene",
    bar: 9,
    bars: 2,
    scene: AgentesScene,
    segments: [
      [850, 1650],
      [1900, 2650],
      [3700, 4100],
      [4400, 5700],
      [6150, 6600],
    ],
    es: "AGENTES",
    en: "Your CLI agents, one shortcut away",
    keys: ["Ctrl", "Shift", "A"],
    move: "rise",
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
    bars: 3,
    es: "UNA PILL.\nTODO A UN ATAJO.",
    en: "One pill. Everything a shortcut away.",
    move: "settle",
  },
  {
    id: "cta",
    kind: "cta",
    bar: 16,
    bars: 2,
    es: "PRUÉBALA",
    en: "Try it",
    move: "pull",
  },
];

/** Atajos que desfilan durante el montaje, uno por planos (7 tomas). */
export const MONTAGE_KEYS: string[][] = [
  ["Ctrl", "Space"],
  ["Ctrl", "Shift", "V"],
  ["Ctrl", "Shift", "S"],
  ["Ctrl", "Shift", "B"],
  ["Ctrl", "Shift", "X"],
  ["Ctrl", "Shift", "C"],
  ["Ctrl", "Shift", "A"],
];

/** Cuadro (ms de la escena original) que representa a cada herramienta en el montaje. */
export const MONTAGE_CARDS: { scene: ComponentType; atMs: number }[] = [
  { scene: LauncherScene, atMs: 4000 },
  { scene: ClipboardScene, atMs: 3700 },
  { scene: TextosScene, atMs: 3900 },
  { scene: FlipboardScene, atMs: 4400 },
  { scene: PizarraScene, atMs: 4000 },
  { scene: ColorScene, atMs: 3000 },
  { scene: AgentesScene, atMs: 4700 },
];
