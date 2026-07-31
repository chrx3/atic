<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import AgentConversation from "$lib/AgentConversation.svelte";
  import { agents, type AgentStatus } from "$lib/agentSessions.svelte";
  import { showAgentsWindow } from "$lib/api";
  import type { PermissionDecision } from "$lib/types";

  let selectedId = $state<string | null>(null);
  let draft = $state("");
  let sending = $state(false);
  let localError = $state<string | null>(null);

  const active = $derived(agents.byId(selectedId) ?? agents.sessions[0]);
  const items = $derived((active?.turns ?? []).flatMap((turn) => turn.items));
  const turnEnds = $derived(
    new Map(
      (active?.turns ?? [])
        .filter((turn) => turn.status !== "running")
        .map((turn) => [turn.items.at(-1)?.id ?? turn.id, turn.costUsd]),
    ),
  );

  onMount(() => {
    void agents.init({ notify: true });
  });

  $effect(() => {
    const activeId = active?.id ?? null;
    agents.watch(activeId);
  });

  onDestroy(() => agents.watch(null));

  function autoScroll(node: HTMLDivElement) {
    const scroll = () => requestAnimationFrame(() => (node.scrollTop = node.scrollHeight));
    const observer = new MutationObserver(scroll);
    observer.observe(node, { childList: true, subtree: true, characterData: true });
    scroll();
    return { destroy: () => observer.disconnect() };
  }

  async function send() {
    const text = draft.trim();
    if (!active || !text || sending || active.status === "working") return;
    draft = "";
    sending = true;
    localError = null;
    try {
      await agents.send(active.id, text, { via: "pill" });
    } catch (cause) {
      draft = text;
      localError = String(cause);
    } finally {
      sending = false;
    }
  }

  async function decide(permissionId: string, decision: PermissionDecision) {
    if (!active) return;
    localError = null;
    try {
      await agents.decide(active.id, permissionId, decision);
    } catch (cause) {
      localError = String(cause);
    }
  }

  function stateLabel(status: AgentStatus) {
    if (status === "working") return "trabajando";
    if (status === "waiting") return "espera permiso";
    if (status === "failed") return "con error";
    return "lista";
  }
</script>

