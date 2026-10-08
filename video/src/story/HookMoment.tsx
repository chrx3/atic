import { AbsoluteFill, interpolate } from "remotion";
import { blinkLid } from "../lib/AticMark";
import { useStageBg } from "../lib/format";
import { Notch } from "../lib/Notch";
import { Screen, SCREEN } from "../lib/Screen";
import { EASE, FONT_SANS } from "../lib/theme";
import { seg, useMs } from "../lib/time";
import { D, Desktop, EditorApp, ERROR_LINES } from "../desk/desk";

/** Gancho: la prueba falla en la terminal, llega un aviso del chat y nace la pill. */
export const HOOK_SEGMENTS: [number, number][] = [[0, 1600]];

export const HookMoment = () => {
  const ms = useMs();
  const cx = SCREEN.w / 2;
  const birth = EASE.liquid(seg(ms, 150, 240));
  const toast = EASE.smoothOut(seg(ms, 700, 380));
  const pulse = 0.5 + 0.5 * Math.sin((ms / 260) * Math.PI);
  const errAt = seg(ms, 200, 260);

  return (
    <AbsoluteFill style={{ background: useStageBg() }}>
      <Screen wallpaper={false}>
        <Desktop>
          <EditorApp
            rect={{ x: 14, y: 58, w: 472, h: 470 }}
            termLines={ERROR_LINES.slice(0, Math.max(1, Math.ceil(errAt * ERROR_LINES.length)))}
            termH={150}
            highlightLine={errAt > 0.6 ? 10 : undefined}
          />
          {/* borde rojo que late sobre la terminal: algo va mal */}
          <div
            style={{
              position: "absolute",
              left: 14,
              top: 58 + 470 - 150,
              width: 472,
              height: 150,
              borderRadius: "0 0 8px 8px",
              boxShadow: `inset 0 0 0 1.5px rgb(255 123 114 / ${0.25 + 0.4 * pulse * errAt})`,
              pointerEvents: "none",
            }}
          />
          <div
            style={{
              position: "absolute",
              left: 232,
              top: 548,
              width: 256,
              padding: "8px 10px",
              borderRadius: 10,
              background: "rgb(255 255 255 / 94%)",
              boxShadow: "0 12px 30px rgb(15 25 55 / 30%)",
              fontFamily: FONT_SANS,
              opacity: toast,
              transform: `translateY(${(1 - toast) * 16}px)`,
            }}
          >
            <div style={{ display: "flex", gap: 7, alignItems: "center" }}>
              <span style={{ width: 18, height: 18, borderRadius: 6, background: D.amber }} />
              <div>
                <div style={{ fontSize: 8.5, fontWeight: 650, color: D.ink }}>Lucía Fuentes · # soporte</div>
                <div style={{ fontSize: 8.2, color: D.inkSoft, lineHeight: 1.3 }}>¿Siguen con problemas? El panel no carga las ventas.</div>
              </div>
            </div>
          </div>
        </Desktop>
        <div
          style={{
            position: "absolute",
            inset: 0,
            opacity: birth,
            transformOrigin: `${cx}px 0`,
            transform: `scale(${0.55 + 0.45 * birth})`,
          }}
        >
          <Notch
            cx={cx}
            lookX={interpolate(ms, [300, 800, 1200], [0, 0.9, -0.3], { extrapolateLeft: "clamp", extrapolateRight: "clamp" })}
            lid={blinkLid(ms, [1000])}
          />
        </div>
      </Screen>
    </AbsoluteFill>
  );
};

