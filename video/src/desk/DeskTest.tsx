import { AbsoluteFill } from "remotion";
import { Screen } from "../lib/Screen";
import { useStageBg } from "../lib/format";
import { BrowserApp, ChatApp, DesignApp, Desktop, EditorApp, ERROR_LINES } from "./desk";

/** Solo para revisar las ventanas del escritorio simulado. */
export const DeskTest = () => (
  <AbsoluteFill style={{ background: useStageBg() }}>
    <Screen wallpaper={false}>
      <Desktop>
        <EditorApp rect={{ x: 14, y: 58, w: 330, h: 300 }} termLines={ERROR_LINES} termH={92} highlightLine={10} />
        <BrowserApp rect={{ x: 150, y: 200, w: 340, h: 300 }} />
        <ChatApp rect={{ x: 14, y: 372, w: 300, h: 230 }} composer="Hola, ya lo estamos revisando" showCaret />
        <DesignApp rect={{ x: 180, y: 330, w: 310, h: 270 }} />
      </Desktop>
    </Screen>
  </AbsoluteFill>
);
