<script lang="ts">
  /**
   * El contenido de una herramienta, a ventana completa.
   *
   * Reemplaza al modal de detalle. El modal servía cuando lo que se abría era
   * una ficha, pero lo que hay adentro es una biblioteca: con doscientas
   * capturas o una transcripción de una hora, un panel de 896 px en medio de
   * una ventana maximizada es la limitación y no el foco. Además obligaba a
   * cerrar para cambiar de herramienta y apilaba diálogo sobre diálogo.
   *
   * Vive DENTRO del marco de la ventana y no en la top layer: la barra de
   * título sigue arrastrando, y los diálogos de verdad —confirmar, ver la
   * transcripción— siguen abriéndose por encima de esto.
   */
  import { TOOLS, type ToolId } from "$core/tools";
  import { localizeTool, t } from "$domain/i18n.svelte";
  import AgentsTool from "$features/agents/AgentsTool.svelte";
  import CapturesTool from "$features/captures/CapturesTool.svelte";
  import ClipboardTool from "$features/clipboard/ClipboardTool.svelte";
  import DictationTool from "$features/dictation/DictationTool.svelte";
  import MeetingsTool from "$features/meetings/MeetingsTool.svelte";
  import CapturesSection from "$features/settings/CapturesSection.svelte";
  import DictationSection from "$features/settings/DictationSection.svelte";
  import MeetingsSection from "$features/settings/MeetingsSection.svelte";
  import ShortcutsSection from "$features/settings/ShortcutsSection.svelte";
  import SnippetsTool from "$features/snippets/SnippetsTool.svelte";
  import { House, Settings } from "$lib/icons";
  import { tabPanel } from "$lib/motion";
  import ToolModalChrome from "$patterns/ToolModalChrome.svelte";
  import Icon from "$ui/Icon.svelte";
  import IconButton from "$ui/IconButton.svelte";
  import SegmentedControl from "$ui/SegmentedControl.svelte";

  type Tab = "detail" | "settings";

  let {
    toolId,
    tab = $bindable<Tab>("detail"),
    snippetsTab = "snippets",
    onClose,
    onSelectTool,
    onOpenSettings,
  }: {
    toolId: ToolId;
    tab?: Tab;
    snippetsTab?: "snippets" | "scratchpad";
    onClose: () => void;
    onSelectTool: (tool: ToolId) => void;
    onOpenSettings?: () => void;
  } = $props();

  /** Las que tienen cuerpo propio. Pizarra y Apps son una acción, no una vista. */
  const BODIED = new Set<ToolId>([
    "meetings",
    "captures",
    "clipboard",
    "snippets",
    "dictation",
    "agents",
  ]);

  const tabs = $derived(
    TOOLS.filter((item) => BODIED.has(item.id))
      .map(localizeTool)
      .map((item) => ({ value: item.id, label: item.label, icon: item.id })),
  );

  const tool = $derived(
    localizeTool(TOOLS.find((item) => item.id === toolId) ?? TOOLS[0]),
  );

  /** Agentes es chat a pantalla: no tiene ajustes propios que valga separar. */
  const chatOnly = $derived(toolId === "agents");
  const activeTab = $derived(chatOnly ? ("detail" as const) : tab);

  // Espejo del tool activo: el control segmentado escribe acá —por eso es un
  // derived escribible— y el cambio real lo hace `onSelectTool`, así el estado
  // sigue viviendo arriba y esta copia se vuelve a alinear sola.
  let picked = $derived(toolId);

  /**
   * Esc vuelve a la rueda, pero solo si no hay un diálogo encima.
   *
   * Los `<dialog>` nativos manejan su propio Esc y el keydown igual burbujea
   * hasta acá: sin esta guarda, cerrar una confirmación cerraría también la
   * herramienta que la abrió.
   */
  function onKeydown(event: KeyboardEvent) {
    if (event.key !== "Escape" || event.defaultPrevented) return;
    if (document.querySelector("dialog[open]")) return;
    event.preventDefault();
    onClose();
  }
</script>

<svelte:window onkeydown={onKeydown} />

<section class="ws" aria-label={tool.label}>
  <div class="ws-bar">
    <IconButton label={t("workspace.back")} size="sm" onclick={onClose}>
      <Icon icon={House} size={14} />
    </IconButton>

    <div class="ws-tabs">
      <SegmentedControl
        bind:value={picked}
        options={tabs}
        label={t("workspace.tools")}
        size="sm"
        full
        onchange={(id) => onSelectTool(id)}
      />
    </div>

    {#if !chatOnly}
      <IconButton
        label={t("chrome.settings")}
        size="sm"
        pressed={tab === "settings"}
        onclick={() => (tab = tab === "settings" ? "detail" : "settings")}
      >
        <Icon icon={Settings} size={14} />
      </IconButton>
    {/if}
  </div>

  <div class="ws-body">
    {#key `${toolId}:${activeTab}`}
      <div class="ws-pane" in:tabPanel|local out:tabPanel|local>
        {#if activeTab === "detail"}
          <ToolModalChrome>
            {#if toolId === "meetings"}
              <MeetingsTool {onOpenSettings} />
            {:else if toolId === "dictation"}
              <DictationTool />
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
        {:else}
          <div class="ws-settings">
            <div class="ws-settings-inner">
              {#if toolId === "meetings"}
                <MeetingsSection />
              {:else if toolId === "dictation"}
                <DictationSection />
              {:else if toolId === "captures"}
                <CapturesSection />
              {:else}
                <div class="flex flex-col gap-3">
                  <p class="text-sm text-muted">
                    {t("tools.noOwnSettings", { label: tool.label })}
                  </p>
                  <ShortcutsSection />
                  {#if onOpenSettings}
                    <button
                      type="button"
                      class="self-start text-xs font-medium text-text underline-offset-2
                             hover:underline"
                      onclick={onOpenSettings}
                    >
                      {t("tools.openAllSettings")}
                    </button>
                  {/if}
                </div>
              {/if}
            </div>
          </div>
        {/if}
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

  .ws-bar {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    gap: 0.5rem;
    border-bottom: 1px solid var(--line);
    padding: 0.375rem 0.5rem;
  }

  /* Tope: con la ventana maximizada, seis pestañas estiradas a 1900px se
     leen como una barra de filtros y no como navegación. */
  .ws-tabs {
    min-width: 0;
    flex: 1;
    max-width: 44rem;
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

  .ws-settings {
    min-height: 0;
    flex: 1;
    overflow-y: auto;
  }

  /* Los ajustes son texto y controles: a ancho completo quedan ilegibles. */
  .ws-settings-inner {
    max-width: 42rem;
    padding: 0.875rem 1rem 1.25rem;
  }
</style>
