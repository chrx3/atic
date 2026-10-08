import { AbsoluteFill } from "remotion";
import { AticMark, blinkLid } from "../lib/AticMark";
import { Icon, type IconName } from "../lib/Icon";
import { C, EASE, FONT_SANS, SHADOW_GOO } from "../lib/theme";
import { seg } from "../lib/time";
import { KeyCap } from "../short/Overlay";

/** Texto de la marca en el video de lanzamiento: negro sobre blanco, la UI de Atic es la oscura. */
export const INK = "#0b0b0c";
export const INK_SOFT = "#6b6f78";

const wordStyle = (ms: number, i: number) => {
  const t = EASE.smoothOut(seg(ms, i * 45, 230));
  return {
    display: "inline-block",
    opacity: t,
    transform: `translateY(${(1 - t) * 46}px)`,
    filter: `blur(${(1 - t) * 10}px)`,
  } as const;
};

/** Diapositiva de texto: frase grande en español, inglés debajo y el atajo como teclas. */
export const TextSlide = ({
  es,
  en,
  keys,
  ms,
}: {
  es: string;
  en: string;
  keys?: string[];
  ms: number;
}) => {
  const lines = es.split("\n");
  const longest = Math.max(...lines.map((l) => l.length));
  const fs = Math.min(150, 940 / (0.5 * longest));
  let wi = 0;
  const sub = EASE.smoothOut(seg(ms, 170, 220));
  return (
    <AbsoluteFill
      style={{
        background: "#ffffff",
        alignItems: "center",
        justifyContent: "center",
        textAlign: "center",
        fontFamily: FONT_SANS,
        padding: "0 70px",
      }}
    >
      <div
        style={{
          fontSize: fs,
          fontWeight: 700,
          letterSpacing: "-0.035em",
          lineHeight: 1.04,
          color: INK,
        }}
      >
        {lines.map((line, li) => (
          <div key={li}>
            {line.split(" ").map((w) => (
              <span key={wi} style={{ ...wordStyle(ms, wi++), marginRight: "0.26em" }}>
                {w}
              </span>
            ))}
          </div>
        ))}
      </div>
      <div
        style={{
          marginTop: 30,
          fontSize: Math.max(36, fs * 0.36),
          fontWeight: 400,
          color: INK_SOFT,
          opacity: sub,
          transform: `translateY(${(1 - sub) * 18}px)`,
        }}
      >
        {en}
      </div>
      {keys && (
        <div style={{ marginTop: 34, display: "flex", gap: 14 }}>
          {keys.map((k, i) => (
            <KeyCap key={k} label={k} ms={ms} at={230 + i * 70} size={44} />
          ))}
        </div>
      )}
    </AbsoluteFill>
  );
};

/** Etiqueta discreta sobre el escritorio: herramienta y atajo, con el estilo de la pill. */
export const ToolChip = ({ label, keys, ms }: { label: string; keys?: string[]; ms: number }) => {
  const t = EASE.smoothOut(seg(ms, 80, 260));
  return (
    <div
      style={{
        position: "absolute",
        left: 34,
        top: 34,
        display: "flex",
        alignItems: "center",
        gap: 14,
        padding: "12px 22px",
        borderRadius: 999,
        background: C.skin,
        color: C.text,
        fontFamily: FONT_SANS,
        fontSize: 30,
        fontWeight: 650,
        filter: SHADOW_GOO,
        opacity: t,
        transform: `translateY(${(1 - t) * -14}px) scale(${0.94 + 0.06 * t})`,
      }}
    >
      {label}
      {keys && (
        <span style={{ display: "inline-flex", gap: 8 }}>
          {keys.map((k) => (
            <span
              key={k}
              style={{
                fontSize: 24,
                fontWeight: 600,
                color: C.muted,
                background: C.surface2,
                borderRadius: 9,
                padding: "3px 12px",
                boxShadow: `inset 0 0 0 1.5px ${C.lineStrong}`,
              }}
            >
              {k}
            </span>
          ))}
        </span>
      )}
    </div>
  );
};

type CardData = { icon: IconName; cat: string; title: string; desc: string };

/** Herramientas y textos reales de la app (core/tools.ts y i18n/es.ts). */
const CARDS: CardData[] = [
  { icon: "meetings", cat: "Reuniones", title: "Grabar y resumir", desc: "Audio del PC, transcripción local y resúmenes editables." },
  { icon: "mic", cat: "Dictado", title: "Voz a texto", desc: "Habla y pega texto en cualquier app con un atajo." },
  { icon: "search", cat: "Apps", title: "Launcher tipo Spotlight", desc: "Abre apps y acciones del PC con Ctrl+Espacio." },
  { icon: "system", cat: "Sistema", title: "Volumen y recursos", desc: "CPU, RAM y pantallas; cierra apps desde la pill." },
  { icon: "window", cat: "Local-first", title: "Todo en tu PC", desc: "Whisper transcribe en local. Código abierto (MIT)." },
  { icon: "snippets", cat: "La pill", title: "A tu medida", desc: "Ordena y elige las herramientas que quieres a mano." },
];

