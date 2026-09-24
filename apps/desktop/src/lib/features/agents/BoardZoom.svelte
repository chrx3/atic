<script lang="ts">
  /**
   * Zoom, acomodo y fondo de la pizarra, flotando arriba a la derecha. El
   * porcentaje es también un botón: vuelve al tamaño real, que es donde el
   * texto de las consolas se lee nítido.
   */
  import { t } from "$domain/i18n.svelte";
  import Icon from "$ui/Icon.svelte";
  import {
    Check,
    Columns3,
    LayoutGrid,
    Maximize,
    Minus,
    Palette,
    Plus,
    Rows3,
  } from "$lib/icons";
  import ChatPopover from "./ChatPopover.svelte";
  import { BACKDROPS, type Arrangement, type Backdrop } from "./agentBoard";

  let {
    backdrop,
    onBackdrop,
    zoom,
    canArrange,
    layout,
    onZoom,
    onReset,
    onFitAll,
    onArrange,
  }: {
    backdrop: Backdrop;
    onBackdrop: (backdrop: Backdrop) => void;
    zoom: number;
    canArrange: boolean;
    /** El acomodo puesto; su botón queda marcado. */
    layout: Arrangement | "free";
    /** Paso relativo: `1.2` acerca, `1 / 1.2` aleja. */
    onZoom: (factor: number) => void;
    onReset: () => void;
    onFitAll: () => void;
    onArrange: (mode: Arrangement) => void;
  } = $props();

  const ARRANGE = [
    { mode: "row", icon: Columns3, label: "page.agents.board.arrangeRow" },
    { mode: "column", icon: Rows3, label: "page.agents.board.arrangeColumn" },
    { mode: "grid", icon: LayoutGrid, label: "page.agents.board.arrangeGrid" },
  ] as const;

  const BACKDROP_LABEL = {
    dots: "page.agents.board.backdropDots",
    grid: "page.agents.board.backdropGrid",
    plain: "page.agents.board.backdropPlain",
  } as const;

  let backdropOpen = $state(false);
</script>

<div class="zoom" role="toolbar" aria-label={t("page.agents.board.view")}>
  <button
    type="button"
    class="btn"
    aria-label={t("page.agents.board.zoomOut")}
    title={t("page.agents.board.zoomOut")}
    onclick={() => onZoom(1 / 1.2)}
  >
    <Icon icon={Minus} size={14} />
  </button>
  <button
    type="button"
    class="btn is-pct"
    title={t("page.agents.board.zoomReset")}
    onclick={onReset}
  >
    {Math.round(zoom * 100)}%
  </button>
  <button
    type="button"
    class="btn"
    aria-label={t("page.agents.board.zoomIn")}
    title={t("page.agents.board.zoomIn")}
    onclick={() => onZoom(1.2)}
  >
    <Icon icon={Plus} size={14} />
  </button>
  <span class="sep" aria-hidden="true"></span>
  <button
    type="button"
    class="btn"
    aria-label={t("page.agents.board.fitAll")}
    title={t("page.agents.board.fitAll")}
    disabled={!canArrange}
    onclick={onFitAll}
  >
    <Icon icon={Maximize} size={14} />
  </button>
  <span class="sep" aria-hidden="true"></span>
  {#each ARRANGE as entry (entry.mode)}
    <button
      type="button"
      class="btn"
      aria-label={t(entry.label)}
      title={t(entry.label)}
      aria-pressed={layout === entry.mode}
      onclick={() => onArrange(entry.mode)}
    >
      <Icon icon={entry.icon} size={14} />
    </button>
  {/each}
  <span class="sep" aria-hidden="true"></span>
  <ChatPopover
    open={backdropOpen}
    onToggle={(open) => (backdropOpen = open)}
    label={t("page.agents.board.backdrop")}
    width={180}
    align="right"
    placement="down"
  >
    {#snippet trigger()}
      <Icon icon={Palette} size={14} />
    {/snippet}
    {#each BACKDROPS as option (option)}
      <button
        type="button"
        class="pick"
        aria-pressed={backdrop === option}
        onclick={() => {
          onBackdrop(option);
          backdropOpen = false;
        }}
      >
        <span class="swatch is-{option}" aria-hidden="true"></span>
        <span class="pick-label">{t(BACKDROP_LABEL[option])}</span>
        {#if backdrop === option}
          <Icon icon={Check} size={13} />
        {/if}
      </button>
    {/each}
  </ChatPopover>
</div>

<style>
  .zoom {
    display: flex;
    align-items: center;
    gap: 2px;
    border-radius: 12px;
    padding: 4px;
    background: color-mix(in sRGB, var(--rb-surface) 82%, transparent);
    box-shadow:
      0 0 0 1px color-mix(in sRGB, var(--rb-text) 10%, transparent),
      0 14px 32px -16px rgb(0 0 0 / 55%);
    backdrop-filter: blur(18px) saturate(1.2);
  }

  .btn {
    display: grid;
    place-items: center;
    min-width: 30px;
    height: 30px;
    border: 0;
    border-radius: 8px;
    padding: 0 6px;
    background: transparent;
    color: var(--rb-muted);
    font: inherit;
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
    cursor: pointer;
    transition:
      background-color var(--duration-fast) ease,
      color var(--duration-fast) ease;
  }

  .btn:hover:not(:disabled) {
    background: color-mix(in sRGB, var(--rb-text) 8%, transparent);
    color: var(--rb-text);
  }

  .btn:disabled {
    opacity: 0.35;
    cursor: default;
  }

  .btn[aria-pressed="true"] {
    background: color-mix(in sRGB, var(--accent) 18%, transparent);
    color: var(--rb-text);
  }

  .btn.is-pct {
    min-width: 46px;
    color: var(--rb-text);
  }

  .zoom :global(.trigger) {
    justify-content: center;
    width: 30px;
    height: 30px;
    padding: 0;
  }

  .pick {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    border: 0;
    border-radius: 8px;
    padding: 6px 8px;
    background: transparent;
    color: var(--rb-text);
    font: inherit;
    font-size: 12.5px;
    text-align: left;
    cursor: pointer;
  }

  .pick:hover {
    background: color-mix(in sRGB, var(--rb-text) 8%, transparent);
  }

  .pick-label {
    flex: 1;
  }

  /* Una muestra del fondo: lo mismo que pinta la pizarra, en chico. */
  .swatch {
    flex-shrink: 0;
    width: 26px;
    height: 18px;
    border-radius: 5px;
    background-color: var(--rb-bg0);
    box-shadow: inset 0 0 0 1px color-mix(in sRGB, var(--rb-text) 12%, transparent);
  }

  .swatch.is-dots {
    background-image: radial-gradient(
      color-mix(in sRGB, var(--rb-text) 30%, transparent) 1px,
      transparent 1.4px
    );
    background-size: 6px 6px;
  }

  .swatch.is-grid {
    background-image:
      linear-gradient(
        to right,
        color-mix(in sRGB, var(--rb-text) 18%, transparent) 1px,
        transparent 1px
      ),
      linear-gradient(
        to bottom,
        color-mix(in sRGB, var(--rb-text) 18%, transparent) 1px,
        transparent 1px
      );
    background-size: 8px 8px;
  }

  .sep {
    width: 1px;
    height: 16px;
    margin: 0 2px;
    background: color-mix(in sRGB, var(--rb-text) 10%, transparent);
  }
</style>
