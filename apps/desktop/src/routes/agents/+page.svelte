<script lang="ts">
  /**
   * La consola de agentes: una burbuja que sale de la pill.
   *
   * # Por qué burbuja y no ventana suelta
   *
   * Una ventana más sería una app aparte que además tiene una pill. La punta
   * apuntando a la pill dice que es la misma cosa desplegada, y hace obvio de
   * dónde salió y adónde vuelve al cerrarse. Rust decide de qué lado va la
   * punta (es quien ve los monitores) y esta vista solo la dibuja.
   *
   * # De dónde sale el aspecto
   *
   * De dos sitios, a propósito:
   *
   *  - **La consola de Claude Code** para el registro: monoespaciada, fondo
   *    casi negro cálido, un acento coral, cajas con el título incrustado en el
   *    borde y avisos con barra vertical. Quien ya usa el CLI reconoce lo que
   *    está mirando; inventar un lenguaje propio acá solo habría hecho que
   *    hubiera que aprender dos.
   *  - **El compositor de las GUI de agentes (T3 Code y parecidas)** para lo de
   *    abajo: caja redondeada, controles en pastillas con su valor a la vista
   *    —modelo, permisos, carpeta—, anillo de contexto y botón circular de
   *    enviar. Un `<select>` de formulario ahí abajo rompía el tono y encima
   *    escondía el valor actual, que es lo que uno mira antes de mandar.
   *
   * El estado no vive acá: vive en `agents`, que escucha desde que arranca la
   * app. Cerrar la burbuja es dejar de mirar, no terminar la sesión.
   */
  import { onMount, tick } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { agents } from "$lib/agentSessions.svelte";
  import { agentBackends, getConfig, onAgentsBubbleAnchor } from "$lib/api";
  import { applyTheme, readCachedTheme } from "$lib/theme";
  import McpServersModal from "$lib/McpServersModal.svelte";
  import PickerMenu from "$lib/PickerMenu.svelte";
  import type { AgentBackendInfo, McpServerConfig } from "$lib/types";

  /** Alias y no nombres completos: el alias sigue apuntando al último de la
   *  familia, así que la lista no envejece con cada release. */
  const MODELS = [
    { id: "", label: "Modelo del CLI" },
    { id: "opus", label: "Opus" },
    { id: "sonnet", label: "Sonnet" },
    { id: "haiku", label: "Haiku" },
  ];

  /** `manual` primero a propósito: con alguien mirando, preguntar cuesta poco
   *  y equivocarse cuesta caro. Los permisivos existen para tareas largas. */
  const MODES = [
    { id: "manual", label: "Preguntar siempre" },
    { id: "acceptEdits", label: "Aceptar ediciones" },
    { id: "plan", label: "Solo planificar" },
    { id: "bypassPermissions", label: "Acceso total" },
  ];

  const CONTEXT_WINDOW = 1_000_000;

  let anchor = $state<{ side: string; offset: number } | null>(null);
  let shown = $state(false);

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
  let menu = $state<"model" | "mode" | "agent" | null>(null);

  let activeId = $state<string | null>(null);
  const active = $derived(agents.byId(activeId));
  const ready = $derived(
    backends.find((b) => b.id === picked)?.available ?? false,
  );
  const enabledMcp = $derived(mcpServers.filter((s) => s.enabled));
  const modelLabel = $derived(
    MODELS.find((m) => m.id === model)?.label ?? "Modelo",
  );
  const modeLabel = $derived(MODES.find((m) => m.id === mode)?.label ?? "Permisos");
  const ctxPct = $derived(
    Math.min(100, ((active?.contextTokens ?? 0) / CONTEXT_WINDOW) * 100),
  );

  onMount(() => {
    applyTheme(readCachedTheme());
    void agents.init();

    // La burbuja no se pinta hasta saber dónde va la punta: al abrirse ya tiene
    // que estar bien, no acomodarse a la vista.
    const un = onAgentsBubbleAnchor((a) => {
      anchor = a;
      shown = false;
      void tick().then(() => requestAnimationFrame(() => (shown = true)));
    });

    void (async () => {
      try {
        backends = await agentBackends();
        picked = backends.find((b) => b.available)?.id ?? backends[0]?.id ?? "";
      } catch (err) {
        error = String(err);
      }
      try {
        mcpServers = parseMcp((await getConfig()).agent_mcp_servers);
      } catch {
        // Sin config se arranca sin servidores extra.
      }
    })();

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !mcpOpen && !menu) {
        event.preventDefault();
        void close();
      }
    };
    window.addEventListener("keydown", onKeyDown);

    return () => {
      window.removeEventListener("keydown", onKeyDown);
      void un.then((fn) => fn());
      agents.watch(null);
    };
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

  $effect(() => {
    const n = active?.log.length ?? 0;
    if (!logEl || n === 0) return;
    void tick().then(() => {
      if (logEl) logEl.scrollTop = logEl.scrollHeight;
    });
  });

  /** Cierra con la animación puesta: ocultar en seco delataría que es una
   *  ventana y no una burbuja que se repliega sobre la pill. */
  async function close() {
    shown = false;
    await new Promise((r) => setTimeout(r, 140));
    try {
      await getCurrentWindow().hide();
    } catch {
      // Sin ventana nativa (preview web) no hay nada que ocultar.
    }
  }

  function mcpConfig(): string | undefined {
    if (enabledMcp.length === 0) return undefined;
    const servers: Record<string, unknown> = {};
    for (const server of enabledMcp) {
      try {
        servers[server.name] = JSON.parse(server.json);
      } catch {
        // Un servidor con JSON roto se salta: mejor arrancar sin él que no
        // arrancar. El aviso ya está donde se edita.
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
      // Se manda la RUTA, no el contenido: el agente sabe leer archivos, y
      // volcarlos acá gastaría contexto en algo que él abre cuando le sirva.
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
      if (active) void send();
      else void start();
    }
  }

  function shortNumber(n: number): string {
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1_000) return `${Math.round(n / 1000)}k`;
    return String(n);
  }

  /** El argumento más informativo de una herramienta. Es lo que se escanea
   *  cuando el agente hizo veinte cosas y buscas una. */
  function toolSummary(input: unknown): string {
    if (!input || typeof input !== "object") return "";
    const o = input as Record<string, unknown>;
    for (const key of ["file_path", "command", "pattern", "path", "url", "prompt"]) {
      const value = o[key];
      if (typeof value === "string") return value;
    }
    return JSON.stringify(o);
  }