/** «Y hay más»: parrilla de tarjetas oscuras sobre blanco. */
export const MoreGrid = ({ ms }: { ms: number }) => {
  const head = EASE.smoothOut(seg(ms, 0, 300));
  return (
    <AbsoluteFill style={{ background: "#ffffff", fontFamily: FONT_SANS }}>
      <div style={{ position: "absolute", left: 0, right: 0, top: 58, textAlign: "center", opacity: head, transform: `translateY(${(1 - head) * 24}px)` }}>
        <div style={{ fontSize: 88, fontWeight: 700, letterSpacing: "-0.03em", color: INK, lineHeight: 1 }}>Y hay más</div>
        <div style={{ fontSize: 38, color: INK_SOFT, marginTop: 10 }}>And there’s more</div>
      </div>
      <div
        style={{
          position: "absolute",
          left: 44,
          right: 44,
          top: 240,
          display: "grid",
          gridTemplateColumns: "1fr 1fr",
          gap: 22,
        }}
      >
        {CARDS.map((c, i) => {
          const t = EASE.smoothOut(seg(ms, 180 + i * 90, 340));
          return (
            <div
              key={c.cat}
              style={{
                background: C.skin,
                borderRadius: 30,
                padding: "22px 24px",
                display: "flex",
                gap: 20,
                alignItems: "center",
                color: C.text,
                height: 210,
                boxSizing: "border-box",
                opacity: t,
                transform: `translateY(${(1 - t) * 40}px) scale(${0.96 + 0.04 * t})`,
                boxShadow: "0 18px 40px rgb(15 15 20 / 16%)",
              }}
            >
              <div style={{ width: 92, height: 92, flex: "none", borderRadius: 26, background: C.elevated, display: "grid", placeItems: "center", color: C.text }}>
                <Icon name={c.icon} size={46} strokeWidth={1.6} />
              </div>
              <div style={{ minWidth: 0 }}>
                <div style={{ fontSize: 22, color: C.faint, fontWeight: 500 }}>{c.cat}</div>
                <div style={{ fontSize: 33, fontWeight: 650, lineHeight: 1.12, marginTop: 2 }}>{c.title}</div>
                <div style={{ fontSize: 22, color: C.muted, lineHeight: 1.25, marginTop: 6 }}>{c.desc}</div>
              </div>
            </div>
          );
        })}
      </div>
    </AbsoluteFill>
  );
};

/** Cierre: marca, qué es Atic y dónde encontrarlo. */
export const EndCard = ({ ms }: { ms: number }) => {
  const a = EASE.smoothOut(seg(ms, 0, 420));
  const b = EASE.smoothOut(seg(ms, 220, 420));
  const c = EASE.smoothOut(seg(ms, 460, 420));
  return (
    <AbsoluteFill
      style={{
        background: "#ffffff",
        alignItems: "center",
        justifyContent: "center",
        textAlign: "center",
        fontFamily: FONT_SANS,
        gap: 24,
      }}
    >
      <div
        style={{
          width: 250,
          height: 250,
          borderRadius: 125,
          background: C.skin,
          display: "grid",
          placeItems: "center",
          filter: "drop-shadow(0 18px 34px rgb(15 15 20 / 28%))",
          opacity: a,
          transform: `scale(${0.7 + 0.3 * a})`,
        }}
      >
        <AticMark size={170} strokeWidth={1.5} ms={ms} lookX={Math.sin(ms / 600) * 0.9} lid={blinkLid(ms, [1100, 2700])} />
      </div>
      <div style={{ fontSize: 130, fontWeight: 700, letterSpacing: "-0.04em", lineHeight: 1, color: INK, opacity: b, transform: `translateY(${(1 - b) * 24}px)` }}>
        Atic
      </div>
      <div style={{ fontSize: 40, color: INK_SOFT, lineHeight: 1.3, opacity: b, transform: `translateY(${(1 - b) * 24}px)` }}>
        Caja de herramientas de escritorio
        <br />
        Desktop toolbox · local-first
      </div>
      <div style={{ display: "grid", gap: 12, justifyItems: "center", opacity: c, transform: `translateY(${(1 - c) * 24}px)`, marginTop: 6 }}>
        <div style={{ fontSize: 42, fontWeight: 600, color: C.text, background: C.skin, borderRadius: 999, padding: "14px 40px" }}>
          github.com/chrx3/atic
        </div>
        <div style={{ fontSize: 38, color: INK }}>chrsx3.com</div>
        <div style={{ fontSize: 28, color: INK_SOFT }}>Windows · macOS · Open source (MIT)</div>
      </div>
    </AbsoluteFill>
  );
};
