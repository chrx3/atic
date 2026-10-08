import { AbsoluteFill, interpolate } from "remotion";
import { blinkLid } from "../lib/AticMark";
import { useStageBg } from "../lib/format";
import { Caption } from "../lib/Caption";
import {
  CB,
  CB_T,
  CURSOR_CLICKS,
  ClipboardPanel,
  PANEL_PILL_BOTTOM,
  cursorAt,
} from "../lib/Clipboard";
import { FloatFromPill } from "../lib/Float";
import { Notch } from "../lib/Notch";
import { Screen, SCREEN } from "../lib/Screen";
import { C, EASE } from "../lib/theme";
import { seg, useMs } from "../lib/time";

/** Puntero discreto (flecha de 11 px lógicos) con un pequeño hundimiento al hacer clic. */
const Pointer = ({ ms }: { ms: number }) => {
  const { x, y } = cursorAt(ms);
  const pressed = CURSOR_CLICKS.some((t) => ms >= t && ms < t + 130);
  return (
    <svg
      width={11}
      height={16}
      viewBox="0 0 11 16"
      style={{
        position: "absolute",
        left: x,
        top: y,
        opacity: seg(ms, 120, 200),
        transform: `scale(${pressed ? 0.86 : 1})`,
        transformOrigin: "0 0",
        filter: "drop-shadow(0 1px 1.5px rgb(0 0 0 / 45%))",
        pointerEvents: "none",
      }}
    >
      <path
        d="M0.8 0.8V12.2L3.6 9.5L5.5 14.3L7.4 13.5L5.5 8.9L9.4 8.9Z"
        fill="#f4f4f0"
        stroke="#121211"
        strokeWidth={1}
        strokeLinejoin="round"
      />
    </svg>
  );
};

export const ClipboardScene = () => {
  const ms = useMs();
  const cx = SCREEN.w / 2;

  // La tira ocupa 458 de 500 px: se acerca la cámara solo cuando ya se cerró.
  const zoomK = EASE.smoothOut(seg(ms, CB_T.stripCloseMs - 100, 850));
  const camera = {
    z: interpolate(zoomK, [0, 1], [1, 1.2]),
    fx: cx,
    fy: 0,
    ax: cx,
    ay: 0,
  };

  return (
    <AbsoluteFill style={{ background: useStageBg() }}>
      <Caption title="Clipboard" sub="Lo que copias, a mano" keys={["Ctrl", "Shift", "V"]} />
      <Screen camera={camera}>
        <Notch
          cx={cx}
          toggles={[300, CB_T.stripCloseMs]}
          hovers={[{ index: 2, fromMs: CB_T.hoverCellMs, toMs: CB_T.stripCloseMs }]}
          lookX={interpolate(ms, [150, 600], [0, 0.9], {
            extrapolateLeft: "clamp",
            extrapolateRight: "clamp",
          })}
          lid={blinkLid(ms, [200])}
        />

        <FloatFromPill
          ms={ms}
          startMs={CB_T.openMs}
          cx={cx}
          pillBottom={PANEL_PILL_BOTTOM}
          w={CB.w}
          h={CB.h}
          radius={CB.radius}
        >
          {() => <ClipboardPanel ms={ms} cx={cx} />}
        </FloatFromPill>

        <Pointer ms={ms} />
      </Screen>
    </AbsoluteFill>
  );
};
