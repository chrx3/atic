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
   * El UA stylesheet del `<dialog>` trae `overflow: auto` y un `max-height`
   * propio: si el contenido lo supera, scrollea el diálogo entero (título
   * incluido). Por eso el elemento es un viewport flex a pantalla completa
   * con `overflow: hidden`, y el scroll vive solo dentro del body o del hijo.
   *
   * Lo que el nativo NO hace y se agrega acá: devolver el foco a donde estaba
   * al cerrar. Sin eso, cerrar un diálogo con teclado deja el foco en el
   * `<body>` y la siguiente tabulación empieza desde el principio de la
   * página.
   *
   * Motion (transitions-polish): abrir invita (`--duration-fast` + scale 0.96),
   * cerrar se aparta (`--duration-quick`). El unmount espera al outro.
   */
  import type { Snippet } from "svelte";

  type Size = "sm" | "md" | "lg" | "xl";

  let {
    title,
    subtitle,
    size = "md",
    dismissible = true,
    /** Si el hijo ya scrollea por su cuenta (p.ej. un panel partido), el body
     *  no debe volver a scrollear: eso movería el encabezado/nav del hijo. */
    scrollBody = true,
    /**
     * Altura fija al tope del `max-h` del panel. Evita que pestañas cortas
     * (ajustes) encojan el diálogo al cambiar de sección.
     */
    fill = false,
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
    scrollBody?: boolean;
    fill?: boolean;
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

  /** Cierre: `--duration-quick` (150ms). Abrir usa `--duration-fast` en CSS. */
  const CLOSE_MS = 150;

  const titleId = $props.id();
  let dialog = $state<HTMLDialogElement | null>(null);
  let closing = $state(false);
  let closeTimer = 0;

  const reduceMotion =
    typeof window !== "undefined" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  $effect(() => {
    const el = dialog;
    if (!el) return;
    const previous = document.activeElement as HTMLElement | null;
    el.showModal();
    return () => {
      if (closeTimer) window.clearTimeout(closeTimer);
      // `?.` porque el elemento pudo desaparecer del DOM antes que el foco:
      // pasa cuando la ventana se cierra con el diálogo abierto.
      previous?.focus?.();
    };
  });

  function requestClose() {
    if (closing) return;
    if (reduceMotion) {
      onClose();
      return;
    }
    closing = true;
    closeTimer = window.setTimeout(() => {
      closeTimer = 0;
      onClose();
    }, CLOSE_MS);
  }

  function onCancel(event: Event) {
    // El nativo cierra con Esc por su cuenta. Se intercepta para que el cierre
    // pase siempre por `onClose` y el estado de arriba no quede desincronizado.
    event.preventDefault();
    if (dismissible) requestClose();
  }

  function onBackdrop(event: MouseEvent) {
    // Con el dialog a pantalla completa, el clic en el área vacía (fuera del
    // panel) llega con el `<dialog>` como target. El panel es un hijo.
    if (dismissible && event.target === dialog) requestClose();
  }
</script>

<!--
  `fixed inset-0 … overflow-hidden` anula el `overflow: auto` del UA. Sin eso
  el diálogo entero scrollea cuando el panel supera su max-height nativo.
-->
<dialog
  bind:this={dialog}
  aria-labelledby={titleId}
  class="modal-root fixed inset-0 m-0 flex h-dvh max-h-none w-screen max-w-none
         items-center justify-center overflow-hidden border-0 bg-transparent
         p-4 text-text open:flex"
  class:is-closing={closing}
  oncancel={onCancel}
  onclick={onBackdrop}
>
  <div
    class="modal-panel flex w-full max-h-[min(85dvh,760px)] flex-col overflow-hidden
           rounded-md border border-line bg-elevated text-text shadow-float
           {fill ? 'h-[min(85dvh,760px)]' : ''}
           {WIDTHS[size]}"
  >
    <div class="flex shrink-0 items-start gap-3 border-b border-line px-4 py-3">
      {#if header}
        {@render header()}
      {:else}
        <div class="flex min-w-0 flex-1 flex-col gap-0.5">
          <h2 id={titleId} class="text-balance text-md font-semibold">{title}</h2>
          {#if subtitle}
            <p class="truncate text-xs text-faint">{subtitle}</p>
          {/if}
        </div>
      {/if}

      {#if dismissible}
        <!-- 32px visibles; el hit area llega a 40 con el pseudo. -->
        <button
          type="button"
          class="relative -mr-1 grid size-8 shrink-0 place-items-center rounded-sm
                 text-muted transition-[color,background-color,transform]
                 duration-(--duration-quick) ease-calm
                 hover:bg-surface-2 hover:text-text active:scale-[0.96]
                 before:absolute before:inset-[-4px] before:content-['']"
          aria-label="Cerrar"
          onclick={requestClose}
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

    <div
      class="flex min-h-0 flex-1 flex-col px-4 py-3
             {scrollBody ? 'overflow-y-auto' : 'overflow-hidden'}"
    >
      {@render children()}
    </div>

    {#if actions}
      <div
        class="flex shrink-0 items-center justify-end gap-2 border-t border-line px-4 py-3"
      >
        {@render actions()}
      </div>
    {/if}
  </div>
</dialog>

<style>
  /*
   * Abrir: `--duration-fast` + `--scale-large` (0.96) + smooth-out.
   * Cerrar: `--duration-quick`, misma escala (sin bounce en el exit).
   */
  .modal-root::backdrop {
    background: var(--rb-backdrop);
    animation: modal-backdrop-in var(--duration-fast) var(--ease-smooth-out) both;
  }

  .modal-root.is-closing {
    pointer-events: none;
  }

  .modal-root.is-closing::backdrop {
    animation: modal-backdrop-out var(--duration-quick) var(--ease-smooth-out) both;
  }

  .modal-panel {
    transform-origin: 50% 50%;
    animation: modal-panel-in var(--duration-fast) var(--ease-smooth-out) both;
  }

  .modal-root.is-closing .modal-panel {
    animation: modal-panel-out var(--duration-quick) var(--ease-smooth-out) both;
  }

  @keyframes modal-backdrop-in {
    from {
      opacity: 0;
    }

    to {
      opacity: 1;
    }
  }

  @keyframes modal-backdrop-out {
    from {
      opacity: 1;
    }

    to {
      opacity: 0;
    }
  }

  @keyframes modal-panel-in {
    from {
      opacity: 0;
      transform: scale(0.96);
      filter: blur(var(--blur-small, 2px));
    }

    to {
      opacity: 1;
      transform: scale(1);
      filter: blur(0);
    }
  }

  @keyframes modal-panel-out {
    from {
      opacity: 1;
      transform: scale(1);
    }

    to {
      opacity: 0;
      transform: scale(0.96);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .modal-root::backdrop,
    .modal-root.is-closing::backdrop,
    .modal-panel,
    .modal-root.is-closing .modal-panel {
      animation: none;
    }
  }
</style>
