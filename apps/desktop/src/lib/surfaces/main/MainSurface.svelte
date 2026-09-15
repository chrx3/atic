<script lang="ts">
  /**
   * La ventana principal.
   *
   * Ya no hay picker: la ventana muestra directamente el cuerpo de una
   * herramienta (Reuniones por defecto) con las pestañas del workspace para
   * cambiar entre las que tienen vista. Todo lo demás vive en la pill: esto es
   * la biblioteca donde el contenido grande se lee y se edita.
   *
   * El estado de dominio lo monta `sessionEffect`.
   */
  import { onMount, untrack } from "svelte";
  import { BODIED_TOOLS, toolById, type ToolId } from "$core/tools";
  import { localizeTool, t } from "$domain/i18n.svelte";
  import type { SettingsSectionId } from "$features/settings/settingsSections";
  import { LAUNCHER_LAB_OPEN_KEY } from "$lib/dev/launcherLab.svelte";
  import { capture } from "$domain/capture.svelte";
  import { config } from "$domain/config.svelte";
  import { dictation } from "$domain/dictation.svelte";
  import { recordings } from "$domain/recordings.svelte";
  import { sessionEffect } from "$domain/session";
  import { toastError, toasts } from "$domain/toasts.svelte";
  import OnboardingModal from "$features/onboarding/OnboardingModal.svelte";
  import PermissionsModal from "$features/permissions/PermissionsModal.svelte";
  import { hasMissingPermissions } from "$features/permissions/model";
  import SearchModal from "$features/search/SearchModal.svelte";
  import { onOpenSearchRequested } from "$ipc/search";
  import { macPermissionsStatus } from "$ipc/permissions";
  import { appUpdate } from "$domain/appUpdate.svelte";
  import {
    closeWindow,
    minimizeWindow,
    onWindowFocus,
    setMainWindowFocused,
    toggleMaximizeWindow,
    windowIsFocused,
  } from "$ipc/windows";
  import AticMark from "$lib/AticMark.svelte";
  import WindowFrame from "$patterns/WindowFrame.svelte";
  import Icon from "$ui/Icon.svelte";
  import IconButton from "$ui/IconButton.svelte";
  import Modal from "$ui/Modal.svelte";
  import ToastStack from "$ui/ToastStack.svelte";
  import { AppWindow, ChevronDown, GraduationCap, Search, Settings } from "$lib/icons";
  import { TOOL_ICONS } from "$lib/icons";
  import Menu from "$ui/Menu.svelte";
  import type { MenuItem } from "$ui/menu";
  import ToolWorkspace from "./ToolWorkspace.svelte";
  import UpdateBubble from "./UpdateBubble.svelte";
  import { provideMainUi } from "./mainUi.svelte";

  const ui = provideMainUi();
  const isDev = import.meta.env.DEV;

  /* Misma cuenta que la pill: la cara dice qué está corriendo ahora. */
  const markState = $derived<"idle" | "recording" | "dictating">(
    capture.active ? "recording" : dictation.active ? "dictating" : "idle",
  );
  let launcherLabOpen = $state(false);

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
      // La ventana muestra lo que quedó en la cola de pegado del dictado.
      "paste",
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

  /**
   * El dictado decide en Rust si pega en la app externa o si deja el texto en
   * Atic. Desde su hilo no puede preguntar si esta ventana tiene el foco, así
   * que se lo reportamos en cada cambio y una vez al arrancar.
   */
  $effect(() => {
    let stop: (() => void) | undefined;
    void windowIsFocused()
      .then((focused) => setMainWindowFocused(focused))
      .catch(() => {
        /* fuera de Tauri */
      });
    void onWindowFocus((focused) => void setMainWindowFocused(focused)).then((un) => {
      stop = un;
    });
    return () => stop?.();
  });

  const tool = $derived(localizeTool(toolById(ui.activeTool)));
  const onboardingDone = $derived(config.current?.onboarding_done === true);

  /**
   * El menú del título: cambiar de herramienta sin una fila de pestañas.
   *
   * Solo entran las que tienen cuerpo —Pizarra, Color y Apps son atajos, no
   * vistas— y el último ítem lleva a los ajustes de la herramienta abierta,
   * que es lo que antes hacía el engranaje de la barra del workspace.
   */
  type ToolPick = ToolId | "settings";
  const SETTINGS_SECTION: Partial<Record<ToolId, SettingsSectionId>> = {
    meetings: "meetings",
    dictation: "dictation",
    captures: "captures",
    agents: "agents",
  };
  const toolItems = $derived.by((): MenuItem<ToolPick>[] => [
    ...BODIED_TOOLS.map((item) => ({
      id: item.id,
      label: localizeTool(item).label,
      icon: TOOL_ICONS[item.id],
      checked: item.id === ui.activeTool,
    })),
    {
      id: "settings",
      label: t("workspace.settingsForTool", { label: tool.label }),
      icon: TOOL_ICONS.settings,
    },
  ]);

  function onToolPick(id: ToolPick) {
    if (id === "settings") {
      ui.openSettings(SETTINGS_SECTION[ui.activeTool] ?? "general");
      return;
    }
    ui.openTool(id);
  }

  /**
   * macOS: al arrancar, si falta algún permiso TCC, se ofrece la pantalla que
   * los pide. Se chequea una sola vez por sesión: cerrarla no la reabre hasta
   * el próximo arranque (o desde Ajustes → Acerca de).
   */
  const isMac = navigator.userAgent.includes("Mac");
  let permissionsChecked = false;

  $effect(() => {
    if (!isMac || !onboardingDone || permissionsChecked) return;
    permissionsChecked = true;
    void macPermissionsStatus()
      .then((status) => {
        if (hasMissingPermissions(status)) ui.openPermissions();
      })
      .catch(() => {
        /* fuera de Tauri */
      });
  });

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
  }

  onMount(() => {
    // Ajustes es un chunk grande (11 secciones) y sin prefetch el primer clic
    // abría el modal vacío mientras cargaba; en dev Vite además lo transforma
    // al vuelo. Se pide en idle, cuando no compite con el arranque.
    const warm = () => void import("$features/settings/SettingsPanel.svelte");
    if (typeof window.requestIdleCallback === "function") {
      const idle = window.requestIdleCallback(warm, { timeout: 4000 });
      return () => window.cancelIdleCallback(idle);
    }
    const timer = setTimeout(warm, 1500);
    return () => clearTimeout(timer);
  });
