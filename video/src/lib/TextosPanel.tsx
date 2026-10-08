import type { ReactNode } from "react";
import { C, FONT_SANS } from "./theme";
import { seg } from "./time";
import { typed } from "./ui";
import { TIcon } from "./textosIcons";

/** Panel fijo del float de Textos (panel_float.rs PANEL_SHAPE). */
export const TEXTOS_W = 312;
export const TEXTOS_H = 372;

export type TextosTab = "board" | "snippets" | "notes";
export type TabSwitch = { at: number; tab: TextosTab };

const TAB_LABEL: Record<TextosTab, string> = {
  board: "Tablero",
  snippets: "Textos",
  notes: "Notas",
};

/** Tokens --rb-* que usan SnippetsList y FlipPagesList. */
const RB = {
  bg0: "#121211",
  muted: "#9a9a90",
  text: "#f0f0ea",
  hairline: "rgb(240 240 234 / 12%)",
  miniBg: "#2a2a26",
};

export const SNIPPETS = [
  { name: "Firma correo", alias: ["firma", "sig"], body: "Saludos cordiales,\nCamila Rojas\nIngeniera de proyectos" },
  { name: "Respuesta soporte", alias: ["soporte"], body: "Hola, gracias por escribirnos. Ya estamos revisando tu caso y te respondemos hoy mismo." },
  { name: "Link agenda", alias: ["agenda"], body: "https://agenda.ejemplo.cl/camila/30min" },
  { name: "Dirección oficina", alias: ["oficina"], body: "Av. Apoquindo 3000, piso 12, Las Condes" },
  { name: "Plantilla minuta", alias: ["minuta"], body: "Asistentes: …\nAcuerdos: …\nPróximos pasos: …" },
];

export const NOTES_TEXT =
  "Llamar a proveedor de sensores\nCambiar la clave del wifi de la planta\n- pedir cotización 2 repuestos";

/** Transición Svelte `tabPanel`: opacity t, translateY(u·4px), 125 ms ease-in-out cuadrático. */
const inOut = (k: number) => (k < 0.5 ? 2 * k * k : 1 - Math.pow(-2 * k + 2, 2) / 2);
const TAB_MS = 125;

const mixHex = (a: string, b: string, t: number) => {
  const pa = [1, 3, 5].map((i) => parseInt(a.slice(i, i + 2), 16));
  const pb = [1, 3, 5].map((i) => parseInt(b.slice(i, i + 2), 16));
  return `rgb(${pa.map((v, i) => Math.round(v + (pb[i] - v) * t)).join(" ")})`;
};

/** Miniatura de una página (mismo dibujo que la tira del Flipboard), viewBox = papel 1200×900. */
const bars = (x: number, y: number, w: number, lines: number, key: string) =>
  Array.from({ length: lines }, (_, i) => (
    <rect
      key={`${key}${i}`}
      x={x + 36}
      y={y + 54 + i * 62}
      width={(w - 72) * (i === lines - 1 ? 0.55 : 1)}
      height={30}
      rx={12}
      fill="rgb(240 240 234 / 38%)"
    />
  ));

const PageMini = ({ kind }: { kind: "full" | "text" | "blank" }) => (
  <div
    style={{
      width: 64,
      height: 48,
      boxSizing: "border-box",
      border: `1px solid ${RB.hairline}`,
      borderRadius: 5,
      background: RB.miniBg,
      overflow: "hidden",
      flex: "none",
    }}
  >
    <svg width={62} height={46} viewBox="0 0 1200 900" style={{ display: "block" }}>
      {kind === "full" && (
        <>
          {bars(24, 24, 280, 2, "a")}
          {/* lista */}
          {[0, 1, 2].map((i) => (
            <g key={i}>
              <rect x={60} y={190 + i * 64} width={38} height={38} rx={8} fill="rgb(240 240 234 / 38%)" />
              <rect x={122} y={196 + i * 64} width={i === 1 ? 110 : 150} height={26} rx={12} fill="rgb(240 240 234 / 38%)" />
            </g>
          ))}
          {bars(340, 24, 280, 3, "b")}
          {/* captura pegada */}
          <rect x={340} y={200} width={520} height={330} rx={16} fill="rgb(143 169 184 / 55%)" />
          <path d="M370 500 L470 420 L540 460 L640 360 L740 430 L830 380" stroke="rgb(240 240 234 / 70%)" strokeWidth={22} fill="none" strokeLinejoin="round" strokeLinecap="round" />
          {/* tinta */}
          <ellipse cx={480} cy={90} rx={210} ry={100} stroke="#e5483f" strokeWidth={22} fill="none" />
        </>
      )}
      {kind === "text" && <>{bars(120, 120, 300, 3, "c")}</>}
    </svg>
  </div>
);

