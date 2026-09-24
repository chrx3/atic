<script lang="ts">
  /**
   * Un selector simple (esfuerzo, modo) para la barra del composer.
   *
   * Sobre `ChatPopover` y no sobre `PickerMenu`: aquel mide contra la ventana
   * y dentro del pane abría hacia abajo, fuera de la vista. Acá abre siempre
   * hacia arriba y con el alto que el pane deja.
   */
  import ChatPopover from "./ChatPopover.svelte";

  type Option = { id: string; label: string; note?: string };

  let {
    label,
    ariaLabel,
    options,
    value,
    open,
    onToggle,
    onPick,
  }: {
    /** Lo que se lee en el botón: el valor actual, corto. */
    label: string;
    ariaLabel: string;
    options: Option[];
    value: string;
    open: boolean;
    onToggle: (open: boolean) => void;
    onPick: (id: string) => void;
  } = $props();

  function pick(id: string) {
    onToggle(false);
    if (id !== value) onPick(id);
  }
</script>

<ChatPopover {open} {onToggle} label={ariaLabel} width={240}>
  {#snippet trigger()}
    <span class="value">{label}</span>
    <span class="caret" aria-hidden="true"></span>
  {/snippet}

  <p class="head">{ariaLabel}</p>
  {#each options as option (option.id)}
    <button
      type="button"
      class="option"
      class:is-selected={option.id === value}
      aria-pressed={option.id === value}
      onclick={() => pick(option.id)}
    >
      <span class="option-label">{option.label}</span>
      {#if option.note}
        <span class="option-note">{option.note}</span>
      {/if}
    </button>
  {/each}
</ChatPopover>

<style>
  .value {
    color: var(--rb-text);
    white-space: nowrap;
  }

  .caret {
    flex-shrink: 0;
    width: 5px;
    height: 5px;
    margin-left: 2px;
    border-right: 1.5px solid currentColor;
    border-bottom: 1.5px solid currentColor;
    transform: translateY(-2px) rotate(45deg);
  }

  .head {
    margin: 2px 8px 6px;
    color: var(--rb-faint);
    font-size: 11px;
    font-weight: 600;
  }

  .option {
    display: flex;
    flex-direction: column;
    gap: 1px;
    width: 100%;
    border: 0;
    border-radius: 7px;
    padding: 6px 8px;
    background: transparent;
    color: var(--rb-text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .option:hover {
    background: color-mix(in sRGB, var(--rb-text) 7%, transparent);
  }

  .option.is-selected {
    background: color-mix(in sRGB, var(--accent) 14%, transparent);
  }

  .option-label {
    font-size: 12.5px;
  }

  .option-note {
    color: var(--rb-faint);
    font-size: 11px;
    text-wrap: pretty;
  }
</style>
