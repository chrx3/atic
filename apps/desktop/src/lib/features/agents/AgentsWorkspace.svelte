<script lang="ts">
  /**
   * La ventana de agentes: el lugar donde se trabaja con ellos.
   *
   * Una ventana normal del sistema —marco, foco, Alt+Tab— en vez del float
   * del overlay: sin modo texto, sin popovers recortados, sin mudanzas entre
   * isla, float y ventana. A la izquierda los agentes (chats y terminales en
   * una lista, con su estado) y lo reciente; al centro el elegido.
   *
   * La pill avisa y, al tocar un agente, pide mostrarlo acá (`agents-focus`).
   */
  import { onMount, untrack } from "svelte";
  import { agents } from "$lib/agentSessions.svelte";
  import {
    rememberedEffort,
    rememberedMode,
    rememberedModelId,
  } from "$lib/agentModels";
  import type { StoredThread } from "$lib/types";
  import {
    agentListModels,
    agentThread,
    agentThreads,
    cliOnPath,
    consoleClose,
    consoleTail,
    onAgentsFocus,
  } from "$ipc/agents";
  import { copyText } from "$ipc/clipboard";
  import { toasts } from "$domain/toasts.svelte";
  import { homeDir } from "@tauri-apps/api/path";
  import { t } from "$domain/i18n.svelte";
  import ConfirmDialog from "$ui/ConfirmDialog.svelte";
  import AgentChatPanel from "./AgentChatPanel.svelte";
  import AgentLogo from "./AgentLogo.svelte";
  import AgentSettingsModal from "./AgentSettingsModal.svelte";
  import FolderBrowser from "./FolderBrowser.svelte";
  import TerminalView from "./TerminalView.svelte";
  import WorkspaceHeader from "./WorkspaceHeader.svelte";
  import WorkspaceSidebar from "./WorkspaceSidebar.svelte";
  import { AGENTS, type AgentDef } from "./agentCatalog";
  import type { ItemStatus, WorkspaceItem } from "./agentWorkspace";
  import { workspace } from "./agentWorkspace.svelte";
  import { canResume, resumeThread } from "./chatResume";
  import { chatTabStatus } from "./chatStatus";

  const CWD_KEY = "atic.agents.startFolder";
  const RECENT_MAX = 40;

  let cwd = $state("");
  let installed = $state<Record<string, boolean>>({});
  let newOpen = $state(false);
  let recent = $state<StoredThread[]>([]);
  let error = $state<string | null>(null);
  /** Terminales cuyo proceso terminó: siguen en la lista hasta cerrarlas. */
  let ended = $state<Record<string, boolean>>({});
  let pendingClose = $state<WorkspaceItem | null>(null);
  let settingsOpen = $state(false);
  let browsingFolder = $state(false);
  let windowFocused = $state(document.hasFocus());

  const choices = $derived(AGENTS.filter((a) => installed[a.cli] !== false));
  const current = $derived(workspace.current);

  function agentOf(backend: string): AgentDef | undefined {
    return AGENTS.find((a) => a.backend === backend);
  }

  function statusOf(item: WorkspaceItem): ItemStatus {
    if (item.kind === "chat") return chatTabStatus(agents.byId(item.session));
    return ended[item.key] ? "ended" : "terminal";
  }

  /**
   * El título de lo que está a la vista: el nombre que le dieron o, si no
   * tiene, lo primero que se le pidió. El nombre del agente queda de respaldo.
   */
  function titleOf(item: WorkspaceItem): string {
    if (item.kind !== "chat") return item.label;
    const live = agents.byId(item.session);
    if (live?.label?.trim()) return live.label.trim();
    for (const turn of live?.turns ?? []) {
      for (const it of turn.items) {
        if (it.kind === "message" && it.role === "user" && it.text.trim()) {
          return it.text.trim().split("\n")[0].slice(0, 120);
        }
      }
    }
    return item.label;
  }

  function folderOf(item: WorkspaceItem): string {
    if (item.kind === "chat") return agents.byId(item.session)?.cwd || cwd;
    return item.cwd || cwd;
  }

  function terminalIn(folder: string) {
    workspace.add({
      kind: "terminal",
      session: null,
      label: t("page.agents.window.shellLabel"),
      cli: null,
      command: null,
      cwd: folder || null,
    });
  }

  async function copyPath(folder: string) {
    try {
      await copyText(folder);
      toasts.push(t("page.agents.window.pathCopied"), 2000);
    } catch (err) {
      error = String(err);
    }
  }

  // ── Empezar ─────────────────────────────────────────────────────────────

  /**
   * Sin carpeta elegida, la del usuario. Mandar `cwd` vacío dejaba al agente
   * en la carpeta del proceso de Atic —en desarrollo, dentro del repo—, donde
   * además rigen los ajustes del proyecto y no los del usuario.
   */
  async function workingDir(): Promise<string | undefined> {
    if (cwd) return cwd;
    return homeDir().catch(() => undefined);
  }

  /**
   * El modelo y esfuerzo que se usaron la última vez con este agente, si el
   * modelo sigue existiendo. Sin nada recordado decide el CLI: no se elige por
   * el usuario el primero de la lista, que suele ser el más caro.
   */
  async function lastChoice(
    backend: string,
  ): Promise<{ model?: string; effort?: string }> {
    const saved = rememberedModelId(backend);
    if (!saved) return {};
    const list = await agentListModels(backend).catch(() => []);
    if (list.length > 0 && !list.some((m) => m.id === saved)) return {};
    return {
      model: saved,
      effort: rememberedEffort(backend, saved, list) || undefined,
    };
  }

  async function startChat(agent: AgentDef, model?: string): Promise<string> {
    const last = model ? { model } : await lastChoice(agent.backend);
    return agents.start(agent.backend, {
      cwd: await workingDir(),
      model: last.model,
      effort: last.effort,
      permissionMode:
        agent.backend === "claude-code" ? rememberedMode(agent.backend) : undefined,
    });
  }

  async function newSession(agent: AgentDef | null, kind: "chat" | "terminal") {
    newOpen = false;
    error = null;
    if (kind === "terminal") {
      workspace.add({
        kind: "terminal",
        session: null,
        label: agent?.name ?? t("page.agents.window.shellLabel"),
        cli: agent?.cli ?? null,
        command: agent?.cli ?? null,
        cwd: cwd || null,
      });
      return;
    }
    if (!agent) return;
    try {
      const id = await startChat(agent);
      workspace.add({ kind: "chat", session: id, label: agent.name, cli: agent.cli });
    } catch (err) {
      error = String(err);
    }
  }

  /** Un chat vacío se reemplaza en su sitio; uno con conversación, no se tira. */
  function isEmptyChat(
    item: WorkspaceItem | null,
  ): item is Extract<WorkspaceItem, { kind: "chat" }> {
    return (
      item?.kind === "chat" && (agents.byId(item.session)?.turns.length ?? 0) === 0
    );
  }

  async function switchAgent(key: string, agent: AgentDef, model?: string) {
    const item = workspace.items.find((i) => i.key === key) ?? null;
    error = null;
    try {
      const id = await startChat(agent, model);
      if (isEmptyChat(item)) {
        const previous = item.session;
        workspace.update(key, { session: id, label: agent.name, cli: agent.cli });
        void agents.stop(previous).catch(() => {});
      } else {
        workspace.add({ kind: "chat", session: id, label: agent.name, cli: agent.cli });
      }
    } catch (err) {
      error = String(err);
    }
  }

  async function openThread(thread: StoredThread, key: string | null = null) {
    if (!canResume(thread)) return;
    const item = key ? (workspace.items.find((i) => i.key === key) ?? null) : null;
    const agent = agentOf(thread.backendId);
    const label = agent?.name ?? thread.backendName;
    error = null;
    try {
      const id = await resumeThread(thread);
      if (isEmptyChat(item)) {
        const previous = item.session;
        workspace.update(item.key, { session: id, label, cli: agent?.cli ?? null });
        void agents.stop(previous).catch(() => {});
      } else {
        workspace.add({ kind: "chat", session: id, label, cli: agent?.cli ?? null });
      }
    } catch (err) {
      error = String(err);
    }
  }

  function pickFolder(path: string) {
    browsingFolder = false;
    cwd = path;
    try {
      localStorage.setItem(CWD_KEY, path);
    } catch {
      /* queda en memoria */
    }
  }

  // ── Cerrar ──────────────────────────────────────────────────────────────

  /** Cerrar termina el proceso: sin deshacer, así que lo vivo pregunta. */
  function requestClose(key: string) {
    const item = workspace.items.find((i) => i.key === key);
    if (!item) return;
    const busy =
      item.kind === "chat"
        ? agents.byId(item.session)?.status === "working"
        : !!item.session && !ended[item.key];
    if (busy) pendingClose = item;
    else void closeNow(item);
  }

  async function closeNow(item: WorkspaceItem) {
    pendingClose = null;
    workspace.remove(item.key);
    if (item.kind === "chat") {
      // Las que abrió otro agente no son nuestras: se dejan de mirar, nada más.
      if (!agents.byId(item.session)?.parent) {
        void agents.stop(item.session).catch(() => {});
      }
    } else if (item.session) {
      void consoleClose(item.session).catch(() => {});
    }
  }

  // ── Enfocar desde la pill ───────────────────────────────────────────────

  // eslint-disable-next-line svelte/prefer-svelte-reactivity -- registro interno, no es estado de vista
  const seenNonces = new Set<number>();

  function focusSession(kind: "chat" | "terminal", session: string) {
    const existing = workspace.findSession(kind, session);
    if (existing) {
      workspace.select(existing.key);
      return;
    }
    if (kind !== "chat") return;
    const live = agents.byId(session);
    if (!live) return;
    const agent = agentOf(live.backendId);
    workspace.add({
      kind: "chat",
      session,
      label: live.label?.trim() || agent?.name || live.backendName,
      cli: agent?.cli ?? null,
    });
  }

  // ── Mirar ───────────────────────────────────────────────────────────────

  /**
   * El chat elegido, con la ventana al frente, cuenta como mirado: sus
   * respuestas no son «nuevas» y la pill no avisa de lo que está en pantalla.
   */
  $effect(() => {
    const item = current;
    const id = item?.kind === "chat" && windowFocused ? item.session : null;
    untrack(() => {
      if (id) {
        if (agents.watching !== id) agents.watch(id);
      } else if (agents.watching) {
        agents.watch(null);
      }
    });
  });

  // Las sesiones que otro agente abre por MCP aparecen solas cuando hablan.
  $effect(() => {
    for (const s of agents.sessions) {
      if (!s.parent || s.turns.length === 0) continue;
      if (untrack(() => workspace.findSession("chat", s.id))) continue;
      const agent = agentOf(s.backendId);
      untrack(() =>
        workspace.add(
          {
            kind: "chat",
            session: s.id,
            label: s.label?.trim() || agent?.name || s.backendName,
            cli: agent?.cli ?? null,
          },
          false,
        ),
      );
    }
  });

  // ── Teclado ─────────────────────────────────────────────────────────────

  function onKey(event: KeyboardEvent) {
    const mod = event.ctrlKey || event.metaKey;
    if (!mod) return;
    const items = workspace.items;
    const index = items.findIndex((i) => i.key === workspace.active);
    if (event.key === "n" || event.key === "N") {
      newOpen = true;
    } else if ((event.key === "w" || event.key === "W") && workspace.active) {
      requestClose(workspace.active);
    } else if (event.key === "Tab" && items.length > 1) {
      const step = event.shiftKey ? -1 : 1;
      workspace.select(items[(index + step + items.length) % items.length].key);
    } else if (/^[1-9]$/.test(event.key) && items[Number(event.key) - 1]) {
      workspace.select(items[Number(event.key) - 1].key);
    } else {
      return;
    }
    event.preventDefault();
  }

  // ── Arranque ────────────────────────────────────────────────────────────

  async function loadRecent() {
    try {
      const list = await agentThreads();
      recent = list
        .filter((th) => !th.parent && th.preview.trim() && canResume(th))
        .slice(0, RECENT_MAX);
    } catch {
      recent = [];
    }
  }

  /** Lo guardado, menos lo que ya no vive; los chats vuelven con su hilo. */
  async function restore() {
    const saved = workspace.load();
    await agents.init();
    // eslint-disable-next-line svelte/prefer-svelte-reactivity -- tabla de paso, no estado de vista
    const terminalsAlive = new Map<string, boolean>();
    await Promise.all(
      saved.map(async (item) => {
        if (item.kind === "terminal" && item.session) {
          terminalsAlive.set(
            item.key,
            await consoleTail(item.session, 1).then(
              () => true,
              () => false,
            ),
          );
        }
        if (item.kind !== "chat") return;
        const live = agents.byId(item.session);
        if (!live || live.turns.length > 0) return;
        const thread = await agentThread(item.session).catch(() => null);
        if (thread) {
          agents.hydrate(item.session, {
            turns: thread.turns,
            cwd: thread.cwd,
            model: thread.model,
            providerSession: thread.providerSession,
          });
        }
      }),
    );
    workspace.keep((item) =>
      item.kind === "chat"
        ? !!agents.byId(item.session)
        : terminalsAlive.get(item.key) === true,
    );
  }

  onMount(() => {
    try {
      cwd = localStorage.getItem(CWD_KEY) ?? "";
    } catch {
      /* carpeta de inicio del usuario */
    }
    void restore();
    void loadRecent();
    void Promise.all(
      AGENTS.map(
        async (a) => [a.cli, await cliOnPath(a.cli).catch(() => true)] as const,
      ),
    ).then((entries) => (installed = Object.fromEntries(entries)));

    const unlisten = onAgentsFocus((request) => {
      if (seenNonces.has(request.nonce)) return;
      seenNonces.add(request.nonce);
      focusSession(request.kind, request.session);
    });
    const onFocus = () => (windowFocused = true);
    const onBlur = () => (windowFocused = false);
    window.addEventListener("focus", onFocus);
    window.addEventListener("blur", onBlur);
    window.addEventListener("keydown", onKey);
    return () => {
      void unlisten.then((un) => un());
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("blur", onBlur);
      window.removeEventListener("keydown", onKey);
      agents.watch(null);
    };
  });

  // Lo reciente cambia cuando un chat termina un turno: se relee al volver.
  $effect(() => {
    if (windowFocused) void loadRecent();
  });
