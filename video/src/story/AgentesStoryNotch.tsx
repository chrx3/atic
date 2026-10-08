import { AgentLogo } from "../lib/agentesIcons";
import { AticMark } from "../lib/AticMark";
import { C, EASE, FONT_SANS, ISLAND, SHADOW_GOO, clamp01, lerp } from "../lib/theme";
import { seg } from "../lib/time";

export type CueState = "working" | "waiting" | "ready";
export type Cue = { at: number; state: CueState };

type Props = {
  cx: number;
  ms: number;
  cues: Cue[];
  /** Cara de permiso (280×148): abre y cierra. */
  faceOpenAt: number;
  faceCloseAt: number;
  /** El cursor entra a APROBAR y hace clic. */
  approveHoverAt: number;
  approvePressAt: number;
  lookX?: number;
  lookY?: number;
  lid?: number;
};

const TAB = { w: 124, h: 44 };
// Pestaña con aviso: 32 + 2 + 26 + 96 + 12 de ancho (islandCueLong) y 42 + 4 de alto.
const CUE = { w: 168, h: 46, btn: 26, msg: 96 };
const FACE = { w: 280, h: 148 };

/** Valor que salta a cada `frames[i].v` en `frames[i].at` y anima 240 ms desde el valor previo. */
const animTo = (
  ms: number,
  initial: number,
  frames: { at: number; v: number }[],
  dur: number,
  ease: (t: number) => number,
) => {
  let value = initial;
  for (const f of frames) {
    if (ms < f.at) break;
    const raw = seg(ms, f.at, dur);
    if (raw < 1) return value + (f.v - value) * ease(raw);
    value = f.v;
  }
  return value;
};

const CUE_TEXT: Record<CueState, string> = {
  working: "Trabajando…",
  waiting: "permiso",
  ready: "Listo",
};

/**
 * Pestaña cerrada de la pill con el chip de agente (working / waiting / ready) y la
 * cara de permiso (RECHAZAR / APROBAR). Mismo path que Notch.tsx.
 */
