<script lang="ts">
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
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import "@xterm/xterm/css/xterm.css";
  import {
    agentsAlwaysOnTop,
    consoleClose,
    consoleOpen,
    consoleResize,
    consoleWrite,
    onConsoleExit,
    onConsoleOutput,
    setAgentsAlwaysOnTop,
    sshListHosts,
  } from "$ipc/agents";
  import { getConfig } from "$ipc/config";
  import type { ConsoleKind, SshHost } from "$lib/types";
  import EmptyState from "$lib/ui/EmptyState.svelte";
  import Icon from "$ui/Icon.svelte";
  import { ArrowLeft, Pin, Plus, SquareTerminal, X } from "$lib/icons";

  let {
    remoteHost = null,
    localCwd = "",
    initialKind = "local",
    initialTabs = null,
    onClose,
    onBack,
    onBarPointerDown,
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
     * Arrastre del float desde la barra (fondo, no controles). La barra NO
     * lleva `data-no-drag` justamente para que este handler pueda tomarla.
     */
    onBarPointerDown?: (e: PointerEvent) => void;
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
      };
  type PaneRect = { key: string; x: number; y: number; width: number; height: number };

  const RAIL_MIN = 54;
  const RAIL_DEFAULT = 128;
  const RAIL_MAX = 224;
  const RAIL_STORAGE_KEY = "atic.agents.consoleRailWidth";

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
    if (node.direction === "right") {
      const half = width / 2;
      return [
        ...collectPaneRects(node.first, x, y, half, height),
        ...collectPaneRects(node.second, x + half, y, half, height),
      ];
    }
    const half = height / 2;
    return [
      ...collectPaneRects(node.first, x, y, width, half),
      ...collectPaneRects(node.second, x, y + half, width, half),
    ];
  }

  let tabs = $state<Tab[]>([]);
  let activeKey = $state("");
  let connecting = $state(false);
  let error = $state<string | null>(null);
  let sshHosts = $state<SshHost[]>([]);
  let ctxMenu = $state<{ x: number; y: number; key: string } | null>(null);
  /** Árbol de splits. Las pestañas fuera del árbol siguen vivas en el rail. */
  let paneTree = $state<PaneNode | null>(null);
  let railWidth = $state(RAIL_DEFAULT);
  let pinned = $state(false);
  let consoleEl = $state<HTMLElement | null>(null);

  /** xterm por pestaña. Fuera de `$state` (ver `Tab`). */
  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- xterm instances are imperative, not UI state.
  const boxes = new Map<string, Box>();
  let stopListen: (() => void) | null = null;
  let seq = 0;

  const active = $derived(tabs.find((t) => t.key === activeKey) ?? null);
  const sessionId = $derived(active?.sessionId ?? null);
  const connected = $derived(!!sessionId);
  const paneRects = $derived(collectPaneRects(paneTree));
  const visiblePaneKeys = $derived(
    paneRects
      .map((pane) => pane.key)
      .filter((key) => tabs.some((tab) => tab.key === key)),
  );
  const paneMode = $derived(visiblePaneKeys.length > 1);
  const railCompact = $derived(railWidth < 92);

  function focusVisiblePane(key: string) {
    requestAnimationFrame(() => {
      fitAndResize(key);
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

  function consumeWorkspaceShortcut(event: KeyboardEvent): boolean {
    if (event.isComposing) return false;
    const key = event.key.toLowerCase();
    const code = event.code;
    const mod = event.ctrlKey || event.metaKey;

    // `code` cubre WebView2/xterm cuando Ctrl transforma `event.key` en un
    // carácter de control antes de que Svelte reciba el acorde.
    if (mod && !event.altKey && (code === "KeyD" || key === "d")) {
      event.preventDefault();
      event.stopPropagation();
      if (!event.repeat) splitPane(event.shiftKey ? "down" : "right");
      return true;
    }

    if (mod && !event.shiftKey && !event.altKey && (code === "KeyN" || key === "n")) {
      event.preventDefault();
      event.stopPropagation();
      if (!event.repeat) newTab("local");
      return true;
    }

    if (mod && !event.shiftKey && !event.altKey && (code === "KeyW" || key === "w")) {
      event.preventDefault();
      event.stopPropagation();
      if (!event.repeat && activeKey) void closeTab(activeKey);
      return true;
    }

    return false;
  }

  /**
   * WebView2 omite algunos `keydown` sin Shift dentro del textarea de xterm,
   * pero xterm sí los traduce a sus bytes de control. Consumirlos acá evita
   * que Ctrl+D llegue como EOF al shell y que Ctrl+N llegue como `next`.
   */
  function consumeTerminalControlData(key: string, data: string): boolean {
    if (data !== "\x04" && data !== "\x0e") return false;
    activeKey = key;
    error = null;
    if (data === "\x04") splitPane("right");
    else newTab("local");
    return true;
  }

  function clampRailWidth(width: number): number {
    return Math.min(RAIL_MAX, Math.max(RAIL_MIN, width));
  }

  function setRailWidth(width: number) {
    railWidth = clampRailWidth(width);
    localStorage.setItem(RAIL_STORAGE_KEY, String(Math.round(railWidth)));
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
    if (event.isComposing || !isInsideConsole(event.target)) return;
    // Captura antes del PTY. El handler de xterm repite esta defensa porque
    // WebView2 no siempre entrega los acordes Ctrl al `window` del overlay.
    consumeWorkspaceShortcut(event);
  }

  // Al sumar/quitar paneles o cambiar el rail, cada xterm cambia de tamaño.
  $effect(() => {
    void paneRects;
    void railWidth;
    requestAnimationFrame(() => {
      for (const key of visiblePaneKeys) fitAndResize(key);
    });
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

  /** Monograma del rail: iniciales del agente ("Claude Code" → CC, "OpenCode 2" → O2). */
  function monogram(label: string): string {
    const parts = label.trim().split(/\s+/).filter(Boolean);
    if (parts.length >= 2) {
      return (parts[0][0] + parts[1][0]).toUpperCase();
    }
    return (parts[0]?.slice(0, 2) ?? "?").toUpperCase();
  }

  const monograms = $derived(tabs.map((_, i) => monogram(tabLabels[i])));

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
   * Tras Conectar / switch: dispara el mismo camino que un clic en el term
   * para que OverlaySurface active set_overlay_text_mode.
   */
  function requestOverlayKeyboard(key = activeKey) {
    const host = boxes.get(key)?.el ?? null;
    if (!host) {
      focusTerm(key);
      return;
    }
    host.dispatchEvent(
      new PointerEvent("pointerdown", { bubbles: true, cancelable: true }),
    );
    // Tras el await de text-mode en OverlaySurface.
    requestAnimationFrame(() => {
      focusTerm(key);
      setTimeout(() => focusTerm(key), 40);
    });
  }

  function closeCtx() {
    ctxMenu = null;
  }

  async function disconnect(key = activeKey) {
    const id = sessionOf(key);
    setSession(key, null);
    if (id) {
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

  async function connect(key = activeKey) {
    const tab = tabOf(key);
    if (!tab || connecting) return;
    error = null;
    if (tab.kind === "ssh" && !hostById(tab.hostId)) {
      error = "Elige un host SSH en la consola (o agrégalo en Ajustes → Agentes).";
      return;
    }
    connecting = true;
    await disconnect(key);
    termOf(key)?.reset();
    try {
      const term = termOf(key);
      // Medir AHORA, no confiar en el último fit: al sembrar desde el
      // lanzador la PTY se abre mientras el float aún emerge (escala 0.55)
      // y el tamaño de ese momento queda chico para siempre.
      fitTermOnly(key);
      const id = await consoleOpen({
        kind: tab.kind,
        hostId: tab.kind === "ssh" ? tab.hostId : null,
        cwd: tab.kind === "local" && localCwd.trim() ? localCwd.trim() : null,
        command: tab.kind === "local" ? tab.command : null,
        cols: term?.cols ?? 80,
        rows: term?.rows ?? 24,
      });
      setSession(key, id);
      requestAnimationFrame(() => {
        fitAndResize(key);
        if (key === activeKey) requestOverlayKeyboard(key);
      });
      // La animación del float dura ~300ms: cuando termina, el contenedor
      // cambia de tamaño por última vez y puede que el ResizeObserver ya
      // haya disparado su último evento ANTES de que hubiera sesión. Estos
      // re-ajustes tardíos son la red: le mandan al PTY el tamaño real.
      setTimeout(() => fitAndResize(key), 350);
      setTimeout(() => fitAndResize(key), 800);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      setSession(key, null);
    } finally {
      connecting = false;
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

  function newTab(
    kind: ConsoleKind,
    opts: {
      label?: string;
      command?: string;
      hostId?: string;
      splitDirection?: SplitDirection;
      splitSourceKey?: string;
    } = {},
  ) {
    if (tabs.length >= MAX_TABS) {
      error = `Ya hay ${MAX_TABS} consolas abiertas. Cerrá alguna para abrir otra.`;
      return;
    }
    closeCtx();
    const treeBefore = paneTree;
    const previousActiveKey = activeKey;
    const key = `t${++seq}`;
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
      },
    ];
    activeKey = key;
    if (opts.splitDirection && opts.splitSourceKey) {
      const sourceKey = opts.splitSourceKey;
      const base = treeBefore ?? leaf(sourceKey);
      paneTree = replacePaneLeaf(base, sourceKey, {
        kind: "split",
        direction: opts.splitDirection,
        first: leaf(sourceKey),
        second: leaf(key),
      });
    } else if (treeBefore && paneLeafKeys(treeBefore).includes(previousActiveKey)) {
      // Nueva consola/pestaña: ocupa el panel activo y conserva los otros splits.
      paneTree = replacePaneLeaf(treeBefore, previousActiveKey, leaf(key));
    } else {
      paneTree = leaf(key);
    }
    error = null;
    if (kind === "ssh") void loadSshHosts();
    // Un cuadro después el `{@attach}` ya creó el xterm, así que `fit()` mide
    // sobre el contenedor real y el PTY nace con el tamaño correcto.
    requestAnimationFrame(() => {
      if (kind === "local" || hostById(tabOf(key)?.hostId ?? null)) {
        openChain = openChain.then(() => connect(key)).catch(() => {});
      }
    });
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
    paneTree = nextTree ?? (activeKey ? leaf(activeKey) : null);
    if (activeKey) focusVisiblePane(activeKey);
  }

  function switchTab(key: string) {
    closeCtx();
    if (paneMode && !visiblePaneKeys.includes(key)) {
      const sourceKey = visiblePaneKeys.includes(activeKey)
        ? activeKey
        : visiblePaneKeys[0];
      paneTree =
        paneTree && sourceKey
          ? replacePaneLeaf(paneTree, sourceKey, leaf(key))
          : leaf(key);
    } else if (!paneMode) {
      paneTree = leaf(key);
    }
    activeKey = key;
    error = null;
    // No se desconecta nada: las otras pestañas siguen vivas.
    focusVisiblePane(key);
  }

  function fitAndResize(key = activeKey) {
    const box = boxes.get(key);
    if (!box) return;
    try {
      box.fit.fit();
    } catch {
      return;
    }
    const id = sessionOf(key);
    if (id) {
      void consoleResize(id, box.term.cols, box.term.rows).catch(() => {});
    }
  }

  /** Re-mide el xterm sin tocar el PTY (antes de que exista sesión). */
  function fitTermOnly(key = activeKey) {
    const box = boxes.get(key);
    if (!box) return;
    try {
      box.fit.fit();
    } catch {
      /* contenedor sin tamaño todavía: queda el default */
    }
  }

  /**
   * Paleta del xterm según el tema resuelto (`data-theme` en :root).
   * La consola sigue el tema de la app; los TUI que pintan su propio fondo
   * (opencode claro) siguen siendo cosa del agente, no del terminal.
   */
  function termTheme(): Record<string, string> {
    const light = document.documentElement.dataset.theme === "light";
    return light
      ? {
          background: "#f7f7f2",
          foreground: "#26231e",
          cursor: "#da7756",
          selectionBackground: "rgba(218, 119, 86, 0.3)",
        }
      : {
          background: "#16181d",
          foreground: "#e6e8ec",
          cursor: "#da7756",
          selectionBackground: "rgba(218, 119, 86, 0.35)",
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
      fontFamily: "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace",
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
      if (consumeWorkspaceShortcut(ev)) return false;
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
        const observer = new ResizeObserver(() => fitAndResize(key));
        observer.observe(el);
        requestAnimationFrame(() => fitAndResize(key));
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
    requestAnimationFrame(() => {
      focusTerm(key);
      setTimeout(() => focusTerm(key), 40);
    });
  }

  onMount(() => {
    const savedRailWidth = Number(localStorage.getItem(RAIL_STORAGE_KEY));
    if (Number.isFinite(savedRailWidth) && savedRailWidth > 0) {
      railWidth = clampRailWidth(savedRailWidth);
    }
    void loadSshHosts();
    void agentsAlwaysOnTop()
      .then((on) => (pinned = on))
      .catch(() => (pinned = false));
    window.addEventListener("keydown", onGlobalKey, true);
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
      onConsoleOutput((p) => {
        const t = tabForSession(p.session);
        if (t) termOf(t.key)?.write(p.data);
      }),
      onConsoleExit((p) => {
        const t = tabForSession(p.session);
        if (!t) return;
        const key = t.key;
        setSession(key, null);
        const code = p.code == null ? "?" : String(p.code);
        termOf(key)?.writeln(`\r\n[sesión terminada · exit ${code}]`);
      }),
    ]).then((uns) => {
      stopListen = () => {
        for (const u of uns) u();
      };
    });

    const onDocPointer = (e: PointerEvent) => {
      // Capture: stopPropagation del menú no alcanza; hay que excluir el .ctx acá.
      if (e.target instanceof Node) {
        const menu = document.querySelector(".console .ctx");
        if (menu?.contains(e.target)) return;
      }
      closeCtx();
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
      document.removeEventListener("pointerdown", onDocPointer, true);
      window.removeEventListener("blur", closeCtx);
      themeObs.disconnect();
    };
  });

  onDestroy(() => {
    stopListen?.();
    void disconnectAll();
    // Los xterm los dispone el teardown de cada `{@attach}`.
  });
</script>

<section class="console console-desk" bind:this={consoleEl} aria-label="Consola">
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
        <span class="rail-slot">
          <button
            type="button"
            class="rail-tab"
            class:is-on={t.key === activeKey}
            aria-current={t.key === activeKey ? "true" : undefined}
            title={tabLabels[i]}
            onclick={() => switchTab(t.key)}
          >
            <span class="mono" aria-hidden="true">{monograms[i]}</span>
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
              <span class="live" title="Sesión activa" aria-hidden="true"></span>
            {/if}
          </button>
          <button
            type="button"
            class="tab-x"
            aria-label="Cerrar {tabLabels[i]}"
            title="Cerrar pestaña"
            onclick={() => void closeTab(t.key)}
          >
            <Icon icon={X} size={9} />
          </button>
        </span>
      {/each}
    </div>
    <div class="rail-add">
      <button
        type="button"
        class="tab-add"
        aria-label="Nueva consola local"
        title="Nueva consola local · Ctrl+N"
        disabled={connecting || tabs.length >= MAX_TABS}
        onclick={() => newTab("local")}
      >
        <Icon icon={Plus} size={12} />
      </button>
      <button
        type="button"
        class="tab-add is-ssh"
        aria-label="Nueva consola SSH"
        title="Nueva consola SSH"
        disabled={connecting || tabs.length >= MAX_TABS}
        onclick={() => newTab("ssh")}
      >
        SSH
      </button>
    </div>
    <button
      type="button"
      class="rail-resizer"
      aria-label={`Cambiar ancho de la barra lateral, ${Math.round(railWidth)} píxeles`}
      title="Arrastra para cambiar el ancho · Doble clic para contraer"
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
            title="Volver al lanzador"
            onclick={onBack}
          >
            <Icon icon={ArrowLeft} size={13} />
            <span>Agentes</span>
          </button>
        {/if}
        <div class="where-block">
          <p class="where" title={active ? tabLabels[tabs.indexOf(active)] : ""}>
            {active ? tabLabels[tabs.indexOf(active)] : "Sin consolas"}
          </p>
          <span class="session-state" class:is-live={!!active?.sessionId}>
            {active?.sessionId
              ? "En ejecución"
              : connecting
                ? "Preparando"
                : "Sin conexión"}
          </span>
        </div>
      </div>
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
      <div class="acts">
        {#if active}
          <button
            type="button"
            class="layout-toggle"
            aria-label="Abrir otro panel a la derecha"
            title="Abrir otro panel a la derecha · Ctrl+D"
            disabled={tabs.length >= MAX_TABS && visiblePaneKeys.length >= tabs.length}
            onclick={() => splitPane("right")}
          >
            <span>Derecha</span>
            <kbd>Ctrl+D</kbd>
          </button>
          <button
            type="button"
            class="layout-toggle"
            aria-label="Abrir otro panel abajo"
            title="Abrir otro panel abajo · Ctrl+Shift+D"
            disabled={tabs.length >= MAX_TABS && visiblePaneKeys.length >= tabs.length}
            onclick={() => splitPane("down")}
          >
            <span>Abajo</span>
            <kbd>Ctrl+⇧D</kbd>
          </button>
        {/if}
        {#if active}
          {#if connected}
            <button
              type="button"
              class="chip"
              disabled={connecting}
              onclick={() => void disconnect()}
            >
              Desconectar
            </button>
          {:else}
            <button
              type="button"
              class="chip is-go"
              disabled={connecting || (active.kind === "ssh" && !activeHost)}
              onclick={() => void connect()}
            >
              {connecting ? "Conectando…" : "Conectar"}
            </button>
          {/if}
        {/if}
        <button
          type="button"
          class="icon-btn pin-btn"
          class:is-on={pinned}
          aria-label={pinned ? "Desfijar ventana" : "Fijar ventana arriba"}
          aria-pressed={pinned}
          title={pinned ? "Desfijar ventana" : "Fijar ventana arriba"}
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
            aria-label="Cerrar consola"
            title="Cerrar consola"
            onclick={() => {
              void disconnectAll();
              onClose();
            }}
          >
            <Icon icon={X} size={12} />
          </button>
        {/if}
      </div>
    </header>

    {#if error}
      <p class="err" role="alert">{error}</p>
    {/if}

    <div class="body" class:is-split={paneMode}>
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

      {#each tabs as t (t.key)}
        {@const paneRect = paneRects.find((pane) => pane.key === t.key)}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="term"
          class:is-active={t.key === activeKey}
          class:is-hidden={!paneRect || (!paneMode && !t.sessionId && !connecting)}
          style={paneRect
            ? `--pane-x: ${paneRect.x}%; --pane-y: ${paneRect.y}%; --pane-width: ${paneRect.width}%; --pane-height: ${paneRect.height}%`
            : undefined}
          {@attach mountTerm(t.key)}
          data-no-drag
          data-selectable
          data-console-term
          onpointerdown={() => onTermPointerDown(t.key)}
          oncontextmenu={(e) => onTermContextMenu(t.key, e)}
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
        </div>
      {/if}
    </div>
  </div>
</section>

<style>
  .console {
    display: flex;
    flex: 1;
    min-width: 0;
    flex-direction: row;
    min-height: 0;
    background: color-mix(in srgb, var(--rb-surface) 88%, var(--rb-bg0, #0f1115));

    /* Overlay: user-select/touch-action none; xterm necesita interactuar. */
    user-select: text;
    -webkit-user-select: text;
    touch-action: auto;
  }

  /* ─── Rail izquierdo: una ficha por consola ───────────────────────────── */
  .rail {
    position: relative;
    display: flex;
    flex-shrink: 0;
    flex-direction: column;
    align-items: center;
    width: var(--rail-width, 2.9rem);
    padding: 0.35rem 0.25rem;
    border-right: 1px solid color-mix(in srgb, var(--rb-border) 70%, transparent);
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
    border: 1px solid transparent;
    border-radius: 0.6rem;
    background: transparent;
    color: var(--rb-muted);
    cursor: pointer;
    transition:
      background-color var(--duration-quick, 75ms) ease,
      color var(--duration-quick, 75ms) ease,
      border-color var(--duration-quick, 75ms) ease;
  }

  .rail-tab:hover:not(:disabled) {
    color: var(--rb-text);
    background: color-mix(in srgb, var(--rb-text) 7%, transparent);
  }

  .rail-tab.is-on {
    color: var(--rb-text);
    border-color: color-mix(in srgb, var(--accent, #da7756) 42%, transparent);
    background: color-mix(in srgb, var(--accent, #da7756) 17%, transparent);
  }

  .rail-tab:focus-visible {
    outline: none;
    border-color: color-mix(in srgb, var(--accent, #da7756) 60%, transparent);
  }

  .mono {
    font-size: 0.66rem;
    font-weight: 750;
    letter-spacing: 0.04em;
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

  .bar {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    justify-content: space-between;
    gap: 0.4rem;
    padding: 0.28rem 0.5rem 0.28rem 0.6rem;
    border-bottom: 1px solid color-mix(in srgb, var(--rb-border) 70%, transparent);
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

  .acts {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    gap: 0.2rem;
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
      border-color var(--duration-quick, 75ms) ease;
  }

  .chip:hover:not(:disabled) {
    color: var(--rb-text);
    border-color: color-mix(in srgb, var(--rb-text) 22%, transparent);
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
    width: 1.6rem;
    height: 1.6rem;
    border: 0;
    border-radius: 0.45rem;
    background: transparent;
    color: var(--rb-muted);
    cursor: pointer;
    transition:
      background-color var(--duration-quick, 75ms) ease,
      color var(--duration-quick, 75ms) ease;
  }

  .icon-btn:hover {
    color: var(--rb-text);
    background: color-mix(in srgb, var(--rb-record) 16%, transparent);
  }

  .err {
    margin: 0;
    padding: 0.25rem 0.65rem;
    border-bottom: 1px solid color-mix(in srgb, var(--rb-record) 35%, transparent);
    color: var(--rb-record);
    font-size: 0.68rem;
    line-height: 1.35;
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
    position: absolute;
    top: calc(var(--pane-y, 0%) + 0.18rem);
    left: calc(var(--pane-x, 0%) + 0.18rem);
    z-index: 0;
    width: calc(var(--pane-width, 100%) - 0.36rem);
    height: calc(var(--pane-height, 100%) - 0.36rem);
    box-sizing: border-box;
    border: 1px solid color-mix(in sRGB, var(--rb-border) 78%, transparent);
    border-radius: 0.58rem;
    padding: 0.22rem 0.42rem 0.42rem;
    overflow: hidden;
    background: var(--rb-bg0);
    cursor: text;
    transition: border-color var(--duration-fast, 150ms) ease;
  }

  .term.is-active {
    z-index: 1;
    border-color: color-mix(in sRGB, var(--agent-accent) 58%, var(--rb-border));
  }

  .term.is-hidden {
    visibility: hidden;
    pointer-events: none;
  }

  .term :global(.xterm) {
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
      opacity var(--duration-quick, 75ms) ease;
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

  .tab-add.is-ssh {
    font-size: 0.5rem;
    letter-spacing: 0.03em;
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

    border: 1px solid color-mix(in sRGB, var(--rb-border-strong) 72%, transparent);
    border-radius: 0.82rem;
    overflow: hidden;
    background: var(--rb-bg0);
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
    border-color: color-mix(in sRGB, var(--agent-accent) 48%, transparent);
    background: color-mix(in sRGB, var(--agent-accent) 13%, var(--rb-surface));
  }

  .console-desk .rail-tab .mono {
    display: grid;
    place-items: center;
    width: 1.72rem;
    height: 1.72rem;
    border-radius: 0.48rem;
    background: color-mix(in sRGB, var(--rb-text) 8%, transparent);
    color: var(--rb-text);
    font-size: 0.58rem;
  }

  .console-desk .rail-tab.is-on .mono {
    background: color-mix(in sRGB, var(--agent-accent) 22%, transparent);
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
    min-height: 2.7rem;
    padding: 0.32rem 0.52rem;
    background: var(--rb-surface);
  }

  .console-desk .bar-start,
  .console-desk .where-block {
    display: flex;
    min-width: 0;
    align-items: center;
  }

  .console-desk .bar-start {
    flex: 1;
    gap: 0.48rem;
  }

  .console-desk .where-block {
    flex-direction: column;
    align-items: flex-start;
    gap: 0.08rem;
  }

  .console-desk .back-btn,
  .console-desk .layout-toggle {
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
      border-color var(--duration-quick, 75ms) ease;
  }

  .console-desk .back-btn {
    padding: 0.14rem 0.34rem 0.14rem 0.24rem;
  }

  .console-desk .back-btn:hover,
  .layout-toggle:hover {
    border-color: color-mix(in sRGB, var(--agent-accent) 35%, transparent);
    background: color-mix(in sRGB, var(--agent-accent) 10%, transparent);
    color: var(--rb-text);
  }

  .console-desk .where {
    color: var(--rb-text);
    font-size: 0.7rem;
    font-weight: 700;
  }

  .console-desk .session-state {
    color: var(--rb-muted);
    font-size: 0.54rem;
    font-weight: 540;
  }

  .console-desk .session-state.is-live {
    color: var(--rb-ok);
  }

  .console-desk .layout-toggle {
    padding: 0.14rem 0.34rem;
  }

  .console-desk .layout-toggle:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .console-desk .layout-toggle kbd {
    border: 1px solid color-mix(in sRGB, var(--rb-border) 90%, transparent);
    border-radius: 0.3rem;
    padding: 0.05rem 0.2rem;
    background: color-mix(in sRGB, var(--rb-text) 5%, transparent);
    color: var(--rb-faint);
    font-family: var(--rb-mono, ui-monospace, monospace);
    font-size: 0.49rem;
    font-weight: 600;
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

  .console-desk .body.is-split {
    background: color-mix(in sRGB, var(--rb-bg0) 92%, var(--rb-text));
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

  @media (width <= 40rem) {
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

    .console-desk .bar {
      gap: 0.2rem;
      padding-inline: 0.45rem;
    }

    .console-desk .back-btn span,
    .console-desk .layout-toggle span {
      display: none;
    }

    .console-desk .layout-toggle {
      padding-inline: 0.35rem;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .console-desk .back-btn,
    .layout-toggle,
    .icon-btn,
    .console-desk .rail-tab {
      transition: none;
    }
  }
</style>
