import { AbsoluteFill, interpolate } from "remotion";
import { AticMark, blinkLid } from "../lib/AticMark";
import { useStageBg } from "../lib/format";
import { Caption } from "../lib/Caption";
import { Launcher, LF, type LauncherTimeline } from "../lib/Launcher";
import { Notch } from "../lib/Notch";
import { Screen, SCREEN } from "../lib/Screen";
import { C, EASE, SHADOW_GOO } from "../lib/theme";
import { seg, useMs } from "../lib/time";

const TL: LauncherTimeline = {
  bornMs: 700,
  typeCoMs: 3400,
  arrowMs: 4300,
  typeCalcMs: 5100,
};
const FLY_MS = 650;
const FLY_DUR = 420;

// Posiciones lógicas (toolSlots.ts): la barra nace centrada; la pill queda a su izquierda.
const BAR_LEFT = SCREEN.w / 2 - LF.barW / 2;
const BAR_TOP = 170;
const DISC = 40;
const SLOT_X = BAR_LEFT - 16 - DISC;

export const LauncherScene = () => {
  const ms = useMs();
  const cx = SCREEN.w / 2;

  const zoomK = EASE.smoothOut(seg(ms, FLY_MS, 700));
  const camera = {
    z: interpolate(zoomK, [0, 1], [1, 0.8]),
    fx: interpolate(zoomK, [0, 1], [cx, 305]),
    fy: 0,
    ax: cx,
    ay: 0,
  };

  // La pill se desprende del techo y vuela como disco de 40 px al hueco izquierdo.
  const fly = EASE.liquid(seg(ms, FLY_MS, FLY_DUR));
  const notchGone = seg(ms, FLY_MS, 140);
  const discX = interpolate(fly, [0, 1], [cx - DISC / 2, SLOT_X]);
  const discY = interpolate(fly, [0, 1], [0, BAR_TOP]);

  return (
    <AbsoluteFill style={{ background: useStageBg() }}>
      <Caption title="Apps" sub="Abre lo que sea, sin soltar el teclado" keys={["Ctrl", "Espacio"]} />
      <Screen camera={camera}>
        {notchGone < 1 && (
          <div style={{ opacity: 1 - notchGone }}>
            <Notch
              cx={cx}
              lookX={interpolate(ms, [200, 500], [0, 0.9], {
                extrapolateLeft: "clamp",
                extrapolateRight: "clamp",
              })}
              lid={blinkLid(ms, [520])}
            />
          </div>
        )}

        {ms >= FLY_MS && (
          <div
            style={{
              position: "absolute",
              left: discX,
              top: discY,
              width: DISC,
              height: DISC,
              borderRadius: "50%",
              background: C.skin,
              filter: SHADOW_GOO,
              display: "grid",
              placeItems: "center",
              opacity: seg(ms, FLY_MS, 100),
            }}
          >
            <AticMark size={28} strokeWidth={1.5} ms={ms} lookX={0.6} />
          </div>
        )}

        {ms >= TL.bornMs && (
          <div style={{ position: "absolute", left: BAR_LEFT, top: BAR_TOP }}>
            <Launcher ms={ms} tl={TL} />
          </div>
        )}
      </Screen>
    </AbsoluteFill>
  );
};
