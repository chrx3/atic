import { createContext, useContext, type ReactNode } from "react";

/** Dónde y a qué escala se dibuja el monitor dentro del cuadro de video. */
export type Format = {
  id: "v" | "h" | "s";
  width: number;
  height: number;
  /** Esquina superior izquierda del monitor (px de salida). */
  screenLeft: number;
  screenTop: number;
  /** Escala pantalla lógica → px de salida. */
  scale: number;
  /** Las escenas pintan su propio título; el video corto lo reemplaza por gráficos. */
  showCaption: boolean;
  /** Ancho lógico del escritorio (500 por defecto); más ancho = más papel tapiz a los lados. */
  logicalW?: number;
  /** Pantalla completa: sin marco redondeado ni sombra. */
  fullBleed?: boolean;
  /** Papel tapiz del escritorio simulado. */
  wallpaper?: "blue" | "light";
};

/** Formato original de las escenas largas (9:16, monitor a 2×). */
export const FORMAT_LONG: Format = {
  id: "v",
  width: 1080,
  height: 1920,
  screenLeft: 40,
  screenTop: 380,
  scale: 2,
  showCaption: true,
};

/** 9:16 para el corto: monitor un poco menor para dejar aire al título. */
export const FORMAT_SHORT_V: Format = {
  id: "v",
  width: 1080,
  height: 1920,
  screenLeft: 77,
  screenTop: 400,
  scale: 1.85,
  showCaption: false,
};

/** 16:9 para el corto: el monitor a la derecha y el texto a la izquierda. */
export const FORMAT_SHORT_H: Format = {
  id: "h",
  width: 1920,
  height: 1080,
  screenLeft: 1010,
  screenTop: 28,
  scale: 1.55,
  showCaption: false,
};

/** 1:1 a pantalla completa: un monitor cuadrado de 660×660 lógicos, fondo claro. */
export const FORMAT_SQUARE: Format = {
  id: "s",
  width: 1080,
  height: 1080,
  screenLeft: 0,
  screenTop: 0,
  scale: 1080 / 660,
  showCaption: false,
  logicalW: 660,
  fullBleed: true,
  wallpaper: "light",
};

const FormatContext = createContext<Format>(FORMAT_LONG);

export const FormatProvider = ({
  format,
  children,
}: {
  format: Format;
  children: ReactNode;
}) => <FormatContext.Provider value={format}>{children}</FormatContext.Provider>;

export const useFormat = () => useContext(FormatContext);

/** Fondo de la escena: opaco en las escenas largas, transparente en el corto (lo pinta el escenario). */
export const useStageBg = () => (useFormat().showCaption ? "#121211" : "transparent");
