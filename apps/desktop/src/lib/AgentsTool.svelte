<script lang="ts">
  /**
   * Vista mínima de agente: probar la capa de punta a punta.
   *
   * Deliberadamente simple. Lo que importa acá es que el contrato funcione —
   * arrancar, mandar, recibir eventos tipados, parar — no cómo se ve. Las
   * vistas ricas (bloques de herramienta plegables, adjuntos, diffs) vienen
   * después, y van a construirse sobre estos mismos eventos sin tocar Rust.
   */
  import { onMount } from "svelte";
  import {
    agentBackends,
    agentSend,
    agentStart,
    agentStop,
    onAgentEvent,
  } from "$lib/api";
  import type { AgentBackendInfo, AgentEventPayload } from "$lib/types";

  let backends = $state<AgentBackendInfo[]>([]);
  let selected = $state("");
  let session = $state<string | null>(null);
  let starting = $state(false);
  let draft = $state("");
  let error = $state<string | null>(null);

  /** Todo lo recibido, en orden. Es el registro crudo de la conversación. */
  let log = $state<AgentEventPayload[]>([]);

  const running = $derived(session !== null);
  const ready = $derived(
    backends.find((b) => b.id === selected)?.available ?? false,
  );

  onMount(() => {
    void (async () => {
      try {
        backends = await agentBackends();
        selected = backends.find((b) => b.available)?.id ?? backends[0]?.id ?? "";
      } catch (err) {
        error = String(err);
      }
    })();

    const unlisten = onAgentEvent((payload) => {
      // Puede haber varias sesiones abiertas; esta vista mira una sola.
      if (payload.session !== session) return;
      log = [...log, payload];
      if (payload.kind === "failed") error = payload.message;
    });

    return () => {
      void unlisten.then((fn) => fn());
      // No se corta la sesión al desmontar: el agente puede estar a mitad de
      // una tarea larga y salir de la pestaña no es pedirle que pare.
    };
  });

  async function start() {
    if (starting || running) return;
    starting = true;
    error = null;
    log = [];
    try {
      session = await agentStart(selected);
    } catch (err) {
      error = String(err);
    } finally {
      starting = false;
    }
  }

  async function send() {
    const text = draft.trim();
    if (!text || !session) return;
    draft = "";
    try {
      await agentSend(session, text);
    } catch (err) {
      error = String(err);
    }
  }

  async function stop() {
    if (!session) return;
    const id = session;
    session = null;
    try {
      await agentStop(id);
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
</script>

<div class="ag">
  <div class="ag-bar">
    <select class="rb-field ag-pick" bind:value={selected} disabled={running}>
      {#each backends as b (b.id)}
        <option value={b.id} disabled={!b.available}>
          {b.displayName}{b.available ? "" : " — no instalado"}
        </option>
      {/each}
    </select>

    {#if running}
      <button type="button" class="rb-btn rb-btn-ghost" onclick={() => void stop()}>
        Terminar
      </button>
    {:else}
      <button
        type="button"
        class="rb-btn rb-btn-primary"
        onclick={() => void start()}
        disabled={starting || !ready}
      >
        {starting ? "Iniciando…" : "Iniciar"}
      </button>
    {/if}
  </div>

  {#if error}
    <p class="ag-error" role="alert">{error}</p>
  {/if}

  <div class="ag-log" role="log">
    {#if log.length === 0}
      <p class="ag-empty">
        {running
          ? "Sesión lista. Escribí abajo."
          : "Elegí un agente e iniciá una sesión."}
      </p>
    {/if}
    {#each log as entry, i (i)}
      {#if entry.kind === "message"}
        <p class="ag-msg">{entry.text}</p>
      {:else if entry.kind === "toolCall"}
        <p class="ag-tool">
          <span class="ag-tool-name">{entry.name}</span>
          <span class="ag-tool-arg">{JSON.stringify(entry.input)}</span>
        </p>
      {:else if entry.kind === "toolResult"}
        <p class="ag-result" class:is-error={entry.isError}>{entry.output}</p>
      {:else if entry.kind === "started"}
        <p class="ag-meta">Sesión {entry.sessionId} · {entry.tools.length} herramientas</p>
      {:else if entry.kind === "finished"}
        <p class="ag-meta">
          Fin{entry.costUsd !== null ? ` · $${entry.costUsd.toFixed(4)}` : ""}
        </p>
      {:else if entry.kind === "notice"}
        <p class="ag-meta">{entry.text}</p>
      {:else if entry.kind === "failed"}
        <p class="ag-error">{entry.message}</p>
      {/if}
    {/each}
  </div>

  <textarea
    class="rb-field ag-input"
    bind:value={draft}
    onkeydown={onKey}
    disabled={!running}
    rows="3"
    placeholder={running ? "Escribí y Enter para enviar…" : "Iniciá una sesión primero"}
    aria-label="Mensaje para el agente"
  ></textarea>
</div>

<style>
  .ag {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    gap: 0.6rem;
  }

  .ag-bar {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .ag-pick {
    max-width: 18rem;
  }

  .ag-log {
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

  .ag-empty,
  .ag-meta {
    margin: 0;
    color: var(--rb-muted);
    font-size: 0.8125rem;
  }

  .ag-msg {
    margin: 0;
    white-space: pre-wrap;
  }

  /* Las llamadas a herramienta se distinguen del texto pero no gritan: son
     contexto de lo que el agente está haciendo, no la respuesta. */
  .ag-tool {
    display: flex;
    gap: 0.4rem;
    margin: 0;
    font-size: 0.8125rem;
  }

  .ag-tool-name {
    flex-shrink: 0;
    color: var(--rb-accent);
    font-weight: 650;
  }

  .ag-tool-arg {
    min-width: 0;
    overflow: hidden;
    color: var(--rb-muted);
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .ag-result {
    max-height: 9rem;
    margin: 0;
    border-left: 2px solid var(--rb-border);
    padding-left: 0.6rem;
    color: var(--rb-muted);
    font-size: 0.8125rem;
    white-space: pre-wrap;
    overflow: auto;
  }

  .ag-result.is-error {
    border-left-color: var(--rb-record);
  }

  .ag-error {
    margin: 0;
    color: var(--rb-record);
    font-size: 0.8125rem;
  }

  .ag-input {
    flex-shrink: 0;
    resize: none;
  }
</style>
