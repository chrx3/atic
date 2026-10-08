import type { CSSProperties, ReactNode } from "react";
import { C, FONT_MONO, SHADOW_GOO } from "./theme";

/** Kbd de la app (lib/ui/Kbd.svelte): una tecla por cada parte del combo. */
export const Kbd = ({ children }: { children: ReactNode }) => (
  <kbd
    style={{
      display: "inline-flex",
      alignItems: "center",
      justifyContent: "center",
      height: 20,
      minWidth: 20,
      padding: "0 4px",
      fontFamily: FONT_MONO,
      fontSize: 11,
      lineHeight: 1.3,
      color: C.muted,
      background: C.surface2,
      border: `1px solid ${C.line}`,
      borderRadius: 5,
    }}
  >
    {children}
  </kbd>
);

/** Silueta mate de un float: relleno --skin con la sombra goo de la app. */
export const SkinBox = ({
  style,
  children,
}: {
  style: CSSProperties;
  children?: ReactNode;
}) => (
  <div
    style={{
      position: "absolute",
      background: C.skin,
      filter: SHADOW_GOO,
      ...style,
    }}
  >
    {children}
  </div>
);

/** Texto que se va escribiendo: devuelve el prefijo visible en `ms`. */
export const typed = (text: string, ms: number, startMs: number, msPerChar: number) => {
  const n = Math.floor((ms - startMs) / msPerChar) + 1;
  return text.slice(0, Math.max(0, Math.min(text.length, n)));
};
