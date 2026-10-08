import { AppWindow, CODE_LINE_H, D, TerminalPanel, type Rect, type TermLine } from "../desk/desk";
import { FONT_MONO, FONT_SANS } from "../lib/theme";

type Tok = [string, string];

/**
 * Copia de `EditorApp` (desk.tsx) con dos cambios: el pool ya corregido
 * (`max: 20`, `connectionTimeoutMillis: 5000`) y la pestaña con el punto de «modificado».
 */
const CODE_HEAD: Tok[][] = [
  [["import ", D.kw], ["{ Pool } ", D.edText], ["from ", D.kw], ['"pg"', D.str], [";", D.edText]],
  [],
  [["const ", D.kw], ["pool ", D.edText], ["= new ", D.kw], ["Pool", D.ty], ["({", D.edText]],
  [["  host: ", D.edText], ["process.env.DB_HOST", D.fn], [",", D.edText]],
];
const POOL_BROKEN: Tok[][] = [
  [["  max: ", D.edText], ["5", D.num], [",", D.edText], ["  // pocas conexiones", D.cm]],
  [["  connectionTimeoutMillis: ", D.edText], ["2000", D.num], [",", D.edText]],
];
const POOL_FIXED: Tok[][] = [
  [["  max: ", D.edText], ["20", D.num], [",", D.edText], ["  // pool ampliado", D.cm]],
  [["  connectionTimeoutMillis: ", D.edText], ["5000", D.num], [",", D.edText]],
];
const CODE_TAIL: Tok[][] = [
  [["});", D.edText]],
  [],
  [["export async function ", D.kw], ["getOrders", D.fn], ["(id: ", D.edText], ["string", D.ty], [") {", D.edText]],
  [["  const ", D.kw], ["{ rows } ", D.edText], ["= await ", D.kw], ["pool.query", D.fn], ["(", D.edText]],
  [['    "SELECT * FROM orders WHERE user = $1"', D.str], [",", D.edText]],
  [["    [id]", D.edText]],
  [["  );", D.edText]],
  [["  return ", D.kw], ["rows", D.edText], [";", D.edText]],
  [["}", D.edText]],
  [],
  [["export async function ", D.kw], ["countOrders", D.fn], ["() {", D.edText]],
  [["  return ", D.kw], ["pool.query", D.fn], ["(", D.edText], ['"SELECT count(*) FROM orders"', D.str], [");", D.edText]],
  [["}", D.edText]],
];

const POOL_START = CODE_HEAD.length;
const POOL_END = POOL_START + POOL_BROKEN.length;

export const AgentesStoryEditor = ({
  rect,
  fixed,
  termLines,
  termH,
  highlightLine,
}: {
  rect: Rect;
  /** El agente ya editó src/db.ts. */
  fixed: boolean;
  termLines: TermLine[];
  termH: number;
  highlightLine?: number;
}) => {
  const code = [...CODE_HEAD, ...(fixed ? POOL_FIXED : POOL_BROKEN), ...CODE_TAIL];
  return (
    <AppWindow rect={rect} title="db.ts — tienda-api — Editor" dark>
      <div style={{ position: "absolute", inset: 0, display: "flex", flexDirection: "column" }}>
        <div style={{ flex: 1, display: "flex", minHeight: 0 }}>
          <div style={{ width: 26, background: D.edSide, display: "grid", gridAutoRows: 22, justifyItems: "center", alignContent: "start", paddingTop: 6, gap: 2 }}>
            {[D.edMuted, D.edMuted, D.edMuted].map((c, i) => (
              <span key={i} style={{ width: 10, height: 10, borderRadius: 3, border: `1.5px solid ${i === 0 ? "#9fb3ff" : c}` }} />
            ))}
          </div>
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ height: 20, display: "flex", background: D.edSide, fontSize: 8, fontFamily: FONT_SANS }}>
              {["db.ts", "orders.test.ts"].map((t, i) => (
                <span key={t} style={{ padding: "0 10px", display: "flex", alignItems: "center", gap: 4, background: i === 0 ? D.edBg : "transparent", color: i === 0 ? D.edText : D.edMuted, borderTop: i === 0 ? "1.5px solid #82aaff" : "1.5px solid transparent" }}>
                  {t}
                  {i === 0 && fixed && <span style={{ width: 4, height: 4, borderRadius: 4, background: D.edText }} />}
                </span>
              ))}
            </div>
            <div style={{ padding: "5px 0", fontFamily: FONT_MONO, fontSize: 8.6, lineHeight: `${CODE_LINE_H}px` }}>
              {code.map((toks, i) => {
                const changed = fixed && i >= POOL_START && i < POOL_END;
                const background = changed
                  ? "rgb(126 231 135 / 14%)"
                  : highlightLine === i
                    ? "rgb(255 123 114 / 14%)"
                    : "transparent";
                return (
                  <div key={i} style={{ display: "flex", background, height: CODE_LINE_H }}>
                    <span style={{ width: 24, textAlign: "right", paddingRight: 8, color: D.edMuted }}>{i + 78}</span>
                    <span style={{ whiteSpace: "pre" }}>
                      {toks.map(([t, c], j) => (
                        <span key={j} style={{ color: c }}>
                          {t}
                        </span>
                      ))}
                    </span>
                  </div>
                );
              })}
            </div>
          </div>
        </div>
        {termH > 0 && <TerminalPanel lines={termLines} height={termH} />}
      </div>
    </AppWindow>
  );
};
