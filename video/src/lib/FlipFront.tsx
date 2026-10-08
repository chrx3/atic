import { FONT_SANS } from "./theme";

/** Título de la ventana ajena que se voltea (aparece en la píldora «sobre …»). */
export const FLIP_WINDOW_TITLE = "Informe de planta";

const line = (w: number, key: number, strong = false) => (
  <div
    key={key}
    style={{
      height: 12,
      width: `${w}%`,
      borderRadius: 6,
      background: strong ? "#c5cad0" : "#dde0e4",
    }}
  />
);

const Ctl = ({ kind }: { kind: "min" | "max" | "close" }) => (
  <svg width={46} height={44} viewBox="0 0 46 44" fill="none" stroke="#5c646c" strokeWidth={1.4} strokeLinecap="round">
    {kind === "min" && <path d="M18 22h10" />}
    {kind === "max" && <rect x="18" y="17" width="10" height="10" rx="1.5" />}
    {kind === "close" && (
      <>
        <path d="m18 17 10 10" />
        <path d="m28 17-10 10" />
      </>
    )}
  </svg>
);

/** Frente del flip: captura de una ventana genérica (documento con texto y un gráfico). */
export const FlipFront = () => {
  const bars = [46, 58, 52, 71, 64, 88, 79];
  return (
    <div
      style={{
        position: "absolute",
        inset: 0,
        background: "#e6e9ec",
        fontFamily: FONT_SANS,
        color: "#23272b",
        overflow: "hidden",
      }}
    >
      {/* barra de título */}
      <div
        style={{
          height: 44,
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          background: "#f5f6f7",
          borderBottom: "1px solid #d8dce0",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 10, paddingLeft: 16, fontSize: 13, color: "#3a4047" }}>
          <svg width={18} height={18} viewBox="0 0 24 24" fill="none" stroke="#526d83" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round">
            <path d="M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.7.7l3.6 3.6A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z" />
            <path d="M14 2v5a1 1 0 0 0 1 1h5" />
          </svg>
          {FLIP_WINDOW_TITLE}
        </div>
        <div style={{ display: "flex" }}>
          <Ctl kind="min" />
          <Ctl kind="max" />
          <Ctl kind="close" />
        </div>
      </div>

      {/* cinta de herramientas */}
      <div style={{ height: 50, display: "flex", alignItems: "center", gap: 10, padding: "0 20px", background: "#fafafb", borderBottom: "1px solid #dde0e4" }}>
        {[64, 48, 48, 36, 36, 80, 36, 36].map((w, i) => (
          <div key={i} style={{ width: w, height: 24, borderRadius: 6, background: i === 1 ? "#dbe5ee" : "#eceef1" }} />
        ))}
      </div>

      {/* hoja */}
      <div
        style={{
          position: "absolute",
          left: 200,
          top: 116,
          width: 880,
          height: 720,
          background: "#ffffff",
          boxShadow: "0 1px 4px rgb(30 36 44 / 16%), 0 8px 24px rgb(30 36 44 / 8%)",
          borderRadius: 4,
          padding: "52px 64px",
          boxSizing: "border-box",
        }}
      >
        <div style={{ fontSize: 40, fontWeight: 650, letterSpacing: "-0.02em", color: "#1f2328", lineHeight: 1.1 }}>
          Informe mensual de operación
        </div>
        <div style={{ marginTop: 10, fontSize: 17, color: "#7b838b" }}>Planta Sur · septiembre</div>

        <div style={{ marginTop: 34, display: "grid", gap: 14 }}>
          {[96, 92, 98, 64].map((w, i) => line(w, i))}
        </div>

        <div style={{ marginTop: 32, fontSize: 19, fontWeight: 600, color: "#2c3339" }}>Caudal de entrada</div>
        <svg width={752} height={210} viewBox="0 0 752 210" style={{ display: "block", marginTop: 14 }}>
          {[0, 1, 2, 3].map((i) => (
            <line key={i} x1={0} x2={752} y1={20 + i * 50} y2={20 + i * 50} stroke="#e3e6ea" strokeWidth={1} />
          ))}
          {bars.map((h, i) => (
            <rect key={i} x={24 + i * 104} y={190 - h * 1.7} width={64} height={h * 1.7} rx={4} fill={i === 5 ? "#526d83" : "#b7c6d3"} />
          ))}
        </svg>

        <div style={{ marginTop: 26, display: "grid", gap: 14 }}>
          {[94, 88, 52].map((w, i) => line(w, i))}
        </div>
      </div>
    </div>
  );
};
