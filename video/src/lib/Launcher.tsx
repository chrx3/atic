import { Easing } from "remotion";
import { Icon, type IconName } from "./Icon";
import { C, EASE, FONT_SANS, lerp } from "./theme";
import { Kbd, typed } from "./ui";
import { seg } from "./time";

/** Constantes de LauncherFloat.svelte. */
export const LF = {
  barW: 324,
  barH: 40,
  favGap: 15,
  dot: 40,
  birthMs: 260,
  beatMs: 90,
  stretchMs: 420,
  chromeMs: 240,
  favStaggerMs: 110,
  favMs: 380,
  recentsMs: 520,
  expandedH: 360,
};

const RECENTS_EASE = Easing.bezier(0.45, 0, 0.2, 1);

type Hit = {
  title: string;
  sub: string;
  icon: IconName | "app-chrome" | "app-code" | "app-music";
  action?: boolean;
};

const RECENTS: Hit[] = [
  { title: "Google Chrome", sub: "En uso", icon: "app-chrome" },
  { title: "Visual Studio Code", sub: "Abierta hace 12 min", icon: "app-code" },
  { title: "Spotify", sub: "Usada hace 3 h", icon: "app-music" },
];

/** Iconos de app: sustitutos genéricos (los reales son bitmaps del .exe). */
const AppGlyph = ({ kind, size }: { kind: string; size: number }) => {
  const r = 3.2;
  if (kind === "app-chrome")
    return (
      <div
        style={{
          width: size,
          height: size,
          borderRadius: "50%",
          background:
            "conic-gradient(#e0574c 0 120deg, #e6b93f 120deg 240deg, #4fa870 240deg 360deg)",
          display: "grid",
          placeItems: "center",
        }}
      >
        <div style={{ width: size * 0.38, height: size * 0.38, borderRadius: "50%", background: "#5b8fd6", boxShadow: "0 0 0 1.5px #f4f4f0" }} />
      </div>
    );
  if (kind === "app-code")
    return (
      <div style={{ width: size, height: size, borderRadius: r, background: "#3b78c4", display: "grid", placeItems: "center", color: "#f4f4f0" }}>
        <svg width={size * 0.62} height={size * 0.62} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2.4} strokeLinecap="round" strokeLinejoin="round">
          <path d="m8 6-6 6 6 6" />
          <path d="m16 6 6 6-6 6" />
        </svg>
      </div>
    );
  return (
    <div style={{ width: size, height: size, borderRadius: "50%", background: "#3f9c62", display: "grid", placeItems: "center" }}>
      <svg width={size * 0.6} height={size * 0.6} viewBox="0 0 24 24" fill="none" stroke="#10140f" strokeWidth={2.6} strokeLinecap="round">
        <path d="M4 9c5-1.5 11-1 16 1.5" />
        <path d="M5.5 13.5c4-1 8.5-.6 12.5 1.2" />
        <path d="M7 17.6c3-.6 6-.3 9 .9" />
      </svg>
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
      <AppGlyph kind={hit.icon} size={size} />
    ) : (
      <Icon name={hit.icon as IconName} size={size} strokeWidth={1.7} />
    )}
  </div>
);

