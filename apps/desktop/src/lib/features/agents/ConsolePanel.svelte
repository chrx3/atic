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
    consoleClose,
    consoleOpen,
    consoleResize,
    consoleWrite,
    onConsoleExit,
    onConsoleOutput,
    sshListHosts,
  } from "$ipc/agents";
  import { getConfig } from "$ipc/config";
  import type { ConsoleKind, SshHost } from "$lib/types";
  import EmptyState from "$lib/ui/EmptyState.svelte";
  import Icon from "$ui/Icon.svelte";
  import { Plus, SquareTerminal, X } from "$lib/icons";

  let {
    remoteHost = null,
    localCwd = "",
    initialKind = "local",
    onClose,
  }: {
    /** Host SSH del destino actual de agentes; default de una pestaña nueva. */
    remoteHost?: SshHost | null;
    /** cwd local opcional al abrir PowerShell. */
    localCwd?: string;
    /** Tipo de la primera pestaña al montar. */
    initialKind?: ConsoleKind;
    onClose?: () => void;
  } = $props();

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
  };

  type Box = { term: Terminal; fit: FitAddon; el: HTMLElement };

  let tabs = $state<Tab[]>([]);
  let activeKey = $state("");
  let connecting = $state(false);
  let error = $state<string | null>(null);
  let sshHosts = $state<SshHost[]>([]);
  let ctxMenu = $state<{ x: number; y: number; key: string } | null>(null);

  /** xterm por pestaña. Fuera de `$state` (ver `Tab`). */
  const boxes = new Map<string, Box>();
  let stopListen: (() => void) | null = null;
  let seq = 0;

  const active = $derived(tabs.find((t) => t.key === activeKey) ?? null);
  const sessionId = $derived(active?.sessionId ?? null);
  const connected = $derived(!!sessionId);

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
      sshHosts.find((h) => h.id === id) ??
      (remoteHost?.id === id ? remoteHost : null)
    );
  }

  const activeHost = $derived(active ? hostById(active.hostId) : null);
  const sshLabel = $derived(activeHost ? hostLabel(activeHost) : null);

  function baseLabel(t: Tab): string {
    if (t.kind === "local") return "Local";
    const h = hostById(t.hostId);
    return h ? hostLabel(h) : "SSH";
  }

  /** Numera solo cuando el nombre se repite: "Local", "Local 2", "Local 3". */
  const tabLabels = $derived.by(() => {
    const total = new Map<string, number>();
    for (const t of tabs) {
      const b = baseLabel(t);
      total.set(b, (total.get(b) ?? 0) + 1);
    }
    const seen = new Map<string, number>();
    return tabs.map((t) => {
      const b = baseLabel(t);
      const n = (seen.get(b) ?? 0) + 1;
      seen.set(b, n);
      return (total.get(b) ?? 0) > 1 ? `${b} ${n}` : b;
    });
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
      error =
        "Elige un host SSH en la consola (o agrégalo en Ajustes → Agentes).";
      return;
    }
    connecting = true;
    await disconnect(key);
    termOf(key)?.reset();
    try {
      const term = termOf(key);
      const id = await consoleOpen({
        kind: tab.kind,
        hostId: tab.kind === "ssh" ? tab.hostId : null,
        cwd: tab.kind === "local" && localCwd.trim() ? localCwd.trim() : null,
        cols: term?.cols ?? 80,
        rows: term?.rows ?? 24,
      });
      setSession(key, id);
      requestAnimationFrame(() => {
        fitAndResize(key);
        requestOverlayKeyboard(key);
      });
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      setSession(key, null);
    } finally {
      connecting = false;
    }
  }

  /** Abre una pestaña y, si ya tiene destino, la conecta sola. */
  function newTab(kind: ConsoleKind) {
    if (tabs.length >= MAX_TABS) {
      error = `Ya hay ${MAX_TABS} consolas abiertas. Cerrá alguna para abrir otra.`;
      return;
    }
    closeCtx();
    const key = `t${++seq}`;
    const hostId =
      kind === "ssh" ? (remoteHost?.id ?? sshHosts[0]?.id ?? null) : null;
    tabs = [...tabs, { key, kind, sessionId: null, hostId }];
    activeKey = key;
    error = null;
    if (kind === "ssh") void loadSshHosts();
    // Un cuadro después el `{@attach}` ya creó el xterm, así que `fit()` mide
    // sobre el contenedor real y el PTY nace con el tamaño correcto.
    requestAnimationFrame(() => {
      if (kind === "local" || hostById(tabOf(key)?.hostId ?? null)) {
        void connect(key);
      }
    });
  }

  async function closeTab(key: string) {
    const idx = tabs.findIndex((t) => t.key === key);
    if (idx < 0) return;
    closeCtx();
    await disconnect(key);
    tabs = tabs.filter((t) => t.key !== key);
    // El xterm lo dispone el teardown del `{@attach}` al salir del DOM.
    if (activeKey === key) {
      activeKey = tabs[Math.min(idx, tabs.length - 1)]?.key ?? "";
    }
  }

  function switchTab(key: string) {
    if (activeKey === key) return;
    closeCtx();
    activeKey = key;
    error = null;
    // No se desconecta nada: las otras pestañas siguen vivas.
    requestAnimationFrame(() => {
      fitAndResize(key);
      if (sessionOf(key)) requestOverlayKeyboard(key);
    });
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

  function makeTerm(key: string): { term: Terminal; fit: FitAddon } {
    const term = new Terminal({
      cursorBlink: true,
      fontSize: 12,
      fontFamily: "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace",
      theme: {
        background: "#0f1115",
        foreground: "#e6e8ec",
        cursor: "#da7756",
        selectionBackground: "rgba(218, 119, 86, 0.35)",
      },
      allowProposedApi: false,
      rightClickSelectsWord: false,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.onData((data) => {
      const id = sessionOf(key);
      if (!id) return;
      void consoleWrite(id, data).catch(() => {});
    });
    // Ctrl/Cmd+V y Ctrl/Cmd+C (con selección): clipboard API explícita.
    term.attachCustomKeyEventHandler((ev) => {
      if (ev.type !== "keydown") return true;
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
    // OverlaySurface (capture) ya pidió text-mode; reforzar foco tras el await.
    requestAnimationFrame(() => {
      focusTerm(key);
      setTimeout(() => focusTerm(key), 40);
    });
  }

  onMount(() => {
    void loadSshHosts();
    newTab(initialKind === "ssh" ? "ssh" : "local");

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
    return () => {
      document.removeEventListener("pointerdown", onDocPointer, true);
      window.removeEventListener("blur", closeCtx);
    };
  });

  onDestroy(() => {
    stopListen?.();
    void disconnectAll();
    // Los xterm los dispone el teardown de cada `{@attach}`.
  });
</script>

<section class="console" aria-label="Consola">
  <header class="bar" data-no-drag>
    <div class="tabs" role="group" aria-label="Consolas abiertas">
      {#each tabs as t, i (t.key)}
        <span class="tab-slot">
          <button
            type="button"
            class="tab"
            class:is-on={t.key === activeKey}
            aria-current={t.key === activeKey ? "true" : undefined}
            title={tabLabels[i]}
            onclick={() => switchTab(t.key)}
          >
            {tabLabels[i]}
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
            <Icon icon={X} size={10} />
          </button>
        </span>
      {/each}
      <button
        type="button"
        class="tab-add"
        aria-label="Nueva consola local"
        title="Nueva consola local"
        disabled={connecting || tabs.length >= MAX_TABS}
        onclick={() => newTab("local")}
      >
        <Icon icon={Plus} size={12} />
      </button>
      <button
        type="button"
        class="tab-add"
        aria-label="Nueva consola SSH"
        title="Nueva consola SSH"
        disabled={connecting || tabs.length >= MAX_TABS}
        onclick={() => newTab("ssh")}
      >
        + SSH
      </button>
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
    </div>
    <div class="acts">
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

  <div class="body">
    {#if tabs.length === 0}
      <div class="empty">
        <EmptyState
          compact
          title="Sin consolas"
          hint="Abre una consola local (PowerShell) o una sesión SSH."
        >
          {#snippet action()}
            <button
              type="button"
              class="chip is-go"
              onclick={() => newTab("local")}
            >
              <Icon icon={SquareTerminal} size={12} />
              Nueva consola
            </button>
          {/snippet}
        </EmptyState>
      </div>
    {:else if active?.kind === "ssh" && !activeHost && !connected}
      <div class="empty">
        <EmptyState
          compact
          title="Sin host remoto"
          hint="Agrega un host en Ajustes → Agentes y vuelve a abrir la consola."
        />
      </div>
    {:else if !connected && !connecting}
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
            <button
              type="button"
              class="chip is-go"
              onclick={() => void connect()}
            >
              <Icon icon={SquareTerminal} size={12} />
              Conectar
            </button>
          {/snippet}
        </EmptyState>
      </div>
    {/if}

    {#each tabs as t (t.key)}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="term"
        class:is-hidden={t.key !== activeKey || (!t.sessionId && !connecting)}
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
</section>

<style>
  .console {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
    background: color-mix(in srgb, var(--rb-surface) 88%, #0f1115);
    /* Overlay: user-select/touch-action none; xterm necesita interactuar. */
    user-select: text;
    -webkit-user-select: text;
    touch-action: auto;
  }

  .bar {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    justify-content: space-between;
    gap: 0.4rem;
    padding: 0.28rem 0.55rem;
    border-bottom: 1px solid color-mix(in srgb, var(--rb-border) 70%, transparent);
  }

  .tabs {
    display: flex;
    min-width: 0;
    align-items: center;
    gap: 0.2rem;
    overflow-x: auto;
    scrollbar-width: thin;
  }

  .tab {
    display: inline-flex;
    max-width: 11rem;
    align-items: center;
    gap: 0.35rem;
    overflow: hidden;
    border: 0;
    border-radius: 0.35rem;
    padding: 0.22rem 0.45rem;
    background: transparent;
    color: var(--rb-muted);
    font: inherit;
    font-size: 0.68rem;
    font-weight: 560;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: pointer;
  }

  .tab:hover:not(:disabled) {
    color: var(--rb-text);
    background: color-mix(in srgb, var(--rb-text) 6%, transparent);
  }

  .tab.is-on {
    color: var(--rb-text);
    background: color-mix(in srgb, var(--accent, #da7756) 18%, transparent);
  }

  .tab:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .live {
    width: 0.35rem;
    height: 0.35rem;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent, #da7756) 85%, #fff);
    box-shadow: 0 0 0 1px
      color-mix(in srgb, var(--accent, #da7756) 40%, transparent);
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
    gap: 0.28rem;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 0.28rem;
    border: 1px solid color-mix(in srgb, var(--rb-border) 80%, transparent);
    border-radius: 999px;
    padding: 0.18rem 0.55rem;
    background: color-mix(in srgb, var(--rb-surface-2) 80%, transparent);
    color: var(--rb-text);
    font: inherit;
    font-size: 0.62rem;
    font-weight: 600;
    cursor: pointer;
  }

  .chip.is-go {
    border-color: color-mix(in srgb, var(--accent, #da7756) 45%, transparent);
    background: color-mix(in srgb, var(--accent, #da7756) 22%, transparent);
  }

  .chip:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .icon-btn {
    display: grid;
    place-items: center;
    width: 1.5rem;
    height: 1.5rem;
    border: 0;
    border-radius: 0.35rem;
    background: transparent;
    color: var(--rb-muted);
    cursor: pointer;
  }

  .icon-btn:hover {
    color: var(--rb-text);
    background: color-mix(in srgb, var(--rb-text) 6%, transparent);
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
    display: flex;
    flex: 1;
    min-height: 0;
    flex-direction: column;
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
    inset: 0;
    z-index: 0;
    padding: 0.2rem 0.35rem 0.35rem;
    overflow: hidden;
    cursor: text;
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
    z-index: 40;
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

  .tab-slot {
    display: inline-flex;
    flex-shrink: 0;
    align-items: center;
    border-radius: 0.35rem;
  }

  .tab-x,
  .tab-add {
    display: inline-flex;
    flex-shrink: 0;
    align-items: center;
    justify-content: center;
    border: 0;
    border-radius: 0.35rem;
    padding: 0.2rem 0.3rem;
    background: transparent;
    color: var(--rb-muted);
    font: inherit;
    font-size: 0.62rem;
    font-weight: 560;
    cursor: pointer;
  }

  /* Se revela al pasar por encima: con varias pestañas, una X siempre visible
     en cada una convierte la barra en ruido. */
  .tab-x {
    margin-left: -0.22rem;
    opacity: 0;
  }

  .tab-slot:hover .tab-x,
  .tab-x:focus-visible {
    opacity: 1;
  }

  .tab-x:hover,
  .tab-add:hover:not(:disabled) {
    color: var(--rb-text);
    background: color-mix(in srgb, var(--rb-text) 8%, transparent);
  }

  .tab-add:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
