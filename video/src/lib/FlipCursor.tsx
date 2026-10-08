import { Easing } from "remotion";
import { TIcon } from "./textosIcons";
import { seg } from "./time";

/** Puntero del video (capa de guion: la app real no dibuja cursor). */
export type CursorKey = {
  ms: number;
  x: number;
  y: number;
  /** Tramo lineal desde la clave anterior (para seguir un trazo). */
  linear?: boolean;
};

const MOVE = Easing.bezier(0.45, 0, 0.2, 1);

/** Posición interpolada del puntero; `null` antes de la primera clave. */
export const cursorPos = (keys: CursorKey[], ms: number) => {
  if (keys.length === 0 || ms < keys[0].ms) return null;
  const last = keys[keys.length - 1];
  if (ms >= last.ms) return { x: last.x, y: last.y };
  let i = 0;
  while (i < keys.length - 1 && ms >= keys[i + 1].ms) i++;
  const a = keys[i];
  const b = keys[i + 1];
  const raw = seg(ms, a.ms, b.ms - a.ms);
  const k = b.linear ? raw : MOVE(raw);
  return { x: a.x + (b.x - a.x) * k, y: a.y + (b.y - a.y) * k };
};

export const FlipCursor = ({
  ms,
  keys,
  clicks = [],
  size = 22,
  fadeOutAt,
}: {
  ms: number;
  keys: CursorKey[];
  /** Instantes de clic: encoge el puntero y suelta un aro. */
  clicks?: number[];
  size?: number;
  /** Instante en que el puntero se desvanece (200 ms). */
  fadeOutAt?: number;
}) => {
  const pos = cursorPos(keys, ms);
  if (!pos) return null;

  const fadeIn = seg(ms, keys[0].ms, 200) * (fadeOutAt === undefined ? 1 : 1 - seg(ms, fadeOutAt, 200));
  let press = 1;
  let ring: { t: number; x: number; y: number } | null = null;
  for (const c of clicks) {
    const d = ms - c;
    if (d < 0) continue;
    if (d < 160) press = Math.min(press, d < 70 ? 1 - 0.16 * (d / 70) : 0.84 + 0.16 * seg(d, 70, 90));
    if (d < 420) {
      const at = cursorPos(keys, c) ?? pos;
      ring = { t: d / 420, x: at.x, y: at.y };
    }
  }

  return (
    <>
      {ring && (
        <div
          style={{
            position: "absolute",
            left: ring.x - size * 0.9,
            top: ring.y - size * 0.9,
            width: size * 1.8,
            height: size * 1.8,
            borderRadius: "50%",
            boxSizing: "border-box",
            border: `${Math.max(1.5, size * 0.07)}px solid rgb(240 240 234 / 70%)`,
            opacity: (1 - ring.t) * 0.8,
            transform: `scale(${0.2 + 0.8 * Easing.out(Easing.cubic)(ring.t)})`,
            pointerEvents: "none",
          }}
        />
      )}
      <div
        style={{
          position: "absolute",
          left: pos.x - size * 0.17,
          top: pos.y - size * 0.17,
          width: size,
          height: size,
          opacity: fadeIn,
          transform: `scale(${press})`,
          transformOrigin: `${size * 0.17}px ${size * 0.17}px`,
          filter: "drop-shadow(0 2px 3px rgb(0 0 0 / 45%))",
          color: "#f7f7f2",
          pointerEvents: "none",
        }}
      >
        <TIcon name="pointer" size={size} strokeWidth={1.4} style={{ fill: "#f7f7f2", stroke: "#121211" }} />
      </div>
    </>
  );
};
