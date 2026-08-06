<script lang="ts" generics="T extends string">
  /**
   * Elegir uno entre pocos. Reemplaza a `.rb-seg` y a las pestañas.
   *
   * Es un grupo de radios de verdad, no botones: las flechas del teclado
   * navegan entre opciones sin salir del grupo, que es lo que la gente espera
   * de un control así y lo que un `<button role="tab">` hay que programar.
   */
  import ToolIcon, { type IconId } from "$lib/ToolIcon.svelte";

  let {
    value = $bindable(),
    options,
    size = "md",
    full = false,
    label,
    onchange,
  }: {
    value: T;
    options: { value: T; label: string; disabled?: boolean; icon?: IconId }[];
    size?: "sm" | "md";
    full?: boolean;
    /** Para lectores de pantalla: qué se está eligiendo. */
    label: string;
    /** Para cuando elegir tiene un efecto y no solo cambia una variable. */
    onchange?: (value: T) => void;
  } = $props();

  const name = $props.id();
  const height = $derived(size === "sm" ? "h-6" : "h-7");
  const iconSize = $derived(size === "sm" ? 12 : 13);
</script>

<div
  role="radiogroup"
  aria-label={label}
  class="inline-flex rounded-sm border border-line bg-surface-2 p-[3px] {full
    ? 'w-full'
    : ''}"
>
  {#each options as option (option.value)}
    <label
      class="seg-opt relative flex flex-1 cursor-pointer items-center justify-center
             rounded-xs text-xs font-medium whitespace-nowrap
             transition-[color,background-color,transform]
             duration-(--duration-quick) ease-calm active:scale-[0.96]
             {height}
             {option.icon ? 'seg-opt--icon gap-1 pl-2 pr-2.5' : 'px-2.5'}
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
      {#if option.icon}
        <span class="seg-icon inline-grid shrink-0 place-items-center" aria-hidden="true">
          <ToolIcon id={option.icon} size={iconSize} strokeWidth={1.5} />
        </span>
      {/if}
      {option.label}
    </label>
  {/each}
</div>

<style>
  /* Óptica: un pelo menos padding del lado del icono. */
  .seg-opt--icon {
    padding-left: 0.45rem;
    padding-right: 0.6rem;
  }

  .seg-icon {
    /* Compensa el centro geométrico de SVGs con más tinta abajo. */
    translate: 0 -0.5px;
  }
</style>