const Row = ({ hit, selected, star }: { hit: Hit; selected: boolean; star?: boolean }) => (
  <div
    style={{
      display: "flex",
      alignItems: "center",
      gap: 2.4,
      borderRadius: 8,
      background: selected ? "rgba(240,240,234,.05)" : "transparent",
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
      {star !== false && (
        <svg width={14} height={14} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.75} strokeLinecap="round" strokeLinejoin="round">
          <path d="M11.525 2.295a.53.53 0 0 1 .95 0l2.31 4.679a2.123 2.123 0 0 0 1.595 1.16l5.166.756a.53.53 0 0 1 .294.904l-3.736 3.638a2.123 2.123 0 0 0-.611 1.878l.882 5.14a.53.53 0 0 1-.771.56l-4.618-2.428a2.122 2.122 0 0 0-1.973 0L6.396 21.01a.53.53 0 0 1-.77-.56l.881-5.139a2.122 2.122 0 0 0-.611-1.879L2.16 9.795a.53.53 0 0 1 .294-.906l5.165-.755a2.122 2.122 0 0 0 1.597-1.16z" />
        </svg>
      )}
    </div>
  </div>
);

const SEARCH_RESULTS: Record<string, Hit[]> = {
  co: [
    { title: "Elegir color", sub: "Cuentagotas: un píxel de la pantalla al portapapeles", icon: "color", action: true },
    { title: "Visual Studio Code", sub: "Aplicación", icon: "app-code" },
    { title: "Google Chrome", sub: "Aplicación", icon: "app-chrome" },
  ],
  "2+3*4": [{ title: "14", sub: "Enter para copiar", icon: "calculator", action: true }],
};

export type LauncherTimeline = {
  /** Instante en que nace la gota. */
  bornMs: number;
  /** Instantes: [empieza a escribir "co", baja la selección, borra y escribe la cuenta]. */
  typeCoMs: number;
  arrowMs: number;
  typeCalcMs: number;
};

/**
 * Launcher (Ctrl+Space): gota de 40 px en el centro → estirón a 324 px →
 * favoritos → recientes → búsqueda. Coordenadas del contenedor: (0,0) es la
 * esquina superior izquierda de la barra.
 */
export const Launcher = ({ ms, tl }: { ms: number; tl: LauncherTimeline }) => {
  const b = tl.bornMs;
  const drop = EASE.smoothOut(seg(ms, b, LF.birthMs));
  const stretchStart = b + LF.birthMs + LF.beatMs;
  const stretch = EASE.liquid(seg(ms, stretchStart, LF.stretchMs));
  const readyAt = stretchStart + LF.stretchMs;
  const chrome = EASE.smoothOut(seg(ms, readyAt, LF.chromeMs));

  const w = lerp(LF.barH, LF.barW, stretch);
  const left = (LF.barW - w) / 2;

  const q1 = typed("co", ms, tl.typeCoMs, 140);
  const q2 = ms >= tl.typeCalcMs ? typed("2+3*4", ms, tl.typeCalcMs, 90) : "";
  const query = ms >= tl.typeCalcMs ? q2 : q1;
  const hasQuery = query.length > 0;

  const recentsStart = readyAt + 500;
  const recentsH = 40 + 28 + RECENTS.length * 48 + 10;
  const grow = RECENTS_EASE(seg(ms, recentsStart, LF.recentsMs));

  let height = lerp(LF.barH, recentsH, grow);
  if (hasQuery) height = LF.expandedH;
  const expanded = hasQuery || grow > 0;

  const hits = query === "co" ? SEARCH_RESULTS.co : query === "2+3*4" ? SEARCH_RESULTS["2+3*4"] : [];
  const selected = query === "co" && ms >= tl.arrowMs ? 1 : 0;

  // Favoritos: tres dots a la derecha de la barra.
  const FAVS: IconName[] = ["clipboard", "captures", "board"];
  const favStart = readyAt;

  const showRecents = !hasQuery && grow > 0;

  return (
    <div
      style={{
        position: "absolute",
        left: 0,
        top: 0,
        width: LF.barW,
        height: LF.barH,
        opacity: drop,
        transform: `scale(${lerp(0.82, 1, drop)})`,
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
            <span style={{ flex: 1, fontSize: 13, lineHeight: 1.2, color: hasQuery ? C.text : C.faint }}>
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
                {hits.map((h, i) => (
                  <Row key={h.title} hit={h} selected={i === selected} star={!h.title.match(/^\d/)} />
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
                    <Kbd>{k}</Kbd>
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
        const a = EASE.smoothOut(seg(ms, favStart + i * LF.favStaggerMs, LF.favMs));
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
