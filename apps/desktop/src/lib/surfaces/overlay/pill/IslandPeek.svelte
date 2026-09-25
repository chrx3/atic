<script lang="ts">
  /**
   * El vistazo de una herramienta, como parte de la isla.
   *
   * Con la tira acoplada abierta, pasar el mouse por una herramienta ya no saca
   * un panel al lado: la isla crece hacia adentro y muestra el vistazo en ese
   * tramo nuevo. Es la misma pieza que se estira, no una segunda cosa.
   *
   * La isla necesita saber cuánto crecer ANTES de mostrarlo, y el alto depende
   * del contenido (el historial llega, una app entra al top). Por eso se mide
   * acá y se avisa con `onsize`; la caja de la pill sale de `contentFor` con
   * esa medida.
   *
   * El gesto y la temporización siguen en `toolPeek.svelte.ts`: entrar acá
   * cancela el cierre, salir lo agenda, igual que en el panel flotante.
   */
  import { untrack } from "svelte";
  import AgentsPeek from "./AgentsPeek.svelte";
  import CapturesPeek from "./CapturesPeek.svelte";
  import MediaPeek from "./MediaPeek.svelte";
  import ClipboardPeek from "./ClipboardPeek.svelte";
  import ColorPeek from "./ColorPeek.svelte";
  import SnippetsPeek from "./SnippetsPeek.svelte";
  import SystemPeek from "./SystemPeek.svelte";
  import {
    enterPeekPanel,
    hideToolPeek,
    leavePeekPanel,
    openPeekTool,
    PEEK_PANEL_ATTR,
    type PeekTool,
  } from "./toolPeek.svelte";

  let {
    tool,
    fallback,
    shown,
    vertical,
    onsize,
  }: {
    tool: PeekTool;
    fallback: string;
    /** Notch al costado: el vistazo es una columna angosta junto a la tira. */
    vertical: boolean;
    /** La isla ya creció a su medida: recién ahí se deja ver. */
    shown: boolean;
    onsize: (size: { w: number; h: number } | null) => void;
  } = $props();

  /**
   * Cuánto dura el fundido de salida al pasar de una herramienta a otra.
   *
   * El contenido nuevo no entra encima del viejo: primero se va el viejo, se
   * cambia, se mide, y la isla se estira a la medida nueva con su propia
   * transición mientras el nuevo aparece. Cambiar de golpe hacía saltar la
   * caja con el contenido todavía a la vista.
   */
  /* Un pelo más que `--duration-quick`: el cambio llega con el fundido ya hecho. */
  const SWAP_MS = 90;

  let el = $state<HTMLElement | null>(null);
  /** La herramienta que se ve. Va un fundido detrás de `tool`. */
  let showing = $state<PeekTool>(untrack(() => tool));
  let swapping = $state(false);

  $effect(() => {
    const next = tool;
    if (next === untrack(() => showing)) {
      swapping = false;
      return;
    }
    swapping = true;
    const timer = setTimeout(() => {
      showing = next;
      swapping = false;
    }, SWAP_MS);
    return () => clearTimeout(timer);
  });

  $effect(() => {
    const node = el;
    if (!node) return;
    const report = () => {
      const w = node.offsetWidth;
      const h = node.offsetHeight;
      onsize(w > 0 && h > 0 ? { w, h } : null);
    };
    report();
    const observer = new ResizeObserver(report);
    observer.observe(node);
    return () => {
      observer.disconnect();
      onsize(null);
    };
  });
</script>

<div
  bind:this={el}
  class="ip"
  class:is-shown={shown}
  class:is-vertical={vertical}
  {...{ [PEEK_PANEL_ATTR]: "" }}
  data-no-drag
  role="group"
  aria-hidden={showing === "agents"}
  onpointerenter={enterPeekPanel}
  onpointerleave={leavePeekPanel}
  onpointerdown={(e) => e.stopPropagation()}
>
  <div class="ip-body" class:is-swapping={swapping}>
    {#key showing}
      {#if showing === "system"}
        <SystemPeek {vertical} onopen={() => openPeekTool("system")} />
      {:else if showing === "clipboard"}
        <ClipboardPeek
          onpasted={hideToolPeek}
          onopen={() => openPeekTool("clipboard")}
        />
      {:else if showing === "captures"}
        <CapturesPeek
          {vertical}
          ondone={hideToolPeek}
          onnew={() => openPeekTool("captures")}
        />
      {:else if showing === "color"}
        <ColorPeek
          {vertical}
          ondone={hideToolPeek}
          onpick={() => openPeekTool("color")}
        />
      {:else if showing === "media"}
        <MediaPeek {vertical} />
      {:else if showing === "snippets"}
        <SnippetsPeek onpasted={hideToolPeek} onopen={() => openPeekTool("snippets")} />
      {:else}
        <AgentsPeek {fallback} {vertical} />
      {/if}
    {/key}
  </div>
</div>

<style>
  /*
   * Ancho fijo, como el panel flotante: un vistazo que se ajusta al contenido
   * movería los botones bajo el puntero y el hover entraría y saldría en loop.
   * El alto sí sigue al contenido; la isla lo acompaña.
   *
   * Sin fondo propio: la piel de la isla ya lo pinta, y es lo que hace que se
   * lea como la misma pieza.
   */
  .ip {
    position: relative;
    box-sizing: border-box;
    width: 16rem;
    padding: 0.2rem 0.75rem 0.7rem;
    color: var(--text);
    font-size: 0.75rem;
    line-height: 1.35;
    opacity: 0;
    pointer-events: none;
    transition: opacity var(--duration-fast, 125ms) var(--ease-smooth-out, ease-out);
  }

  /* Al costado la isla crece hacia la pantalla: angosta y alta, como la
     tira de la que sale. */
  .ip.is-vertical {
    width: 12rem;
    padding: 0.7rem 0.6rem;
  }

  .ip.is-shown {
    opacity: 1;
    pointer-events: auto;
    transition-delay: var(--duration-quick, 75ms);
  }

  .ip-body {
    transition: opacity var(--duration-fast, 125ms) var(--ease-smooth-out, ease-out);
  }

  .ip-body.is-swapping {
    opacity: 0;
    transition-duration: var(--duration-quick, 75ms);
  }

  @media (prefers-reduced-motion: reduce) {
    .ip,
    .ip.is-shown,
    .ip-body {
      transition: none;
    }
  }
</style>
