import { blinkLid, AticMark } from "../lib/AticMark";
import { useFormat } from "../lib/format";
import { C, EASE, FONT_SANS, SHADOW_GOO } from "../lib/theme";
import { seg } from "../lib/time";

/** Cierre: marca, repo y web, con la bajada en español e inglés. */
export const Cta = ({ ms }: { ms: number }) => {
  const format = useFormat();
  const h = format.id === "h";
  const a = EASE.smoothOut(seg(ms, 0, 360));
  const b = EASE.smoothOut(seg(ms, 180, 360));
  const c = EASE.smoothOut(seg(ms, 380, 360));
  const disc = h ? 250 : 300;

  return (
    <div
      style={{
        position: "absolute",
        inset: 0,
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        fontFamily: FONT_SANS,
        color: C.text,
        textAlign: "center",
        padding: "0 80px",
        gap: h ? 22 : 30,
      }}
    >
      <div
        style={{
          width: disc,
          height: disc,
          borderRadius: disc / 2,
          background: C.skin,
          filter: SHADOW_GOO,
          display: "grid",
          placeItems: "center",
          opacity: a,
          transform: `scale(${0.7 + 0.3 * a})`,
        }}
      >
        <AticMark size={disc * 0.66} strokeWidth={1.5} ms={ms} lookX={Math.sin(ms / 600) * 0.9} lid={blinkLid(ms, [1100, 2600])} />
      </div>
      <div
        style={{
          fontSize: h ? 130 : 150,
          fontWeight: 750,
          letterSpacing: "-0.035em",
          lineHeight: 1,
          opacity: b,
          transform: `translateY(${(1 - b) * 26}px)`,
        }}
      >
        Atic
      </div>
      <div
        style={{
          fontSize: h ? 40 : 42,
          lineHeight: 1.3,
          color: C.muted,
          opacity: b,
          transform: `translateY(${(1 - b) * 26}px)`,
        }}
      >
        Caja de herramientas de escritorio
        <br />
        Desktop toolbox · local-first
      </div>
      <div
        style={{
          display: "grid",
          gap: 14,
          justifyItems: "center",
          opacity: c,
          transform: `translateY(${(1 - c) * 26}px)`,
        }}
      >
        <div
          style={{
            fontSize: h ? 46 : 48,
            fontWeight: 600,
            background: C.elevated,
            borderRadius: 999,
            padding: "16px 42px",
            boxShadow: `inset 0 0 0 1.5px ${C.lineStrong}`,
          }}
        >
          github.com/chrx3/atic
        </div>
        <div style={{ fontSize: h ? 42 : 44, color: C.text }}>chrsx3.com</div>
        <div style={{ fontSize: h ? 30 : 32, color: C.faint }}>Windows · macOS · Open source (MIT)</div>
      </div>
    </div>
  );
};