</script>

<div class="workspace">
  <WorkspaceSidebar
    items={workspace.items}
    active={workspace.active}
    {statusOf}
    {titleOf}
    {folderOf}
    {choices}
    {cwd}
    {recent}
    {newOpen}
    onNewToggle={(open) => (newOpen = open)}
    onNew={(agent, kind) => void newSession(agent, kind)}
    onSelect={(key) => workspace.select(key)}
    onClose={requestClose}
    onPickFolder={() => (browsingFolder = true)}
    onResume={(thread) => void openThread(thread)}
    onSettings={() => (settingsOpen = true)}
  />

  <main class="stage">
    {#if current}
      {@const folder = folderOf(current)}
      <WorkspaceHeader
        title={titleOf(current)}
        agentCli={current.cli}
        {folder}
        status={statusOf(current)}
        onTerminalHere={current.kind === "chat" ? () => terminalIn(folder) : undefined}
        onCopyPath={() => void copyPath(folder)}
        onClose={() => requestClose(current.key)}
      />
    {/if}
    <div class="panes">
      {#if error}
        <p class="error" role="alert">{error}</p>
      {/if}

      <!-- Todas montadas y solo la elegida a la vista: un xterm que se desmonta
         pierde su estado, y un chat, lo que se estaba escribiendo. -->
      {#each workspace.items as item (item.key)}
        <section class="pane" class:is-active={item.key === workspace.active}>
          {#if item.kind === "chat"}
            <AgentChatPanel
              sessionId={item.session}
              readOnly={!!agents.byId(item.session)?.parent}
              {choices}
              onOpenThread={(thread) => void openThread(thread, item.key)}
              onSwitchAgent={(agent, model) => void switchAgent(item.key, agent, model)}
            />
          {:else}
            <TerminalView
              sessionId={item.session}
              command={item.command}
              cwd={item.cwd}
              focused={item.key === workspace.active}
              onSession={(id) => workspace.update(item.key, { session: id })}
              onExit={() => (ended = { ...ended, [item.key]: true })}
            />
          {/if}
        </section>
      {/each}

      {#if workspace.items.length === 0}
        <div class="empty">
          <p class="empty-title">{t("page.agents.window.emptyTitle")}</p>
          <p class="empty-hint">{t("page.agents.window.emptyHint")}</p>
          <div class="empty-agents">
            {#each choices as agent (agent.cli)}
              <button
                type="button"
                class="empty-agent"
                onclick={() => void newSession(agent, "chat")}
              >
                <AgentLogo agent={agent.cli} size={22} />
                <span>{agent.name}</span>
              </button>
            {/each}
          </div>
        </div>
      {/if}
    </div>
  </main>
</div>

{#if browsingFolder}
  <FolderBrowser
    initialPath={cwd}
    onPick={pickFolder}
    onClose={() => (browsingFolder = false)}
  />
{/if}

{#if settingsOpen}
  <AgentSettingsModal onClose={() => (settingsOpen = false)} />
{/if}

{#if pendingClose}
  <ConfirmDialog
    title={t("page.agents.window.closeTitle", { name: pendingClose.label })}
    body={t("page.agents.window.closeBody")}
    confirmLabel={t("page.agents.window.closeAction")}
    tone="danger"
    onConfirm={() => pendingClose && void closeNow(pendingClose)}
    onCancel={() => (pendingClose = null)}
  />
{/if}

<style>
  .workspace {
    display: flex;
    height: 100%;
    min-height: 0;
    background: var(--rb-bg0);
    color: var(--rb-text);
    font-family: var(--rb-font);
    -webkit-font-smoothing: antialiased;
  }

  .stage {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }

  .panes {
    position: relative;
    flex: 1;
    min-height: 0;
  }

  /* Apiladas en el mismo lugar; solo la elegida recibe clics y se ve. */
  .pane {
    position: absolute;
    inset: 0;
    visibility: hidden;
  }

  .pane.is-active {
    visibility: visible;
  }

  .error {
    position: absolute;
    top: 10px;
    left: 50%;
    z-index: 5;
    max-width: 70%;
    margin: 0;
    border-radius: 10px;
    padding: 8px 12px;
    background: color-mix(in sRGB, var(--rb-record) 14%, var(--rb-surface));
    color: var(--rb-text);
    font-size: 12px;
    transform: translateX(-50%);
  }

  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    height: 100%;
    padding: 24px;
    text-align: center;
  }

  .empty-title {
    margin: 0;
    font-size: 18px;
    font-weight: 600;
    text-wrap: balance;
  }

  .empty-hint {
    margin: 0 0 14px;
    color: var(--rb-muted);
    font-size: 13px;
  }

  .empty-agents {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 8px;
    max-width: 520px;
  }

  .empty-agent {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    width: 112px;
    border: 0;
    border-radius: 14px;
    padding: 14px 8px;
    background: var(--rb-surface);
    color: var(--rb-text);
    font: inherit;
    font-size: 12.5px;
    cursor: pointer;
    box-shadow: 0 0 0 1px color-mix(in sRGB, var(--rb-text) 8%, transparent);
    transition:
      box-shadow 120ms ease,
      scale 120ms ease;
  }

  .empty-agent:hover {
    box-shadow: 0 0 0 1px color-mix(in sRGB, var(--accent) 50%, transparent);
  }

  .empty-agent:active {
    scale: 0.96;
  }
</style>
