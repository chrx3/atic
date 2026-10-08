import { Composition } from "remotion";
import { FPS, MAIN_FRAMES, Main, SCENES } from "./Main";
import { FORMAT_SHORT_H, FORMAT_SHORT_V } from "./lib/format";
import { DeskTest } from "./desk/DeskTest";
import { LaunchMain } from "./launch/LaunchMain";
import { LAUNCH_FRAMES } from "./launch/launchShots";
import { SQUARE_MOMENTS } from "./launch/squareMoments";
import { STORY_MOMENTS } from "./story/registry";
import { STORY_FRAMES, StoryShortH, StoryShortV } from "./story/StoryShort";
import { Short } from "./short/ShortMain";
import { TOTAL_FRAMES } from "./short/shots";
import { GPUI_PILL_FRAMES, GpuiPillVideo } from "./gpui/GpuiPillMain";

const size = { width: 1080, height: 1920, fps: FPS } as const;

export const Root = () => (
  <>
    <Composition id="Story-9x16" component={StoryShortV} durationInFrames={STORY_FRAMES} width={1080} height={1920} fps={FPS} />
    <Composition id="Story-16x9" component={StoryShortH} durationInFrames={STORY_FRAMES} width={1920} height={1080} fps={FPS} />
    <Composition
      id="Short-9x16"
      component={Short}
      durationInFrames={TOTAL_FRAMES}
      width={FORMAT_SHORT_V.width}
      height={FORMAT_SHORT_V.height}
      fps={FPS}
      defaultProps={{ format: FORMAT_SHORT_V }}
    />
    <Composition
      id="Short-16x9"
      component={Short}
      durationInFrames={TOTAL_FRAMES}
      width={FORMAT_SHORT_H.width}
      height={FORMAT_SHORT_H.height}
      fps={FPS}
      defaultProps={{ format: FORMAT_SHORT_H }}
    />
    {STORY_MOMENTS.map(({ id, component, seconds }) => (
      <Composition key={id} id={id} component={component} durationInFrames={Math.round(seconds * FPS)} {...size} />
    ))}
    <Composition id="Launch-1x1" component={LaunchMain} durationInFrames={LAUNCH_FRAMES} width={1080} height={1080} fps={FPS} />
    {SQUARE_MOMENTS.map(({ id, component, seconds }) => (
      <Composition key={id} id={id} component={component} durationInFrames={Math.round(seconds * FPS)} width={1080} height={1080} fps={FPS} />
    ))}
    <Composition id="Gpui-Pill-16x9" component={GpuiPillVideo} durationInFrames={GPUI_PILL_FRAMES} width={1920} height={1080} fps={FPS} />
    <Composition id="DeskTest" component={DeskTest} durationInFrames={60} {...size} />
    <Composition id="Main" component={Main} durationInFrames={MAIN_FRAMES} {...size} />
    {SCENES.map(({ id, component, seconds }) => (
      <Composition
        key={id}
        id={id}
        component={component}
        durationInFrames={Math.round(seconds * FPS)}
        {...size}
      />
    ))}
  </>
);
