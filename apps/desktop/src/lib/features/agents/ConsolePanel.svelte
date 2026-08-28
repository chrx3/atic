<script lang="ts">
  import { tip } from "$surfaces/overlay/tip.svelte";
  /**
   * Consola embebida (xterm + PTY): N pestañas, cada una Local (PowerShell) o
   * SSH (`ssh -t`).
   *
   * Antes eran exactamente dos —una local y una ssh— y reconectar mataba a la
   * del mismo tipo. Ahora la unidad es la pestaña: cada una tiene su PTY, su id
   * y su ciclo de vida, y abrir o cerrar una no toca a las demás. El tope real
   * lo pone Rust (`MAX_CONSOLES`), porque cada sesión es un proceso vivo.
   */
  import { onDestroy, onMount, untrack } from "svelte";
  import { fade } from "svelte/transition";
  import { ms, MOTION } from "$lib/motion";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import "@xterm/xterm/css/xterm.css";
  import {
    agentStageImage,
    agentsAlwaysOnTop,
    consoleClose,
    consoleGc,
    consoleOpen,
    consoleResize,
    consoleWrite,
    cliOnPath,
    onAgentsWorkspaceShortcut,
    onConsoleExit,
    onConsoleOutput,
    setAgentsAlwaysOnTop,
    sshConfigAliases,
    sshListHosts,
  } from "$ipc/agents";
  import { AGENTS } from "./agentCatalog";
  import { getConfig } from "$ipc/config";
  import {
    CLIPBOARD_OLE_EVENT,
    onAgentsComposerInsert,
    readClipboardDragText,
    type ClipboardOleDetail,
  } from "$ipc/clipboard";
  import { overlayCursor, pillTrace, setOverlayTextMode } from "$ipc/overlay";
  import type { AgentsWorkspaceShortcut } from "$ipc/events";
  import type { AgentsComposerInsert, ConsoleKind, SshHost } from "$lib/types";
  import EmptyState from "$lib/ui/EmptyState.svelte";
  import AccountUsageModal from "./AccountUsageModal.svelte";
  import AgentLogo from "./AgentLogo.svelte";
  import Icon from "$ui/Icon.svelte";
  import {
    ArrowLeft,
    Activity,
    Folder,
    Minus,
    Pin,
    Plus,
    Square,
    SquareTerminal,
    X,
    Keyboard,
    EllipsisVertical,
  } from "$lib/icons";
  import { t } from "$lib/domain/i18n.svelte";
  import { themeBase } from "$lib/theme";

  let {
    remoteHost = null,
    localCwd = "",
    initialKind = "local",
    initialTabs = null,
    onClose,
    onBack,
    onEmpty,
    onPickFolder,
    onBarPointerDown,
    onToggleMaximize,
    onToggleMinimize,
    maximized = false,
    minimized = false,
  }: {
    /** Host SSH del destino actual de agentes; default de una pestaña nueva. */
    remoteHost?: SshHost | null;
    /** cwd local opcional al abrir PowerShell. */
    localCwd?: string;
    /** Tipo de la primera pestaña al montar (si no hay `initialTabs`). */
    initialKind?: ConsoleKind;
    /**
     * Pestañas sembradas al montar (lanzador de agentes): cada una spawnea
     * su CLI directo en vez de la shell. Vacío = una pestaña del tipo inicial.
     */
    initialTabs?: ConsoleSeed[] | null;
    onClose?: () => void;
    /** Vuelve al lanzador sin cerrar las PTYs que siguen vivas. */
    onBack?: () => void;
    /**
     * Se cerró la última pestaña: ya no queda nada que mostrar acá. El
     * lanzador decide qué hacer (volver al inicio de agentes).
     */
    onEmpty?: () => void;
    /**
     * Pide al lanzador que abra el explorador de carpetas (vive allá: es un
     * modal del float entero). Resuelve con la ruta elegida, o `null` si se
     * canceló, así quien lo pide puede seguir el hilo donde lo dejó.
     */
    onPickFolder?: () => Promise<string | null>;
    /**
     * Arrastre del float desde la barra (fondo, no controles). La barra NO
     * lleva `data-no-drag` justamente para que este handler pueda tomarla.
     */
    onBarPointerDown?: (e: PointerEvent) => void;
    /** Agrandar al área de trabajo del monitor (toggle restaura). */
    onToggleMaximize?: () => void;
    /** Colapsar el float a solo la barra (toggle restaura). */
    onToggleMinimize?: () => void;
    maximized?: boolean;
    minimized?: boolean;
  } = $props();

  /** Semilla de pestaña del lanzador: consola local corriendo un agente. */
  type ConsoleSeed = {
    kind: ConsoleKind;
    label?: string;
    command?: string;
  };

  /**
   * Espejo de `MAX_CONSOLES` en `console.rs`. Rust es quien manda; esto solo
   * evita abrir una pestaña que ya sabemos que va a fallar al conectar.
   */
  const MAX_TABS = 12;

  /**
   * Lo que la vista necesita de una pestaña.
   *
   * Sin el `Terminal` adentro a propósito: `$state` proxya en profundidad y
   * envolver un xterm en un Proxy lo rompe. Lo pesado vive en `boxes`.
   */
  type Tab = {
    /** Estable aunque el PTY se reconecte; clave del `{#each}` y de `boxes`. */
    key: string;
    kind: ConsoleKind;
    /** PTY vivo. `null` = sin conectar todavía, o ya terminó. */
    sessionId: string | null;
    /** Host SSH de ESTA pestaña (solo `kind === "ssh"`). */
    hostId: string | null;
    /** Etiqueta fija (nombre del agente) que tapa el default Local/SSH. */
    label: string | null;
    /** CLI a spawnear en la PTY local; `null` = shell del sistema. */
    command: string | null;
    /**
     * Carpeta con la que nació ESTA pestaña. Se congela al crearla: cambiar
     * la carpeta de inicio después apunta a las consolas nuevas, y reconectar
     * una vieja la devuelve a donde estaba, no a la carpeta de moda.
     */
    cwd: string | null;
  };

  type Box = { term: Terminal; fit: FitAddon; el: HTMLElement };

  type SplitDirection = "right" | "down";
  type PaneNode =
    | { kind: "leaf"; key: string }
    | {
        kind: "split";
        direction: SplitDirection;
        first: PaneNode;
        second: PaneNode;
        /** Fracción del espacio para `first` (0..1). Ausente = mitad. */
        ratio?: number;
      };
  type PaneRect = { key: string; x: number; y: number; width: number; height: number };
  /** Un divisor arrastrable entre los dos hijos de un split. Todo en % del body. */
  type PaneDivider = {
    /** Camino al split en el árbol: "f"/"s" por nivel. Estable mientras no cambie la forma. */
    path: string;
    direction: SplitDirection;
    /** Rect completo del split (para traducir el puntero a ratio). */
    x: number;
    y: number;
    width: number;
    height: number;
    /** La costura: coordenada x si es `right`, y si es `down`. */
    seam: number;
  };

  const RAIL_MIN = 54;
  const RAIL_DEFAULT = 128;
  const RAIL_MAX = 224;
  const RAIL_STORAGE_KEY = "atic.agents.consoleRailWidth";
  /** Zoom del texto: offset en px sobre el tamaño base que decide el ancho. */
  const FONT_ZOOM_KEY = "atic.agents.consoleFontZoom";
  const FONT_ZOOM_MIN = -5;
  const FONT_ZOOM_MAX = 12;
  const USAGE_AGENTS = new Set(["claude", "codex", "opencode", "cursor-agent"]);

  function leaf(key: string): PaneNode {
    return { kind: "leaf", key };
  }

  function paneLeafKeys(node: PaneNode | null): string[] {
    if (!node) return [];
    if (node.kind === "leaf") return [node.key];
    return [...paneLeafKeys(node.first), ...paneLeafKeys(node.second)];
  }

  function replacePaneLeaf(
    node: PaneNode,
    key: string,
    replacement: PaneNode,
  ): PaneNode {
    if (node.kind === "leaf") return node.key === key ? replacement : node;
    return {
      ...node,
      first: replacePaneLeaf(node.first, key, replacement),
      second: replacePaneLeaf(node.second, key, replacement),
    };
  }

  function removePaneLeaf(node: PaneNode, key: string): PaneNode | null {
    if (node.kind === "leaf") return node.key === key ? null : node;
    const first = removePaneLeaf(node.first, key);
    const second = removePaneLeaf(node.second, key);
    if (!first) return second;
    if (!second) return first;
    return { ...node, first, second };
  }

  function collectPaneRects(
    node: PaneNode | null,
    x = 0,
    y = 0,
    width = 100,
    height = 100,
  ): PaneRect[] {
    if (!node) return [];
    if (node.kind === "leaf") return [{ key: node.key, x, y, width, height }];
    const ratio = node.ratio ?? 0.5;
    if (node.direction === "right") {
      const first = width * ratio;
      return [
        ...collectPaneRects(node.first, x, y, first, height),
        ...collectPaneRects(node.second, x + first, y, width - first, height),
      ];
    }
    const first = height * ratio;
    return [
      ...collectPaneRects(node.first, x, y, width, first),
      ...collectPaneRects(node.second, x, y + first, width, height - first),
    ];
  }

  /** Un divisor por split, sobre la costura entre sus dos hijos. */
  function collectPaneDividers(
    node: PaneNode | null,
    path = "",
    x = 0,
    y = 0,
    width = 100,
    height = 100,
  ): PaneDivider[] {
    if (!node || node.kind === "leaf") return [];
    const ratio = node.ratio ?? 0.5;
    if (node.direction === "right") {
      const first = width * ratio;
      return [
        { path, direction: "right", x, y, width, height, seam: x + first },
        ...collectPaneDividers(node.first, path + "f", x, y, first, height),
        ...collectPaneDividers(node.second, path + "s", x + first, y, width - first, height),
      ];
    }
    const first = height * ratio;
    return [
      { path, direction: "down", x, y, width, height, seam: y + first },
      ...collectPaneDividers(node.first, path + "f", x, y, width, first),
      ...collectPaneDividers(node.second, path + "s", x, y + first, width, height - first),
    ];
  }

  let tabs = $state<Tab[]>([]);
  let activeKey = $state("");
  let connecting = $state(false);
  let error = $state<string | null>(null);
  let sshHosts = $state<SshHost[]>([]);
  /** `group: true` = menú de una ficha de grupo del rail; `key` es su ancla. */
  let ctxMenu = $state<{ x: number; y: number; key: string; group?: boolean } | null>(
    null,
  );
  /** Árbol de splits. Las pestañas fuera del árbol siguen vivas en el rail. */
  let paneTree = $state<PaneNode | null>(null);
  /**
   * Los grupos: cada división con más de un pane, visible o no.
   *
   * Un split es una unidad — las consolas se dividieron para verse juntas—
   * y en el rail se presenta como UNA ficha. Abrir otra consola no desaloja
   * a nadie: la división queda guardada acá y su ficha la restaura entera.
   * Puede haber varios a la vez; una consola pertenece a lo más a uno. Los
   * mantiene al día el `$effect` de abajo; un grupo muere cuando le queda
   * menos de dos consolas vivas (o al separarlo a mano).
   */
  let groups = $state<PaneNode[]>([]);
  let railWidth = $state(RAIL_DEFAULT);
  /** Un solo zoom para todas las consolas, como el zoom de una app. */
  let fontZoom = $state(0);
  let pinned = $state(false);
  let usageOpen = $state(false);
  let shortcutsOpen = $state(false);
  let moreOpen = $state(false);
  let addMenuOpen = $state(false);
  /** `cli → en PATH`. Se llena al abrir el menú "+" por primera vez. */
  let agentOnPath = $state<Record<string, boolean>>({});
  let agentPathChecked = false;
  /** Aliases `Host` de ~/.ssh/config (VS Code / Cursor los usan igual). */
  let sshAliases = $state<string[]>([]);
  /** Comandos que el usuario guardó desde "Comando…" (`ssh root@ip`, `dashboard`). */
  let savedCmds = $state<string[]>([]);
  let cmdPromptOpen = $state(false);
  let cmdText = $state("");
  let consoleEl = $state<HTMLElement | null>(null);

  /** xterm por pestaña. Fuera de `$state` (ver `Tab`). */
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- xterm instances are imperative, not UI state.
  const boxes = new Map<string, Box>();
  let stopListen: (() => void) | null = null;
  let seq = 0;
  /** Output que llegó antes de mapear la sesión o de abrir el xterm. */
  const outputBuf = new Map<string, string>();
  const OUTPUT_BUF_MAX = 256_000;
  let resolveListen: () => void = () => {};
  const listenReady = new Promise<void>((resolve) => {
    resolveListen = resolve;
  });
  /** El TUI de Cursor/Claude no se recobra si el PTY nace en un panel de 40 px. */
  const MIN_TERM_W = 280;
  const MIN_TERM_H = 160;
  let pendingKeys = $state<Record<string, true>>({});
  let bootedKeys = $state<Record<string, true>>({});
  const bootTimers = new Map<string, number>();

  const active = $derived(tabs.find((t) => t.key === activeKey) ?? null);
  const usageAgent = $derived(
    active?.command && USAGE_AGENTS.has(active.command) ? active.command : null,
  );
  const sessionId = $derived(active?.sessionId ?? null);
  const connected = $derived(!!sessionId);
  const paneRects = $derived(collectPaneRects(paneTree));
  const paneDividers = $derived(collectPaneDividers(paneTree));
  const visiblePaneKeys = $derived(
    paneRects
      .map((pane) => pane.key)
      .filter((key) => tabs.some((tab) => tab.key === key)),
  );
  const paneMode = $derived(visiblePaneKeys.length > 1);

  /** Un árbol sin sus pestañas muertas; `null` si no reúne dos vivas. */
  function prunedTree(tree: PaneNode): PaneNode | null {
    let node: PaneNode | null = tree;
    for (const paneKey of paneLeafKeys(tree)) {
      if (tabs.some((t) => t.key === paneKey)) continue;
      node = node ? removePaneLeaf(node, paneKey) : null;
    }
    return node && paneLeafKeys(node).length > 1 ? node : null;
  }

  /** El grupo (podado) al que pertenece `key`, o `null` si va suelta. */
  function groupWith(key: string): PaneNode | null {
    for (const g of groups) {
      const pruned = prunedTree(g);
      if (pruned && paneLeafKeys(pruned).includes(key)) return pruned;
    }
    return null;
  }

  // Toda división visible ES un grupo: así el rail la funde en una ficha
  // apenas nace (split, drop de arrastre) sin repartir la escritura por
  // cada camino que arma un split. Upsert: reemplaza al grupo que comparta
  // consolas con la vista, le roba miembros a cualquier otro (una consola
  // pertenece a lo más a un grupo) y entierra al que quede con menos de dos.
  $effect(() => {
    if (visiblePaneKeys.length <= 1 || !paneTree) return;
    const visible = paneTree;
    const visibleKeys = paneLeafKeys(visible);
    untrack(() => {
      const next: PaneNode[] = [];
      let replaced = false;
      for (const g of groups) {
        const shares = paneLeafKeys(g).some((k) => visibleKeys.includes(k));
        if (!shares) {
          next.push(g);
        } else if (!replaced) {
          next.push(visible);
          replaced = true;
        } else {
          let rest: PaneNode | null = g;
          for (const k of visibleKeys) {
            rest = rest ? removePaneLeaf(rest, k) : null;
          }
          if (rest && paneLeafKeys(rest).length > 1) next.push(rest);
        }
      }
      if (!replaced) next.push(visible);
      // Solo escribir si cambió algo: el efecto relee `paneTree` a fondo
      // (también en el arrastre de la costura) y reescribir en cada frame
      // despertaría a todo el rail sin motivo.
      const shape = (list: PaneNode[]) =>
        list.map((g) => paneLeafKeys(g).join(",")).join("|");
      if (shape(groups) !== shape(next)) groups = next;
    });
  });

  const railCompact = $derived(railWidth < 92);
  const hasIdleTab = $derived(
    tabs.some((t) => !t.sessionId && !pendingKeys[t.key]),
  );
  const canAddTab = $derived(tabs.length < MAX_TABS || hasIdleTab);

  /**
   * Carpeta con la que nacen las consolas nuevas. La fuente de verdad vive en
   * el lanzador (que la persiste); acá solo se lee y se pide cambiarla.
   */
  const startFolder = $derived(localCwd.trim() || null);

  /** Última carpeta de la ruta: en la barra no cabe el path entero. */
  function folderName(path: string | null): string {
    if (!path) return "Carpeta de inicio";
    const parts = path.split(/[/\\]/).filter(Boolean);
    return parts.at(-1) || path;
  }

  /**
   * Abre el explorador del lanzador. `reopenAddMenu` devuelve al menú "+"
   * después de elegir, que es de donde venía el usuario al crear un agente.
   */
  async function pickStartFolder(reopenAddMenu = false) {
    if (!onPickFolder) return;
    addMenuOpen = false;
    cmdPromptOpen = false;
    moreOpen = false;
    const picked = await onPickFolder();
    if (picked) followStartFolder(picked);
    if (reopenAddMenu && picked) addMenuOpen = true;
  }

  /** Ruta como literal de shell: PowerShell dobla la comilla, sh la escapa. */
  function quotePath(path: string): string {
    const windows =
      typeof navigator !== "undefined" && /Win/i.test(navigator.userAgent);
    return windows
      ? `'${path.replace(/'/g, "''")}'`
      : `'${path.replace(/'/g, "'\\''")}'`;
  }

  /**
   * Muda las consolas vivas a la carpeta recién elegida.
   *
   * Solo las shells locales: una pestaña con un agente adentro está en su TUI,
   * y escribirle un `cd` no cambiaría de carpeta — le mandaría texto al agente
   * y cortaría la sesión. Esas se quedan donde nacieron.
   */
  function followStartFolder(folder: string) {
    const dir = folder.trim();
    if (!dir) return;
    for (const tab of tabs) {
      if (tab.kind !== "local" || tab.command) continue;
      const id = tab.sessionId;
      if (!id) continue;
      tab.cwd = dir;
      void consoleWrite(id, `cd ${quotePath(dir)}\r`).catch(() => {});
    }
  }

  function focusVisiblePane(key: string) {
    scheduleFitVisible();
    requestAnimationFrame(() => {
      if (sessionOf(key)) requestOverlayKeyboard(key);
    });
  }

  function splitPane(direction: SplitDirection) {
    const sourceKey = visiblePaneKeys.includes(activeKey)
      ? activeKey
      : (visiblePaneKeys[0] ?? activeKey);
    if (!sourceKey) {
      newTab("local");
      return;
    }

    const hidden = tabs.find((tab) => !visiblePaneKeys.includes(tab.key));
    if (hidden) {
      const base = paneTree ?? leaf(sourceKey);
      paneTree = replacePaneLeaf(base, sourceKey, {
        kind: "split",
        direction,
        first: leaf(sourceKey),
        second: leaf(hidden.key),
      });
      activeKey = hidden.key;
      error = null;
      focusVisiblePane(hidden.key);
      return;
    }

    const source = active;
    newTab(source?.kind ?? "local", {
      label: source?.label ?? undefined,
      command: source?.command ?? undefined,
      hostId: source?.hostId ?? undefined,
      splitDirection: direction,
      splitSourceKey: sourceKey,
    });
  }

  function traceWorkspaceShortcut(
    source: "window" | "xterm" | "xterm-data" | "native",
    action: "split-right" | "split-down" | "new-console" | "close-console",
    event?: KeyboardEvent,
  ) {
    const details = event
      ? ` code=${event.code || "-"} key=${JSON.stringify(event.key)} repeat=${event.repeat}`
      : "";
    void pillTrace(`[agents-key] dom source=${source} action=${action}${details}`);
  }

  function consumeWorkspaceShortcut(
    event: KeyboardEvent,
    source: "window" | "xterm",
  ): boolean {
    if (event.isComposing) return false;
    const key = event.key.toLowerCase();
    const code = event.code;
    const mod = event.ctrlKey || event.metaKey;

    // `code` cubre WebView2/xterm cuando Ctrl transforma `event.key` en un
    // carácter de control antes de que Svelte reciba el acorde.
    if (mod && !event.altKey && (code === "KeyD" || key === "d")) {
      event.preventDefault();
      event.stopPropagation();
      if (!event.repeat) {
        const direction = event.shiftKey ? "down" : "right";
        traceWorkspaceShortcut(
          source,
          direction === "down" ? "split-down" : "split-right",
          event,
        );
        splitPane(direction);
      }
      return true;
    }

    if (mod && !event.shiftKey && !event.altKey && (code === "KeyN" || key === "n")) {
      event.preventDefault();
      event.stopPropagation();
      if (!event.repeat) {
        traceWorkspaceShortcut(source, "new-console", event);
        newTab("local");
      }
      return true;
    }

    if (mod && !event.shiftKey && !event.altKey && (code === "KeyW" || key === "w")) {
      event.preventDefault();
      event.stopPropagation();
      if (!event.repeat && activeKey) {
        traceWorkspaceShortcut(source, "close-console", event);
        void closeTab(activeKey);
      }
      return true;
    }

    // Zoom. Por `key` y no por `code`: "+"/"-" viven en teclas distintas
    // según el layout (en latam "+" va sin Shift), y el numpad se suma por
    // `code` porque ahí `key` depende de Bloq Num. Sin filtrar `repeat`:
    // mantener apretado sigue acercando, como en un navegador.
    if (mod && !event.altKey && (key === "+" || key === "=" || code === "NumpadAdd")) {
      event.preventDefault();
      event.stopPropagation();
      setFontZoom(fontZoom + 1);
      return true;
    }
    if (mod && !event.altKey && (key === "-" || code === "NumpadSubtract")) {
      event.preventDefault();
      event.stopPropagation();
      setFontZoom(fontZoom - 1);
      return true;
    }
    if (mod && !event.shiftKey && !event.altKey && (key === "0" || code === "Numpad0")) {
      event.preventDefault();
      event.stopPropagation();
      setFontZoom(0);
      return true;
    }

    return false;
  }

  /**
   * WebView2 omite algunos `keydown` sin Shift dentro del textarea de xterm,
   * pero xterm sí los traduce a sus bytes de control. Consumirlos acá evita
   * que Ctrl+D llegue como EOF al CLI y que Ctrl+W borre una palabra.
   */
  function consumeTerminalControlData(key: string, data: string): boolean {
    if (
      data !== "\x04" &&
      data !== "\x0e" &&
      data !== "\x17" &&
      data !== "\x1f"
    ) {
      return false;
    }
    // Ctrl+- llega como 0x1F (Ctrl+_) cuando el keydown no aparece. El precio
    // es que el undo de readline (Ctrl+_) queda detrás del zoom.
    if (data === "\x1f") {
      setFontZoom(fontZoom - 1);
      return true;
    }
    activeKey = key;
    error = null;
    if (data === "\x04") {
      traceWorkspaceShortcut("xterm-data", "split-right");
      splitPane("right");
    } else if (data === "\x0e") {
      traceWorkspaceShortcut("xterm-data", "new-console");
      newTab("local");
    } else if (activeKey) {
      traceWorkspaceShortcut("xterm-data", "close-console");
      void closeTab(key);
    }
    return true;
  }

  function clampFontZoom(zoom: number): number {
    return Math.min(FONT_ZOOM_MAX, Math.max(FONT_ZOOM_MIN, Math.round(zoom)));
  }

  function setFontZoom(zoom: number) {
    const next = clampFontZoom(zoom);
    if (next === fontZoom) return;
    fontZoom = next;
    localStorage.setItem(FONT_ZOOM_KEY, String(next));
    // El refit aplica el tamaño nuevo y re-dimensiona el PTY a los cols/rows
    // resultantes; las pestañas ocultas lo reciben al volver a la vista.
    scheduleFitVisible();
  }

  /** Ctrl+rueda sobre la consola: zoom, como en un navegador o editor. */
  function onConsoleWheel(event: WheelEvent) {
    if (!event.ctrlKey) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.deltaY === 0) return;
    setFontZoom(fontZoom + (event.deltaY < 0 ? 1 : -1));
  }

  function clampRailWidth(width: number): number {
    return Math.min(RAIL_MAX, Math.max(RAIL_MIN, width));
  }

  function setRailWidth(width: number) {
    railWidth = clampRailWidth(width);
    localStorage.setItem(RAIL_STORAGE_KEY, String(Math.round(railWidth)));
  }

  /** Límite del arrastre: ningún hijo baja de esta fracción del split. */
  const SPLIT_RATIO_MIN = 0.15;

  /** El split al que llega `path` ("f"/"s" por nivel desde la raíz). */
  function splitAtPath(path: string): Extract<PaneNode, { kind: "split" }> | null {
    let node: PaneNode | null = paneTree;
    for (const step of path) {
      if (!node || node.kind !== "split") return null;
      node = step === "f" ? node.first : node.second;
    }
    return node && node.kind === "split" ? node : null;
  }

  function resetDividerRatio(path: string) {
    const split = splitAtPath(path);
    if (!split) return;
    split.ratio = 0.5;
    scheduleFitVisible();
  }

  /**
   * Arrastra la costura de un split. El rect del split capturado no cambia
   * durante el gesto (solo depende de los ratios de sus ancestros), así que
   * el puntero se traduce a ratio con una regla de tres contra el body.
   */
  function startDividerDrag(divider: PaneDivider, event: PointerEvent) {
    event.preventDefault();
    event.stopPropagation();
    const body = bodyEl;
    const split = splitAtPath(divider.path);
    if (!body || !split) return;
    const handle = event.currentTarget as HTMLElement;
    const pointerId = event.pointerId;
    handle.setPointerCapture(pointerId);
    const rect = body.getBoundingClientRect();
    const horizontal = divider.direction === "right";
    const originPx = horizontal
      ? rect.left + (divider.x / 100) * rect.width
      : rect.top + (divider.y / 100) * rect.height;
    const sizePx = horizontal
      ? (divider.width / 100) * rect.width
      : (divider.height / 100) * rect.height;

    const onMove = (moveEvent: PointerEvent) => {
      if (sizePx <= 0) return;
      const pos = horizontal ? moveEvent.clientX : moveEvent.clientY;
      split.ratio = Math.min(
        1 - SPLIT_RATIO_MIN,
        Math.max(SPLIT_RATIO_MIN, (pos - originPx) / sizePx),
      );
    };
    const onEnd = () => {
      handle.removeEventListener("pointermove", onMove);
      handle.removeEventListener("pointerup", onEnd);
      handle.removeEventListener("pointercancel", onEnd);
      if (handle.hasPointerCapture(pointerId)) handle.releasePointerCapture(pointerId);
      // El fit corre en vivo por los ResizeObserver; este es el asentamiento.
      scheduleFitVisible();
    };
    handle.addEventListener("pointermove", onMove);
    handle.addEventListener("pointerup", onEnd);
    handle.addEventListener("pointercancel", onEnd);
  }

  function startRailResize(event: PointerEvent) {
    event.preventDefault();
    event.stopPropagation();
    const handle = event.currentTarget as HTMLElement;
    const pointerId = event.pointerId;
    const startX = event.clientX;
    const startWidth = railWidth;
    handle.setPointerCapture(pointerId);

    const onMove = (moveEvent: PointerEvent) => {
      railWidth = clampRailWidth(startWidth + moveEvent.clientX - startX);
    };
    const onEnd = () => {
      localStorage.setItem(RAIL_STORAGE_KEY, String(Math.round(railWidth)));
      handle.removeEventListener("pointermove", onMove);
      handle.removeEventListener("pointerup", onEnd);
      handle.removeEventListener("pointercancel", onEnd);
      if (handle.hasPointerCapture(pointerId)) handle.releasePointerCapture(pointerId);
    };

    handle.addEventListener("pointermove", onMove);
    handle.addEventListener("pointerup", onEnd);
    handle.addEventListener("pointercancel", onEnd);
  }

  function onRailResizeKey(event: KeyboardEvent) {
    const step = event.shiftKey ? 24 : 8;
    if (event.key === "ArrowLeft") setRailWidth(railWidth - step);
    else if (event.key === "ArrowRight") setRailWidth(railWidth + step);
    else if (event.key === "Home") setRailWidth(RAIL_MIN);
    else if (event.key === "End") setRailWidth(RAIL_MAX);
    else return;
    event.preventDefault();
    event.stopPropagation();
  }

  function isInsideConsole(target: EventTarget | null): boolean {
    if (!consoleEl) return false;
    if (target instanceof Node && consoleEl.contains(target)) return true;
    return (
      document.activeElement instanceof Node &&
      consoleEl.contains(document.activeElement)
    );
  }

  function onGlobalKey(event: KeyboardEvent) {
    if (event.isComposing) return;
    // Esc con un menú abierto lo cierra a él, no a la ventana entera: es lo
    // que hace cualquier menú nativo. stopPropagation frena el Esc del float.
    if (
      event.key === "Escape" &&
      (ctxMenu || addMenuOpen || moreOpen || shortcutsOpen)
    ) {
      event.preventDefault();
      event.stopPropagation();
      closeCtx();
      addMenuOpen = false;
      moreOpen = false;
      shortcutsOpen = false;
      cmdPromptOpen = false;
      return;
    }
    if (!isInsideConsole(event.target)) return;
    // Captura antes del PTY. El handler de xterm repite esta defensa porque
    // WebView2 no siempre entrega los acordes Ctrl al `window` del overlay.
    consumeWorkspaceShortcut(event, "window");
  }

  function consumeNativeWorkspaceShortcut(action: AgentsWorkspaceShortcut) {
    traceWorkspaceShortcut("native", action);
    if (action === "split-right") splitPane("right");
    else if (action === "split-down") splitPane("down");
    else if (action === "new-console") newTab("local");
    else if (action === "close-console" && activeKey) void closeTab(activeKey);
  }

  // Al sumar/quitar paneles o cambiar el rail, cada xterm cambia de tamaño.
  $effect(() => {
    void paneRects;
    void railWidth;
    scheduleFitVisible();
  });

  function hostLabel(h: SshHost): string {
    return h.label?.trim() || (h.user?.trim() ? `${h.user}@${h.host}` : h.host);
  }

  function hostOptionLabel(h: SshHost): string {
    const name = h.label?.trim() || h.host;
    if (h.user?.trim()) return `${name} (${h.user}@${h.host})`;
    return name;
  }

  function hostById(id: string | null): SshHost | null {
    if (!id) return null;
    return (
      sshHosts.find((h) => h.id === id) ?? (remoteHost?.id === id ? remoteHost : null)
    );
  }

  const activeHost = $derived(active ? hostById(active.hostId) : null);
  const sshLabel = $derived(activeHost ? hostLabel(activeHost) : null);

  function baseLabel(t: Tab): string {
    if (t.label?.trim()) return t.label.trim();
    if (t.kind === "local") return "Local";
    const h = hostById(t.hostId);
    return h ? hostLabel(h) : "SSH";
  }

  /** Numera solo cuando el nombre se repite: "Local", "Local 2", "Local 3". */
  const tabLabels = $derived.by(() => {
    // eslint-disable-next-line svelte/prefer-svelte-reactivity -- temporary reducer map.
    const total = new Map<string, number>();
    for (const t of tabs) {
      const b = baseLabel(t);
      total.set(b, (total.get(b) ?? 0) + 1);
    }
    // eslint-disable-next-line svelte/prefer-svelte-reactivity -- temporary reducer map.
    const seen = new Map<string, number>();
    return tabs.map((t) => {
      const b = baseLabel(t);
      const n = (seen.get(b) ?? 0) + 1;
      seen.set(b, n);
      return (total.get(b) ?? 0) > 1 ? `${b} ${n}` : b;
    });
  });

  type RailGroup = { anchorKey: string; keys: string[]; tabs: Tab[]; label: string };

  /**
   * Las fichas de grupo del rail, una por división. La vista partida entra
   * directo (el `$effect` que la guarda corre después del render: sin esto,
   * un split recién nacido parpadearía un frame como fichas sueltas).
   */
  const railGroups = $derived.by(() => {
    const trees: PaneNode[] = [];
    const visible = visiblePaneKeys.length > 1 ? paneTree : null;
    const visibleKeys = visible ? paneLeafKeys(visible) : [];
    if (visible) trees.push(visible);
    for (const g of groups) {
      if (visible && paneLeafKeys(g).some((k) => visibleKeys.includes(k))) continue;
      trees.push(g);
    }
    const out: RailGroup[] = [];
    for (const tree of trees) {
      const pruned = prunedTree(tree);
      if (!pruned) continue;
      const keys = paneLeafKeys(pruned);
      const members = keys
        .map((key) => tabs.find((tab) => tab.key === key))
        .filter((tab): tab is Tab => !!tab);
      const anchor = tabs.find((tab) => keys.includes(tab.key));
      if (members.length < 2 || !anchor) continue;
      out.push({
        anchorKey: anchor.key,
        keys,
        tabs: members,
        label: members
          .map((tab) => tabLabels[tabs.indexOf(tab)] ?? "")
          .filter(Boolean)
          .join(" · "),
      });
    }
    return out;
  });

  function tabOf(key: string): Tab | undefined {
    return tabs.find((t) => t.key === key);
  }

  function termOf(key: string): Terminal | null {
    return boxes.get(key)?.term ?? null;
  }

  function sessionOf(key: string): string | null {
    return tabOf(key)?.sessionId ?? null;
  }

  function setSession(key: string, id: string | null) {
    const t = tabOf(key);
    if (t) t.sessionId = id;
  }

  function tabForSession(id: string): Tab | undefined {
    return tabs.find((t) => t.sessionId === id);
  }

  function pushOutput(session: string, data: string) {
    const t = tabForSession(session);
    const term = t ? termOf(t.key) : null;
    if (term) {
      term.write(data);
      if (t) markBooted(t.key);
      return;
    }
    const prev = outputBuf.get(session) ?? "";
    const next =
      prev.length + data.length > OUTPUT_BUF_MAX
        ? (prev + data).slice(-OUTPUT_BUF_MAX)
        : prev + data;
    outputBuf.set(session, next);
  }

  function flushOutput(session: string, key: string) {
    const pending = outputBuf.get(session);
    if (!pending) return;
    outputBuf.delete(session);
    termOf(key)?.write(pending);
    markBooted(key);
  }

  function markPending(key: string, on: boolean) {
    if (on) {
      pendingKeys = { ...pendingKeys, [key]: true };
      return;
    }
    if (!pendingKeys[key]) return;
    const next = { ...pendingKeys };
    delete next[key];
    pendingKeys = next;
  }

  function markBooted(key: string) {
    const timer = bootTimers.get(key);
    if (timer) {
      window.clearTimeout(timer);
      bootTimers.delete(key);
    }
    if (bootedKeys[key]) return;
    bootedKeys = { ...bootedKeys, [key]: true };
    markPending(key, false);
  }

  function armBootTimeout(key: string) {
    const prev = bootTimers.get(key);
    if (prev) window.clearTimeout(prev);
    bootTimers.set(
      key,
      window.setTimeout(() => markBooted(key), 2500),
    );
  }

  function paneLoading(key: string): boolean {
    const tab = tabOf(key);
    if (!tab || bootedKeys[key]) return false;
    if (pendingKeys[key]) return true;
    return !!tab.sessionId && !!tab.command;
  }

  const CONSOLE_SHORTCUTS = [
    { keys: "Ctrl+D", labelKey: "page.agents.shortcutSplitRight" },
    { keys: "Ctrl+Shift+D", labelKey: "page.agents.shortcutSplitDown" },
    { keys: "Ctrl+N", labelKey: "page.agents.shortcutNew" },
    { keys: "Ctrl+W", labelKey: "page.agents.shortcutClose" },
    { keys: "Ctrl + / −", labelKey: "page.agents.shortcutZoom" },
    { keys: "Ctrl+0", labelKey: "page.agents.shortcutZoomReset" },
  ] as const;

  async function loadSshHosts() {
    try {
      sshHosts = await sshListHosts();
    } catch {
      try {
        const cfg = await getConfig();
        sshHosts = cfg.ssh_hosts ?? [];
      } catch {
        sshHosts = [];
      }
    }
    // Una pestaña ssh abierta antes de que llegara la lista se queda sin
    // destino: al llegar se le asigna uno en vez de dejarla muerta.
    for (const t of tabs) {
      if (t.kind === "ssh" && !hostById(t.hostId)) {
        t.hostId = remoteHost?.id ?? sshHosts[0]?.id ?? null;
      }
    }
  }

  /** Solo enfoca xterm (el overlay text-mode lo pide OverlaySurface al clic). */
  function focusTerm(key = activeKey) {
    termOf(key)?.focus();
  }

  /**
   * Pedir teclado al overlay sin un pointerdown sintético: ese clic disparaba
   * `set_focusable` y dejaba la lámina opaca a pantalla completa (pill y main
   * muertos). El modo texto lo activa OverlaySurface con un clic real, o Rust
   * reponiendo click-through después de `set_overlay_text_mode`.
   */
  function requestOverlayKeyboard(key = activeKey) {
    void setOverlayTextMode(true).catch(() => {});
    focusTerm(key);
  }

  function closeCtx() {
    ctxMenu = null;
  }

  const SAVED_CMDS_KEY = "atic.agents.savedCommands";
  const SAVED_CMDS_MAX = 8;

  function toggleAddMenu() {
    addMenuOpen = !addMenuOpen;
    cmdPromptOpen = false;
    if (!addMenuOpen || agentPathChecked) return;
    agentPathChecked = true;
    void Promise.all(
      AGENTS.map(async (agent) => {
        try {
          return [agent.cli, await cliOnPath(agent.cli)] as const;
        } catch {
          return [agent.cli, true] as const;
        }
      }),
    ).then((rows) => {
      agentOnPath = Object.fromEntries(rows);
    });
    void sshConfigAliases()
      .then((aliases) => {
        sshAliases = aliases;
      })
      .catch(() => {
        sshAliases = [];
      });
  }

  function addFromMenu(seed: { kind: ConsoleKind; label?: string; command?: string }) {
    addMenuOpen = false;
    cmdPromptOpen = false;
    newTab(seed.kind, { label: seed.label, command: seed.command });
  }

  /**
   * Corre el instalador oficial del agente en una consola nueva: se ve el
   * progreso y cualquier error, sin ventanas aparte. Al reabrir el menú "+"
   * se reverifica el PATH, así el agente aparece habilitado si terminó bien.
   */
  function installAgent(agent: (typeof AGENTS)[number]) {
    addFromMenu({
      kind: "local",
      label: `Instalar ${agent.name}`,
      command: agent.install,
    });
    agentPathChecked = false;
  }

  function persistSavedCmds() {
    try {
      localStorage.setItem(SAVED_CMDS_KEY, JSON.stringify(savedCmds));
    } catch {
      /* la lista sigue en memoria aunque el storage esté bloqueado */
    }
  }

  function runCmdPrompt() {
    const cmd = cmdText.trim();
    if (!cmd) return;
    savedCmds = [cmd, ...savedCmds.filter((c) => c !== cmd)].slice(0, SAVED_CMDS_MAX);
    persistSavedCmds();
    cmdText = "";
    addFromMenu({ kind: "local", label: cmd, command: cmd });
  }

  function removeSavedCmd(cmd: string) {
    savedCmds = savedCmds.filter((c) => c !== cmd);
    persistSavedCmds();
  }

  async function disconnect(key = activeKey) {
    const id = sessionOf(key);
    setSession(key, null);
    markPending(key, false);
    const timer = bootTimers.get(key);
    if (timer) {
      window.clearTimeout(timer);
      bootTimers.delete(key);
    }
    if (bootedKeys[key]) {
      const next = { ...bootedKeys };
      delete next[key];
      bootedKeys = next;
    }
    if (id) {
      outputBuf.delete(id);
      try {
        await consoleClose(id);
      } catch {
        /* ya cerró */
      }
    }
  }

  async function disconnectAll() {
    await Promise.all(tabs.map((t) => disconnect(t.key)));
  }

  function knownSessionIds(): string[] {
    return tabs
      .map((t) => t.sessionId)
      .filter((id): id is string => !!id);
  }

  async function reapOrphanConsoles() {
    try {
      await consoleGc(knownSessionIds());
    } catch {
      /* backend viejo o mapa ya vacío */
    }
  }

  function termBoxSize(key: string): { w: number; h: number } | null {
    const box = boxes.get(key);
    if (!box) return null;
    const w = box.el.clientWidth;
    const h = box.el.clientHeight;
    if (w < 24 || h < 24) return null;
    return { w, h };
  }

  async function waitForTermReady(key: string, timeoutMs = 2800): Promise<void> {
    const start = performance.now();
    while (performance.now() - start < timeoutMs) {
      const size = termBoxSize(key);
      const hostH = consoleEl?.clientHeight ?? 0;
      if (size && size.w >= MIN_TERM_W && size.h >= MIN_TERM_H && hostH >= 280) {
        fitTermOnly(key);
        return;
      }
      await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    }
    fitTermOnly(key);
  }

  async function connect(key = activeKey) {
    const tab = tabOf(key);
    if (!tab) return;
    error = null;
    if (tab.kind === "ssh" && !hostById(tab.hostId)) {
      error = "Elige un host SSH en la consola (o agrégalo en Ajustes → Agentes).";
      return;
    }
    connecting = true;
    markPending(key, true);
    await listenReady.catch(() => {});
    await waitForTermReady(key);
    if (!tabOf(key)) {
      connecting = false;
      markPending(key, false);
      return;
    }
    await disconnect(key);
    markPending(key, true);
    termOf(key)?.reset();
    try {
      const live = tabOf(key);
      if (!live) return;
      const term = termOf(key);
      // Medir AHORA: el float acaba de crecer. Si spawneamos en el seed
      // (40 px) el TUI del agente nace en 2×1 y no se recobra.
      fitTermOnly(key);
      await reapOrphanConsoles();
      const openOpts = {
        kind: live.kind,
        hostId: live.kind === "ssh" ? live.hostId : null,
        cwd: live.kind === "local" ? live.cwd : null,
        command: live.kind === "local" ? live.command : null,
        // Piso 80×24: si el float todavía es el del lanzador, xterm cabe en
        // 5×2 y el TUI de Cursor nace muerto. SIGWINCH no lo recobra.
        cols: Math.max(80, term && term.cols > 8 ? term.cols : 80),
        rows: Math.max(24, term && term.rows > 8 ? term.rows : 24),
      };
      let id: string;
      try {
        id = await consoleOpen(openOpts);
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        if (!msg.includes(`${MAX_TABS} consolas`)) throw e;
        await reapOrphanConsoles();
        id = await consoleOpen(openOpts);
      }
      if (!tabOf(key)) {
        await consoleClose(id).catch(() => {});
        return;
      }
      setSession(key, id);
      flushOutput(id, key);
      if (live.command && !bootedKeys[key]) armBootTimeout(key);
      else if (!live.command) markBooted(key);
      requestAnimationFrame(() => {
        fitAndResize(key);
        if (key === activeKey) requestOverlayKeyboard(key);
      });
      setTimeout(() => fitAndResize(key), 350);
      setTimeout(() => fitAndResize(key), 800);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      setSession(key, null);
    } finally {
      connecting = false;
      markPending(key, false);
    }
  }

  /**
   * Abre una pestaña y, si ya tiene destino, la conecta sola.
   *
   * La conexión va en COLA y no en paralelo: `connect` guarda un `connecting`
   * global, y sembrar N pestañas del lanzador con N llamadas simultáneas
   * dejaría solo la primera conectada — el resto caía en el guard.
   */
  let openChain: Promise<void> = Promise.resolve();

  function idleTabKey(): string | null {
    const idle = (t: Tab) => !t.sessionId && !pendingKeys[t.key];
    const current = tabOf(activeKey);
    if (current && idle(current)) return current.key;
    return tabs.find(idle)?.key ?? null;
  }

  type NewTabOpts = {
    label?: string;
    command?: string;
    hostId?: string;
    splitDirection?: SplitDirection;
    splitSourceKey?: string;
  };

  function applyTabSeed(tab: Tab, kind: ConsoleKind, opts: NewTabOpts) {
    tab.kind = kind;
    tab.hostId =
      kind === "ssh"
        ? (opts.hostId ?? remoteHost?.id ?? sshHosts[0]?.id ?? null)
        : null;
    tab.label = opts.label?.trim() || null;
    tab.command = kind === "local" ? (opts.command?.trim() || null) : null;
    tab.cwd = kind === "local" ? startFolder : null;
  }

  function layoutTab(
    key: string,
    opts: NewTabOpts,
    treeBefore: PaneNode | null,
    previousActiveKey: string,
  ) {
    activeKey = key;
    const treeKeys = treeBefore ? paneLeafKeys(treeBefore) : [];
    if (opts.splitDirection && opts.splitSourceKey) {
      if (!treeKeys.includes(key)) {
        const sourceKey = opts.splitSourceKey;
        const base = treeBefore ?? leaf(sourceKey);
        paneTree = replacePaneLeaf(base, sourceKey, {
          kind: "split",
          direction: opts.splitDirection,
          first: leaf(sourceKey),
          second: leaf(key),
        });
      }
      return;
    }
    if (treeKeys.includes(key)) return;
    // Con la vista dividida, injertar acá echaba a una consola del grupo. El
    // grupo ya quedó guardado (el $effect lo mantiene): la nueva va sola.
    if (treeBefore && treeKeys.length > 1) {
      paneTree = leaf(key);
      return;
    }
    if (treeBefore && treeKeys.includes(previousActiveKey)) {
      paneTree = replacePaneLeaf(treeBefore, previousActiveKey, leaf(key));
    } else {
      paneTree = leaf(key);
    }
  }

  function queueConnect(key: string, kind: ConsoleKind) {
    if (kind === "local" || hostById(tabOf(key)?.hostId ?? null)) {
      connecting = true;
      markPending(key, true);
      openChain = openChain.then(() => connect(key)).catch(() => {});
    }
  }

  function newTab(kind: ConsoleKind, opts: NewTabOpts = {}) {
    closeCtx();
    const treeBefore = paneTree;
    const previousActiveKey = activeKey;
    let key: string;
    if (tabs.length >= MAX_TABS) {
      const idle = idleTabKey();
      if (!idle) {
        error = `Ya hay ${MAX_TABS} consolas abiertas. Cierra alguna para abrir otra.`;
        return;
      }
      const tab = tabOf(idle);
      if (!tab) return;
      applyTabSeed(tab, kind, opts);
      key = idle;
    } else {
      key = `t${++seq}`;
      const hostId =
        kind === "ssh"
          ? (opts.hostId ?? remoteHost?.id ?? sshHosts[0]?.id ?? null)
          : null;
      tabs = [
        ...tabs,
        {
          key,
          kind,
          sessionId: null,
          hostId,
          label: opts.label?.trim() || null,
          command: opts.command?.trim() || null,
          cwd: kind === "local" ? startFolder : null,
        },
      ];
    }
    layoutTab(key, opts, treeBefore, previousActiveKey);
    error = null;
    if (kind === "ssh") void loadSshHosts();
    // Un cuadro después el `{@attach}` ya creó el xterm. `connect` espera
    // tamaño real + listener; no spawnea en el frame del seed de 40 px.
    queueConnect(key, kind);
  }

  async function closeTab(key: string) {
    const idx = tabs.findIndex((t) => t.key === key);
    if (idx < 0) return;
    const paneIdx = visiblePaneKeys.indexOf(key);
    const nextTree = paneTree ? removePaneLeaf(paneTree, key) : null;
    closeCtx();
    await disconnect(key);
    tabs = tabs.filter((t) => t.key !== key);
    const nextPaneKeys = paneLeafKeys(nextTree).filter((paneKey) =>
      tabs.some((tab) => tab.key === paneKey),
    );
    // El xterm lo dispone el teardown del `{@attach}` al salir del DOM.
    if (activeKey === key) {
      activeKey =
        nextPaneKeys[Math.min(Math.max(paneIdx, 0), nextPaneKeys.length - 1)] ??
        tabs[Math.min(idx, tabs.length - 1)]?.key ??
        "";
    }
    // Si lo cerrado era la vista entera y la consola que hereda el foco
    // pertenece a un grupo, se restaura la división completa — no el miembro
    // solo, que dejaba al grupo "de a uno" hasta clicar su ficha.
    paneTree =
      nextTree ?? (activeKey ? (groupWith(activeKey) ?? leaf(activeKey)) : null);
    if (activeKey) focusVisiblePane(activeKey);
    // Última pestaña cerrada: el panel vacío no ofrece nada que el inicio de
    // agentes no haga mejor. Al final, con el estado ya asentado.
    if (tabs.length === 0) onEmpty?.();
  }

  /* ─── Arrastrar una ficha del rail al área de terminales ────────────────
     Soltar en el borde derecho/inferior de un panel lo divide; soltar al
     centro muestra esa consola ahí (o intercambia si ya estaba visible). */
  type DropZone = "center" | "right" | "down";
  const TAB_DRAG_THRESHOLD = 6;
  let tabDrag = $state<{ key: string; x: number; y: number } | null>(null);
  let dropHint = $state<{ key: string; zone: DropZone } | null>(null);
  /** Panel bajo el cursor al arrastrar desde el clipboard (OLE o HTML5). */
  let clipDropKey = $state<string | null>(null);
  let bodyEl = $state<HTMLElement | null>(null);
  let dragConsumedClick = false;

  function swapPaneLeaf(node: PaneNode, a: string, b: string): PaneNode {
    if (node.kind === "leaf") {
      if (node.key === a) return leaf(b);
      if (node.key === b) return leaf(a);
      return node;
    }
    return {
      ...node,
      first: swapPaneLeaf(node.first, a, b),
      second: swapPaneLeaf(node.second, a, b),
    };
  }

  function dropHintAt(x: number, y: number): { key: string; zone: DropZone } | null {
    if (!bodyEl) return null;
    const r = bodyEl.getBoundingClientRect();
    if (r.width <= 0 || x < r.left || x > r.right || y < r.top || y > r.bottom) {
      return null;
    }
    const rx = ((x - r.left) / r.width) * 100;
    const ry = ((y - r.top) / r.height) * 100;
    const pane = paneRects.find(
      (p) =>
        visiblePaneKeys.includes(p.key) &&
        rx >= p.x &&
        rx <= p.x + p.width &&
        ry >= p.y &&
        ry <= p.y + p.height,
    );
    if (!pane) return null;
    const lx = (rx - pane.x) / pane.width;
    const ly = (ry - pane.y) / pane.height;
    const zone: DropZone = lx > 0.6 ? "right" : ly > 0.6 ? "down" : "center";
    return { key: pane.key, zone };
  }

  function applyTabDrop(dragKey: string, hint: { key: string; zone: DropZone }) {
    const targetKey = hint.key;
    if (dragKey === targetKey) return;
    const dragVisible = visiblePaneKeys.includes(dragKey);
    let base = paneTree ?? leaf(targetKey);
    if (hint.zone === "center") {
      paneTree = dragVisible
        ? swapPaneLeaf(base, dragKey, targetKey)
        : replacePaneLeaf(base, targetKey, leaf(dragKey));
    } else {
      if (dragVisible) base = removePaneLeaf(base, dragKey) ?? leaf(targetKey);
      paneTree = replacePaneLeaf(base, targetKey, {
        kind: "split",
        direction: hint.zone,
        first: leaf(targetKey),
        second: leaf(dragKey),
      });
    }
    activeKey = dragKey;
    error = null;
    focusVisiblePane(dragKey);
  }

  function beginTabDrag(key: string, event: PointerEvent) {
    if (event.button !== 0 || tabs.length < 2) return;
    dragConsumedClick = false;
    const el = event.currentTarget as HTMLElement;
    const sx = event.clientX;
    const sy = event.clientY;
    const pointerId = event.pointerId;
    let engaged = false;

    const onMove = (e: PointerEvent) => {
      if (!engaged) {
        if (Math.hypot(e.clientX - sx, e.clientY - sy) < TAB_DRAG_THRESHOLD) return;
        engaged = true;
        try {
          el.setPointerCapture(pointerId);
        } catch {
          /* sin captura el drag igual funciona mientras el cursor siga encima */
        }
      }
      e.preventDefault();
      tabDrag = { key, x: e.clientX, y: e.clientY };
      dropHint = dropHintAt(e.clientX, e.clientY);
    };
    const onEnd = (e: PointerEvent) => {
      el.removeEventListener("pointermove", onMove);
      el.removeEventListener("pointerup", onEnd);
      el.removeEventListener("pointercancel", onEnd);
      if (el.hasPointerCapture?.(pointerId)) el.releasePointerCapture(pointerId);
      if (engaged) {
        dragConsumedClick = true;
        if (e.type === "pointerup" && dropHint) applyTabDrop(key, dropHint);
      }
      tabDrag = null;
      dropHint = null;
    };
    el.addEventListener("pointermove", onMove);
    el.addEventListener("pointerup", onEnd);
    el.addEventListener("pointercancel", onEnd);
  }

  /**
   * Muestra una consola (o su grupo). El grupo es una unidad: clicar su ficha
   * restaura la división entera, y clicar otra ficha muestra esa consola sola
   * sin desalojar a nadie —el swap por clic se fue; componer una división es
   * el gesto explícito de arrastrar una ficha sobre un pane—.
   */
  function switchTab(key: string) {
    closeCtx();
    paneTree = groupWith(key) ?? leaf(key);
    activeKey = key;
    error = null;
    // No se desconecta nada: las otras pestañas siguen vivas.
    focusVisiblePane(key);
    requestAnimationFrame(() => {
      for (const paneKey of visiblePaneKeys) {
        fitAndResize(paneKey);
        termOf(paneKey)?.refresh(0, Math.max(0, (termOf(paneKey)?.rows ?? 1) - 1));
      }
    });
  }

  /** La ficha de un grupo: vuelve a su división, con la consola activa de antes. */
  function switchToGroup(entry: RailGroup) {
    if (entry.keys.length === 0) return;
    switchTab(entry.keys.includes(activeKey) ? activeKey : entry.keys[0]);
  }

  /** Cerrar la ficha de un grupo cierra todas sus consolas. */
  async function closeGroup(entry: RailGroup) {
    for (const key of [...entry.keys]) {
      await closeTab(key);
    }
  }

  /**
   * Deshace un grupo sin cerrar nada: sus consolas vuelven como fichas
   * sueltas. Si era la división visible hay que colapsar la vista también —
   * con el split en pantalla, el `$effect` lo volvería a guardar al instante.
   */
  function detachGroup(entry: RailGroup) {
    const visible =
      visiblePaneKeys.length > 1 &&
      visiblePaneKeys.some((k) => entry.keys.includes(k));
    groups = groups.filter(
      (g) => !paneLeafKeys(g).some((k) => entry.keys.includes(k)),
    );
    if (visible) {
      const key = entry.keys.includes(activeKey) ? activeKey : entry.keys[0];
      paneTree = leaf(key);
      activeKey = key;
      focusVisiblePane(key);
    }
  }

  /**
   * Saca UNA consola de la división visible; sigue viva como ficha suelta.
   * El grupo guardado se achica acá mismo: si la vista queda en un solo pane
   * el `$effect` ya no corre y el grupo viejo quedaría zombi en el rail.
   */
  function removeFromGroup(key: string) {
    if (visiblePaneKeys.length <= 1 || !paneTree) return;
    const rest = removePaneLeaf(paneTree, key);
    if (!rest) return;
    groups = groups
      .map((g) => (paneLeafKeys(g).includes(key) ? removePaneLeaf(g, key) : g))
      .filter((g): g is PaneNode => !!g && paneLeafKeys(g).length > 1);
    paneTree = rest;
    if (activeKey === key) {
      const next = paneLeafKeys(rest).find((k) => tabs.some((t) => t.key === k));
      if (next) activeKey = next;
    }
    focusVisiblePane(activeKey);
  }

  /**
   * FitAddon lee `parseInt(getComputedStyle(parent).width)`. Con anchos tipo
   * `calc(50% - 0.36rem)` WebView2 a veces no resuelve a px: el fit no-opea y
   * el TUI del agente se queda del tamaño viejo (o en blanco). Medimos con
   * `clientWidth` y fijamos px solo durante la medición.
   *
   * Tampoco se manda un resize al PTY si el panel no está a la vista: un
   * xterm `visibility:hidden` o de 0 px deja al agente en 2×1 y no se recobra.
   */
  let fitSeq = 0;
  let fitting = false;
  let fitQueued: string | null = null;

  function scheduleFitVisible() {
    const pass = ++fitSeq;
    const run = () => {
      if (pass !== fitSeq) return;
      for (const key of visiblePaneKeys) fitAndResize(key);
    };
    requestAnimationFrame(() => {
      requestAnimationFrame(run);
    });
    window.setTimeout(run, 48);
  }

  function fitAndResize(key = activeKey) {
    const box = boxes.get(key);
    if (!box) return;
    if (!visiblePaneKeys.includes(key)) return;
    if (fitting) {
      fitQueued = key;
      return;
    }
    const w = box.el.clientWidth;
    const h = box.el.clientHeight;
    if (w < 24 || h < 24) return;

    const baseFontSize = w < 280 ? 10 : w < 420 ? 11 : 12;
    const nextFontSize = Math.max(7, baseFontSize + fontZoom);
    if (box.term.options.fontSize !== nextFontSize) {
      box.term.options.fontSize = nextFontSize;
    }

    fitting = true;
    const prevWidth = box.el.style.width;
    const prevHeight = box.el.style.height;
    box.el.style.width = `${w}px`;
    box.el.style.height = `${h}px`;
    try {
      box.fit.fit();
    } catch {
      /* contenedor sin tamaño todavía */
    } finally {
      box.el.style.width = prevWidth;
      box.el.style.height = prevHeight;
      fitting = false;
    }
    try {
      box.term.refresh(0, Math.max(0, box.term.rows - 1));
    } catch {
      /* renderer aún no listo */
    }

    const id = sessionOf(key);
    if (id && box.term.cols >= 2 && box.term.rows >= 2) {
      void consoleResize(id, box.term.cols, box.term.rows).catch(() => {});
    }
    const queued = fitQueued;
    fitQueued = null;
    if (queued && queued !== key) fitAndResize(queued);
  }

  /** Re-mide el xterm sin tocar el PTY (antes de que exista sesión). */
  function fitTermOnly(key = activeKey) {
    const box = boxes.get(key);
    if (!box) return;
    const w = box.el.clientWidth;
    const h = box.el.clientHeight;
    if (w < 24 || h < 24) return;
    fitting = true;
    const prevWidth = box.el.style.width;
    const prevHeight = box.el.style.height;
    box.el.style.width = `${w}px`;
    box.el.style.height = `${h}px`;
    try {
      box.fit.fit();
    } catch {
      /* contenedor sin tamaño todavía: queda el default */
    } finally {
      box.el.style.width = prevWidth;
      box.el.style.height = prevHeight;
      fitting = false;
    }
  }

  /**
   * Paleta del xterm según el tema resuelto (`data-theme` en :root).
   * La consola sigue el tema de la app; los TUI que pintan su propio fondo
   * (opencode claro) siguen siendo cosa del agente, no del terminal.
   *
   * Mira el lado de la tinta y no la paleta exacta: hay varios temas claros y
   * varios oscuros, y un terminal solo tiene estas dos versiones.
   */
  function termTheme(): Record<string, string> {
    const light = themeBase(document.documentElement.dataset.theme) === "light";
    return light
      ? {
          background: "#fbfbf8",
          foreground: "#24241f",
          cursor: "#d35f45",
          cursorAccent: "#fbfbf8",
          selectionBackground: "rgba(218, 119, 86, 0.3)",
          black: "#31312c",
          red: "#b43d3d",
          green: "#2f774d",
          yellow: "#806000",
          blue: "#3569a3",
          magenta: "#7d50a1",
          cyan: "#267580",
          white: "#d8d8d0",
          brightBlack: "#74746b",
          brightRed: "#d5544f",
          brightGreen: "#3b9360",
          brightYellow: "#a57a00",
          brightBlue: "#4b83c4",
          brightMagenta: "#9a68bf",
          brightCyan: "#3693a0",
          brightWhite: "#ffffff",
        }
      : {
          background: "#151715",
          foreground: "#e8e8e1",
          cursor: "#e36f52",
          cursorAccent: "#151715",
          selectionBackground: "rgba(218, 119, 86, 0.35)",
          black: "#22241f",
          red: "#e0675f",
          green: "#73b98d",
          yellow: "#d4ad58",
          blue: "#78a9d4",
          magenta: "#b18bd0",
          cyan: "#69b5bd",
          white: "#d9d9d2",
          brightBlack: "#777970",
          brightRed: "#f17b71",
          brightGreen: "#8ed0a4",
          brightYellow: "#e8c572",
          brightBlue: "#94c0e5",
          brightMagenta: "#c9a4e3",
          brightCyan: "#83cbd2",
          brightWhite: "#ffffff",
        };
  }

  /** Re-aplica la paleta a todos los terminales vivos. */
  function applyThemeToTerms() {
    const theme = termTheme();
    for (const box of boxes.values()) box.term.options.theme = theme;
  }

  function makeTerm(key: string): { term: Terminal; fit: FitAddon } {
    const term = new Terminal({
      cursorBlink: true,
      fontSize: 12,
      fontFamily: "Cascadia Mono, SFMono-Regular, Menlo, Consolas, monospace",
      fontWeight: "400",
      fontWeightBold: "700",
      lineHeight: 1.12,
      minimumContrastRatio: 4.5,
      drawBoldTextInBrightColors: true,
      theme: termTheme(),
      allowProposedApi: false,
      rightClickSelectsWord: false,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.onData((data) => {
      if (consumeTerminalControlData(key, data)) return;
      const id = sessionOf(key);
      if (!id) return;
      void consoleWrite(id, data).catch(() => {});
    });
    // Ctrl/Cmd+V y Ctrl/Cmd+C (con selección): clipboard API explícita.
    term.attachCustomKeyEventHandler((ev) => {
      if (ev.type !== "keydown") return true;
      if (consumeWorkspaceShortcut(ev, "xterm")) return false;
      const mod = ev.ctrlKey || ev.metaKey;
      if (mod && (ev.key === "v" || ev.key === "V")) {
        void pasteInto(key);
        return false;
      }
      if (mod && (ev.key === "c" || ev.key === "C") && term.hasSelection()) {
        void copyFrom(key);
        return false;
      }
      return true;
    });
    return { term, fit };
  }

  /**
   * Crea el xterm de una pestaña cuando su contenedor entra al DOM.
   *
   * `untrack` porque un `{@attach}` que lee estado reactivo se vuelve a montar
   * cuando ese estado cambia, y acá remontar significa destruir el terminal y
   * perder el scrollback cada vez que se abre o cierra otra pestaña.
   */
  function mountTerm(key: string) {
    return (el: HTMLElement) =>
      untrack(() => {
        const { term, fit } = makeTerm(key);
        term.open(el);
        boxes.set(key, { term, fit, el });
        const id = sessionOf(key);
        if (id) flushOutput(id, key);
        const observer = new ResizeObserver(() => {
          if (!visiblePaneKeys.includes(key)) return;
          fitAndResize(key);
        });
        observer.observe(el);
        requestAnimationFrame(() => {
          fitAndResize(key);
          term.refresh(0, Math.max(0, term.rows - 1));
        });
        return () => {
          observer.disconnect();
          term.dispose();
          boxes.delete(key);
        };
      });
  }

  async function copyFrom(key = activeKey) {
    const text = termOf(key)?.getSelection() ?? "";
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      /* sin permiso */
    }
  }

  async function pasteInto(key = activeKey) {
    const term = termOf(key);
    if (!term || !sessionOf(key)) return;
    try {
      const text = await navigator.clipboard.readText();
      if (text) term.paste(text);
    } catch {
      /* sin permiso / vacío */
    }
  }

  const IMAGE_EXT = /\.(png|jpe?g|gif|webp)$/i;
  let lastPasted = { text: "", at: 0 };
  let oleWatch: number | null = null;

  function panelIsLive(): boolean {
    const el = consoleEl;
    if (!el) return false;
    if (el.closest("[inert]")) return false;
    if (el.closest(".is-hidden")) return false;
    return true;
  }

  function quotePtyPath(path: string): string {
    const trimmed = path.trim();
    if (!trimmed) return trimmed;
    if (!/[\s"<>|&^()]/.test(trimmed)) return trimmed;
    return `"${trimmed.replace(/"/g, '\\"')}"`;
  }

  function isAticDragTextPath(path: string): boolean {
    const name = path.split(/[/\\]/).pop() ?? "";
    return name.startsWith(".atic-drag-") && name.endsWith(".txt");
  }

  function termKeyHit(x: number, y: number): string | null {
    if (!panelIsLive()) return null;
    for (const key of visiblePaneKeys) {
      const box = boxes.get(key);
      if (!box || !sessionOf(key)) continue;
      const r = box.el.getBoundingClientRect();
      if (x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) return key;
    }
    return null;
  }

  function termKeyAt(x: number, y: number): string | null {
    return (
      termKeyHit(x, y) ??
      (sessionOf(activeKey) && visiblePaneKeys.includes(activeKey) ? activeKey : null) ??
      visiblePaneKeys.find((key) => sessionOf(key)) ??
      null
    );
  }

  function pasteIntoTerm(key: string, text: string): boolean {
    const trimmed = text;
    if (!trimmed) return false;
    const term = termOf(key);
    if (!term || !sessionOf(key)) return false;
    const now = Date.now();
    if (trimmed === lastPasted.text && now - lastPasted.at < 450) return true;
    lastPasted = { text: trimmed, at: now };
    activeKey = key;
    requestOverlayKeyboard(key);
    term.paste(trimmed);
    return true;
  }

  async function stageBlobPath(file: Blob, mimeHint?: string): Promise<string | null> {
    const mime = (mimeHint || file.type || "image/png").toLowerCase();
    if (!mime.startsWith("image/")) return null;
    const buf = new Uint8Array(await file.arrayBuffer());
    let binary = "";
    const chunk = 0x8000;
    for (let i = 0; i < buf.length; i += chunk) {
      binary += String.fromCharCode(...buf.subarray(i, i + chunk));
    }
    return agentStageImage(btoa(binary), mime);
  }

  async function applyClipboardInsert(payload: AgentsComposerInsert) {
    if (!panelIsLive()) return;
    const x = payload.x;
    const y = payload.y;
    const key =
      typeof x === "number" && typeof y === "number"
        ? (termKeyHit(x, y) ??
            (pointInEl(consoleEl, x, y) || pointInEl(bodyEl, x, y)
              ? termKeyAt(-1, -1)
              : null))
        : termKeyAt(-1, -1);
    if (!key) return;
    if (payload.kind === "image" && payload.imagePath) {
      pasteIntoTerm(key, quotePtyPath(payload.imagePath));
      return;
    }
    if (payload.text) pasteIntoTerm(key, payload.text);
  }

  function clipDragTypes(dt: DataTransfer | null): boolean {
    if (!dt) return false;
    const types = [...dt.types];
    return (
      types.includes("Files") ||
      types.includes("text/uri-list") ||
      types.includes("text/plain")
    );
  }

  function onClipDragOver(e: DragEvent) {
    if (tabDrag || !panelIsLive() || !clipDragTypes(e.dataTransfer)) return;
    e.preventDefault();
    if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";
    clipDropKey = termKeyAt(e.clientX, e.clientY);
  }

  function onClipDragLeave(e: DragEvent) {
    const next = e.relatedTarget as Node | null;
    if (next && (e.currentTarget as HTMLElement).contains(next)) return;
    clipDropKey = null;
  }

  function filePathOf(file: File): string | null {
    const path = (file as File & { path?: string }).path;
    return path?.trim() ? path : null;
  }

  function localFromUri(line: string): string | null {
    const t = line.trim();
    if (!t || t.startsWith("#")) return null;
    if (!t.startsWith("file:")) return t;
    let local = decodeURIComponent(t.replace(/^file:\/\//, ""));
    if (/^\/[A-Za-z]:/.test(local)) local = local.slice(1);
    return local || null;
  }

  async function insertDroppedPath(key: string, path: string): Promise<boolean> {
    if (isAticDragTextPath(path)) {
      const text = await readClipboardDragText(path);
      if (!text?.trim()) return false;
      return pasteIntoTerm(key, text);
    }
    return pasteIntoTerm(key, quotePtyPath(path));
  }

  async function onClipDrop(e: DragEvent) {
    clipDropKey = null;
    if (tabDrag || !panelIsLive()) return;
    e.preventDefault();
    e.stopPropagation();
    const dt = e.dataTransfer;
    if (!dt) return;
    const key = termKeyAt(e.clientX, e.clientY);
    if (!key) return;
    try {
      const files = [...(dt.files ?? [])];
      let added = false;
      for (const file of files) {
        const path = filePathOf(file);
        if (path) {
          added = (await insertDroppedPath(key, path)) || added;
          continue;
        }
        if (file.type.startsWith("image/") || IMAGE_EXT.test(file.name)) {
          const staged = await stageBlobPath(file, file.type);
          if (staged) added = pasteIntoTerm(key, quotePtyPath(staged)) || added;
        }
      }
      if (!added) {
        const uri = dt.getData("text/uri-list") || "";
        for (const line of uri.split(/\r?\n/)) {
          const local = localFromUri(line);
          if (!local) continue;
          added = (await insertDroppedPath(key, local)) || added;
        }
      }
      if (!added) {
        const text = dt.getData("text/plain") || dt.getData("text");
        if (text?.trim()) pasteIntoTerm(key, text);
      }
    } catch (err) {
      error = String(err);
    }
  }

  function stopOleWatch() {
    if (oleWatch != null) {
      clearInterval(oleWatch);
      oleWatch = null;
    }
    clipDropKey = null;
  }

  function pointInEl(el: HTMLElement | null, x: number, y: number): boolean {
    if (!el) return false;
    const r = el.getBoundingClientRect();
    return x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
  }

  function startOleWatch() {
    if (oleWatch != null) return;
    oleWatch = window.setInterval(() => {
      if (!panelIsLive()) {
        clipDropKey = null;
        return;
      }
      void overlayCursor()
        .then((pt) => {
          if (!pt) {
            clipDropKey = null;
            return;
          }
          clipDropKey =
            termKeyHit(pt.x, pt.y) ??
            (pointInEl(bodyEl, pt.x, pt.y) ? termKeyAt(-1, -1) : null);
        })
        .catch(() => {
          clipDropKey = null;
        });
    }, 50);
  }

  function onClipboardOle(e: Event) {
    const active = (e as CustomEvent<ClipboardOleDetail>).detail?.active;
    if (active) startOleWatch();
    else stopOleWatch();
  }

  function onTermContextMenu(key: string, e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    focusTerm(key);
    ctxMenu = { x: e.clientX, y: e.clientY, key };
  }

  function onTermPointerDown(key: string) {
    closeCtx();
    activeKey = key;
    error = null;
    // OverlaySurface (capture) ya pidió text-mode; reforzar foco tras el await.
    // Los reintentos re-piden el modo texto SIEMPRE: `force_foreground` corre
    // en un hilo aparte y devuelve antes de agarrar el primer plano, y
    // `document.hasFocus()` contesta true aunque el HWND no sea el primero,
    // así que no hay señal fiable de que el primer pedido haya prendido.
    requestAnimationFrame(() => {
      focusTerm(key);
      setTimeout(() => requestOverlayKeyboard(key), 40);
      setTimeout(() => requestOverlayKeyboard(key), 120);
    });
  }

  onMount(() => {
    const savedRailWidth = Number(localStorage.getItem(RAIL_STORAGE_KEY));
    if (Number.isFinite(savedRailWidth) && savedRailWidth > 0) {
      railWidth = clampRailWidth(savedRailWidth);
    }
    const savedZoom = Number(localStorage.getItem(FONT_ZOOM_KEY));
    if (Number.isFinite(savedZoom) && savedZoom !== 0) {
      fontZoom = clampFontZoom(savedZoom);
    }
    try {
      const saved = JSON.parse(localStorage.getItem(SAVED_CMDS_KEY) ?? "[]") as unknown;
      if (Array.isArray(saved)) {
        savedCmds = saved.filter((c): c is string => typeof c === "string");
      }
    } catch {
      /* lista vacía */
    }
    void loadSshHosts();
    void agentsAlwaysOnTop()
      .then((on) => (pinned = on))
      .catch(() => (pinned = false));
    window.addEventListener("keydown", onGlobalKey, true);
    window.addEventListener(CLIPBOARD_OLE_EVENT, onClipboardOle);
    // Lanzador: N pestañas de agentes. Sin semilla: la pestaña de siempre.
    const seeds = (initialTabs ?? []).slice(0, MAX_TABS);
    if (seeds.length > 0) {
      for (const seed of seeds) {
        newTab(seed.kind === "ssh" ? "ssh" : "local", seed);
      }
      const firstKey = tabs[0]?.key;
      if (firstKey) {
        activeKey = firstKey;
        paneTree = leaf(firstKey);
      }
    } else {
      newTab(initialKind === "ssh" ? "ssh" : "local");
    }

    void Promise.all([
      onAgentsWorkspaceShortcut(consumeNativeWorkspaceShortcut),
      onConsoleOutput((p) => {
        pushOutput(p.session, p.data);
      }),
      onConsoleExit((p) => {
        const t = tabForSession(p.session);
        if (!t) return;
        const key = t.key;
        setSession(key, null);
        const code = p.code == null ? "?" : String(p.code);
        termOf(key)?.writeln(`\r\n[sesión terminada · exit ${code}]`);
      }),
      onAgentsComposerInsert((payload) => void applyClipboardInsert(payload)),
    ]).then((uns) => {
      stopListen = () => {
        for (const u of uns) u();
      };
      resolveListen();
    }).catch(() => {
      resolveListen();
    });

    const onDocPointer = (e: PointerEvent) => {
      // Capture: stopPropagation del menú no alcanza; hay que excluir el .ctx acá.
      if (e.target instanceof Node) {
        const menu = document.querySelector(".console .ctx");
        if (menu?.contains(e.target)) return;
        if ((e.target as HTMLElement).closest?.(".more-menu")) return;
        if ((e.target as HTMLElement).closest?.(".add-menu")) return;
      }
      closeCtx();
      shortcutsOpen = false;
      moreOpen = false;
      addMenuOpen = false;
    };
    document.addEventListener("pointerdown", onDocPointer, true);
    window.addEventListener("blur", closeCtx);

    // Tema en vivo: al cambiar `data-theme` en :root, repintar terminales.
    const themeObs = new MutationObserver(() => applyThemeToTerms());
    themeObs.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["data-theme"],
    });

    return () => {
      window.removeEventListener("keydown", onGlobalKey, true);
      window.removeEventListener(CLIPBOARD_OLE_EVENT, onClipboardOle);
      document.removeEventListener("pointerdown", onDocPointer, true);
      window.removeEventListener("blur", closeCtx);
      stopOleWatch();
      themeObs.disconnect();
    };
  });

  onDestroy(() => {
    stopListen?.();
    for (const timer of bootTimers.values()) window.clearTimeout(timer);
    bootTimers.clear();
    const ids = knownSessionIds();
    for (const id of ids) void consoleClose(id).catch(() => {});
    void disconnectAll();
    // Los xterm los dispone el teardown de cada `{@attach}`.
  });
</script>

<!-- Captura: la rueda con Ctrl es zoom aunque caiga sobre el scroll del xterm. -->
<section
  class="console console-desk"
  bind:this={consoleEl}
  aria-label="Consola"
  onwheelcapture={onConsoleWheel}
>
  <aside
    class="rail"
    class:is-compact={railCompact}
    style={`--rail-width: ${railWidth}px`}
    onpointerdown={(e) => {
      // Zona muerta del rail también arrastra el float.
      if (
        (e.target as HTMLElement).closest("button, select, label, [data-rail-resizer]")
      )
        return;
      if (!onBarPointerDown) return;
      e.preventDefault();
      onBarPointerDown(e);
    }}
  >
    <div class="rail-tabs" role="group" aria-label="Consolas abiertas">
      {#each tabs as t, i (t.key)}
        {@const railGroup = railGroups.find((g) => g.anchorKey === t.key)}
        {#if railGroup}
          <!-- El grupo entero es UNA ficha: clicarla restaura la división. -->
          <span class="rail-slot">
            <button
              type="button"
              class="rail-tab is-group"
              class:is-on={railGroup.keys.includes(activeKey)}
              aria-current={railGroup.keys.includes(activeKey) ? "true" : undefined}
              use:tip={railGroup.label}
              onclick={() => switchToGroup(railGroup)}
              oncontextmenu={(e) => {
                e.preventDefault();
                e.stopPropagation();
                ctxMenu = {
                  x: e.clientX,
                  y: e.clientY,
                  key: railGroup.anchorKey,
                  group: true,
                };
              }}
            >
              <span class="rail-logo is-stack">
                {#each railGroup.tabs.slice(0, 2) as gt (gt.key)}
                  <AgentLogo agent={gt.command} size={14} />
                {/each}
              </span>
              <span class="rail-copy">
                <span class="rail-name">{railGroup.label}</span>
                <span class="rail-status">{railGroup.tabs.length} consolas</span>
              </span>
              {#if railGroup.tabs.some((gt) => gt.sessionId)}
                <span class="live" use:tip={"Sesión activa"} aria-hidden="true"></span>
              {/if}
            </button>
            <button
              type="button"
              class="tab-x"
              aria-label="Cerrar grupo {railGroup.label}"
              use:tip={"Cerrar el grupo y todas sus consolas"}
              onclick={() => void closeGroup(railGroup)}
            >
              <Icon icon={X} size={9} />
            </button>
          </span>
        {:else if !railGroups.some((g) => g.keys.includes(t.key))}
          <span class="rail-slot">
            <button
              type="button"
              class="rail-tab"
              class:is-on={t.key === activeKey}
              class:is-dragging={tabDrag?.key === t.key}
              aria-current={t.key === activeKey ? "true" : undefined}
              use:tip={tabLabels[i]}
              onpointerdown={(e) => beginTabDrag(t.key, e)}
              onclick={() => {
                if (dragConsumedClick) {
                  dragConsumedClick = false;
                  return;
                }
                switchTab(t.key);
              }}
            >
              <span class="rail-logo"><AgentLogo agent={t.command} size={18} /></span>
              <span class="rail-copy">
                <span class="rail-name">{tabLabels[i]}</span>
                <span class="rail-status">
                  {t.sessionId
                    ? "Activa"
                    : connecting && t.key === activeKey
                      ? "Preparando"
                      : "Pausada"}
                </span>
              </span>
              {#if t.sessionId}
                <span class="live" use:tip={"Sesión activa"} aria-hidden="true"></span>
              {/if}
            </button>
            <button
              type="button"
              class="tab-x"
              aria-label="Cerrar {tabLabels[i]}"
              use:tip={"Cerrar pestaña"}
              onclick={() => void closeTab(t.key)}
            >
              <Icon icon={X} size={9} />
            </button>
          </span>
        {/if}
      {/each}
    </div>
    <div class="rail-add">
      <div class="add-menu">
        <button
          type="button"
          class="tab-add"
          aria-label="Nueva consola o agente"
          aria-haspopup="menu"
          aria-expanded={addMenuOpen}
          use:tip={"Nueva consola o agente"}
          disabled={connecting || !canAddTab}
          onclick={toggleAddMenu}
        >
          <Icon icon={Plus} size={12} />
        </button>
        {#if addMenuOpen}
          <div
            class="add-pop"
            role="menu"
            aria-label="Abrir nueva consola"
            tabindex="-1"
            onpointerdown={(e) => e.stopPropagation()}
          >
            {#if onPickFolder}
              <p class="add-group" aria-hidden="true">Se abre en</p>
              <button
                type="button"
                class="add-item is-folder"
                role="menuitem"
                use:tip={startFolder ?? "Carpeta de inicio del usuario"}
                onclick={() => void pickStartFolder(true)}
              >
                <span class="add-glyph"><Icon icon={Folder} size={13} /></span>
                <span class="add-ellipsis">{folderName(startFolder)}</span>
                <span class="add-chevron" aria-hidden="true">›</span>
              </button>
            {/if}
            <p class="add-group" aria-hidden="true">Agentes</p>
            {#each AGENTS as agent (agent.cli)}
              {#if agentOnPath[agent.cli] === false}
                <button
                  type="button"
                  class="add-item"
                  role="menuitem"
                  use:tip={`${agent.name} no está instalado. Abre una consola y ejecuta el instalador oficial.`}
                  onclick={() => installAgent(agent)}
                >
                  <span class="add-glyph"><AgentLogo agent={agent.cli} size={14} /></span>
                  <span class="add-ellipsis">{agent.name}</span>
                  <span class="add-install">Instalar</span>
                </button>
              {:else}
                <button
                  type="button"
                  class="add-item"
                  role="menuitem"
                  use:tip={`Abrir ${agent.name}`}
                  onclick={() =>
                    addFromMenu({
                      kind: "local",
                      label: agent.name,
                      command: agent.cli,
                    })
                  }
                >
                  <span class="add-glyph"><AgentLogo agent={agent.cli} size={14} /></span>
                  {agent.name}
                </button>
              {/if}
            {/each}
            <button
              type="button"
              class="add-item"
              role="menuitem"
              onclick={() => addFromMenu({ kind: "local" })}
            >
              <span class="add-glyph"><Icon icon={SquareTerminal} size={13} /></span>
              Consola local
            </button>
            <button
              type="button"
              class="add-item"
              role="menuitem"
              onclick={() => addFromMenu({ kind: "ssh" })}
            >
              <span class="add-glyph is-ssh-glyph">SSH</span>
              Consola SSH
            </button>
            <button
              type="button"
              class="add-item"
              role="menuitem"
              aria-expanded={cmdPromptOpen}
              onclick={() => (cmdPromptOpen = !cmdPromptOpen)}
            >
              <span class="add-glyph is-ssh-glyph">›_</span>
              Comando…
            </button>
            {#if cmdPromptOpen}
              <input
                class="add-cmd"
                type="text"
                placeholder="ssh root@1.2.3.4 · dashboard"
                aria-label="Comando a ejecutar en una consola nueva"
                bind:value={cmdText}
                {@attach (el) => {
                  void setOverlayTextMode(true).catch(() => {});
                  el.focus();
                }}
                onkeydown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    runCmdPrompt();
                  } else if (e.key === "Escape") {
                    e.stopPropagation();
                    cmdPromptOpen = false;
                  }
                }}
              />
            {/if}
            {#if savedCmds.length > 0}
              <p class="add-group" aria-hidden="true">Guardados</p>
              {#each savedCmds as cmd (cmd)}
                <span class="add-saved">
                  <button
                    type="button"
                    class="add-item"
                    role="menuitem"
                    use:tip={`Abrir consola con «${cmd}»`}
                    onclick={() =>
                      addFromMenu({ kind: "local", label: cmd, command: cmd })}
                  >
                    <span class="add-glyph"
                      ><Icon icon={SquareTerminal} size={13} /></span
                    >
                    <span class="add-ellipsis">{cmd}</span>
                  </button>
                  <button
                    type="button"
                    class="add-forget"
                    aria-label={`Olvidar «${cmd}»`}
                    use:tip={"Olvidar comando"}
                    onclick={() => removeSavedCmd(cmd)}
                  >
                    <Icon icon={X} size={9} />
                  </button>
                </span>
              {/each}
            {/if}
            {#if sshAliases.length > 0}
              <p class="add-group" aria-hidden="true">~/.ssh/config</p>
              {#each sshAliases as alias (alias)}
                <button
                  type="button"
                  class="add-item"
                  role="menuitem"
                  use:tip={`ssh ${alias}`}
                  onclick={() =>
                    addFromMenu({
                      kind: "local",
                      label: alias,
                      command: `ssh ${alias}`,
                    })}
                >
                  <span class="add-glyph is-ssh-glyph">SSH</span>
                  <span class="add-ellipsis">{alias}</span>
                </button>
              {/each}
            {/if}
          </div>
        {/if}
      </div>
    </div>
    <button
      type="button"
      class="rail-resizer"
      aria-label={`Cambiar ancho de la barra lateral, ${Math.round(railWidth)} píxeles`}
      use:tip={"Arrastra para cambiar el ancho · Doble clic para contraer"}
      data-rail-resizer
      onpointerdown={startRailResize}
      onkeydown={onRailResizeKey}
      ondblclick={() => setRailWidth(railCompact ? RAIL_DEFAULT : RAIL_MIN)}
    ></button>
  </aside>

  <div class="col">
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <header
      class="bar"
      onpointerdown={(e) => {
        // Los controles quedan excluidos acá; el resto de la barra arrastra.
        if ((e.target as HTMLElement).closest("button, select, label")) return;
        if (!onBarPointerDown) return;
        e.preventDefault();
        onBarPointerDown(e);
      }}
    >
      <div class="bar-start">
        {#if onBack}
          <button
            type="button"
            class="back-btn"
            aria-label="Volver al lanzador"
            use:tip={"Volver al lanzador"}
            onclick={onBack}
          >
            <Icon icon={ArrowLeft} size={13} />
            <span>Agentes</span>
          </button>
        {/if}
        {#if active?.command}
          <span class="active-agent-logo">
            <AgentLogo agent={active.command} size={18} />
            <span
              class="live session-dot"
              class:is-live={!!active?.sessionId}
              class:is-prep={connecting && !active?.sessionId}
              role="status"
              use:tip={active?.sessionId
                ? t("page.agents.runningAria")
                : connecting
                  ? t("page.agents.preparingAria")
                  : t("page.agents.offlineAria")}
              aria-label={active?.sessionId
                ? t("page.agents.runningAria")
                : connecting
                  ? t("page.agents.preparingAria")
                  : t("page.agents.offlineAria")}
            ></span>
          </span>
        {/if}
        <div class="where-block">
          <p class="where" use:tip={active ? tabLabels[tabs.indexOf(active)] : ""}>
            {active ? tabLabels[tabs.indexOf(active)] : "Sin consolas"}
          </p>
        </div>
        {#if onPickFolder}
          <button
            type="button"
            class="folder-chip"
            use:tip={`Carpeta de inicio: ${startFolder ?? "carpeta del usuario"}. Las consolas nuevas se abren acá y las shells vivas se mudan; los agentes se quedan.`}
            aria-label={`Cambiar carpeta de inicio. Actual: ${startFolder ?? "carpeta del usuario"}`}
            onclick={() => void pickStartFolder()}
          >
            <Icon icon={Folder} size={12} />
            <span>{folderName(startFolder)}</span>
          </button>
        {/if}
      </div>
      <div class="window-actions">
        {#if active?.kind === "ssh"}
          <label class="host-pick">
            <span class="sr">Host SSH</span>
            <select
              class="host-select"
              aria-label="Host SSH"
              disabled={connecting || !!active.sessionId || sshHosts.length === 0}
              value={active.hostId ?? ""}
              onchange={(e) => {
                const v = (e.currentTarget as HTMLSelectElement).value;
                if (active) active.hostId = v || null;
                error = null;
              }}
            >
              {#if sshHosts.length === 0}
                <option value="">Sin hosts</option>
              {:else}
                {#each sshHosts as h (h.id)}
                  <option value={h.id}>{hostOptionLabel(h)}</option>
                {/each}
              {/if}
            </select>
          </label>
        {/if}
        {#if active}
          <div class="more-menu">
            <button
              type="button"
              class="icon-btn"
              aria-label={t("page.agents.moreAria")}
              aria-haspopup="menu"
              aria-expanded={moreOpen}
              use:tip={t("page.agents.more")}
              onclick={() => {
                moreOpen = !moreOpen;
                shortcutsOpen = false;
              }}
            >
              <Icon icon={EllipsisVertical} size={13} />
            </button>
            {#if moreOpen}
              <div class="more-pop" role="menu" aria-label={t("page.agents.moreAria")}>
                <button
                  type="button"
                  class="more-item"
                  role="menuitem"
                  onclick={() => {
                    shortcutsOpen = true;
                    moreOpen = false;
                  }}
                >
                  <Icon icon={Keyboard} size={13} />
                  {t("page.agents.shortcuts")}
                </button>
                {#if usageAgent}
                  <button
                    type="button"
                    class="more-item"
                    role="menuitem"
                    onclick={() => {
                      usageOpen = true;
                      moreOpen = false;
                    }}
                  >
                    <Icon icon={Activity} size={13} />
                    {t("page.agents.usage")}
                  </button>
                {/if}
                {#if !connected}
                  <button
                    type="button"
                    class="more-item"
                    role="menuitem"
                    disabled={connecting || (active.kind === "ssh" && !activeHost)}
                    onclick={() => {
                      moreOpen = false;
                      void connect();
                    }}
                  >
                    <Icon icon={SquareTerminal} size={13} />
                    {connecting
                      ? t("page.agents.preparingAria")
                      : active.kind === "ssh"
                        ? t("page.agents.connect")
                        : t("page.agents.reconnect")}
                  </button>
                {/if}
              </div>
            {/if}
            {#if shortcutsOpen}
              <div
                class="shortcuts-pop"
                role="dialog"
                aria-label={t("page.agents.shortcutsTitle")}
              >
                <p class="shortcuts-title">{t("page.agents.shortcutsTitle")}</p>
                <p class="shortcuts-hint">{t("page.agents.shortcutsHint")}</p>
                <ul class="shortcuts-list">
                  {#each CONSOLE_SHORTCUTS as item (item.keys)}
                    <li>
                      <span>{t(item.labelKey)}</span>
                      <kbd>{item.keys}</kbd>
                    </li>
                  {/each}
                </ul>
              </div>
            {/if}
          </div>
        {/if}
        {#if onToggleMinimize}
          <button
            type="button"
            class="icon-btn"
            aria-label={minimized ? "Restaurar ventana" : "Minimizar a la barra"}
            aria-pressed={minimized}
            use:tip={minimized ? "Restaurar ventana" : "Minimizar a la barra"}
            onclick={onToggleMinimize}
          >
            <Icon icon={Minus} size={12} />
          </button>
        {/if}
        {#if onToggleMaximize}
          <button
            type="button"
            class="icon-btn"
            class:is-on={maximized}
            aria-label={maximized ? "Restaurar tamaño" : "Agrandar al monitor"}
            aria-pressed={maximized}
            use:tip={maximized ? "Restaurar tamaño" : "Agrandar al monitor"}
            onclick={onToggleMaximize}
          >
            <Icon icon={Square} size={11} />
          </button>
        {/if}
        <button
          type="button"
          class="icon-btn pin-btn"
          class:is-on={pinned}
          aria-label={pinned ? "Desfijar ventana" : "Fijar ventana arriba"}
          aria-pressed={pinned}
          use:tip={pinned ? "Desfijar ventana" : "Fijar ventana arriba"}
          onclick={() => {
            const next = !pinned;
            pinned = next;
            void setAgentsAlwaysOnTop(next).catch(() => (pinned = !next));
          }}
        >
          <Icon icon={Pin} size={12} />
        </button>
        {#if onClose}
          <button
            type="button"
            class="icon-btn"
            aria-label="Esconder ventana"
            use:tip={"Esconder ventana. Las consolas siguen corriendo."}
            onclick={() => onClose()}
          >
            <Icon icon={X} size={12} />
          </button>
        {/if}
      </div>
    </header>

    {#if error}
      <p class="err" role="alert">
        <span class="err-text">{error}</span>
        <button
          type="button"
          class="err-x"
          aria-label="Descartar aviso"
          use:tip={"Descartar aviso"}
          onclick={() => (error = null)}
        >
          <Icon icon={X} size={11} />
        </button>
      </p>
    {/if}

    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="body"
      class:is-split={paneMode}
      bind:this={bodyEl}
      ondragover={onClipDragOver}
      ondragleave={onClipDragLeave}
      ondrop={(e) => void onClipDrop(e)}
    >
      {#if tabs.length === 0 && !paneMode}
        <div class="empty">
          <EmptyState
            compact
            title="Sin consolas"
            hint="Abre una consola local (PowerShell) o una sesión SSH."
          >
            {#snippet action()}
              <button type="button" class="chip is-go" onclick={() => newTab("local")}>
                <Icon icon={SquareTerminal} size={12} />
                Nueva consola
              </button>
            {/snippet}
          </EmptyState>
        </div>
      {:else if !paneMode && active?.kind === "ssh" && !activeHost && !connected}
        <div class="empty">
          <EmptyState
            compact
            title="Sin host remoto"
            hint="Agrega un host en Ajustes → Agentes y vuelve a abrir la consola."
          />
        </div>
      {:else if !paneMode && !connected && !connecting}
        <div class="empty">
          <EmptyState
            compact
            title={active?.kind === "local"
              ? "Consola local"
              : `SSH · ${sshLabel ?? "remoto"}`}
            hint={active?.kind === "local"
              ? "PowerShell en este equipo (fallback cmd)."
              : "Abre ssh -t al host seleccionado."}
          >
            {#snippet action()}
              <button type="button" class="chip is-go" onclick={() => void connect()}>
                <Icon icon={SquareTerminal} size={12} />
                Conectar
              </button>
            {/snippet}
          </EmptyState>
        </div>
      {/if}

      <!-- `tab`, no `t`: el nombre corto sombreaba la función i18n `t` y el
           overlay de arranque reventaba el render de todos los paneles. -->
      {#each tabs as tab (tab.key)}
        {@const paneRect = paneRects.find((pane) => pane.key === tab.key)}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="term"
          class:is-active={tab.key === activeKey}
          class:is-hidden={!paneRect}
          class:is-drop={
            clipDropKey === tab.key ||
            (!!tabDrag && tabDrag.key !== tab.key && dropHint?.key === tab.key)
          }
          class:drop-right={!clipDropKey && dropHint?.zone === "right"}
          class:drop-down={!clipDropKey && dropHint?.zone === "down"}
          class:is-join-left={!!paneRect && paneRect.x > 0.01}
          class:is-join-top={!!paneRect && paneRect.y > 0.01}
          style={paneRect
            ? `--pane-x: ${paneRect.x}%; --pane-y: ${paneRect.y}%; --pane-width: ${paneRect.width}%; --pane-height: ${paneRect.height}%`
            : undefined}
          data-no-drag
          data-selectable
          data-console-term
          onpointerdown={() => onTermPointerDown(tab.key)}
          oncontextmenu={(e) => onTermContextMenu(tab.key, e)}
          ondragover={onClipDragOver}
          ondrop={(e) => void onClipDrop(e)}
        >
          <div class="term-host" {@attach mountTerm(tab.key)}></div>
          {#if paneLoading(tab.key)}
            {@const tabName = tab.label || tabLabels[tabs.indexOf(tab)] || ""}
            <!-- Salida suave: el primer output del CLI aparece debajo mientras
                 el velo se disuelve, en vez de un corte seco. -->
            <div class="term-boot" role="status" out:fade={{ duration: ms(MOTION.fast) }}>
              {#if tab.command}
                <span class="term-boot-logo">
                  <AgentLogo agent={tab.command} size={28} />
                </span>
              {/if}
              <p class="term-boot-title">
                {tabName
                  ? t("page.agents.booting", { name: tabName })
                  : t("page.agents.bootingGeneric")}
              </p>
              <p class="term-boot-hint">{t("page.agents.bootingHint")}</p>
              <span class="term-boot-spin" aria-hidden="true"></span>
            </div>
          {/if}
        </div>
      {/each}

      <!-- Costuras de los splits: arrastrar reparte el espacio; doble clic, 50/50. -->
      {#each paneDividers as divider (divider.path)}
        <div
          class="pane-divider"
          class:is-vertical={divider.direction === "right"}
          role="separator"
          aria-orientation={divider.direction === "right" ? "vertical" : "horizontal"}
          use:tip={"Arrastra para repartir el espacio · doble clic: mitades"}
          style={divider.direction === "right"
            ? `left: ${divider.seam}%; top: ${divider.y}%; height: ${divider.height}%;`
            : `top: ${divider.seam}%; left: ${divider.x}%; width: ${divider.width}%;`}
          data-no-drag
          onpointerdown={(e) => startDividerDrag(divider, e)}
          ondblclick={() => resetDividerRatio(divider.path)}
        ></div>
      {/each}

      {#if ctxMenu}
        <div
          class="ctx"
          style:left="{ctxMenu.x}px"
          style:top="{ctxMenu.y}px"
          role="menu"
          tabindex="-1"
          data-no-drag
          onpointerdown={(e) => e.stopPropagation()}
        >
          {#if ctxMenu.group}
            {@const entry = railGroups.find((g) => g.anchorKey === ctxMenu!.key)}
            <button
              type="button"
              class="ctx-item"
              role="menuitem"
              disabled={!entry}
              onclick={() => {
                closeCtx();
                if (entry) detachGroup(entry);
              }}
            >
              Separar grupo
            </button>
            <button
              type="button"
              class="ctx-item"
              role="menuitem"
              disabled={!entry}
              onclick={() => {
                closeCtx();
                if (entry) void closeGroup(entry);
              }}
            >
              Cerrar grupo
            </button>
          {:else}
            <button
              type="button"
              class="ctx-item"
              role="menuitem"
              disabled={!termOf(ctxMenu.key)?.hasSelection()}
              onclick={() => {
                const k = ctxMenu!.key;
                closeCtx();
                void copyFrom(k);
              }}
            >
              Copiar
            </button>
            <button
              type="button"
              class="ctx-item"
              role="menuitem"
              disabled={!sessionOf(ctxMenu.key)}
              onclick={() => {
                const k = ctxMenu!.key;
                closeCtx();
                void pasteInto(k);
              }}
            >
              Pegar
            </button>
            {#if visiblePaneKeys.length > 1 && visiblePaneKeys.includes(ctxMenu.key)}
              <button
                type="button"
                class="ctx-item"
                role="menuitem"
                onclick={() => {
                  const k = ctxMenu!.key;
                  closeCtx();
                  removeFromGroup(k);
                }}
              >
                Sacar del grupo
              </button>
            {/if}
            <button
              type="button"
              class="ctx-item"
              role="menuitem"
              onclick={() => {
                const k = ctxMenu!.key;
                closeCtx();
                void closeTab(k);
              }}
            >
              Cerrar consola
            </button>
          {/if}
        </div>
      {/if}
    </div>
  </div>

  {#if tabDrag}
    {@const dragTab = tabOf(tabDrag.key)}
    <div
      class="tab-ghost"
      style:left="{tabDrag.x}px"
      style:top="{tabDrag.y}px"
      aria-hidden="true"
    >
      <AgentLogo agent={dragTab?.command ?? null} size={16} />
    </div>
  {/if}

  {#if usageOpen && usageAgent}
    {#key usageAgent}
      <AccountUsageModal
        agent={usageAgent}
        onClose={() => (usageOpen = false)}
        onRunUsageCommand={sessionId
          ? () => {
              const id = sessionId;
              void consoleWrite(id, "/usage\r").catch(() => {});
              requestOverlayKeyboard();
            }
          : undefined}
      />
    {/key}
  {/if}
</section>

<style>
  .console {
    position: relative;
    display: flex;
    flex: 1;
    min-width: 0;
    flex-direction: row;
    min-height: 0;
    background: transparent;

    /* Overlay: user-select/touch-action none; xterm necesita interactuar. */
    user-select: text;
    -webkit-user-select: text;
    touch-action: auto;
  }

  /* ─── Rail izquierdo: una ficha por consola ───────────────────────────── */
  .rail {
    position: relative;
    z-index: 2;
    display: flex;
    flex-shrink: 0;
    flex-direction: column;
    align-items: center;
    width: var(--rail-width, 2.9rem);
    padding: 0.35rem 0.25rem;
    border-right: 0;
  }

  .rail-tabs {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.3rem;
    width: 100%;
    min-height: 0;
    overflow-y: auto;
    scrollbar-width: none;
  }

  .rail-slot {
    position: relative;
    display: inline-flex;
    flex-shrink: 0;
  }

  .rail-tab {
    position: relative;
    display: grid;
    place-items: center;
    width: 2.4rem;
    height: 2.4rem;
    border: 0;
    border-radius: 0.6rem;
    background: transparent;
    color: var(--rb-muted);
    cursor: pointer;
    transition:
      background-color var(--duration-quick, 75ms) ease,
      color var(--duration-quick, 75ms) ease,
      transform var(--duration-quick, 75ms) ease;
  }

  .rail-tab:active {
    transform: scale(0.96);
  }

  .rail-tab:hover:not(:disabled) {
    color: var(--rb-text);
    background: color-mix(in srgb, var(--rb-text) 7%, transparent);
  }

  .rail-tab.is-on {
    color: var(--rb-text);
    background: color-mix(in srgb, var(--accent, #da7756) 17%, transparent);
  }

  .rail-tab:focus-visible {
    outline: none;
    box-shadow: inset 0 0 0 2px color-mix(in srgb, var(--accent, #da7756) 55%, transparent);
  }

  .rail-logo,
  .active-agent-logo {
    position: relative;
    display: grid;
    flex: 0 0 auto;
    place-items: center;
    color: var(--rb-text);
  }

  /* Punto de sesión vivo: esquina de la ficha. */
  .rail-tab .live {
    position: absolute;
    top: 0.24rem;
    right: 0.24rem;
  }

  .live {
    display: inline-block;
    width: 0.32rem;
    height: 0.32rem;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent, #da7756) 90%, #fff);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent, #da7756) 22%, transparent);
  }

  .rail-add {
    display: flex;
    margin-top: auto;
    flex-direction: column;
    align-items: center;
    gap: 0.25rem;
    padding-top: 0.3rem;
  }

  .rail-resizer {
    position: absolute;
    top: 0;
    right: -0.28rem;
    z-index: 3;
    width: 0.56rem;
    height: 100%;
    border: 0;
    padding: 0;
    background: transparent;
    cursor: ew-resize;
    touch-action: none;
  }

  .rail-resizer::after {
    position: absolute;
    top: 0.45rem;
    right: 0.25rem;
    bottom: 0.45rem;
    width: 1px;
    border-radius: 999px;
    background: color-mix(in sRGB, var(--rb-border-strong) 72%, transparent);
    content: "";
    opacity: 0;
    transition: opacity var(--duration-fast, 150ms) ease;
  }

  .rail-resizer:hover::after,
  .rail-resizer:focus-visible::after {
    opacity: 1;
  }

  .rail-resizer:focus-visible {
    outline: none;
  }

  /* Columna derecha: barra fina + cuerpo */
  .col {
    display: flex;
    flex: 1;
    min-width: 0;
    flex-direction: column;
  }

  .where {
    margin: 0;
    min-width: 0;
    overflow: hidden;
    color: var(--rb-muted);
    font-size: 0.68rem;
    font-weight: 620;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Carpeta de inicio a la vista y editable sin volver al lanzador. Solo el
     último tramo de la ruta: el path entero vive en el `title`. */
  .folder-chip {
    display: inline-flex;
    min-width: 0;
    max-width: 12rem;
    flex: 0 1 auto;
    align-items: center;
    gap: 0.3rem;
    border: 0;
    border-radius: 0.5rem;
    padding: 0.2rem 0.42rem;
    background: color-mix(in sRGB, var(--rb-text) 5%, transparent);
    color: var(--rb-muted);
    font: inherit;
    font-size: 0.62rem;
    font-weight: 600;
    cursor: pointer;
    transition:
      background-color var(--duration-quick, 75ms) ease,
      color var(--duration-quick, 75ms) ease,
      transform var(--duration-quick, 75ms) ease;
  }

  .folder-chip span {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .folder-chip:hover {
    background: color-mix(in sRGB, var(--rb-text) 10%, transparent);
    color: var(--rb-text);
  }

  .folder-chip:active {
    transform: scale(0.96);
  }

  .bar {
    display: grid;
    flex-shrink: 0;
    align-items: center;
    gap: 0.4rem;
    padding: 0.28rem 0.5rem 0.28rem 0.6rem;
    border-bottom: 0;
    background: transparent;
    cursor: default;
  }

  .host-pick {
    display: flex;
    min-width: 0;
    max-width: 14rem;
    margin-left: 0.15rem;
  }

  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }

  .host-select {
    min-width: 0;
    max-width: 100%;
    border: 1px solid color-mix(in srgb, var(--rb-border) 80%, transparent);
    border-radius: 0.35rem;
    padding: 0.18rem 0.4rem;
    background: color-mix(in srgb, var(--rb-surface-2) 80%, transparent);
    color: var(--rb-text);
    font: inherit;
    font-size: 0.62rem;
    font-weight: 560;
    cursor: pointer;
  }

  .host-select:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .window-actions {
    display: flex;
    min-width: 0;
    flex-shrink: 0;
    flex-wrap: nowrap;
    align-items: center;
    gap: 0.2rem;
  }

  .more-menu {
    position: relative;
    flex: 0 0 auto;
  }

  .more-pop,
  .shortcuts-pop {
    position: absolute;
    top: calc(100% + 0.28rem);
    right: 0;
    left: auto;
    z-index: 20;
    max-width: min(16.5rem, calc(100cqi - 5rem));
  }

  .more-pop {
    display: flex;
    min-width: 11.5rem;
    flex-direction: column;
    gap: 0.08rem;
    border: 1px solid color-mix(in srgb, var(--rb-border) 80%, transparent);
    border-radius: 0.65rem;
    padding: 0.32rem;
    background: color-mix(in srgb, var(--rb-surface) 96%, var(--rb-bg0, #0f1115));
    box-shadow: 0 8px 22px color-mix(in srgb, #000 32%, transparent);
  }

  /* Menús: nacen del gatillo con un beat corto. La salida es instantánea,
     igual que un menú nativo. */
  .more-pop,
  .shortcuts-pop {
    transform-origin: 100% 0;
    animation: pop-in-down var(--duration-fast) var(--ease-smooth-out);
  }

  @keyframes pop-in-down {
    from {
      opacity: 0;
      transform: translateY(-4px) scale(0.98);
    }
  }

  @keyframes pop-in-up {
    from {
      opacity: 0;
      transform: translateY(4px) scale(0.98);
    }
  }

  .more-item {
    display: flex;
    width: 100%;
    align-items: center;
    gap: 0.5rem;
    border: 0;
    border-radius: 0.42rem;
    padding: 0.38rem 0.48rem;
    background: transparent;
    color: var(--rb-text);
    font: inherit;
    font-size: 0.7rem;
    font-weight: 540;
    text-align: left;
    cursor: pointer;
  }

  .more-item:hover:not(:disabled) {
    background: color-mix(in srgb, var(--rb-text) 8%, transparent);
  }

  .more-item:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .shortcuts-pop {
    display: flex;
    width: 16.5rem;
    flex-direction: column;
    gap: 0.35rem;
    border: 1px solid color-mix(in srgb, var(--rb-border) 80%, transparent);
    border-radius: 0.65rem;
    padding: 0.55rem 0.6rem 0.5rem;
    background: color-mix(in srgb, var(--rb-surface) 96%, var(--rb-bg0, #0f1115));
    box-shadow: 0 8px 22px color-mix(in srgb, #000 32%, transparent);
  }

  .shortcuts-title {
    margin: 0;
    color: var(--rb-text);
    font-size: 0.72rem;
    font-weight: 650;
    text-wrap: balance;
  }

  .shortcuts-hint {
    margin: 0;
    color: var(--rb-muted);
    font-size: 0.62rem;
    line-height: 1.35;
  }

  .shortcuts-list {
    display: flex;
    flex-direction: column;
    gap: 0.12rem;
    margin: 0.12rem 0 0;
    padding: 0;
    list-style: none;
  }

  .shortcuts-list li {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.22rem 0;
    color: var(--rb-text);
    font-size: 0.7rem;
    font-weight: 520;
  }

  .shortcuts-list kbd {
    margin-left: auto;
    border: 1px solid color-mix(in srgb, var(--rb-border) 80%, transparent);
    border-radius: 0.28rem;
    padding: 0.08rem 0.32rem;
    color: var(--rb-muted);
    font-size: 0.58rem;
    font-weight: 650;
    font-variant-numeric: tabular-nums;
  }

  .window-actions {
    justify-content: flex-end;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 0.24rem;
    border: 1px solid color-mix(in srgb, var(--rb-border) 80%, transparent);
    border-radius: 999px;
    padding: 0.18rem 0.52rem;
    background: color-mix(in srgb, var(--rb-surface-2) 70%, transparent);
    color: var(--rb-muted);
    font: inherit;
    font-size: 0.62rem;
    font-weight: 600;
    letter-spacing: 0.01em;
    cursor: pointer;
    transition:
      background-color var(--duration-quick, 75ms) ease,
      color var(--duration-quick, 75ms) ease,
      border-color var(--duration-quick, 75ms) ease,
      transform var(--duration-quick, 75ms) ease;
  }

  .chip:hover:not(:disabled) {
    color: var(--rb-text);
    border-color: color-mix(in srgb, var(--rb-text) 22%, transparent);
  }

  .chip:active:not(:disabled) {
    transform: scale(0.96);
  }

  .chip.is-go {
    border-color: color-mix(in srgb, var(--accent, #da7756) 45%, transparent);
    background: color-mix(in srgb, var(--accent, #da7756) 22%, transparent);
    color: var(--rb-text);
  }

  .chip:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .icon-btn {
    display: grid;
    place-items: center;
    width: 2.25rem;
    height: 2.25rem;
    border: 0;
    border-radius: 0.45rem;
    background: transparent;
    color: var(--rb-muted);
    cursor: pointer;
    transition:
      background-color var(--duration-quick, 75ms) ease,
      color var(--duration-quick, 75ms) ease,
      transform var(--duration-quick, 75ms) ease;
  }

  .icon-btn:hover {
    color: var(--rb-text);
    background: color-mix(in srgb, var(--rb-record) 16%, transparent);
  }

  .icon-btn:active {
    transform: scale(0.96);
  }

  .err {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin: 0;
    padding: 0.25rem 0.65rem;
    border-bottom: 1px solid color-mix(in srgb, var(--rb-record) 35%, transparent);
    color: var(--rb-record);
    font-size: 0.68rem;
    line-height: 1.35;
    animation: pop-in-down var(--duration-fast) var(--ease-smooth-out);
  }

  .err-text {
    flex: 1;
    min-width: 0;
  }

  .err-x {
    display: grid;
    flex: none;
    place-items: center;
    width: 1.15rem;
    height: 1.15rem;
    padding: 0;
    border: 0;
    border-radius: 0.35rem;
    background: transparent;
    color: inherit;
    cursor: pointer;
    transition: background var(--duration-fast) var(--ease-smooth-out);
  }

  .err-x:hover {
    background: color-mix(in sRGB, var(--rb-record) 14%, transparent);
  }

  .body {
    position: relative;
    isolation: isolate;
    display: flex;
    flex: 1;
    min-height: 0;
    overflow: hidden;
    background: color-mix(in srgb, var(--rb-surface) 88%, var(--rb-bg0, #0f1115));
  }

  .empty {
    position: absolute;
    inset: 0;
    z-index: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    background: color-mix(in srgb, #0f1115 55%, transparent);
    pointer-events: auto;
  }

  .term {
    /* A ras del cuerpo, solo o dividido: el terminal ocupa el pane entero. */
    position: absolute;
    top: var(--pane-y, 0%);
    left: var(--pane-x, 0%);
    z-index: 0;
    width: var(--pane-width, 100%);
    height: var(--pane-height, 100%);
    box-sizing: border-box;
    border: 0;
    padding: 0;
    overflow: hidden;
    background: var(--rb-bg0);
    cursor: text;
  }

  /* La costura entre dos panes: zona de agarre ancha, línea fina al hover. */
  .pane-divider {
    position: absolute;
    z-index: 2;
    background: transparent;
    touch-action: none;
  }

  .pane-divider.is-vertical {
    width: 9px;
    transform: translateX(-50%);
    cursor: col-resize;
  }

  .pane-divider:not(.is-vertical) {
    height: 9px;
    transform: translateY(-50%);
    cursor: row-resize;
  }

  .pane-divider::after {
    content: "";
    position: absolute;
    inset: 0;
    margin: auto;
    background: color-mix(in sRGB, var(--rb-text) 26%, transparent);
    opacity: 0;
    transition: opacity var(--duration-fast, 120ms);
  }

  .pane-divider.is-vertical::after {
    width: 2px;
  }

  .pane-divider:not(.is-vertical)::after {
    height: 2px;
  }

  .pane-divider:hover::after,
  .pane-divider:active::after {
    opacity: 1;
  }

  .term-host {
    width: 100%;
    height: 100%;
  }

  /* Línea entre paneles, sin inset ni marco exterior. */
  .body.is-split .term.is-join-left {
    box-shadow: -1px 0 0 color-mix(in sRGB, var(--rb-border) 70%, transparent);
  }

  .body.is-split .term.is-join-top {
    box-shadow: 0 -1px 0 color-mix(in sRGB, var(--rb-border) 70%, transparent);
  }

  .body.is-split .term.is-join-left.is-join-top {
    box-shadow:
      -1px 0 0 color-mix(in sRGB, var(--rb-border) 70%, transparent),
      0 -1px 0 color-mix(in sRGB, var(--rb-border) 70%, transparent);
  }

  .term.is-active {
    z-index: 1;
  }

  /* No usar `visibility: hidden`, `opacity: 0` ni sacarlo de pantalla: el
     canvas de xterm en WebView2 deja de pintar y al volver queda en blanco.
     Se queda en su sitio, detrás del pane activo, para que el renderer siga. */
  .term.is-hidden {
    z-index: -1;
    pointer-events: none;
  }

  /* Vista previa del drop: la mitad que va a ocupar la consola arrastrada. */
  .term.is-drop::after {
    content: "";
    position: absolute;
    inset: 0;
    z-index: 10;
    border: 1px dashed color-mix(in sRGB, var(--agent-accent) 72%, transparent);
    border-radius: 0;
    background: color-mix(in sRGB, var(--agent-accent) 13%, transparent);
    pointer-events: none;
  }

  .term.is-drop.drop-right::after {
    left: 50%;
  }

  .term.is-drop.drop-down::after {
    top: 50%;
  }

  .term-boot {
    position: absolute;
    inset: 0;
    z-index: 4;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.42rem;
    padding: 1rem;
    background: color-mix(in srgb, var(--rb-bg0) 92%, transparent);
    pointer-events: none;
    animation: term-boot-in var(--duration-fast) var(--ease-smooth-out);
  }

  @keyframes term-boot-in {
    from {
      opacity: 0;
    }
  }

  .term-boot-logo {
    display: grid;
    margin-bottom: 0.15rem;
    place-items: center;
  }

  .term-boot-title {
    margin: 0;
    color: var(--rb-text);
    font-size: 0.82rem;
    font-weight: 650;
    text-align: center;
    text-wrap: balance;
  }

  .term-boot-hint {
    max-width: 16rem;
    margin: 0;
    color: var(--rb-muted);
    font-size: 0.66rem;
    line-height: 1.4;
    text-align: center;
    text-wrap: pretty;
  }

  .term-boot-spin {
    width: 1.05rem;
    height: 1.05rem;
    margin-top: 0.35rem;
    border: 1.5px solid color-mix(in srgb, var(--rb-muted) 32%, transparent);
    border-top-color: var(--accent, var(--rb-text));
    border-radius: 999px;
    animation: term-boot-spin 0.7s linear infinite;
  }

  @keyframes term-boot-spin {
    to {
      transform: rotate(360deg);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .term-boot-spin {
      animation: none;
      border-top-color: color-mix(in srgb, var(--rb-muted) 32%, transparent);
      opacity: 0.7;
    }

    .more-pop,
    .shortcuts-pop,
    .add-pop,
    .ctx,
    .err,
    .term-boot {
      animation: none;
    }

    .icon-btn:active,
    .rail-tab:active,
    .tab-add:active,
    .chip:active,
    .folder-chip:active,
    .console-desk .back-btn:active {
      transform: none;
    }
  }

  .rail-tab.is-dragging {
    opacity: 0.55;
  }

  .tab-ghost {
    position: fixed;
    z-index: var(--z-popover, 60);
    display: grid;
    width: 1.9rem;
    height: 1.9rem;
    place-items: center;
    border-radius: 0.5rem;
    background: color-mix(in sRGB, var(--rb-surface) 94%, var(--rb-bg0));
    box-shadow: 0 6px 18px color-mix(in sRGB, rgb(0 0 0) 30%, transparent);
    transform: translate(-50%, -130%);
    pointer-events: none;
  }

  .term :global(.xterm) {
    width: 100%;
    height: 100%;
  }

  .term :global(.xterm-helper-textarea) {
    user-select: text;
    -webkit-user-select: text;
  }

  .term :global(.xterm-viewport) {
    overflow-y: auto !important;
  }

  .ctx {
    position: fixed;
    z-index: var(--z-popover, 60);
    display: flex;
    min-width: 7.5rem;
    flex-direction: column;
    gap: 0.1rem;
    border: 1px solid color-mix(in srgb, var(--rb-border) 80%, transparent);
    border-radius: 0.45rem;
    padding: 0.2rem;
    background: color-mix(in srgb, var(--rb-surface) 94%, #0f1115);
    box-shadow: 0 8px 24px color-mix(in srgb, #000 35%, transparent);

    /* Más corto que los popovers: un menú contextual tiene que sentirse ya. */
    transform-origin: 0 0;
    animation: pop-in-down var(--duration-quick) var(--ease-smooth-out);
  }

  .ctx-item {
    border: 0;
    border-radius: 0.3rem;
    padding: 0.35rem 0.55rem;
    background: transparent;
    color: var(--rb-text);
    font: inherit;
    font-size: 0.72rem;
    font-weight: 560;
    text-align: left;
    cursor: pointer;
  }

  .ctx-item:hover:not(:disabled) {
    background: color-mix(in srgb, var(--rb-text) 8%, transparent);
  }

  .ctx-item:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .tab-x,
  .tab-add {
    display: inline-flex;
    flex-shrink: 0;
    align-items: center;
    justify-content: center;
    border: 1px solid transparent;
    border-radius: 0.45rem;
    padding: 0.24rem;
    background: transparent;
    color: var(--rb-muted);
    font: inherit;
    font-size: 0.6rem;
    font-weight: 650;
    cursor: pointer;
    transition:
      background-color var(--duration-quick, 75ms) ease,
      color var(--duration-quick, 75ms) ease,
      opacity var(--duration-quick, 75ms) ease,
      transform var(--duration-quick, 75ms) ease;
  }

  .tab-add:active:not(:disabled) {
    transform: scale(0.96);
  }

  /* Se revela al pasar por encima de la ficha: una X siempre visible en cada
     una convierte el rail en ruido. */
  .tab-x {
    position: absolute;
    right: -0.3rem;
    bottom: -0.3rem;
    padding: 0.14rem;
    opacity: 0;
    background: var(--rb-surface);
    box-shadow: 0 1px 4px color-mix(in srgb, #000 25%, transparent);
    transition: opacity var(--duration-quick, 75ms) ease;
  }

  /* La X visible es diminuta; el área de clic no tiene por qué serlo. */
  .tab-x::before {
    position: absolute;
    inset: -0.3rem;
    content: "";
  }

  .rail-slot:hover .tab-x,
  .rail-slot:focus-within .tab-x,
  .tab-x:focus-visible {
    opacity: 1;
  }

  .tab-x:hover {
    color: var(--rb-record);
    background: color-mix(in srgb, var(--rb-record) 14%, var(--rb-surface));
  }

  .add-menu {
    position: relative;
    display: inline-flex;
  }

  /* Ancla en el rail y abre hacia arriba-derecha, sobre los terminales. */
  .add-pop {
    position: absolute;
    bottom: calc(100% + 0.3rem);
    left: 0;
    z-index: 9;
    display: flex;
    min-width: 10.5rem;
    max-height: 20rem;
    flex-direction: column;
    gap: 0.08rem;
    border: 1px solid color-mix(in sRGB, var(--rb-border) 80%, transparent);
    border-radius: 0.5rem;
    padding: 0.22rem;
    overflow-y: auto;
    background: color-mix(in sRGB, var(--rb-surface) 96%, var(--rb-bg0));
    box-shadow: 0 8px 22px color-mix(in sRGB, rgb(0 0 0) 32%, transparent);

    /* Abre hacia arriba: emerge desde el botón "+". */
    transform-origin: 0 100%;
    animation: pop-in-up var(--duration-fast) var(--ease-smooth-out);
  }

  .add-item {
    display: flex;
    width: 100%;
    align-items: center;
    gap: 0.42rem;
    border: 0;
    border-radius: 0.35rem;
    padding: 0.34rem 0.44rem;
    background: transparent;
    color: var(--rb-text);
    font: inherit;
    font-size: 0.7rem;
    font-weight: 560;
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
  }

  .add-item:hover:not(:disabled) {
    background: color-mix(in sRGB, var(--rb-text) 8%, transparent);
  }

  .add-item:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .add-glyph {
    display: grid;
    width: 1.15rem;
    height: 1.15rem;
    flex: 0 0 auto;
    place-items: center;
    color: var(--rb-muted);
  }

  .add-glyph.is-ssh-glyph {
    font-size: 0.48rem;
    font-weight: 700;
    letter-spacing: 0.04em;
  }

  .add-item.is-folder {
    color: var(--rb-muted);
  }

  /* Chip «Instalar» de un agente que no está en el PATH. */
  .add-install {
    margin-left: auto;
    flex: 0 0 auto;
    border-radius: 0.3rem;
    padding: 0.1rem 0.34rem;
    background: color-mix(in sRGB, var(--accent, #da7756) 16%, transparent);
    color: var(--accent, #da7756);
    font-size: 0.62rem;
    font-weight: 640;
    letter-spacing: 0.02em;
  }

  .add-item:hover .add-install {
    background: color-mix(in sRGB, var(--accent, #da7756) 26%, transparent);
  }

  .add-chevron {
    margin-left: auto;
    padding-left: 0.3rem;
    color: var(--rb-faint);
    font-size: 0.85rem;
    line-height: 1;
  }

  .add-group {
    margin: 0.18rem 0 0;
    padding: 0.14rem 0.44rem;
    border-top: 1px solid color-mix(in sRGB, var(--rb-border) 62%, transparent);
    color: var(--rb-faint);
    font-size: 0.54rem;
    font-weight: 680;
    letter-spacing: 0.05em;
    text-transform: uppercase;
  }

  .add-pop > .add-group:first-child {
    margin-top: 0;
    border-top: 0;
  }

  .add-cmd {
    margin: 0.08rem 0.2rem 0.14rem;
    border: 1px solid color-mix(in sRGB, var(--rb-border) 84%, transparent);
    border-radius: 0.35rem;
    padding: 0.3rem 0.4rem;
    background: color-mix(in sRGB, var(--rb-surface-2) 70%, transparent);
    color: var(--rb-text);
    font: inherit;
    font-size: 0.66rem;
  }

  .add-cmd:focus-visible {
    outline: none;
    border-color: color-mix(in sRGB, var(--accent, var(--rb-text)) 55%, transparent);
  }

  .add-saved {
    position: relative;
    display: flex;
    align-items: center;
  }

  .add-saved .add-item {
    padding-right: 1.4rem;
  }

  .add-forget {
    position: absolute;
    right: 0.2rem;
    display: grid;
    width: 1.05rem;
    height: 1.05rem;
    place-items: center;
    border: 0;
    border-radius: 0.28rem;
    background: transparent;
    color: var(--rb-faint);
    cursor: pointer;
  }

  .add-forget:hover {
    color: var(--rb-record);
    background: color-mix(in sRGB, var(--rb-record) 12%, transparent);
  }

  .add-ellipsis {
    min-width: 0;
    max-width: 11rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tab-add:hover:not(:disabled),
  .tab-add:focus-visible {
    color: var(--rb-text);
    background: color-mix(in srgb, var(--accent, #da7756) 18%, transparent);
  }

  .tab-add:disabled {
    opacity: 0.5;
    cursor: default;
  }

  /* ─── Console desk: navegación legible y estados con contraste ─── */
  .console-desk {
    --agent-accent: var(--rb-record);

    container-name: agents-console;
    container-type: inline-size;
    border: 0;
    border-radius: inherit;
    overflow: hidden;
    background: transparent;
  }

  .console-desk .rail {
    width: var(--rail-width, 8rem);
    min-width: 3.375rem;
    max-width: 14rem;
    padding: 0.4rem 0.34rem;
    background: color-mix(
      in sRGB,
      var(--rb-sidebar, var(--rb-surface-2)) 88%,
      transparent
    );
  }

  .console-desk .rail-tabs {
    align-items: stretch;
    gap: 0.22rem;
  }

  .console-desk .rail-slot {
    width: 100%;
  }

  .console-desk .rail-tab {
    grid-template-columns: 1.72rem minmax(0, 1fr);
    width: 100%;
    height: 2.7rem;
    justify-items: start;
    gap: 0.42rem;
    padding: 0.35rem 0.4rem;
    border-radius: 0.6rem;
    text-align: left;
  }

  .console-desk .rail-tab.is-on {
    background: color-mix(in sRGB, var(--agent-accent) 13%, var(--skin));
  }

  /* Sin chip de fondo detrás del logo: se veía un cuadrado dentro de otro.
     La ficha es la única forma; el estado lo pinta la ficha completa. */
  .console-desk .rail-tab .rail-logo {
    display: grid;
    place-items: center;
    width: 1.72rem;
    height: 1.72rem;
    color: var(--rb-text);
  }

  /* Ficha del grupo: los logos de sus consolas, solapados en diagonal. */
  .console-desk .rail-tab .rail-logo.is-stack {
    display: block;
    position: relative;
  }

  .console-desk .rail-tab .rail-logo.is-stack > :global(*) {
    position: absolute;
    top: 0;
    left: 0;
  }

  .console-desk .rail-tab .rail-logo.is-stack > :global(*:nth-child(2)) {
    inset: auto 0 0 auto;
  }

  .console-desk .rail-copy {
    display: flex;
    min-width: 0;
    flex-direction: column;
    gap: 0.06rem;
  }

  .console-desk .rail-name,
  .console-desk .rail-status {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .rail-name {
    color: var(--rb-text);
    font-size: 0.66rem;
    font-weight: 680;
  }

  .rail-status {
    color: var(--rb-faint);
    font-size: 0.54rem;
    font-weight: 540;
  }

  .console-desk .rail-tab .live {
    top: 0.34rem;
    right: 0.34rem;
    background: var(--rb-ok);
    box-shadow: 0 0 0 2px color-mix(in sRGB, var(--rb-ok) 18%, transparent);
  }

  .console-desk .tab-x {
    right: 0.1rem;
    bottom: 0.12rem;
    width: 1.15rem;
    height: 1.15rem;
    padding: 0.2rem;
    border-radius: 0.35rem;
  }

  .console-desk .bar {
    grid-template-areas: "start window";
    grid-template-columns: minmax(0, 1fr) auto;
    min-height: 2.7rem;
    padding: 0.32rem 0.52rem;
    background: transparent;
  }

  .console-desk .bar-start,
  .console-desk .where-block {
    display: flex;
    min-width: 0;
    align-items: center;
  }

  .console-desk .bar-start {
    grid-area: start;
    gap: 0.48rem;
  }

  .console-desk .window-actions {
    grid-area: window;
  }

  .console-desk .host-pick {
    flex: 1 1 8rem;
  }

  .console-desk .where-block {
    flex-direction: row;
    align-items: center;
    gap: 0.08rem;
  }

  .console-desk .back-btn {
    display: inline-flex;
    align-items: center;
    gap: 0.28rem;
    min-height: 1.7rem;
    border: 1px solid transparent;
    border-radius: 0.5rem;
    background: transparent;
    color: var(--rb-muted);
    font: inherit;
    font-size: 0.62rem;
    font-weight: 650;
    cursor: pointer;
    transition:
      color var(--duration-quick, 75ms) ease,
      background-color var(--duration-quick, 75ms) ease,
      border-color var(--duration-quick, 75ms) ease,
      transform var(--duration-quick, 75ms) ease;
  }

  .console-desk .back-btn:active {
    transform: scale(0.96);
  }

  .console-desk .back-btn {
    padding: 0.14rem 0.34rem 0.14rem 0.24rem;
  }

  .console-desk .back-btn:hover {
    border-color: color-mix(in sRGB, var(--agent-accent) 35%, transparent);
    background: color-mix(in sRGB, var(--agent-accent) 10%, transparent);
    color: var(--rb-text);
  }

  .console-desk .where {
    color: var(--rb-text);
    font-size: 0.7rem;
    font-weight: 700;
  }

  .console-desk .session-dot {
    position: absolute;
    top: -0.12rem;
    right: -0.12rem;
    width: 0.42rem;
    height: 0.42rem;
    background: var(--rb-muted);
    box-shadow: 0 0 0 2px color-mix(in sRGB, var(--skin, var(--rb-surface)) 88%, transparent);
  }

  .console-desk .session-dot.is-live {
    background: var(--rb-ok);
    box-shadow: 0 0 0 2px color-mix(in sRGB, var(--rb-ok) 22%, transparent);
  }

  .console-desk .session-dot.is-prep {
    background: color-mix(in sRGB, var(--rb-ok) 55%, var(--rb-muted));
  }

  .console-desk .icon-btn {
    width: 1.7rem;
    height: 1.7rem;
  }

  .console-desk .icon-btn.is-on {
    color: var(--agent-accent);
    background: color-mix(in sRGB, var(--agent-accent) 13%, transparent);
  }

  .console-desk .body {
    background: var(--rb-bg0);
  }

  .console-desk .rail.is-compact {
    padding-inline: 0.24rem;
  }

  .console-desk .rail.is-compact .rail-tab {
    grid-template-columns: 1fr;
    justify-items: center;
    padding-inline: 0.25rem;
  }

  .console-desk .rail.is-compact .rail-copy {
    display: none;
  }

  .console-desk .empty {
    background: var(--rb-bg0);
  }

  .empty :global(.text-muted) {
    color: var(--rb-text) !important;
  }

  .empty :global(.text-faint) {
    color: var(--rb-muted) !important;
  }

  @container agents-console (width <= 40rem) {
    .console-desk .host-pick {
      max-width: 7.5rem;
      margin-right: 0.15rem;
      margin-left: 0;
    }
  }

  @container agents-console (width <= 34rem) {
    .console-desk .rail {
      width: 3.35rem;
      padding-inline: 0.25rem;
    }

    .console-desk .rail-tab {
      grid-template-columns: 1fr;
      justify-items: center;
      height: 2.65rem;
      padding: 0.3rem;
    }

    .console-desk .rail-copy {
      display: none;
    }

    .console-desk .back-btn span {
      display: none;
    }

    .console-desk .where-block {
      max-width: 9rem;
    }
  }

  @container agents-console (width <= 28rem) {
    .console-desk .rail {
      width: 3rem;
      min-width: 3rem;
      padding-inline: 0.2rem;
    }

    .console-desk .rail-tab {
      height: 2.45rem;
    }

    .console-desk .bar {
      padding-inline: 0.34rem;
    }

    .console-desk .host-pick {
      flex-basis: 5.5rem;
    }

    .console-desk .chip {
      padding-inline: 0.42rem;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .console-desk .back-btn,
    .icon-btn,
    .console-desk .rail-tab {
      transition: none;
    }
  }
</style>