const PAGES: { title: string; line: string | null; kind: "full" | "text" | "blank" }[] = [
  { title: "Página 1", line: "Revisión semanal — planta Sur", kind: "full" },
  { title: "Página 2", line: "Llamar a proveedor de sensores", kind: "text" },
  { title: "Página 3", line: null, kind: "blank" },
];

const BoardPane = ({ hoverIndex }: { hoverIndex: number | null }) => (
  <div style={{ padding: 6, display: "flex", flexDirection: "column" }}>
    {PAGES.map((p, i) => (
      <div
        key={p.title}
        style={{
          display: "flex",
          alignItems: "center",
          gap: 10,
          padding: 6,
          borderRadius: 8,
          background: hoverIndex === i ? RB.bg0 : "transparent",
        }}
      >
        <PageMini kind={p.kind} />
        <div style={{ minWidth: 0, display: "grid", gap: 2 }}>
          <div style={{ fontSize: 12, fontWeight: 600, color: RB.text, lineHeight: 1.25 }}>{p.title}</div>
          <div
            style={{
              fontSize: 11,
              color: RB.muted,
              lineHeight: 1.25,
              whiteSpace: "nowrap",
              overflow: "hidden",
              textOverflow: "ellipsis",
            }}
          >
            {p.line ?? "Página en blanco"}
          </div>
        </div>
      </div>
    ))}
  </div>
);

const firstLine = (s: string) => s.split("\n").find((l) => l.trim() !== "") ?? "";

const SnippetsPane = ({ pasteIndex, pastedAt, ms }: { pasteIndex: number; pastedAt: number; ms: number }) => (
  <div style={{ display: "flex", flexDirection: "column", gap: 4.8, height: "100%", overflow: "hidden" }}>
    <div style={{ padding: "0 2.4px" }}>
      <div
        style={{
          height: 25.6,
          borderRadius: 999,
          padding: "0 7.2px",
          background: "rgb(240 240 234 / 7%)",
          display: "flex",
          alignItems: "center",
          gap: 5.6,
          color: RB.muted,
        }}
      >
        <TIcon name="search" size={12} />
        <span style={{ fontSize: 10, color: "#a9a9a9" }}>Buscar por nombre o palabra…</span>
      </div>
    </div>
    <div style={{ display: "flex", flexDirection: "column", gap: 2.4, overflow: "hidden" }}>
      {SNIPPETS.map((s, i) => (
        <div
          key={s.name}
          style={{
            display: "flex",
            flexDirection: "column",
            gap: 2.4,
            borderRadius: 7.2,
            padding: "4.8px 6.4px",
            background: RB.bg0,
            color: RB.text,
            opacity: i === pasteIndex && ms >= pastedAt ? 0.6 : 1,
            flex: "none",
          }}
        >
          <span style={{ fontSize: 12, fontWeight: 650, lineHeight: 1.3 }}>{s.name}</span>
          <span style={{ fontSize: 11, lineHeight: 1.3, color: RB.text }}>{s.alias.join(" · ")}</span>
          <span
            style={{
              fontSize: 11,
              lineHeight: 1.3,
              color: RB.muted,
              whiteSpace: "nowrap",
              overflow: "hidden",
              textOverflow: "ellipsis",
            }}
          >
            {firstLine(s.body)}
          </span>
        </div>
      ))}
    </div>
  </div>
);

const NotesPane = ({ ms, typeAt, msPerChar }: { ms: number; typeAt: number; msPerChar: number }) => {
  const text = ms >= typeAt ? typed(NOTES_TEXT, ms, typeAt, msPerChar) : "";
  const caret = Math.floor(ms / 530) % 2 === 0 || text.length < NOTES_TEXT.length;
  return (
    <div
      style={{
        height: "100%",
        boxSizing: "border-box",
        borderRadius: 7.2,
        padding: "6.4px 8px",
        background: "rgb(18 18 17 / 80%)",
        color: C.text,
        fontSize: 12,
        lineHeight: 1.45,
        whiteSpace: "pre-wrap",
        overflow: "hidden",
      }}
    >
      {text === "" ? (
        <span style={{ color: "#a9a9a9" }}>Notas temporales…</span>
      ) : (
        <>
          {text}
          <span
            style={{
              display: "inline-block",
              width: 1,
              height: 13,
              marginLeft: 0.5,
              verticalAlign: "-2px",
              background: C.text,
              opacity: caret ? 1 : 0,
            }}
          />
        </>
      )}
    </div>
  );
};

