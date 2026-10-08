import type { CSSProperties } from "react";
import { AgentLogo, UiIcon, type AgentId } from "./agentesIcons";
import { AgentesTerminal } from "./AgentesTerminal";
import { PROMPT, T, statusAt, type AgentStatus } from "./agentesTimeline";
import { C, EASE, FONT_SANS } from "./theme";
import { seg } from "./time";
import { typed } from "./ui";

// Tokens legado --rb-* (tema oscuro) que usa casi todo lo de agentes.
const RB = {
  surface: "#1e1e1b",
  sidebar: "#171714",
  muted: "#9a9a90",
  faint: "#6e6e66",
  text: "#f0f0ea",
  ring: "rgba(240,240,234,.08)",
};

const backOut = (t: number) => {
  const s = 1.70158;
  const u = t - 1;
  return u * u * ((s + 1) * u + s) + 1;
};

/** ConsoleStateDot.svelte: punto de 6 px + texto de 11 px. */
export const StateDot = ({ status, ms }: { status: AgentStatus; ms: number }) => {
  const pulse = 0.675 + 0.325 * Math.cos((ms / 1200) * Math.PI * 2);
  const dot: CSSProperties = { width: 6, height: 6, borderRadius: 6, flex: "none" };
  const label = (text: string, color: string) => (
    <span style={{ fontSize: 11, color, whiteSpace: "nowrap" }}>{text}</span>
  );
  return (
    <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
      {status === "idle" && <span style={{ ...dot, background: "rgba(240,240,234,.22)" }} />}
      {status === "working" && (
        <>
          <span style={{ ...dot, background: C.accent, opacity: pulse }} />
          {label("Trabajando…", RB.muted)}
        </>
      )}
      {status === "waiting" && (
        <>
          <span style={{ ...dot, background: C.accent, boxShadow: "0 0 0 3px rgba(232,232,224,.30)" }} />
          {label("Espera tu permiso", RB.text)}
        </>
      )}
      {status === "unread" && (
        <>
          <span style={{ ...dot, background: C.ok }} />
          {label("Respondió", RB.text)}
        </>
      )}
    </span>
  );
};

const WindowButtons = () => (
  <div style={{ marginLeft: "auto", display: "flex", height: "100%", color: "#d8d8d2" }}>
    {["min", "max", "close"].map((k) => (
      <div key={k} style={{ width: 46, height: "100%", display: "grid", placeItems: "center" }}>
        <svg width={10} height={10} viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth={1}>
          {k === "min" && <path d="M0 5.5h10" />}
          {k === "max" && <rect x={0.5} y={0.5} width={9} height={9} />}
          {k === "close" && <path d="M0 0l10 10M10 0L0 10" />}
        </svg>
      </div>
    ))}
  </div>
);

const AGENT_CARDS: { id: AgentId | "terminal"; name: string }[] = [
  { id: "claude", name: "Claude Code" },
  { id: "opencode", name: "OpenCode" },
  { id: "codex", name: "Codex" },
  { id: "cursor", name: "Cursor" },
  { id: "terminal", name: "Terminal" },
];

