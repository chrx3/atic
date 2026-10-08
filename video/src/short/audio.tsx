import { Audio, Sequence, staticFile } from "remotion";
import { BAR_MS, FPS, MONTAGE_KEYS, SHOTS } from "./shots";

const frames = (ms: number) => Math.round((ms * FPS) / 1000);

const Sfx = ({ file, atMs, volume, durMs }: { file: string; atMs: number; volume: number; durMs: number }) => {
  const from = frames(atMs);
  if (from < 0) return null;
  return (
    <Sequence from={from} durationInFrames={Math.max(1, frames(durMs))} layout="none">
      <Audio src={staticFile(`audio/${file}`)} volume={volume} />
    </Sequence>
  );
};

/**
 * Música (síntesis de `audio/build_track.py`, 150 BPM) más efectos:
 * un whoosh que remata en cada corte y un clic por cada tecla del atajo.
 */
export const ShortAudio = () => {
  const cuts = SHOTS.slice(1).map((s) => s.bar * BAR_MS);
  const clicks: number[] = [];
  for (const s of SHOTS) {
    if (s.kind === "scene" && s.keys) {
      s.keys.forEach((_, i) => clicks.push(s.bar * BAR_MS + 220 + i * 110));
    }
    if (s.kind === "montage") {
      const per = (s.bars * BAR_MS) / MONTAGE_KEYS.length;
      MONTAGE_KEYS.forEach((keys, i) =>
        keys.forEach((_, j) => clicks.push(s.bar * BAR_MS + i * per + j * 45)),
      );
    }
  }
  return (
    <>
      <Audio src={staticFile("audio/track.wav")} volume={0.92} />
      {cuts.map((t) => (
        <Sfx key={`w${t}`} file="sfx_whoosh.wav" atMs={t - 300} volume={0.45} durMs={520} />
      ))}
      {clicks.map((t, i) => (
        <Sfx key={`c${i}`} file="sfx_click.wav" atMs={t} volume={0.38} durMs={90} />
      ))}
    </>
  );
};
