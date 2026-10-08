import type { ReactNode } from "react";
import { TIcon } from "../lib/textosIcons";
import { C, FONT_SANS } from "../lib/theme";
import { seg } from "../lib/time";

/**
 * Copia adaptada de `lib/TextosPanel.tsx` para la historia: mismas medidas y colores,
 * pero con los textos guardados de la historia y el tablero vacío (aún no se ha
 * volteado ninguna ventana). Se quitan la pestaña Notas (solo etiqueta) y el bloc.
 */
export const STORY_TEXTOS_W = 312;
export const STORY_TEXTOS_H = 372;

export type StoryTextosTab = "board" | "snippets";
export type StoryTabSwitch = { at: number; tab: StoryTextosTab };

const TAB_LABEL: Record<StoryTextosTab | "notes", string> = {
  board: "Tablero",
  snippets: "Textos",
  notes: "Notas",
};

/** Tokens --rb-* que usan SnippetsList y FlipPagesList. */
const RB = {
  bg0: "#121211",
  muted: "#9a9a90",
  text: "#f0f0ea",
};

/** Textos guardados (el segundo es el de la historia; el resto, inventados y coherentes). */
export const STORY_SNIPPETS = [
  { name: "Firma correo", alias: ["firma", "sig"], body: "Saludos,\nCamila Rojas\nEquipo de plataforma" },
  {
    name: "Respuesta incidente",
    alias: ["incidente"],
    body: "Ya lo detectamos: es un problema en la base de datos y lo estamos corrigiendo. Te aviso en 10 minutos.",
  },
  { name: "Link de estado", alias: ["estado"], body: "https://estado.tienda.cl" },
  { name: "Plantilla postmortem", alias: ["postmortem"], body: "Qué pasó: …\nCausa: …\nSolución: …" },
  { name: "Dirección oficina", alias: ["oficina"], body: "Av. Apoquindo 3000, piso 12, Las Condes" },
];

/** Transición Svelte `tabPanel`: opacity t, translateY(u·4px), 125 ms ease-in-out cuadrático. */
const inOut = (k: number) => (k < 0.5 ? 2 * k * k : 1 - Math.pow(-2 * k + 2, 2) / 2);
const TAB_MS = 125;

const mixHex = (a: string, b: string, t: number) => {
  const pa = [1, 3, 5].map((i) => parseInt(a.slice(i, i + 2), 16));
  const pb = [1, 3, 5].map((i) => parseInt(b.slice(i, i + 2), 16));
  return `rgb(${pa.map((v, i) => Math.round(v + (pb[i] - v) * t)).join(" ")})`;
};

const firstLine = (s: string) => s.split("\n").find((l) => l.trim() !== "") ?? "";

/** Tablero sin páginas todavía (texto de FlipPagesList vacío). */
const BoardPane = () => (
  <div style={{ padding: "12px 8px", textAlign: "center", fontSize: 12, lineHeight: 1.4, color: RB.muted }}>
    El tablero está vacío. Voltea una ventana para empezar a anotar.
  </div>
);

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
      {STORY_SNIPPETS.map((s, i) => (
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

export type StoryTextosPanelProps = {
  ms: number;
  /** Cambios de pestaña; la primera entrada define la pestaña inicial. */
  switches: StoryTabSwitch[];
  pasteIndex: number;
  pastedAt: number;
};

/** Contenido del `.sf` de SnippetsFloat: cabecera con pestañas y un panel por pestaña. */
export const StoryTextosPanel = ({ ms, switches, pasteIndex, pastedAt }: StoryTextosPanelProps) => {
  let cur = 0;
  for (let i = 0; i < switches.length; i++) if (ms >= switches[i].at) cur = i;
  const current = switches[cur];
  const previous = cur > 0 ? switches[cur - 1] : null;
  const k = cur > 0 ? seg(ms, current.at, TAB_MS) : 1;

  const tabOn = (tab: StoryTextosTab | "notes") => {
    if (tab === current.tab) return cur > 0 ? seg(ms, current.at, 75) : 1;
    if (previous && tab === previous.tab) return 1 - seg(ms, current.at, 75);
    return 0;
  };

  const pane = (tab: StoryTextosTab): ReactNode =>
    tab === "board" ? <BoardPane /> : <SnippetsPane pasteIndex={pasteIndex} pastedAt={pastedAt} ms={ms} />;

  const layers: { tab: StoryTextosTab; t: number }[] = [];
  if (previous && k < 1) layers.push({ tab: previous.tab, t: 1 - inOut(k) });
  layers.push({ tab: current.tab, t: inOut(k) });

  return (
    <div
      style={{
        width: STORY_TEXTOS_W,
        height: STORY_TEXTOS_H,
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
          {(["board", "snippets", "notes"] as const).map((tab) => (
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
