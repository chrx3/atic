<script lang="ts">
  /**
   * Elegir qué capturar: clic en una ventana, arrastre para una región,
   * espacio para la pantalla entera.
   *
   * No dibuja el escritorio en vivo: Rust congela un JPEG antes de mostrar esta
   * ventana y acá se pinta esa foto. La captura final sí es PNG lossless. Sin el
   * congelado, cualquier animación de abajo —un cursor que parpadea, un video—
   * seguiría moviéndose bajo la selección, y no coincidiría con lo que se eligió.
   *
   * Casi todo lo delicado de este archivo es sobre NO dejar la pantalla tapada:
   * la ventana es opaca y a pantalla completa, así que cualquier fallo sin
   * salida deja al usuario sin poder usar el PC. De ahí el watchdog, el cierre
   * con Escape aunque el frame no haya cargado, y el cierre si la imagen falla.
   */
  import type {
    LandingRect,
    OverlayCandidate,
    OverlayInfo,
    OverlayMonitor,
  } from "$core/types";
  import { MOTION, afterTransition, ms, prefersReducedMotion, wait } from "$lib/motion";
  import {
    cancelCaptureSession,
    captureSrc,
    captureShelfLanding,
    completeCaptureFly,
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
  import { tick } from "svelte";

  /** Menos que esto y el arrastre fue un temblor: cuenta como clic. */
  const DRAG_THRESHOLD = 4;

  /** Cierre del overlay antes de disparar: a la par del velo. */
  const FADE_MS = () => ms(MOTION.fast);

  /** Despegue del recorte: más lento que el vuelo, para que se lea como objeto. */
  const LIFT_MS = 320;

  /** Vuelo al thumb del shelf. Corto, con blur en el tramo. */
  const FLY_MS = 150;

  /** Si en este tiempo el frame no se reveló, se cierra la sesión. */
  const WATCHDOG_MS = 5000;

  type Rect = { left: number; top: number; width: number; height: number };

  type FlyCard = Rect & {
    imgL: number;
    imgT: number;
    imgW: number;
    imgH: number;
    lifted: boolean;
    moving: boolean;
  };

  let frameSrc = $state("");
  let frameW = $state(1);
  let frameH = $state(1);
  let frameEl: HTMLImageElement | undefined = $state();
  let candidates = $state<OverlayCandidate[]>([]);
  let monitors = $state<OverlayMonitor[]>([]);
  let revealed = $state(false);
  let hovered = $state<OverlayCandidate | null>(null);
  let region = $state<Rect | null>(null);
  /** Posición del mouse. No es `$state`: el puntero se mueve por DOM. */
  let cursor = { x: 0, y: 0 };
  let pointerEl: HTMLDivElement | undefined;
  let helpEl: HTMLDivElement | undefined;
  let flyEl: HTMLDivElement | undefined = $state();
  let flying = $state(false);
  let fly = $state<FlyCard | null>(null);

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
    monitors = [];
    flying = false;
    fly = null;
  }

  async function close() {
    try {
      await cancelCaptureSession();
    } catch {
      // Ya cerrada. No hay nada que hacer y nadie a quien avisar: la ventana
      // se va igual.
    }
  }

  function waitFrames(n: number): Promise<void> {
    return new Promise((resolve) => {
      const step = (left: number) => {
        if (left <= 0) resolve();
        else requestAnimationFrame(() => step(left - 1));
      };
      requestAnimationFrame(() => step(n - 1));
    });
  }

  /** Mouse CSS del recorte, alineado al PNG congelado. */
  function clientRectFromFrame(rect: Rect): Rect {
    const r = frameEl?.getBoundingClientRect();
    const left = r?.left ?? 0;
    const top = r?.top ?? 0;
    const w = r && r.width > 0 ? r.width : window.innerWidth;
    const h = r && r.height > 0 ? r.height : window.innerHeight;
    return {
      left: left + (rect.left / frameW) * w,
      top: top + (rect.top / frameH) * h,
      width: (rect.width / frameW) * w,
      height: (rect.height / frameH) * h,
    };
  }

  /**
   * El JPEG entero dentro de la tarjeta, recortado por %: al achicar, el
   * recorte viaja con ella.
   */
  function cropPercents(rect: Rect): { l: number; t: number; w: number; h: number } {
    const fw = Math.max(rect.width / frameW, 0.0001);
    const fh = Math.max(rect.height / frameH, 0.0001);
    return {
      l: -(rect.left / frameW / fw) * 100,
      t: -(rect.top / frameH / fh) * 100,
      w: 100 / fw,
      h: 100 / fh,
    };
  }

  function fallbackLanding(from: Rect): LandingRect {
    const mon = hitMonitor(from.left + from.width / 2, from.top + from.height / 2) ?? {
      left: 0,
      top: 0,
      width: frameW,
      height: frameH,
    };
    const mapped = clientRectFromFrame({
      left: 0,
      top: 0,
      width: frameW,
      height: frameH,
    });
    const framePerCssX = frameW / Math.max(mapped.width, 1);
    const framePerCssY = frameH / Math.max(mapped.height, 1);
    const pad = (16 + 8) * framePerCssX;
    const tw = 192 * framePerCssX;
    const th = 120 * framePerCssY;
    return {
      left: mon.left + mon.width - pad - tw,
      top: mon.top + mon.height - pad - th,
      width: tw,
      height: th,
    };
  }

  async function playFly(from: Rect): Promise<void> {
    const start = clientRectFromFrame(from);
    const crop = cropPercents(from);
    fly = {
      left: start.left,
      top: start.top,
      width: start.width,
      height: start.height,
      imgL: crop.l,
      imgT: crop.t,
      imgW: crop.w,
      imgH: crop.h,
      lifted: false,
      moving: false,
    };
    flying = true;
    const landingP = captureShelfLanding(
      from.left,
      from.top,
      from.width,
      from.height,
    ).catch(() => fallbackLanding(from));
    await tick();
    await waitFrames(2);
    if (fly) fly.lifted = true;
    await wait(LIFT_MS);
    const dest = clientRectFromFrame(await landingP);
    if (fly) fly.moving = true;
    await tick();
    await waitFrames(2);
    if (fly) {
      fly.lifted = false;
      fly.left = dest.left;
      fly.top = dest.top;
      fly.width = dest.width;
      fly.height = dest.height;
    }
    await afterTransition(flyEl ?? null, "left", FLY_MS);
    await wait(ms(MOTION.micro));
  }

  /**
   * Dispara una captura. Si hay recorte, se levanta y vuela al shelf; Escape
   * y el clic vacío solo se desvanecen.
   */
  async function shoot(action: () => Promise<unknown>, flyFrom?: Rect) {
    if (done) return;
    done = true;

    const canFly =
      Boolean(flyFrom) &&
      !prefersReducedMotion() &&
      Boolean(frameEl) &&
      (flyFrom?.width ?? 0) > 1 &&
      (flyFrom?.height ?? 0) > 1;

    try {
      if (canFly && flyFrom) {
        const [saved] = await Promise.allSettled([action(), playFly(flyFrom)]);
        if (saved.status === "rejected") throw saved.reason;
        await completeCaptureFly();
      } else {
        revealed = false;
        await Promise.all([wait(FADE_MS()), action()]);
        if (flyFrom) await completeCaptureFly();
      }
    } catch {
      try {
        await completeCaptureFly();
      } catch {
        await close();
      }
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

  function hitMonitor(x: number, y: number): OverlayMonitor | null {
    return (
      monitors.find(
        (m) =>
          x >= m.left && x < m.left + m.width && y >= m.top && y < m.top + m.height,
      ) ?? null
    );
  }

  function onMouseMove(event: MouseEvent) {
    if (done || flying) return;
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
    if (done || flying || !revealed || event.button !== 0) return;
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
      void shoot(
        () => completeRegionCapture(rect.left, rect.top, rect.width, rect.height),
        rect,
      );
      return;
    }

    region = null;
    const point = toFrame(event.clientX, event.clientY);
    const target = hitTest(point.x, point.y);
    // Un clic en el vacío cancela: no hay ventana ahí y arrastrar nada tampoco
    // significa nada.
    if (!target) {
      void shoot(close);
      return;
    }
    void shoot(() => completeWindowCapture(target.hwnd), {
      left: target.left,
      top: target.top,
      width: target.width,
      height: target.height,
    });
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
      const mon = hitMonitor(point.x, point.y);
      const flyRect = mon ?? {
        left: 0,
        top: 0,
        width: frameW,
        height: frameH,
      };
      void shoot(() => completeMonitorCapture(point.x, point.y), flyRect);
    } else if (event.key === "Enter") {
      const rect = region;
      const target = hovered;
      if (rect) {
        void shoot(
          () => completeRegionCapture(rect.left, rect.top, rect.width, rect.height),
          rect,
        );
      } else if (target) {
        void shoot(() => completeWindowCapture(target.hwnd), {
          left: target.left,
          top: target.top,
          width: target.width,
          height: target.height,
        });
      }
    }
  }

  /**
   * El JPEG ya está pintado en el webview oculto: recién ahora se muestra.
   * Si se muestra antes, el usuario ve el fondo #111 un frame —el negro entre
   * el atajo y la foto congelada.
   */
  function onFrameLoad() {
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        if (done) return;
        void showCaptureOverlay()
          .then(() => {
            if (done) return;
            revealed = true;
            void captureOverlayRevealed().catch(() => {});
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
   * El evento trae el JPEG y las ventanas: no hay que volver a preguntarle a
   * Rust. overlayInfo queda para el montaje, si el atajo ganó la carrera.
   */
  async function start(info?: OverlayInfo) {
    const mine = ++token;
    done = false;
    revealed = false;
    region = null;
    hovered = null;
    dragging = false;
    flying = false;
    fly = null;

    try {
      const data = info ?? (await overlayInfo());
      if (mine !== token) return;
      candidates = data.candidates;
      monitors = data.monitors ?? [];
      frameW = Math.max(1, data.width);
      frameH = Math.max(1, data.height);
      // El sufijo evita que el webview sirva el frame de la sesión anterior.
      frameSrc = `${captureSrc(data.framePath)}?t=${Date.now()}`;

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
      on("overlay-session-started", (info) => void start(info)),
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
  class:is-flying={flying}
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

  {#if flying}
    <div class="cap-dim is-on">
      <div class="cap-scrim"></div>
    </div>
  {:else if selection}
    <!-- El velo es una sombra gigante hacia afuera en vez de cuatro divs
         alrededor: así el agujero sigue al recuadro sin cuentas. Posición en
         % del PNG: queda alineado al contenido congelado, no al CSS del
         webview. -->
    <div class="cap-dim" class:is-on={revealed}>
      <div class="cap-hole" style={holeStyle}>
        <span class="cap-v nw" aria-hidden="true"></span>
        <span class="cap-v ne" aria-hidden="true"></span>
        <span class="cap-v se" aria-hidden="true"></span>
        <span class="cap-v sw" aria-hidden="true"></span>
      </div>
      <div class="cap-meta" style={sizeStyle}>
        {#if hovered?.title}
          <span class="cap-name">{hovered.title}</span>
        {/if}
        <span class="cap-size" data-numeric>
          {Math.round(selection.width)} × {Math.round(selection.height)}
        </span>
      </div>
    </div>
  {:else if frameSrc}
    <div class="cap-dim" class:is-on={revealed}>
      <div class="cap-scrim"></div>
    </div>
  {/if}

  {#if fly}
    <div
      bind:this={flyEl}
      class="cap-fly"
      class:is-lifted={fly.lifted}
      class:is-moving={fly.moving}
      style="left:{fly.left}px;top:{fly.top}px;width:{fly.width}px;height:{fly.height}px"
    >
      <img
        src={frameSrc}
        alt=""
        draggable="false"
        style="left:{fly.imgL}%;top:{fly.imgT}%;width:{fly.imgW}%;height:{fly.imgH}%"
      />
    </div>
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

    /* Transparente: la ventana se muestra al empezar la sesión (para que el
       webview cargue) y hasta que el frame pinta no debe verse un telón
       negro tapando el escritorio vivo. */
    background: transparent;
    cursor: none !important;
  }

  /*
   * El PNG entra opaco: es la misma foto del escritorio. Animarlo se lee como
   * una ventana. El velo y la ayuda sí se funden, que es la señal de modo.
   */
  .cap {
    position: fixed;
    inset: 0;
    width: 100vw;
    height: 100vh;
    overflow: hidden;
    user-select: none;
    cursor: none;
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

  .cap.is-flying .cap-help,
  .cap.is-flying .cap-pointer {
    opacity: 0 !important;
    visibility: hidden;
    transition: none !important;
  }

  .cap-fly {
    pointer-events: none;
    position: fixed;
    z-index: 6;
    overflow: hidden;
    box-sizing: border-box;
    border-radius: calc(var(--rb-radius-xs, 5px) + 2px);
    box-shadow: var(--shadow-card, 0 1px 2px rgb(0 0 0 / 20%));
    outline: 1px solid rgb(255 255 255 / 18%);
    outline-offset: -1px;
    transform: translateY(0) scale(1);
    transform-origin: center center;
    will-change: left, top, width, height, transform, filter;
    transition:
      transform 320ms var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1)),
      box-shadow 320ms var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1));
  }

  .cap-fly.is-lifted {
    transform: translateY(-14px) scale(1.03);
    box-shadow: var(--shadow-float, 0 24px 70px rgb(0 0 0 / 45%));
  }

  .cap-fly.is-moving {
    transform: none;
    border-radius: var(--rb-radius-xs, 5px);
    box-shadow: var(--shadow-pop, 0 8px 24px rgb(0 0 0 / 32%));
    outline: 1px solid rgb(255 255 255 / 10%);
    animation: cap-fly-blur 150ms var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1))
      both;
    transition:
      left 150ms var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1)),
      top 150ms var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1)),
      width 150ms var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1)),
      height 150ms var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1)),
      transform 150ms var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1)),
      box-shadow 150ms var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1)),
      border-radius 150ms var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1));
  }

  @keyframes cap-fly-blur {
    0% {
      filter: blur(0);
    }

    40% {
      filter: blur(4px);
    }

    100% {
      filter: blur(0);
    }
  }

  .cap-fly img {
    position: absolute;
    display: block;
    max-width: none;
    pointer-events: none;
  }

  .cap-pointer {
    pointer-events: none;
    position: fixed;
    left: 0;
    top: 0;
    z-index: 5;
    will-change: transform;
  }

  .cap-dim {
    pointer-events: none;
    position: absolute;
    inset: 0;
    opacity: 0;
    transition: opacity var(--duration-fast, 125ms)
      var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1));
  }

  .cap-dim.is-on {
    opacity: 1;
  }

  .cap-hole {
    pointer-events: none;
    position: absolute;
    box-sizing: border-box;
    container-type: size;
    border: 2px dashed var(--screen-select);
    box-shadow:
      0 0 0 1px var(--screen-select-edge, rgb(0 0 0 / 55%)),
      inset 0 0 0 1px rgb(255 255 255 / 22%),
      0 0 0 100000px var(--screen-scrim);
  }

  /*
   * Vértices en L. No son manijas: no se arrastran. El trazo más grueso en
   * las esquinas es lo que hace leer un recuadro en vez de una raya. En un
   * recorte chico se acortan para no taparse entre sí.
   */
  .cap-v {
    position: absolute;
    width: min(22px, 32cqw);
    height: min(22px, 32cqh);
    box-sizing: border-box;
    border-color: var(--screen-select);
    border-style: solid;
    filter: drop-shadow(0 0 0.6px var(--screen-select-edge, rgb(0 0 0 / 80%)));
  }

  .cap-v.nw {
    top: -2px;
    left: -2px;
    border-width: 3px 0 0 3px;
  }

  .cap-v.ne {
    top: -2px;
    right: -2px;
    border-width: 3px 3px 0 0;
  }

  .cap-v.se {
    right: -2px;
    bottom: -2px;
    border-width: 0 3px 3px 0;
  }

  .cap-v.sw {
    bottom: -2px;
    left: -2px;
    border-width: 0 0 3px 3px;
  }

  .cap-meta {
    pointer-events: none;
    position: absolute;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
    transform: translateY(calc(-100% - 6px));
    animation: cap-chip-in var(--duration-fast, 125ms)
      var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1)) both;
  }

  .cap-name,
  .cap-size {
    max-width: min(22rem, 70vw);
    overflow: hidden;
    border-radius: var(--rb-radius-xs, 5px);
    background: var(--screen-chip);
    padding: 2px 8px;
    color: var(--screen-ink);
    font-size: 12px;
    line-height: 1.25;
    white-space: nowrap;
    text-overflow: ellipsis;
    outline: 1px solid rgb(255 255 255 / 10%);
    outline-offset: -1px;
  }

  .cap-name {
    font-weight: 650;
  }

  .cap-size {
    font-family: var(--rb-mono, ui-monospace, monospace);
    font-variant-numeric: tabular-nums;
  }

  @keyframes cap-chip-in {
    from {
      opacity: 0;
      transform: translateY(calc(-100% - 2px));
    }

    to {
      opacity: 1;
      transform: translateY(calc(-100% - 6px));
    }
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
    color: var(--screen-ink);
    box-shadow: 0 8px 24px rgb(0 0 0 / 28%);
    outline: 1px solid rgb(255 255 255 / 10%);
    outline-offset: -1px;
    opacity: 0;
    transform: translateX(-50%) translateY(var(--distance-micro, 4px));
    transition:
      opacity var(--duration-fast, 125ms)
        var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1)),
      transform var(--duration-fast, 125ms)
        var(--ease-smooth-out, cubic-bezier(0.22, 1, 0.36, 1));
    transition-delay: 0ms;
  }

  .cap.is-revealed .cap-help {
    opacity: 1;
    transform: translateX(-50%) translateY(0);
    transition-delay: var(--duration-micro, 80ms);
  }

  @media (prefers-reduced-motion: reduce) {
    .cap-dim,
    .cap-dim.is-on,
    .cap-help,
    .cap.is-revealed .cap-help,
    .cap-meta,
    .cap-size {
      transition: none !important;
      animation: none !important;
    }

    .cap.is-revealed .cap-help {
      opacity: 1;
      transform: translateX(-50%);
    }

    .cap-meta,
    .cap-size {
      opacity: 1;
      transform: translateY(calc(-100% - 6px));
    }

    .cap-fly,
    .cap-fly.is-lifted,
    .cap-fly.is-moving {
      transition: none !important;
      animation: none !important;
      filter: none !important;
    }
  }
</style>
