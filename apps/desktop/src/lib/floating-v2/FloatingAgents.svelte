<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import AgentConversation from "$lib/AgentConversation.svelte";
  import AgentMark from "$lib/AgentMark.svelte";
  import { agents, type AgentStatus } from "$lib/agentSessions.svelte";
  import { effortShortLabel, resolveModelChoice } from "$lib/agentModels";
  import {
    agentBackends,
    agentListModels,
    capturePrimaryMonitor,
    onDictationStatus,
    toggleDictation,
  } from "$lib/api";
  import type {
    AgentBackendInfo,
    AgentItem,
    AgentModel,
    AgentOrigin,
    DictationPhase,
    PermissionDecision,
  } from "$lib/types";

  const MODES = [
    { id: "manual", label: "Preguntar" },
    { id: "acceptEdits", label: "Aceptar ediciones" },
    { id: "plan", label: "Solo plan" },
    { id: "bypassPermissions", label: "Acceso total" },
  ];

  let backends = $state<AgentBackendInfo[]>([]);
  let picked = $state("");
  let discoveredModels = $state<Record<string, AgentModel[]>>({});
  let modelsLoading = $state(false);
  let model = $state("");
  let effort = $state("");
  let mode = $state("manual");
  let cwd = $state("");
  let selectedId = $state<string | null>(null);
  let creating = $state(true);
  let draft = $state("");
  let attachments = $state<string[]>([]);
  let origin = $state<AgentOrigin | null>(null);
  let starting = $state(false);
  let sending = $state(false);
  let stopping = $state(false);
  let localError = $state<string | null>(null);
  let dictation = $state<DictationPhase>("idle");
  let inputEl = $state<HTMLTextAreaElement | null>(null);

  const active = $derived(creating ? undefined : agents.byId(selectedId));
  const backendId = $derived(active?.backendId ?? picked);
  const backend = $derived(backends.find((item) => item.id === backendId));
  const ready = $derived(Boolean(backend?.available));
  const models = $derived(
    active?.models.length ? active.models : (discoveredModels[backendId] ?? []),
  );
  const selectedModel = $derived(models.find((item) => item.id === model));
  const efforts = $derived(selectedModel?.efforts ?? []);
  const items = $derived((active?.turns ?? []).flatMap((turn) => turn.items));
  const turnEnds = $derived(
    new Map(
      (active?.turns ?? [])
        .filter((turn) => turn.status !== "running")
        .map((turn) => [turn.items.at(-1)?.id ?? turn.id, turn.costUsd]),
    ),
  );
  const working = $derived(active?.status === "working");
  const blocked = $derived(working || Boolean(active?.pending.length));
  const hasContent = $derived(Boolean(draft.trim() || attachments.length));
  const canSend = $derived(!starting && !sending && !blocked && hasContent && (Boolean(active) || ready));
  const lastUserItem = $derived.by(() =>
    [...items].reverse().find(
      (item): item is AgentItem & { kind: "message"; role: "user" } =>
        item.kind === "message" && item.role === "user",
    ),
  );
  const visibleError = $derived(localError ?? active?.error ?? null);
  const dictating = $derived(dictation === "listening" || dictation === "transcribing");

  onMount(() => {
    let unlistenDrop: (() => void) | null = null;
    const unlistenDictation = onDictationStatus((status) => {
      dictation = status.phase;
      if (status.phase === "pasted") origin = { ...origin, via: "dictado", files: [...attachments] };
    });

    void (async () => {
      try {
        await agents.init({ notify: true });
        backends = await agentBackends();
        picked = backends.find((item) => item.available)?.id ?? backends[0]?.id ?? "";
        if (agents.sessions[0]) {
          selectedId = agents.sessions[0].id;
          creating = false;
        }
      } catch (cause) {
        localError = String(cause);
      }
      try {
        unlistenDrop = await getCurrentWindow().onDragDropEvent((event) => {
          if (event.payload.type === "drop") addPaths(event.payload.paths);
        });
      } catch {
        // El selector de archivos sigue disponible si WebView2 no expone drop.
      }
    })();

    return () => {
      void unlistenDictation.then((unlisten) => unlisten());
      unlistenDrop?.();
    };
  });

  $effect(() => {
    agents.watch(active?.id ?? null);
  });

  $effect(() => {
    if (!active) return;
    if (picked !== active.backendId) picked = active.backendId;
    if (active.mode && mode !== active.mode) mode = active.mode;
    if (active.cwd && cwd !== active.cwd) cwd = active.cwd;
    if (active.model) {
      const resolved = resolveModelChoice(active.models.length ? active.models : models, active.model);
      if (resolved.modelId && model !== resolved.modelId) model = resolved.modelId;
      const nextEffort = active.effort ?? resolved.effortId;
      if (nextEffort && effort !== nextEffort) effort = nextEffort;
    }
  });

  $effect(() => {
    const id = backendId;
    if (!id || discoveredModels[id] || active?.models.length) return;
    let cancelled = false;
    modelsLoading = true;
    void agentListModels(id)
      .then((list) => {
        if (!cancelled) {
          modelsLoading = false;
          discoveredModels = { ...discoveredModels, [id]: list };
        }
      })
      .catch((cause) => {
        if (!cancelled) {
          modelsLoading = false;
          localError = String(cause);
        }
      });
    return () => {
      cancelled = true;
    };
  });

  $effect(() => {
    if (active || models.length === 0) return;
    if (!model || !models.some((item) => item.id === model)) {
      model = models[0].id;
      effort = models[0].defaultEffort ?? models[0].efforts[0]?.id ?? "";
    }
  });

  $effect(() => {
    if (!draft.trim() && !dictating && attachments.length === 0) origin = null;
  });

  onDestroy(() => agents.watch(null));

  function autoScroll(node: HTMLDivElement) {
    const scroll = () => requestAnimationFrame(() => (node.scrollTop = node.scrollHeight));
    const observer = new MutationObserver(scroll);
    observer.observe(node, { childList: true, subtree: true, characterData: true });
    scroll();
    return { destroy: () => observer.disconnect() };
  }

  function stateLabel(status: AgentStatus) {
    if (status === "working") return "trabajando";
    if (status === "waiting") return "espera permiso";
    if (status === "failed") return "con error";
    return "lista";
  }

  function fileName(path: string) {
    return path.split(/[\\/]/).pop() || path;
  }

  function isImage(path: string) {
    return /\.(png|jpe?g|gif|webp)$/i.test(path);
  }

  function addPaths(paths: string[]) {
    const images = paths.filter(isImage);
    const files = paths.filter((path) => !isImage(path));
    if (images.length) {
      attachments = [...new Set([...attachments, ...images])];
      origin = { via: "archivo", file: fileName(attachments[0]), files: [...attachments] };
    }
    if (files.length) draft = [draft.trim(), ...files].filter(Boolean).join("\n");
    inputEl?.focus();
  }

  function removeAttachment(path: string) {
    attachments = attachments.filter((item) => item !== path);
    if (attachments.length) origin = { via: origin?.via ?? "archivo", file: fileName(attachments[0]), files: [...attachments] };
    else if (!draft.trim()) origin = null;
  }

  async function pickFiles() {
    try {
      const chosen = await openDialog({
        multiple: true,
        filters: [
          { name: "Imágenes", extensions: ["png", "jpg", "jpeg", "gif", "webp"] },
          { name: "Todos", extensions: ["*"] },
        ],
      });
      addPaths(Array.isArray(chosen) ? chosen : chosen ? [chosen] : []);
    } catch (cause) {
      localError = String(cause);
    }
  }

  async function pickFolder() {
    try {
      const chosen = await openDialog({ directory: true, multiple: false });
      if (typeof chosen === "string") cwd = chosen;
    } catch (cause) {
      localError = String(cause);
    }
  }

  async function capture() {
    try {
      const path = await capturePrimaryMonitor();
      attachments = [...attachments, path];
      origin = { via: "captura", file: fileName(path), files: [...attachments] };
      inputEl?.focus();
    } catch (cause) {
      localError = String(cause);
    }
  }

  async function dictate() {
    inputEl?.focus();
    try {
      await toggleDictation();
    } catch (cause) {
      localError = String(cause);
    }
  }

  function selectSession(id: string) {
    selectedId = id;
    creating = false;
    localError = null;
  }

  function newSession() {
    selectedId = null;
    creating = true;
    localError = null;
    cwd = "";
    mode = "manual";
    const first = discoveredModels[picked]?.[0];
    model = first?.id ?? "";
    effort = first?.defaultEffort ?? first?.efforts[0]?.id ?? "";
    requestAnimationFrame(() => inputEl?.focus());
  }

  async function chooseBackend(id: string) {
    picked = id;
    model = "";
    effort = "";
    localError = null;
  }

  async function chooseModel(id: string) {
    model = id;
    const choice = models.find((item) => item.id === id);
    effort = choice?.defaultEffort ?? choice?.efforts[0]?.id ?? "";
    if (!active) return;
    try {
      await agents.setModel(active.id, id, effort || undefined, active.fast ?? undefined);
    } catch (cause) {
      localError = String(cause);
    }
  }

  async function chooseEffort(id: string) {
    effort = id;
    if (!active || !model) return;
    try {
      await agents.setModel(active.id, model, id || undefined, active.fast ?? undefined);
    } catch (cause) {
      localError = String(cause);
    }
  }

  function messageOrigin(): AgentOrigin | undefined {
    if (!origin && attachments.length === 0) return undefined;
    return {
      via: origin?.via ?? "archivo",
      file: origin?.file ?? (attachments[0] ? fileName(attachments[0]) : undefined),
      files: [...attachments],
    };
  }

  async function submit() {
    if (!canSend) return;
    const text = draft.trim() || "Mira esta imagen.";
    const from = messageOrigin();
    const savedAttachments = [...attachments];
    draft = "";
    attachments = [];
    origin = null;
    localError = null;
    try {
      if (active) {
        sending = true;
        await agents.send(active.id, text, from);
      } else {
        starting = true;
        const id = await agents.start(picked, {
          cwd: cwd || undefined,
          model: model || undefined,
          effort: effort || undefined,
          permissionMode: mode,
        });
        selectedId = id;
        creating = false;
        await agents.send(id, text, from);
      }
    } catch (cause) {
      draft = text === "Mira esta imagen." ? "" : text;
      attachments = savedAttachments;
      origin = from ?? null;
      localError = String(cause);
    } finally {
      starting = false;
      sending = false;
    }
  }

  async function retryLast() {
    if (!active || !lastUserItem || sending || working) return;
    sending = true;
    localError = null;
    try {
      await agents.send(active.id, lastUserItem.text, lastUserItem.origin);
    } catch (cause) {
      localError = String(cause);
    } finally {
      sending = false;
    }
  }

  async function stopSession() {
    if (!active || stopping) return;
    const id = active.id;
    stopping = true;
    localError = null;
    try {
      await agents.stop(id);
      const next = agents.sessions.find((session) => session.id !== id);
      selectedId = next?.id ?? null;
      creating = !next;
    } catch (cause) {
      localError = String(cause);
    } finally {
      stopping = false;
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

  function clearError() {
    localError = null;
    if (active) active.error = null;
  }
</script>

<div class="agent-workspace">
  <aside aria-label="Sesiones de agentes">
    <div class="rail-heading">
      <span>SESIONES</span>
      <button type="button" aria-label="Nueva sesión" title="Nueva sesión" onclick={newSession}>+</button>
    </div>
    <div class="session-list">
      {#each agents.sessions as session (session.id)}
        <button
          class:current={!creating && session.id === active?.id}
          type="button"
          onclick={() => selectSession(session.id)}
        >
          <AgentMark backend={session.backendId} size={22} />
          <span>
            <strong>{session.backendName}</strong>
            <small class:waiting={session.pending.length > 0}>
              {session.unread > 0 ? `${session.unread} nueva${session.unread === 1 ? "" : "s"}` : stateLabel(session.status)}
            </small>
          </span>
        </button>
      {/each}
      {#if agents.sessions.length === 0}
        <p>Ninguna sesión activa.</p>
      {/if}
    </div>
  </aside>

  <section class="conversation-pane" aria-label={active ? `Conversación con ${active.backendName}` : "Nueva sesión de agente"}>
    <header class="agent-toolbar">
      <label>
        <span>Agente</span>
        <select aria-label="Agente o proveedor" value={backendId} disabled={Boolean(active)} onchange={(event) => void chooseBackend(event.currentTarget.value)}>
          {#each backends as item (item.id)}
            <option value={item.id} disabled={!item.available}>{item.displayName}{item.available ? "" : " · no disponible"}</option>
          {/each}
        </select>
      </label>
      <label>
        <span>Modelo</span>
        <select aria-label="Modelo" value={model} disabled={modelsLoading || models.length === 0} onchange={(event) => void chooseModel(event.currentTarget.value)}>
          {#if modelsLoading && models.length === 0}<option value="">Cargando…</option>{/if}
          {#each models as item (item.id)}<option value={item.id}>{item.name}</option>{/each}
        </select>
      </label>
      {#if efforts.length > 0}
        <label class="effort-control">
          <span>Esfuerzo</span>
          <select aria-label="Esfuerzo" value={effort} onchange={(event) => void chooseEffort(event.currentTarget.value)}>
            {#each efforts as item (item.id)}<option value={item.id}>{effortShortLabel(item.id)}</option>{/each}
          </select>
        </label>
      {/if}
      {#if active}
        <button class="stop-button" type="button" disabled={stopping} onclick={() => void stopSession()}>
          {working ? "Cancelar" : "Cerrar"}
        </button>
      {/if}
    </header>

    {#if active}
      <div use:autoScroll class="conversation-log">
        {#if items.length > 0}
          <AgentConversation {items} {turnEnds} />
        {:else}
          <div class="empty-state"><strong>Sesión lista</strong><p>Escribe abajo para comenzar.</p></div>
        {/if}
      </div>
    {:else}
      <div class="new-session">
        <AgentMark backend={picked || "codex"} size={40} />
        <div><strong>Nueva sesión</strong><p>Empieza aquí. No hace falta abrir la consola antigua.</p></div>
        <div class="launch-options">
          <label><span>Permisos</span><select bind:value={mode} aria-label="Modo de permisos">{#each MODES as item (item.id)}<option value={item.id}>{item.label}</option>{/each}</select></label>
          <button type="button" class="folder-button" onclick={() => void pickFolder()} title={cwd || "Elegir carpeta"}>
            <span>Carpeta</span><strong>{cwd ? fileName(cwd) : "Elegir…"}</strong>
          </button>
        </div>
        {#if !ready && picked}<p class="backend-warning" role="status">Este agente no está disponible en el equipo.</p>{/if}
      </div>
    {/if}

    <div class="permission-stack">
      {#each active?.pending ?? [] as permission (permission.id)}
        <div class="permission" role="alert">
          <span><strong>{permission.tool}</strong><small>{permission.description || "Solicita continuar"}</small></span>
          <div>
            <button type="button" onclick={() => void decide(permission.id, "deny")}>Denegar</button>
            <button type="button" onclick={() => void decide(permission.id, "allowAlways")}>Siempre</button>
            <button class="allow" type="button" onclick={() => void decide(permission.id, "allow")}>Permitir</button>
          </div>
        </div>
      {/each}
    </div>

    {#if attachments.length > 0}
      <div class="attachments" aria-label="Adjuntos">
        {#each attachments as path (path)}
          <span>
            <img src={convertFileSrc(path)} alt="" />
            <small>{fileName(path)}</small>
            <button type="button" aria-label={`Quitar ${fileName(path)}`} onclick={() => removeAttachment(path)}>×</button>
          </span>
        {/each}
      </div>
    {/if}

    <form class="composer" onsubmit={(event) => { event.preventDefault(); void submit(); }}>
      <div class="composer-actions">
        <button type="button" aria-label="Adjuntar archivos" title="Adjuntar" onclick={() => void pickFiles()}>＋</button>
        <button type="button" aria-label="Capturar pantalla" title="Capturar pantalla" onclick={() => void capture()}>▣</button>
        <button class:active={dictating} type="button" aria-label={dictating ? "Detener dictado" : "Dictar mensaje"} title="Dictado" onclick={() => void dictate()}>◉</button>
      </div>
      <textarea
        bind:this={inputEl}
        bind:value={draft}
        aria-label="Mensaje para el agente"
        placeholder={working ? "El agente está trabajando…" : active ? "Continúa la conversación…" : "¿Qué quieres hacer?"}
        disabled={blocked || starting || sending}
        rows="1"
        onkeydown={(event) => {
          if (event.key === "Enter" && !event.shiftKey) {
            event.preventDefault();
            void submit();
          }
        }}
      ></textarea>
      <button class="send-button" type="submit" disabled={!canSend} aria-label={active ? "Enviar mensaje" : "Iniciar y enviar"}>↑</button>
    </form>

    {#if visibleError}
      <div class="agent-error" role="alert">
        <span>{visibleError}</span>
        {#if active && lastUserItem && !working}<button type="button" onclick={() => void retryLast()}>Reintentar</button>{/if}
        <button type="button" aria-label="Cerrar error" onclick={clearError}>×</button>
      </div>
    {/if}
  </section>
</div>

<style>
  .agent-workspace {
    --ink: var(--pearl-ink, oklch(0.22 0.012 255));
    --muted: var(--pearl-muted, oklch(0.48 0.014 255));
    --line: var(--pearl-line, oklch(0.22 0.012 255 / 0.1));
    --soft: var(--pearl-soft, oklch(0.22 0.012 255 / 0.055));
    --signal: var(--pearl-signal, oklch(0.62 0.18 252));
    --coral: var(--signal);
    --text: var(--ink);
    --dim: var(--muted);
    --faint: color-mix(in oklch, var(--muted) 76%, transparent);
    --card: color-mix(in oklch, var(--ink) 6%, transparent);
    --code: color-mix(in oklch, var(--soft) 70%, transparent);
    --hover: color-mix(in oklch, var(--soft) 80%, transparent);
    --add: oklch(0.56 0.13 145);
    --del: oklch(0.58 0.17 28);
    display: grid;
    height: 100%;
    min-height: 0;
    grid-template-columns: 142px minmax(0, 1fr);
    overflow: hidden;
  }

  aside { min-width: 0; padding: 8px; border-right: 1px solid var(--line); background: color-mix(in oklch, var(--soft) 56%, transparent); }
  .rail-heading { display: flex; height: 29px; padding: 0 3px 5px 5px; align-items: center; justify-content: space-between; color: var(--muted); font-size: 8px; font-weight: 750; letter-spacing: 0.12em; }
  .rail-heading button { width: 26px; height: 26px; border: 0; border-radius: 8px; color: var(--ink); background: var(--soft); font-size: 17px; cursor: pointer; }
  .session-list { display: flex; gap: 4px; flex-direction: column; }
  .session-list > button { display: grid; min-width: 0; min-height: 48px; padding: 7px; grid-template-columns: 22px minmax(0, 1fr); align-items: center; gap: 7px; border: 1px solid transparent; border-radius: 9px; color: var(--ink); background: transparent; text-align: left; cursor: pointer; }
  .session-list > button:hover { background: var(--soft); }
  .session-list > button.current { border-color: var(--line); background: var(--card); }
  .session-list > button span { display: flex; min-width: 0; flex-direction: column; }
  .session-list strong { overflow: hidden; font-size: 9px; text-overflow: ellipsis; white-space: nowrap; }
  .session-list small { margin-top: 2px; color: var(--muted); font-size: 8px; }
  .session-list small.waiting { color: oklch(0.61 0.16 48); }
  .session-list p { margin: 10px 5px; color: var(--muted); font-size: 9px; line-height: 1.45; }

  .conversation-pane { position: relative; display: grid; min-width: 0; min-height: 0; grid-template-rows: auto minmax(0, 1fr) auto auto auto; }
  .agent-toolbar { display: flex; min-width: 0; min-height: 49px; padding: 6px 10px; align-items: flex-end; gap: 5px; border-bottom: 1px solid var(--line); }
  .agent-toolbar label, .launch-options label { display: flex; min-width: 0; flex: 1; flex-direction: column; gap: 2px; }
  .agent-toolbar label > span, .launch-options label > span, .folder-button > span { color: var(--muted); font-size: 7px; font-weight: 700; letter-spacing: 0.06em; text-transform: uppercase; }
  select { width: 100%; min-width: 0; height: 27px; border: 1px solid var(--line); border-radius: 7px; padding: 0 22px 0 7px; color: var(--ink); background: var(--card); font: 9px inherit; outline: none; }
  select:focus-visible { border-color: var(--signal); box-shadow: 0 0 0 2px color-mix(in oklch, var(--signal) 18%, transparent); }
  .effort-control { max-width: 92px; }
  .stop-button { height: 27px; flex: 0 0 auto; border: 1px solid color-mix(in oklch, oklch(0.58 0.17 28) 24%, var(--line)); border-radius: 7px; padding: 0 9px; color: oklch(0.55 0.16 28); background: transparent; font: 9px inherit; cursor: pointer; }

  .conversation-log { min-height: 0; padding: 12px 14px; overflow: auto; scrollbar-width: thin; }
  .conversation-log :global(.mine), .conversation-log :global(.wip), .conversation-log :global(.plan), .conversation-log :global(.think), .conversation-log :global(.warn) { margin-bottom: 11px; }
  .empty-state, .new-session { display: grid; height: 100%; place-content: center; justify-items: center; color: var(--muted); text-align: center; }
  .empty-state strong, .new-session strong { color: var(--ink); font-size: 12px; }
  .empty-state p, .new-session p { margin: 5px 0 0; font-size: 9px; line-height: 1.45; }
  .new-session { min-height: 0; padding: 12px 22px; grid-template-rows: auto auto auto; gap: 8px; }
  .new-session > div:nth-child(2) { max-width: 310px; }
  .launch-options { display: grid; width: min(100%, 350px); grid-template-columns: 1fr 1fr; gap: 6px; }
  .folder-button { display: flex; min-width: 0; height: 43px; flex-direction: column; justify-content: center; align-items: flex-start; border: 1px solid var(--line); border-radius: 7px; padding: 4px 8px; color: var(--ink); background: var(--card); cursor: pointer; }
  .folder-button strong { max-width: 100%; margin-top: 2px; overflow: hidden; font-size: 9px; text-overflow: ellipsis; white-space: nowrap; }
  .backend-warning { color: oklch(0.58 0.17 36); }

  .permission-stack { display: flex; padding: 0 10px; gap: 5px; flex-direction: column; }
  .permission { display: flex; padding: 7px 8px; align-items: center; justify-content: space-between; gap: 8px; border-left: 2px solid oklch(0.64 0.17 48); background: color-mix(in oklch, oklch(0.92 0.05 72) 42%, transparent); font-size: 8px; }
  .permission > span { display: flex; min-width: 0; flex-direction: column; }
  .permission small { max-width: 260px; overflow: hidden; color: var(--muted); text-overflow: ellipsis; white-space: nowrap; }
  .permission > div { display: flex; flex: 0 0 auto; gap: 3px; }
  .permission button { min-height: 25px; border: 1px solid var(--line); border-radius: 6px; padding: 0 7px; color: var(--ink); background: var(--card); font: inherit; cursor: pointer; }
  .permission button.allow { color: var(--rb-on-accent, white); background: var(--ink); }

  .attachments { display: flex; min-width: 0; padding: 5px 10px 0; gap: 5px; overflow-x: auto; }
  .attachments > span { position: relative; display: grid; width: 88px; height: 42px; flex: 0 0 auto; grid-template-columns: 34px minmax(0, 1fr); align-items: center; gap: 5px; border: 1px solid var(--line); border-radius: 7px; padding: 3px 18px 3px 3px; background: var(--soft); }
  .attachments img { width: 34px; height: 34px; border-radius: 5px; object-fit: cover; }
  .attachments small { overflow: hidden; color: var(--muted); font-size: 7px; text-overflow: ellipsis; white-space: nowrap; }
  .attachments button { position: absolute; top: 1px; right: 2px; border: 0; color: var(--muted); background: transparent; cursor: pointer; }

  .composer { display: grid; min-height: 48px; margin: 6px 10px 9px; padding: 5px; grid-template-columns: auto minmax(0, 1fr) 32px; align-items: center; gap: 5px; border: 1px solid var(--line); border-radius: 12px; background: var(--card); }
  .composer-actions { display: flex; gap: 2px; }
  .composer-actions button, .send-button { display: grid; width: 27px; height: 27px; place-items: center; border: 0; border-radius: 7px; color: var(--muted); background: transparent; cursor: pointer; }
  .composer-actions button:hover, .composer-actions button.active { color: var(--signal); background: var(--soft); }
  .composer textarea { min-width: 0; max-height: 62px; resize: none; border: 0; padding: 5px; color: var(--ink); background: transparent; font: 10px/1.35 inherit; outline: none; }
  .composer textarea::placeholder { color: var(--muted); }
  .send-button { width: 30px; height: 30px; color: var(--rb-on-accent, white); background: var(--ink); }
  .send-button:disabled { opacity: 0.28; cursor: default; }

  .agent-error { position: absolute; right: 10px; bottom: 63px; left: 10px; display: flex; min-height: 34px; padding: 6px 7px 6px 9px; align-items: center; gap: 6px; border-left: 2px solid oklch(0.58 0.17 28); color: color-mix(in oklch, oklch(0.64 0.18 28) 76%, var(--ink)); background: color-mix(in oklch, oklch(0.64 0.18 28) 12%, var(--card)); box-shadow: 0 8px 24px oklch(0.2 0.02 28 / 0.13); font-size: 8px; }
  .agent-error span { min-width: 0; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .agent-error button { min-height: 24px; border: 0; border-radius: 6px; padding: 0 7px; color: inherit; background: color-mix(in oklch, currentColor 8%, transparent); font: inherit; cursor: pointer; }

  button:focus-visible { outline: 2px solid var(--signal); outline-offset: 1px; }
  button:disabled, select:disabled { opacity: 0.45; cursor: not-allowed; }

  @media (max-width: 560px) {
    .agent-workspace { grid-template-columns: 1fr; grid-template-rows: 58px minmax(0, 1fr); }
    aside { display: grid; padding: 6px; grid-template-columns: 70px minmax(0, 1fr); border-right: 0; border-bottom: 1px solid var(--line); }
    .rail-heading { height: 46px; padding: 0 5px; }
    .session-list { flex-direction: row; overflow-x: auto; }
    .session-list > button { min-width: 112px; min-height: 46px; }
    .agent-toolbar { overflow-x: auto; }
    .agent-toolbar label { min-width: 110px; }
  }

  @media (prefers-reduced-motion: reduce) {
    * { scroll-behavior: auto !important; }
  }
</style>
