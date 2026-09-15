<script lang="ts">
  /**
   * El contenido de una herramienta, a ventana completa.
   *
   * Ya no tiene barra propia: el selector de herramientas vive en el menú del
   * título (`WindowFrame.titleMenu`) y los ajustes de cada herramienta en
   * Ajustes, que es donde el sistema los espera. Esta capa solo decide qué
   * cuerpo se muestra, y lo cubre todo para que el picker de la pill no se
   * dibuje debajo.
   */
  import { TOOLS, type ToolId } from "$core/tools";
  import { localizeTool } from "$domain/i18n.svelte";
  import AgentsTool from "$features/agents/AgentsTool.svelte";
  import CapturesTool from "$features/captures/CapturesTool.svelte";
  import ClipboardTool from "$features/clipboard/ClipboardTool.svelte";
  import DictationTool from "$features/dictation/DictationTool.svelte";
  import MeetingsTool from "$features/meetings/MeetingsTool.svelte";
  import SnippetsTool from "$features/snippets/SnippetsTool.svelte";
  import type { SettingsSectionId } from "$features/settings/settingsSections";
  import { tabPanel } from "$lib/motion";
  import ToolModalChrome from "$patterns/ToolModalChrome.svelte";

  let {
    toolId,
    snippetsTab = "snippets",
    onOpenSettings,
  }: {
    toolId: ToolId;
    snippetsTab?: "snippets" | "scratchpad";
    /** Con sección: cada herramienta abre la suya (motor de reuniones, etc.). */
    onOpenSettings?: (section?: SettingsSectionId) => void;
  } = $props();

  const tool = $derived(
    localizeTool(TOOLS.find((item) => item.id === toolId) ?? TOOLS[0]),
  );
</script>

<section class="ws" aria-label={tool.label}>
  <div class="ws-body">
    {#key toolId}
      <div class="ws-pane" in:tabPanel|local out:tabPanel|local>
        <ToolModalChrome>
          {#if toolId === "meetings"}
            <MeetingsTool {onOpenSettings} />
          {:else if toolId === "dictation"}
            <DictationTool {onOpenSettings} />
          {:else if toolId === "clipboard"}
            <ClipboardTool />
          {:else if toolId === "snippets"}
            <SnippetsTool initialTab={snippetsTab} />
          {:else if toolId === "captures"}
            <CapturesTool />
          {:else if toolId === "agents"}
            <AgentsTool />
          {/if}
        </ToolModalChrome>
      </div>
    {/key}
  </div>
</section>

<style>
  /*
   * Cubre el picker sin desmontarlo: la rueda conserva su posición y su
   * estado, y volver no la vuelve a animar desde cero.
   */
  .ws {
    position: absolute;
    inset: 0;
    z-index: 2;
    display: flex;
    min-height: 0;
    flex-direction: column;
    background: var(--bg);
  }

  .ws-body {
    position: relative;
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    overflow: hidden;
  }

  .ws-pane {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    overflow: hidden;
    transform-origin: 50% 0;
  }
</style>
