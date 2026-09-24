<script lang="ts">
  /**
   * Vistazo de Color: los últimos colores tomados, a un clic de copiarlos.
   *
   * Los recientes son los del almacenamiento del overlay, que la pill llena
   * con cada `color-picked` (el de la lupa no llega: el overlay tiene su propio
   * `data_directory`). Se leen al abrir. Copia por Rust (`copyText`): el
   * overlay no toma el foco y `navigator.clipboard` se niega sin foco.
   */
  import { t } from "$domain/i18n.svelte";
  import { toastError } from "$domain/toasts.svelte";
  import { loadRecentColors } from "$features/color/colorMath";
  import { copyText } from "$ipc/clipboard";

  let {
    ondone,
    onpick,
    vertical = false,
  }: { ondone: () => void; onpick: () => void; vertical?: boolean } = $props();

  /** Cuánto se ve el «Copiado» antes de cerrar. */
  const COPIED_MS = 650;

  const colors = loadRecentColors();
  let pointed = $state<string | null>(null);
  let copied = $state<string | null>(null);
  const shown = $derived(copied ?? pointed ?? colors[0] ?? null);

  async function copy(hex: string): Promise<void> {
    try {
      await copyText(hex);
      copied = hex;
      window.setTimeout(ondone, COPIED_MS);
    } catch (error) {
      toastError(error);
    }
  }
</script>

<div class="op" class:is-vertical={vertical}>
  {#if colors.length === 0}
    <p class="op-empty">{t("pill.peek.colorEmpty")}</p>
  {:else}
    <ul class="op-swatches">
      {#each colors as hex (hex)}
        <li>
          <button
            type="button"
            class="op-swatch"
            class:is-copied={copied === hex}
            style:--swatch={hex}
            aria-label={t("pill.peek.colorCopy", { hex })}
            onpointerenter={() => (pointed = hex)}
            onpointerleave={() => (pointed = null)}
            onclick={() => void copy(hex)}
          ></button>
        </li>
      {/each}
    </ul>
    {#if shown}
      <p class="op-hex" class:is-copied={copied !== null}>
        <span class="op-dot" style:--swatch={shown} aria-hidden="true"></span>
        <span data-numeric>{shown}</span>
        {#if copied}<span class="op-done">· {t("pill.peek.copied")}</span>{/if}
      </p>
    {/if}
  {/if}

  <button type="button" class="op-open" onclick={onpick}
    >{t("pill.peek.colorPick")}</button
  >
</div>

<style>
  .op {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  .op-empty {
    margin: 0;
    color: var(--muted);
  }

  .op-swatches {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  /* Al costado de la tira: las muestras en columnas parejas y no en una
     fila que se corta donde caiga. */
  .op.is-vertical .op-swatches {
    display: grid;
    grid-template-columns: repeat(4, 1.6rem);
    justify-content: space-between;
  }

  .op-swatch {
    width: 1.6rem;
    height: 1.6rem;
    border: 0;
    border-radius: 999px;
    padding: 0;
    background: var(--swatch);
    box-shadow: inset 0 0 0 1px color-mix(in sRGB, var(--text) 18%, transparent);
    cursor: pointer;
    transition: transform var(--duration-quick) var(--ease-smooth-out);
  }

  .op-swatch:hover,
  .op-swatch.is-copied {
    transform: scale(1.14);
  }

  .op-swatch:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }

  .op-hex {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    margin: 0;
    color: var(--text);
    font-variant-numeric: tabular-nums;
    text-transform: uppercase;
  }

  .op-dot {
    width: 0.6rem;
    height: 0.6rem;
    border-radius: 999px;
    background: var(--swatch);
  }

  .op-done {
    color: var(--ok);
    text-transform: none;
  }

  .op-open {
    align-self: flex-start;
    border: 0;
    padding: 0;
    background: none;
    color: var(--faint);
    font-size: 0.6875rem;
    cursor: pointer;
  }

  .op-open:hover {
    color: var(--text);
  }

  .op-open:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }
</style>
