import { AbsoluteFill, interpolate } from "remotion";
import { blinkLid } from "../lib/AticMark";
import { useStageBg } from "../lib/format";
import { Caption } from "../lib/Caption";
import { Notch, STRIP, type Hover } from "../lib/Notch";
import { Screen, SCREEN } from "../lib/Screen";
import { C, EASE, FONT_SANS } from "../lib/theme";
import { seg, useMs } from "../lib/time";

const LABELS: Record<string, { label: string; keys?: string }> = {
  meetings: { label: "Reuniones", keys: "Ctrl+Shift+R" },
  clipboard: { label: "Clipboard", keys: "Ctrl+Shift+V" },
  snippets: { label: "Textos", keys: "Ctrl+Shift+S" },
  agents: { label: "Agentes", keys: "Ctrl+Shift+A" },
  system: { label: "Sistema" },
  captures: { label: "Capturas", keys: "Ctrl+Shift+4" },
  board: { label: "Pizarra", keys: "Ctrl+Shift+X" },
  color: { label: "Color", keys: "Ctrl+Shift+C" },
  window: { label: "Voltear ventana", keys: "Ctrl+Shift+B" },
};

const OPEN_AT = 900;
const SWEEP_FROM = 1400;
const STEP = 200;
// Recorre las celdas 1..9 (todas menos la marca).
const hovers: Hover[] = STRIP.slice(1).map((_, k) => ({
  index: k + 1,
  fromMs: SWEEP_FROM + k * STEP,
  toMs: SWEEP_FROM + (k + 1) * STEP,
}));

export const NotchIntro = () => {
  const ms = useMs();
  const cx = SCREEN.w / 2;

  // Nacimiento (.is-boot): opacidad 0 y escala .55 → 1 en 240 ms.
  const birth = EASE.liquid(seg(ms, 150, 240));

  // La marca mira alrededor antes de que aparezca la tira.
  const lookX = interpolate(ms, [300, 600, 900], [0, -1.0, 0.9], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });
  const lid = blinkLid(ms, [700, 2600]);

  const current = hovers.find((h) => ms >= h.fromMs && ms < (h.toMs ?? 0));
  const info = current ? LABELS[STRIP[current.index]] : undefined;

  return (
    <AbsoluteFill style={{ background: useStageBg() }}>
      <Caption title="Atic" sub="Tu caja de herramientas vive en una pill" />
      <Screen>
        <div
          style={{
            position: "absolute",
            inset: 0,
            opacity: birth,
            transformOrigin: `${cx}px 0`,
            transform: `scale(${0.55 + 0.45 * birth})`,
          }}
        >
          <Notch cx={cx} toggles={[OPEN_AT]} hovers={hovers} lookX={lookX} lid={lid} />
        </div>
        {info && (
          <div
            style={{
              position: "absolute",
              left: cx,
              top: 64,
              transform: "translateX(-50%)",
              display: "flex",
              gap: 8,
              alignItems: "center",
              fontFamily: FONT_SANS,
              fontSize: 13,
              fontWeight: 600,
              color: C.text,
              background: C.skin,
              padding: "6px 12px",
              borderRadius: 999,
              boxShadow: "0 8px 22px rgb(0 0 0 / 32%)",
              whiteSpace: "nowrap",
            }}
          >
            {info.label}
            {info.keys && <span style={{ color: C.muted, fontWeight: 500 }}>{info.keys}</span>}
          </div>
        )}
      </Screen>
    </AbsoluteFill>
  );
};