export const AgentesStoryNotch = ({
  cx,
  ms,
  cues,
  faceOpenAt,
  faceCloseAt,
  approveHoverAt,
  approvePressAt,
  lookX = 0,
  lookY = 0,
  lid = 1,
}: Props) => {
  const cue = [...cues].reverse().find((c) => ms >= c.at);
  const firstCueAt = cues[0]?.at ?? Infinity;

  const W = animTo(
    ms,
    TAB.w,
    [
      { at: firstCueAt, v: CUE.w },
      { at: faceOpenAt, v: FACE.w },
      { at: faceCloseAt, v: CUE.w },
    ],
    ISLAND.openMs,
    EASE.island,
  );
  const H = animTo(
    ms,
    TAB.h,
    [
      { at: firstCueAt, v: CUE.h },
      { at: faceOpenAt, v: FACE.h },
      { at: faceCloseAt, v: CUE.h },
    ],
    ISLAND.openMs,
    EASE.island,
  );
  const cueT = animTo(ms, 0, [{ at: firstCueAt, v: 1 }], ISLAND.openMs, EASE.liquid);
  const faceT = animTo(
    ms,
    0,
    [
      { at: faceOpenAt, v: 1 },
      { at: faceCloseAt, v: 0 },
    ],
    ISLAND.openMs,
    EASE.liquid,
  );

  const r = H <= 48 ? H / 2 : lerp(24, 22, clamp01((H - 48) / 20));
  const l = cx - W / 2;
  const rt = cx + W / 2;
  const body = `M${l} 0V${H - r}A${r} ${r} 0 0 0 ${l + r} ${H}H${rt - r}A${r} ${r} 0 0 0 ${rt} ${H - r}V0Z`;
  const sl = l - 12;
  const sr = rt + 12;
  const shoulder = `M${sl + 4} 0A20 20 0 0 0 ${sl + 20} 8H${sr - 20}A20 20 0 0 0 ${sr - 4} 0Z`;

  const working = cue?.state === "working";
  const breathe = working ? 1.04 - 0.04 * Math.cos((ms / 2400) * Math.PI * 2) : 1;

  // Marca: centrada en la pestaña sola; a la izquierda del chip cuando hay aviso.
  const bandH = lerp(40, 42, cueT);
  const groupW = 32 + 2 + CUE.btn + CUE.msg;
  const markCueX = cx - groupW / 2 + 16;
  const markX = lerp(cx, markCueX, cueT * (1 - faceT));
  const markY = lerp(20, bandH / 2, cueT * (1 - faceT));
  const chipLeft = cx - groupW / 2 + 34;

  // Chip: se apaga rápido cuando la cara gana y vuelve al cerrarla.
  const gate =
    ms < faceOpenAt
      ? 1
      : ms < faceCloseAt
        ? 1 - EASE.liquid(seg(ms, faceOpenAt, 90))
        : EASE.liquid(seg(ms, faceCloseAt + 120, 140));
  const chipOpacity = cueT * gate;

  const readyIn = cue?.state === "ready" ? EASE.smoothOut(seg(ms, cue.at, 250)) : 1;
  const chipStyle = {
    working: { bg: "transparent", fg: C.muted },
    waiting: { bg: "rgba(232,90,82,.16)", fg: C.rec },
    ready: { bg: "rgba(111,175,136,.14)", fg: C.ok },
  }[cue?.state ?? "working"];
  const logoOpacity = working ? 0.55 + 0.45 * (0.5 - 0.5 * Math.cos((ms / 1800) * Math.PI * 2)) : 1;

  const faceFade = clamp01(seg(ms, faceOpenAt + 90, 90)) * (1 - clamp01(seg(ms, faceCloseAt - 10, 80)));
  const hoverA = EASE.smoothOut(seg(ms, approveHoverAt, 120));
  const pressed = ms >= approvePressAt && ms < approvePressAt + 110;

  const pad = 60;
  return (
    <div style={{ position: "absolute", left: 0, top: 0, width: 0, height: 0, fontFamily: FONT_SANS }}>
      <svg
        width={cx * 2 + pad * 2}
        height={H + pad * 2}
        viewBox={`${-pad} ${-pad} ${cx * 2 + pad * 2} ${H + pad * 2}`}
        style={{
          position: "absolute",
          left: -pad,
          top: -pad,
          overflow: "visible",
          filter: `${SHADOW_GOO}${breathe !== 1 ? ` brightness(${breathe})` : ""}`,
        }}
      >
        <g fill={C.skin} stroke={C.skin} strokeWidth={1.25} strokeLinejoin="round">
          <path d={body} />
          <path d={shoulder} />
        </g>
      </svg>

      <div style={{ position: "absolute", left: markX - 16, top: markY - 16 }}>
        <AticMark ms={ms} lookX={lookX} lookY={lookY} lid={lid} />
      </div>

      {/* chip de agente en la pestaña */}
      {cue && chipOpacity > 0.01 && (
        <div
          style={{
            position: "absolute",
            left: chipLeft,
            top: bandH / 2 - CUE.btn / 2,
            width: CUE.btn + CUE.msg,
            height: CUE.btn,
            boxSizing: "border-box",
            padding: "0 4px",
            borderRadius: 999,
            display: "flex",
            alignItems: "center",
            gap: 4,
            background: chipStyle.bg,
            color: chipStyle.fg,
            opacity: chipOpacity * readyIn,
            transform: cue.state === "ready" ? `translateY(${(1 - readyIn) * 4}px)` : undefined,
            filter: cue.state === "ready" && readyIn < 1 ? `blur(${(1 - readyIn) * 2}px)` : undefined,
          }}
        >
          <AgentLogo
            agent="claude"
            size={18}
            style={{ opacity: logoOpacity }}
          />
          <span
            style={{
              width: CUE.msg - 4,
              textAlign: "center",
              fontSize: 10,
              fontWeight: 650,
              letterSpacing: "0.02em",
              whiteSpace: "nowrap",
            }}
          >
            {CUE_TEXT[cue.state]}
          </span>
        </div>
      )}

      {/* cara de permiso (PS:5650-5697) */}
      {faceFade > 0.01 && (
        <div
          style={{
            position: "absolute",
            left: cx - FACE.w / 2,
            top: 40,
            width: FACE.w,
            height: 104,
            boxSizing: "border-box",
            padding: "0 12px",
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            justifyContent: "center",
            gap: 6,
            opacity: faceFade,
          }}
        >
          <div style={{ display: "flex", alignItems: "center", gap: 6, height: 16 }}>
            <AgentLogo agent="claude" size={16} />
            <span style={{ fontSize: 12, fontWeight: 600, lineHeight: 1.2, color: C.text }}>
              El agente espera tu permiso
            </span>
          </div>
          <div
            style={{
              width: "100%",
              textAlign: "center",
              fontSize: 12,
              lineHeight: 1.2,
              color: C.muted,
              whiteSpace: "nowrap",
              overflow: "hidden",
              textOverflow: "ellipsis",
            }}
          >
            <strong style={{ fontWeight: 600, color: C.text }}>Bash</strong> · Corre las pruebas (npm test)
          </div>
          <div style={{ display: "flex", gap: 8, width: "100%" }}>
            <div
              style={{
                flex: 1,
                height: 32,
                borderRadius: 999,
                display: "grid",
                placeItems: "center",
                background: "rgba(232,90,82,.18)",
                boxShadow: "inset 0 0 0 1px rgba(232,90,82,.42)",
                color: C.rec,
                fontSize: 11,
                fontWeight: 600,
                letterSpacing: "0.06em",
              }}
            >
              RECHAZAR
            </div>
            <div
              style={{
                flex: 1,
                height: 32,
                borderRadius: 999,
                display: "grid",
                placeItems: "center",
                background: `rgba(111,175,136,${0.18 + 0.1 * hoverA})`,
                color: C.ok,
                fontSize: 11,
                fontWeight: 600,
                letterSpacing: "0.06em",
                transform: pressed ? "scale(0.96)" : undefined,
              }}
            >
              APROBAR
            </div>
          </div>
        </div>
      )}
    </div>
  );
};

/** Centro de APROBAR en coordenadas lógicas (cara de 280×148 centrada en cx). */
export const approveCenter = (cx: number) => ({ x: cx + 66, y: 113 });
