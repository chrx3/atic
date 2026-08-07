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
  import Icon from "$ui/Icon.svelte";
  import { X } from "$lib/icons";

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
    /**
     * Tope del panel (CSS length). Por defecto cabe en pantallas chicas sin
     * empujar el chrome fuera de vista. Tools largas (agentes) pasan uno mayor.
     */
    panelMax = "min(85dvh, 760px)",
    /**
     * Overlay anclado a un ancestro `position: relative` (p.ej. agents `.demo`).
     * No usa `showModal()` / top layer: evita cubrir toda la ventana o el
     * overlay transparente del desktop.
     */
    contained = false,
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
    panelMax?: string;
    contained?: boolean;
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
    if (contained) {
      // Sin top layer: el foco no entra solo; lo pedimos al root.
      el.focus();
    } else {
      el.showModal();
    }
    return () => {
      if (closeTimer) window.clearTimeout(closeTimer);
      // `?.` porque el elemento pudo desaparecer del DOM antes que el foco:
      // pasa cuando la ventana se cierra con el diálogo abierto.
      previous?.focus?.();
    };
  });

  // `cancel` solo existe con `showModal()`. En contained, Esc en captura.
  $effect(() => {
    if (!contained || !dismissible) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      requestClose();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
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
    if (dismissible && event.target === event.currentTarget) requestClose();
  }
</script>

<!--
  `fixed inset-0 … overflow-hidden` anula el `overflow: auto` del UA. Sin eso
  el diálogo entero scrollea cuando el panel supera su max-height nativo.
  `contained`: `absolute` + `open` (sin top layer) dentro del ancestro relative.
  `data-no-drag`: en AgentsFloat, el pointerdown del shell arrastra el globo
  y hace preventDefault — sin esto, el click del telón nunca llega a onBackdrop.
-->
<dialog
  bind:this={dialog}
  aria-labelledby={titleId}
  aria-modal="true"
  data-no-drag
  open={contained ? true : undefined}
  tabindex={contained ? -1 : undefined}
  class="modal-root m-0 flex max-h-none max-w-none items-center justify-center
         overflow-hidden border-0 p-4 text-text open:flex
         {contained
           ? 'absolute inset-0 z-20 h-full w-full'
           : 'fixed inset-0 h-dvh w-screen bg-transparent'}"
  class:is-contained={contained}
  class:is-closing={closing}
  oncancel={onCancel}
  onclick={onBackdrop}
>
  <div
    class="modal-panel flex w-full flex-col overflow-hidden
           rounded-md border border-line bg-elevated text-text shadow-float
           {WIDTHS[size]}"
    style:max-height={contained ? "100%" : panelMax}
    style:height={fill ? (contained ? "100%" : panelMax) : undefined}
  >
    <div
      class="flex shrink-0 items-start gap-3 border-b border-line px-4
             {header ? 'py-2.5' : 'py-3'}"
    >
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
          <Icon icon={X} size={14} />
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

  /* Telón propio: `::backdrop` solo existe con `showModal()` / top layer. */
  .modal-root.is-contained {
    background: var(--rb-backdrop);
    animation: modal-backdrop-in var(--duration-fast) var(--ease-smooth-out) both;
  }

  .modal-root.is-closing {
    pointer-events: none;
  }

  .modal-root.is-closing::backdrop {
    animation: modal-backdrop-out var(--duration-quick) var(--ease-smooth-out) both;
  }

  .modal-root.is-contained.is-closing {
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
    .modal-root.is-contained,
    .modal-root.is-contained.is-closing,
    .modal-panel,
    .modal-root.is-closing .modal-panel {
      animation: none;
    }
  }
</style>