/** Estado vacío de la pizarra (BOARD:1612-1680). Tarjeta de Claude Code centrada en (320, 418). */
const EmptyState = ({ ms }: { ms: number }) => {
  const hover = seg(ms, T.click - 220, 150);
  const press = ms >= T.click && ms < T.click + 90 ? 0.96 : 1;
  return (
    <div
      style={{
        position: "absolute",
        left: 264,
        top: 265,
        width: 592,
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        opacity: 1 - seg(ms, T.click + 60, 200),
      }}
    >
      <div style={{ height: 24, fontSize: 18, fontWeight: 600, lineHeight: "24px", color: RB.text }}>
        La pizarra está vacía
      </div>
      <div style={{ height: 19, marginTop: 6, fontSize: 13, lineHeight: "19px", color: RB.muted }}>
        Elige la carpeta y cuántas consolas, y toca el agente para abrirlas.
      </div>
      <div style={{ display: "flex", gap: 8, marginTop: 16, height: 34 }}>
        <div
          style={{
            height: 34,
            borderRadius: 10,
            padding: "0 10px",
            display: "flex",
            alignItems: "center",
            gap: 8,
            background: RB.surface,
            boxShadow: `inset 0 0 0 1px ${RB.ring}`,
            color: RB.muted,
          }}
        >
          <UiIcon name="folder" size={15} />
          <span style={{ fontSize: 13, fontWeight: 600, color: RB.text }}>atic</span>
          <span style={{ fontSize: 11.5, color: RB.faint }}>~/proyectos/atic</span>
          <UiIcon name="chevronRight" size={13} />
        </div>
        <div
          style={{
            height: 34,
            borderRadius: 10,
            padding: 3,
            display: "flex",
            alignItems: "center",
            background: RB.surface,
            boxShadow: `inset 0 0 0 1px ${RB.ring}`,
            color: RB.muted,
          }}
        >
          <div style={{ width: 28, display: "grid", placeItems: "center" }}>
            <UiIcon name="minus" size={13} />
          </div>
          <span style={{ minWidth: 76, textAlign: "center", fontSize: 12.5, fontWeight: 600, color: RB.text }}>
            1 consola
          </span>
          <div style={{ width: 28, display: "grid", placeItems: "center" }}>
            <UiIcon name="plus" size={13} />
          </div>
        </div>
      </div>
      <div style={{ display: "flex", gap: 8, marginTop: 16, height: 76 }}>
        {AGENT_CARDS.map((a, i) => {
          const isTarget = i === 0;
          return (
            <div
              key={a.id}
              style={{
                boxSizing: "border-box",
                width: 112,
                height: 76,
                borderRadius: 14,
                padding: "14px 8px",
                display: "flex",
                flexDirection: "column",
                alignItems: "center",
                gap: 8,
                background: RB.surface,
                boxShadow: isTarget
                  ? `inset 0 0 0 1px rgba(232,232,224,${0.08 + 0.42 * hover})`
                  : `inset 0 0 0 1px ${RB.ring}`,
                transform: isTarget ? `scale(${press})` : undefined,
                color: a.id === "terminal" ? RB.muted : RB.text,
              }}
            >
              {a.id === "terminal" ? (
                <UiIcon name="squareTerminal" size={22} />
              ) : (
                <AgentLogo agent={a.id} size={22} color={RB.text} />
              )}
              <span style={{ fontSize: 12.5, fontWeight: 500, lineHeight: "16px" }}>{a.name}</span>
            </div>
          );
        })}
      </div>
    </div>
  );
};

/** Lista lateral flotante (BLIST): 232 px, alineada abajo (ancho < 1200 ⇒ bottom 88). */
const SideList = ({ ms, status }: { ms: number; status: AgentStatus }) => {
  const rowT = EASE.smoothOut(seg(ms, T.cardBorn + 80, 260));
  const treeH = (24 + 30 + 8) * rowT;
  return (
    <div
      style={{
        position: "absolute",
        left: 12,
        bottom: 88,
        width: 232,
        boxSizing: "border-box",
        padding: 8,
        borderRadius: 16,
        display: "flex",
        flexDirection: "column",
        gap: 8,
        background: "rgba(30,30,27,.82)",
        boxShadow: "0 0 0 1px rgba(240,240,234,.10), 0 18px 40px -18px rgb(0 0 0 / 55%)",
        backdropFilter: "blur(18px) saturate(1.2)",
        color: RB.text,
      }}
    >
      <div style={{ display: "flex", gap: 6 }}>
        <div
          style={{
            flex: 1,
            height: 34,
            borderRadius: 8,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            gap: 6,
            background: "rgba(240,240,234,.07)",
            fontSize: 13,
            fontWeight: 600,
          }}
        >
          <UiIcon name="plus" size={14} />
          Nueva consola
        </div>
        <div style={{ width: 34, height: 34, display: "grid", placeItems: "center", color: RB.muted }}>
          <UiIcon name="panelLeftClose" size={15} />
        </div>
      </div>

      <div style={{ height: treeH, overflow: "hidden", opacity: rowT, marginBottom: treeH ? 0 : -8 }}>
        <div
          style={{
            height: 24,
            padding: "0 8px",
            fontSize: 11,
            fontWeight: 600,
            letterSpacing: "0.02em",
            textTransform: "uppercase",
            lineHeight: "24px",
            color: RB.faint,
          }}
        >
          Consolas abiertas
        </div>
        <div
          style={{
            height: 30,
            borderRadius: 10,
            padding: "0 8px",
            display: "flex",
            alignItems: "center",
            gap: 8,
            background: "rgba(240,240,234,.10)",
          }}
        >
          <AgentLogo agent="claude" size={15} />
          <span style={{ fontSize: 12.5, fontWeight: 500, whiteSpace: "nowrap" }}>Claude Code</span>
          <span style={{ marginLeft: "auto" }}>
            <StateDot status={status} ms={ms} />
          </span>
        </div>
      </div>

      <div
        style={{
          borderTop: "1px solid rgba(240,240,234,.07)",
          paddingTop: 8,
          display: "flex",
          alignItems: "center",
          gap: 6,
        }}
      >
        <div
          style={{
            flex: 1,
            height: 30,
            borderRadius: 8,
            padding: "0 8px",
            display: "flex",
            alignItems: "center",
            gap: 6,
            fontSize: 12,
            color: RB.muted,
          }}
        >
          <UiIcon name="folder" size={13} />
          atic
        </div>
        <div style={{ width: 30, height: 30, display: "grid", placeItems: "center", color: RB.muted }}>
          <UiIcon name="settings" size={15} />
        </div>
      </div>
    </div>
  );
};

