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
  import IconButton from "$ui/IconButton.svelte";

  let {
    title,
    onMinimize,
    onMaximize,
    onClose,
    start,
    actions,
    children,
  }: {
    title: string;
    onMinimize?: () => void;
    onMaximize?: () => void;
    onClose?: () => void;
    /** A la izquierda del título: marca, volver, lo que sea. */
    start?: Snippet;
    /** A la derecha, antes de los controles de ventana. */
    actions?: Snippet;
    children: Snippet;
  } = $props();
</script>

<div class="atic-root flex h-screen flex-col overflow-hidden">
  <header
    data-tauri-drag-region
    class="flex h-9 shrink-0 items-center gap-2 border-b border-line bg-surface px-2"
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

    {#if onMinimize || onMaximize || onClose}
      <div data-no-drag class="flex shrink-0 items-center">
        {#if onMinimize}
          <IconButton label="Minimizar" size="sm" onclick={onMinimize}>
            <svg width="12" height="12" viewBox="0 0 24 24" aria-hidden="true">
              <path d="M6 12h12" stroke="currentColor" stroke-width="2" />
            </svg>
          </IconButton>
        {/if}
        {#if onMaximize}
          <IconButton label="Maximizar" size="sm" onclick={onMaximize}>
            <svg
              width="12"
              height="12"
              viewBox="0 0 24 24"
              fill="none"
              aria-hidden="true"
            >
              <rect
                x="6"
                y="6"
                width="12"
                height="12"
                stroke="currentColor"
                stroke-width="2"
              />
            </svg>
          </IconButton>
        {/if}
        {#if onClose}
          <IconButton label="Cerrar" size="sm" variant="danger" onclick={onClose}>
            <svg
              width="12"
              height="12"
              viewBox="0 0 24 24"
              fill="none"
              aria-hidden="true"
            >
              <path
                d="M6 6l12 12M18 6L6 18"
                stroke="currentColor"
                stroke-width="2"
                stroke-linecap="round"
              />
            </svg>
          </IconButton>
        {/if}
      </div>
    {/if}
  </header>

  <main id="main-content" class="min-h-0 flex-1 overflow-hidden bg-bg">
    {@render children()}
  </main>
</div>
