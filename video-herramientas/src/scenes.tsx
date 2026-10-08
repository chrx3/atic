import React from "react";
import { AbsoluteFill, Img, interpolate, Sequence, staticFile, useCurrentFrame } from "remotion";
import { BEAT_S, FPS, TOOLS, type ToolShot } from "./timeline";
import { Clip, ease, FONT, INK, MUTED, Words } from "./ui";

const BEAT = BEAT_S * FPS;
const WHITE = "#ffffff";

/** Tiempos 0–4: el gancho, dos golpes de texto sobre blanco. */
export const Hook: React.FC = () => {
  const f = useCurrentFrame();
  const second = f >= Math.round(2 * BEAT);
  return (
    <AbsoluteFill style={{ background: WHITE, justifyContent: "center", alignItems: "center" }}>
      {second ? (
        <Words key="b" text={"Una sola pill."} size={124} at={Math.round(2 * BEAT)} />
      ) : (
        <Words key="a" text={"Once herramientas."} size={104} />
      )}
    </AbsoluteFill>
  );
};

/** Tiempos 4–8: la marca. */
export const Brand: React.FC = () => {
  const f = useCurrentFrame();
  const pop = ease(f, 0, 22);
  return (
    <AbsoluteFill style={{ background: WHITE, justifyContent: "center", alignItems: "center", gap: 34 }}>
      <Img
        src={staticFile("icon.png")}
        style={{
          width: 230,
          height: 230,
          transform: `scale(${0.6 + 0.4 * pop})`,
          opacity: pop,
          filter: `drop-shadow(0 24px 40px rgba(0,0,0,${0.22 * pop}))`,
        }}
      />
      <Words text="Atic" size={150} at={6} />
      <Words text="Tu caja de herramientas de escritorio." size={40} weight={500} color={MUTED} at={16} stagger={2} />
    </AbsoluteFill>
  );
};

/** Plano a pantalla completa con titular abajo sobre un degradé. */
const FullShot: React.FC<{ shot: ToolShot; beats: number; titleAt?: number; chip?: boolean }> = ({
  shot,
  beats,
  titleAt = 4,
  chip = false,
}) => {
  const f = useCurrentFrame();
  const dur = Math.round(beats * BEAT);
  const punch = interpolate(f, [0, 20], [1.07, 1], { extrapolateRight: "clamp" });
  const out = interpolate(f, [dur - 8, dur], [1, 0.0], { extrapolateLeft: "clamp" });
  return (
    <AbsoluteFill style={{ background: "#000" }}>
      <AbsoluteFill style={{ transform: `scale(${punch})` }}>
        <Clip src={shot.clip} from={shot.from} width={1080} height={1080} crop={shot.crop} cropEnd={shot.cropEnd} duration={dur} />
      </AbsoluteFill>
      {chip ? (
        <Chip text={shot.sub} />
      ) : (
        <>
      <AbsoluteFill
        style={{
          background: "linear-gradient(180deg, rgba(0,0,0,0) 58%, rgba(0,0,0,0.78) 100%)",
          opacity: out,
        }}
      />
      <div style={{ position: "absolute", left: 64, right: 64, bottom: 70, opacity: out }}>
        <Words text={shot.title} size={78} color="#fff" at={titleAt} style={{ textAlign: "left" }} />
        <Words
          text={shot.sub}
          size={34}
          weight={500}
          color="rgba(255,255,255,0.72)"
          at={titleAt + 10}
          stagger={2}
          style={{ textAlign: "left", marginTop: 14, letterSpacing: "-0.01em" }}
        />
      </div>
        </>
      )}
    </AbsoluteFill>
  );
};

/** Etiqueta chica abajo al centro: el nombre de la herramienta. */
const Chip: React.FC<{ text: string }> = ({ text }) => {
  const f = useCurrentFrame();
  const t = ease(f, 2, 18);
  return (
    <div style={{ position: "absolute", left: 0, right: 0, bottom: 56, display: "flex", justifyContent: "center" }}>
      <div
        style={{
          padding: "16px 30px",
          borderRadius: 999,
          background: "rgba(12,12,14,0.72)",
          backdropFilter: "blur(18px)",
          border: "1px solid rgba(255,255,255,0.12)",
          color: "#fff",
          fontFamily: FONT,
          fontWeight: 600,
          fontSize: 32,
          letterSpacing: "-0.01em",
          opacity: t,
          transform: `translateY(${(1 - t) * 20}px)`,
        }}
      >
        {text}
      </div>
    </div>
  );
};

/**
 * Plano de herramienta sobre blanco: el titular entra solo, sube, y la UI
 * aparece en una tarjeta debajo.
 */
