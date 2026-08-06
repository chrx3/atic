<script lang="ts">
  /**
   * Detalle de una herramienta del picker: cuerpo de la tool + ajustes propios.
   */
  import { toolById, type ToolId } from "$core/tools";
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

  const tool = $derived(toolById(toolId));

  const TABS = $derived([
    { value: "detail" as const, label: "Detalle", icon: toolId },
    { value: "settings" as const, label: "Ajustes", icon: "settings" as const },
  ]);

  const hasOwnSettings = $derived(
    toolId === "meetings" || toolId === "dictation" || toolId === "captures",
  );

  /** Agentes es chat a pantalla: sin Detalle/Ajustes ni blurb duplicado. */
  const chatOnly = $derived(toolId === "agents");
  const activeTab = $derived(chatOnly ? ("detail" as const) : tab);

  const titleId = $props.id();
</script>

<Modal title={tool.label} subtitle={tool.short} size="xl" scrollBody={false} {onClose}>
  {#snippet header()}
    <div class="flex min-w-0 flex-1 flex-col gap-3">
      <div class="flex min-w-0 items-start gap-3">
        <div
          class="detail-mark flex size-9 shrink-0 items-center justify-center rounded-md
                 bg-surface-2 text-muted"
          aria-hidden="true"
        >
          <ToolIcon id={toolId} size={18} strokeWidth={1.4} />
        </div>
        <div class="min-w-0">
          <h2 id={titleId} class="text-balance text-md font-semibold">{tool.label}</h2>
          {#if !chatOnly}
            <p class="mt-0.5 text-xs text-muted text-pretty">{tool.blurb}</p>
          {/if}
        </div>
      </div>
      {#if !chatOnly}
        <SegmentedControl
          bind:value={tab}
          options={TABS}
          label="Vista del detalle"
          size="sm"
        />
      {/if}
    </div>
  {/snippet}

  <div class="detail-body" class:is-chat={chatOnly}>
    {#key activeTab}
      <div class="tab-pane" in:tabPanel|local out:tabPanel|local>
        {#if activeTab === "detail"}
          <div class="tool-host">
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
                  {#if hasOwnSettings}
                    Ajustes de {tool.label}.
                  {:else}
                    {tool.label} no tiene una sección propia. Acá están los atajos del
                    sistema; el resto vive en Ajustes generales.
                  {/if}
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
                    Abrir todos los ajustes
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
  .detail-body {
    position: relative;
    display: flex;
    height: min(70vh, calc(100dvh - 9rem));
    min-height: 0;
    flex: 1;
    flex-direction: column;
    overflow: hidden;
  }

  .detail-body.is-chat {
    height: min(78vh, calc(100dvh - 7rem));
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
    min-height: 0;
    flex: 1;
    overflow: hidden;
  }

  .settings-host {
    min-height: 0;
    flex: 1;
    overflow-y: auto;
    padding: 0.25rem 0.15rem 0.5rem;
  }
</style>
