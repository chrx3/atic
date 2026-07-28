/**
 * La geometría del globo anclado a la pill.
 *
 * # Por qué vive aparte
 *
 * En `routes/agents/+page.svelte` convivía con el protocolo y con el render, y
 * no tiene nada que ver con ninguno de los dos: acá no se sabe qué es un agente
 * ni qué es un turno. Es solo dónde está la burbuja, de qué lado le sale la
 * punta y si ya se puede mostrar.
 *
 * # La conversión que hay que hacer una sola vez
 *
 * Rust razona en píxeles **físicos** —es lo que usa Win32— y el CSS en lógicos.
 * A escala 100% son el mismo número y la diferencia no se ve; a 125% el
 * contenido quedaba un 25% más ancho que su ventana y se recortaba por los dos
 * lados. La división por `devicePixelRatio` pasa **una vez**, acá, y no
 * repartida por reglas de estilo.
 */
import { tick } from "svelte";
import type { BubbleOpen } from "$lib/api";

/** Cómo salió la burbuja, ya en píxeles de CSS. */
export interface Anchor {
  side: string;
  offset: number;
  w: number;
  h: number;
}

export class Bubble {
  anchor = $state<Anchor | null>(null);
  /** Ya se puede pintar: antes de esto no se sabe dónde va la punta. */
  shown = $state(false);
  /**
   * La arrastraste: la punta deja de dibujarse.
   *
   * Movida de sitio ya no sale de la pill, y una punta que apunta a donde la
   * pill no está es peor que ninguna. Al volver a abrirla se re-ancla y vuelve.
   */
  detached = $state(false);
  /** Duración del vuelo desde la pill, para fundir el contenido a la par. */
  flight = $state(0);

  /** Antes de este instante, los movimientos son del vuelo y no del usuario. */
  #settledAt = 0;

  /** Llegó de Rust: dónde cae, de qué lado y cuánto dura el vuelo. */
  place(a: BubbleOpen): void {
    const dpr = window.devicePixelRatio || 1;
    this.anchor = {
      side: a.side,
      offset: a.offset / dpr,
      w: a.w / dpr,
      h: a.h / dpr,
    };
    this.detached = false;
    // La ventana está volando desde la pill: el contenido se funde durante ese
    // mismo tiempo, así que crecer y aparecer son un solo gesto.
    this.flight = a.flight;
    this.shown = false;
    // El vuelo empieza YA; ignorar los `moved` que provoca, o la burbuja se
    // daría por arrastrada antes de terminar de abrirse.
    this.#settledAt = Date.now() + a.flight + 120;
    void tick().then(() => requestAnimationFrame(() => (this.shown = true)));
  }

  /**
   * La ventana se movió. Devuelve si contó como arrastre del usuario.
   *
   * Se mira el reloj y no el gesto: Windows también la mueve al acomodarla
   * contra un borde, y eso cuenta igual; lo que no cuenta es el vuelo con el
   * que se acaba de abrir.
   */
  moved(): boolean {
    if (Date.now() < this.#settledAt) return false;
    this.detached = true;
    return true;
  }

  /**
   * El usuario la estiró: el contenido tiene que seguir a la ventana.
   *
   * Lo aplica la vista y no un evento de vuelta de Rust: durante el arrastre
   * son decenas de cambios por segundo, y esperar el ida y vuelta dejaría el
   * contenido corriendo detrás del borde. Rust mueve la ventana; esto mueve lo
   * que hay adentro, con el mismo número.
   */
  resized(w: number, h: number): void {
    if (!this.anchor) return;
    this.anchor = { ...this.anchor, w, h };
  }

  /** Apaga el contenido. Mover y ocultar la ventana es cosa de Rust. */
  hide(): void {
    this.shown = false;
  }

  /**
   * De qué lados se agarra para redimensionar.
   *
   * De los dos OPUESTOS a la punta, siempre. Arrastrar por el lado anclado
   * despegaría el globo de la pill mientras lo estirás, que es justo lo que la
   * geometría de Rust se encarga de evitar. Las agarraderas viven en el margen
   * de la sombra (`--inset`), así que no pisan ni la franja de arrastre ni el
   * botón de cerrar.
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
    // Los respaldos son el tamaño de la VENTANA, no el del globo: `.bub` mide
    // `--w` con el marco de la sombra incluido. Solo se usan en el instante
    // previo al primer ancla, cuando la burbuja todavía no se pinta.
    return `--tail: ${this.anchor?.offset ?? 40}px; --fade: ${
      this.flight || 200
    }ms; --w: ${this.anchor?.w ?? 704}px; --h: ${this.anchor?.h ?? 644}px`;
  }
}
