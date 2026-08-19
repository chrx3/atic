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
  /**
   * Isla acoplada en reposo: grosor contra el borde y largo a lo largo de él.
   *
   * Fina a propósito: acoplada deja de ser un botón y pasa a ser una pestaña.
   * El largo es lo que la vuelve agarrable —10×10 sería imposible de apuntar—
   * y lo que le da al líquido con qué formar el cuello cuando abre un float.
   */
  islandThick: 10,
  /**
   * **Tiene que valer lo mismo que `bar`.** No es estética: es lo que impide
   * un bucle.
   *
   * La isla se abre con el puntero encima y se cierra al salir. Si al abrirse
   * encogiera en el eje largo (56 → 40), un cursor apoyado en el extremo
   * quedaría fuera de la caja nueva: saldría, cerraría, la caja volvería a
   * crecer bajo el cursor, entraría, abriría… a 60 Hz.
   *
   * Igualándolo a `bar`, el rectángulo abierto **contiene** siempre al
   * cerrado: en el eje del borde solo crece y en el perpendicular no cambia.
   * `pillPlan.test.ts` lo fija.
   */
  islandLong: 40,
  /** Lado del botón de herramienta dentro de la isla abierta. */
  islandTool: 34,
  /** Hueco entre botones de la tira. */
  islandGap: 6,
  /** Diámetro de la gota de grabación/dictado que cuelga del chrome. */
  recDrop: 36,
  /** Hueco entre el cuerpo y la gota: bajo el alcance del goo (~10 px). */
  recDropGap: 8,
  /**
   * Extra de alto de la rueda cuando hay gota viva.
   *
   * El pivote es el centro, así que el extra se parte arriba y abajo. 28 px
   * dejan ~24 px bajo el disco de 232, que es lo que pide la gota de 36 con
   * cuello de 8 sin recortar el filtro.
   */
  wheelLiveHang: 28,
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
/**
 * `dock*` clava el lado que toca el borde y centra el eje perpendicular.
 *
 * Hacen falta los cuatro y no alcanza con `topLeft`: acoplada a la derecha, al
 * expandirse tiene que crecer **hacia adentro**, o sea conservando `x + w`. Con
 * `topLeft` crecería hacia afuera y se saldría de la pantalla.
 */
export type Pivot =
  | "topLeft"
  | "center"
  | "panel"
  | "bottomLeft"
  | "cursor"
  | "dockLeft"
  | "dockRight"
  | "dockTop"
  | "dockBottom";

export type ResizeOutcome = {
  /** Cambió el tamaño (false = descartado por uno más nuevo). */
  ok: boolean;
  /** El panel abrió hacia arriba. */
  up: boolean;
};