</script>

{#snippet titleMenu()}
  <Menu
    items={toolItems}
    label={t("workspace.tools")}
    onpick={onToolPick}
    triggerClass="flex min-w-0 items-center gap-1.5 rounded-xs px-1 py-0.5 text-xs
                   font-medium text-text transition-colors
                   duration-(--duration-quick) ease-calm hover:bg-surface-2
                   focus-visible:[outline:2px_solid_var(--accent)]"
    trigger={toolTrigger}
  />
{/snippet}

{#snippet toolTrigger()}
  <Icon icon={TOOL_ICONS[ui.activeTool]} size={13} />
  <span class="min-w-0 truncate">{tool.label}</span>
  <Icon icon={ChevronDown} size={11} class="shrink-0 text-faint" />
{/snippet}

<svelte:window onkeydown={onKeydown} />

<WindowFrame
  title={tool.label}
  minimizeLabel={t("chrome.minimize")}
  maximizeLabel={t("chrome.maximize")}
  closeLabel={t("chrome.close")}
  onMinimize={() => void minimizeWindow()}
  onMaximize={() => void toggleMaximizeWindow()}
  onClose={() => void closeWindow()}
  {titleMenu}
>
  {#snippet start()}
    <AticMark size={18} strokeWidth={1.5} alive track="window" state={markState} />
  {/snippet}

  {#snippet actions()}
    <IconButton label={t("chrome.search")} size="sm" onclick={() => ui.openSearch()}>
      <Icon icon={Search} size={14} />
    </IconButton>

    <IconButton
      label={t("chrome.settings")}
      size="sm"
      onclick={() => ui.openSettings()}
    >
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
    {/if}
  {/snippet}

  <div class="shell">
    <!--
      Sin picker: la ventana muestra el cuerpo de la herramienta activa. El
      menú del título (y el buscador) cambian entre las que tienen vista.
    -->
    <ToolWorkspace
      toolId={ui.activeTool}
      snippetsTab={ui.snippetsTab}
      onOpenSettings={(section) => ui.openSettings(section ?? "general")}
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

{#if ui.permissionsOpen}
  <PermissionsModal onClose={() => ui.closePermissions()} />
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

{#if ui.settingsOpen}
  <Modal
    title={t("chrome.settings")}
    size="lg"
    fill
    scrollBody={false}
    onClose={() => ui.closeSettings()}
  >
    <div class="-mx-4 -my-3 flex h-full min-h-0 flex-1 flex-col overflow-hidden">
      {#await import("$features/settings/SettingsPanel.svelte")}
        <div class="flex h-full w-full items-center justify-center text-sm text-muted">
          {t("settings.loading")}
        </div>
      {:then { default: SettingsPanel }}
        {#key ui.settingsSection}
          <SettingsPanel initialSection={ui.settingsSection} />
        {/key}
      {/await}
    </div>
  </Modal>
{/if}

<ToastStack items={toasts.items} onDismiss={(id) => toasts.dismiss(id)} />

<style>
  .shell {
    position: relative;
    display: flex;
    height: 100%;
    min-height: 0;
  }
</style>
