<script lang="ts">
  import { tip } from "$surfaces/overlay/tip.svelte";
  /** Lanzador compacto y shell persistente de las consolas locales. */
  import ConsolePanel from "./ConsolePanel.svelte";
  import FolderBrowser from "./FolderBrowser.svelte";
  import AgentLogo from "./AgentLogo.svelte";
  import Icon from "$ui/Icon.svelte";
  import { ArrowRight, ChevronRight, Folder, Minus, Plus, X } from "$lib/icons";
  import { onMount, tick } from "svelte";
  import { emit } from "@tauri-apps/api/event";
  import {
    AGENTS_PATH_CHANGED,
    AGENTS_REVEAL_CONSOLE,
    AGENTS_ISLAND_LAUNCH,
    agentThread,
    agentsEnsureWindow,
    cliOnPath,
    consoleBeginTransfer,
    consoleEndTransfer,
    consoleTransferDeliver,
    listDirectories,
    onAgentsTransferAck,
    setAgentsWindowOpen,
    takeAgentsIslandLaunch,
    type AgentsIslandLaunchDetail,
  } from "$ipc/agents";
  import { currentWindowLabel, hideWindow } from "$ipc/windows";
  import { OVERLAY_LABEL } from "$surfaces/overlay/contract";
  import {
    transferInbox,
    overlayTransferBus,
    AGENTS_OVERLAY_DETACH,
    AGENTS_OVERLAY_DETACHED,
    AGENTS_WINDOW_LABEL,
    type TransferPayload,
    type OverlayTransferRole,
    type AgentsOverlayDetachDetail,
  } from "./consoleTransfer.svelte";
  import { toasts } from "$domain/toasts.svelte";
  import { agentsFloatLive } from "$surfaces/overlay/agents/agentsIslandHost.svelte";
  import { AGENTS, installCommand, shownAgents } from "./agentCatalog";
  import { config } from "$domain/config.svelte";
  import { sessionEffect } from "$domain/session";
  import { t } from "$domain/i18n.svelte";
  import { agents } from "$lib/agentSessions.svelte";
  import { chatTabsKey, loadChatTabs, saveChatTabs, type ChatTabRecord } from "./chatTabs";

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
    onRevealFloat,
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
    /** Isla: las consolas viven en el float y el usuario pidió ir a ellas. */
    onRevealFloat?: () => void;
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
  /**
   * Adopción en vuelo: el panel que monte no siembra su pestaña inicial
   * (quedaría duplicando las adoptadas). Se resetea al terminar de recibir.
   */
  let incomingTransfer = $state(false);
  /** Chats retomados que siembran el panel al montarlo; se vacía al usarse. */
  let restoredChats = $state<ChatTabRecord[]>([]);
  /**
   * La ventana está a la vista. Oculta (cerrada a la bandeja) no es destino de
   * pegado: el historial debe caer donde el usuario mira.
   */
  let windowShown = $state(true);
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
    restoredChats.length > 0
      ? restoredChats.map((record) => ({
          kind: "local" as const,
          label: record.label || undefined,
          command: record.command ?? undefined,
          hubSession: record.session,
          chat: true,
        }))
      : missingCli
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

  /**
   * Botón "Consolas activas". Solo el clic explícito agranda el float: si lo
   * hiciera `revealLiveConsole` (corre en cada apertura de la cara), el float
   * crecía a su último reposo —el rect de la cara— y los dos selectores
   * quedaban encimados.
   */
  function goToLiveConsoles() {
    if (hasConsole) showView("console");
    else if (island && agentsFloatLive.on) onRevealFloat?.();
  }

  let wasShown = false;
  $effect(() => {
    const justOpened = shown && !wasShown;
    wasShown = shown;
    if (!justOpened) return;
    revealLiveConsole();
    // Re-sincronizar con el host en CADA apertura: tras una recarga la vista
    // pudo quedar desalineada del otro lado (la isla grande mostrando el
    // setup = el hueco muerto). El estado real es este.
    onViewChange?.(view);
    if (view === "setup") onBrowserChange?.(browsing);
  });

  /** Instancia viva de ConsolePanel, para instalar sin remontar la consola. */
  let panel = $state<ConsolePanel | null>(null);

  /* ─── Mudanza entre ventanas ────────────────────────────────────────────
     Dueño único: la emisora protege (`begin`), la receptora adopta y confirma
     (`end` + ack), y recién ahí la emisora suelta sin matar. Si el ack no
     llega, se levanta la protección y todo se queda donde estaba. */
  const myLabel = currentWindowLabel();

  /**
   * Dónde guarda este lanzador sus chats. La isla y el float comparten
   * webview —y storage—: con la misma clave, uno retomaría los del otro.
   */
  function chatKey(): string {
    return chatTabsKey(island ? "island" : myLabel);
  }

  /**
   * Vuelve a mostrar los chats que esta vista tenía antes de recargarse. La
   * sesión siguió viva en Rust; el hilo se relee de la base, porque el store
   * de esta webview arrancó vacío.
   */
  async function restoreChats(stored: ChatTabRecord[]) {
    if (stored.length === 0) return;
    await agents.init();
    const alive = stored.filter((record) => agents.byId(record.session));
    saveChatTabs(chatKey(), alive);
    if (alive.length === 0) return;
    await Promise.all(
      alive.map(async (record) => {
        if ((agents.byId(record.session)?.turns.length ?? 0) > 0) return;
        try {
          const thread = await agentThread(record.session);
          if (!thread) return;
          agents.hydrate(record.session, {
            turns: thread.turns,
            cwd: thread.cwd,
            model: thread.model,
            providerSession: thread.providerSession,
          });
        } catch {
          /* sin hilo guardado la ficha vuelve igual, vacía */
        }
      }),
    );
    if (hasConsole) {
      panel?.adoptChats(alive);
      return;
    }
    restoredChats = alive;
    setHasConsole(true);
    await tick();
    restoredChats = [];
  }
  /** Fuera del float, el hogar es la ventana dedicada; desde ella, la pill. */
  const otherLabel = myLabel === OVERLAY_LABEL ? AGENTS_WINDOW_LABEL : OVERLAY_LABEL;
  const DETACH_ACK_MS = 4000;
  let detachBusy = $state(false);
  /** Baja del oyente de acks de mudanza (se arma en el onMount). */
  let ackUnlisten: (() => void) | null = null;
  // Caché de trabajo: acks en vuelo. No es estado de vista.
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- ver panel: se muta fuera del render
  const pendingDetach = new Map<string, { done: (adopted: string[]) => void }>();

  /** Espera el ack de la receptora (misma espera para la otra ventana y el bus). */
  function waitTransferAck(transferId: string): Promise<string[]> {
    return new Promise<string[]>((resolve) => {
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
  }

  /** Suelta las adoptadas sin matarlas; si no queda nada, vuelve al setup. */
  function settleAfterDetach(adopted: string[]): void {
    // Los chats mudados ya son de la otra vista. Se sacan a mano: si el panel
    // se desmonta abajo, no llega a guardar su lista nueva, y al recargar esta
    // vista los retomaría duplicados.
    saveChatTabs(
      chatKey(),
      loadChatTabs(chatKey()).filter((r) => !adopted.includes(`hub:${r.session}`)),
    );
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
  }

  /**
   * Adoptar un paquete: montar la consola si hace falta y tomar las fichas.
   *
   * Es la mitad común de los dos caminos. Lo que cambia es CÓMO se confirma:
   * la otra ventana necesita un evento por Rust; la otra instancia del mismo
   * overlay, nada — resuelve la promesa en el acto.
   */
  async function adoptPayload(payload: TransferPayload): Promise<string[]> {
    // El panel monta al poner `hasConsole`: si sembrara su pestaña inicial
    // ahora, quedaría como ficha extra junto a las adoptadas.
    incomingTransfer = true;
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
    if (adopted.length > 0) showView("console");
    incomingTransfer = false;
    return adopted;
  }

  /** Adoptar lo que llegó desde OTRA ventana: marca en Rust y ack por evento. */
  async function receiveTransfer(payload: TransferPayload) {
    const adopted = await adoptPayload(payload);
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
    // De vuelta en la pill con el float cerrado: sin esto, la mudanza no se
    // ve en ningún lado. En la ventana dedicada `shown` siempre es true. Los
    // traspasos coreografiados (detach/retach) no avisan: la emisora confirmó.
    if (adopted.length > 0 && !shown && !payload.quiet && myLabel === OVERLAY_LABEL) {
      toasts.push(t("page.agents.console.receivedBack"));
    }
  }

  $effect(() => {
    const inbox = transferInbox.current;
    if (!inbox || inbox.to !== myLabel) return;
    transferInbox.take();
    void receiveTransfer(inbox);
  });

  /**
   * Instancia que SOY dentro del overlay: la cara de la isla o el float. En
   * la ventana dedicada no hay roles (null) y el bus local no aplica.
   */
  const transferRole: OverlayTransferRole | null = $derived(
    myLabel === OVERLAY_LABEL ? (island ? "island" : "float") : null,
  );

  /**
   * Traspaso intra-overlay dirigido a ESTA instancia: adoptar del bus.
   *
   * La confirmación es la promesa de la emisora, no un evento: misma ventana,
   * mismo contexto JS. Adoptar reclama las sesiones en Rust (`console_attach`)
   * ANTES de que la emisora las suelte, así que nunca quedan sin dueño.
   */
  $effect(() => {
    const req = overlayTransferBus.current;
    if (!req || !transferRole || req.to !== transferRole) return;
    overlayTransferBus.take();
    void adoptPayload(req.payload).then(req.done, () => req.done([]));
  });

  /**
   * La ventana dedicada publica que hospeda consolas vivas y a la vista.
   *
   * Sin esto `agents_open()` (Rust) queda falso con las PTY acá y el pegado
   * del historial sale a la app de atrás en vez de entrar a la sesión. La isla
   * lleva su bandera aparte, así que no se pisan; oculta no cuenta porque pegar
   * en una consola que no se ve no da feedback.
   */
  $effect(() => {
    if (myLabel !== AGENTS_WINDOW_LABEL) return;
    const on = hasConsole && windowShown;
    void setAgentsWindowOpen(on).catch(() => {});
    return () => {
      void setAgentsWindowOpen(false).catch(() => {});
    };
  });

  /**
   * Mudar estas consolas a la otra ventana, vivas y con scrollback.
   *
   * El destino lo decide `otherLabel`: el overlay manda a la ventana dedicada
   * y la ventana dedicada vuelve al overlay.
   */
  async function detachTo() {
    if (detachBusy || !panel) return;
    const body = await panel.buildTransferPayload().catch(() => null);
    // Fichas, no solo PTYs: las del hub viajan como vista (sin `sessions`).
    if (!body || body.tabs.length === 0) {
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
    const adopted = await waitTransferAck(transferId);
    if (adopted.length === 0) {
      await consoleEndTransfer(body.sessions).catch(() => {});
      detachBusy = false;
      toasts.push(t("page.agents.console.detachFailed"));
      return;
    }
    settleAfterDetach(adopted);
    detachBusy = false;
    toasts.push(t("page.agents.console.detachedOk"));
  }

  /**
   * Mudar estas consolas a la OTRA instancia del overlay (isla ⇄ float).
   *
   * Mismo protocolo que `detachTo`, pero el transporte es el bus local (misma
   * ventana) en vez del evento vía Rust: sin ventana dedicada que abrir y sin
   * espera de montaje ajena — el bus retiene la oferta hasta que la receptora
   * la toma. Al terminar avisa con `AGENTS_OVERLAY_DETACHED` para que la pill
   * cierre/abra superficies.
   */
  async function detachToLocal(to: OverlayTransferRole): Promise<void> {
    if (detachBusy || !transferRole) return;
    // Sin consola montada no hay `panel` (el float se quedó en el setup tras
    // cerrar sus fichas). El retach igual tiene que volver a la isla: cae por
    // el camino vacío de acá abajo en vez de morir en silencio.
    const body = panel ? await panel.buildTransferPayload().catch(() => null) : null;
    // Fichas, no solo PTYs: las del hub viajan como vista (sin `sessions`).
    if (!body || body.tabs.length === 0) {
      // Retach de un float vacío: no hay nada que mudar, pero sí que cerrar
      // y abrir la cara. Se avisa igual (vacío) y la pill hace el resto.
      if (to === "island") {
        window.dispatchEvent(
          new CustomEvent(AGENTS_OVERLAY_DETACHED, { detail: { to, empty: true } }),
        );
        return;
      }
      toasts.push(t("page.agents.console.nothingToMove"));
      return;
    }
    detachBusy = true;
    // Sin `console_begin_transfer`: la protección de la PTY ya no es una marca
    // con TTL que el primer `console_close` se comía, sino el registro de
    // vistas de Rust. La receptora reclama antes de que esta suelte.
    const payload: TransferPayload = {
      ...body,
      transferId: crypto.randomUUID(),
      from: myLabel,
      to: myLabel,
      quiet: true,
    };
    const adopted = await overlayTransferBus.offer(to, payload);
    if (adopted.length === 0) {
      detachBusy = false;
      toasts.push(t("page.agents.console.detachFailed"));
      return;
    }
    settleAfterDetach(adopted);
    detachBusy = false;
    // Sin aviso de éxito: isla ⇄ float es la misma ventana y la mudanza se
    // ve. «Mudadas a la otra ventana» salía en cada despegue y re-acople.
    window.dispatchEvent(new CustomEvent(AGENTS_OVERLAY_DETACHED, { detail: { to } }));
  }

  /**
   * Devolver estas consolas a la isla. No mueve nada directo: la pill
   * orquesta (abre la cara al terminar y cierra el float), así que esto
   * solo pide el re-acople por el mismo evento que clipboard/textos.
   */
  function retachToIsland(): void {
    void emit("dock-tool-face", "agents").catch(() => {});
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
    // Cerrar consolas termina también los chats: su sesión vive en Rust y,
    // sin esto, quedaba viva y volvía sola en la próxima apertura.
    for (const record of loadChatTabs(chatKey())) {
      void agents.stop(record.session).catch(() => {});
    }
    saveChatTabs(chatKey(), []);
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
    // La carpeta guardada puede haber desaparecido (checkouts viejos): si no
    // existe, se limpia y vuelve a la carpeta de inicio del usuario.
    if (cwd) {
      void listDirectories(cwd).catch(() => {
        cwd = "";
        saveCwd();
      });
    }
    // Leído ya, antes de que un panel que monte primero guarde su lista vacía.
    void restoreChats(loadChatTabs(chatKey()));
    refreshPath();
    // Re-sincronizar la vista con el host: una recarga puede dejar del otro
    // lado una vista/tamaño viejos (la isla quedaba grande con el setup y
    // aparecía el hueco muerto). Al montar, el estado real es este.
    onViewChange?.(view);
    if (view === "setup") onBrowserChange?.(browsing);
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
    // La pill pide mudar mis consolas a la otra instancia del overlay
    // (detach/retach del grab). Solo responde quien ES el origen.
    const onOverlayDetach = (event: Event) => {
      const detail = (event as CustomEvent<AgentsOverlayDetachDetail>).detail;
      if (!detail || !transferRole || detail.from !== transferRole) return;
      void detachToLocal(detail.to);
    };
    // Ocultar la ventana la saca de los destinos de pegado (T5): sin esto la
    // bandera queda encendida con las PTY vivas detrás de una ventana invisible.
    const syncShown = () => (windowShown = document.visibilityState !== "hidden");
    syncShown();
    window.addEventListener(AGENTS_REVEAL_CONSOLE, onReveal);
    window.addEventListener(AGENTS_PATH_CHANGED, onPathChanged);
    window.addEventListener(AGENTS_ISLAND_LAUNCH, onIslandLaunch);
    window.addEventListener(AGENTS_OVERLAY_DETACH, onOverlayDetach);
    document.addEventListener("visibilitychange", syncShown);
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
      window.removeEventListener(AGENTS_OVERLAY_DETACH, onOverlayDetach);
      document.removeEventListener("visibilitychange", syncShown);
      ackUnlisten?.();
    };
  });
</script>

<div class="agent-views" class:is-island={island}>
  <section
    class="launcher-view"
    class:has-console={hasConsole}
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
      {#if hasConsole || (island && agentsFloatLive.on)}
        <button
          type="button"
          class="live-status"
          use:tip={t("page.agents.backToConsoles")}
          onclick={goToLiveConsoles}
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
        onChatTabsChange={(tabs) => saveChatTabs(chatKey(), tabs)}
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
        suppressInitialTab={incomingTransfer}
        onRetachRequest={myLabel === OVERLAY_LABEL && !island
          ? retachToIsland
          : undefined}
        onBarPointerDown={island ? undefined : onHeaderPointerDown}
        dense={island}
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

  /* En isla la cara dibuja su propio fondo, así que el fill del float sobra…
     salvo que haya una consola viva debajo: nunca se oculta (WebView2 congela
     el canvas de xterm con visibility/opacity), así que sin este tapador el
     selector sale transparente y los dos se pintan encima. */
  .agent-views.is-island .launcher-view.has-console {
    background: var(--skin);
  }

  .agent-views.is-island .drag-rail {
    min-height: 0;
    flex-basis: 0;
    padding: 0;
  }

  .agent-views.is-island .drag-rail:not(:has(.live-status)) {
    display: none;
  }

  /* El rail se colapsa en isla (el grab ya es el agarre), pero con el atajo a
     la consola viva tiene que medir algo: a flex-basis 0 el botón queda
     aplastado contra el setup y no hay por dónde volver. */
  .agent-views.is-island .drag-rail:has(.live-status) {
    min-height: 1.3rem;
    flex-basis: 1.3rem;
    padding: 0.15rem 0.45rem 0 0.55rem;
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
