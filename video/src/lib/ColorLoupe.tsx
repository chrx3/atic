import { Easing } from "remotion";
import { C, FONT_MONO, SHADOW_GOO, lerp } from "./theme";
import { seg } from "./time";

/** Geometría de la lupa (ficha B.2, píxeles CSS). */
export const LOUPE = { padding: 24, body: { w: 264, h: 103 }, window: { w: 312, h: 160 }, grid: 13, offset: 28 } as const;

const easeOut = Easing.bezier(0, 0, 0.58, 1);
const smoothOut = Easing.bezier(0.22, 1, 0.36, 1);
const exit = Easing.bezier(0.4, 0, 1, 1);

const parse = (hex: string): [number, number, number] => [
  parseInt(hex.slice(1, 3), 16),
  parseInt(hex.slice(3, 5), 16),
  parseInt(hex.slice(5, 7), 16),
];
const luminance = (hex: string) => {
  const [r, g, b] = parse(hex).map((v) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};
/** `inkOn` (colorMath.ts): #111 si contrasta más que #fff, si no #fff. */
export const inkOn = (hex: string) => {
  const l = luminance(hex);
  const vsDark = (l + 0.05) / (luminance("#111111") + 0.05);
  const vsLight = 1.05 / (l + 0.05);
  return vsDark > vsLight ? "#111111" : "#ffffff";
};

const BODY = { x: 24, y: 24, w: 264, h: 103, r: 24 };
/** Lóbulo: el canvas de la lupa engordado 12 px por lado. */
const LOBE = { x: 16, y: 22, w: 88, h: 88, r: 22 };
/** El filtro engorda ~1.7 px por lado (GOO_GROW); se compensa encogiendo las formas. */
const GROW = 1.7;
const shrink = (s: typeof BODY) => ({ x: s.x + GROW, y: s.y + GROW, w: s.w - 2 * GROW, h: s.h - 2 * GROW, r: s.r - GROW });

export type LoupeProps = {
  /** Esquina superior izquierda del cuerpo de la gota, en coordenadas de pantalla. */
  x: number;
  y: number;
  ms: number;
  /** Instante en que nace la lupa (fase `in`). */
  inAt: number;
  /** Instante del primer parche leído; null mientras no llega. */
  readyAt: number | null;
  /** Instante del clic que copia; null si aún no se copia. */
  copyAt: number | null;
  /** 169 colores (13x13) del parche actual. */
  cells: string[] | null;
};

const Help = () => (
  <div style={{ flex: 1, fontSize: 10, lineHeight: 1.4, color: C.muted }}>Clic o Enter: copiar · R: editar</div>
);

/** Lupa de color: gota líquida con el parche ampliado, el valor y la ayuda (ColorLoupeSurface). */
export const ColorLoupe = ({ x, y, ms, inAt, readyAt, copyAt, cells }: LoupeProps) => {
  if (ms < inAt) return null;
  const tIn = ms - inAt;
  const copying = copyAt !== null && ms >= copyAt;
  const out = copying ? seg(ms, copyAt, 200) : 0;
  if (copying && out >= 1) return null;

  let stageOpacity = easeOut(seg(tIn, 0, 220));
  let bodyScale = lerp(0.84, 1, smoothOut(seg(tIn, 0, 260)));
  let flash = 1;
  if (copying) {
    stageOpacity = 1 - exit(out);
    bodyScale =
      out < 0.35 ? lerp(1, 1.05, exit(out / 0.35)) : lerp(1.05, 0.94, exit((out - 0.35) / 0.65));
    flash = out < 0.35 ? 1 + 0.3 * exit(out / 0.35) : 1.3 - 0.3 * exit((out - 0.35) / 0.65);
  }

  const ready = readyAt !== null && ms >= readyAt && cells !== null;
  const center = cells ? cells[84] : null;
  const hex = center ? center.toUpperCase() : null;
  const ink = hex ? inkOn(hex) : "#111111";
  const open = ready ? seg(ms, readyAt, 320) : 0;
  const gridOpacity = ready ? lerp(0.35, 1, smoothOut(open)) : 0.45;
  const gridScale = ready ? lerp(0.86, 1, smoothOut(open)) : 1;
  // La gota respira mientras lee: brillo 1.00-1.08, ciclo de 2.4 s (Skin.svelte).
  const breathe = ready && !copying ? 1.04 - 0.04 * Math.cos((2 * Math.PI * (ms - readyAt)) / 2400) : 1;

  const body = shrink(BODY);
  const lobe = shrink(LOBE);

  return (
    <div
      style={{
        position: "absolute",
        left: x - LOUPE.padding,
        top: y - LOUPE.padding,
        width: LOUPE.window.w,
        height: LOUPE.window.h,
        opacity: stageOpacity,
        filter: flash !== 1 ? `brightness(${flash})` : undefined,
        fontFamily: '"Segoe UI", sans-serif',
        fontSize: 12,
        lineHeight: 1.4,
        color: C.text,
      }}
    >
      <div
        style={{
          position: "absolute",
          inset: 0,
          transformOrigin: `${BODY.x + 32}px ${BODY.y + 32}px`,
          transform: `scale(${bodyScale})`,
        }}
      >
        {/* piel: unión suavizada del cuerpo y el lóbulo de la lupa */}
        <svg
          width={LOUPE.window.w}
          height={LOUPE.window.h}
          viewBox={`0 0 ${LOUPE.window.w} ${LOUPE.window.h}`}
          style={{ position: "absolute", left: 0, top: 0, overflow: "visible", filter: `${SHADOW_GOO} brightness(${breathe})` }}
        >
          <defs>
            <filter id="loupe-goo" filterUnits="userSpaceOnUse" x={-20} y={-20} width={352} height={200} colorInterpolationFilters="sRGB">
              <feGaussianBlur stdDeviation={6} />
              <feColorMatrix type="matrix" values="1 0 0 0 0  0 1 0 0 0  0 0 1 0 0  0 0 0 18 -7" />
            </filter>
          </defs>
          <g filter="url(#loupe-goo)" fill={C.skin}>
            <rect x={body.x} y={body.y} width={body.w} height={body.h} rx={body.r} />
            <rect x={lobe.x} y={lobe.y} width={lobe.w} height={lobe.h} rx={lobe.r} />
          </g>
        </svg>

        {/* parche ampliado 13x13 -> 64x64, pixelado */}
        <div
          style={{
            position: "absolute",
            left: 28,
            top: 34,
            width: 64,
            height: 64,
            borderRadius: 15,
            overflow: "hidden",
            outline: "1px solid rgb(240 240 234 / 14%)",
            opacity: gridOpacity,
            transform: `scale(${gridScale})`,
          }}
        >
          <svg width={64} height={64} viewBox="0 0 130 130" shapeRendering="crispEdges" style={{ display: "block" }}>
            {cells
              ? cells.map((c, i) => (
                  <rect key={i} x={(i % LOUPE.grid) * 10} y={Math.floor(i / LOUPE.grid) * 10} width={10} height={10} fill={c} />
                ))
              : null}
            {cells && <rect x={61} y={61} width={8} height={8} fill="none" stroke={ink} strokeWidth={2} />}
          </svg>
        </div>

        {/* valor + ayuda */}
        <div style={{ position: "absolute", left: 104, top: 35.5, width: 170 }}>
          {ready && hex ? (
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 8,
                width: "fit-content",
                maxWidth: "100%",
                padding: "5px 8px",
                borderRadius: 6,
                background: hex,
                color: ink,
                font: `600 12px/1.4 ${FONT_MONO}`,
              }}
            >
              <span style={{ width: 12, height: 12, borderRadius: 3, background: hex, outline: "1px solid currentColor", flex: "none" }} />
              {hex}
            </div>
          ) : (
            <div style={{ padding: "5px 0", color: C.muted }}>Leyendo color…</div>
          )}
          <div style={{ display: "flex", alignItems: "center", gap: 8, marginTop: 6 }}>
            <Help />
            <div
              style={{
                fontSize: 11,
                padding: "3px 6px",
                borderRadius: 5,
                background: "rgb(240 240 234 / 8%)",
                color: C.text,
              }}
            >
              Editar
            </div>
          </div>
        </div>

        <div
          style={{
            position: "absolute",
            right: LOUPE.window.w - 274,
            top: 103,
            fontSize: 10,
            lineHeight: 1.4,
            color: C.muted,
          }}
        >
          Cancelar · Esc
        </div>
      </div>
    </div>
  );
};
