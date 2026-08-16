<script lang="ts">
  /**
   * Elegir qué capturar: clic en una ventana, arrastre para una región,
   * espacio para la pantalla entera.
   *
   * No dibuja el escritorio en vivo: Rust congela un PNG antes de mostrar esta
   * ventana y acá se pinta esa foto. Sin el congelado, cualquier animación de
   * abajo —un cursor que parpadea, un video— seguiría moviéndose bajo la
   * selección, y la captura no coincidiría con lo que se eligió.
   *
   * Casi todo lo delicado de este archivo es sobre NO dejar la pantalla tapada:
   * la ventana es opaca y a pantalla completa, así que cualquier fallo sin
   * salida deja al usuario sin poder usar el PC. De ahí el watchdog, el cierre
   * con Escape aunque el frame no haya cargado, y el cierre si la imagen falla.
   */
  import type { OverlayCandidate } from "$core/types";
  import { MOTION, ms } from "$lib/motion";
  import {
    cancelCaptureSession,
    captureSrc,
    completeMonitorCapture,
    completeRegionCapture,
    completeWindowCapture,
    overlayInfo,
    showCaptureOverlay,
    captureOverlayRevealed,
  } from "$ipc/captures";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { on } from "$ipc/events";

  /** Menos que esto y el arrastre fue un temblor: cuenta como clic. */
  const DRAG_THRESHOLD = 4;

  /** Cierre del overlay antes de disparar: más rápido que la entrada. */
  const FADE_MS = () => ms(MOTION.quick);

  /** Si en este tiempo el frame no se reveló, se cierra la sesión. */
  const WATCHDOG_MS = 5000;

  type Rect = { left: number; top: number; width: number; height: number };

  let frameSrc = $state("");
  let candidates = $state<OverlayCandidate[]>([]);
  let revealed = $state(false);
  let hovered = $state<OverlayCandidate | null>(null);
  let region = $state<Rect | null>(null);
  let cursor = $state({ x: 0, y: 0 });

  let dragging = false;
  let dragStart = { x: 0, y: 0 };
  let done = false;

  /** Sube con cada sesión: descarta lo que quedó en vuelo de la anterior. */
  let token = 0;

  const selection = $derived<Rect | null>(
    region ??
      (hovered
        ? {
            left: hovered.left,
            top: hovered.top,
            width: hovered.width,
            height: hovered.height,
          }
        : null),
  );

  function reset() {
    token += 1;
    done = true;
    revealed = false;
    region = null;
    hovered = null;
    dragging = false;
    frameSrc = "";
    candidates = [];
  }

  async function close() {
    try {
      await cancelCaptureSession();
    } catch {
      // Ya cerrada. No hay nada que hacer y nadie a quien avisar: la ventana
      // se va igual.
    }
  }

  /** Dispara una captura, dejando antes que el overlay se desvanezca. */
  async function shoot(action: () => Promise<unknown>) {
    if (done) return;
    done = true;
    revealed = false;
    await new Promise((resolve) => setTimeout(resolve, FADE_MS()));
    try {
      await action();
    } catch {
      await close();
    }
  }

  /**
   * Qué ventana hay bajo el cursor.
   *
   * La lista viene de Rust ordenada de arriba abajo en el z-order, así que la
   * primera que contiene el punto es la que se ve.
   */
  function hitTest(x: number, y: number): OverlayCandidate | null {
    return (
      candidates.find(
        (c) =>
          x >= c.left && x < c.left + c.width && y >= c.top && y < c.top + c.height,
      ) ?? null
    );
  }

  function onMouseMove(event: MouseEvent) {
    cursor = { x: event.clientX, y: event.clientY };
    if (!dragging) {
      hovered = hitTest(event.clientX, event.clientY);
      return;
    }
    const width = Math.abs(event.clientX - dragStart.x);
    const height = Math.abs(event.clientY - dragStart.y);
    if (width > DRAG_THRESHOLD || height > DRAG_THRESHOLD) {
      region = {
        left: Math.min(dragStart.x, event.clientX),
        top: Math.min(dragStart.y, event.clientY),
        width,
        height,
      };
      hovered = null;
    }
  }

  function onMouseDown(event: MouseEvent) {
    if (!revealed || event.button !== 0) return;
    dragging = true;
    dragStart = { x: event.clientX, y: event.clientY };
    region = null;
  }

  function onMouseUp(event: MouseEvent) {
    if (done || !revealed) return;
    const wasDragging = dragging;
    const rect = region;
    dragging = false;

    if (
      wasDragging &&
      rect &&
      (rect.width > DRAG_THRESHOLD || rect.height > DRAG_THRESHOLD)
    ) {
      void shoot(() =>
        completeRegionCapture(rect.left, rect.top, rect.width, rect.height),
      );
      return;
    }

    region = null;
    const target = hitTest(event.clientX, event.clientY);
    // Un clic en el vacío cancela: no hay ventana ahí y arrastrar nada tampoco
    // significa nada.
    void shoot(() => (target ? completeWindowCapture(target.hwnd) : close()));
  }

  function onKeydown(event: KeyboardEvent) {
    if (done) return;

    // Escape SIEMPRE cierra, aunque el frame no haya cargado: si no, un fallo
    // de carga deja la pantalla tapada por una ventana opaca sin salida.
    if (event.key === "Escape") {
      void shoot(close);
      return;
    }
    if (!revealed) return;

    if (event.key === " ") {
      event.preventDefault();
      void shoot(() => completeMonitorCapture(cursor.x, cursor.y));
    } else if (event.key === "Enter") {
      const rect = region;
      const target = hovered;
      if (rect) {
        void shoot(() =>
          completeRegionCapture(rect.left, rect.top, rect.width, rect.height),
        );
      } else if (target) {
        void shoot(() => completeWindowCapture(target.hwnd));
      }
    }
  }

  /**
   * El frame ya está pintado: recién ahora se puede mostrar la ventana.
   *
   * Orden: pintar a opacidad 0 → mostrar la ventana nativa → recién entonces
   * marcar `revealed`. Si se revela antes del `show()`, la transición termina
   * oculta y el modo captura aparece de golpe.
   */
  function onFrameLoad() {
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        if (done) return;
        void showCaptureOverlay()
          .then(() => {
            requestAnimationFrame(() => {
              if (done) return;
              revealed = true;
              // Recién acá la selección es usable: avisarle a Rust para que
              // no cancele por watchdog. Si el ack se pierde, lo peor que
              // pasa es que la sesión se cierre sola a los 6 s.
              void captureOverlayRevealed().catch(() => {});
            });
          })
          .catch(() => {
            done = true;
            void close();
          });
      });
    });
  }

  /** El PNG congelado no cargó: cerrar en vez de tapar la pantalla. */
  function onFrameError() {
    if (done) return;
    done = true;
    void close();
  }

  /**
   * Arranca una sesión.
   *
   * Solo por evento: la ventana existe oculta desde que arranca la app, y al
   * montar todavía no hay ningún frame congelado que mostrar.
   */
  async function start() {
    const mine = ++token;
    done = false;
    revealed = false;
    region = null;
    hovered = null;
    dragging = false;
    frameSrc = "";

    try {
      const info = await overlayInfo();
      if (mine !== token) return;
      candidates = info.candidates;
      // El sufijo evita que el webview sirva el frame de la sesión anterior.
      frameSrc = `${captureSrc(info.framePath)}?t=${Date.now()}`;

      setTimeout(() => {
        if (mine === token && !revealed && !done) {
          done = true;
          void close();
        }
      }, WATCHDOG_MS);
    } catch {
      revealed = false;
      await close();
    }
  }

  $effect(() => {
    const pending = Promise.all([
      on("overlay-session-started", () => void start()),
      on("overlay-session-ended", () => {
        // Un «ended» de la sesión anterior puede llegar tarde, después de que
        // ya arrancó otra. Se pregunta antes de tumbar nada.
        void overlayInfo().catch(() => reset());
      }),
      getCurrentWindow().onFocusChanged(({ payload }) => {
        if (!payload || revealed || frameSrc) return;
        void overlayInfo()
          .then(() => start())
          .catch(() => {
            // Sin sesión: el foco llegó por otra razón.
          });
      }),
    ]);

    // Si el atajo se apretó antes de que este webview escuchara, ya hay sesión.
    void overlayInfo()
      .then(() => start())
      .catch(() => {
        // Todavía no hay sesión: es lo normal al arrancar la app.
      });

    return () => void pending.then((offs) => offs.forEach((off) => off()));
  });
