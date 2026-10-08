import { AbsoluteFill, Easing } from "remotion";
import { blinkLid } from "../lib/AticMark";
import { useStageBg } from "../lib/format";
import { Caption } from "../lib/Caption";
import { ColorDesk, patchAt } from "../lib/ColorDesk";
import { ColorLoupe, LOUPE } from "../lib/ColorLoupe";
import { Notch } from "../lib/Notch";
import type { Pt } from "../lib/PizarraBar";
import { ArrowCursor } from "../lib/PizarraMarks";
import { Screen, SCREEN } from "../lib/Screen";
import { C } from "../lib/theme";
import { seg, useMs } from "../lib/time";

/* ---------- guion (ms) ---------- */

const T = {
  stripOpen: 200,
  cursorIn: 100,
  hoverFrom: 450,
  hoverTo: 950,
  stripClose: 950,
  /** Ctrl + Shift + C: nace la lupa junto al cursor. */
  loupe: 1300,
  /** Primer parche leído (muestreo a ~30 fps). */
  firstSample: 1390,
  /** Clic: copia el valor y la gota se despide con destello (200 ms). */
  copy: 4200,
} as const;

const SAMPLE_MS = 33;
const PILL_CELL: Pt = [SCREEN.w / 2 + 161, 24];

/** Recorrido del cursor: cada parada cae sobre una zona de color distinta del escritorio. */
const STOPS: { at: number; p: Pt }[] = [
  { at: 1300, p: [150, 124] },
  { at: 1700, p: [128, 152] },
  { at: 2040, p: [74, 178] },
  { at: 2380, p: [184, 248] },
  { at: 2720, p: [72, 306] },
  { at: 3060, p: [130, 380] },
  { at: 3400, p: [186, 443] },
  { at: 3740, p: [140, 506] },
];
const HOP_MS = 260;

const travel = Easing.bezier(0.3, 0, 0.2, 1);

type Move = { t0: number; t1: number; to: Pt };
const MOVES: Move[] = [
  { t0: T.cursorIn, t1: 420, to: PILL_CELL },
  { t0: 900, t1: STOPS[0].at, to: STOPS[0].p },
  ...STOPS.slice(1).map((s) => ({ t0: s.at - HOP_MS, t1: s.at, to: s.p })),
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

export const ColorScene = () => {
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
        <ColorDesk />

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
