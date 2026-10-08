import { Easing } from "remotion";
import { WIN } from "../lib/agentesTimeline";
import type { Camera } from "../lib/Screen";
import { seg } from "../lib/time";

/** Instantes (ms) del momento «Agentes» de la historia (Ctrl+Shift+A → Aprobar → «12 passed»). */
export const T = {
  /** Ctrl+Shift+A: la ventana de agentes aparece. */
  winOpen: 450,
  hoverCard: 900,
  click: 1150,
  cardBorn: 1200,
  composerFocus: 1450,
  bootEnd: 1550,
  typeStart: 1600,
  typeMsPerChar: 17,
  send: 2500,
  working: 2550,
  /** El agente edita src/db.ts: el editor del fondo cambia. */
  edit: 3750,
  dialog: 4100,
  cueWaiting: 4500,
  faceOpen: 4750,
  approvePress: 5500,
  approve: 5580,
  ready: 6150,
  /** La ventana de agentes se retira y el editor del fondo muestra la prueba en verde. */
  winClose: 6700,
  editorPass: 6750,
  end: 8000,
} as const;

export const PROMPT = "Sube el pool de conexiones y arregla el timeout";

export type AgentStatus = "idle" | "working" | "waiting" | "unread";

export const statusAt = (ms: number): AgentStatus =>
  ms < T.working ? "idle" : ms < T.dialog ? "working" : ms < T.approve ? "waiting" : ms < T.ready ? "working" : "unread";

const CAM_EASE = Easing.bezier(0.45, 0, 0.2, 1);

const camWin = (eff: number, u: number, v: number): Camera => ({
  z: eff / WIN.s,
  fx: WIN.x + WIN.s * u,
  fy: WIN.y + WIN.s * (v + WIN.bar),
  ax: 250,
  ay: 330,
});

export const CAM = {
  over: { z: 1, fx: 250, fy: 0, ax: 250, ay: 0 } as Camera,
  board: camWin(0.78, 560, 360),
  card: camWin(0.58, 640, 380),
  term: camWin(0.9, 540, 400),
  /** Pill de frente: cara de permiso. */
  top: { z: 1.15, fx: 250, fy: 0, ax: 250, ay: 0 } as Camera,
  /** Pill arriba y el final de la salida del agente abajo (12 passed). */
  result: { z: 1.42, fx: 250, fy: 0, ax: 250, ay: 0 } as Camera,
  /** Código corregido arriba y terminal del editor abajo (esquina inferior izquierda del editor). */
  editorTerm: { z: 1.3, fx: 14, fy: 92, ax: 0, ay: 0 } as Camera,
};

const MOVES: { at: number; dur: number; to: Camera }[] = [
  { at: T.winOpen, dur: 650, to: CAM.board },
  { at: T.click + 50, dur: 600, to: CAM.card },
  { at: T.send - 50, dur: 750, to: CAM.term },
  { at: T.dialog + 300, dur: 600, to: CAM.top },
  { at: T.approve + 120, dur: 600, to: CAM.result },
  { at: T.winClose - 50, dur: 700, to: CAM.editorTerm },
];

const mixCam = (a: Camera, b: Camera, t: number): Camera => ({
  z: a.z * Math.pow(b.z / a.z, t),
  fx: a.fx + (b.fx - a.fx) * t,
  fy: a.fy + (b.fy - a.fy) * t,
  ax: a.ax + (b.ax - a.ax) * t,
  ay: a.ay + (b.ay - a.ay) * t,
});

export const cameraAt = (ms: number): Camera => {
  let cam = CAM.over;
  for (const m of MOVES) {
    if (ms < m.at) break;
    const raw = seg(ms, m.at, m.dur);
    if (raw < 1) return mixCam(cam, m.to, CAM_EASE(raw));
    cam = m.to;
  }
  return cam;
};
