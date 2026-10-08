import { BrowserApp } from "../desk/desk";

/** Título de la ventana ajena que se voltea (aparece en la píldora «sobre …»). */
export const FLIP_WINDOW_TITLE = "Panel · Rendimiento";

/** Ventana del navegador simulado, en px lógicos de la pantalla (la que se voltea). */
export const FLIP_WINDOW = { w: 450, h: 400 } as const;

/**
 * Frente del flip: el navegador del escritorio con el gráfico del pico. Se dibuja a su
 * tamaño lógico y se escala para llenar la tarjeta (`cardW` px reales de ancho).
 */
export const StoryFlipFront = ({ cardW }: { cardW: number }) => (
  <div style={{ position: "absolute", inset: 0, overflow: "hidden", background: "#fff" }}>
    <div
      style={{
        position: "absolute",
        left: 0,
        top: 0,
        width: FLIP_WINDOW.w,
        height: FLIP_WINDOW.h,
        transform: `scale(${cardW / FLIP_WINDOW.w})`,
        transformOrigin: "0 0",
      }}
    >
      <BrowserApp rect={{ x: 0, y: 0, w: FLIP_WINDOW.w, h: FLIP_WINDOW.h }} />
    </div>
  </div>
);
