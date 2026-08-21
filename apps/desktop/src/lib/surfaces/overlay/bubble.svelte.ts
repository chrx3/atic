/**
 * La geometría del globo anclado a la pill.
 *
 * # Por qué vive aparte
 *
 * Antes convivía con el protocolo y con el render del float de agentes, y no
 * tiene nada que ver con ninguno de los dos: acá no se sabe qué es un agente ni
 * qué es un turno. Es solo dónde está la burbuja, de qué lado le sale el cuello
 * y si ya se puede mostrar.
 *
 * # La conversión que se fue
 *
 * Acá había una división por `devicePixelRatio`: Rust razonaba en píxeles
 * físicos —es lo que usa Win32— y mandaba el tamaño de una ventana. Dentro del
 * overlay el globo es un `div`, así que la geometría **nunca sale de CSS** y la
 * conversión pasa una sola vez, en Rust, con la escala de la ventana que de
 * verdad lo pinta.
 */
import { tick } from "svelte";
import type { BubbleOpen } from "$core/types";
import { MOTION, ms } from "$lib/motion";
import { BUBBLE_MIN_H, BUBBLE_MIN_W } from "./contract";

/** Dónde salió la burbuja, en píxeles CSS del overlay. */
export interface Anchor {
  side: string;
  offset: number;
  x: number;
  y: number;
  w: number;
  h: number;
}

// Los mínimos del globo viven en `contract.ts`, que es lo que el test compara
// contra Rust. Se re-exportan porque la consola los pide desde acá.
export { BUBBLE_MIN_H, BUBBLE_MIN_W };

export class Bubble {
  anchor = $state<Anchor | null>(null);
  /** Ya se puede pintar: antes de esto no se sabe dónde va el cuello. */
  shown = $state(false);
  /**
   * Sigue en pantalla, aunque sea replegándose.
   *
   * Separado de `shown` para que la capa fundida se desmonte al TERMINAR el
   * cierre y no al empezarlo: montada, sigue el rectángulo de la pill y vuelve
   * a pasar el filtro cada vez que se mueve, o sea que arrastrar la pill con la
   * consola cerrada costaría un difuminado por cuadro para no dibujar nada.
   */
  alive = $state(false);
  /** Llegó de Rust: dónde cae y de qué lado le sale el cuello. */
  place(a: BubbleOpen): void {
    // Idempotente con la misma geometría (abierta o a mitad de abrir):
    // reasignar reinicia `$effect`s de skin / hit-rect y, si aún no hay
    // `.is-shown`, vuelve a apagar el morph → blob pegado sin barra usable.
    const cur = this.anchor;
    if (
      this.alive &&
      cur &&
      cur.side === a.side &&
      cur.offset === a.offset &&
      cur.x === a.x &&
      cur.y === a.y &&
      cur.w === a.w &&
      cur.h === a.h
    ) {
      return;
    }
    this.anchor = {
      side: a.side,
      offset: a.offset,
      x: a.x,
      y: a.y,
      w: a.w,
      h: a.h,
    };
    // Si ya estaba abierta, solo reancorar: resetear `shown` apaga
    // `pointer-events` (vía `.float-emerge`) y la ventana queda muerta un
    // frame —o para siempre si el rAF se pierde bajo carga.
    if (this.alive && this.shown) return;
    this.shown = false;
    this.alive = true;
    // Un cuadro replegado (`.float-emerge` sin `.is-shown`): scale + viaje
    // hacia la pill. Sin ese frame, aparece de golpe en vez de nacer.
    // El timeout cubre WebView2 dormido: con el overlay click-through el rAF
    // a veces no corre hasta el próximo clic.
    void tick().then(() => {
      requestAnimationFrame(() => {
        if (this.alive) this.shown = true;
      });
      window.setTimeout(() => {
        if (this.alive && !this.shown) this.shown = true;
      }, 50);
    });
  }

  /**
   * La arrastraron.
   *
   * Ya no marca nada de «despegada»: el cuello se calcula contra los dos
   * rectángulos en vivo, así que estira siguiendo al globo y se corta cuando de
   * verdad están lejos. Antes hacía falta la marca porque el cuello era un
   * dibujo fijo que, movido de sitio, apuntaba a donde la pill no estaba.
   */
  moveBy(dx: number, dy: number): void {
    if (!this.anchor) return;
    this.moveTo(this.anchor.x + dx, this.anchor.y + dy);
  }

  /** Coloca la esquina superior izquierda (coords CSS del overlay). */
  moveTo(x: number, y: number): void {
    if (!this.anchor) return;
    this.anchor = { ...this.anchor, x, y };
  }

  /**
   * El usuario la estiró. El borde anclado NO se mueve.
   *
   * Si el cuello sale por arriba, crecer empuja el borde de abajo; si sale por
   * abajo, crece hacia arriba. Es lo que hace que el cuello siga tocando la
   * pill mientras arrastrás, en vez de despegarse y tener que reacomodarlo
   * después. La cuenta estaba en Rust, que además movía la ventana; ahora que
   * el globo es un `div`, mover la ventana era el único motivo para ir hasta
   * allá.
   */
  resize(w: number, h: number): void {
    const a = this.anchor;
    if (!a) return;
    w = Math.max(BUBBLE_MIN_W, Math.round(w));
    h = Math.max(BUBBLE_MIN_H, Math.round(h));
    this.anchor = {
      ...a,
      w,
      h,
      x: a.side === "right" ? a.x + a.w - w : a.x,
      y: a.side === "bottom" ? a.y + a.h - h : a.y,
    };
  }

  /**
   * Marco libre (x, y, w, h) en coords CSS del overlay.
   *
   * Para estirar desde cualquier borde/esquina: el caller ya clampeó mínimos
   * y movió la esquina opuesta. No preserva el borde del cuello.
   */
  setFrame(x: number, y: number, w: number, h: number): void {
    const a = this.anchor;
    if (!a) return;
    this.anchor = {
      ...a,
      x: Math.round(x),
      y: Math.round(y),
      w: Math.max(BUBBLE_MIN_W, Math.round(w)),
      h: Math.max(BUBBLE_MIN_H, Math.round(h)),
    };
  }

  /** Apaga el contenido y, al terminar el repliegue, desmonta la piel. */
  hide(): void {
    this.shown = false;
    window.setTimeout(() => {
      if (!this.shown) this.alive = false;
    }, ms(MOTION.floatClose));
  }

  /**
   * De qué lados se agarra para redimensionar.
   *
   * De los dos OPUESTOS al cuello, siempre. Arrastrar por el lado anclado
   * despegaría el globo de la pill mientras lo estiras, que es justo lo que la
   * geometría de Rust se encarga de evitar.
   */
  get grips(): { v: "top" | "bottom"; h: "left" | "right" } {
    switch (this.anchor?.side) {
      case "bottom":
        return { v: "top", h: "right" };
      case "right":
        return { v: "bottom", h: "left" };
      // `top` (lo habitual: el globo cuelga de la pill) y `left`.
      default:
        return { v: "bottom", h: "right" };
    }
  }

  /** Lo que va en el `style` del globo. */
  get vars(): string {
    const a = this.anchor;
    return `--tail: ${a?.offset ?? 40}px; --x: ${a?.x ?? 0}px; --y: ${
      a?.y ?? 0
    }px; --w: ${a?.w ?? 580}px; --h: ${a?.h ?? 520}px`;
  }
}
