import type { CSSProperties, ReactNode } from "react";
import { PIcon, type PizarraIconName } from "./pizarraIcons";
import { C, FONT_SANS } from "./theme";

export type Pt = [number, number];
export type Rect = { x: number; y: number; w: number; h: number };

export type ToolId = "pen" | "arrow" | "ellipse" | "rect" | "highlight" | "text" | "crop";

/** Herramientas en el orden de la barra (ficha A.4). */
export const TOOLS: { id: ToolId; icon: PizarraIconName; label: string; key: string }[] = [
  { id: "pen", icon: "pen", label: "Lápiz", key: "1" },
  { id: "arrow", icon: "arrow", label: "Flecha", key: "2" },
  { id: "ellipse", icon: "ellipse", label: "Círculo", key: "3" },
  { id: "rect", icon: "rect", label: "Rectángulo", key: "4" },
  { id: "highlight", icon: "highlight", label: "Resaltador", key: "5" },
  { id: "text", icon: "text", label: "Texto", key: "6" },
  { id: "crop", icon: "crop", label: "Recortar", key: "7" },
];

/** Los 6 colores de trazo (annotateModel.ts) con su halo de texto. */
export const STROKE_COLORS = [
  { hex: "#ff3b30", halo: "#ffffffcc" },
  { hex: "#ffcc00", halo: "#00000099" },
  { hex: "#34c759", halo: "#00000099" },
  { hex: "#0a84ff", halo: "#ffffffcc" },
  { hex: "#ffffff", halo: "#00000099" },
  { hex: "#1c1c1e", halo: "#ffffffcc" },
] as const;

export const BAR_H = 46;
const BAR_W = 504;
const DISCARD_EXTRA = 56;
export const barWidth = (discard: boolean) => BAR_W + (discard ? DISCARD_EXTRA : 0);

/** Rectángulos en coordenadas locales de la barra (ficha A.3: todo derivado del CSS). */
export const barRects = (discard = false) => ({
  tool: (i: number): Rect => ({ x: 12 + 30 * i, y: 10, w: 28, h: 26 }),
  style: { x: 232, y: 10, w: 36, h: 26 } as Rect,
  undo: { x: 280, y: 10, w: 28, h: 26 } as Rect,
  redo: { x: 310, y: 10, w: 28, h: 26 } as Rect,
  copy: { x: 356, y: 10, w: 74, h: 26 } as Rect,
  save: { x: 436, y: 10, w: 26, h: 26 } as Rect,
  close: { x: 468, y: 10, w: discard ? 82 : 26, h: 26 } as Rect,
  popover: { x: 230, y: 44, w: 178, h: 68 } as Rect,
  swatch: (i: number): Rect => ({ x: 236 + 28 * i, y: 50, w: 26, h: 26 }),
  width: (i: number): Rect => ({ x: 236 + 34 * i, y: 80, w: 32, h: 26 }),
});

export const center = (r: Rect): Pt => [r.x + r.w / 2, r.y + r.h / 2];
export const inRect = (p: Pt | null, r: Rect) =>
  p !== null && p[0] >= r.x && p[0] <= r.x + r.w && p[1] >= r.y && p[1] <= r.y + r.h;

const HOVER = "rgb(240 240 234 / 10%)";
const HOVER_ACTION = "rgb(240 240 234 / 14%)";
const ON = "rgb(232 232 224 / 22%)";

const place = (r: Rect): CSSProperties => ({
  position: "absolute",
  left: r.x,
  top: r.y,
  width: r.w,
  height: r.h,
});

const Group = ({ rect, children, bg = C.elevated }: { rect: Rect; children?: ReactNode; bg?: string }) => (
  <div style={{ ...place(rect), borderRadius: 8, background: bg }}>{children}</div>
);

