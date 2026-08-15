/**
 * Qué partes del overlay reciben el mouse.
 *
 * El overlay cubre el monitor entero, así que por defecto es click-through: si
 * no, sería una lámina invisible tapando el escritorio. Rust le pregunta a este
 * registro dónde están las zonas vivas y arma la ventana solo cuando el cursor
 * entra a una.
 *
 * Cada superficie se registra con su elemento y el registro se encarga del
 * resto. Se mide con `getBoundingClientRect` (coords CSS del viewport). Rust
 * guarda esos CSS y arma con `ScreenToClient` — mismo espacio, también en el
 * 2º monitor. Si hay transform de emerge, se usa la caja de **layout**
 * (`left`/`top` + `offsetWidth/Height`): el scale de `.float-emerge` no mueve
 * `left`/`top`, y `ResizeObserver` no dispara al terminar el morph, así que
 * publicar el bounding visual dejaba el hit-rect corrido para siempre (clics
 * “pasan de largo” al app de atrás).
 */

import { setOverlayHitRects, setOverlayItemDrag, setOverlayPointerGesture, type HitRect } from "$ipc/overlay";

/** El `id` viaja a Rust: necesita distinguir la pill, que es de donde cuelga
 *  la burbuja de agentes. */
type Rect = HitRect;

/** Lee un custom property **inline** en px (`12px` → 12). */
function inlinePx(el: HTMLElement, prop: string): number | null {
  const raw = el.style.getPropertyValue(prop).trim();
  if (!raw) return null;
  const n = Number.parseFloat(raw);
  return Number.isFinite(n) ? n : null;
}

/**
 * Caja de layout en coords CSS del viewport (para hit-rects).
 *
 * Con `.float-emerge`, el origen del scale no es top-left: el bounding visual
 * queda corrido respecto a `left`/`top`. Los clics caen en la caja de layout
 * (tras `pointer-events: auto`), así que el hit-rect tiene que coincidir con
 * esa caja, no con la silueta escalada.
 *
 * Los floats de bubble publican `--x/--y/--w/--h` **en el root** (mismo
 * espacio que el overlay). Hay que leerlos del `style` inline, no de
 * `getComputedStyle`: las custom properties se heredan, y un hijo (p. ej.
 * las bolitas de favoritos a la derecha de la barra) publicaría la caja del
 * padre — clics “pasan de largo” justo donde está el control.
 */
