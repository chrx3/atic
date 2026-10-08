import { AbsoluteFill, interpolate } from "remotion";
import { blinkLid } from "../lib/AticMark";
import { useStageBg } from "../lib/format";
import { Notch } from "../lib/Notch";
import { Screen, SCREEN } from "../lib/Screen";
import { useMs } from "../lib/time";
import { ChatApp, Desktop, EditorApp, type TermLine } from "../desk/desk";

const PASS_LINES: TermLine[] = [
  { t: "$ npm test", c: "cmd" },
  { t: " PASS  src/orders.test.ts", c: "ok" },
  { t: "  ✓ getOrders › devuelve los pedidos", c: "ok" },
  { t: "Tests:  12 passed, 12 total", c: "ok" },
];

/** Remate: el bug quedó resuelto y el escritorio sigue tal cual estaba; la pill vuelve a su sitio. */
export const PayoffDeskMoment = () => {
  const ms = useMs();
  const cx = SCREEN.w / 2;
  return (
    <AbsoluteFill style={{ background: useStageBg() }}>
      <Screen wallpaper={false}>
        <Desktop time="14:41">
          <EditorApp rect={{ x: 14, y: 58, w: 472, h: 300 }} termLines={PASS_LINES} termH={110} />
          <ChatApp
            rect={{ x: 60, y: 372, w: 426, h: 232 }}
            extraMessage="Ya lo detectamos: es un problema en la base de datos y lo estamos corrigiendo. Te aviso en 10 minutos."
          />
        </Desktop>
        <Notch
          cx={cx}
          toggles={[350, 2500]}
          lookX={interpolate(ms, [0, 700, 2900, 3600], [0.9, -0.5, -0.5, 0], { extrapolateLeft: "clamp", extrapolateRight: "clamp" })}
          lid={blinkLid(ms, [1500, 3900])}
        />
      </Screen>
    </AbsoluteFill>
  );
};
