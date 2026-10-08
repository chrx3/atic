import { AbsoluteFill, interpolate } from "remotion";
import { blinkLid } from "../lib/AticMark";
import { Notch } from "../lib/Notch";
import { Screen, SCREEN } from "../lib/Screen";
import { useMs } from "../lib/time";

/** Remate: el notch queda solo, abre la tira, la cierra y la marca mira a cámara. */
export const PayoffScene = () => {
  const ms = useMs();
  const cx = SCREEN.w / 2;
  return (
    <AbsoluteFill>
      <Screen>
        <Notch
          cx={cx}
          toggles={[350, 2500]}
          lookX={interpolate(ms, [0, 700, 2900, 3600], [0.9, -0.5, -0.5, 0], {
            extrapolateLeft: "clamp",
            extrapolateRight: "clamp",
          })}
          lid={blinkLid(ms, [1500, 3900])}
        />
      </Screen>
    </AbsoluteFill>
  );
};