const Btn = ({
  rect,
  hover,
  on,
  disabled,
  pressed,
  hoverBg = HOVER,
  bg,
  color,
  children,
  style,
}: {
  rect: Rect;
  hover?: boolean;
  on?: boolean;
  disabled?: boolean;
  pressed?: boolean;
  hoverBg?: string;
  bg?: string;
  color?: string;
  children: ReactNode;
  style?: CSSProperties;
}) => {
  const lit = (hover || on) && !disabled;
  return (
    <div
      style={{
        ...place(rect),
        borderRadius: 5,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        background: bg ?? (on ? ON : hover && !disabled ? hoverBg : "transparent"),
        color: color ?? (lit ? C.text : C.muted),
        opacity: disabled ? 0.35 : 1,
        transform: pressed ? "scale(0.96)" : undefined,
        ...style,
      }}
    >
      {children}
    </div>
  );
};

export type PizarraBarProps = {
  tool: ToolId;
  /** Índice en STROKE_COLORS del color activo. */
  colorIndex: number;
  /** Nivel de grosor 1..3. */
  level?: 1 | 2 | 3;
  /** Popover de estilo abierto y ms desde que abrió (fundido de 75 ms). */
  popoverOpen?: boolean;
  popoverAgeMs?: number;
  hasStrokes?: boolean;
  /** X en rojo con «¿Descartar?». */
  discard?: boolean;
  /** Cursor en coordenadas locales de la barra (o null si no está encima). */
  cursor?: Pt | null;
  /** Botón recién pulsado (scale 0.96): "style" | "swatch:i" | "tool:i" | ... */
  pressed?: string | null;
};

