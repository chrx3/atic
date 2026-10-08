import React from "react";
import { Composition } from "remotion";
import { Main } from "./Main";
import { FPS, TOTAL_FRAMES } from "./timeline";

export const Root: React.FC = () => (
  <Composition id="Herramientas" component={Main} durationInFrames={TOTAL_FRAMES} fps={FPS} width={1080} height={1080} />
);