export function layoutRect(el: HTMLElement): {
  x: number;
  y: number;
  w: number;
  h: number;
} {
  const cs = getComputedStyle(el);
  const varX = inlinePx(el, "--x");
  const varY = inlinePx(el, "--y");
  const varW = inlinePx(el, "--w");
  const varH = inlinePx(el, "--h");
  const ow = (varW && varW > 0 ? varW : el.offsetWidth) || 0;
  const oh = (varH && varH > 0 ? varH : el.offsetHeight) || 0;

  if (varX != null && varY != null && ow > 0 && oh > 0) {
    // `--x/--y` ya están en coords del overlay (= viewport del webview).
    return { x: varX, y: varY, w: ow, h: oh };
  }

  const transformed = cs.transform !== "none";
  if (
    (transformed || cs.position === "absolute" || cs.position === "fixed") &&
    cs.left !== "auto" &&
    cs.top !== "auto" &&
    ow > 0 &&
    oh > 0
  ) {
    const parent = el.offsetParent as HTMLElement | null;
    const origin = parent?.getBoundingClientRect();
    const x = (origin?.left ?? 0) + (Number.parseFloat(cs.left) || 0);
    const y = (origin?.top ?? 0) + (Number.parseFloat(cs.top) || 0);
    return { x, y, w: ow, h: oh };
  }

  const r = el.getBoundingClientRect();
  // Scale transform sin left/top usable: conservar tamaño de layout anclado
  // al top-left visual (mejor que la zona chica cerca de la pill).
  if (
    ow > 0 &&
    oh > 0 &&
    (Math.abs(r.width - ow) > 1 || Math.abs(r.height - oh) > 1)
  ) {
    return { x: r.x, y: r.y, w: ow, h: oh };
  }
  return {
    x: r.x,
    y: r.y,
    w: r.width || ow,
    h: r.height || oh,
  };
}

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
  /**
   * Reactivo a propósito: los floats leen esto para sacar su silueta del goo
   * y apagar filtros de emerge durante el gesto. Si fuera un campo opaco, el
   * `$effect` no se enteraría al soltar.
   */
  #dragging = $state(false);
  /** Si el pointerup se pierde, el hit-rect fullscreen no puede quedar eterno. */
  #dragWatchdog = 0;
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
      el.removeEventListener("transitionend", this.#onTransitionEnd);
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
    this.#armDragWatchdog(value);
    // Al soltar hay que volver a medir: durante el drag `#publish` deja de
    // actualizar `live` y el hit-rect queda en pantalla completa.
    if (!value) this.#sent = "";
    // Atomic en Rust YA: el hit-rect fullscreen viaja por rAF+IPC y llega
    // tarde. En ese hueco el overlay se desarma y se pierde el pointerup.
    void setOverlayPointerGesture(value).catch(() => {});
    if (value) void this.flush();
    else this.schedule();
  }

  #armDragWatchdog(on: boolean) {
    if (this.#dragWatchdog) {
      clearTimeout(this.#dragWatchdog);
      this.#dragWatchdog = 0;
    }
    if (!on || typeof window === "undefined") return;
    this.#dragWatchdog = window.setTimeout(() => this.resetInteraction(), 15_000);
  }

  get dragging(): boolean {
    return this.#dragging;
  }

  /**
   * Recuperación: drag a medias / hit-rect fullscreen pegado / OLE passthrough.
   * Esc y dismiss del overlay lo llaman para volver a click-through normal.
   */
  resetInteraction(): void {
    this.dragging = false;
    // Tras OLE out-drag Rust vació HIT_RECTS; si `#sent` sigue igual, el
    // próximo publish no reenvía y el overlay queda muerto al mouse.
    this.#sent = "";
    this.schedule();
    void setOverlayItemDrag(false).catch(() => {});
    void setOverlayPointerGesture(false).catch(() => {});
  }

  /**
   * Fuerza republicar hit-rects aunque la geometría no haya cambiado.
   * Obligatorio al salir de `set_overlay_item_drag(true)` (Rust limpió la lista).
   */
  async recoverHits(): Promise<void> {
    this.#sent = "";
    await this.flush();
  }

  /** Reagrupa por frame: mover una superficie dispara muchas mediciones. */
  schedule(): void {
    if (this.#frame) return;
    this.#frame = requestAnimationFrame(() => {
      this.#frame = 0;
      void this.#publish();
    });
  }

  /**
   * Publica ya (sin esperar al rAF). Antes de abrir un float: si el hit-rect
   * de la pill aún no llegó a Rust, el ancla sale con geometría vieja.
   */
  async flush(): Promise<void> {
    if (this.#frame) {
      cancelAnimationFrame(this.#frame);
      this.#frame = 0;
    }
    await this.#publish();
  }

  #onTransitionEnd = (event: TransitionEvent) => {
    // Solo el root registrado (no hijos): emerge usa transform/opacity.
    if (event.target !== event.currentTarget) return;
    if (
      event.propertyName !== "transform" &&
      event.propertyName !== "opacity"
    ) {
      return;
    }
    this.schedule();
  };

  #observe(el: HTMLElement): void {
    if (!this.#observer) {
      this.#observer = new ResizeObserver(() => this.schedule());
    }
    this.#observer.observe(el);
    // El morph de `.float-emerge` no cambia layout: sin esto el hit-rect
    // medido a mitad del scale queda congelado (ResizeObserver no dispara).
    el.addEventListener("transitionend", this.#onTransitionEnd);
  }

  async #publish(): Promise<void> {
    // Durante el drag: un solo hit-rect a pantalla completa. Remedir `live` e
    // IPC por frame era costo dominante (layout + stringify + invoke). Los
    // floats además se sacan del goo mientras dura el gesto.
    if (this.#dragging) {
      if (this.#sent === "drag!") return;
      this.#sent = "drag!";
      try {
        await setOverlayHitRects([
          {
            id: "drag",
            x: 0,
            y: 0,
            w: window.innerWidth,
            h: window.innerHeight,
          },
        ]);
      } catch {
        // Fuera de Tauri (dev en navegador) no hay a quién avisarle.
      }
      return;
    }

    // Medir por superficie: `live` lo leen cuellos / placeBesidePill.
    const measured: Rect[] = [...this.#els.entries()]
      .map(([id, el]) => ({ id, r: layoutRect(el) }))
      .filter(({ r }) => r.w > 0 && r.h > 0)
      .map(({ id, r }) => ({ id, x: r.x, y: r.y, w: r.w, h: r.h }));

    // Comparar antes de mandar: el `ResizeObserver` y los efectos disparan
    // mucho más seguido de lo que la geometría cambia de verdad.
    const key = JSON.stringify(measured);
    if (key === this.#sent) return;
    this.#sent = key;
    this.live = Object.fromEntries(measured.map((r) => [r.id, r]));

    try {
      await setOverlayHitRects(measured);
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
