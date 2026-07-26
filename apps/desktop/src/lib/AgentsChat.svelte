<script lang="ts">
  /**
   * La conversación con un agente. Misma vista en la ventana y en la pill.
   *
   * No guarda nada: lee de [`agents`], que sobrevive a que este componente se
   * desmonte. Cerrar el panel es dejar de mirar, no terminar la sesión — la
   * diferencia entre las dos cosas es el punto de tener agentes en la pill.
   */
  import { onMount, tick } from "svelte";
  import { agents } from "$lib/agentSessions.svelte";
  import { agentBackends } from "$lib/api";
  import type { AgentBackendInfo } from "$lib/types";

  let { compact = false }: { compact?: boolean } = $props();

  let backends = $state<AgentBackendInfo[]>([]);
  let picked = $state("");
  let starting = $state(false);
  let draft = $state("");
  let error = $state<string | null>(null);
  let logEl = $state<HTMLElement | null>(null);

  /** Cuál se está mirando. Empieza en la primera que haya. */
  let activeId = $state<string | null>(null);

  const active = $derived(agents.byId(activeId));
  const ready = $derived(
    backends.find((b) => b.id === picked)?.available ?? false,
  );

  onMount(() => {
    void agents.init();
    void (async () => {
      try {
        backends = await agentBackends();
        picked =
          backends.find((b) => b.available)?.id ?? backends[0]?.id ?? "";
      } catch (err) {
        error = String(err);
      }
    })();

    return () => {
      // Deja de mirar: a partir de acá lo que llegue cuenta como no leído y la
      // pill lo avisa.
      agents.watch(null);
    };
  });

  // Adoptar una sesión viva sin pedir permiso: si el usuario abrió el panel y
  // hay algo corriendo, eso es lo que vino a ver.
  $effect(() => {
    if (activeId && agents.byId(activeId)) return;
    activeId = agents.sessions[0]?.id ?? null;
  });

  $effect(() => {
    agents.watch(activeId);
  });

  // Seguir el final del log. Sin esto, la respuesta larga aparece arriba y hay
  // que bajar a mano justo cuando el agente todavía está escribiendo.
  $effect(() => {
    const n = active?.log.length ?? 0;
    if (!logEl || n === 0) return;
    void tick().then(() => {
      if (logEl) logEl.scrollTop = logEl.scrollHeight;
    });
  });

  async function start() {
    if (starting || !picked) return;
    starting = true;
    error = null;
    try {
      activeId = await agents.start(picked);
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

  function onKey(event: KeyboardEvent) {
    // Enter manda; Shift+Enter hace salto de línea.
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      void send();
    }
  }

  function statusLabel(id: string): string {
    const s = agents.byId(id);
    if (!s) return "";
    if (s.status === "working") return " · trabajando";
    if (s.status === "failed") return " · error";
    return s.unread > 0 ? ` · ${s.unread} nuevo${s.unread > 1 ? "s" : ""}` : "";
  }
</script>

<div class="ac" class:is-compact={compact}>
  <div class="ac-bar" data-no-drag>
    {#if agents.sessions.length > 0}
      <select class="rb-field ac-pick" bind:value={activeId}>
        {#each agents.sessions as s (s.id)}
          <option value={s.id}>{s.backendName}{statusLabel(s.id)}</option>
        {/each}
      </select>
      <button
        type="button"
        class="rb-btn rb-btn-ghost ac-mini"
        onclick={() => void start()}
        disabled={starting || !ready}
        title="Abrir otra sesión"
      >
        Nueva
      </button>
      <button
        type="button"
        class="rb-btn rb-btn-ghost ac-mini"
        onclick={() => void stop()}
      >
        Terminar
      </button>
    {:else}
      <select class="rb-field ac-pick" bind:value={picked}>
        {#each backends as b (b.id)}
          <option value={b.id} disabled={!b.available}>
            {b.displayName}{b.available ? "" : " — no instalado"}
          </option>
        {/each}
      </select>
      <button
        type="button"
        class="rb-btn rb-btn-primary ac-mini"
        onclick={() => void start()}
        disabled={starting || !ready}
      >
        {starting ? "Iniciando…" : "Iniciar"}
      </button>
    {/if}
  </div>

  {#if error || active?.error}
    <p class="ac-error" role="alert">{error ?? active?.error}</p>
  {/if}

  <div class="ac-log" role="log" bind:this={logEl} data-no-drag>
    {#if !active}
      <p class="ac-empty">
        {#if backends.length === 0}
          Buscando agentes instalados…
        {:else if !ready}
          No hay ningún agente instalado. Atic usa el que ya tengas en la
          consola, con tu sesión y tus herramientas.
        {:else}
          Inicia una sesión y sigue trabajando: el agente queda corriendo aunque
          cierres este panel, y la pill avisa cuando responde.
        {/if}
      </p>
    {:else if active.log.length === 0}
      <p class="ac-empty">Sesión lista. Escribe abajo.</p>
    {/if}

    {#each active?.log ?? [] as entry, i (i)}
      {#if entry.kind === "message"}
        <p class="ac-msg">{entry.text}</p>
      {:else if entry.kind === "toolCall"}
        <p class="ac-tool">
          <span class="ac-tool-name">{entry.name}</span>
          <span class="ac-tool-arg">{JSON.stringify(entry.input)}</span>
        </p>
      {:else if entry.kind === "toolResult"}
        <p class="ac-result" class:is-error={entry.isError}>{entry.output}</p>
      {:else if entry.kind === "started"}
        <p class="ac-meta">{entry.tools.length} herramientas · {entry.cwd}</p>
      {:else if entry.kind === "finished"}
        <p class="ac-meta">
          Fin{entry.costUsd !== null ? ` · $${entry.costUsd.toFixed(4)}` : ""}
        </p>
      {:else if entry.kind === "notice"}
        <p class="ac-meta">{entry.text}</p>
      {:else if entry.kind === "failed"}
        <p class="ac-error">{entry.message}</p>
      {/if}
    {/each}

    {#if active?.status === "working"}
      <p class="ac-meta ac-working">Trabajando…</p>
    {/if}
  </div>

  <textarea
    class="rb-field ac-input"
    bind:value={draft}
    onkeydown={onKey}
    disabled={!active}
    rows={compact ? 2 : 3}
    placeholder={active ? "Escribe y Enter para enviar…" : "Inicia una sesión primero"}
    aria-label="Mensaje para el agente"
    data-no-drag
  ></textarea>
</div>

<style>
  .ac {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    gap: 0.6rem;
  }

  .ac-bar {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .ac-pick {
    min-width: 0;
    flex: 1;
    max-width: 18rem;
  }

  .ac-log {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    gap: 0.45rem;
    border-radius: 0.6rem;
    padding: 0.75rem;
    background: var(--rb-bg0);
    overflow: auto;
  }

  .ac-empty,
  .ac-meta {
    margin: 0;
    color: var(--rb-muted);
    font-size: 0.8125rem;
  }

  .ac-empty {
    line-height: 1.45;
  }

  .ac-msg {
    margin: 0;
    white-space: pre-wrap;
  }

  /* Las llamadas a herramienta se distinguen del texto pero no gritan: son
     contexto de lo que el agente está haciendo, no la respuesta. */
  .ac-tool {
    display: flex;
    gap: 0.4rem;
    margin: 0;
    font-size: 0.8125rem;
  }

  .ac-tool-name {
    flex-shrink: 0;
    color: var(--rb-accent);
    font-weight: 650;
  }

  .ac-tool-arg {
    min-width: 0;
    overflow: hidden;
    color: var(--rb-muted);
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .ac-result {
    max-height: 9rem;
    margin: 0;
    border-left: 2px solid var(--rb-border);
    padding-left: 0.6rem;
    color: var(--rb-muted);
    font-size: 0.8125rem;
    white-space: pre-wrap;
    overflow: auto;
  }

  .ac-result.is-error {
    border-left-color: var(--rb-record);
  }

  .ac-error {
    margin: 0;
    color: var(--rb-record);
    font-size: 0.8125rem;
  }

  .ac-working {
    opacity: 0.75;
  }

  .ac-input {
    flex-shrink: 0;
    resize: none;
  }

  /* En la pill hay ~330 px de alto para todo: el chrome se achica para que lo
     que quede sea conversación. */
  .ac.is-compact {
    gap: 0.4rem;
    font-size: 0.8125rem;
  }

  .ac.is-compact .ac-log {
    padding: 0.5rem;
    gap: 0.35rem;
  }

  .ac.is-compact .ac-mini {
    padding: 0.2rem 0.5rem;
    font-size: 0.75rem;
  }

  .ac.is-compact .ac-result {
    max-height: 6rem;
  }
</style>
