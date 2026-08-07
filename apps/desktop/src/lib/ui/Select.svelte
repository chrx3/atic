<script lang="ts" generics="T extends string">
  /**
   * Elegir uno entre muchos.
   *
   * Es un `<select>` nativo y no un menú propio: el desplegable del sistema
   * sabe buscar escribiendo, se abre donde entra en pantalla y funciona con
   * lector de pantalla sin que nadie lo programe. Para elegir entre pocas
   * opciones visibles a la vez está `SegmentedControl`; para un menú con
   * contenido rico, `Menu`.
   */
  import Icon from "$ui/Icon.svelte";
  import { ChevronDown } from "$lib/icons";

  let {
    value = $bindable(),
    options,
    disabled = false,
    invalid = false,
    ...rest
  }: {
    value: T;
    options: { value: T; label: string; disabled?: boolean }[];
    disabled?: boolean;
    invalid?: boolean;
    [key: string]: unknown;
  } = $props();
</script>

<div class="relative">
  <select
    bind:value
    {disabled}
    aria-invalid={invalid ? "true" : undefined}
    class="h-8 w-full appearance-none rounded-sm border bg-surface-2 pr-7 pl-2 text-sm
           text-text transition-colors duration-(--duration-quick) ease-calm
           disabled:opacity-45
           {invalid ? 'border-danger' : 'border-line focus:border-line-strong'}"
    {...rest}
  >
    {#each options as option (option.value)}
      <option value={option.value} disabled={option.disabled}>{option.label}</option>
    {/each}
  </select>

  <Icon
    icon={ChevronDown}
    size={10}
    class="pointer-events-none absolute top-1/2 right-2 -translate-y-1/2 text-faint"
    aria-hidden="true"
  />
</div>
