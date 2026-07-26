<script lang="ts">
  /**
   * La consola de agentes: ventana propia, no un panel de la pill.
   *
   * Por qué ventana: acá se lee salida larga, se revisa qué tocó una
   * herramienta y se aprueba o se niega. Todo eso necesita espacio y quedarse
   * abierto — lo contrario de lo que hace la pill, que aparece, resuelve una
   * cosa y se va. La pill queda como avisador: te dice que pasó algo y te trae
   * acá.
   *
   * El estado NO vive en esta página. Vive en `agents`, que escucha desde que
   * arranca la app: cerrar la ventana es dejar de mirar, no terminar la sesión.
   */
  import { onMount, tick } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { agents } from "$lib/agentSessions.svelte";
  import { agentBackends, getConfig } from "$lib/api";
  import { applyTheme, readCachedTheme } from "$lib/theme";
  import AticMark from "$lib/AticMark.svelte";
  import ToolIcon from "$lib/ToolIcon.svelte";
  import McpServersModal from "$lib/McpServersModal.svelte";
  import type { AgentBackendInfo, McpServerConfig } from "$lib/types";

  /**
   * Modelos ofrecidos.
   *
   * Alias y no nombres completos: el alias siempre apunta al último de esa
   * familia, así que la lista no envejece con cada release. «Por defecto» deja
   * decidir al CLI, que es lo correcto cuando ya lo configuraste allá.
   */
  const MODELS = [
    { id: "", label: "El de tu CLI" },
    { id: "opus", label: "Opus" },
    { id: "sonnet", label: "Sonnet" },
    { id: "haiku", label: "Haiku" },
  ];

  /**
   * Cuánto se pregunta antes de actuar.
   *
   * `manual` es el default a propósito: una interfaz gráfica con alguien
   * mirando es justo el caso donde preguntar cuesta poco y equivocarse cuesta
   * caro. Los modos permisivos existen porque para tareas largas parar en cada
   * archivo no es viable, pero elegirlos es una decisión consciente.
   */
  const MODES = [
    { id: "manual", label: "Preguntar siempre" },
    { id: "acceptEdits", label: "Aceptar ediciones" },
    { id: "plan", label: "Solo planificar" },
    { id: "bypassPermissions", label: "No preguntar nada" },
  ];

  /** Ventana de contexto de referencia para la barra. */
  const CONTEXT_WINDOW = 1_000_000;

  let backends = $state<AgentBackendInfo[]>([]);
  let picked = $state("");
  let model = $state("");
  let mode = $state("manual");
  let cwd = $state("");
  let starting = $state(false);
  let error = $state<string | null>(null);
  let draft = $state("");
  let logEl = $state<HTMLElement | null>(null);
  let inputEl = $state<HTMLTextAreaElement | null>(null);
  let mcpOpen = $state(false);
  let mcpServers = $state<McpServerConfig[]>([]);

  let activeId = $state<string | null>(null);
  const active = $derived(agents.byId(activeId));
  const ready = $derived(
    backends.find((b) => b.id === picked)?.available ?? false,
  );
  const enabledMcp = $derived(mcpServers.filter((s) => s.enabled));

  onMount(() => {
    applyTheme(readCachedTheme());
    void agents.init();

    void (async () => {
      try {
        backends = await agentBackends();
        picked = backends.find((b) => b.available)?.id ?? backends[0]?.id ?? "";
      } catch (err) {
        error = String(err);
      }
      try {
        const cfg = await getConfig();
        mcpServers = parseMcp(cfg.agent_mcp_servers);
      } catch {
        // Sin config, se arranca sin servidores extra.
      }
    })();

    return () => agents.watch(null);
  });

  function parseMcp(raw: string | undefined): McpServerConfig[] {
    if (!raw) return [];
    try {
      const parsed = JSON.parse(raw);
      return Array.isArray(parsed) ? parsed : [];
    } catch {
      return [];
    }
  }

  $effect(() => {
    if (activeId && agents.byId(activeId)) return;
    activeId = agents.sessions[0]?.id ?? null;
  });

  $effect(() => {
    agents.watch(activeId);
  });

  // Seguir el final de la conversación. Sin esto la respuesta larga aparece
  // arriba y hay que bajar a mano justo cuando el agente sigue escribiendo.
  $effect(() => {
    const n = active?.log.length ?? 0;
    if (!logEl || n === 0) return;
    void tick().then(() => {
      if (logEl) logEl.scrollTop = logEl.scrollHeight;
    });
  });

  /**
   * Los servidores habilitados, en el formato que espera el CLI.
   *
   * Cada uno guarda su JSON como string para poder pegarlo tal cual desde la
   * documentación del servidor; acá se arma el objeto que va en `--mcp-config`.
   */
  function mcpConfig(): string | undefined {
    if (enabledMcp.length === 0) return undefined;
    const servers: Record<string, unknown> = {};
    for (const server of enabledMcp) {
      try {
        servers[server.name] = JSON.parse(server.json);
      } catch {
        // Un servidor con JSON roto se salta: mejor arrancar sin él que no
        // arrancar. El aviso ya está en la pantalla donde se edita.
      }
    }
    return JSON.stringify({ mcpServers: servers });
  }

  async function pickFolder() {
    try {
      const chosen = await openDialog({ directory: true, multiple: false });
      if (typeof chosen === "string") cwd = chosen;
    } catch (err) {
      error = String(err);
    }
  }

  async function attach() {
    try {
      const chosen = await openDialog({ multiple: true });
      const paths = Array.isArray(chosen) ? chosen : chosen ? [chosen] : [];
      if (paths.length === 0) return;
      // Se adjunta la RUTA, no el contenido: el agente ya sabe leer archivos y
      // tiene permiso sobre su directorio. Volcar el archivo entero en el
      // mensaje gastaría contexto en algo que él puede abrir cuando le sirva.
      draft = [draft.trim(), ...paths].filter(Boolean).join("\n");
      inputEl?.focus();
    } catch (err) {
      error = String(err);
    }
  }

  async function start() {
    if (starting || !picked) return;
    starting = true;
    error = null;
    try {
      activeId = await agents.start(picked, {
        cwd: cwd || undefined,
        model: model || undefined,
        permissionMode: mode,
        mcpConfig: mcpConfig(),
      });
    } catch (err) {
      error = String(err);
    } finally {
      starting = false;
    }
  }

  async function send() {
    const text = draft.trim();
    if (!text || !activeId) return;
    draft = "";
    try {
      await agents.send(activeId, text);
    } catch (err) {
      error = String(err);
    }
  }

  async function stop() {
    if (!activeId) return;
    try {
      await agents.stop(activeId);
    } catch (err) {
      error = String(err);
    }
  }

  async function decide(permissionId: string, allow: boolean) {
    if (!activeId) return;
    try {
      await agents.decide(activeId, permissionId, allow);
    } catch (err) {
      error = String(err);
    }
  }

  function onKey(event: KeyboardEvent) {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      void send();
    }
  }

  function shortNumber(n: number): string {
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1_000) return `${Math.round(n / 1000)}k`;
    return String(n);
  }

  /** El argumento más informativo de una herramienta, para el encabezado. */
  function toolSummary(input: unknown): string {
    if (!input || typeof input !== "object") return "";
    const o = input as Record<string, unknown>;
    for (const key of ["file_path", "command", "pattern", "path", "url", "prompt"]) {
      const value = o[key];
      if (typeof value === "string") return value;
    }
    return JSON.stringify(o);
  }

  const win = () => {
    try {
      return getCurrentWindow();
    } catch {
      return null;
    }
  };