/** Barra flotante de la pizarra (AnnotateSurface `.editor.is-board .bar`). */
export const PizarraBar = ({
  tool,
  colorIndex,
  level = 2,
  popoverOpen = false,
  popoverAgeMs = 999,
  hasStrokes = false,
  discard = false,
  cursor = null,
  pressed = null,
}: PizarraBarProps) => {
  const R = barRects(discard);
  const color = STROKE_COLORS[colorIndex].hex;
  const width = barWidth(discard);
  const popT = Math.min(1, popoverAgeMs / 75);

  return (
    <div
      style={{
        position: "relative",
        width,
        height: BAR_H,
        borderRadius: 14,
        background: C.surface2,
        boxShadow: "0 24px 70px rgb(0 0 0 / 45%)",
        outline: `1px solid ${C.line}`,
        outlineOffset: -1,
        fontFamily: FONT_SANS,
      }}
    >
      {/* herramientas */}
      <Group rect={{ x: 10, y: 8, w: 212, h: 30 }}>
        {TOOLS.map((t, i) => (
          <Btn
            key={t.id}
            rect={{ ...R.tool(i), x: R.tool(i).x - 10, y: R.tool(i).y - 8 }}
            hover={inRect(cursor, R.tool(i))}
            on={tool === t.id}
            pressed={pressed === `tool:${i}`}
          >
            <PIcon name={t.icon} size={15} />
          </Btn>
        ))}
      </Group>

      {/* color y grosor */}
      <Group rect={{ x: 230, y: 8, w: 40, h: 30 }}>
        <Btn
          rect={{ x: 2, y: 2, w: 36, h: 26 }}
          hover={inRect(cursor, R.style)}
          on={popoverOpen}
          pressed={pressed === "style"}
          style={{ gap: 3 }}
        >
          <span
            style={{
              width: 6 + level * 2,
              height: 6 + level * 2,
              borderRadius: "50%",
              background: color,
              boxShadow: "0 0 0 1px rgb(0 0 0 / 25%) inset",
            }}
          />
          <PIcon name="chevron" size={11} />
        </Btn>
      </Group>

      {/* deshacer / rehacer */}
      <Group rect={{ x: 278, y: 8, w: 62, h: 30 }}>
        <Btn
          rect={{ x: 2, y: 2, w: 28, h: 26 }}
          hover={inRect(cursor, R.undo)}
          disabled={!hasStrokes}
        >
          <PIcon name="undo" size={15} />
        </Btn>
        <Btn rect={{ x: 32, y: 2, w: 28, h: 26 }} hover={inRect(cursor, R.redo)} disabled>
          <PIcon name="redo" size={15} />
        </Btn>
      </Group>

      {/* acciones */}
      <Btn
        rect={R.copy}
        hoverBg={HOVER_ACTION}
        bg={C.accent}
        color={C.onAccent}
        style={{ gap: 6, fontSize: 11, fontWeight: 500 }}
      >
        <PIcon name="copy" size={14} />
        Copiar
      </Btn>
      <Btn rect={R.save} hover={inRect(cursor, R.save)} hoverBg={HOVER_ACTION}>
        <PIcon name="save" size={14} />
      </Btn>
      <Btn
        rect={R.close}
        hover={inRect(cursor, R.close)}
        hoverBg={HOVER_ACTION}
        bg={discard ? C.rec : undefined}
        color={discard ? "#fff" : undefined}
        style={{ gap: 6, fontSize: 11, fontWeight: 500, whiteSpace: "nowrap" }}
      >
        {discard ? "¿Descartar?" : <PIcon name="close" size={14} />}
      </Btn>

      {/* popover de color y grosor */}
      {popoverOpen && (
        <div
          style={{
            ...place(R.popover),
            boxSizing: "border-box",
            padding: 6,
            borderRadius: 14,
            background: C.surface2,
            boxShadow: "0 24px 70px rgb(0 0 0 / 45%)",
            outline: `1px solid ${C.line}`,
            outlineOffset: -1,
            opacity: popT,
            transform: `translateY(${(1 - popT) * -4}px)`,
          }}
        >
          {STROKE_COLORS.map((c, i) => {
            const r = R.swatch(i);
            const selected = i === colorIndex;
            const inset = selected ? 2 : 4;
            return (
              <div
                key={c.hex}
                style={{
                  position: "absolute",
                  left: r.x - R.popover.x,
                  top: r.y - R.popover.y,
                  width: 26,
                  height: 26,
                  transform: pressed === `swatch:${i}` ? "scale(0.96)" : undefined,
                }}
              >
                <span
                  style={{
                    position: "absolute",
                    inset,
                    borderRadius: "50%",
                    background: c.hex,
                    boxShadow: selected
                      ? `0 0 0 1px rgb(0 0 0 / 25%) inset, 0 0 0 2px ${C.surface2}, 0 0 0 3px ${C.text}`
                      : "0 0 0 1px rgb(0 0 0 / 25%) inset",
                  }}
                />
              </div>
            );
          })}
          {[1, 2, 3].map((v, i) => {
            const r = R.width(i);
            const selected = v === level;
            return (
              <Btn
                key={v}
                rect={{ x: r.x - R.popover.x, y: r.y - R.popover.y, w: 32, h: 26 }}
                on={selected}
                hover={inRect(cursor, r)}
              >
                <span
                  style={{
                    width: 18,
                    height: v * 2,
                    borderRadius: 999,
                    background: color,
                    boxShadow: "0 0 0 1px rgb(0 0 0 / 25%)",
                  }}
                />
              </Btn>
            );
          })}
        </div>
      )}
    </div>
  );
};

/** Chip pastilla de la pizarra (`.status`), p. ej. la ayuda inferior. */
export const PizarraChip = ({ children, opacity = 1 }: { children: ReactNode; opacity?: number }) => (
  <div
    style={{
      padding: "5px 10px",
      borderRadius: 999,
      fontFamily: FONT_SANS,
      fontSize: 11,
      lineHeight: 1.4,
      color: C.muted,
      background: C.surface2,
      boxShadow: "0 24px 70px rgb(0 0 0 / 45%)",
      outline: `1px solid ${C.line}`,
      outlineOffset: -1,
      whiteSpace: "nowrap",
      opacity,
    }}
  >
    {children}
  </div>
);
