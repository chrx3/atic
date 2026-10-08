/**
 * Guion del video sobre la grilla del tema: 128 BPM, el primer golpe cae a
 * los 15 ms y el drop entra en el tiempo 64 (30 s).
 */

export const FPS = 60;
export const BPM = 128;
export const BEAT_S = 60 / BPM;
export const FIRST_BEAT_S = 0.015;

export const beatToFrame = (beat: number) => Math.round((FIRST_BEAT_S + beat * BEAT_S) * FPS);

/** Recorte del clip en px CSS de la pantalla simulada (1080×1080). */
export type Crop = { x: number; y: number; w: number; h: number };

export type ToolShot = {
  id: string;
  title: string;
  sub: string;
  clip: string;
  /** Segundo del clip en que arranca el plano. */
  from: number;
  /** Encuadre al entrar y al salir (zoom lento entre ambos). */
  crop: Crop;
  cropEnd?: Crop;
  /** card: UI en tarjeta bajo el titular (blanco). full: pantalla completa. */
  look: "card" | "full";
};

export type Segment =
  | { kind: "hook"; beat: number; beats: number }
  | { kind: "brand"; beat: number; beats: number }
  | { kind: "pill"; beat: number; beats: number; shot: ToolShot }
  | { kind: "tool"; beat: number; beats: number; shot: ToolShot }
  | { kind: "montage"; beat: number; beats: number }
  | { kind: "end"; beat: number; beats: number };

export const PILL_SHOT: ToolShot = {
  id: "pill",
  title: "Vive en el notch.",
  sub: "Pasa el mouse y aparecen tus herramientas.",
  clip: "intro-pill.mp4",
  from: 0,
  crop: { x: 220, y: 0, w: 640, h: 640 },
  cropEnd: { x: 340, y: 0, w: 400, h: 400 },
  look: "full",
};

export const TOOLS: ToolShot[] = [
  { id: "launcher", title: "Abre lo que sea.", sub: "Apps · Ctrl + Espacio", clip: "launcher.mp4", from: 0.1, crop: { x: 250, y: 290, w: 780, h: 636 }, cropEnd: { x: 330, y: 380, w: 640, h: 522 }, look: "card" },
  { id: "clipboard", title: "Todo lo que copias.", sub: "Clipboard", clip: "clipboard.mp4", from: 0.7, crop: { x: 280, y: 0, w: 520, h: 424 }, cropEnd: { x: 340, y: 0, w: 500, h: 408 }, look: "card" },
  { id: "snippets", title: "Lo que escribes siempre.", sub: "Textos", clip: "snippets.mp4", from: 1.3, crop: { x: 300, y: 0, w: 480, h: 392 }, cropEnd: { x: 320, y: 0, w: 440, h: 359 }, look: "card" },
  { id: "captures", title: "Recorta y comparte.", sub: "Capturas", clip: "captures.mp4", from: 0.3, crop: { x: 100, y: 230, w: 980, h: 800 }, cropEnd: { x: 480, y: 560, w: 600, h: 490 }, look: "card" },
  { id: "board", title: "Marca la pantalla.", sub: "Pizarra", clip: "board.mp4", from: 0.3, crop: { x: 100, y: 0, w: 980, h: 800 }, cropEnd: { x: 170, y: 20, w: 850, h: 694 }, look: "card" },
  { id: "color", title: "Cualquier color, al píxel.", sub: "Color", clip: "color.mp4", from: 0.3, crop: { x: 120, y: 100, w: 900, h: 734 }, cropEnd: { x: 250, y: 110, w: 760, h: 620 }, look: "card" },
  { id: "agents", title: "Tus agentes, con interfaz.", sub: "Agentes · Claude Code, Codex, Cursor", clip: "agents.mp4", from: 0.45, crop: { x: 280, y: 290, w: 760, h: 760 }, cropEnd: { x: 330, y: 300, w: 660, h: 660 }, look: "full" },
  { id: "system", title: "Tu PC, bajo control.", sub: "Sistema", clip: "system.mp4", from: 1.2, crop: { x: 290, y: 0, w: 500, h: 500 }, cropEnd: { x: 300, y: 0, w: 480, h: 480 }, look: "full" },
  { id: "dictation", title: "Habla. Se escribe.", sub: "Dictado", clip: "dictation.mp4", from: 0, crop: { x: 405, y: 0, w: 270, h: 220 }, cropEnd: { x: 170, y: 0, w: 740, h: 604 }, look: "card" },
  { id: "meetings", title: "Graba y resume reuniones.", sub: "Reuniones · transcripción local", clip: "meetings.mp4", from: 0.9, crop: { x: 0, y: 0, w: 1080, h: 881 }, cropEnd: { x: 60, y: 60, w: 960, h: 783 }, look: "card" },
  { id: "flip", title: "Da vuelta la ventana.", sub: "Flipboard · notas en el reverso", clip: "flip.mp4", from: 0, crop: { x: 40, y: 120, w: 1000, h: 816 }, cropEnd: { x: 120, y: 140, w: 880, h: 718 }, look: "card" },
];

const TOOL_BEATS = 8;

function build(): Segment[] {
  const out: Segment[] = [
    { kind: "hook", beat: 0, beats: 4 },
    { kind: "brand", beat: 4, beats: 4 },
    { kind: "pill", beat: 8, beats: 8, shot: PILL_SHOT },
  ];
  let beat = 16;
  for (const shot of TOOLS) {
    out.push({ kind: "tool", beat, beats: TOOL_BEATS, shot });
    beat += TOOL_BEATS;
  }
  out.push({ kind: "montage", beat, beats: 8 });
  beat += 8;
  out.push({ kind: "end", beat, beats: 16 });
  return out;
}

export const SEGMENTS = build();
const last = SEGMENTS[SEGMENTS.length - 1];
export const TOTAL_FRAMES = beatToFrame(last.beat + last.beats);
