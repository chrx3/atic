import { CAM_FLAT, type Cam3D } from "../lib/cam3d";
import { EASE, lerp } from "../lib/theme";
import { seg } from "../lib/time";
import { BAR_MS, BEAT_MS, TOTAL_SECONDS, type Shot } from "./shots";

const easeInOut = (t: number) => (t < 0.5 ? 2 * t * t : 1 - Math.pow(-2 * t + 2, 2) / 2);

export type PulseTiming = {
  barMs: number;
  beatMs: number;
  /** Tramos de compases [desde, hasta) donde suena el bombo a negras. Sin él: patrón del corto original. */
  kickBars?: [number, number][];
};

const DEFAULT_TIMING: PulseTiming = { barMs: BAR_MS, beatMs: BEAT_MS };

/** Golpe del bombo (1 → 0 en ~0.25 s): hace «latir» el monitor con la música. */
export const kickPulse = (gMs: number, timing: PulseTiming = DEFAULT_TIMING) => {
  const t = gMs;
  const { barMs, beatMs } = timing;
  const bar = t / barMs;
  if (timing.kickBars) {
    const on = timing.kickBars.some(([a, b]) => bar >= a && bar < b);
    return on ? Math.exp(-(t % beatMs) / 110) : 0;
  }
  const inDrop = (bar >= 2 && bar < 13) || bar >= 16;
  if (t < 200) return Math.exp(-t / 110);
  if (inDrop) return Math.exp(-(t % beatMs) / 110);
  if (bar >= 13 && bar < 16) return 0.4 * Math.exp(-(t % (beatMs * 2)) / 160);
  return 0;
};

/**
 * Pose 3D del monitor durante un plano. Todos entran con un giro corto que
 * asienta a los ~0.4 s y luego derivan despacio; el signo alterna por plano
 * para que la cámara no gire siempre hacia el mismo lado.
 */
export const shotCam = (
  shot: Shot,
  index: number,
  t: number,
  dur: number,
  pulse: number,
  landscape = false,
): Cam3D => {
  const p = Math.min(1, t / dur);
  const enter = EASE.smoothOut(seg(t, 0, 420));
  const dir = index % 2 === 0 ? 1 : -1;
  const cam: Cam3D = { ...CAM_FLAT };

  switch (shot.move) {
    case "hook": {
      // Primer plano del notch que se abre a plano general.
      const k = easeInOut(seg(t, 0, dur));
      cam.s = lerp(landscape ? 1.6 : 2.0, 1.08, k);
      cam.oy = 4;
      cam.rx = lerp(20, 5, k);
      cam.ry = lerp(-16, 8, k);
      cam.rz = lerp(-3, 0, k);
      break;
    }
    case "orbit": {
      const k = easeInOut(p);
      cam.ry = lerp(-40, 16, k);
      cam.rx = lerp(12, 4, k);
      cam.s = lerp(1.2, 1.0, k);
      cam.tz = lerp(-120, 60, k);
      break;
    }
    case "push": {
      cam.ry = dir * (1 - enter) * 28;
      cam.rx = (1 - enter) * 9;
      cam.tz = (1 - enter) * -320 + p * 90;
      cam.s = lerp(0.96, 1.06, p);
      break;
    }
    case "swing": {
      cam.ry = dir * lerp(-22, 22, easeInOut(p));
      cam.rx = 5 - p * 6;
      cam.rz = dir * (1 - enter) * 4;
      cam.tz = (1 - enter) * -220;
      cam.s = 1 + 0.05 * p;
      break;
    }
    case "rise": {
      cam.rx = lerp(24, 2, enter) - p * 3;
      cam.ty = (1 - enter) * 180;
      cam.ry = dir * lerp(-8, 8, p);
      cam.tz = (1 - enter) * -260;
      cam.s = 1 + 0.04 * p;
      break;
    }
    case "settle": {
      // Plano de remate: casi quieto para que el titular respire.
      const k = easeInOut(p);
      cam.s = lerp(1.0, 0.9, k);
      cam.ry = lerp(dir * 10, 0, k);
      cam.rx = lerp(5, 0, k);
      break;
    }
    case "pull": {
      const k = easeInOut(p);
      cam.s = lerp(1.16, 0.94, k);
      cam.ry = lerp(dir * 16, 0, k);
      cam.rx = lerp(6, 0, k);
      cam.rz = lerp(dir * -2, 0, k);
      break;
    }
  }
  cam.s *= 1 + 0.014 * pulse;
  return cam;
};

export const TOTAL_MS = TOTAL_SECONDS * 1000;
