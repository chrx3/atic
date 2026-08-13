/**
 * Geometría de la pill: un solo lugar decide de qué tamaño está la ventana.
 *
 * El modelo anterior tenía cinco funciones que redimensionaban (`fitWindow`,
 * `resizeAroundCenter`, `shrinkFromRadial`, `fitPanelExpanded`,
 * `resetPillChrome`) coordinadas por ocho banderas booleanas. Todas respondían
 * la misma pregunta —¿qué tamaño va ahora?— que es **estado derivado**.
 *
 * Acá la respuesta se deriva una vez y un escenario la aplica. El ejecutor
 * vive en `pillCssStage`: escribe `left`/`top` en un div del overlay, así que
 * mover es síncrono y no hay destinos obsoletos que descartar.
 */

/** Medidas base. Todo lo demás se mide del DOM. */
export const PILL = {
  /** Respiro alrededor del contenido dentro de la ventana. */
  pad: 4,
  /** Alto de la barra compacta y diámetro del disco en reposo. */
  bar: 40,
  /**
   * Lado del escenario cuadrado de la rueda: el disco (232 px, fijado en
   * `.p-wheel` del CSS) más 10 px de aire por lado.
   *
   * El aire no es decorativo. Las gotas de la rueda se funden con cuellos que
   * viven FUERA del disco de 232; sin pad, el goo se recorta contra el borde
   * de la ventana.
   */
  wheel: 252,
  /** Ancho del panel de historial / fragmentos. */
  panelW: 312,
  /** Alto del panel (sin la barra). */
  panelH: 332,
} as const;

export type Size = { w: number; h: number };

/** Tamaño de ventana que contiene un contenido dado. */
export function windowFor(content: Size): Size {
  return {
    w: Math.ceil(content.w) + PILL.pad * 2,
    h: Math.ceil(content.h) + PILL.pad * 2,
  };
}

/** Dos tamaños iguales dentro de un píxel: no vale la pena tocar la ventana. */
export function sameSize(a: Size, b: Size): boolean {
  return Math.abs(a.w - b.w) < 1 && Math.abs(a.h - b.h) < 1;
}

/**
 * ¿La ventana debe crecer ANTES de animar el contenido?
 *
 * Es la regla que reemplaza a `chromeHidden` / `radialClosing` / `quickClose`:
 *
 *   - Creciendo: agrandar primero, si no el contenido nuevo se recorta contra
 *     los bordes mientras entra.
 *   - Encogiendo: animar primero y achicar al final, si no el contenido viejo
 *     se recorta mientras sale.
 *
 * En ambos casos la ventana es la unión de origen y destino durante toda la
 * animación, así que nunca hay recorte ni un frame vacío que tapar.
 */
export function growsFirst(from: Size, to: Size): boolean {
  return to.w > from.w || to.h > from.h;
}

/**
 * Punto que se conserva al redimensionar.
 *
 * - `topLeft`: crecimiento normal.
 * - `center`: la rueda crece desde la marca en vez de desplegarse.
 * - `panel`: la barra no se mueve; el panel abre hacia abajo, o hacia arriba
 *   si no entra. Rust decide la dirección porque es quien conoce los monitores.
 * - `bottomLeft`: la barra vive en el borde de abajo. Es el pivote del colapso
 *   de un panel que abrió hacia arriba: ahí Rust ya no puede deducir la
 *   dirección (al encoger, el tamaño chico siempre "entra hacia abajo"), así
 *   que la recuerda el frontend, que fue quien recibió el `up`.
 * - `cursor`: no conserva nada. Centra la ventana ya redimensionada en el
 *   puntero. Es el único absoluto: crecer e ir al cursor en una escritura.
 */
export type Pivot = "topLeft" | "center" | "panel" | "bottomLeft" | "cursor";

export type ResizeOutcome = {
  /** Cambió el tamaño (false = descartado por uno más nuevo). */
  ok: boolean;
  /** El panel abrió hacia arriba. */
  up: boolean;
};