</script>

<div
  class="bub"
  class:is-shown={shown}
  data-side={anchor?.side ?? "top"}
  style="--tail: {anchor?.offset ?? 40}px"
>
  <span class="bub-tail" aria-hidden="true"></span>

  <div class="bub-body">
    <!-- Cabecera al estilo de la consola: el título vive incrustado en el
         borde de la caja, no en una barra aparte. -->
    <header class="hdr" data-tauri-drag-region>
      <span class="hdr-name" data-tauri-drag-region>Agentes</span>

      {#if agents.sessions.length > 0}
        <div class="tabs" role="tablist" aria-label="Sesiones">
          {#each agents.sessions as s (s.id)}
            <button
              type="button"
              role="tab"
              class="tab"
              class:active={s.id === activeId}
              aria-selected={s.id === activeId}
              onclick={() => (activeId = s.id)}
            >
              {#if s.pending.length > 0}
                <span class="dot is-wait"></span>
              {:else if s.status === "working"}
                <span class="dot is-busy"></span>
              {:else if s.unread > 0}
                <span class="dot is-new"></span>
              {:else}
                <span class="dot"></span>
              {/if}
              {s.backendName}
            </button>
          {/each}
        </div>
      {/if}

      <button
        type="button"
        class="hdr-x"
        onclick={() => void close()}
        aria-label="Cerrar">×</button
      >
    </header>

    <div class="log" bind:this={logEl} role="log">
      {#if !active}
        <!-- Caja de bienvenida con el título en el borde, como el CLI. -->
        <section class="card">
          <h2 class="card-title">Atic · agentes</h2>
          <p class="card-line">
            Lanza el agente que ya tienes instalado, con tu sesión, tus
            herramientas y tus skills. Atic solo le pone cara.
          </p>
          <p class="card-line dim">
            Elige abajo el agente, el modelo y cuánto quieres que pregunte.
            Escribe y pulsa Enter para empezar.
          </p>
        </section>

        {#if backends.length > 0 && !ready}
          <p class="warn">
            No se encontró el ejecutable. Instálalo y ábrelo una vez en la
            consola para iniciar sesión; Atic usa esa misma cuenta.
          </p>
        {/if}
      {:else}
        {#each active.log as entry, i (i)}
          {#if entry.kind === "message"}
            <p class="msg">{entry.text}</p>
          {:else if entry.kind === "toolCall"}
            <p class="tool">
              <span class="tool-mark">⏺</span>
              <span class="tool-name">{entry.name}</span>
              <span class="tool-arg">{toolSummary(entry.input)}</span>
            </p>
          {:else if entry.kind === "toolResult"}
            <pre class="out" class:is-error={entry.isError}>{entry.output}</pre>
          {:else if entry.kind === "started"}
            <section class="card">
              <h2 class="card-title">{entry.model || "sesión"}</h2>
              <p class="card-line dim">{entry.cwd}</p>
              <p class="card-line dim">
                {entry.tools.length} herramientas · {entry.slashCommands.length}
                comandos{entry.mcpServers.length > 0
                  ? ` · MCP: ${entry.mcpServers.map((s) => s.name).join(", ")}`
                  : ""}
              </p>
            </section>
          {:else if entry.kind === "finished"}
            <p class="meta">
              ─ fin del turno{entry.costUsd !== null
                ? ` · $${entry.costUsd.toFixed(4)}`
                : ""}
            </p>
          {:else if entry.kind === "notice"}
            <p class="notice">{entry.text}</p>
          {:else if entry.kind === "failed"}
            <p class="warn">{entry.message}</p>
          {/if}
        {/each}

        {#if active.status === "working"}
          <p class="meta live">⏺ trabajando…</p>
        {/if}
      {/if}
    </div>

    <!-- El permiso va pegado al compositor: ahí están los ojos mientras el
         agente trabaja, y es una decisión, no una línea más del registro. -->
    {#each active?.pending ?? [] as p (p.id)}
      <div class="perm" role="alertdialog" aria-label="Permiso pendiente">
        <div class="perm-copy">
          <p class="perm-t">Quiere usar <strong>{p.tool}</strong></p>
          <p class="perm-w">{p.description || toolSummary(p.input)}</p>
        </div>
        <div class="perm-acts">
          <button type="button" class="btn" onclick={() => void decide(p.id, false)}>
            Denegar
          </button>
          <button
            type="button"
            class="btn is-go"
            onclick={() => void decide(p.id, true)}
          >
            Permitir
          </button>
        </div>
      </div>
    {/each}

    {#if error || active?.error}
      <p class="warn warn-bar" role="alert">{error ?? active?.error}</p>
    {/if}

    <div class="cmp">
      <div class="cmp-box">
        <textarea
          class="cmp-in"
          bind:this={inputEl}
          bind:value={draft}
          onkeydown={onKey}
          rows="2"
          placeholder={active
            ? "Escribe · Enter envía, Shift+Enter salta línea"
            : "Describe lo que quieres y Enter para empezar…"}
          aria-label="Mensaje para el agente"
        ></textarea>

        <div class="cmp-row">
          {#if !active}
            <!-- Estas tres solo se pueden fijar al arrancar, así que están a la
                 vista con su valor puesto y no escondidas en un ajuste. -->
            <PickerMenu
              label={backends.find((b) => b.id === picked)?.displayName ??
                "Agente"}
              open={menu === "agent"}
              options={backends.map((b) => ({
                id: b.id,
                label: b.displayName,
                disabled: !b.available,
              }))}
              value={picked}
              onToggle={() => (menu = menu === "agent" ? null : "agent")}
              onPick={(id) => {
                picked = id;
                menu = null;
              }}
            />
            <PickerMenu
              label={modelLabel}
              open={menu === "model"}
              options={MODELS}
              value={model}
              onToggle={() => (menu = menu === "model" ? null : "model")}
              onPick={(id) => {
                model = id;
                menu = null;
              }}
            />
            <PickerMenu
              label={modeLabel}
              open={menu === "mode"}
              options={MODES}
              value={mode}
              onToggle={() => (menu = menu === "mode" ? null : "mode")}
              onPick={(id) => {
                mode = id;
                menu = null;
              }}
            />
            <button type="button" class="chip" onclick={() => void pickFolder()}>
              {cwd ? cwd.split(/[\\/]/).pop() : "Carpeta"}
            </button>
            <button type="button" class="chip" onclick={() => (mcpOpen = true)}>
              MCP{enabledMcp.length > 0 ? ` · ${enabledMcp.length}` : ""}
            </button>
          {:else}
            <span class="chip is-static">{active.model || active.backendName}</span>
            <span class="chip is-static">{modeLabel}</span>
            <button type="button" class="chip" onclick={() => void attach()}>
              Adjuntar
            </button>
            <button type="button" class="chip" onclick={() => void stop()}>
              Terminar
            </button>
          {/if}

          <span class="cmp-gap"></span>

          {#if active}
            <!-- El contexto es el recurso que se agota sin avisar: anillo
                 siempre visible, no un comando que haya que recordar. -->
            <span
              class="ring"
              title="Contexto: {shortNumber(active.contextTokens)} tokens"
              style="--pct: {ctxPct}"
            >
              <span class="ring-n">{shortNumber(active.contextTokens)}</span>
            </span>
            {#if active.costUsd > 0}
              <span class="cost">${active.costUsd.toFixed(3)}</span>
            {/if}
          {/if}

          <button
            type="button"
            class="go"
            onclick={() => (active ? void send() : void start())}
            disabled={starting || (!active && !ready) || (!!active && !draft.trim())}
            aria-label={active ? "Enviar" : "Iniciar sesión"}
          >
            <svg viewBox="0 0 24 24" width="15" height="15" aria-hidden="true">
              <path
                d="M12 19V5M5 12l7-7 7 7"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
          </button>
        </div>
      </div>
    </div>
  </div>
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
    background: transparent;
    overflow: hidden;
  }

  /* ─── La burbuja ────────────────────────────────────────────────────────
   *
   * El hueco para la punta se reserva con padding en el lado que apunta, así
   * el cuerpo nunca la tapa y la sombra la envuelve entera.
   */
  .bub {
    --coral: #d97757;
    --ink: #17151400;
    --shell: #1c1917;
    --line: #332e2b;
    --text: #e7e2dd;
    --dim: #8d827a;
    --faint: #6b615a;

    position: relative;
    display: flex;
    width: 100vw;
    height: 100vh;
    box-sizing: border-box;
    padding: 14px;

    opacity: 0;
    transform: scale(0.94);
    transition:
      opacity 140ms ease,
      transform 180ms cubic-bezier(0.34, 1.3, 0.64, 1);
  }
  .bub.is-shown {
    opacity: 1;
    transform: scale(1);
  }

  /* Crece DESDE la punta: es lo que hace que se lea como desplegarse de la
     pill y no como una ventana que apareció encima. */
  .bub[data-side="top"] {
    transform-origin: var(--tail) 0;
    padding-top: 14px;
    padding-bottom: 0;
  }
  .bub[data-side="bottom"] {
    transform-origin: var(--tail) 100%;
    padding-top: 0;
    padding-bottom: 14px;
  }
  .bub[data-side="left"] {
    transform-origin: 0 var(--tail);
    padding-left: 14px;
    padding-right: 0;
  }
  .bub[data-side="right"] {
    transform-origin: 100% var(--tail);
    padding-left: 0;
    padding-right: 14px;
  }

  @media (prefers-reduced-motion: reduce) {
    .bub {
      transition: opacity 100ms linear;
      transform: none;
    }
    .bub.is-shown {
      transform: none;
    }
  }

  .bub-body {
    display: flex;
    min-width: 0;
    flex: 1;
    flex-direction: column;
    border: 1px solid var(--line);
    border-radius: 18px;
    background: var(--shell);
    box-shadow: 0 18px 48px rgb(0 0 0 / 45%);
    color: var(--text);
    overflow: hidden;
  }

  /* La punta: un cuadrado girado 45°, con el mismo borde y fondo que el
     cuerpo. Un triángulo con `border` no puede llevar borde propio, y sin él
     se ve como un pegote suelto al lado de la burbuja. */
  .bub-tail {
    position: absolute;
    width: 16px;
    height: 16px;
    border: 1px solid var(--line);
    background: var(--shell);
    transform: rotate(45deg);
  }
  .bub[data-side="top"] .bub-tail {
    top: 7px;
    left: calc(var(--tail) - 8px);
    border-right: 0;
    border-bottom: 0;
    border-top-left-radius: 4px;
  }
  .bub[data-side="bottom"] .bub-tail {
    bottom: 7px;
    left: calc(var(--tail) - 8px);
    border-left: 0;
    border-top: 0;
    border-bottom-right-radius: 4px;
  }
  .bub[data-side="left"] .bub-tail {
    top: calc(var(--tail) - 8px);
    left: 7px;
    border-top: 0;
    border-right: 0;
    border-bottom-left-radius: 4px;
  }
  .bub[data-side="right"] .bub-tail {
    top: calc(var(--tail) - 8px);
    right: 7px;
    border-bottom: 0;
    border-left: 0;
    border-top-right-radius: 4px;
  }

  /* ─── Cabecera ──────────────────────────────────────────────────────── */
  .hdr {
    display: flex;
    height: 2.1rem;
    flex-shrink: 0;
    align-items: center;
    gap: 0.5rem;
    padding: 0 0.5rem 0 0.9rem;
  }

  .hdr-name {
    color: var(--coral);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.75rem;
  }

  .tabs {
    display: flex;
    min-width: 0;
    gap: 0.2rem;
    overflow-x: auto;
    scrollbar-width: none;
  }

  .tab {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    border: 0;
    border-radius: 999px;
    padding: 0.15rem 0.55rem;
    background: transparent;
    color: var(--dim);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.6875rem;
    white-space: nowrap;
    cursor: pointer;
  }
  .tab.active {
    background: #2a2522;
    color: var(--text);
  }

  .dot {
    width: 0.35rem;
    height: 0.35rem;
    border-radius: 999px;
    background: var(--faint);
  }
  .dot.is-wait {
    background: var(--coral);
  }
  .dot.is-busy {
    background: var(--dim);
    animation: pulse 1.6s ease-in-out infinite;
  }
  .dot.is-new {
    background: #7dd3a0;
  }

  @keyframes pulse {
    0%,
    100% {
      opacity: 0.35;
    }
    50% {
      opacity: 1;
    }
  }

  .hdr-x {
    margin-left: auto;
    border: 0;
    border-radius: 0.35rem;
    padding: 0.1rem 0.45rem;
    background: transparent;
    color: var(--faint);
    font-size: 1rem;
    line-height: 1;
    cursor: pointer;
  }
  .hdr-x:hover {
    color: var(--text);
  }

  /* ─── Registro ──────────────────────────────────────────────────────── */
  .log {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    gap: 0.6rem;
    padding: 0.35rem 0.95rem 0.9rem;
    overflow: auto;
  }

  /* Caja con el título incrustado en el borde: el gesto que define la consola
     de Claude Code y lo que hace que un bloque se lea como una unidad. */
  .card {
    position: relative;
    margin-top: 0.5rem;
    border: 1px solid var(--line);
    border-radius: 10px;
    padding: 0.75rem 0.85rem 0.7rem;
  }

  .card-title {
    position: absolute;
    top: -0.55rem;
    left: 0.7rem;
    margin: 0;
    padding: 0 0.35rem;
    background: var(--shell);
    color: var(--coral);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.6875rem;
    font-weight: 400;
  }

  .card-line {
    margin: 0 0 0.35rem;
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.75rem;
    line-height: 1.55;
  }
  .card-line:last-child {
    margin-bottom: 0;
  }
  .card-line.dim {
    color: var(--dim);
  }

  .msg {
    margin: 0;
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.78125rem;
    line-height: 1.6;
    white-space: pre-wrap;
  }

  .tool {
    display: flex;
    gap: 0.45rem;
    margin: 0;
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.75rem;
  }
  .tool-mark {
    color: var(--coral);
  }
  .tool-name {
    flex-shrink: 0;
    color: var(--text);
  }
  .tool-arg {
    min-width: 0;
    overflow: hidden;
    color: var(--dim);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Salida de herramienta con barra a la izquierda, como los avisos del CLI:
     se distingue del texto del agente sin gritar. */
  .out {
    max-height: 12rem;
    margin: 0 0 0 0.35rem;
    border-left: 2px solid var(--line);
    padding: 0 0 0 0.7rem;
    color: var(--dim);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.71875rem;
    line-height: 1.5;
    white-space: pre-wrap;
    overflow: auto;
  }
  .out.is-error {
    border-left-color: var(--coral);
  }

  .meta,
  .notice {
    margin: 0;
    color: var(--faint);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.6875rem;
  }
  .notice {
    border-left: 2px solid var(--line);
    padding-left: 0.7rem;
  }
  .live {
    animation: pulse 1.6s ease-in-out infinite;
  }

  .warn {
    margin: 0;
    color: var(--coral);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.71875rem;
    line-height: 1.5;
  }
  .warn-bar {
    flex-shrink: 0;
    padding: 0 0.95rem 0.5rem;
  }

  /* ─── Permiso ───────────────────────────────────────────────────────── */
  .perm {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    gap: 0.85rem;
    border-top: 1px solid color-mix(in srgb, var(--coral) 45%, transparent);
    padding: 0.6rem 0.95rem;
    background: color-mix(in srgb, var(--coral) 12%, transparent);
  }
  .perm-copy {
    min-width: 0;
    flex: 1;
  }
  .perm-t {
    margin: 0;
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.75rem;
  }
  .perm-w {
    margin: 0.1rem 0 0;
    overflow: hidden;
    color: var(--dim);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.6875rem;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .perm-acts {
    display: flex;
    flex-shrink: 0;
    gap: 0.35rem;
  }

  .btn {
    border: 1px solid var(--line);
    border-radius: 999px;
    padding: 0.25rem 0.7rem;
    background: transparent;
    color: var(--text);
    font-family: inherit;
    font-size: 0.71875rem;
    cursor: pointer;
  }
  .btn.is-go {
    border-color: var(--coral);
    background: var(--coral);
    color: #1c1917;
    font-weight: 600;
  }

  /* ─── Compositor ────────────────────────────────────────────────────── */
  .cmp {
    flex-shrink: 0;
    padding: 0 0.7rem 0.7rem;
  }

  .cmp-box {
    border: 1px solid var(--line);
    border-radius: 14px;
    padding: 0.55rem 0.6rem 0.5rem;
    background: #211d1b;
  }
  .cmp-box:focus-within {
    border-color: color-mix(in srgb, var(--coral) 55%, var(--line));
  }

  .cmp-in {
    display: block;
    width: 100%;
    box-sizing: border-box;
    border: 0;
    padding: 0.1rem 0.15rem 0.4rem;
    background: transparent;
    color: var(--text);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.78125rem;
    line-height: 1.55;
    resize: none;
  }
  .cmp-in:focus {
    outline: none;
  }
  .cmp-in::placeholder {
    color: var(--faint);
  }

  .cmp-row {
    display: flex;
    align-items: center;
    gap: 0.3rem;
  }

  .cmp-gap {
    flex: 1;
  }

  /* Pastilla con el valor puesto. Un `<select>` acá escondía el valor actual,
     que es justo lo que uno mira antes de mandar. */
  .chip {
    border: 1px solid var(--line);
    border-radius: 999px;
    padding: 0.2rem 0.6rem;
    background: transparent;
    color: var(--dim);
    font-family: inherit;
    font-size: 0.6875rem;
    max-width: 10rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: pointer;
  }
  .chip:hover {
    color: var(--text);
  }
  .chip.is-static {
    cursor: default;
  }

  /* Anillo de contexto: el relleno es un cono cónico recortado con máscara. */
  .ring {
    position: relative;
    display: inline-flex;
    width: 1.75rem;
    height: 1.75rem;
    flex-shrink: 0;
    align-items: center;
    justify-content: center;
    border-radius: 999px;
    background: conic-gradient(
      var(--coral) calc(var(--pct) * 1%),
      #35302c 0
    );
  }
  .ring::after {
    position: absolute;
    border-radius: 999px;
    background: #211d1b;
    content: "";
    inset: 2px;
  }
  .ring-n {
    position: relative;
    z-index: 1;
    color: var(--dim);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.5625rem;
  }

  .cost {
    color: var(--faint);
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 0.625rem;
  }

  .go {
    display: inline-flex;
    width: 1.75rem;
    height: 1.75rem;
    flex-shrink: 0;
    align-items: center;
    justify-content: center;
    border: 0;
    border-radius: 999px;
    background: var(--coral);
    color: #1c1917;
    cursor: pointer;
  }
  .go:disabled {
    background: #35302c;
    color: var(--faint);
    cursor: default;
  }
</style>