</script>

<svelte:window onkeydown={onKeydown} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="cap"
  class:is-revealed={revealed}
  onmousemove={onMouseMove}
  onmousedown={onMouseDown}
  onmouseup={onMouseUp}
  oncontextmenu={(event) => {
    event.preventDefault();
    if (revealed) void shoot(close);
  }}
>
  {#if frameSrc}
    <img
      src={frameSrc}
      alt=""
      draggable="false"
      class="cap-frame"
      onload={onFrameLoad}
      onerror={onFrameError}
    />
  {/if}

  {#if selection}
    <!-- El velo es una sombra gigante hacia afuera en vez de cuatro divs
         alrededor: así el agujero sigue al recuadro sin cuentas. -->
    <div
      class="cap-hole"
      style="left:{selection.left}px; top:{selection.top}px;
             width:{selection.width}px; height:{selection.height}px;"
    ></div>
    <div
      class="cap-size"
      data-numeric
      style="left:{selection.left}px; top:{Math.max(2, selection.top - 26)}px;"
    >
      {Math.round(selection.width)} × {Math.round(selection.height)}
    </div>
  {:else if revealed}
    <div class="cap-scrim"></div>
  {/if}

  <!-- La ayuda sigue al cursor en horizontal: en dos monitores, fijarla al
       centro la deja en la otra pantalla. -->
  <div class="cap-help" style="left:{cursor.x}px;">
    Clic: ventana · Arrastrar: región · Espacio: pantalla · Esc cancela
  </div>
</div>

<style>
  /* La mira en cruz dice que lo que se está haciendo es apuntar. El fondo se
     ve solo hasta que carga la foto congelada. */
  :global(html),
  :global(body) {
    overflow: hidden;
    margin: 0;
    background: var(--screen-backdrop);
    cursor: crosshair;
  }

  /*
   * Activar captura: entrada deliberada (open lento), salida más rápida.
   * Solo opacity en el root (un PNG a pantalla completa + blur es caro).
   * CSS transition para poder interrumpir con Esc.
   */
  .cap {
    position: fixed;
    inset: 0;
    width: 100vw;
    height: 100vh;
    overflow: hidden;
    user-select: none;
    opacity: 0;
    transition: opacity var(--duration-quick, 150ms)
      var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1));
  }

  .cap.is-revealed {
    opacity: 1;
    transition: opacity var(--duration-slow, 400ms)
      var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1));
  }

  .cap-frame {
    position: absolute;
    inset: 0;
    display: block;
    width: 100%;
    height: 100%;
  }

  .cap-hole {
    pointer-events: none;
    position: absolute;
    box-sizing: border-box;
    border: 2px solid var(--screen-select, #2f9e44);
    box-shadow: 0 0 0 100000px var(--screen-scrim);
  }

  .cap-size {
    pointer-events: none;
    position: absolute;
    border-radius: var(--rb-radius-xs, 5px);
    background: var(--screen-chip);
    padding: 2px 6px;
    font-family: var(--rb-mono, ui-monospace, monospace);
    font-size: 12px;
    white-space: nowrap;
    color: var(--screen-ink, #fff);
  }

  .cap-scrim {
    pointer-events: none;
    position: absolute;
    inset: 0;
    background: var(--screen-scrim);
  }

  .cap-help {
    pointer-events: none;
    position: fixed;
    bottom: 2rem;
    z-index: 2;
    border-radius: var(--rb-radius-sm, 8px);
    background: var(--screen-chip);
    padding: 6px 14px;
    font-size: 14px;
    white-space: nowrap;
    color: var(--screen-ink, #fff);
    box-shadow: 0 8px 24px rgb(0 0 0 / 28%);
    outline: 1px solid rgb(255 255 255 / 10%);
    outline-offset: -1px;
    opacity: 0;
    transform: translateX(-50%) translateY(var(--distance-base, 8px));
    transition:
      opacity var(--duration-fast, 250ms) var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1)),
      transform var(--duration-fast, 250ms) var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1));
    transition-delay: 0ms;
  }

  .cap.is-revealed .cap-help {
    opacity: 1;
    transform: translateX(-50%) translateY(0);
    transition-delay: var(--duration-micro, 80ms);
  }

  @media (prefers-reduced-motion: reduce) {
    .cap,
    .cap.is-revealed,
    .cap-help,
    .cap.is-revealed .cap-help {
      transition: none !important;
      transform: none !important;
    }

    .cap {
      opacity: 0;
    }

    .cap.is-revealed {
      opacity: 1;
    }

    .cap.is-revealed .cap-help {
      opacity: 1;
      transform: translateX(-50%);
    }
  }
</style>