/** BoardZoom: barra quieta (opacidad .4) arriba a la derecha. */
const ZoomBar = () => {
  const btn: CSSProperties = { width: 30, height: 30, display: "grid", placeItems: "center", color: RB.muted };
  return (
    <div
      style={{
        position: "absolute",
        top: 12,
        right: 12,
        display: "flex",
        alignItems: "center",
        gap: 2,
        padding: 4,
        borderRadius: 12,
        background: "rgba(30,30,27,.82)",
        boxShadow: "0 0 0 1px rgba(240,240,234,.10)",
        opacity: 0.4,
      }}
    >
      <div style={btn}>
        <UiIcon name="minus" size={14} />
      </div>
      <div style={{ minWidth: 46, textAlign: "center", fontSize: 11.5, color: RB.text }}>100%</div>
      <div style={btn}>
        <UiIcon name="plus" size={14} />
      </div>
      <div style={{ width: 1, height: 16, background: "rgba(240,240,234,.10)", margin: "0 2px" }} />
      <div style={btn}>
        <UiIcon name="maximize" size={14} />
      </div>
    </div>
  );
};

/** Tarjeta de consola (BoardCard): 760×480 en (302,120). */
const ConsoleCard = ({ ms, status }: { ms: number; status: AgentStatus }) => {
  const t = backOut(seg(ms, T.cardBorn, 320));
  const u = 1 - t;
  const ping = seg(ms, T.ready, 1100);
  const pingEase = EASE.smoothOut(ping);
  const ring = "0 0 0 1.5px rgba(232,232,224,.70), 0 24px 60px -18px rgb(0 0 0 / 65%)";
  const pingRing =
    ping > 0 && ping < 1
      ? `, 0 0 0 ${2 + 16 * pingEase}px rgba(232,232,224,${0.85 * (1 - pingEase)})`
      : "";
  const action = (icon: "forward" | "paperclip" | "maximize2" | "x", size: number) => (
    <div style={{ width: 24, height: 24, display: "grid", placeItems: "center", color: RB.faint }}>
      <UiIcon name={icon} size={size} />
    </div>
  );
  return (
    <div
      style={{
        position: "absolute",
        left: 302,
        top: 120,
        width: 760,
        height: 480,
        borderRadius: 12,
        overflow: "hidden",
        display: "flex",
        flexDirection: "column",
        background: C.bg,
        boxShadow: ring + pingRing,
        opacity: Math.min(1, t * 1.8),
        transform: `translateY(${u * 6}px) scale(${0.94 + 0.06 * t})`,
        transformOrigin: "50% 60%",
      }}
    >
      <div
        style={{
          height: 32,
          flex: "none",
          display: "flex",
          alignItems: "center",
          gap: 8,
          padding: "0 6px 0 12px",
          background: RB.surface,
          color: RB.text,
          fontSize: 12,
        }}
      >
        <AgentLogo agent="claude" size={14} />
        <span style={{ fontWeight: 560 }}>Claude Code</span>
        <StateDot status={status} ms={ms} />
        <span style={{ flex: 1 }} />
        {action("forward", 12)}
        {action("paperclip", 12)}
        {action("maximize2", 12)}
        {action("x", 13)}
      </div>
      <div style={{ position: "relative", flex: 1, minHeight: 0 }}>
        <AgentesTerminal ms={ms} />
      </div>
    </div>
  );
};

