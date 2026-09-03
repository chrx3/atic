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
  import { t } from "$domain/i18n.svelte";

  /** Menos que esto y el arrastre fue un temblor: cuenta como clic. */
  const DRAG_THRESHOLD = 4;

  /** Cierre del overlay antes de disparar: más rápido que la entrada. */
  const FADE_MS = () => ms(MOTION.quick);

  /** Si en este tiempo el frame no se reveló, se cierra la sesión. */
  const WATCHDOG_MS = 5000;

  type Rect = { left: number; top: number; width: number; height: number };

  let frameSrc = $state("");
  let frameW = $state(1);
  let frameH = $state(1);
  let frameEl: HTMLImageElement | undefined = $state();
  let candidates = $state<OverlayCandidate[]>([]);
  let revealed = $state(false);
  let hovered = $state<OverlayCandidate | null>(null);
  let region = $state<Rect | null>(null);
  /** Posición del mouse. No es `$state`: el puntero se mueve por DOM. */
  let cursor = { x: 0, y: 0 };
  let pointerEl: HTMLDivElement | undefined;
  let helpEl: HTMLDivElement | undefined;

  let dragging = false;
  let dragStartClient = { x: 0, y: 0 };
  let dragStartFrame = { x: 0, y: 0 };
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

  const holeStyle = $derived(
    selection
      ? `left:${pct(selection.left, frameW)}%; top:${pct(selection.top, frameH)}%;
         width:${pct(selection.width, frameW)}%; height:${pct(selection.height, frameH)}%;`
      : "",
  );

  const sizeStyle = $derived(
    selection
      ? `left:${pct(selection.left, frameW)}%; top:${pct(selection.top, frameH)}%;`
      : "",
  );

  function pct(value: number, total: number): number {
    return total > 0 ? (value / total) * 100 : 0;
  }

  /**
   * Mouse CSS → píxel del PNG. El recuadro se dibuja en % de la imagen, así
   * que queda pegado al contenido congelado aunque WebView2 y el DPI no
   * coincidan con `scale_factor`.
   */
  function toFrame(clientX: number, clientY: number): { x: number; y: number } {
    const el = frameEl;
    const nw = el && el.naturalWidth > 0 ? el.naturalWidth : frameW;
    const nh = el && el.naturalHeight > 0 ? el.naturalHeight : frameH;
    const r = el?.getBoundingClientRect();
    const w = r && r.width > 0 ? r.width : window.innerWidth;
    const h = r && r.height > 0 ? r.height : window.innerHeight;
    const left = r?.left ?? 0;
    const top = r?.top ?? 0;
    return {
      x: ((clientX - left) * nw) / w,
      y: ((clientY - top) * nh) / h,
    };
  }

  function reset() {
    token += 1;
    done = true;
    revealed = false;
    region = null;
    hovered = null;
    dragging = false;
    if (pointerEl) pointerEl.hidden = true;
    frameSrc = "";
    frameW = 1;
    frameH = 1;
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
    cursor.x = event.clientX;
    cursor.y = event.clientY;
    if (pointerEl) {
      pointerEl.hidden = false;
      pointerEl.style.transform = `translate(${cursor.x}px, ${cursor.y}px)`;
    }
    if (helpEl) helpEl.style.left = `${cursor.x}px`;
    const point = toFrame(event.clientX, event.clientY);
    if (!dragging) {
      hovered = hitTest(point.x, point.y);
      return;
    }
    const cssW = Math.abs(event.clientX - dragStartClient.x);
    const cssH = Math.abs(event.clientY - dragStartClient.y);
    if (cssW > DRAG_THRESHOLD || cssH > DRAG_THRESHOLD) {
      region = {
        left: Math.min(dragStartFrame.x, point.x),
        top: Math.min(dragStartFrame.y, point.y),
        width: Math.abs(point.x - dragStartFrame.x),
        height: Math.abs(point.y - dragStartFrame.y),
      };
      hovered = null;
    }
  }

  function onMouseDown(event: MouseEvent) {
    if (!revealed || event.button !== 0) return;
    dragging = true;
    dragStartClient = { x: event.clientX, y: event.clientY };
    dragStartFrame = toFrame(event.clientX, event.clientY);
    region = null;
  }

  function onMouseUp(event: MouseEvent) {
    if (done || !revealed) return;
    const wasDragging = dragging;
    const rect = region;
    dragging = false;

    if (wasDragging && rect) {
      void shoot(() =>
        completeRegionCapture(rect.left, rect.top, rect.width, rect.height),
      );
      return;
    }

    region = null;
    const point = toFrame(event.clientX, event.clientY);
    const target = hitTest(point.x, point.y);
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
      const point = toFrame(cursor.x, cursor.y);
      void shoot(() => completeMonitorCapture(point.x, point.y));
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
    frameW = 1;
    frameH = 1;

    try {
      const info = await overlayInfo();
      if (mine !== token) return;
      candidates = info.candidates;
      frameW = Math.max(1, info.width);
      frameH = Math.max(1, info.height);
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
      bind:this={frameEl}
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
         alrededor: así el agujero sigue al recuadro sin cuentas. Posición en
         % del PNG: queda alineado al contenido congelado, no al CSS del
         webview. -->
    <div class="cap-hole" style={holeStyle}></div>
    <div class="cap-size" data-numeric style={sizeStyle}>
      {Math.round(selection.width)} × {Math.round(selection.height)}
    </div>
  {:else if revealed}
    <div class="cap-scrim"></div>
  {/if}

  <!-- La ayuda sigue al cursor en horizontal: en dos monitores, fijarla al
       centro la deja en la otra pantalla. -->
  <div class="cap-help" bind:this={helpEl}>
    {t("page.captureHud.help")}
  </div>

  <div class="cap-pointer" bind:this={pointerEl} hidden aria-hidden="true">
    <svg width="18" height="24" viewBox="0 0 18 24" fill="none">
      <path
        d="M1.2 1.2 1.2 20.2 6.1 15.4 9.4 23.1 12.2 21.9 8.8 14.1 16.2 13.9Z"
        fill="#fff"
        stroke="#111"
        stroke-width="1.4"
        stroke-linejoin="round"
      />
    </svg>
  </div>
</div>

<style>
  /* La mira la dibujamos nosotros: el cursor del SO queda detrás del PNG
     congelado (WebView2 a pantalla completa / escritorio virtual). */
  :global(html),
  :global(body) {
    overflow: hidden;
    margin: 0;
    background: var(--screen-backdrop);
    cursor: none !important;
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
    cursor: none;
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
    object-fit: fill;
    cursor: none;
  }

  .cap-pointer {
    pointer-events: none;
    position: fixed;
    left: 0;
    top: 0;
    z-index: 5;
    will-change: transform;
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
    transform: translateY(calc(-100% - 4px));
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
