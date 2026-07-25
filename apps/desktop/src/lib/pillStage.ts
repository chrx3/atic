/**
 * Geometría de la pill: un solo lugar decide de qué tamaño está la ventana.
 *
 * El modelo anterior tenía cinco funciones que redimensionaban (`fitWindow`,
 * `resizeAroundCenter`, `shrinkFromRadial`, `fitPanelExpanded`,
 * `resetPillChrome`) coordinadas por ocho banderas booleanas. Todas respondían
 * la misma pregunta —¿qué tamaño va ahora?— que es **estado derivado**.
 *
 * Acá la respuesta se deriva una vez y un reconciliador la aplica. El
 * reconciliador es idempotente y cancelable: si llega un destino nuevo a mitad
 * de camino, el anterior se abandona sin tocar la ventana.
 */

import { invoke } from "@tauri-apps/api/core";

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
   * El aire no es decorativo. La apertura anima el disco con `--morph-ease`
   * —cubic-bezier(.34, 1.25, .64, 1)—, que SOBREPASA ~6% antes de asentar: el
   * disco llega a ~246 px. Con la ventana justo a 232 ese pico se recortaba
   * contra los bordes y se veían los cuatro costados del círculo achatados
   * durante el rebote.
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

/**
 * Aplica tamaños de ventana en serie, descartando los obsoletos.
 *
 * Un solo `invoke` hace resize + reposición: como no hay dos IPC, no existe el
 * frame intermedio con la ventana ya redimensionada pero aún mal ubicada.
 */
export function createStage(label: string) {
  let generation = 0;
  let current: Size | null = null;

  /** Último tamaño aplicado; permite decidir crecer-antes vs encoger-después. */
  function applied(): Size | null {
    return current;
  }

  /**
   * Lleva la ventana a `target`. `ok: false` significa que otro destino la
   * reemplazó mientras tanto y el llamador debe abandonar sin animar más.
   */
  async function resize(
    target: Size,
    pivot: Pivot = "topLeft",
    animate = false,
  ): Promise<ResizeOutcome> {
    // Ya está en ese tamaño: evita IPC redundante y, sobre todo, evita que un
    // pivote `center` recentre la ventana sin que nada haya cambiado.
    //
    // `cursor` se exceptúa: no es un pivote sino una posición absoluta, así que
    // saltearlo por "mismo tamaño" dejaría la rueda donde estaba en vez de
    // traerla al puntero.
    if (pivot !== "cursor" && current && sameSize(current, target)) {
      return { ok: true, up: false };
    }
    const mine = ++generation;
    try {
      const result = await invoke<{ up: boolean }>("resize_floating", {
        label,
        width: target.w,
        height: target.h,
        anchor: pivot,
        animate,
      });
      if (generation !== mine) return { ok: false, up: false };
      current = target;
      return { ok: true, up: result.up };
    } catch (err) {
      console.warn("resize_floating", err);
      return { ok: generation === mine, up: false };
    }
  }

  /**
   * Registra un tamaño aplicado por fuera (un morph que corrió en Rust).
   *
   * Sin esto el reconciliador vería un tamaño desactualizado, emitiría un
   * resize redundante y ese resize cancelaría el morph a mitad de camino.
   */
  function adopt(size: Size) {
    current = size;
    generation++;
  }

  return { resize, applied, adopt };
}
