<script lang="ts">
  /**
   * Un diálogo modal. Es la primitiva más reutilizada de la app: nueve
   * consumidores.
   *
   * Usa `<dialog>` nativo con `showModal()`, y eso no es un detalle de
   * implementación — es lo que da gratis tres cosas que a mano se hacen mal:
   * el fondo queda inerte, el foco no se escapa del diálogo, y el elemento va
   * a la top layer del navegador, por encima de cualquier `z-index`. Por eso
   * un modal puede dibujarse sobre el hub sin que nadie coordine capas.
   *
   * Lo que el nativo NO hace y se agrega acá: devolver el foco a donde estaba
   * al cerrar. Sin eso, cerrar un diálogo con teclado deja el foco en el
   * `<body>` y la siguiente tabulación empieza desde el principio de la
   * página.
   */
  import type { Snippet } from "svelte";

  type Size = "sm" | "md" | "lg" | "xl";

  let {
    title,
    subtitle,
    size = "md",
    dismissible = true,
    onClose,
    header,
    actions,
    children,
  }: {
    title: string;
    subtitle?: string;
    size?: Size;
    /** `false` bloquea Esc y el clic en el telón: la decisión es obligatoria. */
    dismissible?: boolean;
    onClose: () => void;
    /** Reemplaza al encabezado por defecto, conservando el título accesible. */
    header?: Snippet;
    actions?: Snippet;
    children: Snippet;
  } = $props();

  const WIDTHS: Record<Size, string> = {
    sm: "max-w-80",
    md: "max-w-112",
    lg: "max-w-160",
    xl: "max-w-224",
  };

  const titleId = $props.id();
  let dialog = $state<HTMLDialogElement | null>(null);

  $effect(() => {
    const el = dialog;
    if (!el) return;
    const previous = document.activeElement as HTMLElement | null;
    el.showModal();
    return () => {
      // `?.` porque el elemento pudo desaparecer del DOM antes que el foco:
      // pasa cuando la ventana se cierra con el diálogo abierto.
      previous?.focus?.();
    };
  });

  function onCancel(event: Event) {
    // El nativo cierra con Esc por su cuenta. Se intercepta para que el cierre
    // pase siempre por `onClose` y el estado de arriba no quede desincronizado.
    event.preventDefault();
    if (dismissible) onClose();
  }

  function onBackdrop(event: MouseEvent) {
    // El clic en el telón llega con el `<dialog>` como target: el contenido
    // está en un hijo, así que cualquier clic de adentro tiene otro target.
    if (dismissible && event.target === dialog) onClose();
  }
</script>

<dialog
  bind:this={dialog}
  aria-labelledby={titleId}
  class="m-auto w-[calc(100%-2rem)] bg-transparent p-0 backdrop:bg-black/50 {WIDTHS[
    size
  ]}"
  oncancel={onCancel}
  onclick={onBackdrop}
>
  <div
    class="flex max-h-[85vh] flex-col overflow-hidden rounded-md border border-line
           bg-elevated text-text shadow-float"
  >
    <div class="flex items-start gap-3 border-b border-line px-4 py-3">
      {#if header}
        {@render header()}
      {:else}
        <div class="flex min-w-0 flex-1 flex-col gap-0.5">
          <h2 id={titleId} class="truncate text-md font-semibold">{title}</h2>
          {#if subtitle}
            <p class="truncate text-xs text-faint">{subtitle}</p>
          {/if}
        </div>
      {/if}

      {#if dismissible}
        <button
          type="button"
          class="-mr-1 grid size-6 shrink-0 place-items-center rounded-xs text-muted
                 transition-colors duration-(--duration-quick) ease-calm
                 hover:bg-surface-2 hover:text-text"
          aria-label="Cerrar"
          onclick={onClose}
        >
          <svg
            width="14"
            height="14"
            viewBox="0 0 24 24"
            fill="none"
            aria-hidden="true"
          >
            <path
              d="M6 6l12 12M18 6L6 18"
              stroke="currentColor"
              stroke-width="1.8"
              stroke-linecap="round"
            />
          </svg>
        </button>
      {/if}
    </div>

    <div class="min-h-0 flex-1 overflow-y-auto px-4 py-3">
      {@render children()}
    </div>

    {#if actions}
      <div class="flex items-center justify-end gap-2 border-t border-line px-4 py-3">
        {@render actions()}
      </div>
    {/if}
  </div>
</dialog>
