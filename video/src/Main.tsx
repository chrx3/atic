import type { ComponentType } from "react";
import { AbsoluteFill, Sequence, interpolate, useCurrentFrame } from "remotion";
import { C } from "./lib/theme";
import { AgentesScene } from "./scenes/AgentesScene";
import { ClipboardScene } from "./scenes/ClipboardScene";
import { ColorScene } from "./scenes/ColorScene";
import { FlipboardScene } from "./scenes/FlipboardScene";
import { LauncherScene } from "./scenes/LauncherScene";
import { NotchIntro } from "./scenes/NotchIntro";
import { OutroScene } from "./scenes/OutroScene";
import { PizarraScene } from "./scenes/PizarraScene";
import { TextosScene } from "./scenes/TextosScene";

export const FPS = 60;
const secs = (s: number) => Math.round(s * FPS);

/** Orden y duración (s) de cada escena en el video completo. */
export const SCENES: { id: string; component: ComponentType; seconds: number }[] = [
  { id: "NotchIntro", component: NotchIntro, seconds: 3.8 },
  { id: "Launcher", component: LauncherScene, seconds: 6.5 },
  { id: "Clipboard", component: ClipboardScene, seconds: 6 },
  { id: "Textos", component: TextosScene, seconds: 6 },
  { id: "Flipboard", component: FlipboardScene, seconds: 6 },
  { id: "Pizarra", component: PizarraScene, seconds: 6 },
  { id: "Color", component: ColorScene, seconds: 5 },
  { id: "Agentes", component: AgentesScene, seconds: 7 },
  { id: "Outro", component: OutroScene, seconds: 3.5 },
];

export const MAIN_FRAMES = SCENES.reduce((n, s) => n + secs(s.seconds), 0);

/** Corte con un respiro corto por el fondo oscuro (6 cuadros a cada lado). */
const Cut = ({ frames, children }: { frames: number; children: React.ReactNode }) => {
  const frame = useCurrentFrame();
  const edge = 6;
  const opacity = interpolate(frame, [0, edge, frames - edge, frames], [0, 1, 1, 0], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });
  return <AbsoluteFill style={{ opacity }}>{children}</AbsoluteFill>;
};

export const Main = () => {
  let from = 0;
  return (
    <AbsoluteFill style={{ background: C.bg }}>
      {SCENES.map(({ id, component: Scene, seconds }) => {
        const frames = secs(seconds);
        const start = from;
        from += frames;
        return (
          <Sequence key={id} from={start} durationInFrames={frames} name={id}>
            <Cut frames={frames}>
              <Scene />
            </Cut>
          </Sequence>
        );
      })}
    </AbsoluteFill>
  );
};
