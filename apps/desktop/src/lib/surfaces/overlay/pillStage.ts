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
  bar: 52,
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
   * Isla acoplada en reposo: pestaña que nace del muro, no un disco pegado.
   *
   * `thick` es lo que asoma hacia adentro. Si vale lo mismo que `long`,
   * `pillShape` sale un círculo y el `smin` con la pared se lee como una
   * bola con un mordisco, no como un notch.
   */
  islandThick: 40,
  /**
   * Pestaña con aviso de agente o dock achicado: cabe un icono pequeño
   * dentro del padding del root y de la isla. Sigue siendo pestaña,
   * menor que `islandTool`.
   *
   * Tiene que ser menor que `islandTool`: si la cerrada fuera más gorda que
   * la tira abierta, el hover de abrir encogería un eje y rearmaría el bucle.
   */
  islandCueThick: 42,
  /**
   * Dintel de la isla, a lo largo del borde.
   *
   * Más largo que `thick` a propósito: si se acercan, `pillShape` sale un
   * círculo. El valor es una cápsula tipo Dynamic Island, no una pestaña
   * justa a la marca. Un aviso cabe adentro sin alargar; varios sí.
   */
  islandLong: 124,
  /** La marca de Atic dentro de la pestaña. Se ve siempre, haya aviso o no. */
  islandMark: 32,
  /**
   * Botón de aviso (agente o update) al lado de la marca en la pestaña.
   *
   * La marca ya no cede el sitio al aviso: conviven, y la pestaña se alarga
   * a lo largo del borde en vez de taparla.
   */
  islandCueBtn: 26,
  /** Logo de agente / icono de update dentro de ese botón. */
  islandCueMark: 14,
  /** Lado del botón de herramienta dentro de la isla abierta. */
  islandTool: 44,
  /** Hueco entre botones de la tira. Chico a propósito: el clic entre iconos
   *  tiene que caer en una herramienta, no en el cuerpo (que abre la rueda). */
  islandGap: 2,
  /**
   * Tarjeta de cara expandida colgada de la pestaña (un solo blob, gap 0).
   *
   * Fijo al abrir: el contenido vivo (timer, texto) no puede re-medir la caja
   * a cada frame. El CSS tiene que sumar exacto: pestaña + `islandCardH`.
   */
  islandCardW: 280,
  islandCardH: 104,
  /**
   * Cara de panel (clipboard, textos, agentes): lista o lanzador con scroll
   * dentro de la isla, no un float. Fijo al abrir, igual que `islandCard*`.
   */
  islandClipW: 280,
  islandClipH: 252,
  /**
   * Cara de agentes: el lanzador es chico; la consola pide más alto.
   * Dos medidas fijas para no re-medir el DOM a cada tecla.
   */
  islandAgentsSetupW: 400,
  islandAgentsSetupH: 188,
  islandAgentsW: 440,
  islandAgentsH: 496,
  /** Radio de la cara con contenido: si fuera pastilla, un panel alto se lee gota. */
  islandClipR: 22,
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
  /** Cada aviso extra debajo del primero, en la barra flotante. */
  agentStackRow: 28,
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
