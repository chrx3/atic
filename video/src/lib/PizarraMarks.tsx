import type { Pt } from "./PizarraBar";

/** Formas de la pizarra (annotateDraw.ts): trazo liso, sin sombra ni contorno. */

const common = {
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
};

/** Flecha: línea hasta el cuello + triángulo relleno (AM:273, AD:232). */
export const ArrowShape = ({ from, to, color, width }: { from: Pt; to: Pt; color: string; width: number }) => {
  const dx = to[0] - from[0];
  const dy = to[1] - from[1];
  const len = Math.hypot(dx, dy);
  if (len < 0.5) {
    return <circle cx={from[0]} cy={from[1]} r={width / 2} fill={color} />;
  }
  const ux = dx / len;
  const uy = dy / len;
  const head = Math.min(Math.max(width * 3.6, 10), len);
  const half = head * 0.45;
  const nx = to[0] - ux * head;
  const ny = to[1] - uy * head;
  const a: Pt = [nx - uy * half, ny + ux * half];
  const b: Pt = [nx + uy * half, ny - ux * half];
  return (
    <g>
      <path d={`M${from[0]} ${from[1]}L${nx} ${ny}`} fill="none" stroke={color} strokeWidth={width} {...common} />
      <path d={`M${to[0]} ${to[1]}L${a[0]} ${a[1]}L${b[0]} ${b[1]}Z`} fill={color} />
    </g>
  );
};

/** Círculo: elipse inscrita en el rectángulo arrastrado, solo trazo. */
export const EllipseShape = ({ from, to, color, width }: { from: Pt; to: Pt; color: string; width: number }) => (
  <ellipse
    cx={(from[0] + to[0]) / 2}
    cy={(from[1] + to[1]) / 2}
    rx={Math.abs(to[0] - from[0]) / 2}
    ry={Math.abs(to[1] - from[1]) / 2}
    fill="none"
    stroke={color}
    strokeWidth={width}
    {...common}
  />
);

/** Lápiz: puntos unidos con curvas cuadráticas por el punto medio (AD:205-230). */
export const PenShape = ({ points, color, width }: { points: Pt[]; color: string; width: number }) => {
  if (points.length === 0) return null;
  const p0 = points[0];
  let d = `M${p0[0]} ${p0[1]}`;
  if (points.length === 1) {
    d += `L${p0[0]} ${p0[1]}`;
  } else {
    for (let i = 1; i < points.length - 1; i++) {
      const mx = (points[i][0] + points[i + 1][0]) / 2;
      const my = (points[i][1] + points[i + 1][1]) / 2;
      d += `Q${points[i][0]} ${points[i][1]} ${mx} ${my}`;
    }
    const last = points[points.length - 1];
    d += `L${last[0]} ${last[1]}`;
  }
  return <path d={d} fill="none" stroke={color} strokeWidth={width} {...common} />;
};

const TEXT_FONT = '600 26px "Segoe UI", system-ui, -apple-system, "Helvetica Neue", Arial, sans-serif';

/**
 * Texto de la pizarra. Confirmado: strokeText con el halo y encima fillText.
 * Escribiéndose (`editing`): contenteditable con text-shadow del halo.
 */
export const TextShape = ({
  at,
  text,
  color,
  halo,
  size = 26,
  editing = false,
  caret = false,
}: {
  at: Pt;
  text: string;
  color: string;
  halo: string;
  size?: number;
  editing?: boolean;
  caret?: boolean;
}) => {
  const base = {
    position: "absolute" as const,
    left: at[0],
    top: at[1],
    font: TEXT_FONT,
    fontSize: size,
    lineHeight: 1.25,
    whiteSpace: "pre" as const,
  };
  const caretEl = (
    <span
      style={{
        display: "inline-block",
        width: 2,
        height: size * 1.05,
        marginLeft: 1,
        verticalAlign: "top",
        marginTop: size * 0.1,
        background: color,
        opacity: caret ? 1 : 0,
      }}
    />
  );
  if (editing) {
    return (
      <div style={{ ...base, color, textShadow: `0 0 2px ${halo}, 0 0 2px ${halo}` }}>
        {text}
        {caretEl}
      </div>
    );
  }
  return (
    <>
      <div
        style={{
          ...base,
          color: halo,
          WebkitTextStroke: `${Math.max(2, size / 7)}px ${halo}`,
          paintOrder: "stroke fill",
        }}
      >
        {text}
      </div>
      <div style={{ ...base, color }}>{text}</div>
    </>
  );
};

/** Puntero del sistema (mismo trazo que el puntero del overlay de captura, CO:617-625). */
export const ArrowCursor = ({ x, y, scale = 1 }: { x: number; y: number; scale?: number }) => (
  <svg
    width={18}
    height={24}
    viewBox="0 0 18 24"
    fill="none"
    style={{
      position: "absolute",
      left: x - 1.2,
      top: y - 1.2,
      overflow: "visible",
      transformOrigin: "1.2px 1.2px",
      transform: `scale(${scale})`,
      filter: "drop-shadow(0 1px 1px rgb(0 0 0 / 25%))",
    }}
  >
    <path
      d="M1.2 1.2 1.2 20.2 6.1 15.4 9.4 23.1 12.2 21.9 8.8 14.1 16.2 13.9Z"
      fill="#fff"
      stroke="#111"
      strokeWidth={1.4}
      strokeLinejoin="round"
    />
  </svg>
);

/** Cursor en cruz (`crosshair`) que la pizarra pone sobre el lienzo. */
export const Crosshair = ({ x, y }: { x: number; y: number }) => {
  const s = 11;
  return (
    <svg
      width={s * 2 + 6}
      height={s * 2 + 6}
      viewBox={`${-s - 3} ${-s - 3} ${s * 2 + 6} ${s * 2 + 6}`}
      style={{ position: "absolute", left: x - s - 3, top: y - s - 3, overflow: "visible" }}
    >
      <g strokeLinecap="butt">
        <path d={`M${-s} 0H${s}M0 ${-s}V${s}`} stroke="#fff" strokeWidth={3} />
        <path d={`M${-s + 1} 0H${s - 1}M0 ${-s + 1}V${s - 1}`} stroke="#111" strokeWidth={1} />
      </g>
    </svg>
  );
};
