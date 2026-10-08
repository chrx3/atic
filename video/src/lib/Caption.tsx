import { useFormat } from "./format";
import { C, EASE, FONT_SANS } from "./theme";
import { seg, useMs } from "./time";

export const Keys = ({ keys, size = 1 }: { keys: string[]; size?: number }) => (
  <span style={{ display: "inline-flex", gap: 8 * size }}>
    {keys.map((k) => (
      <span
        key={k}
        style={{
          fontFamily: FONT_SANS,
          fontSize: 34 * size,
          fontWeight: 600,
          color: C.text,
          padding: `${6 * size}px ${16 * size}px`,
          borderRadius: 12 * size,
          background: C.elevated,
          boxShadow: `inset 0 0 0 1.5px ${C.lineStrong}, 0 3px 0 rgb(0 0 0 / 45%)`,
        }}
      >
        {k}
      </span>
    ))}
  </span>
);

/** Título de escena sobre el monitor (capa del video, no de la app). */
export const Caption = ({
  title,
  sub,
  keys,
  inMs = 0,
}: {
  title: string;
  sub?: string;
  keys?: string[];
  inMs?: number;
}) => {
  const ms = useMs();
  const { showCaption } = useFormat();
  if (!showCaption) return null;
  const t = EASE.smoothOut(seg(ms, inMs, 500));
  return (
    <div
      style={{
        position: "absolute",
        left: 80,
        right: 80,
        top: 130,
        fontFamily: FONT_SANS,
        color: C.text,
        opacity: t,
        transform: `translateY(${(1 - t) * 24}px)`,
      }}
    >
      <div style={{ fontSize: 104, fontWeight: 650, letterSpacing: "-0.03em", lineHeight: 1 }}>
        {title}
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 18, marginTop: 22, minHeight: 56 }}>
        {keys && <Keys keys={keys} />}
        {sub && (
          <span style={{ fontSize: 40, color: C.muted, fontWeight: 400, lineHeight: 1.2 }}>{sub}</span>
        )}
      </div>
    </div>
  );
};
