import { AbsoluteFill, interpolate } from "remotion";
import { blinkLid } from "../lib/AticMark";
import { useStageBg } from "../lib/format";
import { Notch, STRIP, type Hover } from "../lib/Notch";
import { Screen, SCREEN } from "../lib/Screen";
import { C, FONT_SANS } from "../lib/theme";
import { useMs } from "../lib/time";
import { BrowserApp, Desktop, EditorApp, ERROR_LINES } from "../desk/desk";

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

const OPEN_AT = 100;
const SWEEP_FROM = 600;
const STEP = 200;
const hovers: Hover[] = STRIP.slice(1).map((_, k) => ({
  index: k + 1,
  fromMs: SWEEP_FROM + k * STEP,
  toMs: SWEEP_FROM + (k + 1) * STEP,
}));

/** Segundo plano: la tira se abre sobre el escritorio y recorre las herramientas. */
export const STRIP_SEGMENTS: [number, number][] = [[0, 2500]];

export const StripMoment = () => {
  const ms = useMs();
  const cx = SCREEN.w / 2;
  const current = hovers.find((h) => ms >= h.fromMs && ms < (h.toMs ?? 0));
  const info = current ? LABELS[STRIP[current.index]] : undefined;
  return (
    <AbsoluteFill style={{ background: useStageBg() }}>
      <Screen wallpaper={false}>
        <Desktop>
          <EditorApp rect={{ x: 14, y: 58, w: 472, h: 300 }} termLines={ERROR_LINES} termH={110} highlightLine={10} />
          <BrowserApp rect={{ x: 60, y: 372, w: 426, h: 232 }} />
        </Desktop>
        <Notch
          cx={cx}
          toggles={[OPEN_AT]}
          hovers={hovers}
          lookX={interpolate(ms, [0, 500], [-0.3, 0.6], { extrapolateLeft: "clamp", extrapolateRight: "clamp" })}
          lid={blinkLid(ms, [400])}
        />
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
