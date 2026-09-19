<script lang="ts">
  import { tip } from "$surfaces/overlay/tip.svelte";
  /** Lanzador compacto y shell persistente de las consolas locales. */
  import ConsolePanel from "./ConsolePanel.svelte";
  import FolderBrowser from "./FolderBrowser.svelte";
  import AgentLogo from "./AgentLogo.svelte";
  import Icon from "$ui/Icon.svelte";
  import { ArrowRight, ChevronRight, Folder, Minus, Plus, X } from "$lib/icons";
  import { onMount, tick } from "svelte";
  import {
    AGENTS_PATH_CHANGED,
    AGENTS_REVEAL_CONSOLE,
    AGENTS_ISLAND_LAUNCH,
    agentsEnsureWindow,
    cliOnPath,
    consoleBeginTransfer,
    consoleEndTransfer,
    consoleTransferDeliver,
    onAgentsTransferAck,
    takeAgentsIslandLaunch,
    type AgentsIslandLaunchDetail,
  } from "$ipc/agents";
  import { currentWindowLabel, hideWindow } from "$ipc/windows";
  import { OVERLAY_LABEL } from "$surfaces/overlay/contract";
  import {
    transferInbox,
    AGENTS_WINDOW_LABEL,
    type TransferPayload,
  } from "./consoleTransfer.svelte";
  import { toasts } from "$domain/toasts.svelte";
  import { AGENTS, installCommand, shownAgents } from "./agentCatalog";
  import { config } from "$domain/config.svelte";
  import { sessionEffect } from "$domain/session";
  import { t } from "$domain/i18n.svelte";

  type LauncherView = "setup" | "console";

  let {
    onHeaderPointerDown,
    onClose,
    onViewChange,
    onBrowserChange,
    onToggleMaximize,
    onToggleMinimize,
    onLiveChange,
    onNeedsAttention,
    maximized = false,
    minimized = false,
    shown = false,
    island = false,
  }: {
    onHeaderPointerDown?: (e: PointerEvent) => void;
    /** Cerrar el float. El overlay no lo pasa: ahí solo se minimiza o agranda. */
    onClose?: () => void;
    onViewChange?: (view: LauncherView) => void;
    onBrowserChange?: (open: boolean) => void;
    onToggleMaximize?: () => void;
    onToggleMinimize?: () => void;
    /** Hay PTYs montadas: el float debe esconder, no destruir. */
    onLiveChange?: (live: boolean) => void;
    /** Una consola enmudeció fuera de vista: el dueño avisa (toast). */
    onNeedsAttention?: (label: string) => void;
    maximized?: boolean;
    minimized?: boolean;
    /** El float está a la vista: si hay consolas vivas, mostrarlas. */
    shown?: boolean;
    /** Cara de la isla: sin chrome de ventana, el blob es el marco. */
    island?: boolean;
  } = $props();

  /** El tope duro lo pone Rust (MAX_CONSOLES). */
  const MAX_INSTANCES = 6;

  /** La carpeta elegida sobrevive a cerrar el float y a reiniciar la app. */
  const CWD_STORAGE_KEY = "atic.agents.startFolder";

  /**
   * Los agentes elegidos en Ajustes. Vacío = todos.
   *
   * Se filtra la grilla y no el catálogo: `AGENTS` sigue siendo la lista de lo
   * que Atic sabe lanzar —la usa la comprobación de PATH y la vista de
   * instalación—, y esto es solo qué se ofrece.
   */
  // El lanzador vive en la ventana principal y en el float del overlay, y
  // ninguna de las dos tenía por qué montar `config` antes de esto.
  $effect(() => sessionEffect(["config"]));

  const visible = $derived(shownAgents(config.current?.agents_shown ?? []));

  let selected = $state<string>(AGENTS[0].cli);
  let count = $state(1);
  let cwd = $state("");
  let browsing = $state(false);
  let view = $state<LauncherView>("setup");
  let hasConsole = $state(false);
  let onPath = $state<Record<string, boolean>>({});
  let pathReady = $state(false);

  const chosen = $derived(
    visible.find((agent) => agent.cli === selected) ?? visible[0] ?? AGENTS[0],
  );
  // Sobre `chosen` y no sobre `selected`: si Ajustes ocultó al seleccionado,
  // el lanzador ya muestra otro —`visible[0]`— y el estado del PATH tiene que
  // ser el de ese, o ofrece instalar un agente que no es el que abre.
  const missingCli = $derived(pathReady && onPath[chosen.cli] === false);

  /**
   * Patrón radio de la grilla: un solo tope de Tab (el elegido) y ←→/↑↓
   * eligen con envolvente. Con clic el navegador ya deja el foco en la celda,
   * así que el roving queda consistente por cualquier camino.
   */
  let pickerEl = $state<HTMLDivElement | null>(null);

  function onPickerKeydown(event: KeyboardEvent) {
    const step =
      event.key === "ArrowRight" || event.key === "ArrowDown"
        ? 1
        : event.key === "ArrowLeft" || event.key === "ArrowUp"
          ? -1
          : 0;
    if (!step || !pickerEl) return;
    const options = Array.from(
      pickerEl.querySelectorAll<HTMLButtonElement>(".agent-option"),
    );
    // El target de un keydown es el elemento con foco, pero contiene() hace
    // robusto el caso de un hijo (el logo) que algún día reciba el foco.
    const target = event.target instanceof Element ? event.target : null;
    const current = options.findIndex(
      (option) => option === target || (target && option.contains(target)),
    );
    if (current < 0 || options.length < 2) return;
    const next = options[(current + step + options.length) % options.length];
    event.preventDefault();
    next.focus();
    selected = next.dataset.cli ?? selected;
  }
  // Con el CLI ausente la consola se siembra con su instalador oficial, la
  // misma mecánica que el botón "Instalar" del menú "+" de ConsolePanel.
  const seeds = $derived(
    missingCli
      ? [
          {
            kind: "local" as const,
            label: t("page.agents.installNamed", { name: chosen.name }),
            command: installCommand(chosen),
          },
        ]
      : Array.from({ length: Math.max(1, Math.min(count, MAX_INSTANCES)) }, () => ({
          kind: "local" as const,
          label: chosen.name,
          command: chosen.cli,
        })),
  );
  const launchLabel = $derived(
    missingCli
      ? t("page.agents.installNamed", { name: chosen.name })
      : t("page.agents.openNamed", { name: chosen.name }),
  );

  function setHasConsole(next: boolean) {
    hasConsole = next;
    onLiveChange?.(next);
  }

  function showView(next: LauncherView) {
    view = next;
    onViewChange?.(next);
    // Volver al setup re-mira el PATH: puede haber terminado un instalador
    // mientras estábamos en la consola.
    if (next === "setup") refreshPath();
  }

  function revealLiveConsole() {
    if (hasConsole) showView("console");
  }

  let wasShown = false;
  $effect(() => {
    const justOpened = shown && !wasShown;
    wasShown = shown;
    if (justOpened) revealLiveConsole();
  });

  /** Instancia viva de ConsolePanel, para instalar sin remontar la consola. */
  let panel = $state<ConsolePanel | null>(null);

  /* ─── Mudanza entre ventanas ────────────────────────────────────────────
     Dueño único: la emisora protege (`begin`), la receptora adopta y confirma
     (`end` + ack), y recién ahí la emisora suelta sin matar. Si el ack no
     llega, se levanta la protección y todo se queda donde estaba. */
  const myLabel = currentWindowLabel();
  /** Fuera del float, el hogar es la ventana dedicada; desde ella, la pill. */
  const otherLabel = myLabel === OVERLAY_LABEL ? AGENTS_WINDOW_LABEL : OVERLAY_LABEL;
  const DETACH_ACK_MS = 4000;
  let detachBusy = $state(false);
  /** Baja del oyente de acks de mudanza (se arma en el onMount). */
  let ackUnlisten: (() => void) | null = null;
  // Caché de trabajo: acks en vuelo. No es estado de vista.
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- ver panel: se muta fuera del render
  const pendingDetach = new Map<string, { done: (adopted: string[]) => void }>();

  /** Adoptar lo que llegó al buzón de esta ventana. */
  async function receiveTransfer(payload: TransferPayload) {
    if (!hasConsole) setHasConsole(true);
    await tick();
    let guard = 0;
    while (!panel && guard++ < 60) {
      await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    }
    let adopted: string[] = [];
    if (panel) {
      try {
        adopted = await panel.adoptSessions(
          payload.tabs,
          payload.tree,
          payload.activeSession,
        );
      } catch {
        adopted = [];
      }
    }
    try {
      await consoleEndTransfer(payload.sessions);
    } catch {
      /* la marca caduca sola */
    }
    try {
      await consoleTransferDeliver(
        payload.from,
        "agents-transfer-ack",
        JSON.stringify({ transferId: payload.transferId, adopted }),
      );
    } catch {
      /* la emisora cubre con su timeout */
    }
    if (adopted.length > 0) showView("console");
    // De vuelta en la pill con el float cerrado: sin esto, la mudanza no se
    // ve en ningún lado. En la ventana dedicada `shown` siempre es true.
    if (adopted.length > 0 && !shown && myLabel === OVERLAY_LABEL) {
      toasts.push(t("page.agents.console.receivedBack"));
    }
  }

  $effect(() => {
    const inbox = transferInbox.current;
    if (!inbox || inbox.to !== myLabel) return;
    transferInbox.take();
    void receiveTransfer(inbox);
  });

  /** Mudar estas consolas a la otra ventana, vivas y con scrollback. */
  async function detachTo() {
    if (detachBusy || !panel) return;
    const body = await panel.buildTransferPayload().catch(() => null);
    if (!body || body.sessions.length === 0) {
      toasts.push(t("page.agents.console.nothingToMove"));
      return;
    }
    detachBusy = true;
    // A la ventana dedicada: primero existe y al frente; sin ella el ack no
    // llega nunca y el timeout devolvería todo.
    if (otherLabel === AGENTS_WINDOW_LABEL) {
      try {
        await agentsEnsureWindow();
      } catch {
        detachBusy = false;
        toasts.push(t("page.agents.console.detachFailed"));
        return;
      }
    }
    try {
      await consoleBeginTransfer(body.sessions);
    } catch {
      detachBusy = false;
      toasts.push(t("page.agents.console.detachFailed"));
      return;
    }
    const transferId = crypto.randomUUID();
    const payload: TransferPayload = {
      ...body,
      transferId,
      from: myLabel,
      to: otherLabel,
    };
    try {
      await consoleTransferDeliver(
        otherLabel,
        "agents-transfer",
        JSON.stringify(payload),
      );
    } catch {
      await consoleEndTransfer(body.sessions).catch(() => {});
      detachBusy = false;
      toasts.push(t("page.agents.console.detachFailed"));
      return;
    }
    const adopted = await new Promise<string[]>((resolve) => {
      const timer = window.setTimeout(() => {
        pendingDetach.delete(transferId);
        resolve([]);
      }, DETACH_ACK_MS);
      pendingDetach.set(transferId, {
        done: (list: string[]) => {
          window.clearTimeout(timer);
          pendingDetach.delete(transferId);
          resolve(list);
        },
      });
    });
    if (adopted.length === 0) {
      await consoleEndTransfer(body.sessions).catch(() => {});
      detachBusy = false;
      toasts.push(t("page.agents.console.detachFailed"));
      return;
    }
    const remaining = panel?.clearTransferred(adopted) ?? 0;
    if (remaining === 0) {
      // Sin fichas no hay consola que mostrar: vuelve al setup. Las sesiones
      // ya son de la otra ventana, así que desmontar no mata nada. Y si esta
      // era la ventana dedicada, se esconde sola: cumplió.
      setHasConsole(false);
      showView("setup");
      if (myLabel === AGENTS_WINDOW_LABEL) {
        void hideWindow().catch(() => {});
      }
    }
    detachBusy = false;
    toasts.push(t("page.agents.console.detachedOk"));
  }

  function launch() {
    if (missingCli) {
      // El CLI no está: el botón instala en vez de lanzar. Con la consola ya
      // montada `initialTabs` no aplica; se pide la pestaña a la instancia.
      if (hasConsole) panel?.installAgent(chosen);
      else setHasConsole(true);
      showView("console");
      return;
    }
    if (hasConsole) {
      // `initialTabs` solo siembra al montar. Acá hay que sumar pestañas
      // sin matar las que ya corren.
      panel?.openAgent(chosen, count);
      showView("console");
      return;
    }
    setHasConsole(true);
    showView("console");
  }

  function applyIslandLaunch(detail: AgentsIslandLaunchDetail) {
    selected = detail.cli;
    if (detail.count != null) {
      count = Math.max(1, Math.min(MAX_INSTANCES, detail.count));
    }
    if (detail.cwd != null) cwd = detail.cwd;
    launch();
  }

  function backToSetup() {
    // `showView` ya re-mira el PATH por si el instalador corrió mientras tanto.
    showView("setup");
  }

  function resetSessions() {
    // Al desmontar ConsolePanel su onDestroy cierra todas las PTYs.
    setHasConsole(false);
    showView("setup");
  }

  function setBrowsing(open: boolean) {
    browsing = open;
    // Solo en el lanzador: en la consola el float ya es grande y su tamaño lo
    // maneja la otra rama, así que reencuadrar acá lo encogería al cerrar.
    if (view === "setup") onBrowserChange?.(open);
  }

  function saveCwd() {
    try {
      localStorage.setItem(CWD_STORAGE_KEY, cwd);
    } catch {
      /* la carpeta sigue en memoria aunque el storage esté bloqueado */
    }
  }

  /** Pendiente de `requestFolder`, resuelta al elegir carpeta o al cancelar. */
  let folderResolve: ((path: string | null) => void) | null = null;

  function settleFolder(path: string | null) {
    const resolve = folderResolve;
    folderResolve = null;
    setBrowsing(false);
    resolve?.(path);
  }

  /**
   * Abre el explorador y avisa qué se eligió. La consola lo usa para retomar
   * el hilo donde estaba (volver al menú "+" con la carpeta ya cambiada).
   */
  function requestFolder(): Promise<string | null> {
    // Un explorador a la vez: si quedaba uno pendiente, se cancela.
    folderResolve?.(null);
    setBrowsing(true);
    return new Promise((resolve) => {
      folderResolve = resolve;
    });
  }

  /** Reverifica qué CLIs están en el PATH (al montar y al volver del instalador). */
  function refreshPath() {
    void Promise.all(
      AGENTS.map(async (agent) => {
        try {
          return [agent.cli, await cliOnPath(agent.cli)] as const;
        } catch {
          return [agent.cli, true] as const;
        }
      }),
    ).then((rows) => {
      onPath = Object.fromEntries(rows);
      pathReady = true;
    });
  }

  onMount(() => {
    try {
      cwd = localStorage.getItem(CWD_STORAGE_KEY) ?? "";
    } catch {
      /* sin storage: arranca en la carpeta de inicio del usuario */
    }
    refreshPath();
    const pending = takeAgentsIslandLaunch();
    if (pending) applyIslandLaunch(pending);
    const onReveal = () => revealLiveConsole();
    // Un instalador corrió en la consola y terminó: el setup tiene que
    // enterarse aunque esté a la vista en ese momento.
    const onPathChanged = () => refreshPath();
    const onIslandLaunch = (event: Event) => {
      const detail =
        takeAgentsIslandLaunch() ??
        (event as CustomEvent<AgentsIslandLaunchDetail>).detail;
      if (detail) applyIslandLaunch(detail);
    };
    window.addEventListener(AGENTS_REVEAL_CONSOLE, onReveal);
    window.addEventListener(AGENTS_PATH_CHANGED, onPathChanged);
    window.addEventListener(AGENTS_ISLAND_LAUNCH, onIslandLaunch);
    void onAgentsTransferAck((raw) => {
      try {
        const ack = JSON.parse(raw) as { transferId: string; adopted: string[] };
        pendingDetach.get(ack.transferId)?.done(ack.adopted ?? []);
      } catch {
        /* ruido */
      }
    }).then((unlisten) => {
      ackUnlisten = unlisten;
    });
    return () => {
      window.removeEventListener(AGENTS_REVEAL_CONSOLE, onReveal);
      window.removeEventListener(AGENTS_PATH_CHANGED, onPathChanged);
      window.removeEventListener(AGENTS_ISLAND_LAUNCH, onIslandLaunch);
      ackUnlisten?.();
    };
  });