const CardShot: React.FC<{ shot: ToolShot; beats: number }> = ({ shot, beats }) => {
  const f = useCurrentFrame();
  const dur = Math.round(beats * BEAT);
  const rise = Math.round(1.5 * BEAT);
  const t = ease(f, rise, 26);
  const cardW = 944;
  const cardH = 770;
  return (
    <AbsoluteFill style={{ background: WHITE }}>
      <div
        style={{
          position: "absolute",
          left: 0,
          right: 0,
          top: interpolate(t, [0, 1], [430, 70]),
          transform: `scale(${interpolate(t, [0, 1], [1, 0.72])})`,
          transformOrigin: "50% 0",
        }}
      >
        <Words text={shot.title} size={96} />
        <Words text={shot.sub} size={38} weight={500} color={MUTED} at={10} stagger={2} style={{ marginTop: 16, letterSpacing: "-0.01em" }} />
      </div>
      <div
        style={{
          position: "absolute",
          left: (1080 - cardW) / 2,
          top: 262,
          width: cardW,
          height: cardH,
          borderRadius: 30,
          overflow: "hidden",
          boxShadow: "0 40px 90px rgba(10,12,30,0.28), 0 0 0 1px rgba(0,0,0,0.06)",
          transform: `translateY(${(1 - t) * 900}px) scale(${0.94 + 0.06 * t})`,
        }}
      >
        <Clip
          src={shot.clip}
          from={shot.from}
          width={cardW}
          height={cardH}
          crop={shot.crop}
          cropEnd={shot.cropEnd}
          duration={dur}
          startAt={rise - 6}
        />
      </div>
    </AbsoluteFill>
  );
};

/** Tras el drop: un golpe de titular en negro y corte a pantalla completa. */
const DarkShot: React.FC<{ shot: ToolShot; beats: number }> = ({ shot, beats }) => {
  const f = useCurrentFrame();
  const cut = Math.round(1.5 * BEAT);
  if (f < cut) {
    return (
      <AbsoluteFill style={{ background: "#050506", justifyContent: "center", alignItems: "center" }}>
        <Words text={shot.title} size={96} color="#fff" />
      </AbsoluteFill>
    );
  }
  return (
    <AbsoluteFill>
      <Sequence from={cut} layout="none">
        <FullShot shot={shot} beats={beats - 1.5} chip />
      </Sequence>
    </AbsoluteFill>
  );
};

export const PillScene: React.FC<{ shot: ToolShot; beats: number }> = ({ shot, beats }) => (
  <FullShot shot={shot} beats={beats} titleAt={30} />
);

export const ToolScene: React.FC<{ shot: ToolShot; beats: number }> = ({ shot, beats }) =>
  shot.look === "card" ? <CardShot shot={shot} beats={beats} /> : <DarkShot shot={shot} beats={beats} />;

/** «Y hay más»: parrilla con todas las herramientas corriendo a la vez. */
export const Montage: React.FC<{ beats: number }> = () => {
  const f = useCurrentFrame();
  const tiles = TOOLS.slice(0, 9);
  const size = 300;
  const gap = 22;
  const top = 250;
  const left = (1080 - (size * 3 + gap * 2)) / 2;
  return (
    <AbsoluteFill style={{ background: WHITE }}>
      <div style={{ position: "absolute", top: 90, left: 0, right: 0 }}>
        <Words text="Todo en una sola pill." size={76} />
      </div>
      {tiles.map((shot, i) => {
        const t = ease(f, 4 + i * 2, 20);
        const col = i % 3;
        const row = Math.floor(i / 3);
        return (
          <div
            key={shot.id}
            style={{
              position: "absolute",
              left: left + col * (size + gap),
              top: top + row * (size + gap),
              width: size,
              height: size,
              borderRadius: 22,
              overflow: "hidden",
              opacity: t,
              transform: `translateY(${(1 - t) * 60}px) scale(${0.9 + 0.1 * t})`,
              boxShadow: "0 18px 40px rgba(10,12,30,0.18), 0 0 0 1px rgba(0,0,0,0.05)",
            }}
          >
            <Clip src={shot.clip} from={shot.from} width={size} height={size} crop={shot.crop} duration={600} />
          </div>
        );
      })}
    </AbsoluteFill>
  );
};

/** Cierre: marca, promesa y dónde bajarlo. */
export const End: React.FC<{ beats: number }> = () => {
  const f = useCurrentFrame();
  const pop = ease(f, 0, 22);
  const url = ease(f, Math.round(3 * BEAT), 20);
  return (
    <AbsoluteFill style={{ background: WHITE, justifyContent: "center", alignItems: "center" }}>
      <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 26, marginTop: -40 }}>
        <Img
          src={staticFile("icon.png")}
          style={{ width: 190, height: 190, transform: `scale(${0.6 + 0.4 * pop})`, opacity: pop, filter: "drop-shadow(0 22px 36px rgba(0,0,0,0.2))" }}
        />
        <Words text="Atic" size={132} at={5} />
        <Words text="Gratis y de código abierto · Windows y macOS" size={36} weight={500} color={MUTED} at={Math.round(1.5 * BEAT)} stagger={2} />
        <div
          style={{
            marginTop: 18,
            padding: "20px 38px",
            borderRadius: 999,
            background: INK,
            color: "#fff",
            fontFamily: FONT,
            fontWeight: 600,
            fontSize: 38,
            letterSpacing: "-0.01em",
            opacity: url,
            transform: `translateY(${(1 - url) * 24}px)`,
          }}
        >
          github.com/chrx3/atic
        </div>
      </div>
    </AbsoluteFill>
  );
};
