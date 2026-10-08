import type { ComponentType } from "react";
import { AbsoluteFill, Audio, Sequence, staticFile, useCurrentFrame, useVideoConfig } from "remotion";
import { Cam3DProvider } from "../lib/cam3d";
import { FormatProvider, type Format } from "../lib/format";
import { TimeRemap } from "../lib/time";
import { ShortAudio } from "./audio";
import { shotCam, kickPulse } from "./camera";
import { Cta } from "./Cta";
import { Montage } from "./Montage";
import { Flash, KeyCap, ShotText } from "./Overlay";
import { PayoffScene } from "./Payoff";
import { BPM, SHOTS, type Shot } from "./shots";
import { Stage } from "./Stage";
import { useFormat } from "../lib/format";
import { MONTAGE_KEYS } from "./shots";
import { C, FONT_SANS } from "../lib/theme";

export type ShortOptions = {
  shots?: Shot[];
  montageCards?: { scene: ComponentType; atMs: number }[];
  montageKeys?: string[][];
  /** Música y efectos incrustados (se desactiva cuando el usuario pone su propio tema). */
  audio?: boolean;
  /** Tema propio del usuario (archivo en `public/`); reemplaza a la música sintética. */
  audioSrc?: string;
  /** Ritmo de los cortes; 150 BPM por defecto. */
  bpm?: number;
  /** Compases con bombo para el latido del monitor. */
  kickBars?: [number, number][];
};

const ShotView = ({
  shot,
  index,
  opts,
}: {
  shot: Shot;
  index: number;
  opts: ShortOptions;
}) => {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const format = useFormat();
  const ms = (frame / fps) * 1000;
  const barMs = 240000 / (opts.bpm ?? BPM);
  const durMs = shot.bars * barMs;
  const gMs = shot.bar * barMs + ms;
  const pulse = kickPulse(gMs, { barMs, beatMs: barMs / 4, kickBars: opts.kickBars });
  const cam = shotCam(shot, index, ms, durMs, pulse, format.id === "h");

  let body: React.ReactNode = null;
  if (shot.kind === "scene" && shot.scene && shot.segments) {
    const Scene = shot.scene;
    body = (
      <Cam3DProvider cam={cam}>
        <TimeRemap segments={shot.segments} durationMs={durMs}>
          <Scene />
        </TimeRemap>
      </Cam3DProvider>
    );
  } else if (shot.kind === "montage") {
    body = <Montage ms={ms} durMs={durMs} cards={opts.montageCards} />;
  } else if (shot.kind === "payoff") {
    const Payoff = shot.scene ?? PayoffScene;
    body = (
      <Cam3DProvider cam={cam}>
        <Payoff />
      </Cam3DProvider>
    );
  } else if (shot.kind === "cta") {
    body = <Cta ms={ms} />;
  }

  return (
    <AbsoluteFill>
      {body}
      {shot.kind === "montage" ? (
        <MontageText ms={ms} durMs={durMs} shot={shot} keysList={opts.montageKeys ?? MONTAGE_KEYS} />
      ) : shot.kind === "cta" ? null : (
        <ShotText es={shot.es} en={shot.en} keys={shot.keys} ms={ms} />
      )}
      {index > 0 && <Flash ms={ms} />}
    </AbsoluteFill>
  );
};

/** Título fijo + atajos que desfilan al ritmo del vuelo entre planos. */
const MontageText = ({
  ms,
  durMs,
  shot,
  keysList,
}: {
  ms: number;
  durMs: number;
  shot: Shot;
  keysList: string[][];
}) => {
  const format = useFormat();
  const h = format.id === "h";
  const per = durMs / keysList.length;
  const i = Math.min(keysList.length - 1, Math.floor(ms / per));
  const local = ms - i * per;
  const keys = keysList[i];
  return (
    <>
      <ShotText es={shot.es} en={shot.en} ms={ms} />
      <div
        style={{
          position: "absolute",
          left: h ? 110 : 77,
          top: h ? 690 : 330,
          display: "flex",
          gap: 12,
        }}
      >
        {keys.map((k, j) => (
          <KeyCap key={`${i}-${k}`} label={k} ms={local} at={j * 45} size={h ? 46 : 40} />
        ))}
      </div>
    </>
  );
};

/** Corto de ~29 s: 12 planos sincronizados a 150 BPM. */
export const Short = ({ format, ...opts }: { format: Format } & ShortOptions) => {
  const shots = opts.shots ?? SHOTS;
  const withAudio = opts.audio ?? true;
  const barMs = 240000 / (opts.bpm ?? BPM);
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const gMs = (frame / fps) * 1000;
  return (
    <FormatProvider format={format}>
      <AbsoluteFill style={{ background: C.bg }}>
        <Stage gMs={gMs} />
        {shots.map((shot, i) => (
          <Sequence
            key={shot.id}
            from={Math.round((shot.bar * barMs * fps) / 1000)}
            durationInFrames={Math.round((shot.bars * barMs * fps) / 1000)}
            name={shot.id}
          >
            <ShotView shot={shot} index={i} opts={opts} />
          </Sequence>
        ))}
        {opts.audioSrc && <Audio src={staticFile(opts.audioSrc)} />}
        {withAudio && !opts.audioSrc && <ShortAudio />}
      </AbsoluteFill>
    </FormatProvider>
  );
};