</script>

<div class="agent-views" class:is-island={island}>
  <section
    class="launcher-view"
    class:is-hidden={view !== "setup"}
    aria-hidden={view !== "setup" ? "true" : undefined}
    inert={view !== "setup"}
    aria-label={t("page.agents.launcher.openAria")}
  >
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <header
      class="drag-rail"
      aria-label={t("page.agents.launcher.moveAria")}
      onpointerdown={(event) => {
        if (onHeaderPointerDown && !(event.target as HTMLElement).closest("button")) {
          onHeaderPointerDown(event);
        }
      }}
    >
      {#if hasConsole}
        <button
          type="button"
          class="live-status"
          use:tip={t("page.agents.backToConsoles")}
          onclick={revealLiveConsole}
        >
          <span class="live-dot" aria-hidden="true"></span>
          {t("page.agents.liveConsoles")}
        </button>
      {/if}
      <div class="chrome">
        {#if onClose}
          <button
            type="button"
            class="close"
            aria-label={t("chrome.close")}
            use:tip={t("page.agents.hideHint")}
            onclick={onClose}
          >
            <Icon icon={X} size={13} />
          </button>
        {/if}
      </div>
    </header>

    <div class="setup">
      <div
        class="agent-picker"
        role="radiogroup"
        aria-label={t("page.agents.launcher.agentAria")}
        tabindex="-1"
        bind:this={pickerEl}
        onkeydown={onPickerKeydown}
      >
        {#each visible as agent (agent.cli)}
          <button
            type="button"
            class="agent-option"
            class:is-on={selected === agent.cli}
            class:is-missing={pathReady && onPath[agent.cli] === false}
            role="radio"
            aria-checked={selected === agent.cli}
            tabindex={selected === agent.cli ? 0 : -1}
            data-cli={agent.cli}
            aria-label={pathReady && onPath[agent.cli] === false
              ? t("page.agents.launcher.notOnPath", { name: agent.name })
              : agent.name}
            use:tip={pathReady && onPath[agent.cli] === false
              ? t("page.agents.launcher.notOnPath", { name: agent.name })
              : agent.name}
            onclick={() => (selected = agent.cli)}
          >
            <span class="agent-logo"><AgentLogo agent={agent.cli} size={22} /></span>
          </button>
        {/each}
      </div>

      <div class="launch-row">
        <button
          type="button"
          class="folder"
          use:tip={cwd || t("page.agents.console.startFolderUser")}
          onclick={() => void requestFolder()}
        >
          <Icon icon={Folder} size={15} />
          <span>{cwd.trim() || t("page.agents.console.startFolder")}</span>
          <span class="chevron" aria-hidden="true"
            ><Icon icon={ChevronRight} size={14} /></span
          >
        </button>

        <div
          class="stepper"
          role="group"
          aria-label={t("page.agents.launcher.consolesCount")}
        >
          <button
            type="button"
            aria-label={t("page.agents.launcher.fewerConsoles")}
            disabled={count <= 1}
            onclick={() => (count = Math.max(1, count - 1))}
          >
            <Icon icon={Minus} size={13} />
          </button>
          <span class="count"
            >{count}
            {count === 1
              ? t("page.agents.consoleSingular")
              : t("page.agents.consolePlural")}</span
          >
          <button
            type="button"
            aria-label={t("page.agents.launcher.moreConsoles")}
            disabled={count >= MAX_INSTANCES}
            onclick={() => (count = Math.min(MAX_INSTANCES, count + 1))}
          >
            <Icon icon={Plus} size={13} />
          </button>
        </div>

        {#if hasConsole}
          <button
            type="button"
            class="reset"
            aria-label={t("page.agents.launcher.killConsoles")}
            use:tip={t("page.agents.launcher.killConsolesTip")}
            onclick={resetSessions}
          >
            <Icon icon={X} size={13} />
          </button>
        {/if}
      </div>

      <button
        type="button"
        class="launch"
        use:tip={missingCli
          ? t("page.agents.launcher.notInstalled", { name: chosen.name })
          : undefined}
        onclick={launch}
      >
        <span>{launchLabel}</span>
        <span class="arrow" aria-hidden="true"
          ><Icon icon={ArrowRight} size={14} /></span
        >
      </button>
    </div>
  </section>

  {#if hasConsole}
    <div
      class="console-view"
      class:is-hidden={view !== "console"}
      aria-hidden={view !== "console" ? "true" : undefined}
      inert={view !== "console"}
    >
      <ConsolePanel
        bind:this={panel}
        initialTabs={seeds}
        localCwd={cwd}
        onBack={backToSetup}
        onEmpty={resetSessions}
        onPickFolder={requestFolder}
        onToggleMaximize={island ? undefined : onToggleMaximize}
        onToggleMinimize={island ? undefined : onToggleMinimize}
        windowChrome={!island}
        {maximized}
        {minimized}
        visible={shown && !minimized}
        {onNeedsAttention}
        onDetachRequest={island ? undefined : detachTo}
        {detachBusy}
        onBarPointerDown={island ? undefined : onHeaderPointerDown}
      />
    </div>
  {/if}
</div>

{#if browsing}
  <FolderBrowser
    initialPath={cwd}
    onPick={(path) => {
      cwd = path;
      saveCwd();
      settleFolder(path);
    }}
    onClose={() => settleFolder(null)}
  />
{/if}

<style>
  .agent-views {
    --agent-accent: var(--accent, var(--rb-accent, var(--rb-text)));

    position: relative;
    display: flex;
    min-height: 0;
    flex: 1;
    overflow: hidden;
    border-radius: inherit;
    background: transparent;
    container-name: agents-launcher;
    container-type: inline-size;
  }

  .launcher-view,
  .console-view {
    position: absolute;
    inset: 0;
    display: flex;
    min-height: 0;
    flex-direction: column;
    opacity: 1;
  }

  .launcher-view {
    z-index: 1;
    overflow: hidden;
    border-radius: inherit;
    background: var(--skin);
    transition: opacity var(--duration-fast, 125ms) var(--ease-smooth-out, ease);
  }

  .console-view {
    z-index: 0;
  }

  .is-hidden {
    pointer-events: none;
  }

  /* Crossfade real: el lanzador se disuelve ENCIMA de la consola (que nunca
     deja de pintar) en vez de un corte seco. `visibility` espera al fade para
     no cortarlo; al volver, el default (sin delay) lo muestra al instante.
     Mantiene z-index 1: con visibility:hidden ya no pinta ni recibe clics. */
  .launcher-view.is-hidden {
    visibility: hidden;
    opacity: 0;
    transition:
      opacity var(--duration-fast, 125ms) var(--ease-smooth-out, ease),
      visibility 0s linear var(--duration-fast, 125ms);
  }

  /* No opacity 0, visibility:hidden ni transform: en WebView2 el canvas
     de xterm se congela y la consola queda en beige vacío. Se queda detrás
     del lanzador (fondo --skin) para que el renderer siga vivo. */

  .drag-rail {
    display: flex;
    min-height: 2rem;
    flex: 0 0 2rem;
    align-items: center;
    justify-content: flex-end;
    padding: 0.2rem 0.42rem 0.15rem 0.7rem;
    border-bottom: 0;
    background: transparent;
    cursor: move;
  }

  .live-status {
    display: inline-flex;
    min-width: 0;
    align-items: center;
    gap: 0.35rem;
    margin-right: auto;
    border: 0;
    padding: 0.15rem 0.35rem 0.15rem 0.1rem;
    background: transparent;
    color: var(--rb-muted);
    font: inherit;
    font-size: 0.625rem;
    font-weight: 600;
    cursor: pointer;
    border-radius: 0.4rem;
  }

  .live-status:hover {
    color: var(--rb-text);
    background: color-mix(in sRGB, var(--rb-text) 7%, transparent);
  }

  .live-dot {
    width: 0.35rem;
    height: 0.35rem;
    flex: 0 0 auto;
    border-radius: 999px;
    background: var(--rb-ok);
  }

  .close,
  .reset {
    display: grid;
    flex: 0 0 auto;
    place-items: center;
    border: 0;
    background: transparent;
    color: var(--rb-muted);
    cursor: pointer;
    transition:
      background-color var(--duration-fast, 125ms) var(--ease-smooth-out, ease),
      color var(--duration-fast, 125ms) var(--ease-smooth-out, ease),
      transform var(--duration-quick, 75ms) ease;
  }

  .close:active,
  .reset:active {
    transform: scale(0.96);
  }

  .chrome {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    gap: 0.1rem;
    margin-left: auto;
  }

  .close {
    width: 2.25rem;
    height: 2.25rem;
    border-radius: 0.45rem;
  }

  .close:hover,
  .reset:hover {
    background: color-mix(in sRGB, var(--rb-text) 7%, transparent);
    color: var(--rb-text);
  }

  .setup {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;

    /* Ritmo: la decisión (picker → modificadores) apretada. */
    gap: 0.5rem;
    padding: 0.75rem;
    overflow: auto;
  }

  /* Sin marco ni separadores: en las esquinas redondeadas del contenedor los
     bordes se perdían y ensuciaban. El único color es el acento del elegido;
     el resto se lee por hover. Las celdas se reparten TODO el ancho. */
  .agent-picker {
    display: grid;
    width: 100%;
    grid-template-columns: repeat(auto-fit, minmax(2.5rem, 1fr));
    gap: 0.35rem;
    align-self: stretch;
  }

  .agent-option {
    display: grid;
    min-height: 2.75rem;
    place-items: center;
    border: 0;
    border-radius: 0.6rem;
    padding: 0;
    background: transparent;
    color: var(--rb-text);
    font: inherit;
    cursor: pointer;
    transition:
      background-color var(--duration-fast, 125ms) var(--ease-smooth-out, ease),
      box-shadow var(--duration-fast, 125ms) var(--ease-smooth-out, ease),
      opacity var(--duration-fast, 125ms) var(--ease-smooth-out, ease);
  }

  .agent-option:hover {
    background: color-mix(in sRGB, var(--rb-text) 6%, transparent);
  }

  .agent-option:active {
    background: color-mix(in sRGB, var(--rb-text) 10%, transparent);
  }

  .agent-option.is-on {
    background: color-mix(in sRGB, var(--agent-accent) 12%, transparent);

    /* 56% en vez de 72%: el anillo no le pelea al CTA, que es la acción. */
    box-shadow: inset 0 0 0 1px color-mix(in sRGB, var(--agent-accent) 56%, transparent);
  }

  .agent-option.is-missing {
    opacity: 0.42;
  }

  .agent-option.is-missing.is-on {
    opacity: 0.72;
  }

  .agent-logo {
    display: grid;
    width: 1.4rem;
    height: 1.4rem;
    place-items: center;
    color: var(--rb-text);
    transition: transform var(--duration-fast, 125ms) var(--ease-smooth-out, ease);
  }

  .agent-option:hover .agent-logo,
  .agent-option.is-on .agent-logo {
    transform: scale(1.08);
  }

  .agent-option:active .agent-logo {
    transform: scale(0.96);
  }

  /* Una fila de modificadores: dónde (carpeta) y cuántas (stepper) cambian el
     lanzamiento; la carpeta ocupa el resto del ancho porque su contenido es
     el que crece. El reset entra y sale sin dejar huecos (flex, no grid). */
  .launch-row {
    display: flex;
    gap: 0.4rem;
    align-items: stretch;
  }

  .folder,
  .stepper {
    min-height: 2.45rem;
    border: 0; /* mismo lenguaje que el selector de agentes: sin bordes, fondos suaves */
    border-radius: 0.62rem;
    font: inherit;
  }

  .folder {
    display: flex;
    flex: 1 1 auto;
    min-width: 0;
    align-items: center;
    gap: 0.48rem;
    padding: 0.4rem 0.62rem;
    background: color-mix(in sRGB, var(--rb-surface-2) 62%, transparent);
    color: var(--rb-muted);
    text-align: left;
    cursor: pointer;
    transition:
      background-color var(--duration-fast, 125ms) var(--ease-smooth-out, ease),
      color var(--duration-fast, 125ms) var(--ease-smooth-out, ease);
  }

  .stepper {
    flex: none;
    display: grid;
    grid-template-columns: 1.9rem max-content 1.9rem;
    align-items: stretch;
    gap: 0.2rem;
    min-width: max-content;
    padding: 0.25rem;
    background: color-mix(in sRGB, var(--rb-surface-2) 62%, transparent);
  }

  .folder:hover {
    background: color-mix(in sRGB, var(--rb-text) 8%, transparent);
    color: var(--rb-text);
  }

  .folder > span:not(.chevron) {
    min-width: 0;
    flex: 1;
    overflow: hidden;
    font-size: 0.7rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .chevron {
    display: grid;
    place-items: center;

    /* muted, no faint: la flecha es la señal de «hay más» y como gráfica
       significativa pide 3:1 (faint da 2.9:1 en claro). */
    color: var(--rb-muted);
  }

  .stepper button {
    display: grid;
    width: 1.9rem;
    place-items: center;
    border: 0;
    border-radius: 0.42rem;
    background: transparent;
    color: var(--rb-text);
    font-size: 0.95rem;
    line-height: 1;
    cursor: pointer;
    transition:
      background-color var(--duration-fast, 125ms) var(--ease-smooth-out, ease),
      transform var(--duration-fast, 125ms) var(--ease-smooth-out, ease),
      opacity var(--duration-fast, 125ms) var(--ease-smooth-out, ease);
  }

  .stepper button:hover:not(:disabled) {
    background: color-mix(in sRGB, var(--rb-text) 8%, transparent);
  }

  .stepper button:active:not(:disabled) {
    background: color-mix(in sRGB, var(--rb-text) 12%, transparent);

    /* 0.96: bajo 0.95 el encogimiento se siente exagerado. */
    transform: scale(0.96);
  }

  .stepper button:disabled {
    color: var(--rb-faint);
    cursor: default;
    opacity: 0.4;
  }

  .count {
    display: grid;
    place-items: center;
    padding-inline: 0.5rem;
    color: var(--rb-text);
    font-size: 0.68rem;
    font-weight: 650;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .reset {
    width: 2.45rem;
    min-height: 2.45rem;
    border: 0;
    border-radius: 0.62rem;
    background: color-mix(in sRGB, var(--rb-surface-2) 62%, transparent);
    transition:
      background-color var(--duration-fast, 125ms) var(--ease-smooth-out, ease),
      color var(--duration-fast, 125ms) var(--ease-smooth-out, ease),
      transform var(--duration-quick, 75ms) ease;
  }

  .reset:hover {
    background: color-mix(in sRGB, var(--rb-record) 14%, transparent);
    color: var(--rb-record);
  }

  /* El commit va solo: fila propia a todo lo ancho. La acción más frecuente
     (abrir con lo ya elegido) gana el ancho completo y una letra más que los
     modificadores. */
  .launch {
    display: flex;
    width: 100%;
    min-height: 2.55rem;
    align-items: center;
    justify-content: center;
    gap: 0.42rem;
    margin-top: 0.2rem; /* cierra el ritmo: el commit va más separado */
    padding: 0.42rem 0.75rem;
    background: var(--agent-accent);
    color: var(--rb-on-accent);
    font-size: 0.72rem;
    font-weight: 700;
    cursor: pointer;
    transition:
      background-color var(--duration-fast, 125ms) var(--ease-smooth-out, ease),
      transform var(--duration-fast, 125ms) var(--ease-smooth-out, ease);
  }

  .launch:disabled {
    cursor: not-allowed;
    opacity: 0.55;
    transform: none;
  }

  .launch:hover:not(:disabled) {
    background: color-mix(in sRGB, var(--agent-accent) 87%, var(--rb-text));
    transform: translateY(-1px);
  }

  .launch:active:not(:disabled) {
    transform: translateY(0) scale(0.96);
  }

  .launch .arrow {
    margin-left: 0.12rem;
  }

  button:focus-visible {
    outline: none;
    box-shadow: var(--rb-focus);
  }

  /* La estructura es la misma a todo ancho (elegir / condicionar / commit):
     el breakpoint solo aprieta densidad, no reordena. */
  @container agents-launcher (width <= 35rem) {
    .setup {
      gap: 0.45rem;
      padding: 0.6rem;
    }

    .launch {
      margin-top: 0.15rem;
    }
  }

  @container agents-launcher (width <= 28rem) {
    .drag-rail {
      min-height: 1.8rem;
      flex-basis: 1.8rem;
    }

    .folder > span:not(.chevron),
    .launch {
      font-size: 0.66rem;
    }

    .stepper button {
      width: 1.65rem;
    }
  }

  .agent-views.is-island {
    --agent-accent: var(--accent, var(--rb-accent, var(--rb-text)));

    background: transparent;
    border-radius: 0;
  }

  .agent-views.is-island .launcher-view {
    background: transparent;
    border-radius: 0;
  }

  .agent-views.is-island .drag-rail {
    min-height: 0;
    flex-basis: 0;
    padding: 0;
  }

  .agent-views.is-island .drag-rail:not(:has(.live-status)) {
    display: none;
  }

  .agent-views.is-island .chrome {
    display: none;
  }

  .agent-views.is-island .setup {
    flex: 1 1 auto;
    gap: 0.4rem;
    padding: 0.15rem 0.05rem 0.2rem;
    justify-content: flex-start;
  }

  .agent-views.is-island .launch {
    min-height: 2.05rem;
  }

  @media (prefers-reduced-motion: reduce) {
    .launcher-view,
    .console-view,
    .agent-option,
    .agent-logo,
    .folder,
    .stepper button,
    .close,
    .reset,
    .launch,
    .live-status {
      transition: none;
      transform: none;
    }

    .stepper button:active:not(:disabled),
    .close:active,
    .reset:active,
    .launch:active:not(:disabled) {
      transform: none;
    }
  }
</style>
