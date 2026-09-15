<script lang="ts">
  /**
   * El marco de una ventana sin decoración del sistema.
   *
   * Las acciones de ventana llegan por props y no se resuelven acá: un patrón
   * no habla con Tauri. Quien monta el marco es quien sabe qué ventana es y qué
   * puede hacer con ella — la principal se minimiza y se maximiza, un panel
   * flotante solo se cierra.
   *
   * `data-tauri-drag-region` es lo único específico de Tauri, y es un atributo,
   * no una llamada: marca qué parte de la barra arrastra la ventana. Los
   * controles llevan `data-no-drag` o arrastrarían en vez de responder.
   *
   * En Mac los tres controles van a la izquierda y con los colores del sistema
   * (rojo/amarillo/verde); en Windows quedan a la derecha, monocromos.
   */
  import type { Snippet } from "svelte";
  import Icon from "$ui/Icon.svelte";
  import IconButton from "$ui/IconButton.svelte";
  import { Minus, Square, X } from "$lib/icons";
  import { t } from "$domain/i18n.svelte";

  let {
    title,
    onMinimize,
    onMaximize,
    onClose,
    minimizeLabel,
    maximizeLabel,
    closeLabel,
    start,
    titleMenu,
    actions,
    children,
  }: {
    title: string;
    onMinimize?: () => void;
    onMaximize?: () => void;
    onClose?: () => void;
    minimizeLabel?: string;
    maximizeLabel?: string;
    closeLabel?: string;
    /** A la izquierda del título: marca, volver, lo que sea. */
    start?: Snippet;
    /**
     * Reemplaza el texto del título por un control —el menú de herramientas—.
     * El `<h1>` sigue existiendo para el lector de pantalla; adentro va el
     * disparador, que es lo que en macOS se hace con el menú del documento.
     */
    titleMenu?: Snippet;
    /** A la derecha, antes de los controles de ventana. */
    actions?: Snippet;
    children: Snippet;
  } = $props();

  /** WKWebView en Mac reporta `MacIntel`; WebView2 en Windows, `Win32`. */
  const isMac =
    typeof navigator !== "undefined" &&
    /mac/i.test(navigator.platform || navigator.userAgent);

  const minLabel = $derived(minimizeLabel ?? t("chrome.minimize"));
  const maxLabel = $derived(maximizeLabel ?? t("chrome.maximize"));
  const xLabel = $derived(closeLabel ?? t("chrome.close"));

  const hasControls = $derived(Boolean(onMinimize || onMaximize || onClose));
</script>

{#snippet windowControls()}
  {#if isMac}
    <!-- Orden de AppKit: cerrar, minimizar, zoom, siempre a la izquierda. -->
    <div data-no-drag class="mac-controls flex shrink-0 items-center">
      {#if onClose}
        <button
          type="button"
          class="mac-control mac-close"
          aria-label={xLabel}
          title={xLabel}
          onclick={onClose}
        >
          <Icon icon={X} size={9} strokeWidth={1.5} />
        </button>
      {/if}
      {#if onMinimize}
        <button
          type="button"
          class="mac-control mac-min"
          aria-label={minLabel}
          title={minLabel}
          onclick={onMinimize}
        >
          <Icon icon={Minus} size={9} strokeWidth={1.5} />
        </button>
      {/if}
      {#if onMaximize}
        <button
          type="button"
          class="mac-control mac-max"
          aria-label={maxLabel}
          title={maxLabel}
          onclick={onMaximize}
        >
          <Icon icon={Square} size={7} strokeWidth={1.5} />
        </button>
      {/if}
    </div>
  {:else}
    <!--
      Los tres glifos de ventana llevan `size` distinto A PROPÓSITO.

      Lucide dibuja cada icono con la tinta que necesita dentro del mismo
      cuadro de 24: la raya llega a 14 unidades, el cuadrado a 18 y la equis
      solo a 12. Con un `size` común —los 12 que había— la equis quedaba un
      tercio más chica que el cuadrado, que es lo que se veía en la barra.
      Estos números igualan la TINTA en ~10 px, y `absoluteStrokeWidth` fija
      el trazo en 1 px para los tres pese al `size` distinto: el mismo grosor
      que los iconos de acción de al lado (1.75 sobre 14 ≈ 1 px).
    -->
    <div data-no-drag class="flex shrink-0 items-center">
      {#if onMinimize}
        <IconButton label={minLabel} size="sm" onclick={onMinimize}>
          <Icon icon={Minus} size={17} strokeWidth={1} absoluteStrokeWidth />
        </IconButton>
      {/if}
      {#if onMaximize}
        <IconButton label={maxLabel} size="sm" onclick={onMaximize}>
          <Icon icon={Square} size={13} strokeWidth={1} absoluteStrokeWidth />
        </IconButton>
      {/if}
      {#if onClose}
        <IconButton label={xLabel} size="sm" variant="danger" onclick={onClose}>
          <Icon icon={X} size={20} strokeWidth={1} absoluteStrokeWidth />
        </IconButton>
      {/if}
    </div>
  {/if}
{/snippet}

<div class="atic-root flex h-screen flex-col overflow-hidden">
  <header
    data-tauri-drag-region
    class="flex h-9 shrink-0 items-center gap-2 bg-bg px-2"
  >
    {#if isMac && hasControls}
      {@render windowControls()}
    {/if}

    {#if start}
      <div data-no-drag class="flex shrink-0 items-center gap-1">{@render start()}</div>
    {/if}

    <!--
      El título también arrastra: es la zona más grande y la más obvia.
      Sin `truncate` cuando hay menú: es `overflow: hidden`, y recortaría el
      desplegable a la altura de esta fila. El truncado va en el texto.
    -->
    <h1
      data-tauri-drag-region
      class="flex min-w-0 flex-1 items-center text-xs font-medium text-muted select-none"
      aria-label={title}
    >
      {#if titleMenu}
        <span data-no-drag class="flex min-w-0 items-center">{@render titleMenu()}</span
        >
      {:else}
        <span class="truncate">{title}</span>
      {/if}
    </h1>

    {#if actions}
      <div data-no-drag class="flex shrink-0 items-center gap-1">
        {@render actions()}
      </div>
    {/if}

    {#if !isMac && hasControls}
      {@render windowControls()}
    {/if}
  </header>

  <main id="main-content" data-no-drag class="min-h-0 flex-1 overflow-hidden bg-bg">
    {@render children()}
  </main>
</div>

<style>
  .mac-controls {
    gap: 0.5rem;
    padding: 0 0.4rem 0 0.3rem;
  }

  .mac-control {
    display: inline-flex;
    width: 12px;
    height: 12px;
    align-items: center;
    justify-content: center;
    padding: 0;
    border: 0;
    border-radius: 9999px;
    color: color-mix(in sRGB, black 55%, transparent);
    cursor: default;
    line-height: 0;
  }

  /* Los glifos aparecen al pasar el mouse, como en AppKit. */
  .mac-control :global(svg) {
    opacity: 0;
    transition: opacity 90ms ease-out;
  }

  .mac-controls:hover .mac-control :global(svg),
  .mac-control:focus-visible :global(svg) {
    opacity: 1;
  }

  .mac-close {
    background: var(--rb-mac-close);
  }

  .mac-min {
    background: var(--rb-mac-min);
  }

  .mac-max {
    background: var(--rb-mac-max);
  }

  .mac-control:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }
</style>
