import { AbsoluteFill, Audio, Sequence, staticFile, useCurrentFrame, useVideoConfig } from "remotion";
import { FORMAT_SQUARE, FormatProvider } from "../lib/format";
import { EASE } from "../lib/theme";
import { seg, TimeRemap } from "../lib/time";
import { LAUNCH_BEAT_MS, LAUNCH_SHOTS, type LaunchShot } from "./launchShots";
import { EndCard, MoreGrid, TextSlide, ToolChip } from "./slides";

const Shot = ({ shot }: { shot: LaunchShot }) => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const ms = (frame / fps) * 1000;
  const durMs = shot.beats * LAUNCH_BEAT_MS;

  if (shot.kind === "slide") return <TextSlide es={shot.es} en={shot.en} keys={shot.keys} ms={ms} />;
  if (shot.kind === "more") return <MoreGrid ms={ms} />;
  if (shot.kind === "end") return <EndCard ms={ms} />;

  // Escritorio a pantalla completa: entra con un pequeño golpe de zoom y deriva despacio.
  const Scene = shot.scene;
  const punch = shot.punch ?? 1.045;
  const enter = EASE.smoothOut(seg(ms, 0, 380));
  const drift = 1 + 0.03 * Math.min(1, ms / durMs);
  const scale = (1 + (punch - 1) * (1 - enter)) * drift;
  return (
    <AbsoluteFill>
      <div style={{ position: "absolute", inset: 0, transform: `scale(${scale})`, transformOrigin: "50% 0" }}>
        <TimeRemap segments={shot.segments} durationMs={durMs}>
          <Scene />
        </TimeRemap>
      </div>
      {shot.chip && <ToolChip label={shot.chip.label} keys={shot.chip.keys} ms={ms} />}
    </AbsoluteFill>
  );
};

/** Lanzamiento de Atic, 1:1, texto grande sobre blanco y escritorio a pantalla completa. */
export const LaunchMain = () => (
  <FormatProvider format={FORMAT_SQUARE}>
    <AbsoluteFill style={{ background: "#ffffff" }}>
      {LAUNCH_SHOTS.map((shot, i) => (
        <Sequence
          key={i}
          from={Math.round((shot.beat * LAUNCH_BEAT_MS * 60) / 1000)}
          durationInFrames={Math.round((shot.beats * LAUNCH_BEAT_MS * 60) / 1000)}
          name={`${shot.kind}-${i}`}
        >
          <Shot shot={shot} />
        </Sequence>
      ))}
      <Audio src={staticFile("audio/user-start.wav")} />
    </AbsoluteFill>
  </FormatProvider>
);
