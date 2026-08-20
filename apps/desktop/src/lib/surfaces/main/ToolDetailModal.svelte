<script lang="ts">
  /**
   * Detalle de una herramienta del picker: cuerpo de la tool + ajustes propios.
   *
   * El chrome de identidad (título, blurb, icono) vive acá una sola vez.
   * `ToolModalChrome` marca a las tools hijas para que `ToolPage` no lo repita.
   */
  import { toolById, type ToolId } from "$core/tools";
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
  import { tabPanel } from "$lib/motion";
  import ToolModalChrome from "$patterns/ToolModalChrome.svelte";
  import ToolIcon from "$lib/ToolIcon.svelte";
  import Modal from "$ui/Modal.svelte";
  import SegmentedControl from "$ui/SegmentedControl.svelte";

  type Tab = "detail" | "settings";

  let {
    toolId,
    tab = $bindable<"detail" | "settings">("detail"),
    snippetsTab = "snippets",
    onClose,
    onOpenSettings,
  }: {
    toolId: ToolId;
    tab?: Tab;
    snippetsTab?: "snippets" | "scratchpad";
    onClose: () => void;
    onOpenSettings?: () => void;
  } = $props();

  const tool = $derived(localizeTool(toolById(toolId)));

  const TABS = $derived([
    { value: "detail" as const, label: t("tools.detailTab"), icon: toolId },
    { value: "settings" as const, label: t("chrome.settings"), icon: "settings" as const },
  ]);

  /** Agentes es chat a pantalla: sin Detalle/Ajustes ni blurb duplicado. */
  const chatOnly = $derived(toolId === "agents");
  const activeTab = $derived(chatOnly ? ("detail" as const) : tab);

  const titleId = $props.id();
</script>

<!--
  `fill` fija el panel al tope: sin eso, una altura en `vh` del body podía
  superar el max-h del diálogo y el overflow:hidden del chrome recortaba
  sin scrollbar. Agentes usa un panel más alto para el chat.
-->
<Modal
  title={tool.label}
  subtitle={tool.short}
  size="xl"
  scrollBody={false}
  fill
  panelMax={chatOnly ? "min(92dvh, 900px)" : "min(88dvh, 820px)"}
  {onClose}
>
  {#snippet header()}
    <div class="flex min-w-0 flex-1 flex-col gap-2">
      <div class="flex min-w-0 items-center gap-2">
        <div
          class="detail-mark flex size-7 shrink-0 items-center justify-center rounded-md
                 bg-surface-2 text-muted"
          aria-hidden="true"
        >
          <ToolIcon id={toolId} size={14} strokeWidth={1.4} />
        </div>
        <div class="min-w-0">
          <h2 id={titleId} class="truncate text-sm font-semibold leading-tight">
            {tool.label}
          </h2>
          {#if !chatOnly}
            <p class="truncate text-micro text-faint">{tool.blurb}</p>
          {/if}
        </div>
      </div>
      {#if !chatOnly}
        <SegmentedControl
          bind:value={tab}
          options={TABS}
          label={t("tools.detailView")}
          size="sm"
          full
        />
      {/if}
    </div>
  {/snippet}

  <div class="detail-body">
    {#key activeTab}
      <div class="tab-pane" in:tabPanel|local out:tabPanel|local>
        {#if activeTab === "detail"}
          <div class="tool-host">
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
          </div>
        {:else}
          <div class="settings-host">
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
                    onclick={() => {
                      onClose();
                      onOpenSettings();
                    }}
                  >
                    {t("tools.openAllSettings")}
                  </button>
                {/if}
              </div>
            {/if}
          </div>
        {/if}
      </div>
    {/key}
  </div>
</Modal>

<style>
  /*
   * Cancela el padding del body del Modal y llena el alto del panel (`fill`).
   * La altura la define el Modal — acá solo `flex:1; min-height:0` para que
   * ListDetail / grids scrolleen dentro sin recortes.
   */
  .detail-body {
    position: relative;
    display: flex;
    height: 100%;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    overflow: hidden;
    margin: -0.75rem -1rem;
  }

  .tab-pane {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    overflow: hidden;
    transform-origin: 50% 0;
  }

  .detail-mark :global(svg) {
    /* Óptica: el punto de reuniones / barras pesan un pelo abajo. */
    translate: 0 -0.5px;
  }

  .tool-host {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    overflow: hidden;
  }

  .settings-host {
    min-height: 0;
    flex: 1;
    overflow-y: auto;
    padding: 0.625rem 0.75rem 0.875rem;
  }
</style>
