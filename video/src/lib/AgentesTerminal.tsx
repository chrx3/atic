import type { ReactNode } from "react";
import { C } from "./theme";
import { PROMPT, T } from "./agentesTimeline";

/** Tema oscuro del terminal (terminalTheme.ts). */
export const TERM = {
  bg: "#151715",
  fg: "#e8e8e1",
  cursor: "#e36f52",
  red: "#e0675f",
  green: "#73b98d",
  yellow: "#d4ad58",
  blue: "#78a9d4",
  dim: "#777970",
  claude: "#da7756",
};

export const TERM_FONT = 'Cascadia Mono, SFMono-Regular, Menlo, Consolas, monospace';
const FONT_PX = 12.5;
const ROW = 14; // 12.5 px × lineHeight 1.12
const MAX_ROWS = 31;

type Seg = { t: string; c?: string; b?: boolean };
type TextRow = {
  kind: "text";
  at: number;
  until?: number;
  /** ms por carácter; 0 = aparece completa. */
  cps?: number;
  segs: Seg[];
  bg?: string;
};
type BoxRow = {
  kind: "box";
  at: number;
  until?: number;
  width: number;
  border: string;
  rows: Seg[][];
};
type Row = TextRow | BoxRow;

const t = (text: string, c?: string, b?: boolean): Seg => ({ t: text, c, b });
const line = (at: number, segs: Seg[], extra: Partial<TextRow> = {}): TextRow => ({
  kind: "text",
  at,
  segs,
  ...extra,
});
const blank = (at: number): TextRow => line(at, []);

const DIFF_DEL = "#3a1f1d";
const DIFF_ADD = "#1d3325";

/** Salida inventada de un agente de código (el TUI real lo pinta el CLI). */
const ROWS: Row[] = [
  {
    kind: "box",
    at: T.bootEnd,
    width: 344,
    border: TERM.claude,
    rows: [
      [t("✻ ", TERM.claude), t("Te damos la bienvenida a "), t("Claude Code", undefined, true)],
      [t("  /help para ver los comandos", TERM.dim)],
      [t("  cwd: ~/proyectos/atic", TERM.dim)],
    ],
  },
  blank(T.bootEnd),
  line(T.send + 50, [t("> ", TERM.dim), t(PROMPT)], { bg: "#22241f" }),
  blank(T.send + 50),
  line(2750, [t("● "), t("Voy a revisar el log del build.")], { cps: 10 }),
  blank(3100),
  line(3150, [t("● ", TERM.green), t("Read", undefined, true), t("(build.log)")]),
  line(3250, [t("  ⎿  Leídas 84 líneas", TERM.dim)]),
  line(3400, [t("● ", TERM.green), t("Update", undefined, true), t("(src/pill/stage.rs)")]),
  line(3500, [t("  ⎿  Actualizado con 1 adición y 1 eliminación", TERM.dim)]),
  line(3550, [t("     40      let bar_width = measure();", TERM.dim)]),
  line(3600, [t("     41  -   let w: u32 = bar_width;")], { bg: DIFF_DEL }),
  line(3650, [t("     41  +   let w: f32 = bar_width;")], { bg: DIFF_ADD }),
  line(3700, [t("     42      stage.resize(w);", TERM.dim)]),
  line(3750, [t("● "), t("Bash", undefined, true), t("(cargo test --locked -p atic-desktop)")]),
  {
    kind: "box",
    at: T.dialog,
    until: T.approve,
    width: 452,
    border: TERM.dim,
    rows: [
      [t("Comando de Bash", TERM.dim)],
      [t("  cargo test --locked -p atic-desktop")],
      [t("  Corre los tests del crate", TERM.dim)],
      [t("¿Quieres continuar?", undefined, true)],
      [t("❯ 1. Sí", TERM.blue)],
      [t("  2. Sí, y no volver a preguntar", TERM.fg)],
      [t("  3. No, y decirle qué hacer distinto", TERM.fg)],
    ],
  },
  line(5620, [t("  ⎿  test result: ok. ", TERM.dim), t("12 passed", TERM.green), t("; 0 failed", TERM.dim)], {
    cps: 6,
  }),
  blank(5900),
  line(5900, [t("● "), t("Listo. ", undefined, true), t("El ancho de la barra era u32 y se truncaba.")], { cps: 6 }),
  line(6230, [t("  Lo cambié a f32 y los tests pasan.")], { cps: 6 }),
];

const rowCount = (r: Row) => (r.kind === "box" ? r.rows.length + 1 : 1);

const renderSegs = (segs: Seg[], visible: number): ReactNode => {
  let left = visible;
  return segs.map((s, i) => {
    if (left <= 0) return null;
    const text = s.t.slice(0, left);
    left -= s.t.length;
    return (
      <span key={i} style={{ color: s.c, fontWeight: s.b ? 700 : 400 }}>
        {text}
      </span>
    );
  });
};

const Cursor = ({ ms }: { ms: number }) => (
  <span
    style={{
      display: "inline-block",
      width: 7.3,
      height: ROW - 2,
      marginLeft: 1,
      verticalAlign: "top",
      background: TERM.cursor,
      opacity: Math.floor(ms / 530) % 2 ? 0.25 : 1,
    }}
  />
);

/** TerminalView: marco #121211 con el lienzo xterm (#151715) inset 8/4/4/12. */
export const AgentesTerminal = ({ ms }: { ms: number }) => {
  const bootT = ms < T.bootEnd;
  const visible = ROWS.filter((r) => ms >= r.at && (r.until === undefined || ms < r.until));
  const total = visible.reduce((n, r) => n + rowCount(r), 0);
  const scroll = Math.max(0, total - MAX_ROWS) * ROW;
  const last = visible[visible.length - 1];

  return (
    <div style={{ position: "absolute", inset: 0, background: C.bg }}>
      <div
        style={{
          position: "absolute",
          inset: "8px 4px 4px 12px",
          background: TERM.bg,
          overflow: "hidden",
          fontFamily: TERM_FONT,
          fontSize: FONT_PX,
          lineHeight: `${ROW}px`,
          color: TERM.fg,
        }}
      >
        {bootT && (
          <div
            style={{
              position: "absolute",
              inset: 0,
              display: "grid",
              placeItems: "center",
              fontFamily: "inherit",
              fontSize: 12,
              color: "#9a9a90",
            }}
          >
            Arrancando…
          </div>
        )}
        <div style={{ transform: `translateY(${-scroll}px)`, padding: "0 4px" }}>
          {visible.map((r, i) => {
            if (r.kind === "box") {
              return (
                <div
                  key={i}
                  style={{
                    boxSizing: "border-box",
                    width: r.width,
                    height: rowCount(r) * ROW,
                    padding: `${ROW / 2 - 1}px 10px`,
                    border: `1px solid ${r.border}`,
                    borderRadius: 6,
                    whiteSpace: "pre",
                  }}
                >
                  {r.rows.map((segs, k) => (
                    <div key={k} style={{ height: ROW }}>
                      {renderSegs(segs, 999)}
                    </div>
                  ))}
                </div>
              );
            }
            const chars = r.segs.reduce((n, s) => n + s.t.length, 0);
            const shown = r.cps ? Math.min(chars, Math.floor((ms - r.at) / r.cps) + 1) : chars;
            return (
              <div
                key={i}
                style={{
                  height: ROW,
                  whiteSpace: "pre",
                  background: r.bg,
                  marginLeft: -4,
                  marginRight: -4,
                  padding: "0 4px",
                }}
              >
                {renderSegs(r.segs, shown)}
                {r === last && !bootT && <Cursor ms={ms} />}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
};
