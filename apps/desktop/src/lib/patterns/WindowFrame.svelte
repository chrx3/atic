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
    /** A la derecha, antes de los controles de ventana. */
    actions?: Snippet;
    children: Snippet;
  } = $props();

  const minLabel = $derived(minimizeLabel ?? t("chrome.minimize"));
  const maxLabel = $derived(maximizeLabel ?? t("chrome.maximize"));
  const xLabel = $derived(closeLabel ?? t("chrome.close"));
</script>

<div class="atic-root flex h-screen flex-col overflow-hidden">
  <header
    data-tauri-drag-region
    class="flex h-9 shrink-0 items-center gap-2 bg-bg px-2"
  >
    {#if start}
      <div data-no-drag class="flex shrink-0 items-center gap-1">{@render start()}</div>
    {/if}

    <!-- El título también arrastra: es la zona más grande y la más obvia. -->
    <h1
      data-tauri-drag-region
      class="min-w-0 flex-1 truncate text-xs font-medium text-muted select-none"
    >
      {title}
    </h1>

    {#if actions}
      <div data-no-drag class="flex shrink-0 items-center gap-1">
        {@render actions()}
      </div>
    {/if}

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
    {#if onMinimize || onMaximize || onClose}
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
  </header>

  <main id="main-content" data-no-drag class="min-h-0 flex-1 overflow-hidden bg-bg">
    {@render children()}
  </main>
</div>
