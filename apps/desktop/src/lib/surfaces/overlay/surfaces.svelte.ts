/**
 * Qué partes del overlay reciben el mouse.
 *
 * El overlay cubre el monitor entero, así que por defecto es click-through: si
 * no, sería una lámina invisible tapando el escritorio. Rust le pregunta a este
 * registro dónde están las zonas vivas y arma la ventana solo cuando el cursor
 * entra a una.
 *
 * Cada superficie se registra con su elemento y el registro se encarga del
 * resto. Nadie calcula coordenadas a mano: `getBoundingClientRect()` ya da
 * píxeles CSS relativos al viewport, y el viewport ES el overlay.
 */

import { setOverlayHitRects, type HitRect } from "$ipc/overlay";

/** El `id` viaja a Rust: necesita distinguir la pill, que es de donde cuelga
 *  la burbuja de agentes. */
type Rect = HitRect;

class OverlaySurfaces {
  /**
   * Lo último que se midió de cada superficie.
   *
   * Lo lee quien tiene que dibujar CONTRA otra: el cuello de la burbuja sale
   * de la silueta de la pill, y sin esto tendría que volver a medirla por su
   * cuenta —con otro observador y otro reloj— para llegar al mismo número que
   * acá ya se calculó.
   */
  live = $state<Record<string, Rect>>({});

  #els = new Map<string, HTMLElement>();
  #observer: ResizeObserver | null = null;
  #frame = 0;
  #dragging = false;
  /** Último envío, para no repetir el mismo IPC en cada frame. */
  #sent = "";

  /**
   * Registra una superficie. Devuelve la función para darla de baja, con la
   * forma que espera el `return` de un `$effect`.
   */
  add(id: string, el: HTMLElement): () => void {
    this.#els.set(id, el);
    this.#observe(el);
    this.schedule();
    return () => {
      this.#els.delete(id);
      this.#observer?.unobserve(el);
      this.schedule();
    };
  }

  /**
   * Mientras se arrastra, todo el overlay recibe el mouse.
   *
   * Sin esto, mover rápido una superficie deja al puntero fuera de su
   * rectángulo, Rust desarma la ventana a mitad del gesto y el arrastre se
   * corta solo. Publicar la pantalla entera es más simple que perseguir la
   * forma cuadro a cuadro, y dura lo que dura el gesto.
   */
  set dragging(value: boolean) {
    if (this.#dragging === value) return;
    this.#dragging = value;
    this.schedule();
  }

  get dragging(): boolean {
    return this.#dragging;
  }

  /** Reagrupa por frame: mover una superficie dispara muchas mediciones. */
  schedule(): void {
    if (this.#frame) return;
    this.#frame = requestAnimationFrame(() => {
      this.#frame = 0;
      void this.#publish();
    });
  }

  #observe(el: HTMLElement): void {
    if (!this.#observer) {
      this.#observer = new ResizeObserver(() => this.schedule());
    }
    this.#observer.observe(el);
  }

  async #publish(): Promise<void> {
    // Medir SIEMPRE por superficie, aunque a Rust vaya otra cosa: durante un
    // arrastre se le manda un rectángulo de pantalla completa, y si `live`
    // saliera de ahí, la burbuja perdería de vista a la pill justo mientras se
    // la lleva.
    const measured: Rect[] = [...this.#els.entries()]
      .map(([id, el]) => ({ id, r: el.getBoundingClientRect() }))
      .filter(({ r }) => r.width > 0 && r.height > 0)
      .map(({ id, r }) => ({ id, x: r.x, y: r.y, w: r.width, h: r.height }));

    const rects: Rect[] = this.#dragging
      ? [{ id: "drag", x: 0, y: 0, w: window.innerWidth, h: window.innerHeight }]
      : measured;

    // Comparar antes de mandar: el `ResizeObserver` y los efectos disparan
    // mucho más seguido de lo que la geometría cambia de verdad.
    const key = JSON.stringify(measured) + (this.#dragging ? "!" : "");
    if (key === this.#sent) return;
    this.#sent = key;
    this.live = Object.fromEntries(measured.map((r) => [r.id, r]));

    try {
      await setOverlayHitRects(rects);
    } catch {
      // Fuera de Tauri (dev en navegador) no hay a quién avisarle.
    }
  }
}

export const surfaces = new OverlaySurfaces();

/**
 * Atajo para usar desde un componente:
 *
 * ```svelte
 * <div bind:this={el}>…</div>
 * <script>$effect(() => (el ? liveArea("pill", el) : undefined));</script>
 * ```
 */
export function liveArea(id: string, el: HTMLElement): () => void {
  return surfaces.add(id, el);
}
