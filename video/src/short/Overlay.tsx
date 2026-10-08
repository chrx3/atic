import { useFormat } from "../lib/format";
import { C, EASE, FONT_SANS } from "../lib/theme";
import { seg } from "../lib/time";

/** Tecla física que se hunde al pulsarse. `at` = ms en que se pulsa. */
export const KeyCap = ({ label, ms, at, size }: { label: string; ms: number; at: number; size: number }) => {
  const appear = EASE.smoothOut(seg(ms, at - 90, 120));
  const press = seg(ms, at, 90);
  const release = seg(ms, at + 90, 160);
  const down = press * (1 - release);
  return (
    <span
      style={{
        display: "inline-block",
        fontFamily: FONT_SANS,
        fontSize: size,
        fontWeight: 650,
        lineHeight: 1,
        color: C.text,
        padding: `${size * 0.22}px ${size * 0.46}px`,
        borderRadius: size * 0.3,
        background: C.elevated,
        boxShadow: `inset 0 0 0 ${size * 0.045}px ${C.lineStrong}, 0 ${(1 - down) * size * 0.1}px 0 rgb(0 0 0 / 55%)`,
        transform: `translateY(${down * size * 0.1}px) scale(${1 + (1 - appear) * 0.3})`,
        opacity: appear,
      }}
    >
      {label}
    </span>
  );
};

/** Ajusta el tamaño para que una línea de mayúsculas entre en `maxWidth`. */
const fitFont = (text: string, base: number, maxWidth: number) =>
  Math.min(base, maxWidth / (0.64 * Math.max(...text.split("\n").map((l) => l.length))));

/**
 * Título del plano: palabra en español que golpea, línea en inglés debajo y
 * el atajo como teclas que se pulsan una tras otra.
 */
export const ShotText = ({
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
  const format = useFormat();
  const h = format.id === "h";
  const maxW = h ? 820 : 926;
  const fs = fitFont(es, h ? 210 : 150, maxW);
  const slam = EASE.smoothOut(seg(ms, 0, 260));
  const sub = EASE.smoothOut(seg(ms, 140, 260));
  const keySize = h ? 44 : 38;

  return (
    <div
      style={{
        position: "absolute",
        left: h ? 110 : 77,
        top: h ? 300 : 96,
        width: maxW,
        fontFamily: FONT_SANS,
        color: C.text,
      }}
    >
      <div
        style={{
          fontSize: fs,
          fontWeight: 750,
          letterSpacing: "-0.03em",
          lineHeight: 1.02,
          whiteSpace: "pre-line",
          opacity: slam,
          transform: `translateX(${(1 - slam) * -40}px) scale(${1 + (1 - slam) * 0.28})`,
          transformOrigin: "0 60%",
          filter: `blur(${(1 - slam) * 10}px)`,
          textShadow: "0 6px 30px rgb(0 0 0 / 45%)",
        }}
      >
        {es}
      </div>
      <div
        style={{
          marginTop: h ? 14 : 4,
          fontSize: h ? 58 : 42,
          fontWeight: 400,
          lineHeight: 1.15,
          color: C.muted,
          opacity: sub,
          transform: `translateY(${(1 - sub) * 16}px)`,
        }}
      >
        {en}
      </div>
      {keys && (
        <div style={{ marginTop: h ? 30 : 18, display: "flex", gap: keySize * 0.3 }}>
          {keys.map((k, i) => (
            <KeyCap key={k} label={k} ms={ms} at={220 + i * 110} size={keySize} />
          ))}
        </div>
      )}
    </div>
  );
};

/** Destello blanco al inicio de un corte. */
export const Flash = ({ ms }: { ms: number }) => (
  <div
    style={{
      position: "absolute",
      inset: 0,
      background: "#fff",
      opacity: 0.32 * (1 - seg(ms, 0, 150)),
      pointerEvents: "none",
    }}
  />
);
