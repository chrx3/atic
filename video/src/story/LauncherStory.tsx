import { Easing } from "remotion";
import { Icon, type IconName } from "../lib/Icon";
import { LF } from "../lib/Launcher";
import { C, EASE, FONT_SANS, lerp } from "../lib/theme";
import { seg } from "../lib/time";
import { Kbd, typed } from "../lib/ui";

/**
 * Launcher (Ctrl+Espacio) con los datos de la historia. Copia adaptada de `lib/Launcher.tsx`:
 * mismas medidas, colores y tiempos; cambian los recientes (Editor / Navegador / Mensajes),
 * la búsqueda («cor» → Correo) y se añade el cierre real (tuck de favoritos → recede).
 * Coordenadas del contenedor: (0,0) es la esquina superior izquierda de la barra.
 */

const RECENTS_EASE = Easing.bezier(0.45, 0, 0.2, 1);

type AppKind = "app-editor" | "app-browser" | "app-chat" | "app-mail" | "app-calc";

type Hit = {
  title: string;
  sub: string;
  icon: IconName | AppKind;
  action?: boolean;
};

const RECENTS: Hit[] = [
  { title: "Editor", sub: "Abierta hace 12 min", icon: "app-editor" },
  { title: "Navegador", sub: "Abierta hace 40 min", icon: "app-browser" },
  { title: "Mensajes", sub: "En uso", icon: "app-chat" },
];

/** Resultados de «cor»: prefijo (Correo), contiene (Elegir color) y subsecuencia (Calculadora). */
const RESULTS: Hit[] = [
  { title: "Correo", sub: "Aplicación", icon: "app-mail" },
  { title: "Elegir color", sub: "Cuentagotas: un píxel de la pantalla al portapapeles", icon: "color", action: true },
  { title: "Calculadora", sub: "Aplicación", icon: "app-calc" },
];

export const LAUNCHER_QUERY = "cor";

const FAVS: IconName[] = ["clipboard", "captures", "board"];

/** Cuánto tardan los favoritos en volver a la barra (último en salir = el primero en entrar). */
export const LAUNCHER_TUCK_MS = (FAVS.length - 1) * LF.favStaggerMs + LF.favMs;
/** El cuerpo de la barra se repliega a disco. */
export const LAUNCHER_RECEDE_MS = 320;

export type LauncherStoryTimeline = {
  /** Instante en que nace la gota. */
  bornMs: number;
  /** Empieza a escribirse la consulta. */
  typeMs: number;
  /** Enter sobre la fila seleccionada: arranca el cierre. */
  enterMs: number;
};

/** Instante en que el launcher desaparece del todo. */
export const launcherGoneMs = (tl: LauncherStoryTimeline) =>
  tl.enterMs + LAUNCHER_TUCK_MS + LAUNCHER_RECEDE_MS;

/** Iconos de app: sustitutos genéricos coherentes con los programas del escritorio simulado. */
const AppGlyph = ({ kind, size }: { kind: AppKind; size: number }) => {
  const r = 3.2;
  const base = { width: size, height: size, borderRadius: r, display: "grid", placeItems: "center" } as const;
  if (kind === "app-editor")
    return (
      <div style={{ ...base, background: "linear-gradient(160deg,#3a415c,#1b1d26)", boxShadow: "inset 0 0 0 1px rgba(255,255,255,.14)" }}>
        <svg width={size * 0.68} height={size * 0.68} viewBox="0 0 24 24" fill="none" stroke="#9fb3ff" strokeWidth={2.6} strokeLinecap="round" strokeLinejoin="round">
          <path d="m8 7-5 5 5 5" />
          <path d="m16 7 5 5-5 5" />
          <path d="m13.5 5-3 14" />
        </svg>
      </div>
    );
  if (kind === "app-browser")
    return (
      <div style={{ ...base, borderRadius: "50%", background: "radial-gradient(circle at 35% 30%, #6aa2ff, #2f6feb 70%)" }}>
        <svg width={size * 0.78} height={size * 0.78} viewBox="0 0 24 24" fill="none" stroke="#f4f7ff" strokeWidth={1.9} strokeLinecap="round">
          <circle cx="12" cy="12" r="8.5" />
          <ellipse cx="12" cy="12" rx="3.7" ry="8.5" />
          <path d="M3.5 12h17" />
        </svg>
      </div>
    );
  if (kind === "app-chat")
    return (
      <div style={{ ...base, background: "#6d4bd8" }}>
        <svg width={size * 0.66} height={size * 0.66} viewBox="0 0 24 24" fill="#f4f4f0">
          <path d="M6.5 4h11A2.5 2.5 0 0 1 20 6.5v7a2.5 2.5 0 0 1-2.5 2.5H12l-4.5 3.5V16h-1A2.5 2.5 0 0 1 4 13.5v-7A2.5 2.5 0 0 1 6.5 4Z" />
        </svg>
      </div>
    );
  if (kind === "app-mail")
    return (
      <div style={{ ...base, background: "linear-gradient(160deg,#5b9bff,#2f6feb)" }}>
        <svg width={size * 0.7} height={size * 0.7} viewBox="0 0 24 24" fill="none" stroke="#f4f7ff" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round">
          <rect x="3" y="5.5" width="18" height="13" rx="2.5" />
          <path d="m3.8 7.6 8.2 6 8.2-6" />
        </svg>
      </div>
    );
  return (
    <div style={{ ...base, background: "#56606f", color: "#f4f4f0" }}>
      <Icon name="calculator" size={size * 0.68} strokeWidth={1.9} />
    </div>
  );
};

