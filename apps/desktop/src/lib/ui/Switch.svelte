<script lang="ts">
  /**
   * Un interruptor. Reemplaza a `.rb-check`.
   *
   * Es un `<input type="checkbox">` de verdad, escondido pero presente: así se
   * puede tabular, se puede activar con espacio y los lectores de pantalla lo
   * anuncian como lo que es. Un `<div role="switch">` habría que enseñarle
   * todo eso a mano.
   */
  let {
    checked = $bindable(false),
    label,
    hint,
    disabled = false,
    onchange,
  }: {
    checked?: boolean;
    label: string;
    hint?: string;
    disabled?: boolean;
    onchange?: (checked: boolean) => void;
  } = $props();

  const id = $props.id();
</script>

<div class="flex items-start gap-2">
  <input
    {id}
    type="checkbox"
    class="peer sr-only"
    bind:checked
    {disabled}
    onchange={() => onchange?.(checked)}
  />
  <label
    for={id}
    class="mt-px inline-flex h-4 w-7 shrink-0 cursor-pointer items-center rounded-pill border
           border-line bg-surface-2 p-px
           transition-colors duration-(--duration-quick) ease-calm
           peer-checked:border-accent peer-checked:bg-accent
           peer-disabled:cursor-not-allowed peer-disabled:opacity-45
           peer-focus-visible:outline peer-focus-visible:outline-2
           peer-focus-visible:outline-offset-2 peer-focus-visible:outline-accent"
    aria-hidden="true"
  >
    <span
      class="size-3 rounded-pill bg-faint transition-transform duration-(--duration-quick)
             ease-calm peer-checked:translate-x-3 peer-checked:bg-on-accent"
    ></span>
  </label>

  <div class="flex min-w-0 flex-col gap-0.5">
    <label for={id} class="cursor-pointer text-sm text-text select-none">{label}</label>
    {#if hint}
      <p class="text-xs text-faint">{hint}</p>
    {/if}
  </div>
</div>
