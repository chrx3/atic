import type { ReactNode } from "react";
import { AticMark } from "./AticMark";
import { Icon, type IconName } from "./Icon";
import { C, EASE, ISLAND, PILL, SHADOW_GOO, lerp } from "./theme";
import { seg, toggled, useMs } from "./time";

/** Tira por defecto: marca · 8 herramientas · Ventana (N = 10). */
export const STRIP: ("mark" | IconName)[] = [
  "mark",
  "meetings",
  "clipboard",
  "snippets",
  "agents",
  "system",
  "captures",
  "board",
  "color",
  "window",
];

export type Hover = { index: number; fromMs: number; toMs?: number };

type Props = {
  /** Centro horizontal de la pill en coordenadas lógicas de la pantalla. */
  cx: number;
  /** Instantes (ms) que alternan abrir / cerrar la tira. */
  toggles?: number[];
  hovers?: Hover[];
  lookX?: number;
  lookY?: number;
  lid?: number;
  /** Celdas resaltadas de forma fija (p. ej. la herramienta activa). */
  activeIndex?: number | null;
  /** Grabando: la marca cerrada muestra el cuadrado rojo. */
  recording?: boolean;
  children?: ReactNode;
};

const CLOSED = { w: PILL.islandLong, h: 44 };
const N = STRIP.length;
const OPEN = { w: N * PILL.islandTool + (N - 1) * PILL.islandGap, h: 48 };

/** Pill acoplada al techo (surface = edge): silueta mate con esquinas inferiores redondas. */
export const Notch = ({
  cx,
  toggles = [],
  hovers = [],
  lookX = 0,
  lookY = 0,
  lid = 1,
  activeIndex = null,
  recording = false,
  children,
}: Props) => {
  const ms = useMs();
  const p = toggled(ms, toggles, ISLAND.openMs, EASE.island);
  const markOut = toggled(ms, toggles, ISLAND.openMs, EASE.liquid);

  const W = lerp(CLOSED.w, OPEN.w, p);
  const H = lerp(CLOSED.h, OPEN.h, p);
  const r = H / 2;
  const l = cx - W / 2;
  const rt = cx + W / 2;
  const body = `M${l} 0V${H - r}A${r} ${r} 0 0 0 ${l + r} ${H}H${rt - r}A${r} ${r} 0 0 0 ${rt} ${H - r}V0Z`;
  // Hombro contra el techo: menisco de 8 px de alto, 12 px más ancho por lado.
  const sl = l - 12;
  const sr = rt + 12;
  const shoulder = `M${sl + 4} 0A20 20 0 0 0 ${sl + 20} 8H${sr - 20}A20 20 0 0 0 ${sr - 4} 0Z`;

  const pad = 60;
  return (
    <div style={{ position: "absolute", left: 0, top: 0, width: 0, height: 0 }}>
      <svg
        width={cx * 2 + pad * 2}
        height={H + pad * 2}
        viewBox={`${-pad} ${-pad} ${cx * 2 + pad * 2} ${H + pad * 2}`}
        style={{
          position: "absolute",
          left: -pad,
          top: -pad,
          overflow: "visible",
          filter: SHADOW_GOO,
        }}
      >
        <g fill={C.skin} stroke={C.skin} strokeWidth={1.25} strokeLinejoin="round">
          <path d={body} />
          <path d={shoulder} />
        </g>
      </svg>

      {/* marca de la pestaña cerrada */}
      <div
        style={{
          position: "absolute",
          left: cx - PILL.islandMark / 2,
          top: 20 - PILL.islandMark / 2,
          opacity: 1 - markOut,
        }}
      >
        <AticMark
          ms={ms}
          lookX={lookX}
          lookY={lookY}
          lid={lid}
          recording={recording}
        />
      </div>

      {/* tira de herramientas: recortada a la silueta para que no queden iconos fuera al cerrar */}
      <div
        style={{
          position: "absolute",
          left: l,
          top: 0,
          width: W,
          height: H,
          overflow: "hidden",
          borderRadius: `0 0 ${r}px ${r}px`,
        }}
      >
      <div style={{ position: "absolute", left: -l, top: 0 }}>
      {STRIP.map((id, i) => {
        const delay = Math.abs((N - 1) / 2 - i) * ISLAND.staggerMs;
        // Al cerrar salen primero las celdas de los extremos, junto con la silueta.
        const closeDelay = ((N - 1) / 2 - Math.abs((N - 1) / 2 - i)) * ISLAND.staggerMs;
        const t = toggled(ms, toggles, ISLAND.openMs, EASE.liquid, delay, closeDelay);
        const bunch = ((N - 1) / 2 - i) * (PILL.islandTool + PILL.islandGap);
        const tx = bunch * 0.22 * (1 - t);
        const sc = lerp(0.91, 1, t);

        const h = hovers.find((x) => x.index === i && ms >= x.fromMs);
        let hover = 0;
        if (h) {
          const inn = EASE.liquid(seg(ms, h.fromMs, 240));
          hover = h.toMs !== undefined && ms >= h.toMs
            ? inn * (1 - EASE.liquid(seg(ms, h.toMs, 240)))
            : inn;
        }
        const active = activeIndex === i ? 1 : 0;
        const lit = Math.max(hover, active);
        const color = mix(C.muted, C.text, lit);

        return (
          <div
            key={id}
            style={{
              position: "absolute",
              left: cx - OPEN.w / 2 + i * (PILL.islandTool + PILL.islandGap) + 0,
              top: 2,
              width: PILL.islandTool,
              height: PILL.islandTool,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              opacity: t,
              transform: `translateX(${tx}px) scale(${sc * (1 + 0.14 * hover)})`,
              color,
            }}
          >
            {id === "mark" ? (
              <AticMark ms={ms} color={C.text} lookX={lookX} lookY={lookY} lid={lid} />
            ) : (
              <Icon name={id} size={22} strokeWidth={1.6} />
            )}
          </div>
        );
      })}
      </div>
      </div>
      {children}
    </div>
  );
};

/** Mezcla lineal entre dos colores #rrggbb. */
const mix = (a: string, b: string, t: number) => {
  const pa = [1, 3, 5].map((i) => parseInt(a.slice(i, i + 2), 16));
  const pb = [1, 3, 5].map((i) => parseInt(b.slice(i, i + 2), 16));
  const c = pa.map((v, i) => Math.round(v + (pb[i] - v) * t));
  return `rgb(${c[0]} ${c[1]} ${c[2]})`;
};

export const NOTCH_OPEN = OPEN;
export const NOTCH_CLOSED = CLOSED;