const HitIcon = ({ hit, size, box }: { hit: Hit; size: number; box: number }) => (
  <div
    style={{
      width: box,
      height: box,
      borderRadius: 6.4,
      background: hit.action ? "rgba(111,175,136,.18)" : "rgba(240,240,234,.06)",
      color: hit.action ? C.ok : C.muted,
      display: "grid",
      placeItems: "center",
      flex: "none",
    }}
  >
    {hit.icon.startsWith("app-") ? (
      <AppGlyph kind={hit.icon as AppKind} size={size} />
    ) : (
      <Icon name={hit.icon as IconName} size={size} strokeWidth={1.7} />
    )}
  </div>
);

const Row = ({ hit, selected, pressed = false }: { hit: Hit; selected: boolean; pressed?: boolean }) => (
  <div
    style={{
      display: "flex",
      alignItems: "center",
      gap: 2.4,
      borderRadius: 8,
      background: selected ? "rgba(240,240,234,.05)" : "transparent",
      transform: pressed ? "scale(.985)" : "none",
    }}
  >
    <div style={{ display: "flex", alignItems: "center", gap: 11.2, padding: "7.2px 5.6px 7.2px 8.8px", flex: 1, minWidth: 0 }}>
      <HitIcon hit={hit} size={18} box={32} />
      <div style={{ minWidth: 0, lineHeight: 1.25 }}>
        <div style={{ fontSize: 14.4, fontWeight: 600, color: C.text, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
          {hit.title}
        </div>
        <div style={{ fontSize: 11.2, color: C.muted, marginTop: 0.8, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
          {hit.sub}
        </div>
      </div>
    </div>
    <div style={{ width: 40, height: 40, display: "grid", placeItems: "center", color: C.faint, opacity: selected ? 1 : 0.55, marginRight: 2.4 }}>
      <svg width={14} height={14} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.75} strokeLinecap="round" strokeLinejoin="round">
        <path d="M11.525 2.295a.53.53 0 0 1 .95 0l2.31 4.679a2.123 2.123 0 0 0 1.595 1.16l5.166.756a.53.53 0 0 1 .294.904l-3.736 3.638a2.123 2.123 0 0 0-.611 1.878l.882 5.14a.53.53 0 0 1-.771.56l-4.618-2.428a2.122 2.122 0 0 0-1.973 0L6.396 21.01a.53.53 0 0 1-.77-.56l.881-5.139a2.122 2.122 0 0 0-.611-1.879L2.16 9.795a.53.53 0 0 1 .294-.906l5.165-.755a2.122 2.122 0 0 0 1.597-1.16z" />
      </svg>
    </div>
  </div>
);

/**
 * Launcher: gota de 40 px → estirón a 324 px → favoritos → recientes → búsqueda → cierre
 * (favoritos vuelven, luego la barra se repliega a disco).
 */
export const LauncherStory = ({ ms, tl }: { ms: number; tl: LauncherStoryTimeline }) => {
  const b = tl.bornMs;
  const drop = EASE.smoothOut(seg(ms, b, LF.birthMs));
  const stretchStart = b + LF.birthMs + LF.beatMs;
  const stretch = EASE.liquid(seg(ms, stretchStart, LF.stretchMs));
  const readyAt = stretchStart + LF.stretchMs;
  const chrome = EASE.smoothOut(seg(ms, readyAt, LF.chromeMs));

  const recedeStart = tl.enterMs + LAUNCHER_TUCK_MS;
  const closing = ms >= recedeStart;
  const recede = EASE.liquid(seg(ms, recedeStart, LAUNCHER_RECEDE_MS));
  if (ms >= recedeStart + LAUNCHER_RECEDE_MS) return null;

  const w = lerp(lerp(LF.barH, LF.barW, stretch), LF.barH, recede);
  const left = (LF.barW - w) / 2;

  const query = closing ? "" : typed(LAUNCHER_QUERY, ms, tl.typeMs, 140);
  const hasQuery = query.length > 0;

  const recentsStart = readyAt + 500;
  const recentsH = 40 + 28 + RECENTS.length * 48 + 10;
  const grow = RECENTS_EASE(seg(ms, recentsStart, LF.recentsMs));

  let height = lerp(LF.barH, recentsH, grow);
  if (hasQuery) height = LF.expandedH;
  if (closing) height = LF.barH;
  const expanded = !closing && (hasQuery || grow > 0);

  const showRecents = !hasQuery && !closing && grow > 0;
  const enterT = ms - tl.enterMs;
  const pressed = enterT >= 0 && enterT < 140;

  return (
    <div
      style={{
        position: "absolute",
        left: 0,
        top: 0,
        width: LF.barW,
        height: LF.barH,
        opacity: drop * (1 - recede),
        transform: `scale(${lerp(0.82, 1, drop) * lerp(1, 0.82, recede)})`,
        fontFamily: FONT_SANS,
      }}
    >
      {/* panel (barra → panel) */}
      <div
        style={{
          position: "absolute",
          left,
          top: 0,
          width: w,
          height,
          boxSizing: "border-box",
          background: C.skin,
          borderRadius: expanded ? 18 : 999,
          overflow: expanded ? "visible" : "hidden",
          boxShadow: expanded
            ? "0 18px 48px rgba(240,240,234,.18), inset 0 0 0 1px rgba(240,240,234,.10)"
            : "none",
          filter: expanded ? "none" : "drop-shadow(0 10px 22px rgb(0 0 0 / 38%))",
          color: C.text,
        }}
      >
        <div style={{ opacity: chrome, width: LF.barW }}>
          {/* barra */}
          <div
            style={{
              height: 40,
              display: "flex",
              alignItems: "center",
              padding: "0 5.6px 0 8px",
              gap: 4.8,
              boxSizing: "border-box",
              borderBottom: expanded ? "1px solid rgba(240,240,234,.10)" : "none",
            }}
          >
            <span style={{ color: C.muted, display: "grid" }}>
              <Icon name="search" size={16} strokeWidth={1.7} />
            </span>
            <span style={{ flex: 1, fontSize: 13, lineHeight: 1.2, color: hasQuery ? C.text : C.faint, whiteSpace: "nowrap" }}>
              {hasQuery ? query : "Buscar apps…"}
              {hasQuery && <span style={{ display: "inline-block", width: 1, height: 14, background: C.text, marginLeft: 1, verticalAlign: "middle", opacity: Math.floor(ms / 500) % 2 ? 0 : 1 }} />}
            </span>
          </div>

          {showRecents && (
            <>
              <div style={{ padding: "7.2px 11.2px 1.6px", fontSize: 9.9, fontWeight: 600, letterSpacing: "0.08em", color: C.faint, textTransform: "uppercase" }}>
                Recientes
              </div>
              <div style={{ padding: 5.6 }}>
                {RECENTS.map((h, i) => {
                  const a = EASE.smoothOut(seg(ms, recentsStart + 120 + Math.min(i, 10) * 45, 380));
                  return (
                    <div key={h.title} style={{ opacity: a, transform: `translateY(${(1 - a) * -6}px)` }}>
                      <Row hit={h} selected={i === 0} />
                    </div>
                  );
                })}
              </div>
            </>
          )}

          {hasQuery && (
            <>
              <div style={{ padding: 5.6, height: LF.expandedH - 40 - 34.6, boxSizing: "border-box" }}>
                {RESULTS.map((h, i) => (
                  <Row key={h.title} hit={h} selected={i === 0} pressed={i === 0 && pressed} />
                ))}
              </div>
              <div
                style={{
                  position: "absolute",
                  left: 0,
                  right: 0,
                  bottom: 0,
                  display: "flex",
                  justifyContent: "space-between",
                  gap: 6.4,
                  padding: "5.6px 8px 8px",
                  borderTop: "1px solid rgba(240,240,234,.10)",
                  color: C.faint,
                  fontSize: 9.6,
                }}
              >
                {[
                  ["↑↓", "navegar"],
                  ["Enter", "abrir"],
                  ["Esc", "cerrar"],
                ].map(([k, t]) => (
                  <span key={k} style={{ display: "inline-flex", alignItems: "center", gap: 4 }}>
                    <span
                      style={{
                        display: "inline-flex",
                        transform: k === "Enter" && pressed ? "scale(.92)" : "none",
                        filter: k === "Enter" && pressed ? "brightness(1.8)" : "none",
                      }}
                    >
                      <Kbd>{k}</Kbd>
                    </span>
                    {t}
                  </span>
                ))}
              </div>
            </>
          )}
        </div>
      </div>

      {/* favoritos */}
      {FAVS.map((name, i) => {
        const inK = EASE.smoothOut(seg(ms, readyAt + i * LF.favStaggerMs, LF.favMs));
        const tuck = EASE.smoothOut(seg(ms, tl.enterMs + (FAVS.length - 1 - i) * LF.favStaggerMs, LF.favMs));
        const a = inK * (1 - tuck);
        return (
          <div
            key={name}
            style={{
              position: "absolute",
              left: LF.barW + LF.favGap + i * (LF.dot + LF.favGap),
              top: 0,
              width: LF.dot,
              height: LF.dot,
              borderRadius: "50%",
              background: C.skin,
              color: C.muted,
              display: "grid",
              placeItems: "center",
              filter: "drop-shadow(0 10px 22px rgb(0 0 0 / 38%))",
              opacity: a,
              transform: `translateX(${(1 - a) * -23}px) scale(${lerp(0.82, 1, a)})`,
            }}
          >
            <Icon name={name} size={20} strokeWidth={1.7} />
          </div>
        );
      })}
    </div>
  );
};
