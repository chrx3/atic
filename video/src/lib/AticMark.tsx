import { C } from "./theme";

type Props = {
  size?: number;
  strokeWidth?: number;
  color?: string;
  /** Mirada en unidades del viewBox (máx 1.18 en la app). */
  lookX?: number;
  lookY?: number;
  /** Párpado 1 = abierto, 0.1 = cerrado. */
  lid?: number;
  /** Ms de escena, para el vaivén de ±1.2°. */
  ms?: number;
  /** Cuadrado rojo de "grabando" en lugar de los ojos. */
  recording?: boolean;
};

const EYE_Y = 11.15;
const MAX_LOOK = 1.18;

/** Logo vivo de Atic (lib/AticMark.svelte): una «a» con dos ojos que miran. */
export const AticMark = ({
  size = 32,
  strokeWidth = 1.6,
  color = C.text,
  lookX = 0,
  lookY = 0,
  lid = 1,
  ms = 0,
  recording = false,
}: Props) => {
  const sway = Math.sin(ms / 1100) * 1.2;
  const rot = (lookX / MAX_LOOK) * 12 * 0.6 + sway;
  const spread = 1.88 + Math.abs(lookX) * 0.1;
  const ry = Math.max(0.08, 1.32 * lid);
  const eye = (cx: number, side: 1 | -1) => (
    <g transform={`translate(${cx + lookX} ${EYE_Y + lookY})`}>
      <ellipse cx="0" cy="0" rx="0.88" ry={ry} fill="currentColor" stroke="none" />
      {lid > 0.88 && (
        <path
          d={`M ${0.836 * side} 1.241 Q ${0.312 * side} 1.684 ${-0.062 * side} 1.861`}
          fill="none"
          stroke="currentColor"
          strokeWidth="0.14"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      )}
    </g>
  );
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={strokeWidth}
      strokeLinecap="butt"
      style={{ overflow: "visible", display: "block", color }}
    >
      <g
        style={{
          transformBox: "view-box",
          transformOrigin: "12px 12px",
          transform: `translate(${lookX * 0.9}px, ${lookY * 0.4}px) rotate(${rot}deg)`,
        }}
      >
        <circle cx="12" cy="12" r="5.5" />
        <path d="M17.5 6.5V17.5" />
        {recording ? (
          <rect x="9.8" y="9.8" width="4.4" height="4.4" rx="1.15" fill={C.rec} stroke="none" />
        ) : (
          <>
            {eye(12 - spread, 1)}
            {eye(12 + spread, -1)}
          </>
        )}
      </g>
    </svg>
  );
};

/** Parpadeo determinista: devuelve el párpado (1 abierto) en `ms`. */
export const blinkLid = (ms: number, blinkAtMs: number[]) => {
  for (const t of blinkAtMs) {
    const d = ms - t;
    if (d >= 0 && d < 110) {
      const k = d < 55 ? d / 55 : (110 - d) / 55;
      return 1 - 0.9 * k;
    }
  }
  return 1;
};