</script>

<div class="cons">
  <header class="cons-bar" data-tauri-drag-region>
    <span class="cons-mark"><AticMark size={15} strokeWidth={1.5} /></span>
    <span class="cons-title" data-tauri-drag-region>Agentes</span>

    {#if agents.sessions.length > 0}
      <div class="cons-tabs" role="tablist" aria-label="Sesiones">
        {#each agents.sessions as s (s.id)}
          <button
            type="button"
            role="tab"
            class="cons-tab"
            class:active={s.id === activeId}
            class:is-waiting={s.pending.length > 0}
            aria-selected={s.id === activeId}
            onclick={() => (activeId = s.id)}
          >
            {s.backendName}
            {#if s.pending.length > 0}
              <span class="cons-dot" title="Espera tu permiso"></span>
            {:else if s.status === "working"}
              <span class="cons-dot is-busy" title="Trabajando"></span>
            {:else if s.unread > 0}
              <span class="cons-dot is-new" title="{s.unread} sin leer"></span>
            {/if}
          </button>
        {/each}
      </div>
    {/if}

    <div class="cons-win">
      <button
        type="button"
        class="rb-icon-btn"
        onclick={() => void win()?.minimize()}
        aria-label="Minimizar">–</button
      >
      <button
        type="button"
        class="rb-icon-btn"
        onclick={() => void win()?.hide()}
        aria-label="Cerrar">×</button
      >
    </div>
  </header>

  {#if !active}
    <!-- Sin sesión: la pantalla es la configuración de arranque. Estas
         opciones solo se pueden fijar al empezar, así que no tiene sentido
         esconderlas detrás de un botón. -->
    <div class="cons-setup">
      <h2 class="cons-h">Nueva sesión</h2>
      <p class="cons-sub">
        Atic no reemplaza a tu agente: lanza el que ya tienes instalado, con tu
        sesión, tus herramientas y tus skills. Solo le pone una interfaz.
      </p>

      <label class="rb-label">
        Agente
        <select class="rb-field" bind:value={picked}>
          {#each backends as b (b.id)}
            <option value={b.id} disabled={!b.available}>
              {b.displayName}{b.available ? "" : " — no instalado"}
            </option>
          {/each}
        </select>
      </label>

      <div class="cons-grid">
        <label class="rb-label">
          Modelo
          <select class="rb-field" bind:value={model}>
            {#each MODELS as m (m.id)}
              <option value={m.id}>{m.label}</option>
            {/each}
          </select>
        </label>

        <label class="rb-label">
          Permisos
          <select class="rb-field" bind:value={mode}>
            {#each MODES as m (m.id)}
              <option value={m.id}>{m.label}</option>
            {/each}
          </select>
        </label>
      </div>

      <label class="rb-label">
        Carpeta de trabajo
        <span class="cons-row">
          <input
            class="rb-field"
            bind:value={cwd}
            placeholder="La del proyecto que quieras que toque"
          />
          <button type="button" class="rb-btn rb-btn-ghost" onclick={() => void pickFolder()}>
            Elegir
          </button>
        </span>
      </label>

      <div class="cons-row cons-mcp">
        <span class="rb-hint">
          {enabledMcp.length === 0
            ? "Sin servidores MCP extra."
            : `${enabledMcp.length} servidor(es) MCP se sumarán a esta sesión.`}
        </span>
        <button type="button" class="rb-btn rb-btn-ghost" onclick={() => (mcpOpen = true)}>
          Servidores MCP
        </button>
      </div>

      {#if error}
        <p class="cons-error" role="alert">{error}</p>
      {/if}

      <button
        type="button"
        class="rb-btn rb-btn-primary cons-go"
        onclick={() => void start()}
        disabled={starting || !ready}
      >
        {starting ? "Iniciando…" : "Iniciar sesión"}
      </button>

      {#if backends.length > 0 && !ready}
        <p class="rb-hint">
          No se encontró el ejecutable. Instálalo y ábrelo una vez en la consola
          para iniciar sesión; Atic usa esa misma cuenta.
        </p>
      {/if}
    </div>
  {:else}
    <div class="cons-log" bind:this={logEl} role="log">
      {#each active.log as entry, i (i)}
        {#if entry.kind === "message"}
          <p class="cons-msg">{entry.text}</p>
        {:else if entry.kind === "toolCall"}
          <div class="cons-tool">
            <span class="cons-tool-name">{entry.name}</span>
            <span class="cons-tool-arg">{toolSummary(entry.input)}</span>
          </div>
        {:else if entry.kind === "toolResult"}
          <pre class="cons-out" class:is-error={entry.isError}>{entry.output}</pre>
        {:else if entry.kind === "started"}
          <p class="cons-meta">
            {entry.model} · {entry.cwd} · {entry.tools.length} herramientas{entry
              .mcpServers.length > 0
              ? ` · MCP: ${entry.mcpServers.map((s) => s.name).join(", ")}`
              : ""}
          </p>
        {:else if entry.kind === "finished"}
          <p class="cons-meta">
            fin del turno{entry.costUsd !== null
              ? ` · $${entry.costUsd.toFixed(4)}`
              : ""}
          </p>
        {:else if entry.kind === "notice"}
          <p class="cons-meta">{entry.text}</p>
        {:else if entry.kind === "failed"}
          <p class="cons-error">{entry.message}</p>
        {/if}
      {/each}

      {#if active.status === "working"}
        <p class="cons-meta cons-live">trabajando…</p>
      {/if}
    </div>

    <!-- El permiso va abajo, pegado al compositor: es donde están los ojos
         cuando el agente está trabajando, y es una decisión, no una línea más
         del registro. -->
    {#each active.pending as p (p.id)}
      <div class="cons-perm" role="alertdialog" aria-label="Permiso pendiente">
        <div class="cons-perm-copy">
          <p class="cons-perm-title">
            Quiere usar <strong>{p.tool}</strong>
          </p>
          <p class="cons-perm-what">{p.description || toolSummary(p.input)}</p>
        </div>
        <div class="cons-perm-acts">
          <button
            type="button"
            class="rb-btn rb-btn-ghost"
            onclick={() => void decide(p.id, false)}>Denegar</button
          >
          <button
            type="button"
            class="rb-btn rb-btn-primary"
            onclick={() => void decide(p.id, true)}>Permitir</button
          >
        </div>
      </div>
    {/each}

    {#if error || active.error}
      <p class="cons-error cons-error-bar" role="alert">{error ?? active.error}</p>
    {/if}

    <div class="cons-compose">
      <textarea
        class="cons-input"
        bind:this={inputEl}
        bind:value={draft}
        onkeydown={onKey}
        rows="3"
        placeholder="Escribe y Enter para enviar · Shift+Enter para salto de línea"
        aria-label="Mensaje para el agente"
      ></textarea>

      <div class="cons-actions">
        <button
          type="button"
          class="cons-act"
          onclick={() => void attach()}
          title="Adjuntar archivos (se envía la ruta)"
          aria-label="Adjuntar archivos"
        >
          <ToolIcon id="captures" size={15} strokeWidth={1.5} />
        </button>

        <!-- El contexto es el recurso que se agota sin avisar: por eso está
             siempre visible y no detrás de un comando. -->
        <span class="cons-ctx" title="Contexto usado">
          <span class="cons-ctx-track">
            <span
              class="cons-ctx-fill"
              style="width: {Math.min(
                100,
                (active.contextTokens / CONTEXT_WINDOW) * 100,
              )}%"
            ></span>
          </span>
          {shortNumber(active.contextTokens)}
        </span>

        {#if active.costUsd > 0}
          <span class="cons-cost" title="Costo de la sesión">
            ${active.costUsd.toFixed(3)}
          </span>
        {/if}

        <span class="cons-spacer"></span>

        <button type="button" class="rb-btn rb-btn-ghost" onclick={() => void stop()}>
          Terminar
        </button>
        <button
          type="button"
          class="rb-btn rb-btn-primary"
          onclick={() => void send()}
          disabled={!draft.trim()}
        >
          Enviar
        </button>
      </div>
    </div>
  {/if}
</div>

{#if mcpOpen}
  <McpServersModal
    servers={mcpServers}
    onSave={(next) => {
      mcpServers = next;
      mcpOpen = false;
    }}
    onClose={() => (mcpOpen = false)}
  />
{/if}

<style>
  :global(html),
  :global(body) {
    margin: 0;
    height: 100%;
    background: var(--rb-bg0);
  }

  .cons {
    display: flex;
    width: 100vw;
    height: 100vh;
    box-sizing: border-box;
    flex-direction: column;
    background: var(--rb-bg0);
    color: var(--rb-text);
    overflow: hidden;
  }

  .cons-bar {
    display: flex;
    height: 2.4rem;
    flex-shrink: 0;
    align-items: center;
    gap: 0.5rem;
    border-bottom: 1px solid var(--rb-border);
    padding: 0 0.4rem 0 0.7rem;
    background: var(--rb-surface);
  }

  .cons-mark {
    display: flex;
    color: var(--rb-muted);
  }

  .cons-title {
    color: var(--rb-muted);
    font-size: 0.75rem;
    font-weight: 600;
    letter-spacing: 0.04em;
  }

  .cons-tabs {
    display: flex;
    min-width: 0;
    gap: 0.25rem;
    overflow-x: auto;
  }

  .cons-tab {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    border: 0;
    border-radius: 0.4rem;
    padding: 0.2rem 0.5rem;
    background: transparent;
    color: var(--rb-muted);
    font-size: 0.75rem;
    white-space: nowrap;
    cursor: pointer;
  }
  .cons-tab.active {
    background: var(--rb-bg0);
    color: var(--rb-text);
  }
  .cons-tab.is-waiting {
    color: var(--rb-record);
  }

  .cons-dot {
    width: 0.4rem;
    height: 0.4rem;
    border-radius: 999px;
    background: var(--rb-record);
  }
  .cons-dot.is-busy {
    background: var(--rb-muted);
    animation: cons-pulse 1.6s ease-in-out infinite;
  }
  .cons-dot.is-new {
    background: var(--rb-accent);
  }

  @keyframes cons-pulse {
    0%,
    100% {
      opacity: 0.4;
    }
    50% {
      opacity: 1;
    }
  }

  .cons-win {
    display: flex;
    margin-left: auto;
    gap: 0.15rem;
  }

  /* ─── Arranque ──────────────────────────────────────────────────────── */
  .cons-setup {
    display: flex;
    max-width: 34rem;
    flex-direction: column;
    gap: 0.85rem;
    margin: 0 auto;
    padding: 2rem 1.5rem;
    overflow: auto;
  }

  .cons-h {
    margin: 0;
    font-size: 1rem;
    font-weight: 650;
  }

  .cons-sub {
    margin: 0;
    color: var(--rb-muted);
    font-size: 0.8125rem;
    line-height: 1.5;
  }

  .cons-grid {
    display: grid;
    gap: 0.85rem;
    grid-template-columns: 1fr 1fr;
  }

  .cons-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .cons-row .rb-field {
    min-width: 0;
    flex: 1;
  }

  .cons-mcp {
    justify-content: space-between;
    border-top: 1px solid var(--rb-border);
    padding-top: 0.85rem;
  }

  .cons-go {
    margin-top: 0.4rem;
  }

  /* ─── Conversación ──────────────────────────────────────────────────── */
  .cons-log {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    gap: 0.6rem;
    padding: 1rem 1.1rem;
    overflow: auto;
  }

  .cons-msg {
    margin: 0;
    font-size: 0.875rem;
    line-height: 1.55;
    white-space: pre-wrap;
  }

  .cons-meta {
    margin: 0;
    color: var(--rb-faint);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.6875rem;
  }

  .cons-live {
    animation: cons-pulse 1.6s ease-in-out infinite;
  }

  /* La herramienta se lee como una línea de consola: nombre y argumento. Es
     lo que se escanea cuando el agente hizo veinte cosas y buscas una. */
  .cons-tool {
    display: flex;
    gap: 0.5rem;
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.75rem;
  }

  .cons-tool-name {
    flex-shrink: 0;
    color: var(--rb-accent);
    font-weight: 650;
  }

  .cons-tool-arg {
    min-width: 0;
    overflow: hidden;
    color: var(--rb-muted);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .cons-out {
    max-height: 14rem;
    margin: 0;
    border-left: 2px solid var(--rb-border);
    padding: 0 0 0 0.7rem;
    color: var(--rb-muted);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.75rem;
    line-height: 1.5;
    white-space: pre-wrap;
    overflow: auto;
  }
  .cons-out.is-error {
    border-left-color: var(--rb-record);
  }

  .cons-error {
    margin: 0;
    color: var(--rb-record);
    font-size: 0.8125rem;
  }
  .cons-error-bar {
    flex-shrink: 0;
    padding: 0 1.1rem 0.5rem;
  }

  /* ─── Permiso ───────────────────────────────────────────────────────── */
  .cons-perm {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    gap: 1rem;
    border-top: 1px solid var(--rb-record);
    padding: 0.7rem 1.1rem;
    background: color-mix(in srgb, var(--rb-record) 10%, transparent);
  }

  .cons-perm-copy {
    min-width: 0;
    flex: 1;
  }

  .cons-perm-title {
    margin: 0;
    font-size: 0.8125rem;
  }

  .cons-perm-what {
    margin: 0.15rem 0 0;
    color: var(--rb-muted);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.75rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .cons-perm-acts {
    display: flex;
    flex-shrink: 0;
    gap: 0.4rem;
  }

  /* ─── Compositor ────────────────────────────────────────────────────── */
  .cons-compose {
    display: flex;
    flex-shrink: 0;
    flex-direction: column;
    gap: 0.5rem;
    border-top: 1px solid var(--rb-border);
    padding: 0.7rem 1.1rem 0.8rem;
    background: var(--rb-surface);
  }

  .cons-input {
    width: 100%;
    box-sizing: border-box;
    border: 1px solid var(--rb-border);
    border-radius: 0.5rem;
    padding: 0.5rem 0.6rem;
    background: var(--rb-bg0);
    color: var(--rb-text);
    font-family: inherit;
    font-size: 0.8125rem;
    line-height: 1.5;
    resize: none;
  }
  .cons-input:focus {
    outline: 2px solid var(--rb-accent);
    outline-offset: -1px;
  }

  .cons-actions {
    display: flex;
    align-items: center;
    gap: 0.6rem;
  }

  .cons-act {
    display: inline-flex;
    align-items: center;
    border: 0;
    border-radius: 0.4rem;
    padding: 0.3rem;
    background: transparent;
    color: var(--rb-muted);
    cursor: pointer;
  }
  .cons-act:hover {
    color: var(--rb-text);
  }

  .cons-ctx {
    display: inline-flex;
    align-items: center;
    gap: 0.4rem;
    color: var(--rb-faint);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.6875rem;
  }

  .cons-ctx-track {
    display: block;
    width: 3.5rem;
    height: 0.25rem;
    border-radius: 999px;
    background: var(--rb-border);
    overflow: hidden;
  }

  .cons-ctx-fill {
    display: block;
    height: 100%;
    background: var(--rb-accent);
  }

  .cons-cost {
    color: var(--rb-faint);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.6875rem;
  }

  .cons-spacer {
    flex: 1;
  }
</style>
