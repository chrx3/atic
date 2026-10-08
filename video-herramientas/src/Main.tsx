import React from "react";
import { AbsoluteFill, Audio, interpolate, Sequence, staticFile, useVideoConfig } from "remotion";
import { beatToFrame, SEGMENTS, type Segment } from "./timeline";
import { Brand, End, Hook, Montage, PillScene, ToolScene } from "./scenes";

const render = (s: Segment) => {
  switch (s.kind) {
    case "hook":
      return <Hook />;
    case "brand":
      return <Brand />;
    case "pill":
      return <PillScene shot={s.shot} beats={s.beats} />;
    case "tool":
      return <ToolScene shot={s.shot} beats={s.beats} />;
    case "montage":
      return <Montage beats={s.beats} />;
    case "end":
      return <End beats={s.beats} />;
  }
};

export const Main: React.FC = () => {
  const { durationInFrames } = useVideoConfig();
  return (
    <AbsoluteFill style={{ background: "#fff" }}>
      {SEGMENTS.map((s, i) => {
        const from = i === 0 ? 0 : beatToFrame(s.beat);
        const to = beatToFrame(s.beat + s.beats);
        return (
          <Sequence key={i} from={from} durationInFrames={to - from} name={s.kind === "tool" ? s.shot.id : s.kind}>
            {render(s)}
          </Sequence>
        );
      })}
      <Audio
        src={staticFile("audio/music.mp3")}
        volume={(f) => interpolate(f, [durationInFrames - 120, durationInFrames - 4], [1, 0], { extrapolateLeft: "clamp", extrapolateRight: "clamp" })}
      />
    </AbsoluteFill>
  );
};
