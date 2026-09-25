<script lang="ts">
  /**
   * Botón con popover para la barra del composer.
   *
   * Abre hacia arriba —el composer vive al pie del panel— y se cierra con un
   * clic afuera o con Esc. El Esc no sigue de largo: sin frenarlo, el float
   * lo toma como «achicar» y se lleva el panel entero.
   */
  import type { Snippet } from "svelte";

  let {
    open,
    onToggle,
    label,
    trigger,
    children,
    width = 280,
    align = "left",
    placement = "up",
  }: {
    open: boolean;
    onToggle: (open: boolean) => void;
    /** Nombre accesible del botón. */
    label: string;
    trigger: Snippet;
    children: Snippet;
    width?: number;
    align?: "left" | "right";
    /** Hacia dónde abre: arriba en el composer, abajo en una barra de arriba. */
    placement?: "up" | "down";
  } = $props();

  let rootEl = $state<HTMLElement | null>(null);
  /** Lo que cabe de verdad: el pane recorta, y en la isla mide 440 px. */
  let room = $state<{ height: number; width: number } | null>(null);

  const MARGIN = 12;

  function measure() {
    if (!rootEl) return;
    const rect = rootEl.getBoundingClientRect();
    const bound = (
      rootEl.closest<HTMLElement>("[data-console-term]") ?? document.documentElement
    ).getBoundingClientRect();
    room = {
      height: Math.max(
        140,
        placement === "down"
          ? bound.bottom - rect.bottom - MARGIN
          : rect.top - bound.top - MARGIN,
      ),
      width: Math.max(
        180,
        align === "right"
          ? rect.right - bound.left - MARGIN
          : bound.right - rect.left - MARGIN,
      ),
    };
  }

  $effect(() => {
    if (!open) return;
    measure();
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  });

  $effect(() => {
    if (!open) return;
    const onDown = (event: PointerEvent) => {
      if (rootEl && !rootEl.contains(event.target as Node)) onToggle(false);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      onToggle(false);
    };
    window.addEventListener("pointerdown", onDown, true);
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("keydown", onKey, true);
    };
  });
</script>

<div class="anchor" bind:this={rootEl}>
  <button
    type="button"
    class="trigger"
    aria-haspopup="dialog"
    aria-expanded={open}
    aria-label={label}
    title={label}
    onclick={() => onToggle(!open)}
  >
    {@render trigger()}
  </button>
  {#if open}
    <div
      class="pop"
      class:is-right={align === "right"}
      class:is-down={placement === "down"}
      style:width={`${room ? Math.min(width, room.width) : width}px`}
      style:max-height={room ? `${Math.min(420, room.height)}px` : undefined}
    >
      {@render children()}
    </div>
  {/if}
</div>

<style>
  .anchor {
    position: relative;
    min-width: 0;
  }

  .trigger {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    max-width: 100%;
    height: 28px;
    border: 0;
    border-radius: 8px;
    padding: 0 8px;
    background: transparent;
    color: var(--rb-muted);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
    transition:
      background-color 120ms ease,
      color 120ms ease;
  }

  .trigger:hover,
  .trigger[aria-expanded="true"] {
    background: color-mix(in sRGB, var(--rb-text) 8%, transparent);
    color: var(--rb-text);
  }

  .pop {
    position: absolute;
    bottom: calc(100% + 8px);
    left: 0;
    z-index: 20;
    display: flex;
    flex-direction: column;
    max-height: 380px;
    overflow-y: auto;
    overscroll-behavior: contain;
    border-radius: 12px;
    padding: 6px;
    background: var(--rb-surface-elevated, var(--rb-surface));
    color: var(--rb-text);
    box-shadow:
      0 0 0 1px color-mix(in sRGB, var(--rb-text) 10%, transparent),
      0 12px 32px -8px rgb(0 0 0 / 45%);
  }

  .pop.is-right {
    right: 0;
    left: auto;
  }

  .pop.is-down {
    top: calc(100% + 8px);
    bottom: auto;
  }
</style>
