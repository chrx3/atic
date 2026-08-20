<script lang="ts">
  /**
   * La ventana principal.
   *
   * Shell: picker líquido (rueda + cards). El detalle de cada tool y sus
   * ajustes viven en un modal. El estado de dominio lo monta `sessionEffect`.
   */
  import { untrack } from "svelte";
  import { toolById } from "$core/tools";
  import { localizeTool, t } from "$domain/i18n.svelte";
  import { LAUNCHER_LAB_OPEN_KEY } from "$lib/dev/launcherLab.svelte";
  import { pickerLab } from "$lib/dev/pickerLab.svelte";
  import { config } from "$domain/config.svelte";
  import { recordings } from "$domain/recordings.svelte";
  import { sessionEffect } from "$domain/session";
  import { toastError, toasts } from "$domain/toasts.svelte";
  import OnboardingModal from "$features/onboarding/OnboardingModal.svelte";
  import SearchModal from "$features/search/SearchModal.svelte";
  import { onOpenSearchRequested } from "$ipc/search";
  import { appUpdate } from "$domain/appUpdate.svelte";
  import { closeWindow, minimizeWindow, toggleMaximizeWindow } from "$ipc/windows";
  import WindowFrame from "$patterns/WindowFrame.svelte";
  import Icon from "$ui/Icon.svelte";
  import IconButton from "$ui/IconButton.svelte";
  import Modal from "$ui/Modal.svelte";
  import ToastStack from "$ui/ToastStack.svelte";
  import { AppWindow, GraduationCap, Search, Settings, SlidersHorizontal } from "$lib/icons";
  import ToolDetailModal from "./ToolDetailModal.svelte";
  import ToolRail from "./ToolRail.svelte";
  import UpdateBubble from "./UpdateBubble.svelte";
  import { provideMainUi } from "./mainUi.svelte";

  const ui = provideMainUi();
  const isDev = import.meta.env.DEV;
  let launcherLabOpen = $state(false);

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

  $effect(() => {
    if (!isDev) return;
    const sync = () => {
      launcherLabOpen = localStorage.getItem(LAUNCHER_LAB_OPEN_KEY) === "1";
    };
    sync();
    window.addEventListener("storage", sync);
    return () => window.removeEventListener("storage", sync);
  });

  function toggleLauncherLab() {
    if (!isDev) return;
    const on = localStorage.getItem(LAUNCHER_LAB_OPEN_KEY) === "1";
    if (on) localStorage.removeItem(LAUNCHER_LAB_OPEN_KEY);
    else localStorage.setItem(LAUNCHER_LAB_OPEN_KEY, "1");
    launcherLabOpen = !on;
    window.dispatchEvent(
      new StorageEvent("storage", {
        key: LAUNCHER_LAB_OPEN_KEY,
        newValue: on ? null : "1",
      }),
    );
  }

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

  // Ctrl+K / titlebar: buscador in-app. El tool Apps usa el launcher de sistema.
  $effect(() => {
    let stop: (() => void) | undefined;
    void onOpenSearchRequested(() => ui.openSearch()).then((un) => {
      stop = un;
    });
    return () => stop?.();
  });

  const tool = $derived(localizeTool(toolById(ui.activeTool)));
  const onboardingDone = $derived(config.current?.onboarding_done === true);

  $effect(() => {
    if (isDev) return;
    if (!onboardingDone) return;
    // `startPolling` consulta en el acto, y ese `check()` lee y escribe
    // `checking`/`error` del store. Sin `untrack` el efecto se invalida a sí
    // mismo —escribe lo que acaba de leer, y el teardown lo vuelve a
    // escribir—, así que Svelte aborta el árbol de efectos entero con
    // `effect_update_depth_exceeded`. No se cae solo el aviso de update: se
    // queda sin reactividad TODA la ventana (picker, modales, layout).
    return untrack(() => appUpdate.startPolling());
  });

  function onKeydown(event: KeyboardEvent) {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
      event.preventDefault();
      ui.openSearch();
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
  minimizeLabel={t("chrome.minimize")}
  maximizeLabel={t("chrome.maximize")}
  closeLabel={t("chrome.close")}
  onMinimize={() => void minimizeWindow()}
  onMaximize={() => void toggleMaximizeWindow()}
  onClose={() => void closeWindow()}
>
  {#snippet actions()}
    <IconButton label={t("chrome.search")} size="sm" onclick={() => ui.openSearch()}>
      <Icon icon={Search} size={14} />
    </IconButton>

    <IconButton label={t("chrome.settings")} size="sm" onclick={() => ui.openSettings()}>
      <Icon icon={Settings} size={14} />
    </IconButton>

    <IconButton
      label={t("chrome.replayTutorial")}
      size="sm"
      pressed={Boolean(config.current && !config.current.onboarding_done)}
      onclick={() => void ui.replayOnboarding().catch(toastError)}
    >
      <Icon icon={GraduationCap} size={14} />
    </IconButton>

    {#if isDev}
      <IconButton
        label={launcherLabOpen ? t("chrome.launcherLabClose") : t("chrome.launcherLab")}
        size="sm"
        pressed={launcherLabOpen}
        onclick={() => toggleLauncherLab()}
      >
        <Icon icon={AppWindow} size={14} />
      </IconButton>
      <IconButton
        label={pickerLab.open ? t("chrome.pickerLabClose") : t("chrome.pickerLab")}
        size="sm"
        pressed={pickerLab.open}
        onclick={() => pickerLab.toggle()}
      >
        <Icon icon={SlidersHorizontal} size={14} />
      </IconButton>
    {/if}
  {/snippet}

  <div class="shell">
    <ToolRail
      activeTool={ui.activeTool}
      onSelect={(id) => ui.openTool(id)}
      onOpenDetail={(id) => ui.openDetail(id)}
    />
    <UpdateBubble />
  </div>
</WindowFrame>

{#if config.current && !config.current.onboarding_done}
  {#key ui.onboardingReplay}
    <OnboardingModal
      replay={ui.replayingOnboarding}
      onDone={() => {
        ui.replayingOnboarding = false;
        toasts.push(t("onboarding.nowPractice"));
      }}
    />
  {/key}
{/if}

{#if ui.searchOpen}
  <SearchModal
    onClose={() => ui.closeSearch()}
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
    onOpenSettings={() => ui.openSettings()}
  />
{/if}

{#if ui.settingsOpen}
  <Modal
    title={t("chrome.settings")}
    size="lg"
    fill
    scrollBody={false}
    onClose={() => ui.closeSettings()}
  >
    <div class="-mx-4 -my-3 flex h-full min-h-0 flex-1 flex-col overflow-hidden">
      {#await import("$features/settings/SettingsPanel.svelte") then { default: SettingsPanel }}
        {#key ui.settingsSection}
          <SettingsPanel initialSection={ui.settingsSection} />
        {/key}
      {/await}
    </div>
  </Modal>
{/if}

<ToastStack items={toasts.items} onDismiss={(id) => toasts.dismiss(id)} />

{#if isDev && pickerLab.open && PickerLabPanel}
  <PickerLabPanel />
{/if}

<style>
  .shell {
    position: relative;
    display: flex;
    height: 100%;
    min-height: 0;
  }
</style>