/** BoardComposer: 420 px en reposo, 680 px con foco (340 ms, rebote de isla). */
const Composer = ({ ms }: { ms: number }) => {
  const hasTarget = ms >= T.cardBorn + 50;
  const widen = EASE.island(seg(ms, T.composerFocus, 340));
  const W = 420 + (680 - 420) * widen;
  const focused = ms >= T.composerFocus;
  const text = ms >= T.send ? "" : typed(PROMPT, ms, T.typeStart, T.typeMsPerChar);
  const hasText = text.length > 0 && ms >= T.typeStart;
  const enabledAt = T.typeStart;
  const pop = hasText ? EASE.island(seg(ms, enabledAt, 320)) : 1;
  const sendScale = hasText ? 0.72 + 0.28 * pop : 1;
  const placeholder = hasTarget ? "Escríbele a Claude Code…" : "Toca una consola para escribirle";

  return (
    <div
      style={{
        position: "absolute",
        left: 560 - W / 2,
        bottom: 16 + (focused ? 2 * widen : 0),
        width: W,
        boxSizing: "border-box",
        display: "flex",
        alignItems: "flex-end",
        gap: 8,
        padding: "8px 8px 8px 10px",
        borderRadius: 18,
        background: "rgba(30,30,27,.82)",
        boxShadow: focused
          ? "0 0 0 1px rgba(232,232,224,.55), 0 24px 48px -18px rgb(0 0 0 / 60%)"
          : "0 0 0 1px rgba(240,240,234,.11), 0 18px 40px -16px rgb(0 0 0 / 55%)",
        backdropFilter: "blur(18px) saturate(1.2)",
      }}
    >
      {hasTarget && (
        <div
          style={{
            height: 28,
            flex: "none",
            padding: "0 10px 0 8px",
            borderRadius: 999,
            display: "flex",
            alignItems: "center",
            gap: 6,
            background: "rgba(240,240,234,.07)",
            fontSize: 12,
            fontWeight: 560,
            color: RB.text,
          }}
        >
          <AgentLogo agent="claude" size={13} />
          Claude Code
        </div>
      )}
      <div
        style={{
          flex: 1,
          minWidth: 0,
          minHeight: 30,
          display: "flex",
          alignItems: "center",
          fontSize: 13.5,
          lineHeight: 1.45,
          whiteSpace: "nowrap",
          color: hasText ? RB.text : RB.faint,
        }}
      >
        {hasText ? text : placeholder}
        {focused && hasText && (
          <span style={{ width: 1, height: 16, background: RB.text, marginLeft: 1, opacity: Math.floor(ms / 530) % 2 ? 0 : 1 }} />
        )}
      </div>
      <div
        style={{
          width: 30,
          height: 30,
          flex: "none",
          borderRadius: "50%",
          display: "grid",
          placeItems: "center",
          background: hasText ? C.accent : "rgba(240,240,234,.12)",
          color: hasText ? C.onAccent : RB.muted,
          transform: `scale(${sendScale})`,
        }}
      >
        <UiIcon name="arrowUp" size={15} strokeWidth={2} />
      </div>
    </div>
  );
};

/** Chispa de envío (BOARD:865-895): del botón a la tarjeta en 520 ms. */
const Spark = ({ ms }: { ms: number }) => {
  const p = seg(ms, T.send, 520);
  if (p <= 0 || p >= 1) return null;
  const k = EASE.smoothOut(p);
  const x = 877 + (682 - 877) * k;
  const y = 721 + (360 - 721) * k - 40 * Math.sin(Math.PI * k);
  const s = 1.1 + (0.4 - 1.1) * k;
  return (
    <div
      style={{
        position: "absolute",
        left: x - 5,
        top: y - 5,
        width: 10,
        height: 10,
        borderRadius: "50%",
        background: C.accent,
        boxShadow: "0 0 10px 2px rgba(232,232,224,.6)",
        opacity: Math.sin(Math.PI * p),
        transform: `scale(${s})`,
      }}
    />
  );
};

/** Ventana del SO «Consolas de agentes» (1120×760 + barra de título aproximada). */
export const AgentsWindow = ({ ms }: { ms: number }) => {
  const open = EASE.smoothOut(seg(ms, T.winOpen, 300));
  const status = statusAt(ms);
  return (
    <div
      style={{
        position: "absolute",
        left: 0,
        top: 0,
        width: 1120,
        height: 792,
        borderRadius: 8,
        overflow: "hidden",
        background: C.bg,
        boxShadow: "0 0 0 1px rgba(255,255,255,.14), 0 30px 80px rgb(0 0 0 / 50%)",
        fontFamily: FONT_SANS,
        color: RB.text,
        opacity: open,
        transform: `scale(${0.96 + 0.04 * open})`,
        transformOrigin: "50% 40%",
      }}
    >
      {/* barra de título nativa (aproximada; la real depende del SO) */}
      <div
        style={{
          height: 32,
          display: "flex",
          alignItems: "center",
          padding: "0 0 0 12px",
          background: "#1b1b1a",
          color: "#d8d8d2",
          fontSize: 12,
        }}
      >
        Consolas de agentes
        <WindowButtons />
      </div>

      {/* pizarra */}
      <div
        style={{
          position: "absolute",
          left: 0,
          top: 32,
          width: 1120,
          height: 760,
          overflow: "hidden",
          backgroundColor: C.bg,
          backgroundImage:
            "radial-gradient(rgba(240,240,234,.13) 1px, transparent 1.4px)",
          backgroundSize: "24px 24px",
        }}
      >
        {ms < T.click + 300 && <EmptyState ms={ms} />}
        {ms >= T.cardBorn && <ConsoleCard ms={ms} status={status} />}
        <SideList ms={ms} status={status} />
        <ZoomBar />
        <Composer ms={ms} />
        <Spark ms={ms} />
      </div>
    </div>
  );
};
