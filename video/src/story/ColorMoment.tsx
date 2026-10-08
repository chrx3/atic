import { AbsoluteFill, Easing } from "remotion";
import { blinkLid } from "../lib/AticMark";
import { Caption } from "../lib/Caption";
import { ColorLoupe, LOUPE } from "../lib/ColorLoupe";
import { useStageBg } from "../lib/format";
import { Notch } from "../lib/Notch";
import type { Pt } from "../lib/PizarraBar";
import { ArrowCursor } from "../lib/PizarraMarks";
import { Screen, SCREEN } from "../lib/Screen";
import { seg, useMs } from "../lib/time";
import { ColorStoryDesk, patchAt, swatchPoint } from "./ColorStoryDesk";

/**
 * Paso 5 de la historia: en el programa de diseño, Ctrl+Shift+C abre la lupa, que recorre la
 * paleta (Acento, Éxito, Aviso) y termina en el Primario; el clic copia #FF6B3D.
 */

/* ---------- guion (ms) ---------- */

const T = {
  cursorIn: 60,
  stripOpen: 100,
  hoverFrom: 300,
  hoverTo: 800,
  stripClose: 800,
  /** Ctrl + Shift + C: nace la lupa junto al cursor. */
  loupe: 1100,
  /** Primer parche leído (muestreo a ~30 fps). */
  firstSample: 1190,
  /** Clic: copia el valor y la gota se despide con destello (200 ms). */
  copy: 4100,
} as const;

const SAMPLE_MS = 33;
/** Celda de Color en la tira de la pill (índice 8). */
const PILL_CELL: Pt = [SCREEN.w / 2 + 161, 24];

/** Paradas del cursor: cada una cae sobre una muestra de la paleta (el hex de la lupa cambia). */
const STOPS: { at: number; hop: number; p: Pt }[] = [
  { at: 1100, hop: 300, p: swatchPoint("Acento", 6, -2) },
  { at: 1800, hop: 260, p: swatchPoint("Éxito", -8, 0) },
  { at: 2500, hop: 260, p: swatchPoint("Aviso", 8, 2) },
  { at: 3400, hop: 400, p: swatchPoint("Primario", -2, 0) },
];

const travel = Easing.bezier(0.3, 0, 0.2, 1);

type Move = { t0: number; t1: number; to: Pt };
const MOVES: Move[] = [
  { t0: T.cursorIn, t1: 380, to: PILL_CELL },
  ...STOPS.map((s) => ({ t0: s.at - s.hop, t1: s.at, to: s.p })),
];
const CURSOR_START: Pt = [472, 96];

const cursorAt = (ms: number): Pt => {
  let prev = CURSOR_START;
  for (const m of MOVES) {
    if (ms >= m.t1) {
      prev = m.to;
      continue;
    }
    if (ms >= m.t0) {
      const u = travel(seg(ms, m.t0, m.t1 - m.t0));
      return [prev[0] + (m.to[0] - prev[0]) * u, prev[1] + (m.to[1] - prev[1]) * u];
    }
    break;
  }
  return prev;
};

export const ColorMoment = () => {
  const ms = useMs();
  const cx = SCREEN.w / 2;
  const cursor = cursorAt(ms);

  // La lupa lee ~30 veces por segundo: el parche y el valor se refrescan a ese ritmo.
  const sampleMs = Math.floor(ms / SAMPLE_MS) * SAMPLE_MS;
  const sampled = cursorAt(sampleMs);
  const cells = ms >= T.firstSample ? patchAt(Math.floor(sampled[0]), Math.floor(sampled[1])) : null;

  const pressed = ms >= T.copy && ms < T.copy + 110;
  const loupeVisible = ms >= T.loupe && ms < T.copy + 200;

  return (
    <AbsoluteFill style={{ background: useStageBg() }}>
      <Caption title="Color" sub="Elige cualquier píxel y se copia" keys={["Ctrl", "Shift", "C"]} />
      <Screen camera={{ z: 1, fx: 0, fy: 0, ax: 0, ay: 0 }} wallpaper={false}>
        <ColorStoryDesk />

        <Notch
          cx={cx}
          toggles={[T.stripOpen, T.stripClose]}
          hovers={[{ index: 8, fromMs: T.hoverFrom, toMs: T.hoverTo }]}
          lookX={Math.max(-1, Math.min(1, (cursor[0] - cx) / 160)) * 0.9}
          lookY={Math.max(0, Math.min(1, (cursor[1] - 20) / 240)) * 0.7}
          lid={blinkLid(ms, [700])}
        />

        {loupeVisible && (
          <ColorLoupe
            x={cursor[0] + LOUPE.offset}
            y={cursor[1] + LOUPE.offset}
            ms={ms}
            inAt={T.loupe}
            readyAt={ms >= T.firstSample ? T.firstSample : null}
            copyAt={ms >= T.copy ? T.copy : null}
            cells={cells}
          />
        )}

        <div style={{ opacity: seg(ms, T.cursorIn - 20, 120) }}>
          <ArrowCursor x={cursor[0]} y={cursor[1]} scale={pressed ? 0.88 : 1} />
        </div>
      </Screen>
    </AbsoluteFill>
  );
};

/**
 * Tramos de la escena (ms) que, seguidos, la cuentan en 1.6 s (≈1.4×): nace la lupa sobre el
 * Acento, el salto Éxito → Aviso, y la vuelta al Primario con el clic y el destello.
 */
export const COLOR_SEGMENTS: [number, number][] = [
  [1050, 1650],
  [2250, 2750],
  [3300, 4400],
];
/** La lupa lee el Primario (#FF6B3D) justo antes del clic. */
export const COLOR_HERO_MS = 4040;
