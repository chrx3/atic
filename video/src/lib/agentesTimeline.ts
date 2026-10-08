import { Easing } from "remotion";
import type { Camera } from "./Screen";
import { seg } from "./time";

/** Ventana del SO de agentes (1120×760 + barra de título) dibujada a escala dentro de la pantalla lógica. */
export const WIN = { s: 0.44, x: 3, y: 190, w: 1120, h: 760, bar: 32 } as const;

/** Punto de la pizarra (u,v desde su esquina) → coordenadas lógicas de la pantalla. */
export const boardToScreen = (u: number, v: number) => ({
  x: WIN.x + WIN.s * u,
  y: WIN.y + WIN.s * (v + WIN.bar),
});

/** Instantes (ms) de la escena. */
export const T = {
  stripOpen: 200,
  hoverFrom: 560,
  winOpen: 900,
  click: 1500,
  cardBorn: 1550,
  composerFocus: 1800,
  bootEnd: 1900,
  typeStart: 1900,
  typeMsPerChar: 20,
  send: 2600,
  working: 2650,
  dialog: 3800,
  cueWaiting: 4400,
  faceOpen: 4650,
  approvePress: 5480,
  approve: 5560,
  ready: 6200,
} as const;

export const PROMPT = "Revisa por qué falla el build";

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
  top: { z: 1.15, fx: 250, fy: 0, ax: 250, ay: 0 } as Camera,
  board: camWin(0.78, 560, 360),
  card: camWin(0.58, 640, 380),
  term: camWin(0.9, 540, 400),
  over: { z: 1, fx: 250, fy: 0, ax: 250, ay: 0 } as Camera,
};

const MOVES: { at: number; dur: number; to: Camera }[] = [
  { at: 900, dur: 700, to: CAM.board },
  { at: 1600, dur: 650, to: CAM.card },
  { at: 2650, dur: 800, to: CAM.term },
  { at: 4100, dur: 600, to: CAM.top },
  { at: 5700, dur: 700, to: CAM.over },
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

/** Trayecto del cursor: cada punto es el instante de llegada; entre puntos se suaviza. */
export type Waypoint = { at: number; x: number; y: number };

export const pathAt = (ms: number, pts: Waypoint[]) => {
  if (ms <= pts[0].at) return { x: pts[0].x, y: pts[0].y };
  for (let i = 1; i < pts.length; i++) {
    if (ms <= pts[i].at) {
      const a = pts[i - 1];
      const b = pts[i];
      const k = CAM_EASE(seg(ms, a.at, b.at - a.at));
      return { x: a.x + (b.x - a.x) * k, y: a.y + (b.y - a.y) * k };
    }
  }
  const last = pts[pts.length - 1];
  return { x: last.x, y: last.y };
};
