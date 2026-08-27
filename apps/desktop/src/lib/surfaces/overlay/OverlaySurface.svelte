<script lang="ts">
  /**
   * El overlay: una sola superficie para todo lo que se funde.
   *
   * Cubre el escritorio virtual y por defecto deja pasar el mouse. Las
   * superficies que viven acá se registran en `surfaces.svelte.ts` para que
   * Rust sepa dónde armar la ventana.
   *
   * Hospeda la pill —con su rueda y sus paneles— y el float de agentes. Que
   * compartan documento no es un detalle de organización: es lo único que
   * permite que el cuello de la burbuja se FUNDA con la pill, porque tanto un
   * filtro SVG como el campo de distancia solo alcanzan lo que está en el mismo
   * `document`.
   */
  import { page } from "$app/state";
  import { AGENTS_ENABLED } from "$core/tools";
  import AgentsFloat from "./agents/AgentsFloat.svelte";
  import ClipboardFloat from "./clipboard/ClipboardFloat.svelte";
  import LauncherFloat from "./launcher/LauncherFloat.svelte";
  import SnippetsFloat from "./snippets/SnippetsFloat.svelte";
  import PillSurface from "./pill/PillSurface.svelte";
  import PracticeCoach from "$features/onboarding/PracticeCoach.svelte";
  import { getConfig } from "$ipc/config";
  import {
    onOverlayYieldMain,
    onOverlayReady,
    onPillVisibility,
    setOverlayCssViewport,
    setOverlayTextMode,
  } from "$ipc/overlay";
  import { liveArea, surfaces } from "./surfaces.svelte";
  import { liquid } from "./group.svelte";
  import Skin from "$liquid/Skin.svelte";
  import { BLEND, CELL, SMOOTH } from "$liquid/constants";
  import {
    LAUNCHER_LAB_OPEN_KEY,
    launcherLab,
  } from "$lib/dev/launcherLab.svelte";
  import type { Component } from "svelte";

  const debug = $derived(page.url.searchParams.has("debug"));
  const isDev = import.meta.env.DEV;

  /**
   * Banco de pruebas del sistema líquido, solo en dev.
   *
   * Rust construye la URL de esta ventana, así que no hay forma de pedirle el
   * lab por query. Va por `localStorage`, que las ventanas comparten por ser
   * del mismo origen: se enciende desde `/dev/liquid-lab` en la ventana normal
   * y el evento `storage` lo despierta acá. Es el único camino para ver el
   * sistema líquido dentro de WebView2 sin tocar `src-tauri/`.
   *
   * La importación es dinámica para que no entre al bundle de producción.
   */
  const LAB_KEY = "atic-liquid-lab";
  let Lab = $state<Component<{ standalone?: boolean; onClose?: () => void }> | null>(
    null,
  );

  /** Panel compacto de perillas (launcher + blend/cell). Solo DEV. */
  let LauncherLabPanel = $state<
    typeof import("$lib/dev/LauncherLabPanel.svelte").default | null
  >(null);
  let launcherLabEl = $state<HTMLElement | null>(null);

  const skinBlend = $derived(isDev && launcherLab.open ? launcherLab.blend : BLEND);
  // Misma calidad quieto y en movimiento. Antes el drag bajaba a celda 12 y
  // suavizado 0 "por costo", y la pill se veía poligonal al moverse; el campo
  // real es chico (pill ~350 muestras, float grande ~10k) y remeshear fino a
  // 60 Hz cuesta menos que un cuadro. El traslado rígido ni siquiera remeshea.
  const skinCell = $derived(isDev && launcherLab.open ? launcherLab.cell : CELL);
  const skinSmooth = SMOOTH;

  async function syncLab() {
    if (!isDev) return;
    if (localStorage.getItem(LAB_KEY) === "1") {
      Lab ??= (await import("$lib/dev/LiquidLab.svelte")).default;
    } else {
      Lab = null;
    }
  }

  async function syncLauncherLab() {
    if (!isDev) return;
    const want = localStorage.getItem(LAUNCHER_LAB_OPEN_KEY) === "1";
    if (!want) {
      launcherLab.close();
      return;
    }
    LauncherLabPanel ??= (await import("$lib/dev/LauncherLabPanel.svelte")).default;
    launcherLab.open = true;
  }

  $effect(() => {
    if (!isDev || !launcherLab.open) {
      document.documentElement.style.removeProperty("--goo-grow");
      return;
    }
    document.documentElement.style.setProperty(
      "--goo-grow",
      `${launcherLab.gooGrow}px`,
    );
    return () => document.documentElement.style.removeProperty("--goo-grow");
  });

  /**
   * El lab necesita el mouse entero.
   *
   * El overlay es click-through salvo donde una superficie publica su zona
   * viva. El host del lab cubre el viewport, así que registrarlo arma la
   * ventana sobre todo el escritorio — que es justo lo que hace falta para
   * arrastrar el globo, y por lo que conviene apagarlo al terminar.
   */
  let labEl = $state<HTMLElement | null>(null);
  $effect(() => (labEl && Lab ? liveArea("lab", labEl) : undefined));
  $effect(() =>
    launcherLabEl && launcherLab.open && LauncherLabPanel
      ? liveArea("launcher-lab", launcherLabEl)
      : undefined,
  );

  /**
   * Montar y desmontar, no esconder.
   *
   * Ocultar la pill era `window.hide()`. Acá desmontarla es lo que hace que
   * deje de publicar su zona viva — y con eso el overlay vuelve a ser
   * completamente transparente al mouse, que es el equivalente real de «no
   * está».
   *
   * Sin configuración, mejor visible que invisible: una pill que no aparece no
   * deja forma de llegar a los ajustes para volver a mostrarla.
   */
  let shown = $state(true);

  $effect(() => {
    void getConfig()
      .then((cfg) => (shown = cfg.show_pill))
      .catch(() => (shown = true));
    const pending = onPillVisibility((visible) => (shown = visible));
    return () => void pending.then((off) => off());
  });

  /** Espacio CSS real: fly-to y hit-test tienen que usar el mismo, no `client/DPI`. */
  let cssWidth = $state(0);
  let cssHeight = $state(0);
  $effect(() => {
    const w = cssWidth;
    const h = cssHeight;
    if (w <= 1 || h <= 1) return;
    void setOverlayCssViewport(w, h).catch(() => {
      // Fuera de Tauri no hay a quién avisarle.
    });
  });

  $effect(() => {
    const pending = onOverlayReady(() => {
      const w = window.innerWidth;
      const h = window.innerHeight;
      if (w > 1 && h > 1) {
        void setOverlayCssViewport(w, h).catch(() => {});
      }
      void surfaces.recoverHits();
    });
    return () => void pending.then((off) => off());
  });

  $effect(() => {
    const pending = onOverlayYieldMain(() => {
      if (surfaces.dragging) surfaces.resetInteraction();
    });
    return () => void pending.then((off) => off());
  });

  $effect(() => {
    void syncLab();
    void syncLauncherLab();
    const onStorage = (event: StorageEvent) => {
      if (event.key === LAB_KEY || event.key === null) void syncLab();
      if (event.key === LAUNCHER_LAB_OPEN_KEY || event.key === null) {
        void syncLauncherLab();
      }
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  });

  function onOverlayDevKey(event: KeyboardEvent) {
    if (!isDev) return;
    if (event.key === "Escape" && launcherLab.open) {
      // Launcher vivo (aunque mid-morph): Esc lo cierra LauncherFloat; lab queda.
      if (document.querySelector(".lf")) return;
      event.preventDefault();
      launcherLab.close();
    }
  }

  /**
   * El teclado.
   *
   * El overlay nace inactivable para no robarle el foco a la app en la que
   * estés escribiendo, y el precio es que tampoco puede recibir teclas. Acá
   * adentro hay campos de texto —el compositor de agentes, la búsqueda del
   * historial, el bloc—, así que el foco se pide al entrar a uno y se devuelve
   * al salir.
   *
   * Va en el overlay y no en cada superficie a propósito: el permiso es de la
   * VENTANA, hay una sola, y repartir la decisión entre los componentes es
   * justo cómo se olvida un campo y deja de aceptar teclas en silencio.
   */
  let textMode = false;

  function editable(node: EventTarget | null): HTMLElement | null {
    if (!(node instanceof HTMLElement)) return null;
    // Incluye xterm: el clic cae en el viewport/canvas, no en el textarea helper.
    const el = node.closest(
      "input, textarea, [contenteditable='true'], [data-console-term], .xterm",
    );
    return el instanceof HTMLElement ? el : null;
  }

  /** xterm sigue montado al esconder el float; no puede dejar el overlay en modo texto. */
  function hiddenAgentsHost(node: EventTarget | null): boolean {
    if (!(node instanceof Element)) return false;
    const host = node.closest(".af");
    if (!(host instanceof HTMLElement)) return false;
    return host.classList.contains("is-off") || !host.classList.contains("is-shown");
  }

  function focusEditable(el: HTMLElement) {
    if (el.matches("input, textarea, [contenteditable='true']")) {
      el.focus();
      return;
    }
    const root = el.closest(".xterm, [data-console-term]") ?? el;
    const helper = root.querySelector("textarea.xterm-helper-textarea");
    if (helper instanceof HTMLTextAreaElement) {
      helper.focus();
      return;
    }
    el.focus();
  }

  async function enterTextMode(event: PointerEvent) {
    const el = editable(event.target);
    if (!el || hiddenAgentsHost(el)) return;
    // SIEMPRE se re-pide a Rust, sin mirar `textMode` ni `document.hasFocus()`:
    // los dos mienten en esta ventana. Al clicar otra app Windows nos quita el
    // teclado sin que el flag se entere (`onFocusIn` lo repone sin pasar por
    // Rust, `yield_to_capture` baja `focusable` a espaldas del front), y
    // Chromium sigue contestando `hasFocus() === true` porque su foco interno
    // nunca se movió aunque el HWND ya no sea el primero. Confiar en
    // cualquiera de los dos dejaba la consola muda hasta tocar un botón —de
    // los que llaman a `set_overlay_text_mode` sin condición, y por eso
    // funcionaban—. El costo es un IPC por clic en un campo: nada.
    textMode = true;
    try {
      await setOverlayTextMode(true);
    } catch {
      // Fuera de Tauri no hay ventana a la que pedirle el foco.
    }
    // Después del viaje a Rust: hasta que la ventana no es activable, el
    // webview no acepta el foco y el clic no deja el cursor en el campo.
    focusEditable(el);
  }

  async function leaveTextMode() {
    if (!textMode) return;
    textMode = false;
    try {
      await setOverlayTextMode(false);
    } catch {
      // Ídem.
    }
  }

  $effect(() => {
    // `focusout` y no `blur`: burbujea, así que un solo oyente cubre todos los
    // campos, incluidos los que todavía no existen. El `setTimeout` deja pasar
    // el salto entre dos campos —del asunto al cuerpo— sin devolver y volver a
    // pedir el foco en el medio.
    const onFocusOut = () => {
      setTimeout(() => {
        const ae = document.activeElement;
        if (!editable(ae) || hiddenAgentsHost(ae)) void leaveTextMode();
      }, 0);
    };
    // Foco programático (launcher al abrir, etc.): el hijo pide
    // `set_overlay_text_mode` y luego `input.focus()`. Sin este sync,
    // `textMode` seguiría en false y `leaveTextMode` no devolvería el teclado.
    const onFocusIn = () => {
      const ae = document.activeElement;
      if (!editable(ae) || hiddenAgentsHost(ae) || textMode) return;
      textMode = true;
    };
    // Si el foco salta a otra app (Alt+Tab, clic fuera), el `focusout` del DOM
    // a veces no corre: el textarea sigue siendo `activeElement` y el modo
    // texto queda pegado. `blur` de la ventana lo devuelve igual.
    const onWindowBlur = () => void leaveTextMode();
    const onLeave = () => void leaveTextMode();
    document.addEventListener("focusin", onFocusIn);
    document.addEventListener("focusout", onFocusOut);
    window.addEventListener("blur", onWindowBlur);
    window.addEventListener("atic-overlay-leave-text", onLeave);
    return () => {
      document.removeEventListener("focusin", onFocusIn);
      document.removeEventListener("focusout", onFocusOut);
      window.removeEventListener("blur", onWindowBlur);
      window.removeEventListener("atic-overlay-leave-text", onLeave);
      void leaveTextMode();
    };
  });
</script>

<svelte:window
  onkeydown={onOverlayDevKey}
  bind:innerWidth={cssWidth}
  bind:innerHeight={cssHeight}
/>

<!--
  El orden importa y es al revés de como se lee.

  El float va PRIMERO (debajo en DOM). El z-index del float es mayor que el de
  la pill (`layers.css`): así el header del float (pin, cerrar) gana el clic
  cuando se solapa con el hit-box de la pill junto al cuello. La silueta de la
  pill sigue en la capa fundida (Skin) por encima del cuerpo.

  Además de DOM: `--z-overlay-float` > `--z-overlay-pill`.
-->
<div
  class="ov"
  class:is-debug={debug}
  class:is-dragging={surfaces.dragging}
  onpointerdowncapture={enterTextMode}
>
  {#if Lab}
    <div class="lab-host" bind:this={labEl}>
      <Lab
        onClose={() => {
          // `storage` no dispara en la misma ventana: hay que bajar el lab acá.
          localStorage.removeItem(LAB_KEY);
          Lab = null;
        }}
      />
    </div>
  {:else}
    <!--
      La piel de lo que se funde, por isla.

      Un campo de distancia solo funde lo que comparte campo. Superficies más
      lejos que REACH van en islas distintas: arrastrar una no remuestrea la
      otra. Primero en el orden, o sea debajo de todo: es una silueta, y el
      contenido —texto, iconos, controles— vive encima con la misma geometría.
    -->
    {#each liquid.islands as island (island.id)}
      <Skin
        shapes={island.shapes}
        blend={skinBlend}
        cell={skinCell}
        smooth={skinSmooth}
      />
    {/each}

    <!-- Floats siempre montados: escuchan anclas/dismiss aunque estén cerrados. -->
    {#if AGENTS_ENABLED}
      <AgentsFloat />
    {/if}
    <ClipboardFloat />
    <SnippetsFloat />
    <LauncherFloat />
    {#if shown}
      <PillSurface />
    {/if}
    <PracticeCoach />

    {#if isDev && launcherLab.open && LauncherLabPanel}
      <div class="launcher-lab-host" bind:this={launcherLabEl}>
        <LauncherLabPanel />
      </div>
    {/if}
  {/if}
</div>

<style>
  :global(html),
  :global(body) {
    overflow: hidden;
    width: 100%;
    height: 100%;
    margin: 0;
    background: transparent;
  }

  .ov {
    /* `100vw/100vh` en WebView2 a veces es un recuadro más chico que la
       ventana: html overflow:hidden recorta la pill antes del borde real. */
    position: fixed;
    inset: 0;

    /* Sin `touch-action` el navegador se queda el gesto para hacer pan y los
       `pointermove` dejan de llegar apenas arranca un arrastre. */
    touch-action: none;
    user-select: none;
  }

  .lab-host {
    position: fixed;
    inset: 0;
  }

  .launcher-lab-host {
    position: fixed;
    top: 3.25rem;
    left: 0.75rem;
    z-index: 90;
    width: min(19rem, calc(100vw - 1.5rem));
    max-height: min(70vh, 34rem);
    pointer-events: auto;
  }

  /* `?debug` en la URL: marca el borde de la lámina, que si no es invisible. */
  .ov.is-debug {
    box-sizing: border-box;
    border: 2px dashed var(--warn);
  }

  /*
   * Durante drag: sin transition de emerge en floats (left/top cada frame).
   */
  .ov.is-dragging :global(.float-emerge) {
    transition: none !important;
  }

  .ov.is-dragging :global(.skin) {
    will-change: transform;
  }
</style>
