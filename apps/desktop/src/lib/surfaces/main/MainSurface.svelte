<script lang="ts">
  /**
   * La ventana principal.
   *
   * Shell: picker líquido (rueda + cards). El detalle de cada tool y sus
   * ajustes viven en un modal. El estado de dominio lo monta `sessionEffect`.
   */
  import { toolById } from "$core/tools";
  import { pickerLab } from "$lib/dev/pickerLab.svelte";
  import { config } from "$domain/config.svelte";
  import { recordings } from "$domain/recordings.svelte";
  import { sessionEffect } from "$domain/session";
  import { toasts } from "$domain/toasts.svelte";
  import OnboardingModal from "$features/onboarding/OnboardingModal.svelte";
  import SearchModal from "$features/search/SearchModal.svelte";
  import SettingsPanel from "$features/settings/SettingsPanel.svelte";
  import { closeWindow, minimizeWindow, toggleMaximizeWindow } from "$ipc/windows";
  import WindowFrame from "$patterns/WindowFrame.svelte";
  import IconButton from "$ui/IconButton.svelte";
  import Modal from "$ui/Modal.svelte";
  import ToastStack from "$ui/ToastStack.svelte";
  import ToolDetailModal from "./ToolDetailModal.svelte";
  import ToolRail from "./ToolRail.svelte";
  import { provideMainUi } from "./mainUi.svelte";

  const ui = provideMainUi();
  const isDev = import.meta.env.DEV;

  // Panel estático en dev: sin dynamic import que pueda dejar la UI a medias.
  let PickerLabPanel = $state<typeof import("$lib/dev/PickerLabPanel.svelte").default | null>(
    null,
  );
  $effect(() => {
    if (!isDev) return;
    let cancelled = false;
    void import("$lib/dev/PickerLabPanel.svelte").then((m) => {
      if (!cancelled) PickerLabPanel = m.default;
    });
    return () => {
      cancelled = true;
    };
  });

  $effect(() =>
    sessionEffect([
      "config",
      "recordings",
      "models",
      "capture",
      "dictation",
      "clipboard",
      "snippets",
      "captures",
      "summaries",
    ]),
  );

  const tool = $derived(toolById(ui.activeTool));

  let settingsOpen = $state(false);
  let searchOpen = $state(false);

  function onKeydown(event: KeyboardEvent) {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
      event.preventDefault();
      searchOpen = true;
    }
    if (!isDev) return;
    if (event.key === "Escape" && pickerLab.open) {
      event.preventDefault();
      pickerLab.close();
      return;
    }
    if (event.ctrlKey && event.altKey && event.key.toLowerCase() === "p") {
      event.preventDefault();
      pickerLab.toggle();
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<WindowFrame
  title={tool.label}
  onMinimize={() => void minimizeWindow()}
  onMaximize={() => void toggleMaximizeWindow()}
  onClose={() => void closeWindow()}
>
  {#snippet actions()}
    <IconButton label="Buscar (Ctrl+K)" size="sm" onclick={() => (searchOpen = true)}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" aria-hidden="true">
        <circle cx="11" cy="11" r="6" stroke="currentColor" stroke-width="1.8" />
        <path
          d="m20 20-3.5-3.5"
          stroke="currentColor"
          stroke-width="1.8"
          stroke-linecap="round"
        />
      </svg>
    </IconButton>

    <IconButton label="Ajustes" size="sm" onclick={() => (settingsOpen = true)}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" aria-hidden="true">
        <circle cx="12" cy="12" r="3" stroke="currentColor" stroke-width="1.8" />
        <path
          d="M12 3v2M12 19v2M3 12h2M19 12h2M5.6 5.6l1.4 1.4M17 17l1.4 1.4M18.4 5.6L17 7M7 17l-1.4 1.4"
          stroke="currentColor"
          stroke-width="1.8"
          stroke-linecap="round"
        />
      </svg>
    </IconButton>

    {#if isDev}
      <IconButton
        label={pickerLab.open ? "Cerrar ajuste picker" : "Ajustar rueda y cards"}
        size="sm"
        pressed={pickerLab.open}
        onclick={() => pickerLab.toggle()}
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" aria-hidden="true">
          <path
            d="M4 8h16M4 16h10"
            stroke="currentColor"
            stroke-width="1.8"
            stroke-linecap="round"
          />
          <circle cx="18" cy="16" r="2.5" stroke="currentColor" stroke-width="1.8" />
          <circle cx="10" cy="8" r="2.5" stroke="currentColor" stroke-width="1.8" />
        </svg>
      </IconButton>
    {/if}
  {/snippet}

  <div class="shell">
    <ToolRail
      activeTool={ui.activeTool}
      onSelect={(id) => ui.openTool(id)}
      onOpenDetail={(id) => ui.openDetail(id)}
    />
  </div>
</WindowFrame>

{#if config.current && !config.current.onboarding_done}
  <OnboardingModal onDone={() => toasts.push("Listo. Podés grabar cuando quieras.")} />
{/if}

{#if searchOpen}
  <SearchModal
    onClose={() => (searchOpen = false)}
    onNavigate={(hit) => {
      if (hit.kind === "recording") {
        recordings.select(hit.id);
        ui.openDetail("meetings");
      } else if (hit.kind === "scratchpad") {
        ui.snippetsTab = "scratchpad";
        ui.openDetail("snippets");
      }
    }}
  />
{/if}

{#if ui.detailTool}
  <ToolDetailModal
    toolId={ui.detailTool}
    bind:tab={ui.detailTab}
    snippetsTab={ui.snippetsTab}
    onClose={() => ui.closeDetail()}
    onOpenSettings={() => (settingsOpen = true)}
  />
{/if}

{#if settingsOpen}
  <Modal
    title="Ajustes"
    size="lg"
    fill
    scrollBody={false}
    onClose={() => (settingsOpen = false)}
  >
    <div class="-mx-4 -my-3 flex h-full min-h-0 flex-1 flex-col overflow-hidden">
      <SettingsPanel />
    </div>
  </Modal>
{/if}

<ToastStack items={toasts.items} onDismiss={(id) => toasts.dismiss(id)} />

{#if isDev && pickerLab.open && PickerLabPanel}
  <PickerLabPanel />
{/if}

<style>
  .shell {
    display: flex;
    height: 100%;
    min-height: 0;
  }
</style>
