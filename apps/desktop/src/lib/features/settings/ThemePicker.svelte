<script lang="ts">
  /**
   * Elegir paleta.
   *
   * No es un `SegmentedControl` porque los temas ya no son tres, y sobre todo
   * porque un tema se elige mirándolo: cada opción lleva su `data-theme`, así
   * que la muestra se pinta con SUS tokens y no con los de la app. Es el mismo
   * truco del kitchen sink, y funciona por el `inline` de `tokens.css`.
   *
   * Grupo de radios de verdad, como `SegmentedControl`: las flechas del teclado
   * recorren las opciones sin salir del grupo.
   */
  import { onMount } from "svelte";
  import { resolveTheme, type UiTheme } from "$lib/theme";

  let {
    value,
    options,
    label,
    onchange,
  }: {
    value: UiTheme;
    /**
     * `colors` es para el personalizado, que no tiene paleta en el CSS: sus
     * tokens se pisan en la muestra igual que los pisa el root.
     */
    options: {
      value: UiTheme;
      label: string;
      colors?: Record<string, string>;
    }[];
    /** Para lectores de pantalla: qué se está eligiendo. */
    label: string;
    onchange: (value: UiTheme) => void;
  } = $props();

  const name = $props.id();

  // "Sistema" no tiene paleta propia: la muestra enseña la que se aplicaría.
  let systemTheme = $state<UiTheme>("dark");
  onMount(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const sync = () => (systemTheme = resolveTheme("system"));
    sync();
    mq.addEventListener("change", sync);
    return () => mq.removeEventListener("change", sync);
  });

  const preview = (theme: UiTheme) => (theme === "system" ? systemTheme : theme);

  const swatchStyle = (colors?: Record<string, string>) =>
    colors
      ? Object.entries(colors)
          .map(([token, value]) => `--${token}: ${value}`)
          .join("; ")
      : undefined;
</script>

<!-- Se acomoda al ancho del panel: dos columnas en el modal angosto, tres o
     más cuando hay sitio. -->
<div
  role="radiogroup"
  aria-label={label}
  class="grid grid-cols-[repeat(auto-fill,minmax(8rem,1fr))] gap-1.5"
>
  {#each options as option (option.value)}
    <label
      class="flex cursor-pointer items-center gap-2 rounded-sm border p-1.5
             transition-colors duration-(--duration-quick) ease-calm
             {option.value === value
        ? 'border-line-strong bg-elevated text-text shadow-card'
        : 'border-line text-muted hover:text-text'}"
    >
      <input
        type="radio"
        {name}
        class="sr-only"
        value={option.value}
        checked={option.value === value}
        onchange={() => onchange(option.value)}
      />
      <!-- Papel, superficie, acento y tinta: los cuatro que se miran al elegir. -->
      <span
        data-theme={preview(option.value)}
        style={swatchStyle(option.colors)}
        aria-hidden="true"
        class="flex h-7 w-9 shrink-0 flex-col justify-between rounded-xs border
               border-line bg-bg p-1"
      >
        <span class="h-1.5 rounded-xs bg-surface-2"></span>
        <span class="flex items-center gap-1">
          <span class="h-1.5 w-1.5 rounded-pill bg-accent"></span>
          <span class="h-1 flex-1 rounded-pill bg-muted"></span>
        </span>
      </span>
      <span class="truncate text-xs font-medium">{option.label}</span>
    </label>
  {/each}
</div>