const HeadTab = ({ label, on }: { label: string; on: number }) => (
  <div
    style={{
      minHeight: 28,
      boxSizing: "border-box",
      display: "flex",
      alignItems: "center",
      padding: "3.2px 8.8px",
      borderRadius: 999,
      fontSize: 11,
      fontWeight: 600,
      color: mixHex(C.muted, C.accent, on),
      background: `rgb(232 232 224 / ${(0.12 * on).toFixed(3)})`,
    }}
  >
    {label}
  </div>
);

export type TextosPanelProps = {
  ms: number;
  /** Cambios de pestaña; la primera entrada define la pestaña inicial. */
  switches: TabSwitch[];
  pasteIndex?: number;
  pastedAt?: number;
  notesTypeAt?: number;
  notesMsPerChar?: number;
  /** Fila de Tablero resaltada (hover) desde este instante. */
  hoverPage?: { index: number; fromMs: number };
};

/** Contenido del `.sf` de SnippetsFloat: cabecera con pestañas y un panel por pestaña. */
export const TextosPanel = ({
  ms,
  switches,
  pasteIndex = 1,
  pastedAt = Infinity,
  notesTypeAt = Infinity,
  notesMsPerChar = 12,
  hoverPage,
}: TextosPanelProps) => {
  let cur = 0;
  for (let i = 0; i < switches.length; i++) if (ms >= switches[i].at) cur = i;
  const current = switches[cur];
  const previous = cur > 0 ? switches[cur - 1] : null;
  const k = cur > 0 ? seg(ms, current.at, TAB_MS) : 1;

  const tabOn = (tab: TextosTab) => {
    if (tab === current.tab) return cur > 0 ? seg(ms, current.at, 75) : 1;
    if (previous && tab === previous.tab) return 1 - seg(ms, current.at, 75);
    return 0;
  };

  const pane = (tab: TextosTab): ReactNode => {
    if (tab === "board") {
      const h = hoverPage && ms >= hoverPage.fromMs ? hoverPage.index : null;
      return <BoardPane hoverIndex={h} />;
    }
    if (tab === "snippets") return <SnippetsPane pasteIndex={pasteIndex} pastedAt={pastedAt} ms={ms} />;
    return <NotesPane ms={ms} typeAt={notesTypeAt} msPerChar={notesMsPerChar} />;
  };

  const layers: { tab: TextosTab; t: number }[] = [];
  if (previous && k < 1) layers.push({ tab: previous.tab, t: 1 - inOut(k) });
  layers.push({ tab: current.tab, t: inOut(k) });

  return (
    <div
      style={{
        width: TEXTOS_W,
        height: TEXTOS_H,
        boxSizing: "border-box",
        padding: "7.2px 8px 8.8px",
        display: "flex",
        flexDirection: "column",
        color: C.text,
        fontFamily: FONT_SANS,
      }}
    >
      <div style={{ display: "flex", alignItems: "center", minHeight: 32, gap: 5.6, marginBottom: 5.6 }}>
        <div style={{ display: "flex", gap: 4.8 }}>
          {(["board", "snippets", "notes"] as TextosTab[]).map((tab) => (
            <HeadTab key={tab} label={TAB_LABEL[tab]} on={tabOn(tab)} />
          ))}
        </div>
        <div style={{ flex: 1 }} />
        <div style={{ display: "flex", gap: 2.4, color: C.faint }}>
          {(
            [
              ["pin", 13],
              ["panelTopClose", 14],
              ["x", 14],
            ] as const
          ).map(([name, size]) => (
            <div key={name} style={{ width: 28, height: 28, borderRadius: 6.4, display: "grid", placeItems: "center" }}>
              <TIcon name={name} size={size} />
            </div>
          ))}
        </div>
      </div>
      <div style={{ position: "relative", flex: 1, minHeight: 0 }}>
        {layers.map(({ tab, t }) => (
          <div
            key={tab}
            style={{
              position: "absolute",
              inset: 0,
              opacity: t,
              transform: `translateY(${(1 - t) * 4}px)`,
            }}
          >
            {pane(tab)}
          </div>
        ))}
      </div>
    </div>
  );
};
