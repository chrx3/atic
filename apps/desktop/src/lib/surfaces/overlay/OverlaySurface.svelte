<script lang="ts">
  /**
   * El overlay: una sola superficie para todo lo que se funde.
   *
   * Cubre el escritorio virtual y por defecto deja pasar el mouse. Las
   * superficies que viven acá se registran en `surfaces.svelte.ts` para que
   * Rust sepa dónde armar la ventana.
   *
   * Hospeda la pill —con su rueda y sus paneles— y la consola de agentes. Que
   * compartan documento no es un detalle de organización: es lo único que
   * permite que el cuello de la burbuja se FUNDA con la pill, porque tanto un
   * filtro SVG como el campo de distancia solo alcanzan lo que está en el mismo
   * `document`.
   */
  import { page } from "$app/state";
  import AgentsSurface from "$lib/AgentsSurface.svelte";
  import PillSurface from "./pill/PillSurface.svelte";
  import { getConfig } from "$ipc/config";
  import { onPillVisibility, setOverlayTextMode } from "$ipc/overlay";
  import { liveArea } from "./surfaces.svelte";
  import type { Component } from "svelte";

  const debug = $derived(page.url.searchParams.has("debug"));

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

  async function syncLab() {
    if (!import.meta.env.DEV) return;
    if (localStorage.getItem(LAB_KEY) === "1") {
      Lab ??= (await import("$lib/dev/LiquidLab.svelte")).default;
    } else {
      Lab = null;
    }
  }

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

  $effect(() => {
    void syncLab();
    const onStorage = (event: StorageEvent) => {
      if (event.key === LAB_KEY || event.key === null) void syncLab();
    };
    window.addEventListener("storage", onStorage);
    return () => window.removeEventListener("storage", onStorage);
  });

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
    const el = node.closest("input, textarea, [contenteditable='true']");
    return el instanceof HTMLElement ? el : null;
  }

  async function enterTextMode(event: PointerEvent) {
    const el = editable(event.target);
    if (!el || textMode) return;
    textMode = true;
    try {
      await setOverlayTextMode(true);
    } catch {
      // Fuera de Tauri no hay ventana a la que pedirle el foco.
    }
    // Después del viaje a Rust: hasta que la ventana no es activable, el
    // webview no acepta el foco y el clic no deja el cursor en el campo.
    el.focus();
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
        if (!editable(document.activeElement)) void leaveTextMode();
      }, 0);
    };
    document.addEventListener("focusout", onFocusOut);
    return () => {
      document.removeEventListener("focusout", onFocusOut);
      void leaveTextMode();
    };
  });
</script>

<!--
  El orden importa y es al revés de como se lee.

  La consola va PRIMERO para que quede debajo. Con la pill primero, el globo
  —que es enorme y recibe el mouse en todo su rectángulo— la tapaba: se seguía
  viendo, porque la copia de su silueta vive en la capa fundida y esa se pinta
  encima del cuerpo, pero no se podía ni clicar ni arrastrar.

  Y es el orden correcto de fondo: la pill es el control que siempre tiene que
  estar a mano. Lo que se despliega desde ella pasa por detrás.
-->
<div class="ov" class:is-debug={debug} onpointerdowncapture={enterTextMode}>
  {#if Lab}
    <div class="lab-host" bind:this={labEl}>
      <Lab onClose={() => localStorage.removeItem(LAB_KEY)} />
    </div>
  {:else}
    <!-- Siempre montada, visible o no: es quien escucha los eventos de sesión,
         y desmontarla dejaría la consola sorda mientras está cerrada. -->
    <AgentsSurface />
    {#if shown}
      <PillSurface />
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
    position: relative;
    width: 100vw;
    height: 100vh;

    /* Sin `touch-action` el navegador se queda el gesto para hacer pan y los
       `pointermove` dejan de llegar apenas arranca un arrastre. */
    touch-action: none;
    user-select: none;
  }

  .lab-host {
    position: fixed;
    inset: 0;
  }

  /* `?debug` en la URL: marca el borde de la lámina, que si no es invisible. */
  .ov.is-debug {
    box-sizing: border-box;
    border: 2px dashed var(--warn);
  }
</style>
