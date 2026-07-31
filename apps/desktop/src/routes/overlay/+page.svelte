<script lang="ts">
  /**
   * El overlay: una sola superficie para todo lo que se funde.
   *
   * Cubre el escritorio virtual y por defecto deja pasar el mouse. Las
   * superficies que viven acá se registran en `overlaySurfaces` para que Rust
   * sepa dónde armar la ventana.
   *
   * Hospeda la pill —con su rueda y sus paneles— y la consola de agentes. Que
   * compartan documento no es un detalle de organización: es lo único que
   * permite que el cuello de la burbuja se FUNDA con la pill, porque un filtro
   * SVG solo alcanza lo que está en el mismo `document`.
   */
  import { page } from "$app/state";
  import { onMount, type Component } from "svelte";
  import PillSurface from "$lib/PillSurface.svelte";
  import AgentsSurface from "$lib/AgentsSurface.svelte";
  import { getConfig, onPillVisibility, setOverlayTextMode } from "$lib/api";
  import { liveArea } from "$lib/overlaySurfaces.svelte";

  const debug = $derived(page.url.searchParams.has("debug"));

  /**
   * Banco de pruebas del sistema líquido, solo en dev.
   *
   * Rust construye la URL de esta ventana, así que no hay forma de pedirle el
   * lab por query. Va por `localStorage`, que las ventanas comparten por ser
   * del mismo origen: se enciende desde `/dev/liquid-lab` en la ventana normal
   * y el evento `storage` lo despierta acá. Es el único camino para ver el
   * filtro dentro de WebView2 sin tocar `src-tauri/`.
   *
   * El componente se importa dinámicamente para que no entre al bundle de
   * producción.
   */
  const LAB_KEY = "atic-liquid-lab";
  let Lab = $state<Component<{ standalone?: boolean; onClose?: () => void }> | null>(null);

  async function syncLab() {
    if (!import.meta.env.DEV) return;
    if (localStorage.getItem(LAB_KEY) === "1") {
      Lab ??= (await import("$lib/dev/LiquidLab.svelte")).default;
    } else {
      Lab = null;
    }
  }

  function closeLab() {
    localStorage.removeItem(LAB_KEY);
    Lab = null;
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
   * Montar/desmontar en vez de esconder.
   *
   * Ocultar la pill era `window.hide()`. Acá desmontarla es lo que hace que
   * deje de publicar su zona viva — y con eso el overlay vuelve a ser
   * completamente transparente al mouse, que es el equivalente real de "no
   * está".
   */
  let shown = $state(true);

  /**
   * El teclado.
   *
   * El overlay nace inactivable para no robarle el foco a la app en la que
   * estés escribiendo, y el precio es que tampoco puede recibir teclas. Acá
   * adentro hay campos de texto —el compositor de agentes, la búsqueda del
   * historial, el bloc de notas—, así que el foco se pide al entrar a uno y se
   * devuelve al salir.
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

  onMount(() => {
    // Sin config, mejor visible que invisible: una pill que no aparece no deja
    // forma de llegar a los ajustes para volver a mostrarla.
    void getConfig()
      .then((cfg) => (shown = cfg.show_pill))
      .catch(() => (shown = true));
    const un = onPillVisibility((v) => (shown = v));

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

    void syncLab();
    const onStorage = (e: StorageEvent) => {
      if (e.key === LAB_KEY || e.key === null) void syncLab();
    };
    window.addEventListener("storage", onStorage);

    return () => {
      void un.then((fn) => fn());
      document.removeEventListener("focusout", onFocusOut);
      window.removeEventListener("storage", onStorage);
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
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="ov" class:is-debug={debug} onpointerdowncapture={enterTextMode}>
  <!-- Siempre montada, visible o no: es quien escucha los eventos de sesión,
       y desmontarla dejaría la consola sorda mientras está cerrada. -->
  {#if Lab}
    <div class="lab-host" bind:this={labEl}>
      <Lab onClose={closeLab} />
    </div>
  {:else}
    <AgentsSurface />
    {#if shown}
      <PillSurface />
    {/if}
  {/if}
</div>

<style>
  :global(html),
  :global(body) {
    margin: 0;
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: transparent;
  }

  .ov {
    position: relative;
    width: 100vw;
    height: 100vh;
  }

  .lab-host {
    position: fixed;
    inset: 0;
  }

  .ov.is-debug {
    box-sizing: border-box;
    border: 2px dashed rgba(255, 120, 60, 0.9);
  }

  /* Sin esto el navegador se queda el gesto para hacer pan y los
     `pointermove` dejan de llegar apenas arranca un arrastre. */
  .ov {
    touch-action: none;
    user-select: none;
  }
</style>