<div class="agent-workspace">
  <aside aria-label="Sesiones de agentes">
    <div class="session-heading">
      <span>SESIONES</span>
      <button type="button" aria-label="Abrir consola completa de agentes" onclick={() => void showAgentsWindow()}>↗</button>
    </div>

    {#if agents.sessions.length > 0}
      <div class="session-list">
        {#each agents.sessions as session (session.id)}
          <button
            class:current={session.id === active?.id}
            type="button"
            onclick={() => (selectedId = session.id)}
          >
            <span class="session-name">{session.backendName}</span>
            <span class:waiting={session.pending.length > 0} class="session-state">
              {session.unread > 0 ? `${session.unread} nueva${session.unread === 1 ? "" : "s"}` : stateLabel(session.status)}
            </span>
          </button>
        {/each}
      </div>
    {:else}
      <p class="no-sessions">No hay agentes activos.</p>
      <button class="open-console" type="button" onclick={() => void showAgentsWindow()}>Iniciar en consola</button>
    {/if}
  </aside>

  <section class="conversation-pane" aria-label={active ? `Conversación con ${active.backendName}` : "Agentes"}>
    {#if active}
      <header>
        <span class="agent-avatar">{active.backendName.slice(0, 1).toUpperCase()}</span>
        <span class="agent-title">
          <strong>{active.backendName}</strong>
          <small>{stateLabel(active.status)}{active.model ? ` · ${active.model}` : ""}</small>
        </span>
        <i class:working={active.status === "working"} class:waiting={active.pending.length > 0}></i>
      </header>

      <div use:autoScroll class="conversation-log">
        {#if items.length > 0}
          <AgentConversation {items} {turnEnds} />
        {:else}
          <div class="empty-conversation">
            <strong>Sesión lista</strong>
            <p>Continúa aquí sin salir de la superficie flotante.</p>
          </div>
        {/if}
      </div>

      <div class="permission-stack">
        {#each active.pending as permission (permission.id)}
          <div class="permission" role="alert">
            <span><strong>{permission.tool}</strong>{permission.description ? ` · ${permission.description}` : ""}</span>
            <div>
              <button type="button" onclick={() => void decide(permission.id, "deny")}>Denegar</button>
              <button class="allow" type="button" onclick={() => void decide(permission.id, "allow")}>Permitir</button>
            </div>
          </div>
        {/each}
      </div>

      <form class="composer" onsubmit={(event) => { event.preventDefault(); void send(); }}>
        <textarea
          bind:value={draft}
          aria-label="Mensaje para el agente"
          placeholder={active.status === "working" ? "El agente está trabajando…" : "Escribe un mensaje…"}
          disabled={active.status === "working" || sending}
          rows="1"
          onkeydown={(event) => {
            if (event.key === "Enter" && !event.shiftKey) {
              event.preventDefault();
              void send();
            }
          }}
        ></textarea>
        <button type="submit" disabled={!draft.trim() || active.status === "working" || sending} aria-label="Enviar mensaje">↑</button>
      </form>

      {#if localError || active.error}<p class="agent-error" role="status">{localError ?? active.error}</p>{/if}
    {:else}
      <div class="empty-conversation">
        <strong>Tu agente, dentro del flujo</strong>
        <p>Inicia una sesión en la consola completa; aparecerá aquí y seguirá trabajando aunque cierres la pill.</p>
        <button class="open-console" type="button" onclick={() => void showAgentsWindow()}>Abrir consola</button>
      </div>
    {/if}
  </section>
</div>

<style>
  .agent-workspace {
    --coral: var(--signal);
    --text: var(--ink);
    --dim: var(--muted);
    --faint: color-mix(in oklch, var(--muted) 72%, transparent);
    --line: var(--soft-line);
    --card: color-mix(in oklch, white 48%, transparent);
    --code: color-mix(in oklch, var(--pearl-deep) 56%, white);
    --hover: color-mix(in oklch, var(--pearl-deep) 72%, transparent);
    --add: oklch(0.56 0.13 145);
    --del: oklch(0.58 0.17 28);
    display: grid;
    height: 100%;
    grid-template-columns: 166px minmax(0, 1fr);
    overflow: hidden;
  }

  aside {
    min-width: 0;
    padding: 13px 10px;
    border-right: 1px solid var(--soft-line);
    background: color-mix(in oklch, var(--pearl-deep) 58%, transparent);
  }

  .session-heading {
    display: flex;
    padding: 0 4px 10px;
    align-items: center;
    justify-content: space-between;
    color: var(--muted);
    font-size: 8px;
    font-weight: 720;
    letter-spacing: 0.12em;
  }

  .session-heading button {
    width: 25px;
    height: 25px;
    border: 1px solid var(--soft-line);
    border-radius: 8px;
    color: var(--muted);
    background: color-mix(in oklch, white 46%, transparent);
    cursor: pointer;
  }

  .session-list { display: flex; gap: 6px; flex-direction: column; }
  .session-list > button {
    display: flex;
    min-width: 0;
    padding: 9px;
    flex-direction: column;
    align-items: flex-start;
    border: 1px solid transparent;
    border-radius: 11px;
    color: inherit;
    background: transparent;
    text-align: left;
    cursor: pointer;
  }
  .session-list > button:hover { background: color-mix(in oklch, white 38%, transparent); }
  .session-list > button.current { border-color: var(--soft-line); background: color-mix(in oklch, white 68%, transparent); }
  .session-name { max-width: 100%; overflow: hidden; font-size: 10px; font-weight: 680; text-overflow: ellipsis; white-space: nowrap; }
  .session-state { margin-top: 3px; color: var(--muted); font-size: 8px; }
  .session-state.waiting { color: oklch(0.58 0.17 38); }

  .conversation-pane { position: relative; display: grid; min-width: 0; grid-template-rows: 46px minmax(0, 1fr) auto 46px; }
  .conversation-pane > header { display: flex; padding: 0 14px; gap: 9px; align-items: center; border-bottom: 1px solid var(--soft-line); }
  .agent-avatar { display: grid; width: 27px; height: 27px; flex: 0 0 auto; place-items: center; border-radius: 9px; color: white; background: var(--signal); font-size: 10px; font-weight: 750; }
  .agent-title { display: flex; min-width: 0; flex-direction: column; }
  .agent-title strong { font-size: 10px; }
  .agent-title small { max-width: 280px; margin-top: 2px; overflow: hidden; color: var(--muted); font-size: 8px; text-overflow: ellipsis; white-space: nowrap; }
  .conversation-pane > header i { width: 7px; height: 7px; margin-left: auto; border-radius: 50%; background: oklch(0.62 0.14 145); }
  .conversation-pane > header i.working { background: var(--signal); box-shadow: 0 0 0 4px color-mix(in oklch, var(--signal) 14%, transparent); }
  .conversation-pane > header i.waiting { background: oklch(0.64 0.17 38); }

  .conversation-log { min-height: 0; padding: 14px 16px; overflow: auto; scrollbar-width: thin; }
  .conversation-log :global(.mine), .conversation-log :global(.wip), .conversation-log :global(.plan), .conversation-log :global(.think), .conversation-log :global(.warn) { margin-bottom: 12px; }

  .permission-stack { display: flex; padding: 0 12px; gap: 6px; flex-direction: column; }
  .permission { display: flex; padding: 8px 9px; gap: 8px; align-items: center; justify-content: space-between; border: 1px solid color-mix(in oklch, oklch(0.64 0.17 38) 35%, transparent); border-radius: 10px; background: color-mix(in oklch, oklch(0.92 0.05 72) 48%, white); font-size: 8px; }
  .permission > span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .permission > div { display: flex; gap: 4px; }
  .permission button { padding: 4px 7px; border: 1px solid var(--soft-line); border-radius: 6px; color: var(--ink); background: white; font: inherit; cursor: pointer; }
  .permission button.allow { color: white; background: var(--ink); }

  .composer { display: grid; margin: 6px 12px 10px; padding: 5px 6px 5px 10px; grid-template-columns: 1fr 30px; align-items: center; border: 1px solid var(--line); border-radius: 13px; background: color-mix(in oklch, white 60%, transparent); }
  .composer textarea { min-width: 0; max-height: 52px; padding: 4px 0; resize: none; border: 0; outline: none; color: var(--ink); background: transparent; font: 10px/1.35 inherit; }
  .composer textarea::placeholder { color: var(--muted); }
  .composer button { width: 28px; height: 28px; border: 0; border-radius: 9px; color: white; background: var(--ink); cursor: pointer; }
  .composer button:disabled { opacity: 0.3; cursor: default; }

  .empty-conversation { display: grid; height: 100%; padding: 30px; place-content: center; justify-items: center; color: var(--muted); text-align: center; }
  .empty-conversation strong { color: var(--ink); font-size: 12px; }
  .empty-conversation p { max-width: 290px; margin: 7px 0 14px; font-size: 9px; line-height: 1.5; }
  .no-sessions { margin: 8px 4px 10px; color: var(--muted); font-size: 9px; line-height: 1.45; }
  .open-console { min-height: 30px; padding: 0 10px; border: 0; border-radius: 9px; color: white; background: var(--ink); font: 9px inherit; cursor: pointer; }
  .agent-error { position: absolute; right: 12px; bottom: 58px; left: 12px; margin: 0; padding: 7px 9px; border-radius: 8px; color: oklch(0.5 0.17 28); background: color-mix(in oklch, oklch(0.94 0.04 28) 78%, white); font-size: 8px; }
</style>
