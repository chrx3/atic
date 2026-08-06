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
    class="relative mt-px inline-flex h-4 w-7 shrink-0 cursor-pointer items-center
           rounded-pill border p-px
           transition-[color,background-color,border-color,transform]
           duration-(--duration-quick) ease-calm active:scale-[0.96]
           before:absolute before:-inset-y-2 before:-inset-x-1 before:content-['']
           peer-disabled:cursor-not-allowed peer-disabled:opacity-45
           peer-focus-visible:outline peer-focus-visible:outline-2
           peer-focus-visible:outline-offset-2 peer-focus-visible:outline-accent
           {checked ? 'border-accent bg-accent' : 'border-line bg-surface-2'}"
    aria-hidden="true"
  >
    <!-- El estado se decide en JS y no con `peer-checked:`.
         `peer-*` compila a un selector de HERMANO (`~`), y la perilla es hija
         del label: el track cambiaba de color y ella se quedaba quieta. -->
    <span
      class="size-3 rounded-pill transition-transform duration-(--duration-quick) ease-calm
             {checked ? 'translate-x-3.5 bg-on-accent' : 'translate-x-0 bg-faint'}"
    ></span>
  </label>

  <div class="flex min-w-0 flex-col gap-0.5">
    <label for={id} class="cursor-pointer text-sm text-text select-none">{label}</label>
    {#if hint}
      <p class="text-xs text-faint">{hint}</p>
    {/if}
  </div>
</div>
