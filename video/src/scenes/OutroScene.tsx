import { AbsoluteFill } from "remotion";
import { AticMark, blinkLid } from "../lib/AticMark";
import { C, EASE, FONT_SANS } from "../lib/theme";
import { seg, useMs } from "../lib/time";

/** Cierre: marca viva + qué es Atic + dónde bajarlo. */
export const OutroScene = () => {
  const ms = useMs();
  const a = EASE.smoothOut(seg(ms, 100, 600));
  const b = EASE.smoothOut(seg(ms, 500, 600));
  const c = EASE.smoothOut(seg(ms, 900, 600));
  const lookX = Math.sin(ms / 700) * 0.9;

  return (
    <AbsoluteFill
      style={{
        background: C.bg,
        alignItems: "center",
        justifyContent: "center",
        fontFamily: FONT_SANS,
        color: C.text,
        textAlign: "center",
        padding: "0 90px",
      }}
    >
      <div
        style={{
          width: 260,
          height: 260,
          borderRadius: 130,
          background: C.skin,
          display: "grid",
          placeItems: "center",
          filter: "drop-shadow(0 10px 22px rgb(0 0 0 / 38%))",
          opacity: a,
          transform: `scale(${0.8 + 0.2 * a})`,
        }}
      >
        <AticMark size={170} strokeWidth={1.5} ms={ms} lookX={lookX} lid={blinkLid(ms, [1400, 3300])} />
      </div>
      <div style={{ marginTop: 60, fontSize: 150, fontWeight: 650, letterSpacing: "-0.035em", lineHeight: 1, opacity: b, transform: `translateY(${(1 - b) * 24}px)` }}>
        Atic
      </div>
      <div style={{ marginTop: 34, fontSize: 46, lineHeight: 1.3, color: C.muted, opacity: b, transform: `translateY(${(1 - b) * 24}px)` }}>
        Caja de herramientas de escritorio,
        <br />
        local-first, en una pill.
      </div>
      <div style={{ marginTop: 70, opacity: c, transform: `translateY(${(1 - c) * 24}px)`, display: "grid", gap: 18, justifyItems: "center" }}>
        <div style={{ fontSize: 40, color: C.text, background: C.elevated, borderRadius: 999, padding: "16px 40px", boxShadow: `inset 0 0 0 1.5px ${C.lineStrong}` }}>
          github.com/chrx3/atic
        </div>
        <div style={{ fontSize: 34, color: C.faint }}>Windows · macOS · Open source (MIT)</div>
      </div>
    </AbsoluteFill>
  );
};
