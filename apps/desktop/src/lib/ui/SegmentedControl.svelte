<script lang="ts" generics="T extends string">
  /**
   * Elegir uno entre pocos. Reemplaza a `.rb-seg` y a las pestañas.
   *
   * Es un grupo de radios de verdad, no botones: las flechas del teclado
   * navegan entre opciones sin salir del grupo, que es lo que la gente espera
   * de un control así y lo que un `<button role="tab">` hay que programar.
   */
  let {
    value = $bindable(),
    options,
    size = "md",
    full = false,
    label,
    onchange,
  }: {
    value: T;
    options: { value: T; label: string; disabled?: boolean }[];
    size?: "sm" | "md";
    full?: boolean;
    /** Para lectores de pantalla: qué se está eligiendo. */
    label: string;
    /** Para cuando elegir tiene un efecto y no solo cambia una variable. */
    onchange?: (value: T) => void;
  } = $props();

  const name = $props.id();
  const height = $derived(size === "sm" ? "h-6" : "h-7");
</script>

<div
  role="radiogroup"
  aria-label={label}
  class="inline-flex rounded-sm border border-line bg-surface-2 p-px {full
    ? 'w-full'
    : ''}"
>
  {#each options as option (option.value)}
    <label
      class="relative flex flex-1 cursor-pointer items-center justify-center rounded-xs px-2.5
             text-xs font-medium whitespace-nowrap
             transition-colors duration-(--duration-quick) ease-calm
             {height}
             {option.value === value
        ? 'bg-elevated text-text shadow-card'
        : 'text-muted hover:text-text'}
             {option.disabled ? 'pointer-events-none opacity-45' : ''}"
    >
      <input
        type="radio"
        {name}
        class="sr-only"
        value={option.value}
        checked={option.value === value}
        disabled={option.disabled}
        onchange={() => {
          value = option.value;
          onchange?.(option.value);
        }}
      />
      {option.label}
    </label>
  {/each}
</div>
