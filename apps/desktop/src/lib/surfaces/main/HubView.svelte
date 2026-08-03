<script lang="ts">
  /**
   * El hub: las seis herramientas como tarjetas.
   *
   * No se funde con nada, y eso es a propósito — la regla del sistema líquido
   * dice que dos formas se funden cuando una SALE de la otra, y estas están
   * puestas, no desplegadas.
   */
  import { TOOLS, type ToolId } from "$core/tools";

  let {
    onOpen,
    ready,
  }: {
    onOpen: (tool: ToolId) => void;
    /** Qué herramientas ya están reescritas. El resto se marca en obra. */
    ready: ToolId[];
  } = $props();
</script>

<div class="@container/hub h-full overflow-y-auto p-4">
  <div class="grid gap-2 @md/hub:grid-cols-2 @lg/hub:grid-cols-3">
    {#each TOOLS as tool (tool.id)}
      {@const enabled = ready.includes(tool.id)}
      <button
        type="button"
        disabled={!enabled}
        onclick={() => onOpen(tool.id)}
        class="flex flex-col items-start gap-1 rounded-md border border-line bg-surface p-3
               text-left transition-colors duration-(--duration-quick) ease-calm
               hover:border-line-strong hover:bg-surface-2
               disabled:pointer-events-none disabled:opacity-45"
      >
        <span class="text-micro text-faint uppercase">{tool.short}</span>
        <span class="text-md font-semibold text-text">{tool.label}</span>
        <span class="text-xs text-muted">{tool.blurb}</span>
        {#if !enabled}
          <span class="mt-1 text-micro text-faint uppercase">en obra</span>
        {/if}
      </button>
    {/each}
  </div>
</div>
