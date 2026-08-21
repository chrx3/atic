<script lang="ts">
  /**
   * Consola embebida (xterm + PTY): Local (PowerShell) o SSH (`ssh -t`).
   * Dos sesiones concurrentes (local + ssh); cambiar de tab no mata el PTY.
   */
  import { onDestroy, onMount } from "svelte";
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
  import { SquareTerminal, X } from "$lib/icons";

  let {
    remoteHost = null,
    localCwd = "",
    initialKind = "local",
    onClose,
  }: {
    /** Host SSH del destino actual de agentes (tab SSH); default del picker. */
    remoteHost?: SshHost | null;
    /** cwd local opcional al abrir PowerShell. */
    localCwd?: string;
    /** Tab inicial al montar el panel. */
    initialKind?: ConsoleKind;
    onClose?: () => void;
  } = $props();

  let tab = $state<ConsoleKind>("local");
  let connecting = $state(false);
  let error = $state<string | null>(null);
  let sshHosts = $state<SshHost[]>([]);
  /** Host elegido en la consola (independiente del destino del chat). */
  let pickedHostId = $state<string | null>(null);
  let ctxMenu = $state<{ x: number; y: number; kind: ConsoleKind } | null>(
    null,
  );

  let localHostEl = $state<HTMLElement | null>(null);
  let sshHostEl = $state<HTMLElement | null>(null);
  let localSessionId = $state<string | null>(null);
  let sshSessionId = $state<string | null>(null);

  let localTerm: Terminal | null = null;
  let sshTerm: Terminal | null = null;
  let localFit: FitAddon | null = null;
  let sshFit: FitAddon | null = null;

  let stopListen: (() => void) | null = null;

  const sessionId = $derived(tab === "local" ? localSessionId : sshSessionId);
  const connected = $derived(!!sessionId);
  const activeHost = $derived.by(() => {
    if (pickedHostId) {
      return (
        sshHosts.find((h) => h.id === pickedHostId) ??
        (remoteHost?.id === pickedHostId ? remoteHost : null)
      );
    }
    return remoteHost ?? sshHosts[0] ?? null;
  });
  const sshLabel = $derived(
    activeHost
      ? activeHost.label ||
          (activeHost.user
            ? `${activeHost.user}@${activeHost.host}`
            : activeHost.host)
      : null,
  );

  function hostOptionLabel(h: SshHost): string {
    const name = h.label?.trim() || h.host;
    if (h.user?.trim()) return `${name} (${h.user}@${h.host})`;
    return name;
  }

  function termOf(kind: ConsoleKind): Terminal | null {
    return kind === "local" ? localTerm : sshTerm;
  }

  function fitOf(kind: ConsoleKind): FitAddon | null {
    return kind === "local" ? localFit : sshFit;
  }

  function hostOf(kind: ConsoleKind): HTMLElement | null {
    return kind === "local" ? localHostEl : sshHostEl;
  }

  function sessionOf(kind: ConsoleKind): string | null {
    return kind === "local" ? localSessionId : sshSessionId;
  }

  function setSession(kind: ConsoleKind, id: string | null) {
    if (kind === "local") localSessionId = id;
    else sshSessionId = id;
  }

  function kindForSession(id: string): ConsoleKind | null {
    if (localSessionId === id) return "local";
    if (sshSessionId === id) return "ssh";
    return null;
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
    if (
      pickedHostId &&
      !sshHosts.some((h) => h.id === pickedHostId) &&
      remoteHost?.id !== pickedHostId
    ) {
      pickedHostId = remoteHost?.id ?? sshHosts[0]?.id ?? null;
    }
    if (!pickedHostId) {
      pickedHostId = remoteHost?.id ?? sshHosts[0]?.id ?? null;
    }
  }

  /** Solo enfoca xterm (el overlay text-mode lo pide OverlaySurface al clic). */
  function focusTerm(kind: ConsoleKind = tab) {
    termOf(kind)?.focus();
  }

  /**
   * Tras Conectar / switch: dispara el mismo camino que un clic en el term
   * para que OverlaySurface active set_overlay_text_mode.
   */
  function requestOverlayKeyboard(kind: ConsoleKind = tab) {
    const host = hostOf(kind);
    if (!host) {
      focusTerm(kind);
      return;
    }
    host.dispatchEvent(
      new PointerEvent("pointerdown", { bubbles: true, cancelable: true }),
    );
    // Tras el await de text-mode en OverlaySurface.
    requestAnimationFrame(() => {
      focusTerm(kind);
      setTimeout(() => focusTerm(kind), 40);
    });
  }

  function closeCtx() {
    ctxMenu = null;
  }

  async function disconnect(kind: ConsoleKind = tab) {
    const id = sessionOf(kind);
    setSession(kind, null);
    if (id) {
      try {
        await consoleClose(id);
      } catch {
        /* ya cerró */
      }
    }
  }

  async function disconnectAll() {
    await Promise.all([disconnect("local"), disconnect("ssh")]);
  }

  async function connect() {
    if (connecting) return;
    error = null;
    if (tab === "ssh" && !activeHost) {
      error =
        "Elige un host SSH en la consola (o agrégalo en Ajustes → Agentes).";
      return;
    }
    connecting = true;
    const kind = tab;
    await disconnect(kind);
    termOf(kind)?.reset();
    try {
      const term = termOf(kind);
      const cols = term?.cols ?? 80;
      const rows = term?.rows ?? 24;
      const id = await consoleOpen({
        kind,
        hostId: kind === "ssh" ? activeHost?.id : null,
        cwd: kind === "local" && localCwd.trim() ? localCwd.trim() : null,
        cols,
        rows,
      });
      setSession(kind, id);
      requestAnimationFrame(() => {
        fitAndResize(kind);
        requestOverlayKeyboard(kind);
      });
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      setSession(kind, null);
    } finally {
      connecting = false;
    }
  }

  function switchTab(next: ConsoleKind) {
    if (tab === next) return;
    closeCtx();
    tab = next;
    error = null;
    if (next === "ssh") void loadSshHosts();
    // No disconnect: la otra sesión sigue viva.
    requestAnimationFrame(() => {
      fitAndResize(next);
      if (sessionOf(next)) requestOverlayKeyboard(next);
    });
  }

  function fitAndResize(kind: ConsoleKind = tab) {
    const fit = fitOf(kind);
    const term = termOf(kind);
    const host = hostOf(kind);
    if (!fit || !term || !host) return;
    try {
      fit.fit();
    } catch {
      return;
    }
    const id = sessionOf(kind);
    if (id) {
      void consoleResize(id, term.cols, term.rows).catch(() => {});
    }
  }

  function makeTerm(kind: ConsoleKind): { term: Terminal; fit: FitAddon } {
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
      const id = sessionOf(kind);
      if (!id) return;
      void consoleWrite(id, data).catch(() => {});
    });
    // Ctrl/Cmd+V y Ctrl/Cmd+C (con selección): clipboard API explícita.
    term.attachCustomKeyEventHandler((ev) => {
      if (ev.type !== "keydown") return true;
      const mod = ev.ctrlKey || ev.metaKey;
      if (mod && (ev.key === "v" || ev.key === "V")) {
        void pasteInto(kind);
        return false;
      }
      if (mod && (ev.key === "c" || ev.key === "C") && term.hasSelection()) {
        void copyFrom(kind);
        return false;
      }
      return true;
    });
    return { term, fit };
  }

  function ensureTerm(kind: ConsoleKind, host: HTMLElement) {
    if (kind === "local") {
      if (localTerm) return;
      const { term, fit } = makeTerm("local");
      localTerm = term;
      localFit = fit;
      term.open(host);
    } else {
      if (sshTerm) return;
      const { term, fit } = makeTerm("ssh");
      sshTerm = term;
      sshFit = fit;
      term.open(host);
    }
    try {
      fitOf(kind)?.fit();
    } catch {
      /* layout aún no listo */
    }
  }

  async function copyFrom(kind: ConsoleKind = tab) {
    const text = termOf(kind)?.getSelection() ?? "";
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      /* sin permiso */
    }
  }

  async function pasteInto(kind: ConsoleKind = tab) {
    const term = termOf(kind);
    if (!term || !sessionOf(kind)) return;
    try {
      const text = await navigator.clipboard.readText();
      if (text) term.paste(text);
    } catch {
      /* sin permiso / vacío */
    }
  }

  function onTermContextMenu(kind: ConsoleKind, e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    focusTerm(kind);
    ctxMenu = { x: e.clientX, y: e.clientY, kind };
  }

  function onTermPointerDown(kind: ConsoleKind) {
    closeCtx();
    // OverlaySurface (capture) ya pidió text-mode; reforzar foco tras el await.
    requestAnimationFrame(() => {
      focusTerm(kind);
      setTimeout(() => focusTerm(kind), 40);
    });
  }

  onMount(() => {
    tab = initialKind === "ssh" ? "ssh" : "local";
    pickedHostId = remoteHost?.id ?? null;
    void loadSshHosts();

    void Promise.all([
      onConsoleOutput((p) => {
        const kind = kindForSession(p.session);
        if (!kind) return;
        termOf(kind)?.write(p.data);
      }),
      onConsoleExit((p) => {
        const kind = kindForSession(p.session);
        if (!kind) return;
        setSession(kind, null);
        const code = p.code == null ? "?" : String(p.code);
        termOf(kind)?.writeln(`\r\n[sesión terminada · exit ${code}]`);
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
    localTerm?.dispose();
    sshTerm?.dispose();
    localTerm = null;
    sshTerm = null;
    localFit = null;
    sshFit = null;
  });

  $effect(() => {
    const kind = tab;
    const localEl = localHostEl;
    const sshEl = sshHostEl;
    if (localEl) ensureTerm("local", localEl);
    if (sshEl) ensureTerm("ssh", sshEl);
    const observer = new ResizeObserver(() => fitAndResize(kind));
    if (localEl) observer.observe(localEl);
    if (sshEl) observer.observe(sshEl);
    requestAnimationFrame(() => fitAndResize(kind));
    return () => observer.disconnect();
  });
</script>

<section class="console" aria-label="Consola">
  <header class="bar" data-no-drag>
    <div class="tabs" role="tablist" aria-label="Destino de consola">
      <button
        type="button"
        class="tab"
        class:is-on={tab === "local"}
        role="tab"
        aria-selected={tab === "local"}
        disabled={connecting}
        onclick={() => switchTab("local")}
      >
        Local
        {#if localSessionId}
          <span class="live" title="Sesión activa" aria-hidden="true"></span>
        {/if}
      </button>
      <button
        type="button"
        class="tab"
        class:is-on={tab === "ssh"}
        role="tab"
        aria-selected={tab === "ssh"}
        disabled={connecting}
        title={sshLabel ? `SSH · ${sshLabel}` : "SSH"}
        onclick={() => switchTab("ssh")}
      >
        SSH
        {#if sshSessionId}
          <span class="live" title="Sesión activa" aria-hidden="true"></span>
        {/if}
      </button>
      {#if tab === "ssh"}
        <label class="host-pick">
          <span class="sr">Host SSH</span>
          <select
            class="host-select"
            aria-label="Host SSH"
            disabled={connecting || !!sshSessionId || sshHosts.length === 0}
            value={pickedHostId ?? ""}
            onchange={(e) => {
              const v = (e.currentTarget as HTMLSelectElement).value;
              pickedHostId = v || null;
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
          disabled={connecting || (tab === "ssh" && !activeHost)}
          onclick={() => void connect()}
        >
          {connecting ? "Conectando…" : "Conectar"}
        </button>
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
    {#if tab === "ssh" && !activeHost && !connected}
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
          title={tab === "local" ? "Consola local" : `SSH · ${sshLabel ?? "remoto"}`}
          hint={tab === "local"
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

    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="term"
      class:is-hidden={tab !== "local" || (!localSessionId && !connecting)}
      bind:this={localHostEl}
      data-no-drag
      data-selectable
      data-console-term
      onpointerdown={() => onTermPointerDown("local")}
      oncontextmenu={(e) => onTermContextMenu("local", e)}
    ></div>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="term"
      class:is-hidden={tab !== "ssh" || (!sshSessionId && !connecting)}
      bind:this={sshHostEl}
      data-no-drag
      data-selectable
      data-console-term
      onpointerdown={() => onTermPointerDown("ssh")}
      oncontextmenu={(e) => onTermContextMenu("ssh", e)}
    ></div>

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
          disabled={!termOf(ctxMenu.kind)?.hasSelection()}
          onclick={() => {
            const k = ctxMenu!.kind;
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
          disabled={!sessionOf(ctxMenu.kind)}
          onclick={() => {
            const k = ctxMenu!.kind;
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
</style>
