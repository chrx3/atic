<script lang="ts" generics="T extends string">
  /**
   * La navegación entre secciones de ajustes.
   *
   * Es una lista vertical arriba de cierto ancho y un `SegmentedControl`
   * horizontal debajo, decidido por **container query**: el mismo panel de
   * ajustes puede vivir en una ventana ancha o en un modal angosto.
   */
  import ToolIcon, { type IconId } from "$lib/ToolIcon.svelte";
  import SegmentedControl from "$ui/SegmentedControl.svelte";

  let {
    value = $bindable(),
    sections,
  }: {
    value: T;
    sections: { value: T; label: string; icon?: IconId }[];
  } = $props();
</script>

<!-- Angosto: una fila de pestañas. -->
<div class="p-3 pb-0 @md/settings:hidden">
  <SegmentedControl bind:value options={sections} label="Sección de ajustes" full />
</div>

<!-- Ancho: una columna. -->
<nav
  aria-label="Secciones de ajustes"
  class="hidden w-44 shrink-0 flex-col gap-0.5 border-r border-line p-2
         @md/settings:flex"
>
  {#each sections as section (section.value)}
    <button
      type="button"
      aria-current={section.value === value ? "page" : undefined}
      onclick={() => (value = section.value)}
      class="nav-row relative flex min-h-9 items-center gap-2 rounded-sm px-2 py-1.5
             text-left text-sm
             transition-[color,background-color,transform]
             duration-(--duration-quick) ease-calm active:scale-[0.96]
             {section.value === value
        ? 'bg-surface-2 text-text'
        : 'text-muted hover:bg-surface-2 hover:text-text'}"
    >
      {#if section.icon}
        <span class="nav-icon inline-grid shrink-0 place-items-center" aria-hidden="true">
          <ToolIcon id={section.icon} size={15} strokeWidth={1.4} />
        </span>
      {/if}
      <span class="min-w-0 truncate">{section.label}</span>
    </button>
  {/each}
</nav>

<style>
  .nav-icon {
    translate: 0 -0.5px;
  }
</style>
