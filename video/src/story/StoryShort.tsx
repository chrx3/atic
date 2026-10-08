import { FORMAT_SHORT_H, FORMAT_SHORT_V } from "../lib/format";
import { Short } from "../short/ShortMain";
import { STORY_MONTAGE_CARDS, STORY_MONTAGE_KEYS, STORY_SHOTS } from "./storyShots";

/**
 * Tema del usuario («Tilt Tomorrow», paranormalroom13): tramo 139.22–169.22 s,
 * 128 BPM. Entra con 2 compases de subida; el primer drop cae en el compás 2,
 * el respiro en los compases 8-9 y el segundo drop en el 10.
 */
export const STORY_BPM = 128;
export const STORY_BARS = 16;
export const STORY_FRAMES = Math.round(((STORY_BARS * 240) / STORY_BPM) * 60);

const opts = {
  shots: STORY_SHOTS,
  montageCards: STORY_MONTAGE_CARDS,
  montageKeys: STORY_MONTAGE_KEYS,
  audio: false,
  audioSrc: "audio/user-cut.wav",
  bpm: STORY_BPM,
  kickBars: [
    [2, 8],
    [10, 16],
  ] as [number, number][],
};

export const StoryShortV = () => <Short format={FORMAT_SHORT_V} {...opts} />;
export const StoryShortH = () => <Short format={FORMAT_SHORT_H} {...opts} />;
