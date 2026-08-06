<script lang="ts">
  /**
   * Demo Claude Code → chat real sobre el store canónico.
   *
   * Spawnea el CLI local (sin leer tokens). El transcript usa AgentConversation
   * (mensajes, tools, thinking, plan). Historial vía agent_threads + resume.
   */
  import { onMount, tick } from "svelte";
  import { open as openDialog } from "@tauri-apps/plugin-dialog";
  import { agents } from "$lib/agentSessions.svelte";
  import {
    agentBackends,
    agentClaudeSessions,
    agentClaudeTranscript,
    agentListModels,
    agentThread,
    agentThreadDelete,
    agentThreads,
  } from "$ipc/agents";
  import {
    effortShortLabel,
    modeShortLabel,
    modelLabelFor,
    PERMISSION_MODES,
    rememberCwd,
    rememberMode,
    rememberModel,
    rememberedCwd,
    rememberedMode,
    rememberedModel,
  } from "$lib/agentModels";
  import AgentConversation from "$lib/AgentConversation.svelte";
  import PickerMenu from "$lib/PickerMenu.svelte";
  import EmptyState from "$lib/ui/EmptyState.svelte";
  import SlashPalette from "./SlashPalette.svelte";
  import { resolveSlashCommands, skillsAsCommands } from "./slashCatalog";
  import type {
    AgentItem,
    AgentModel,
    AgentTurn,
    ClaudeCodeSession,
    SlashCommand,
    StoredThread,
  } from "$lib/types";

  let {
    variant = "panel",
  }: {
    variant?: "panel" | "float";
  } = $props();

  const BACKEND = "claude-code";
  const ACCENT = "#da7756";
  /** Effort de Claude Code (no va por modelo como Cursor). */
  const EFFORT_KEY = "atic.agents.claude-code.effort";

  let draft = $state("");
  let sessionId = $state<string | null>(null);
  let available = $state<boolean | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let logEl = $state<HTMLElement | null>(null);
  let cwd = $state("");
  let model = $state("");
  let models = $state<AgentModel[]>([]);
  let modelsLoading = $state(false);
  let modelMenuOpen = $state(false);
  let effortMenuOpen = $state(false);
  let modeMenuOpen = $state(false);
  let effort = $state("");
  /** Modo de permisos (manual / acceptEdits / plan / bypass). */
  let mode = $state(rememberedMode(BACKEND));
  /** Evita spawns concurrentes al calentar la sesión. */
  let starting = $state(false);

  let historyOpen = $state(false);
  let threads = $state<StoredThread[]>([]);
  let threadsLoading = $state(false);
  /** Sesiones del CLI en `~/.claude/projects` para el cwd actual. */
  let cliSessions = $state<ClaudeCodeSession[]>([]);
  let cliLoading = $state(false);
  /** Aviso tras reanudar del CLI. */
  let cliResumed = $state(false);
  let resumeNote = $state("");
  /** Sesión elegida en el picker; pide modo (como el diálogo de Claude Code). */
  let resumePick = $state<ClaudeCodeSession | null>(null);
  /**
   * Cómo reanudar, al estilo Claude Code:
   * - summary → /compact tras resume
   * - full → historial completo en UI + contexto intacto
   * - context → solo contexto en Claude, chat vacío en Atic
   */
  type ResumeMode = "summary" | "full" | "context";
  const RESUME_PREF_KEY = "atic.agents.resumeMode";
  /** Hilo guardado en modo lectura (sin proceso vivo). */
  let archive = $state<StoredThread | null>(null);
  let archiveLoading = $state(false);
  /** Diálogo de compactar contexto (sesión viva). */
  let compactOpen = $state(false);
  let compactKeep = $state("");
  /** Índice activo del menú `/`. */
  let slashIndex = $state(0);

  const modelOptions = $derived(
    models.map((m) => ({
      id: m.id,
      label: m.name || m.id,
      note: m.description || undefined,
    })),
  );

  const EFFORT_LEVELS = [
    { id: "low", label: "Low", note: "Rápido" },
    { id: "medium", label: "Medium", note: "Equilibrado" },
    { id: "high", label: "High", note: "Más pensamiento" },
    { id: "xhigh", label: "Extra high", note: "Profundo" },
    { id: "max", label: "Max", note: "Máximo" },
    { id: "auto", label: "Auto", note: "Lo decide Claude" },
  ] as const;

  const effortOptions = $derived(
    EFFORT_LEVELS.map((e) => ({
      id: e.id,
      label: e.label,
      note: e.note,
    })),
  );

  const modeOptions = PERMISSION_MODES.map((m) => ({
    id: m.id,
    label: m.label,
    note: m.note,
  }));

  const session = $derived(agents.byId(sessionId));
  const working = $derived(session?.status === "working" || busy);

  const effortLabel = $derived(
    effort
      ? effortShortLabel(effort)
      : session?.effort
        ? effortShortLabel(session.effort)
        : "Effort",
  );
  const modeLabel = $derived(
    modeShortLabel(mode || session?.mode || "manual"),
  );
  const contextChip = $derived.by((): string | null => {
    const tokens = session?.contextTokens ?? 0;
    if (tokens <= 0) return null;
    const used = formatTokenCount(tokens);
    const size = session?.contextSize;
    if (size && size > 0) return `${used} / ${formatTokenCount(size)}`;
    return used;
  });
  const waiting = $derived((session?.pending.length ?? 0) > 0);
  const folderLabel = $derived(
    cwd ? (cwd.split(/[\\/]/).filter(Boolean).pop() ?? cwd) : "Carpeta",
  );
  const modelLabel = $derived(
    modelsLoading && models.length === 0
      ? "Modelos…"
      : modelLabelFor(
          model,
          models.map((m) => ({ id: m.id, name: m.name })),
        ) || "Modelo",
  );
  const liveTurns = $derived(session?.turns ?? []);
  const viewTurns = $derived(archive ? archive.turns : liveTurns);

  const conversationItems = $derived.by((): AgentItem[] =>
    viewTurns.flatMap((t) => t.items),
  );

  const compacting = $derived(
    !archive &&
      working &&
      conversationItems.some(
        (i) =>
          i.kind === "notice" && i.text.startsWith("Compactando el contexto"),
      ),
  );

  const statusLabel = $derived(
    available === false
      ? "sin CLI"
      : archive
        ? "archivo"
        : waiting
          ? "permiso"
          : compacting
            ? "compactando"
            : working
              ? "trabajando"
              : sessionId
                ? (session?.status ?? "lista")
                : "lista",
  );

  const turnEnds = $derived.by((): Map<string, number | null> => {
    const map = new Map<string, number | null>();
    for (const turn of viewTurns) {
      if (turn.status === "running" || turn.items.length === 0) continue;
      const last = turn.items[turn.items.length - 1];
      map.set(last.id, turn.costUsd);
    }
    return map;
  });

  const streamingLive = $derived(
    !archive &&
      conversationItems.some(
        (i) =>
          (i.kind === "message" || i.kind === "reasoning") && i.streaming,
      ),
  );

  const ctaDisabled = $derived(
    available === false ||
      waiting ||
      !!archive ||
      (!working && !draft.trim()),
  );
  const ctaLabel = $derived(
    working && !waiting ? "Detener" : sessionId ? "Enviar" : "Iniciar",
  );

  /**
   * Catálogo del handshake; si aún no llegó, cache + skills de disco +
   * fallback local — `/` no espera el spawn del CLI.
   */
  const slashCommands = $derived(
    resolveSlashCommands(
      session?.commands,
      agents.catalog[BACKEND],
      skillsAsCommands(agents.skills),
    ),
  );

  /**
   * `/algo` al inicio del draft (sin espacio todavía) → menú abierto.
   * Con espacio ya es el comando + args y el palette cierra.
   */
  const slashQuery = $derived.by((): string | null => {
    if (archive || available === false) return null;
    const m = draft.match(/^\/([^\s]*)$/);
    return m ? m[1] : null;
  });

  const slashFiltered = $derived.by((): SlashCommand[] => {
    if (slashQuery === null) return [];
    const q = slashQuery.toLowerCase();
    if (!q) return slashCommands;
    return slashCommands.filter((c) => {
      const name = c.name.toLowerCase();
      if (name.startsWith(q)) return true;
      const hay = `${c.description} ${c.argumentHint}`.toLowerCase();
      return hay.includes(q);
    });
  });

  const slashOpen = $derived(slashQuery !== null);

  const slashActive = $derived(
    slashFiltered.length === 0
      ? 0
      : Math.min(slashIndex, slashFiltered.length - 1),
  );

  $effect(() => {
    agents.watch(sessionId);
  });

  $effect(() => {
    void conversationItems.length;
    void session?.status;
    void archive?.id;
    void tick().then(() => {
      if (logEl) logEl.scrollTop = logEl.scrollHeight;
    });
  });

  $effect(() => {
    const live = session?.model;
    if (live && live !== model) model = live;
  });

  $effect(() => {
    const live = session?.mode;
    // Solo modos de permiso reales; un ResumeMode ("full") no debe contaminar.
    if (
      live &&
      live !== mode &&
      PERMISSION_MODES.some((m) => m.id === live)
    ) {
      mode = live;
    }
  });

  function ago(seconds: number): string {
    const diff = Math.max(0, Math.floor(Date.now() / 1000 - seconds));
    if (diff < 60) return "ahora";
    if (diff < 3600) return `${Math.floor(diff / 60)}m`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h`;
    return `${Math.floor(diff / 86400)}d`;
  }

  function formatTokenCount(n: number): string {
    if (n >= 1_000_000) {
      const m = n / 1_000_000;
      return `${m >= 10 ? Math.round(m) : m.toFixed(1).replace(/\.0$/, "")}M`;
    }
    if (n >= 1000) {
      const k = n / 1000;
      return `${k >= 10 ? Math.round(k) : k.toFixed(1).replace(/\.0$/, "")}k`;
    }
    return String(n);
  }

  async function refreshAvailability() {
    try {
      const list = await agentBackends();
      available = list.find((b) => b.id === BACKEND)?.available ?? false;
    } catch (err) {
      available = false;
      error = String(err);
    }
  }

  async function loadModels() {
    modelsLoading = true;
    try {
      const list = await agentListModels(BACKEND);
      models = list;
      const remembered = rememberedModel(
        BACKEND,
        list.map((m) => ({ id: m.id, label: m.name, note: m.description })),
      );
      if (remembered) model = remembered;
      else if (list[0] && !model) model = list[0].id;
    } catch (err) {
      error = String(err);
    } finally {
      modelsLoading = false;
    }
  }

  async function loadThreads() {
    threadsLoading = true;
    try {
      const list = await agentThreads();
      threads = list.filter((t) => t.backendId === BACKEND);
    } catch (err) {
      error = String(err);
    } finally {
      threadsLoading = false;
    }
  }

  async function loadCliSessions() {
    if (!cwd.trim()) {
      cliSessions = [];
      return;
    }
    cliLoading = true;
    try {
      cliSessions = await agentClaudeSessions(cwd);
    } catch (err) {
      error = String(err);
      cliSessions = [];
    } finally {
      cliLoading = false;
    }
  }

  async function toggleHistory() {
    historyOpen = !historyOpen;
    modelMenuOpen = false;
    if (historyOpen) {
      await Promise.all([loadThreads(), loadCliSessions()]);
    }
  }

  function askResumeMode(s: ClaudeCodeSession) {
    resumePick = s;
    modelMenuOpen = false;
  }

  function cancelResumePick() {
    resumePick = null;
  }

  async function confirmResume(resumeMode: ResumeMode, remember: boolean) {
    const s = resumePick;
    if (!s) return;
    resumePick = null;
    if (remember) {
      try {
        localStorage.setItem(RESUME_PREF_KEY, resumeMode);
      } catch {
        /* ignore */
      }
    }
    await resumeCli(s, resumeMode);
  }

  async function resumeCli(s: ClaudeCodeSession, resumeMode: ResumeMode) {
    const chosen = resumeMode;
    error = null;
    busy = true;
    modelMenuOpen = false;
    historyOpen = false;
    archive = null;
    try {
      if (sessionId) await agents.stop(sessionId);
      cwd = s.cwd || cwd;
      if (cwd) rememberCwd(BACKEND, cwd);

      let prior: AgentTurn[] = [];
      if (chosen === "full") {
        try {
          prior = await agentClaudeTranscript(cwd || s.cwd, s.id);
        } catch (err) {
          console.warn("transcript CLI:", err);
        }
      }

      // Ojo: `resumeMode` ("full"|"summary"|"context") NO es permission-mode.
      const permissionMode =
        PERMISSION_MODES.some((m) => m.id === mode)
          ? mode
          : rememberedMode(BACKEND);

      const id = await agents.start(BACKEND, {
        resume: s.id,
        cwd: cwd || undefined,
        model: model || undefined,
        permissionMode,
      });
      sessionId = id;
      agents.watch(id);
      agents.hydrate(id, {
        turns: prior,
        cwd,
        model,
        providerSession: s.id,
      });

      if (chosen === "summary") {
        // Igual que “Resume from summary” en Claude Code: compacta el contexto.
        await agents.compact(id);
        resumeNote =
          "Reanudada desde resumen. Compactando contexto (/compact)…";
      } else if (chosen === "full") {
        resumeNote =
          "Sesión completa. Historial en pantalla; Claude tiene el contexto intacto.";
      } else {
        resumeNote =
          "Solo contexto. Claude conserva la sesión; el chat de Atic empieza vacío.";
      }
      cliResumed = true;
    } catch (err) {
      error = String(err);
      cliResumed = false;
      resumeNote = "";
    } finally {
      busy = false;
    }
  }

  async function openArchive(id: string) {
    archiveLoading = true;
    error = null;
    try {
      const full = await agentThread(id);
      if (!full) {
        error = "No se encontró esa conversación.";
        return;
      }
      archive = full;
      historyOpen = false;
    } catch (err) {
      error = String(err);
    } finally {
      archiveLoading = false;
    }
  }

  function leaveArchive() {
    archive = null;
  }

  async function resumeArchive() {
    const thread = archive;
    if (!thread?.providerSession) {
      error = "Esta conversación no se puede reanudar (sin id del proveedor).";
      return;
    }
    error = null;
    busy = true;
    modelMenuOpen = false;
    try {
      if (sessionId) await agents.stop(sessionId);
      if (thread.cwd) {
        cwd = thread.cwd;
        rememberCwd(BACKEND, thread.cwd);
      }
      if (thread.model) {
        model = thread.model;
        rememberModel(BACKEND, thread.model);
      }
      const id = await agents.start(BACKEND, {
        resume: thread.providerSession,
        cwd: thread.cwd || undefined,
        model: thread.model || undefined,
        permissionMode: PERMISSION_MODES.some((m) => m.id === mode)
          ? mode
          : rememberedMode(BACKEND),
      });
      sessionId = id;
      agents.watch(id);
      agents.hydrate(id, {
        turns: thread.turns as AgentTurn[],
        cwd: thread.cwd,
        model: thread.model,
        providerSession: thread.providerSession,
      });
      archive = null;
      cliResumed = true;
    } catch (err) {
      error = String(err);
    } finally {
      busy = false;
    }
  }

  async function deleteThread(id: string) {
    try {
      await agentThreadDelete(id);
      threads = threads.filter((t) => t.id !== id);
      if (archive?.id === id) archive = null;
    } catch (err) {
      error = String(err);
    }
  }

  async function pickFolder() {
    if (sessionId || archive) return;
    try {
      const chosen = await openDialog({ directory: true, multiple: false });
      if (typeof chosen === "string") {
        cwd = chosen;
        rememberCwd(BACKEND, chosen);
        if (historyOpen) void loadCliSessions();
      }
    } catch (err) {
      error = String(err);
    }
  }

  async function pickModel(next: string) {
    model = next;
    rememberModel(BACKEND, next);
    modelMenuOpen = false;
    if (sessionId && next) {
      try {
        await agents.setModel(sessionId, next, effort || undefined);
      } catch (err) {
        error = String(err);
      }
    }
  }

  async function pickEffort(level: string) {
    effort = level;
    try {
      localStorage.setItem(EFFORT_KEY, level);
    } catch {
      /* ignore */
    }
    effortMenuOpen = false;
    error = null;
    const id = await ensureSession();
    if (!id || archive) return;
    try {
      // Solo effort: no pisar el modelo vivo con string vacío.
      if (model) await agents.setModel(id, model, level);
      else await agents.send(id, `/effort ${level}`);
    } catch (err) {
      error = String(err);
    }
  }

  async function pickMode(next: string) {
    if (!PERMISSION_MODES.some((m) => m.id === next)) return;
    const hadSession = !!sessionId;
    mode = next;
    rememberMode(BACKEND, next);
    modeMenuOpen = false;
    modelMenuOpen = false;
    effortMenuOpen = false;
    error = null;
    if (archive) return;
    const id = await ensureSession();
    if (!id) return;
    // Sesión nueva ya arrancó con --permission-mode; solo reenviar en caliente.
    if (!hadSession) return;
    try {
      await agents.send(id, `/permissions ${next}`);
    } catch (err) {
      error = String(err);
    }
  }

  /**
   * Calienta el CLI (handshake → catálogo de `/`) sin mandar un mensaje.
   * Se llama al enfocar el composer o al escribir `/`.
   */
  async function ensureSession(): Promise<string | null> {
    if (sessionId) return sessionId;
    if (archive || available === false || starting) return sessionId;
    starting = true;
    error = null;
    try {
      const id = await agents.start(BACKEND, {
        cwd: cwd || undefined,
        model: model || undefined,
        effort: effort || undefined,
        permissionMode: PERMISSION_MODES.some((m) => m.id === mode)
          ? mode
          : rememberedMode(BACKEND),
      });
      sessionId = id;
      agents.watch(id);
      return id;
    } catch (err) {
      error = String(err);
      return null;
    } finally {
      starting = false;
    }
  }

  function openCompact() {
    if (working || archive) return;
    compactKeep = "";
    compactOpen = true;
    historyOpen = false;
    modelMenuOpen = false;
    effortMenuOpen = false;
    modeMenuOpen = false;
  }

  async function runCompact() {
    if (working || archive) return;
    const keep = compactKeep.trim();
    compactOpen = false;
    error = null;
    const id = await ensureSession();
    if (!id) return;
    try {
      await agents.compact(id, keep || undefined);
      resumeNote = keep
        ? "Compactando con instrucciones de conservación…"
        : "Compactando el contexto…";
      cliResumed = true;
    } catch (err) {
      error = String(err);
    }
  }

  async function send() {
    const text = draft.trim();
    if (!text || working || available === false || archive) return;

    // `/effort` solo → select, no mandar el usage al chat.
    if (/^\/effort\s*$/i.test(text)) {
      draft = "";
      effortMenuOpen = true;
      modelMenuOpen = false;
      modeMenuOpen = false;
      void ensureSession();
      return;
    }
    if (/^\/compact\s*$/i.test(text)) {
      draft = "";
      openCompact();
      void ensureSession();
      return;
    }
    if (/^\/model\s*$/i.test(text)) {
      draft = "";
      modelMenuOpen = true;
      effortMenuOpen = false;
      modeMenuOpen = false;
      return;
    }
    if (/^\/permissions\s*$/i.test(text)) {
      draft = "";
      modeMenuOpen = true;
      modelMenuOpen = false;
      effortMenuOpen = false;
      void ensureSession();
      return;
    }
    if (/^\/plan\s*$/i.test(text)) {
      draft = "";
      void pickMode("plan");
      return;
    }

    error = null;
    cliResumed = false;
    modelMenuOpen = false;
    effortMenuOpen = false;
    modeMenuOpen = false;
    busy = true;
    const pending = text;
    draft = "";
    try {
      const id = await ensureSession();
      if (!id) {
        draft = pending;
        return;
      }
      await agents.send(id, pending);
      void loadThreads();
    } catch (err) {
      error = String(err);
      draft = pending;
    } finally {
      busy = false;
    }
  }

  async function stop() {
    const id = sessionId;
    if (!id) return;
    error = null;
    modelMenuOpen = false;
    sessionId = null;
    busy = false;
    try {
      await agents.stop(id);
      void loadThreads();
    } catch (err) {
      error = String(err);
    }
  }

  async function approveFirst() {
    const p = session?.pending[0];
    if (!sessionId || !p) return;
    try {
      await agents.decide(sessionId, p.id, "allow");
    } catch (err) {
      error = String(err);
    }
  }

  function pickSlash(cmd: SlashCommand) {
    slashIndex = 0;
    if (cmd.name === "compact") {
      draft = "";
      void ensureSession().then(() => openCompact());
      return;
    }
    if (cmd.name === "effort") {
      draft = "";
      effortMenuOpen = true;
      modelMenuOpen = false;
      modeMenuOpen = false;
      void ensureSession();
      return;
    }
    if (cmd.name === "model") {
      draft = "";
      modelMenuOpen = true;
      effortMenuOpen = false;
      modeMenuOpen = false;
      return;
    }
    if (cmd.name === "permissions") {
      draft = "";
      modeMenuOpen = true;
      modelMenuOpen = false;
      effortMenuOpen = false;
      void ensureSession();
      return;
    }
    if (cmd.name === "plan") {
      draft = "";
      void pickMode("plan");
      return;
    }
    // Listo para args, o el nombre solo si no pide nada.
    draft = cmd.argumentHint ? `/${cmd.name} ` : `/${cmd.name}`;
  }

  function onKey(e: KeyboardEvent) {
    if (slashOpen) {
      if (slashFiltered.length > 0 && e.key === "ArrowDown") {
        e.preventDefault();
        slashIndex = (slashActive + 1) % slashFiltered.length;
        return;
      }
      if (slashFiltered.length > 0 && e.key === "ArrowUp") {
        e.preventDefault();
        slashIndex =
          (slashActive - 1 + slashFiltered.length) % slashFiltered.length;
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        draft = draft.replace(/^\/[^\s]*$/, "");
        return;
      }
      if (
        slashFiltered.length > 0 &&
        ((e.key === "Enter" && !e.shiftKey) || e.key === "Tab")
      ) {
        const cmd = slashFiltered[slashActive];
        if (cmd) {
          e.preventDefault();
          pickSlash(cmd);
          return;
        }
      }
    }
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      if (working || archive) return;
      void send();
    }
  }

  /**
   * Al cerrar el float se desmonta este componente; la sesión del store sigue
   * viva. Al reabrir, reenganchamos el hilo en curso para no perder el chat.
   */
  function adoptLiveSession() {
    if (sessionId) return;
    const live = [...agents.sessions]
      .reverse()
      .find(
        (s) =>
          s.backendId === BACKEND &&
          (s.status === "ready" ||
            s.status === "working" ||
            s.status === "waiting"),
      );
    if (!live) return;
    sessionId = live.id;
    agents.watch(live.id);
    if (live.cwd) cwd = live.cwd;
    if (live.model) model = live.model;
    if (live.mode && PERMISSION_MODES.some((m) => m.id === live.mode)) {
      mode = live.mode;
    }
    if (live.effort) effort = live.effort;
  }

  onMount(() => {
    cwd = rememberedCwd(BACKEND);
    // Si un resume previo contaminó el modo con "full", volver al recordado.
    if (!PERMISSION_MODES.some((m) => m.id === mode)) {
      mode = rememberedMode(BACKEND);
    }
    try {
      effort = localStorage.getItem(EFFORT_KEY) || "";
    } catch {
      effort = "";
    }
    void (async () => {
      await agents.init();
      adoptLiveSession();
      // Skills de disco: catálogo rico sin esperar el handshake.
      void agents.loadSkills(cwd || undefined);
    })();
    void refreshAvailability();
    void loadModels();
    void loadThreads();
    const onWinKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        if (modelMenuOpen || effortMenuOpen || modeMenuOpen) {
          e.preventDefault();
          modelMenuOpen = false;
          effortMenuOpen = false;
          modeMenuOpen = false;
        }
      }
    };
    window.addEventListener("keydown", onWinKey);
    return () => {
      window.removeEventListener("keydown", onWinKey);
      agents.watch(null);
    };
  });

  // Al cambiar carpeta, releer skills (son por proyecto + usuario).
  $effect(() => {
    const folder = cwd;
    if (archive) return;
    void agents.loadSkills(folder || undefined);
  });

  // Al escribir `/`, calentar el CLI para traer el catálogo completo del handshake.
  $effect(() => {
    if (slashQuery !== null && !sessionId && !archive && available === true) {
      void ensureSession();
    }
  });
</script>

<div
  class="demo"
  class:is-float={variant === "float"}
  class:is-panel={variant === "panel"}
  class:is-menu-open={modelMenuOpen ||
    effortMenuOpen ||
    modeMenuOpen ||
    historyOpen ||
    !!resumePick ||
    compactOpen ||
    slashOpen}
  style="--accent: {ACCENT}"
  data-demo="claude-code"
  data-agent="claude-code"
>
  <header class="top">
    <h2
      class="name"
      title={archive
        ? "Archivo · solo lectura"
        : "Claude Code · login local del CLI"}
    >
      Claude
    </h2>
    <div class="top-acts" data-no-drag>
      {#if sessionId && !archive}
        <button
          type="button"
          class="icon-btn"
          class:is-on={compactOpen}
          disabled={working || available === false}
          aria-label="Compactar contexto"
          title="Compactar contexto (/compact)"
          onclick={() => openCompact()}
        >
          <svg viewBox="0 0 24 24" width="13" height="13" aria-hidden="true">
            <path
              d="M4 7h16M7 12h10M9 17h6"
              fill="none"
              stroke="currentColor"
              stroke-width="1.8"
              stroke-linecap="round"
            />
          </svg>
        </button>
      {/if}
      <button
        type="button"
        class="icon-btn"
        class:is-on={historyOpen}
        aria-label="Historial"
        title="Historial"
        onclick={() => void toggleHistory()}
      >
        <svg viewBox="0 0 24 24" width="13" height="13" aria-hidden="true">
          <path
            d="M12 8v5l3 2M4.5 12a7.5 7.5 0 1 0 3-6"
            fill="none"
            stroke="currentColor"
            stroke-width="1.8"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
          <path
            d="M4 5v4h4"
            fill="none"
            stroke="currentColor"
            stroke-width="1.8"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
        </svg>
      </button>
      {#if !archive && available !== false}
        <div class="mode-pick">
          <PickerMenu
            label={modeLabel}
            open={modeMenuOpen}
            options={modeOptions}
            value={mode || session?.mode || "manual"}
            title="Modo de permisos: {modeLabel}"
            ariaLabel="Modo de permisos"
            onToggle={() => {
              modeMenuOpen = !modeMenuOpen;
              if (modeMenuOpen) {
                modelMenuOpen = false;
                effortMenuOpen = false;
              }
            }}
            onPick={(id) => void pickMode(id)}
          />
        </div>
      {/if}
      {#if contextChip}
        <p class="badge is-ctx" title="Contexto usado">
          {contextChip}
        </p>
      {/if}
      <p
        class="badge"
        class:is-live={available === true && !archive}
        class:is-off={available === false}
        class:is-busy={working && available !== false && !archive}
        class:is-arch={!!archive}
        class:is-plan={(mode || session?.mode) === "plan" && !archive}
        title={available === false
          ? "Claude Code no está en el PATH"
          : statusLabel}
      >
        <span class="badge-dot" aria-hidden="true"></span>
        <span class="badge-t">
          {#if available === null}
            …
          {:else if available}
            {statusLabel}
          {:else}
            CLI
          {/if}
        </span>
      </p>
    </div>
  </header>

  {#if historyOpen}
    <aside class="hist" aria-label="Conversaciones guardadas">
      <div class="hist-h">
        <span>Historial</span>
        <button
          type="button"
          class="icon-btn"
          data-no-drag
          aria-label="Cerrar historial"
          title="Cerrar"
          onclick={() => (historyOpen = false)}
        >
          <svg viewBox="0 0 24 24" width="12" height="12" aria-hidden="true">
            <path
              d="M6 6l12 12M18 6L6 18"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              stroke-linecap="round"
            />
          </svg>
        </button>
      </div>

      <p class="hist-sec" title="Igual que /resume en Claude Code">
        CLI · esta carpeta
      </p>
      {#if !cwd.trim()}
        <div class="hist-empty-wrap">
          <EmptyState title="Elegí una carpeta" hint="Para ver sesiones del CLI.">
            {#snippet action()}
              <button
                type="button"
                class="chip"
                data-no-drag
                disabled={!!sessionId || !!archive}
                onclick={() => void pickFolder()}
              >
                Carpeta
              </button>
            {/snippet}
          </EmptyState>
        </div>
      {:else if cliLoading}
        <p class="hist-empty">Buscando…</p>
      {:else if cliSessions.length === 0}
        <div class="hist-empty-wrap">
          <EmptyState title="Sin sesiones CLI" hint="Probá otra carpeta." />
        </div>
      {:else}
        <ul class="hist-list">
          {#each cliSessions as s (s.id)}
            <li>
              <button
                type="button"
                class="hist-row"
                data-no-drag
                title="Elegir cómo reanudar"
                onclick={() => askResumeMode(s)}
              >
                <span class="hist-prev">{s.preview || s.id}</span>
                <span class="hist-meta">
                  <span class="hist-ago">{ago(s.updatedAt)}</span>
                  <span class="hist-tag">CLI</span>
                </span>
              </button>
            </li>
          {/each}
        </ul>
      {/if}

      <p class="hist-sec">Atic</p>
      {#if threadsLoading}
        <p class="hist-empty">Cargando…</p>
      {:else if threads.length === 0}
        <div class="hist-empty-wrap">
          <EmptyState title="Sin historial" hint="Aparece al cerrar un turno." />
        </div>
      {:else}
        <ul class="hist-list">
          {#each threads as t (t.id)}
            <li>
              <button
                type="button"
                class="hist-row"
                data-no-drag
                onclick={() => void openArchive(t.id)}
              >
                <span class="hist-prev">{t.preview || "Sin título"}</span>
                <span class="hist-meta">
                  <span class="hist-ago">{ago(t.updatedAt)}</span>
                  {#if t.providerSession}
                    <span class="hist-tag">reanudable</span>
                  {/if}
                </span>
              </button>
              <button
                type="button"
                class="hist-del"
                data-no-drag
                aria-label="Borrar conversación"
                title="Borrar"
                onclick={() => void deleteThread(t.id)}
              >
                ×
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    </aside>
  {/if}

  <div class="log" bind:this={logEl} role="log" aria-label="Conversación">
    {#if available === false}
      <div class="empty">
        <EmptyState
          title="Sin CLI"
          hint="Instalá Claude Code y ejecutá claude auth login."
        />
      </div>
    {:else if archiveLoading}
      <div class="empty">
        <EmptyState title="Abriendo…" />
      </div>
    {:else if conversationItems.length === 0 && !working}
      <div class="empty">
        <EmptyState
          title={cwd.trim() ? "Escribí abajo" : "Elegí carpeta"}
          hint={cwd.trim()
            ? "El historial guarda cada turno."
            : "Después, el primer mensaje."}
        >
          {#snippet action()}
            {#if !cwd.trim() && !sessionId && !archive}
              <button
                type="button"
                class="chip"
                data-no-drag
                onclick={() => void pickFolder()}
              >
                Carpeta
              </button>
            {/if}
          {/snippet}
        </EmptyState>
      </div>
    {:else}
      <div class="thread">
        <AgentConversation items={conversationItems} {turnEnds} />
      </div>
    {/if}

    {#if working && !waiting && !streamingLive && !archive}
      <p class="live" aria-live="polite">
        <span class="live-dot" aria-hidden="true"></span>
        {compacting ? "compactando…" : "…"}
      </p>
    {/if}
  </div>

  {#if compactOpen}
    <div
      class="resume-dlg"
      role="dialog"
      aria-modal="true"
      aria-label="Compactar contexto"
      data-no-drag
    >
      <div class="resume-card">
        <p class="resume-t">Compactar contexto</p>
        <p class="resume-d is-full">
          Igual que <code>/compact</code> en Claude Code: resume la conversación
          en un resumen para liberar tokens. El historial local del CLI se
          conserva; en Atic queda el resumen.
        </p>
        <label class="compact-keep">
          <span class="compact-keep-l">Conservar (opcional)</span>
          <textarea
            class="compact-keep-in"
            rows="3"
            placeholder="Ej: decisión de usar Postgres, error pendiente en auth.ts…"
            bind:value={compactKeep}
          ></textarea>
        </label>
        <div class="resume-opts">
          <button
            type="button"
            class="resume-opt"
            onclick={() => void runCompact()}
          >
            <span class="resume-opt-t">Compactar</span>
            <span class="resume-opt-d"
              >Genera el resumen y recorta el chat visible.</span
            >
          </button>
          <button
            type="button"
            class="resume-opt"
            onclick={() => (compactOpen = false)}
          >
            <span class="resume-opt-t">Cancelar</span>
            <span class="resume-opt-d">Seguir con el historial intacto.</span>
          </button>
        </div>
      </div>
    </div>
  {/if}

  {#if resumePick}
    <div
      class="resume-dlg"
      role="dialog"
      aria-modal="true"
      aria-label="Cómo reanudar la sesión"
      data-no-drag
    >
      <div class="resume-card">
        <p class="resume-t">¿Cómo reanudamos?</p>
        <p class="resume-d">
          {resumePick.preview || resumePick.id}
        </p>
        <div class="resume-opts">
          <button
            type="button"
            class="resume-opt"
            onclick={() => void confirmResume("summary", false)}
          >
            <span class="resume-opt-t">Desde resumen</span>
            <span class="resume-opt-d"
              >Como en Claude Code: corre <code>/compact</code> y sigue con el
              resumen (menos tokens).</span
            >
          </button>
          <button
            type="button"
            class="resume-opt is-go"
            onclick={() => void confirmResume("full", false)}
          >
            <span class="resume-opt-t">Sesión completa</span>
            <span class="resume-opt-d"
              >Historial completo en pantalla y contexto intacto en Claude.</span
            >
          </button>
          <button
            type="button"
            class="resume-opt"
            onclick={() => void confirmResume("context", false)}
          >
            <span class="resume-opt-t">Solo contexto</span>
            <span class="resume-opt-d"
              >Sin pintar mensajes viejos en Atic; Claude conserva la sesión.</span
            >
          </button>
        </div>
        <div class="resume-foot">
          <button
            type="button"
            class="chip"
            onclick={() => void confirmResume("full", true)}
          >
            Completa y recordar
          </button>
          <button type="button" class="chip" onclick={cancelResumePick}>
            Cancelar
          </button>
        </div>
      </div>
    </div>
  {/if}

  {#if cliResumed && !archive}
    <p class="cli-note" role="status">
      <span class="cli-note-t">{resumeNote || "Sesión reanudada."}</span>
      <button
        type="button"
        class="icon-btn"
        data-no-drag
        aria-label="Cerrar aviso"
        title="Cerrar"
        onclick={() => (cliResumed = false)}
      >
        <svg viewBox="0 0 24 24" width="11" height="11" aria-hidden="true">
          <path
            d="M6 6l12 12M18 6L6 18"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
          />
        </svg>
      </button>
    </p>
  {/if}

  {#if archive}
    <div class="archive-bar" data-no-drag>
      <button
        type="button"
        class="icon-btn"
        aria-label="Volver al chat"
        title="Volver"
        onclick={leaveArchive}
      >
        <svg viewBox="0 0 24 24" width="13" height="13" aria-hidden="true">
          <path
            d="M15 6l-6 6 6 6"
            fill="none"
            stroke="currentColor"
            stroke-width="1.8"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
        </svg>
      </button>
      <button
        type="button"
        class="chip is-go"
        disabled={!archive.providerSession || available === false}
        onclick={() => void resumeArchive()}
      >
        Continuar
      </button>
    </div>
  {/if}

  {#each session?.pending ?? [] as p (p.id)}
    {#if !archive}
      <div class="perm" role="alertdialog" aria-label="Permiso pendiente">
        <div class="perm-copy">
          <p class="perm-t" title={p.description}>
            <strong>{p.tool}</strong>
            {#if p.description}
              <span class="perm-w"> · {p.description}</span>
            {/if}
          </p>
        </div>
        <div class="perm-acts" data-no-drag>
          <button
            type="button"
            class="icon-btn"
            aria-label="Denegar"
            title="Denegar"
            onclick={() => void agents.decide(sessionId!, p.id, "deny")}
          >
            <svg viewBox="0 0 24 24" width="13" height="13" aria-hidden="true">
              <path
                d="M6 6l12 12M18 6L6 18"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                stroke-linecap="round"
              />
            </svg>
          </button>
          <button
            type="button"
            class="icon-btn is-go"
            aria-label="Permitir"
            title="Permitir"
            onclick={() => void approveFirst()}
          >
            <svg viewBox="0 0 24 24" width="13" height="13" aria-hidden="true">
              <path
                d="M5 12.5l5 5L19 7"
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
    {/if}
  {/each}

  {#if error || session?.error}
    <p class="warn" role="alert">{error ?? session?.error}</p>
  {/if}

  <footer class="composer">
    {#if slashOpen}
      <SlashPalette
        commands={slashFiltered}
        activeIndex={slashActive}
        emptyHint={slashCommands.length
          ? "Sin coincidencias"
          : starting
            ? "Cargando comandos…"
            : "Sin comandos"}
        onPick={pickSlash}
        onHover={(i) => (slashIndex = i)}
      />
    {/if}
    <textarea
      class="in"
      rows="1"
      placeholder={available === false
        ? "Sin CLI…"
        : archive
          ? "Solo lectura…"
          : "Mensaje o /…"}
      bind:value={draft}
      onkeydown={onKey}
      oninput={() => (slashIndex = 0)}
      onfocus={() => {
        if (!archive && available === true) void ensureSession();
      }}
      disabled={available === false || !!archive}
      aria-label="Mensaje"
    ></textarea>
    <div class="row">
      <div class="set" data-no-drag>
        <button
          type="button"
          class="chip is-folder"
          class:is-on={!!cwd}
          class:is-locked={!!sessionId || !!archive}
          title={cwd || "Elegir carpeta de trabajo"}
          aria-label={cwd ? `Carpeta: ${folderLabel}` : "Elegir carpeta"}
          disabled={!!sessionId || !!archive}
          onclick={() => void pickFolder()}
        >
          <svg viewBox="0 0 24 24" width="12" height="12" aria-hidden="true">
            <path
              d="M3 7.5A1.5 1.5 0 0 1 4.5 6H9l1.5 1.5H19.5A1.5 1.5 0 0 1 21 9v8.5a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 17.5z"
              fill="none"
              stroke="currentColor"
              stroke-width="1.7"
              stroke-linejoin="round"
            />
          </svg>
          <span class="chip-t">{folderLabel}</span>
        </button>
        {#if models.length > 0 || modelsLoading}
          <div class="model">
            <PickerMenu
              label={modelLabel}
              open={modelMenuOpen}
              options={modelOptions}
              value={model}
              loading={modelsLoading && models.length === 0}
              loadingMessage="Modelos…"
              onToggle={() => {
                modelMenuOpen = !modelMenuOpen;
                if (modelMenuOpen) effortMenuOpen = false;
              }}
              onPick={(id) => void pickModel(id)}
            />
          </div>
        {/if}
        {#if !archive && available !== false}
          <div class="model">
            <PickerMenu
              label={effortLabel}
              open={effortMenuOpen}
              options={effortOptions}
              value={effort || session?.effort || ""}
              onToggle={() => {
                effortMenuOpen = !effortMenuOpen;
                if (effortMenuOpen) modelMenuOpen = false;
              }}
              onPick={(id) => void pickEffort(id)}
            />
          </div>
        {/if}
      </div>
      <div class="acts" data-no-drag>
        <button
          type="button"
          class="send"
          class:is-stop={working && !waiting}
          disabled={ctaDisabled}
          aria-label={ctaLabel}
          title={ctaLabel}
          onclick={() => (working && !waiting ? void stop() : void send())}
        >
          {#if working && !waiting}
            <svg viewBox="0 0 24 24" width="13" height="13" aria-hidden="true">
              <rect x="6" y="6" width="12" height="12" rx="1.5" fill="currentColor" />
            </svg>
          {:else if sessionId}
            <svg viewBox="0 0 24 24" width="13" height="13" aria-hidden="true">
              <path
                d="M4 12h12M12 6l6 6-6 6"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
          {:else}
            <svg viewBox="0 0 24 24" width="13" height="13" aria-hidden="true">
              <path d="M8 5.5v13l11-6.5z" fill="currentColor" />
            </svg>
          {/if}
        </button>
      </div>
    </div>
  </footer>
</div>

<style>
  .demo {
    --pad: 0.45rem;
    --r-outer: 26px;
    --r-in: 10px;
    --r-chip: 999px;
    /* Tokens que heredan AgentConversation / ToolCard / Message */
    --coral: var(--accent, #da7756);
    --text: var(--rb-text);
    --dim: var(--rb-muted);
    --faint: var(--rb-faint);
    --line: var(--rb-border);
    --card: var(--rb-surface-2);
    --code: var(--rb-surface-2);
    --hover: color-mix(in srgb, var(--rb-text) 6%, transparent);
    --add: var(--rb-ok);
    --del: var(--rb-record);
    position: relative;
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    box-sizing: border-box;
    color: var(--rb-text);
    font-family: var(--rb-font);
    background: var(--rb-surface);
    overflow: hidden;
    --accent: #da7756;
  }

  .demo.is-float {
    border-radius: var(--r-outer);
  }

  .demo.is-float .top {
    padding-right: 2.5rem;
  }

  .demo.is-menu-open {
    overflow: visible;
  }

  .top {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    justify-content: space-between;
    gap: 0.45rem;
    border-bottom: 1px solid var(--rb-hairline);
    padding: 0.35rem 0.55rem;
  }

  .name {
    margin: 0;
    min-width: 0;
    font-size: 0.78rem;
    font-weight: 650;
    letter-spacing: -0.01em;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .top-acts {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    gap: 0.25rem;
  }

  .icon-btn {
    display: grid;
    place-items: center;
    width: 1.75rem;
    height: 1.75rem;
    border: 1px solid var(--rb-border);
    border-radius: var(--r-chip);
    padding: 0;
    background: transparent;
    color: var(--rb-muted);
    cursor: pointer;
    transition:
      color var(--duration-quick) var(--ease-smooth-out),
      background var(--duration-quick) var(--ease-smooth-out),
      border-color var(--duration-quick) var(--ease-smooth-out),
      transform var(--duration-quick) var(--ease-smooth-out);
  }

  .icon-btn:hover,
  .icon-btn.is-on {
    color: var(--rb-text);
    background: color-mix(in srgb, var(--rb-text) 6%, transparent);
  }

  .icon-btn:active {
    transform: scale(0.96);
  }

  .icon-btn.is-go {
    border-color: transparent;
    background: var(--accent);
    color: #fff;
  }

  .icon-btn.is-go:hover {
    color: #fff;
    background: color-mix(in srgb, var(--accent) 88%, #000);
  }

  .badge {
    margin: 0;
    flex-shrink: 0;
    min-height: 1.5rem;
    display: inline-flex;
    align-items: center;
    gap: 0.28rem;
    border-radius: var(--r-chip);
    padding: 0.1rem 0.4rem 0.1rem 0.3rem;
    font-size: 0.6rem;
    font-weight: 600;
    letter-spacing: 0.01em;
    font-variant-numeric: tabular-nums;
    color: var(--rb-muted);
    background: var(--rb-surface-2);
    transition:
      color var(--duration-quick) var(--ease-smooth-out),
      background var(--duration-quick) var(--ease-smooth-out);
  }

  .badge-dot {
    width: 0.35rem;
    height: 0.35rem;
    border-radius: 999px;
    background: currentColor;
    opacity: 0.7;
  }

  .badge-t {
    max-width: 5.5rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .badge.is-live {
    color: var(--rb-ok);
    background: color-mix(in srgb, var(--rb-ok) 14%, transparent);
  }

  .badge.is-off {
    color: var(--rb-warn);
    background: color-mix(in srgb, var(--rb-warn) 14%, transparent);
  }

  .badge.is-busy {
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 14%, transparent);
  }

  .badge.is-arch {
    color: var(--rb-info);
    background: color-mix(in srgb, var(--rb-info) 14%, transparent);
  }

  .badge.is-plan {
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }

  .badge.is-ctx {
    color: var(--rb-faint);
    background: color-mix(in srgb, var(--rb-text) 5%, transparent);
    text-transform: none;
    letter-spacing: 0.01em;
    font-variant-numeric: tabular-nums;
  }

  .mode-pick {
    position: relative;
    z-index: 8;
  }

  .mode-pick :global(.pm-chip) {
    min-height: 1.5rem;
    max-width: 5.5rem;
    border-color: var(--rb-border);
    color: var(--rb-muted);
    font-size: 0.6rem;
    padding: 0.08rem 0.4rem;
  }

  .mode-pick :global(.pm-chip:hover),
  .mode-pick :global(.pm-chip.is-open) {
    color: var(--rb-text);
    background: color-mix(in srgb, var(--rb-text) 6%, transparent);
  }

  .hist {
    flex-shrink: 0;
    max-height: 38%;
    overflow: auto;
    border-bottom: 1px solid var(--rb-hairline);
    background: color-mix(in srgb, var(--rb-surface) 92%, var(--rb-bg0));
    padding: 0.4rem 0.55rem 0.45rem;
  }

  .hist-h {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.35rem;
    margin-bottom: 0.25rem;
    font-size: 0.62rem;
    font-weight: 600;
    letter-spacing: 0.03em;
    text-transform: uppercase;
    color: var(--rb-faint);
  }

  .hist-sec {
    margin: 0.4rem 0 0.15rem;
    font-size: 0.55rem;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--rb-faint);
  }

  .hist-sec:first-of-type {
    margin-top: 0.05rem;
  }

  .resume-dlg {
    position: absolute;
    inset: 0;
    z-index: 20;
    display: grid;
    place-items: center;
    padding: 0.75rem;
    background: color-mix(in srgb, var(--rb-text) 28%, transparent);
  }

  .resume-card {
    width: min(22rem, 100%);
    border-radius: 14px;
    padding: 0.85rem 0.9rem 0.75rem;
    background: var(--rb-surface);
    box-shadow: 0 12px 32px color-mix(in srgb, var(--rb-text) 18%, transparent);
  }

  .resume-t {
    margin: 0;
    font-size: 0.95rem;
    font-weight: 650;
    text-wrap: balance;
  }

  .resume-d {
    margin: 0.25rem 0 0.7rem;
    font-size: 0.75rem;
    line-height: 1.35;
    color: var(--rb-muted);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .resume-d.is-full {
    display: block;
    -webkit-line-clamp: unset;
    line-clamp: unset;
    overflow: visible;
  }

  .compact-keep {
    display: flex;
    flex-direction: column;
    gap: 0.3rem;
    margin: 0 0 0.7rem;
  }

  .compact-keep-l {
    font-size: 0.7rem;
    font-weight: 600;
    color: var(--rb-muted);
  }

  .compact-keep-in {
    width: 100%;
    resize: vertical;
    border: 1px solid var(--rb-border);
    border-radius: 10px;
    padding: 0.45rem 0.55rem;
    background: var(--rb-surface-2, transparent);
    color: var(--rb-text);
    font: inherit;
    font-size: 0.75rem;
    line-height: 1.4;
  }

  .compact-keep-in:focus {
    outline: none;
    border-color: color-mix(in srgb, var(--accent) 55%, var(--rb-border));
    box-shadow: var(--rb-focus);
  }

  .resume-opts {
    display: flex;
    flex-direction: column;
    gap: 0.4rem;
  }

  .resume-opt {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.15rem;
    border: 1px solid var(--rb-border);
    border-radius: 12px;
    padding: 0.55rem 0.7rem;
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition:
      background var(--duration-quick) var(--ease-smooth-out),
      border-color var(--duration-quick) var(--ease-smooth-out),
      transform var(--duration-quick) var(--ease-smooth-out);
  }

  .resume-opt:hover {
    background: color-mix(in srgb, var(--rb-text) 5%, transparent);
  }

  .resume-opt:active {
    transform: scale(0.96);
  }

  .resume-opt.is-go {
    border-color: color-mix(in srgb, var(--accent) 45%, var(--rb-border));
    background: color-mix(in srgb, var(--accent) 10%, transparent);
  }

  .resume-opt-t {
    font-size: 0.8rem;
    font-weight: 600;
  }

  .resume-opt-d {
    font-size: 0.7rem;
    line-height: 1.35;
    color: var(--rb-muted);
    text-wrap: pretty;
  }

  .resume-opt-d code {
    font-size: 0.65rem;
    color: var(--rb-text);
  }

  .resume-foot {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 0.35rem;
    margin-top: 0.65rem;
  }

  .cli-note {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    justify-content: space-between;
    gap: 0.35rem;
    margin: 0;
    padding: 0.28rem 0.45rem 0.28rem 0.55rem;
    font-size: 0.65rem;
    line-height: 1.3;
    color: var(--rb-muted);
    background: color-mix(in srgb, var(--accent) 10%, var(--rb-surface));
    box-shadow: 0 -1px 0 var(--rb-hairline);
  }

  .cli-note-t {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .hist-empty {
    margin: 0.35rem 0;
    font-size: 0.78rem;
    color: var(--rb-muted);
  }

  .hist-empty-wrap {
    margin: 0.15rem 0 0.35rem;
  }

  .hist-empty-wrap :global(.flex) {
    padding: 0.55rem 0.35rem;
    gap: 0.35rem;
  }

  .hist-empty-wrap :global(.text-sm) {
    font-size: 0.75rem;
  }

  .hist-empty-wrap :global(.text-xs) {
    font-size: 0.68rem;
  }

  .hist-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.12rem;
  }

  .hist-list li {
    display: flex;
    align-items: stretch;
    gap: 0.15rem;
  }

  .hist-row {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.08rem;
    border: 0;
    border-radius: 9px;
    padding: 0.3rem 0.4rem;
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition:
      background var(--duration-quick) var(--ease-smooth-out),
      transform var(--duration-quick) var(--ease-smooth-out);
  }

  .hist-row:hover {
    background: color-mix(in srgb, var(--rb-text) 5%, var(--rb-surface-2));
  }

  .hist-row:active {
    transform: scale(0.96);
  }

  .hist-prev {
    width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.75rem;
    font-weight: 500;
  }

  .hist-meta {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem;
    font-size: 0.6rem;
    color: var(--rb-faint);
    font-variant-numeric: tabular-nums;
  }

  .hist-tag {
    color: var(--accent);
  }

  .hist-del {
    flex-shrink: 0;
    width: 2rem;
    border: 0;
    border-radius: 9px;
    background: transparent;
    color: var(--rb-faint);
    font-size: 1rem;
    line-height: 1;
    cursor: pointer;
    transition:
      color var(--duration-quick) var(--ease-smooth-out),
      background var(--duration-quick) var(--ease-smooth-out),
      transform var(--duration-quick) var(--ease-smooth-out);
  }

  .hist-del:hover {
    color: var(--rb-record);
    background: color-mix(in srgb, var(--rb-record) 10%, transparent);
  }

  .hist-del:active {
    transform: scale(0.96);
  }

  .log {
    display: flex;
    min-height: 0;
    flex: 1;
    flex-direction: column;
    gap: 0.4rem;
    overflow: auto;
    padding: 0.55rem 0.65rem 0.5rem;
    scrollbar-width: thin;
  }

  .thread {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }

  .empty {
    display: flex;
    flex: 1;
    flex-direction: column;
    align-items: stretch;
    justify-content: center;
    max-width: 22rem;
    padding: 0.2rem 0;
  }

  .empty :global(.flex) {
    align-items: flex-start;
    text-align: left;
    padding-left: 0.05rem;
    padding-right: 0.05rem;
    gap: 0.3rem;
  }

  .empty :global(.text-sm) {
    font-size: 0.78rem;
  }

  .empty :global(.text-xs) {
    font-size: 0.68rem;
  }

  .live {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
    margin: 0.1rem 0 0;
    font-size: 0.65rem;
    color: var(--accent);
  }

  .live-dot {
    width: 0.32rem;
    height: 0.32rem;
    border-radius: 999px;
    background: var(--accent);
    animation: pulse 1.1s var(--ease-smooth-out) infinite;
  }

  .archive-bar {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    justify-content: flex-end;
    gap: 0.3rem;
    padding: 0.3rem 0.5rem;
    background: color-mix(in srgb, var(--rb-info) 8%, var(--rb-surface));
    box-shadow: 0 -1px 0 var(--rb-hairline);
  }

  .perm {
    display: flex;
    flex-shrink: 0;
    flex-direction: row;
    align-items: center;
    gap: 0.4rem;
    margin: 0 0.5rem 0.35rem;
    border-radius: calc(var(--r-in) + 2px);
    padding: 0.3rem 0.4rem;
    background: color-mix(in srgb, var(--rb-warn) 12%, var(--rb-surface));
    box-shadow: 0 0 0 1px color-mix(in srgb, var(--rb-warn) 28%, transparent);
  }

  .perm-copy {
    min-width: 0;
    flex: 1;
  }

  .perm-t {
    margin: 0;
    font-size: 0.68rem;
    line-height: 1.3;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .perm-w {
    color: var(--rb-muted);
    font-weight: 400;
  }

  .perm-acts {
    display: flex;
    flex-shrink: 0;
    gap: 0.2rem;
  }

  .warn {
    margin: 0;
    flex-shrink: 0;
    padding: 0.3rem 0.55rem;
    font-size: 0.65rem;
    line-height: 1.3;
    color: var(--rb-warn);
    background: color-mix(in srgb, var(--rb-warn) 12%, transparent);
  }

  .composer {
    position: relative;
    z-index: 6;
    flex-shrink: 0;
    padding: var(--pad) 0.55rem 0.5rem;
    background: color-mix(in srgb, var(--rb-surface) 88%, var(--rb-bg0));
    box-shadow: 0 -1px 0 var(--rb-hairline);
  }

  .in {
    width: 100%;
    min-height: 2rem;
    max-height: 6.5rem;
    resize: none;
    border: 1px solid var(--rb-border);
    border-radius: var(--r-in);
    padding: 0.4rem 0.55rem;
    background: var(--rb-surface);
    color: var(--rb-text);
    font: inherit;
    font-size: 0.8rem;
    line-height: 1.35;
    outline: none;
    field-sizing: content;
    transition:
      border-color var(--duration-quick) var(--ease-smooth-out),
      box-shadow var(--duration-quick) var(--ease-smooth-out);
  }

  .in:disabled {
    opacity: 0.55;
  }

  .in::placeholder {
    color: color-mix(in srgb, var(--rb-text) 42%, transparent);
  }

  .in:focus {
    border-color: color-mix(in srgb, var(--accent) 55%, var(--rb-border));
    box-shadow: var(--rb-focus);
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.4rem;
    margin-top: 0.3rem;
  }

  .set,
  .acts {
    display: flex;
    min-width: 0;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.25rem;
  }

  .chip {
    display: inline-flex;
    min-height: 1.75rem;
    align-items: center;
    gap: 0.28rem;
    border: 1px solid var(--rb-border);
    border-radius: var(--r-chip);
    padding: 0.15rem 0.5rem;
    background: transparent;
    color: var(--rb-muted);
    font: inherit;
    font-size: 0.65rem;
    max-width: 7.5rem;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: pointer;
    transition:
      color var(--duration-quick) var(--ease-smooth-out),
      background var(--duration-quick) var(--ease-smooth-out),
      border-color var(--duration-quick) var(--ease-smooth-out),
      transform var(--duration-quick) var(--ease-smooth-out);
  }

  .chip-t {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .chip.is-folder {
    max-width: 6.5rem;
    padding-left: 0.4rem;
  }

  .chip:hover:not(:disabled) {
    color: var(--rb-text);
    background: color-mix(in srgb, var(--rb-text) 6%, transparent);
  }

  .chip:active:not(:disabled) {
    transform: scale(0.96);
  }

  .chip:disabled,
  .chip.is-locked {
    opacity: 0.5;
    cursor: default;
  }

  .chip.is-on {
    color: var(--rb-text);
    border-color: color-mix(in srgb, var(--accent) 40%, var(--rb-border));
  }

  .chip.is-go {
    border-color: transparent;
    background: var(--accent);
    color: #fff;
    max-width: none;
    min-height: 1.75rem;
    padding: 0.15rem 0.65rem;
  }

  .model {
    position: relative;
    z-index: 7;
    max-width: 8rem;
  }

  .model :global(.pm-chip) {
    min-height: 1.75rem;
    max-width: 8rem;
    border-color: var(--rb-border);
    color: var(--rb-muted);
    font-size: 0.65rem;
    padding: 0.1rem 0.45rem;
    transition:
      color var(--duration-quick) var(--ease-smooth-out),
      background var(--duration-quick) var(--ease-smooth-out),
      border-color var(--duration-quick) var(--ease-smooth-out),
      transform var(--duration-quick) var(--ease-smooth-out);
  }

  .model :global(.pm-chip:hover),
  .model :global(.pm-chip.is-open) {
    color: var(--rb-text);
    background: color-mix(in srgb, var(--rb-text) 6%, transparent);
  }

  .model :global(.pm-chip:active) {
    transform: scale(0.96);
  }

  .send {
    display: inline-flex;
    width: 1.85rem;
    height: 1.85rem;
    min-height: 1.85rem;
    min-width: 1.85rem;
    align-items: center;
    justify-content: center;
    border: 0;
    border-radius: var(--r-chip);
    padding: 0;
    background: var(--accent);
    color: #fff;
    font: inherit;
    cursor: pointer;
    transition:
      background var(--duration-quick) var(--ease-smooth-out),
      opacity var(--duration-quick) var(--ease-smooth-out),
      transform var(--duration-quick) var(--ease-smooth-out),
      filter var(--duration-quick) var(--ease-smooth-out);
  }

  .send:hover:not(:disabled) {
    filter: brightness(1.05);
  }

  .send:active:not(:disabled) {
    transform: scale(0.96);
  }

  .send:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .send.is-stop {
    background: var(--rb-record);
  }

  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .live-dot {
      animation: none;
    }
    .chip:active:not(:disabled),
    .send:active:not(:disabled),
    .icon-btn:active,
    .hist-row:active,
    .hist-del:active,
    .model :global(.pm-chip:active) {
      transform: none;
    }
  }
</style>
