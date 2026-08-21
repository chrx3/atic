<script lang="ts">
  /**
   * Demo Claude Code → chat real sobre el store canónico.
   *
   * Spawnea el CLI local (sin leer tokens). El transcript usa AgentConversation
   * (mensajes, tools, thinking, plan). Historial vía agent_threads + resume.
   */
  import { onMount, tick } from "svelte";
  import { agents } from "$lib/agentSessions.svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import {
    agentBackends,
    agentClaudeSessions,
    agentClaudeTranscript,
    agentListModels,
    agentStageImage,
    agentThread,
    agentThreadDelete,
    agentThreads,
    agentsAlwaysOnTop,
    setAgentsAlwaysOnTop,
    sshListHosts,
  } from "$ipc/agents";
  import { getConfig } from "$ipc/config";
  import { onAgentsComposerInsert, readClipboardDragText } from "$ipc/clipboard";
  import { pickAgentFiles } from "$ipc/dialogs";
  import { withAgentsDismissSuppressed } from "$surfaces/overlay/agents/dismissGuard";
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
  import Icon from "$ui/Icon.svelte";
  import {
    ArrowUp,
    ChevronLeft,
    Cpu,
    Folder,
    History,
    Paperclip,
    Pin,
    Square,
    SquareTerminal,
    X,
  } from "$lib/icons";
  import FolderBrowser from "./FolderBrowser.svelte";
  import SlashPalette from "./SlashPalette.svelte";
  import SshHostsPanel from "./SshHostsPanel.svelte";
  import {
    isChatStatusNoise,
    statusToastMessage,
  } from "./chatNotifications";
  import { resolveSlashCommands, skillsAsCommands } from "./slashCatalog";
  import { config } from "$domain/config.svelte";
  import { toasts } from "$domain/toasts.svelte";
  import { tryMainUi } from "$surfaces/main/mainUi.svelte";
  import Modal from "$ui/Modal.svelte";
  import type {
    AgentItem,
    AgentModel,
    AgentOrigin,
    AgentsComposerInsert,
    AgentTurn,
    AppConfig,
    ClaudeCodeSession,
    SlashCommand,
    SshHost,
    StoredThread,
  } from "$lib/types";

  let {
    variant = "panel",
    onHeaderPointerDown,
    onClose,
  }: {
    variant?: "panel" | "float";
    /** Arrastre del float (header), mismo patrón que clipboard/snippets. */
    onHeaderPointerDown?: (event: PointerEvent) => void;
    /** Cerrar el float (botón X en el header). */
    onClose?: () => void;
  } = $props();

  /** Ventana principal: deep-link a Ajustes. En el float es null. */
  const mainUi = tryMainUi();

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
  let inputEl = $state<HTMLTextAreaElement | null>(null);
  /** Imágenes listas para `origin.files` (ruta absoluta). */
  let attaches = $state<{ path: string; name: string }[]>([]);
  /** Etiqueta `origin.via` del próximo envío con adjuntos. */
  let attachVia = $state("adjunto");
  let dropActive = $state(false);

  /** Sugerencias del empty state (rellenan el draft; no son prompts del CLI). */
  const SUGGESTIONS = [
    "Resume los cambios recientes del repo",
    "Explica este error y cómo arreglarlo",
    "Propón un plan corto para este proyecto",
  ] as const;
  let cwd = $state("");
  /** `null` = local; id de host en config = remoto. */
  let remoteHostId = $state<string | null>(null);
  let sshHosts = $state<SshHost[]>([]);
  /** Selector Local | Remoto en el header. */
  let destMenuOpen = $state(false);
  let destRoot = $state<HTMLDivElement | null>(null);
  /** Panel inline de hosts (float / sin MainUi). */
  let hostsPanelOpen = $state(false);
  let hostsPanelCfg = $state<AppConfig | null>(null);
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
  /** Modal de uso (costo / contexto / turnos). */
  let usageOpen = $state(false);
  /** Explorador interno de carpeta de trabajo (cwd). */
  let folderOpen = $state(false);
  /** Índice activo del menú `/`. */
  let slashIndex = $state(0);
  /** Pin sticky del float: Esc / clic afuera no cierran si está fijado. */
  let pinned = $state(false);
  /** Lightbox del adjunto en el composer. */
  let attachPreview = $state<string | null>(null);
  /** Panel inferior de consola (PTY local / SSH). */
  let consoleOpen = $state(false);

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
  const selectedHost = $derived(
    remoteHostId
      ? (sshHosts.find((h) => h.id === remoteHostId) ?? null)
      : null,
  );
  /** Remoto no depende del `claude` local en PATH. */
  const agentOk = $derived(!!remoteHostId || available === true);
  const agentMissing = $derived(!remoteHostId && available === false);
  const destLabel = $derived(
    selectedHost?.label || (remoteHostId ? "Remoto" : "Local"),
  );
  const folderLabel = $derived(
    cwd
      ? remoteHostId
        ? cwd
        : (cwd.split(/[\\/]/).filter(Boolean).pop() ?? cwd)
      : remoteHostId
        ? "cwd remoto"
        : "Carpeta",
  );
  /**
   * El cwd del CLI se fija al spawn. Bloquear solo mid-turno / archivo —
   * no por tener `sessionId` (ensureSession al foco dejaba el chip muerto).
   */
  const folderBlocked = $derived(
    !!archive || working || waiting || starting,
  );
  const folderChipTitle = $derived(
    archive
      ? "Sal del archivo para cambiar la carpeta"
      : working || waiting || starting
        ? "No se puede cambiar la carpeta mientras el agente trabaja"
        : cwd || "Elegir carpeta de trabajo",
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
    viewTurns.flatMap((t) => t.items.filter((i) => !isChatStatusNoise(i))),
  );

  const compacting = $derived(
    !archive &&
      working &&
      viewTurns.some((t) =>
        t.items.some(
          (i) =>
            i.kind === "notice" && i.text.startsWith("Compactando el contexto"),
        ),
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
      if (turn.status === "running") continue;
      // Divisor sobre el último ítem visible; turnos solo-status no marcan.
      const visible = turn.items.filter((i) => !isChatStatusNoise(i));
      if (visible.length === 0) continue;
      const last = visible[visible.length - 1];
      // El costo sigue en el store / UsageModal; el hilo no lo muestra.
      map.set(last.id, null);
    }
    return map;
  });

  /** Costo para el modal: sesión viva o suma de turnos en archivo. */
  const usageCostUsd = $derived.by(() => {
    if (!archive) return session?.costUsd ?? 0;
    let sum = 0;
    for (const turn of archive.turns) {
      if (turn.costUsd != null) sum += turn.costUsd;
    }
    return sum;
  });
  const usageContextTokens = $derived(
    archive ? 0 : (session?.contextTokens ?? 0),
  );
  const usageContextSize = $derived(
    archive ? null : (session?.contextSize ?? null),
  );

  const streamingLive = $derived(
    !archive &&
      conversationItems.some(
        (i) =>
          (i.kind === "message" || i.kind === "reasoning") && i.streaming,
      ),
  );

  const ctaDisabled = $derived(
    agentMissing ||
      waiting ||
      !!archive ||
      (!working && !draft.trim() && attaches.length === 0),
  );

  const IMAGE_EXT = /\.(png|jpe?g|gif|webp)$/i;

  function fileName(path: string): string {
    const parts = path.replace(/\\/g, "/").split("/");
    return parts[parts.length - 1] || path;
  }

  function isImagePath(path: string): boolean {
    return IMAGE_EXT.test(path);
  }

  function addAttachPath(path: string) {
    const trimmed = path.trim();
    if (!trimmed) return;
    if (!isImagePath(trimmed)) {
      // El puente solo embebe imágenes; el resto va como ruta en el texto.
      draft = draft.trim() ? `${draft.trimEnd()}\n${trimmed}` : trimmed;
      return;
    }
    if (attaches.some((a) => a.path === trimmed)) return;
    attaches = [...attaches, { path: trimmed, name: fileName(trimmed) }];
  }

  function removeAttach(path: string) {
    attaches = attaches.filter((a) => a.path !== path);
  }

  function applyComposerInsert(payload: AgentsComposerInsert) {
    if (archive || available === false || consoleOpen) return;
    if (payload.kind === "image" && payload.imagePath) {
      attachVia = "portapapeles";
      addAttachPath(payload.imagePath);
    } else if (payload.text) {
      const t = payload.text;
      draft = draft.trim() ? `${draft.trimEnd()}\n${t}` : t;
    }
    void tick().then(() => inputEl?.focus());
  }

  function toggleConsole() {
    const next = !consoleOpen;
    consoleOpen = next;
    if (next) {
      inputEl?.blur();
      destMenuOpen = false;
      modelMenuOpen = false;
      effortMenuOpen = false;
      modeMenuOpen = false;
    }
  }

  async function stageBlob(file: Blob, mimeHint?: string) {
    const mime = (mimeHint || file.type || "image/png").toLowerCase();
    if (!mime.startsWith("image/")) return;
    const buf = new Uint8Array(await file.arrayBuffer());
    let binary = "";
    const chunk = 0x8000;
    for (let i = 0; i < buf.length; i += chunk) {
      binary += String.fromCharCode(...buf.subarray(i, i + chunk));
    }
    const b64 = btoa(binary);
    const path = await agentStageImage(b64, mime);
    addAttachPath(path);
  }

  async function onComposerPaste(e: ClipboardEvent) {
    if (archive || available === false) return;
    const items = e.clipboardData?.items;
    if (!items?.length) return;
    const images: DataTransferItem[] = [];
    for (const item of items) {
      if (item.kind === "file" && item.type.startsWith("image/")) {
        images.push(item);
      }
    }
    if (images.length === 0) return;
    e.preventDefault();
    try {
      attachVia = "portapapeles";
      for (const item of images) {
        const file = item.getAsFile();
        if (file) await stageBlob(file, item.type);
      }
    } catch (err) {
      error = String(err);
    }
  }

  function onComposerDragOver(e: DragEvent) {
    if (archive || available === false) return;
    if (!e.dataTransfer) return;
    const types = [...e.dataTransfer.types];
    if (
      types.includes("Files") ||
      types.includes("text/uri-list") ||
      types.includes("text/plain")
    ) {
      e.preventDefault();
      dropActive = true;
      if (e.dataTransfer) e.dataTransfer.dropEffect = "copy";
    }
  }

  function onComposerDragLeave(e: DragEvent) {
    const next = e.relatedTarget as Node | null;
    if (next && (e.currentTarget as HTMLElement).contains(next)) return;
    dropActive = false;
  }

  function isAticDragTextPath(path: string): boolean {
    const name = path.split(/[/\\]/).pop() ?? "";
    return name.startsWith(".atic-drag-") && name.endsWith(".txt");
  }

  function appendDraftText(text: string) {
    const t = text.trimEnd();
    if (!t) return;
    draft = draft.trim() ? `${draft.trimEnd()}\n${t}` : t;
  }

  async function tryInsertAticDragText(path: string): Promise<boolean> {
    if (!isAticDragTextPath(path)) return false;
    const text = await readClipboardDragText(path);
    if (!text?.trim()) return false;
    appendDraftText(text);
    return true;
  }

  async function onComposerDrop(e: DragEvent) {
    dropActive = false;
    if (archive || available === false) return;
    e.preventDefault();
    e.stopPropagation();
    try {
      const dt = e.dataTransfer;
      if (!dt) return;
      attachVia = "archivo";
      const files = [...(dt.files ?? [])];
      let added = false;
      for (const file of files) {
        const path = (file as File & { path?: string }).path;
        if (path) {
          if (await tryInsertAticDragText(path)) {
            added = true;
            continue;
          }
          addAttachPath(path);
          added = true;
          continue;
        }
        if (file.type.startsWith("image/")) {
          await stageBlob(file, file.type);
          added = true;
        }
      }
      if (!added) {
        const uri = dt.getData("text/uri-list") || "";
        for (const line of uri.split(/\r?\n/)) {
          const t = line.trim();
          if (!t || t.startsWith("#")) continue;
          let local = t;
          if (t.startsWith("file:")) {
            local = decodeURIComponent(t.replace(/^file:\/\//, ""));
            // `file:///C:/…` → `/C:/…` en algunos hosts; quitar el slash.
            if (/^\/[A-Za-z]:/.test(local)) local = local.slice(1);
          }
          if (!local) continue;
          if (await tryInsertAticDragText(local)) {
            added = true;
            continue;
          }
          addAttachPath(local);
          added = true;
        }
      }
      if (!added) {
        const text = dt.getData("text/plain") || dt.getData("text");
        if (text?.trim()) appendDraftText(text);
      }
      void tick().then(() => inputEl?.focus());
    } catch (err) {
      error = String(err);
    }
  }

  async function pickAttaches() {
    if (archive || available === false || working) return;
    try {
      const paths = await withAgentsDismissSuppressed(() => pickAgentFiles());
      if (paths.length) attachVia = "adjunto";
      for (const p of paths) addAttachPath(p);
      void tick().then(() => inputEl?.focus());
    } catch (err) {
      error = String(err);
    }
  }
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

  /** Evita toast repetido al re-renderizar el mismo notice. */
  const toastedStatusIds = new Set<string>();

  $effect(() => {
    agents.watch(sessionId);
  });

  $effect(() => {
    // Archivo: no spamear toasts al abrir historial.
    if (archive) return;
    const catalog = models.map((m) => ({ id: m.id, name: m.name }));
    for (const turn of liveTurns) {
      for (const item of turn.items) {
        if (toastedStatusIds.has(item.id)) continue;
        if (!isChatStatusNoise(item)) continue;
        toastedStatusIds.add(item.id);
        const msg = statusToastMessage(item, catalog);
        if (msg) toasts.push(msg, 3500);
      }
    }
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
    // El índice `~/.claude/projects` es local; no aplica a sesiones remotas.
    if (remoteHostId || !cwd.trim()) {
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
  }

  /** Abre Ajustes → Agentes, o un panel inline si estamos en el float. */
  async function openManageHosts() {
    destMenuOpen = false;
    if (mainUi) {
      mainUi.openSettings("agents");
      return;
    }
    try {
      hostsPanelCfg = config.current ?? (await getConfig());
    } catch (e) {
      toasts.push(String(e));
      return;
    }
    hostsPanelOpen = true;
  }

  function closeHostsPanel() {
    hostsPanelOpen = false;
    void loadSshHosts();
  }

  function startOptionsBase(): {
    cwd?: string;
    remoteHostId?: string;
    model?: string;
    effort?: string;
    permissionMode: string;
  } {
    return {
      cwd: cwd || undefined,
      remoteHostId: remoteHostId || undefined,
      model: model || undefined,
      effort: effort || undefined,
      permissionMode: PERMISSION_MODES.some((m) => m.id === mode)
        ? mode
        : rememberedMode(BACKEND),
    };
  }

  async function setDestination(nextHostId: string | null) {
    if (remoteHostId === nextHostId) {
      destMenuOpen = false;
      return;
    }
    if (working || waiting || starting) {
      destMenuOpen = false;
      toasts.push("No se puede cambiar el destino mientras el agente trabaja", 3500);
      return;
    }
    const prev = sessionId;
    if (prev) {
      sessionId = null;
      try {
        await agents.stop(prev);
      } catch {
        /* ignore */
      }
      cliResumed = false;
      resumeNote = "";
    }
    remoteHostId = nextHostId;
    destMenuOpen = false;
    if (nextHostId) {
      const host = sshHosts.find((h) => h.id === nextHostId);
      if (host?.default_remote_cwd) cwd = host.default_remote_cwd;
      cliSessions = [];
    } else {
      cwd = rememberedCwd(BACKEND);
    }
    if (prev) {
      toasts.push(
        "Destino actualizado. El próximo mensaje inicia una sesión nueva.",
        4000,
      );
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

      if (remoteHostId) {
        error = "Reanudar sesiones del CLI solo está disponible en Local.";
        return;
      }
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
        if (!thread.remoteHostId) rememberCwd(BACKEND, thread.cwd);
      }
      remoteHostId = thread.remoteHostId ?? null;
      if (thread.model) {
        model = thread.model;
        rememberModel(BACKEND, thread.model);
      }
      const id = await agents.start(BACKEND, {
        resume: thread.providerSession,
        cwd: thread.cwd || undefined,
        remoteHostId: thread.remoteHostId || undefined,
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

  function openFolderBrowser() {
    if (archive) {
      toasts.push("Sal del archivo para cambiar la carpeta.", 3500);
      return;
    }
    if (working || waiting || starting) {
      toasts.push(
        "No se puede cambiar la carpeta mientras el agente trabaja",
        3500,
      );
      return;
    }
    if (remoteHostId) {
      const next = window.prompt(
        "Carpeta de trabajo remota (path POSIX)",
        cwd || selectedHost?.default_remote_cwd || "",
      );
      if (next != null) void applyFolder(next);
      return;
    }
    folderOpen = true;
  }

  async function applyFolder(chosen: string) {
    const next = chosen.trim();
    folderOpen = false;
    if (!next) return;
    if (working || waiting || starting) {
      toasts.push(
        "No se puede cambiar la carpeta mientras el agente trabaja",
        3500,
      );
      return;
    }
    const norm = (p: string) => p.replace(/\\/g, "/").replace(/\/+$/, "");
    if (norm(cwd) === norm(next)) return;

    // El proceso del CLI ya nació con el cwd anterior: hay que cerrarlo.
    // El próximo ensureSession/send arranca limpio en la carpeta nueva.
    const prev = sessionId;
    if (prev) {
      sessionId = null;
      try {
        await agents.stop(prev);
      } catch {
        /* ignore */
      }
      cliResumed = false;
      resumeNote = "";
    }
    cwd = next;
    if (!remoteHostId) rememberCwd(BACKEND, next);
    if (historyOpen) void loadCliSessions();
    if (prev) {
      toasts.push(
        "Carpeta actualizada. El próximo mensaje inicia una sesión nueva.",
        4000,
      );
    }
  }

  async function pickModel(next: string) {
    model = next;
    rememberModel(BACKEND, next);
    modelMenuOpen = false;
    const label =
      modelLabelFor(
        next,
        models.map((m) => ({ id: m.id, name: m.name })),
      ) || next;
    // Sin sesión no llega notice del CLI: avisar acá.
    if (!sessionId) toasts.push(`Modelo: ${label}`, 3500);
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
    if (!id || archive) {
      if (!archive) toasts.push(`Esfuerzo: ${effortShortLabel(level)}`, 3500);
      return;
    }
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
    if (!hadSession) {
      toasts.push(`Permisos: ${modeShortLabel(next)}`, 3500);
      return;
    }
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
    if (archive || agentMissing || starting) return sessionId;
    if (remoteHostId && !cwd.trim()) {
      error = "Indica un cwd remoto (path POSIX) antes de iniciar.";
      return null;
    }
    starting = true;
    error = null;
    try {
      const id = await agents.start(BACKEND, startOptionsBase());
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
    const pendingFiles = attaches.map((a) => a.path);
    if (
      (!text && pendingFiles.length === 0) ||
      working ||
      agentMissing ||
      archive
    ) {
      return;
    }

    // `/effort` solo → select, no mandar el usage al chat.
    if (text && pendingFiles.length === 0 && /^\/effort\s*$/i.test(text)) {
      draft = "";
      effortMenuOpen = true;
      modelMenuOpen = false;
      modeMenuOpen = false;
      void ensureSession();
      return;
    }
    if (text && pendingFiles.length === 0 && /^\/compact\s*$/i.test(text)) {
      draft = "";
      openCompact();
      void ensureSession();
      return;
    }
    if (text && pendingFiles.length === 0 && /^\/model\s*$/i.test(text)) {
      draft = "";
      modelMenuOpen = true;
      effortMenuOpen = false;
      modeMenuOpen = false;
      return;
    }
    if (text && pendingFiles.length === 0 && /^\/permissions\s*$/i.test(text)) {
      draft = "";
      modeMenuOpen = true;
      modelMenuOpen = false;
      effortMenuOpen = false;
      void ensureSession();
      return;
    }
    if (text && pendingFiles.length === 0 && /^\/plan\s*$/i.test(text)) {
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
    const pendingAttaches = attaches;
    draft = "";
    attaches = [];
    const viaLabel = attachVia;
    const origin: AgentOrigin | undefined =
      pendingFiles.length > 0
        ? {
            via: viaLabel,
            file:
              pendingAttaches.length === 1
                ? pendingAttaches[0].name
                : `${pendingAttaches.length} imágenes`,
            files: pendingFiles,
          }
        : undefined;
    try {
      const id = await ensureSession();
      if (!id) {
        draft = pending;
        attaches = pendingAttaches;
        return;
      }
      await agents.send(id, pending, origin);
      void loadThreads();
    } catch (err) {
      error = String(err);
      draft = pending;
      attaches = pendingAttaches;
    } finally {
      busy = false;
    }
  }

  /** Solo corta el turno en curso; la sesión sigue abierta. */
  async function interrupt() {
    const id = sessionId;
    if (!id) return;
    error = null;
    modelMenuOpen = false;
    busy = false;
    try {
      await agents.interrupt(id);
    } catch (err) {
      error = String(err);
    }
  }

  function applySuggestion(text: string) {
    if (archive || available === false) return;
    draft = text;
    void tick().then(() => {
      inputEl?.focus();
      const len = draft.length;
      inputEl?.setSelectionRange(len, len);
    });
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

  async function togglePin() {
    const next = !pinned;
    pinned = next;
    try {
      await setAgentsAlwaysOnTop(next);
    } catch {
      pinned = !next;
    }
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
    if (variant === "float") {
      void agentsAlwaysOnTop()
        .then((on) => {
          pinned = on;
        })
        .catch(() => {
          pinned = false;
        });
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
    void loadSshHosts();
    const onWinKey = (e: KeyboardEvent) => {
      // Consola a pantalla: no interceptar teclas del PTY (salvo Esc para cerrar).
      if (consoleOpen) {
        if (e.key === "Escape") {
          e.preventDefault();
          e.stopPropagation();
          consoleOpen = false;
        }
        return;
      }
      if (e.key !== "Escape") return;
      if (modelMenuOpen || effortMenuOpen || modeMenuOpen || destMenuOpen) {
        e.preventDefault();
        modelMenuOpen = false;
        effortMenuOpen = false;
        modeMenuOpen = false;
        destMenuOpen = false;
        return;
      }
      // No robamos Esc a slash / modales / resume; el composer los maneja.
      if (
        slashOpen ||
        usageOpen ||
        folderOpen ||
        hostsPanelOpen ||
        compactOpen ||
        resumePick ||
        attachPreview
      )
        return;
      if (historyOpen) {
        e.preventDefault();
        historyOpen = false;
      }
    };
    // Capture: Esc cierra la consola antes de que el float se cierre.
    window.addEventListener("keydown", onWinKey, true);
    let stopInsert: (() => void) | undefined;
    void onAgentsComposerInsert(applyComposerInsert).then((un) => {
      stopInsert = un;
    });
    return () => {
      window.removeEventListener("keydown", onWinKey, true);
      stopInsert?.();
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
    if (slashQuery !== null && !sessionId && !archive && agentOk) {
      void ensureSession();
    }
  });

  // Cierra el menú de destino al pulsar fuera (mismo patrón que PickerMenu).
  $effect(() => {
    if (!destMenuOpen) return;
    const onPointerDown = (event: PointerEvent) => {
      const el = destRoot;
      const target = event.target;
      if (!(target instanceof Node) || !el) return;
      if (el.contains(target)) return;
      destMenuOpen = false;
    };
    const id = window.setTimeout(() => {
      window.addEventListener("pointerdown", onPointerDown, true);
    }, 0);
    return () => {
      window.clearTimeout(id);
      window.removeEventListener("pointerdown", onPointerDown, true);
    };
  });
</script>

<div
  class="demo"
  class:is-float={variant === "float"}
  class:is-panel={variant === "panel"}
  class:is-menu-open={modelMenuOpen ||
    effortMenuOpen ||
    modeMenuOpen ||
    destMenuOpen ||
    !!resumePick ||
    compactOpen ||
    usageOpen ||
    folderOpen ||
    hostsPanelOpen ||
    slashOpen}
  style="--accent: {ACCENT}"
  data-demo="claude-code"
  data-agent="claude-code"
>
  {#snippet sessionControls()}
    <div class="dest" bind:this={destRoot}>
      <button
        type="button"
        class="chip is-dest"
        class:is-on={!!remoteHostId}
        class:is-locked={folderBlocked}
        title={remoteHostId
          ? `Remoto: ${destLabel}`
          : "Local (este equipo)"}
        aria-label={`Destino: ${destLabel}`}
        aria-expanded={destMenuOpen}
        aria-disabled={folderBlocked}
        disabled={folderBlocked}
        onclick={() => {
          if (folderBlocked) return;
          destMenuOpen = !destMenuOpen;
          if (destMenuOpen) {
            modelMenuOpen = false;
            effortMenuOpen = false;
            modeMenuOpen = false;
            void loadSshHosts();
          }
        }}
      >
        <Icon icon={Cpu} size={12} />
        <span class="chip-t">{destLabel}</span>
      </button>
      {#if destMenuOpen}
        <div class="dest-menu" role="listbox" aria-label="Destino de sesión">
          <button
            type="button"
            class="dest-opt"
            class:is-active={!remoteHostId}
            role="option"
            aria-selected={!remoteHostId}
            onclick={() => void setDestination(null)}
          >
            Local
          </button>
          {#if sshHosts.length === 0}
            <p class="dest-hint">Sin hosts. Agregalos en Ajustes → Agentes.</p>
          {:else}
            {#each sshHosts as h (h.id)}
              <button
                type="button"
                class="dest-opt"
                class:is-active={remoteHostId === h.id}
                role="option"
                aria-selected={remoteHostId === h.id}
                onclick={() => void setDestination(h.id)}
              >
                {h.label || (h.user ? `${h.user}@${h.host}` : h.host)}
              </button>
            {/each}
          {/if}
          <button
            type="button"
            class="dest-opt dest-manage"
            onclick={() => void openManageHosts()}
          >
            Gestionar hosts…
          </button>
        </div>
      {/if}
    </div>
    <button
      type="button"
      class="chip is-folder"
      class:is-on={!!cwd}
      class:is-locked={folderBlocked}
      title={remoteHostId
        ? "cwd remoto (path POSIX)"
        : folderChipTitle}
      aria-label={cwd ? `Carpeta: ${folderLabel}` : "Elegir carpeta"}
      aria-disabled={folderBlocked}
      onclick={openFolderBrowser}
    >
      <Icon icon={Folder} size={12} />
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
  {/snippet}

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <header class="top">
    <div
      class="brand"
      title={archive
        ? "Archivo · solo lectura"
        : "Claude Code · login local del CLI"}
    >
      <img
        class="brand-mark"
        src="/brands/claude.svg"
        width="18"
        height="18"
        alt="Claude"
        draggable="false"
      />
    </div>
    {#if variant === "float" && onHeaderPointerDown}
      <!-- Solo el hueco central arrastra: si el drag vive en todo el header,
           pin / historial / Bypass / X pierden el clic (sobre todo con otra
           app detrás del overlay). -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div
        class="top-drag"
        aria-hidden="true"
        onpointerdown={onHeaderPointerDown}
      ></div>
    {:else if variant === "float"}
      <div class="top-drag" aria-hidden="true"></div>
    {/if}
    <div class="top-acts" data-no-drag>
      {#if variant === "float"}
        <button
          type="button"
          class="icon-btn"
          class:is-on={pinned}
          aria-label={pinned ? "Desfijar" : "Fijar arriba"}
          aria-pressed={pinned}
          title={pinned ? "Desfijar" : "Fijar arriba"}
          onclick={() => void togglePin()}
        >
          <Icon icon={Pin} size={13} />
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
        <Icon icon={History} size={13} />
      </button>
      <button
        type="button"
        class="icon-btn"
        class:is-on={consoleOpen}
        aria-label={consoleOpen ? "Volver al chat" : "Consola"}
        title={consoleOpen ? "Volver al chat" : "Consola"}
        onclick={toggleConsole}
      >
        <Icon icon={SquareTerminal} size={13} />
      </button>
      <button
        type="button"
        class="icon-btn"
        class:is-on={usageOpen}
        aria-label="Uso"
        title="Uso · costo y contexto"
        onclick={() => (usageOpen = true)}
      >
        <Icon icon={Cpu} size={13} />
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
        <button
          type="button"
          class="badge is-ctx"
          title="Contexto usado · ver uso"
          aria-label="Contexto usado: {contextChip}"
          onclick={() => (usageOpen = true)}
        >
          {contextChip}
        </button>
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
      {#if variant === "float" && onClose}
        <button
          type="button"
          class="icon-btn"
          aria-label="Cerrar"
          title="Cerrar"
          onclick={onClose}
        >
          <Icon icon={X} size={13} />
        </button>
      {/if}
    </div>
  </header>

  <div
    class="hist-layer"
    class:is-open={historyOpen}
    inert={!historyOpen}
  >
    <button
      type="button"
      class="hist-scrim"
      tabindex={historyOpen ? 0 : -1}
      aria-label="Cerrar historial"
      data-no-drag
      onclick={() => (historyOpen = false)}
    ></button>
    <aside
      class="hist"
      aria-label="Conversaciones guardadas"
      aria-hidden={!historyOpen}
    >
      <div class="hist-h">
        <span>Historial</span>
        <button
          type="button"
          class="icon-btn"
          data-no-drag
          aria-label="Cerrar historial"
          title="Cerrar"
          tabindex={historyOpen ? 0 : -1}
          onclick={() => (historyOpen = false)}
        >
          <Icon icon={X} size={12} />
        </button>
      </div>

      <div class="hist-body">
        {#if remoteHostId}
          <p class="hist-sec">CLI remoto</p>
          <div class="hist-empty-wrap">
            <EmptyState
              title="No disponible en remoto"
              hint="El índice de sesiones del CLI es local. Usa el historial Atic."
            />
          </div>
        {:else}
          <p class="hist-sec" title="Igual que /resume en Claude Code">
            CLI · esta carpeta
          </p>
          {#if !cwd.trim()}
            <div class="hist-empty-wrap">
              <EmptyState
                title="Elige una carpeta"
                hint="Para ver sesiones del CLI."
              >
                {#snippet action()}
                  <button
                    type="button"
                    class="chip"
                    class:is-locked={folderBlocked}
                    data-no-drag
                    aria-disabled={folderBlocked}
                    title={folderChipTitle}
                    onclick={openFolderBrowser}
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
              <EmptyState title="Sin sesiones CLI" hint="Prueba otra carpeta." />
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
        {/if}

        <p class="hist-sec">Atic</p>
        {#if threadsLoading}
          <p class="hist-empty">Cargando…</p>
        {:else if threads.length === 0}
          <div class="hist-empty-wrap">
            <EmptyState
              title="Sin historial"
              hint="Aparece al cerrar un turno."
            />
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
                    {#if t.remoteHostId}
                      <span class="hist-tag">SSH</span>
                    {/if}
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
      </div>
    </aside>
  </div>

  {#if consoleOpen}
    {#await import("./ConsolePanel.svelte") then { default: ConsolePanel }}
      <ConsolePanel
        remoteHost={selectedHost}
        localCwd={remoteHostId ? "" : cwd}
        initialKind={remoteHostId ? "ssh" : "local"}
        onClose={() => (consoleOpen = false)}
      />
    {/await}
  {:else}
    <div
      class="log"
      bind:this={logEl}
      role="log"
      aria-label="Conversación"
      data-selectable
    >
      {#if agentMissing}
        <div class="empty">
          <EmptyState
            title="Sin CLI"
            hint="Instala Claude Code y ejecuta claude auth login. O elige un host SSH remoto."
          />
        </div>
      {:else if archiveLoading}
        <div class="empty">
          <EmptyState title="Abriendo…" />
        </div>
      {:else if conversationItems.length === 0 && !working}
        <div class="empty" class:is-hero={!archive}>
          {#if archive}
            <EmptyState title="Sin mensajes" hint="Este archivo está vacío." />
          {:else}
            <div class="hero" data-no-drag>
              <p class="hero-t">
                {cwd.trim()
                  ? "Pregunta lo que necesites"
                  : "Elige carpeta y empieza"}
              </p>
              <p class="hero-h">
                {cwd.trim()
                  ? "Claude Code · sesión local en Atic"
                  : "Primero la carpeta de trabajo, abajo."}
              </p>
              <div class="hero-sugs" role="group" aria-label="Sugerencias">
                {#each SUGGESTIONS as sug, i (sug)}
                  <button
                    type="button"
                    class="sug"
                    style="--i: {i}"
                    disabled={available == null}
                    onclick={() => applySuggestion(sug)}
                  >
                    {sug}
                  </button>
                {/each}
              </div>
            </div>
          {/if}
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
  {/if}

  {#if usageOpen}
    {#await import("./UsageModal.svelte") then { default: UsageModal }}
      <UsageModal
        costUsd={usageCostUsd}
        contextTokens={usageContextTokens}
        contextSize={usageContextSize}
        model={model || session?.model || archive?.model || ""}
        effort={effort || session?.effort || null}
        mode={mode || session?.mode || null}
        turns={viewTurns}
        archive={!!archive}
        onClose={() => (usageOpen = false)}
      />
    {/await}
  {/if}

  {#if attachPreview}
    <Modal
      title="Adjunto"
      size="lg"
      contained
      panelMax="min(90dvh, 880px)"
      onClose={() => (attachPreview = null)}
    >
      <div class="attach-lightbox">
        <img
          src={convertFileSrc(attachPreview)}
          alt="Vista ampliada del adjunto"
          class="attach-lightbox-img"
        />
      </div>
    </Modal>
  {/if}

  {#if folderOpen}
    <FolderBrowser
      initialPath={cwd}
      onPick={applyFolder}
      onClose={() => (folderOpen = false)}
    />
  {/if}

  {#if hostsPanelOpen && hostsPanelCfg}
    <Modal
      title="Hosts SSH"
      size="md"
      contained
      panelMax="min(90dvh, 640px)"
      onClose={closeHostsPanel}
    >
      <SshHostsPanel
        bind:config={hostsPanelCfg}
        onToast={(msg) => toasts.push(msg)}
      />
    </Modal>
  {/if}

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

  {#if cliResumed && !archive && !consoleOpen}
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
        <Icon icon={X} size={11} />
      </button>
    </p>
  {/if}

  {#if archive && !consoleOpen}
    <div class="archive-bar" data-no-drag>
      <button
        type="button"
        class="icon-btn"
        aria-label="Volver al chat"
        title="Volver"
        onclick={leaveArchive}
      >
        <Icon icon={ChevronLeft} size={13} />
      </button>
      <button
        type="button"
        class="chip is-go"
        disabled={!archive.providerSession || (!archive.remoteHostId && available === false)}
        onclick={() => void resumeArchive()}
      >
        Continuar
      </button>
    </div>
  {/if}

  {#if !consoleOpen && (error || session?.error)}
    <p class="warn" role="alert">{error ?? session?.error}</p>
  {/if}

  {#if !consoleOpen}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <footer
      class="composer"
      class:is-drop={dropActive}
      data-no-drag
      ondragover={onComposerDragOver}
      ondragleave={onComposerDragLeave}
      ondrop={(e) => void onComposerDrop(e)}
    >
      <div
        class="composer-card"
        class:is-readonly={!!archive}
        class:is-drop={dropActive}
        class:has-perm={!archive && (session?.pending?.length ?? 0) > 0}
      >
        {#if !archive && (session?.pending?.length ?? 0) > 0}
          <div class="perm-stack" data-no-drag>
            {#each session?.pending ?? [] as p (p.id)}
              <div class="perm" role="alertdialog" aria-label="Permiso pendiente">
                <p class="perm-t" title={p.description ?? p.tool}>
                  <strong>{p.tool}</strong>
                  {#if p.description}
                    <span class="perm-w"> · {p.description}</span>
                  {/if}
                </p>
                <div class="perm-acts">
                  <button
                    type="button"
                    class="perm-btn is-danger"
                    onclick={() => void agents.decide(sessionId!, p.id, "deny")}
                  >
                    rechazar
                  </button>
                  <button
                    type="button"
                    class="perm-btn"
                    onclick={() => void agents.decide(sessionId!, p.id, "allow")}
                  >
                    aprobar
                  </button>
                  <button
                    type="button"
                    class="perm-btn is-go"
                    onclick={() => void agents.decide(sessionId!, p.id, "allowAlways")}
                  >
                    aprobar siempre
                  </button>
                </div>
              </div>
            {/each}
          </div>
        {/if}
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
        {#if attaches.length > 0}
          <div class="attach-row" data-no-drag>
            {#each attaches as a (a.path)}
              <div class="attach-chip">
                <button
                  type="button"
                  class="attach-thumb-btn"
                  aria-label="Ampliar {a.name}"
                  title="Ampliar"
                  onclick={() => (attachPreview = a.path)}
                >
                  <img
                    class="attach-thumb"
                    src={convertFileSrc(a.path)}
                    alt=""
                    draggable="false"
                  />
                </button>
                <button
                  type="button"
                  class="attach-x"
                  aria-label="Quitar {a.name}"
                  title="Quitar"
                  disabled={!!archive || working}
                  onclick={() => {
                    if (attachPreview === a.path) attachPreview = null;
                    removeAttach(a.path);
                  }}
                >
                  <Icon icon={X} size={11} />
                </button>
              </div>
            {/each}
          </div>
        {/if}
        <textarea
          class="in"
          rows="1"
          bind:this={inputEl}
          placeholder={agentMissing
            ? "Sin CLI…"
            : archive
              ? "Solo lectura…"
              : dropActive
                ? "Suelta para adjuntar…"
                : remoteHostId
                  ? "Mensaje al agente remoto…"
                  : "Mensaje, /… o pega una imagen"}
          bind:value={draft}
          onkeydown={onKey}
          onpaste={(e) => void onComposerPaste(e)}
          oninput={() => (slashIndex = 0)}
          onfocus={() => {
            if (!archive && agentOk) void ensureSession();
          }}
          disabled={agentMissing || !!archive}
          aria-label="Mensaje"
        ></textarea>
        <div class="row">
          <div class="set" data-no-drag>
            {@render sessionControls()}
          </div>
          <div class="acts" data-no-drag>
            <button
              type="button"
              class="icon-btn attach-btn"
              aria-label="Adjuntar archivos"
              title="Adjuntar archivos"
              disabled={agentMissing || !!archive || working}
              onclick={() => void pickAttaches()}
            >
              <Icon icon={Paperclip} size={14} />
            </button>
            <button
              type="button"
              class="send"
              class:is-stop={working && !waiting}
              disabled={ctaDisabled}
              aria-label={ctaLabel}
              title={ctaLabel}
              onclick={() => (working && !waiting ? void interrupt() : void send())}
            >
              <Icon
                icon={working && !waiting ? Square : ArrowUp}
                size={14}
              />
            </button>
          </div>
        </div>
      </div>
    </footer>
  {/if}
</div>

<style>
  .demo {
    --pad: 0.55rem;
    --r-outer: 26px;
    --r-card: 15px;
    --r-in: 10px;
    --r-chip: 999px;
    /* Controles del header: misma altura óptica (pin, historial, Bypass, ready). */
    --top-ctrl: 1.75rem;
    --top-ctrl-fs: 0.625rem;
    --top-ctrl-r: 0.4rem;
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

  .demo.is-menu-open {
    overflow: visible;
  }

  .top {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    justify-content: space-between;
    gap: 0.4rem;
    border-bottom: 1px solid transparent;
    padding: 0.32rem 0.65rem 0.28rem;
  }

  .top-drag {
    flex: 1;
    min-width: 0.75rem;
    align-self: stretch;
    /* Zona de arrastre dedicada (solo float). */
    cursor: grab;
    touch-action: none;
    user-select: none;
  }

  .top-drag:active {
    cursor: grabbing;
  }

  .brand {
    display: grid;
    place-items: center;
    flex-shrink: 0;
  }

  .brand-mark {
    display: block;
    width: 1.1rem;
    height: 1.1rem;
    object-fit: contain;
    pointer-events: none;
    user-select: none;
  }

  .top-acts {
    position: relative;
    /* Encima de grips de AgentsFloat (z 7): pin/X no deben quedar bajo el resize. */
    z-index: 9;
    display: flex;
    flex-shrink: 0;
    align-items: center;
    gap: 0.2rem;
    min-height: var(--top-ctrl);
  }

  .icon-btn {
    display: grid;
    place-items: center;
    box-sizing: border-box;
    width: var(--top-ctrl);
    height: var(--top-ctrl);
    border: 1px solid transparent;
    border-radius: var(--top-ctrl-r);
    padding: 0;
    background: transparent;
    color: var(--rb-faint);
    cursor: pointer;
    box-shadow: none;
    filter: none;
    transition:
      color var(--duration-quick) var(--ease-smooth-out),
      background var(--duration-quick) var(--ease-smooth-out),
      border-color var(--duration-quick) var(--ease-smooth-out),
      transform var(--duration-quick) var(--ease-smooth-out);
  }

  /* El SVG (fill none) solo hit-testeaba el trazo: zona mínima. */
  .icon-btn :global(svg) {
    pointer-events: none;
  }

  .icon-btn:hover,
  .icon-btn.is-on {
    color: var(--rb-text);
    background: color-mix(in srgb, var(--rb-text) 8%, transparent);
    box-shadow: none;
    filter: none;
  }

  .icon-btn:active {
    transform: scale(0.96);
  }

  .badge {
    margin: 0;
    flex-shrink: 0;
    box-sizing: border-box;
    height: var(--top-ctrl);
    min-height: var(--top-ctrl);
    display: inline-flex;
    align-items: center;
    gap: 0.26rem;
    border: 0;
    border-radius: var(--r-chip);
    padding: 0 0.45rem 0 0.38rem;
    font: inherit;
    font-size: var(--top-ctrl-fs);
    font-weight: 500;
    line-height: 1;
    letter-spacing: 0.01em;
    font-variant-numeric: tabular-nums;
    color: var(--rb-muted);
    background: color-mix(in srgb, var(--rb-text) 5%, transparent);
    transition:
      color var(--duration-quick) var(--ease-smooth-out),
      background var(--duration-quick) var(--ease-smooth-out);
  }

  button.badge {
    cursor: pointer;
  }

  button.badge:hover {
    background: color-mix(in srgb, var(--rb-text) 8%, transparent);
  }

  button.badge.is-live:hover {
    background: color-mix(in srgb, var(--rb-ok) 20%, transparent);
  }

  button.badge:active {
    transform: scale(0.96);
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
    display: flex;
    align-items: center;
    height: var(--top-ctrl);
  }

  .mode-pick :global(.pm-chip) {
    box-sizing: border-box;
    height: var(--top-ctrl);
    min-height: var(--top-ctrl);
    max-width: 5.5rem;
    border-color: transparent;
    background: color-mix(in srgb, var(--rb-text) 5%, transparent);
    color: var(--rb-muted);
    font-size: var(--top-ctrl-fs);
    font-weight: 500;
    line-height: 1;
    letter-spacing: 0.01em;
    padding: 0 0.45rem;
  }

  .mode-pick :global(.pm-chip:hover),
  .mode-pick :global(.pm-chip.is-open) {
    color: var(--rb-text);
    background: color-mix(in srgb, var(--rb-text) 8%, transparent);
  }

  .hist-layer {
    position: absolute;
    inset: 0;
    z-index: 18;
    overflow: hidden;
    pointer-events: none;
  }

  .hist-layer.is-open {
    pointer-events: auto;
  }

  .hist-scrim {
    position: absolute;
    inset: 0;
    margin: 0;
    padding: 0;
    border: none;
    cursor: default;
    background: color-mix(in srgb, var(--rb-text) 16%, transparent);
    opacity: 0;
    transition: opacity 240ms ease-out;
  }

  .hist-layer.is-open .hist-scrim {
    opacity: 1;
  }

  .hist {
    position: absolute;
    top: 0;
    left: 0;
    bottom: 0;
    z-index: 1;
    display: flex;
    flex-direction: column;
    width: min(300px, 85%);
    max-width: 100%;
    box-sizing: border-box;
    background: color-mix(in srgb, var(--rb-surface) 96%, var(--rb-bg0));
    /* Sin sombra blanda: con translateX(-100%) el glow se filtraba al panel. */
    box-shadow: none;
    transform: translateX(-100%);
    transition: transform 240ms ease-out;
  }

  .hist-layer.is-open .hist {
    transform: translateX(0);
    /* Solo separación dura respecto al scrim; sin halo. */
    box-shadow: 1px 0 0 color-mix(in srgb, var(--rb-text) 8%, transparent);
  }

  .hist-h {
    display: flex;
    flex-shrink: 0;
    align-items: center;
    justify-content: space-between;
    gap: 0.35rem;
    padding: 0.55rem 0.55rem 0.35rem;
    font-size: 0.62rem;
    font-weight: 600;
    letter-spacing: 0.03em;
    text-transform: uppercase;
    color: var(--rb-faint);
  }

  .hist-body {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 0 0.55rem 0.55rem;
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
    padding: 0.45rem 0.75rem 0.35rem;
    scrollbar-width: thin;
    /* El overlay pone user-select:none / touch-action:none; acá se copia. */
    user-select: text;
    -webkit-user-select: text;
    touch-action: auto;
    cursor: text;
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
    align-items: center;
    justify-content: center;
    align-self: center;
    width: 100%;
    max-width: 26rem;
    padding: 0.4rem 0.25rem 0.6rem;
  }

  .empty.is-hero {
    max-width: 28rem;
  }

  .empty :global(.flex) {
    align-items: center;
    text-align: center;
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

  .hero {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.35rem;
    width: 100%;
    text-align: center;
    animation: hero-in 0.32s var(--ease-smooth-out) both;
  }

  .hero-t {
    margin: 0;
    font-size: 0.92rem;
    font-weight: 600;
    letter-spacing: -0.01em;
    color: var(--rb-text);
    text-wrap: balance;
  }

  .hero-h {
    margin: 0;
    max-width: 22rem;
    font-size: 0.72rem;
    line-height: 1.4;
    color: var(--rb-muted);
    text-wrap: pretty;
  }

  .hero-sugs {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 0.35rem;
    margin-top: 0.55rem;
  }

  .sug {
    border: 1px solid color-mix(in srgb, var(--rb-text) 10%, transparent);
    border-radius: var(--r-chip);
    padding: 0.28rem 0.65rem;
    background: color-mix(in srgb, var(--rb-text) 3.5%, transparent);
    color: var(--rb-muted);
    font: inherit;
    font-size: 0.68rem;
    line-height: 1.3;
    cursor: pointer;
    animation: sug-in 0.34s var(--ease-smooth-out) both;
    animation-delay: calc(0.06s + var(--i, 0) * 0.05s);
    transition:
      color var(--duration-quick) var(--ease-smooth-out),
      background var(--duration-quick) var(--ease-smooth-out),
      border-color var(--duration-quick) var(--ease-smooth-out),
      transform var(--duration-quick) var(--ease-smooth-out);
  }

  .sug:hover:not(:disabled) {
    color: var(--rb-text);
    border-color: color-mix(in srgb, var(--accent) 35%, var(--rb-border));
    background: color-mix(in srgb, var(--rb-text) 7%, transparent);
  }

  .sug:active:not(:disabled) {
    transform: scale(0.96);
  }

  .sug:disabled {
    opacity: 0.45;
    cursor: not-allowed;
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

  /* Permiso: una fila densa dentro del composer, no una tarjeta aparte. */
  .perm-stack {
    display: flex;
    flex-direction: column;
    gap: 0.2rem;
    margin: 0 0 0.1rem;
  }

  .perm {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.25rem 0.35rem;
    min-width: 0;
    border-radius: calc(var(--r-in) - 2px);
    padding: 0.18rem 0.22rem 0.18rem 0.4rem;
    background: color-mix(in srgb, var(--rb-text) 4.5%, transparent);
    animation: perm-in var(--duration-fast) var(--ease-smooth-out) both;
  }

  .perm-t {
    margin: 0;
    min-width: 0;
    flex: 1 1 auto;
    font-size: 0.625rem;
    font-weight: 600;
    line-height: 1.25;
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
    flex-wrap: nowrap;
    align-items: center;
    gap: 0.15rem;
  }

  .perm-btn {
    position: relative;
    display: inline-flex;
    min-height: 1.4rem;
    align-items: center;
    border: 0;
    border-radius: 999px;
    padding: 0 0.42rem;
    background: color-mix(in srgb, var(--rb-text) 7%, transparent);
    color: var(--rb-text);
    font-size: 0.55rem;
    font-weight: 650;
    letter-spacing: 0.01em;
    cursor: pointer;
    transition:
      transform var(--duration-quick) var(--ease-smooth-out),
      background var(--duration-quick) var(--ease-smooth-out),
      color var(--duration-quick) var(--ease-smooth-out);
  }

  /* Hit ≥40px en alto sin agrandar el chip visual. */
  .perm-btn::after {
    content: "";
    position: absolute;
    inset-block: 50%;
    inset-inline: 0;
    height: 40px;
    transform: translateY(-50%);
  }

  .perm-btn:hover {
    background: color-mix(in srgb, var(--rb-text) 12%, transparent);
  }

  .perm-btn:active {
    transform: scale(0.96);
  }

  .perm-btn:focus-visible {
    outline: none;
    box-shadow: inset 0 0 0 1.5px var(--accent);
  }

  .perm-btn.is-danger {
    background: transparent;
    color: var(--rb-record);
  }

  .perm-btn.is-danger:hover {
    background: color-mix(in srgb, var(--rb-record) 12%, transparent);
  }

  .perm-btn.is-go {
    background: color-mix(in srgb, var(--accent) 16%, transparent);
    color: var(--accent);
  }

  .perm-btn.is-go:hover {
    background: color-mix(in srgb, var(--accent) 24%, transparent);
  }

  @keyframes perm-in {
    from {
      opacity: 0;
      transform: translateY(var(--distance-micro, 4px));
      filter: blur(var(--blur-small, 2px));
    }

    to {
      opacity: 1;
      transform: translateY(0);
      filter: blur(0);
    }
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
    /* Encima de grips de AgentsFloat (z 7): Local / carpeta / modelo. */
    z-index: 8;
    flex-shrink: 0;
    padding: 0.35rem 0.7rem 0.7rem;
    background: transparent;
  }

  .composer-card {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 0.35rem;
    border: 1px solid color-mix(in srgb, var(--rb-text) 9%, transparent);
    border-radius: var(--r-card);
    padding: 0.55rem 0.6rem 0.45rem;
    background: color-mix(in srgb, var(--rb-surface-2) 88%, var(--rb-bg0));
    box-shadow:
      0 1px 0 color-mix(in srgb, var(--rb-text) 4%, transparent) inset,
      0 10px 28px color-mix(in srgb, var(--rb-text) 8%, transparent);
    transition:
      border-color var(--duration-quick) var(--ease-smooth-out),
      box-shadow var(--duration-quick) var(--ease-smooth-out);
  }

  .composer-card.has-perm {
    gap: 0.3rem;
    padding-top: 0.4rem;
  }

  .composer-card:focus-within {
    border-color: color-mix(in srgb, var(--accent) 42%, var(--rb-border));
    box-shadow:
      0 1px 0 color-mix(in srgb, var(--rb-text) 4%, transparent) inset,
      0 10px 28px color-mix(in srgb, var(--rb-text) 8%, transparent),
      var(--rb-focus);
  }

  .composer-card.is-readonly {
    opacity: 0.72;
  }

  .composer-card.is-drop {
    border-color: color-mix(in srgb, var(--accent) 55%, var(--rb-border));
    background: color-mix(in srgb, var(--accent) 8%, var(--rb-surface-2));
  }

  .attach-row {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
    padding: 0 0.1rem 0.15rem;
  }

  .attach-chip {
    position: relative;
    display: inline-flex;
    flex-shrink: 0;
    border: 1px solid var(--rb-border);
    border-radius: 8px;
    padding: 0.15rem;
    background: color-mix(in srgb, var(--rb-text) 4%, transparent);
  }

  .attach-thumb-btn {
    display: block;
    margin: 0;
    padding: 0;
    border: 0;
    border-radius: 5px;
    background: transparent;
    cursor: zoom-in;
    line-height: 0;
  }

  .attach-thumb-btn:hover .attach-thumb {
    outline-color: color-mix(in srgb, var(--accent) 55%, transparent);
  }

  .attach-thumb {
    display: block;
    width: 2.25rem;
    height: 2.25rem;
    border-radius: 5px;
    object-fit: cover;
    outline: 1px solid rgba(255, 255, 255, 0.1);
  }

  .attach-x {
    position: absolute;
    top: -0.3rem;
    right: -0.3rem;
    display: grid;
    place-items: center;
    width: 1rem;
    height: 1rem;
    border: 1px solid var(--rb-border);
    border-radius: 999px;
    padding: 0;
    background: var(--rb-surface);
    color: var(--rb-faint);
    cursor: pointer;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.25);
  }

  .attach-x:hover:not(:disabled) {
    color: var(--rb-text);
    background: var(--rb-surface-2);
  }

  .attach-x:disabled {
    opacity: 0.35;
    cursor: default;
  }

  .attach-lightbox {
    display: flex;
    max-height: min(70dvh, 720px);
    align-items: center;
    justify-content: center;
    overflow: auto;
    border-radius: 6px;
    background: var(--rb-surface-2);
    padding: 0.5rem;
  }

  .attach-lightbox-img {
    max-width: 100%;
    max-height: min(66dvh, 680px);
    object-fit: contain;
  }

  .attach-btn:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .in {
    width: 100%;
    min-height: 2.4rem;
    max-height: 7.5rem;
    resize: none;
    border: 0;
    border-radius: 0;
    padding: 0.15rem 0.2rem 0.1rem;
    background: transparent;
    color: var(--rb-text);
    font: inherit;
    font-size: 0.82rem;
    line-height: 1.4;
    outline: none;
    field-sizing: content;
  }

  .in:disabled {
    opacity: 0.55;
  }

  .in::placeholder {
    color: color-mix(in srgb, var(--rb-text) 40%, transparent);
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.45rem;
    min-width: 0;
  }

  .set,
  .acts {
    display: flex;
    min-width: 0;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.22rem;
  }

  .acts {
    flex-shrink: 0;
    flex-wrap: nowrap;
  }

  .chip {
    display: inline-flex;
    min-height: 1.55rem;
    align-items: center;
    gap: 0.24rem;
    border: 1px solid color-mix(in srgb, var(--rb-text) 8%, transparent);
    border-radius: var(--r-chip);
    padding: 0.1rem 0.45rem;
    background: color-mix(in srgb, var(--rb-text) 3%, transparent);
    color: var(--rb-muted);
    font: inherit;
    font-size: 0.62rem;
    max-width: 7rem;
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

  .chip :global(svg) {
    pointer-events: none;
    flex-shrink: 0;
  }

  .chip-t {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .chip.is-folder {
    max-width: 6rem;
    padding-left: 0.35rem;
  }

  .chip.is-dest {
    max-width: 7.5rem;
  }

  .dest {
    position: relative;
  }

  .dest-menu {
    position: absolute;
    z-index: 40;
    bottom: calc(100% + 0.35rem);
    left: 0;
    min-width: 10rem;
    max-width: 16rem;
    padding: 0.35rem;
    border: 1px solid var(--rb-border);
    border-radius: 10px;
    background: var(--rb-surface-2, var(--rb-surface));
    box-shadow: 0 10px 28px color-mix(in srgb, var(--rb-text) 12%, transparent);
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
  }

  .dest-opt {
    border: 0;
    border-radius: 7px;
    padding: 0.4rem 0.55rem;
    background: transparent;
    color: var(--rb-text);
    font: inherit;
    font-size: 0.75rem;
    text-align: left;
    cursor: pointer;
  }

  .dest-opt:hover,
  .dest-opt.is-active {
    background: color-mix(in srgb, var(--accent) 14%, transparent);
  }

  .dest-hint {
    margin: 0;
    padding: 0.35rem 0.45rem;
    font-size: 0.68rem;
    color: var(--rb-muted);
  }

  .dest-manage {
    margin-top: 0.2rem;
    border-top: 1px solid var(--rb-border);
    border-radius: 0 0 7px 7px;
    color: var(--rb-muted);
    font-size: 0.7rem;
  }

  .chip:hover:not(:disabled) {
    color: var(--rb-text);
    background: color-mix(in srgb, var(--rb-text) 7%, transparent);
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
    border-color: color-mix(in srgb, var(--accent) 38%, transparent);
  }

  .chip.is-go {
    border-color: transparent;
    background: var(--accent);
    color: #fff;
    max-width: none;
    min-height: 1.55rem;
    padding: 0.1rem 0.6rem;
  }

  .model {
    position: relative;
    z-index: 7;
    max-width: 7.5rem;
  }

  .model :global(.pm-chip) {
    min-height: 1.55rem;
    max-width: 7.5rem;
    border-color: color-mix(in srgb, var(--rb-text) 8%, transparent);
    background: color-mix(in srgb, var(--rb-text) 3%, transparent);
    color: var(--rb-muted);
    font-size: 0.62rem;
    padding: 0.08rem 0.4rem;
    transition:
      color var(--duration-quick) var(--ease-smooth-out),
      background var(--duration-quick) var(--ease-smooth-out),
      border-color var(--duration-quick) var(--ease-smooth-out),
      transform var(--duration-quick) var(--ease-smooth-out);
  }

  .model :global(.pm-chip:hover),
  .model :global(.pm-chip.is-open) {
    color: var(--rb-text);
    background: color-mix(in srgb, var(--rb-text) 7%, transparent);
  }

  .model :global(.pm-chip:active) {
    transform: scale(0.96);
  }

  .send {
    display: inline-flex;
    width: 2rem;
    height: 2rem;
    min-height: 2rem;
    min-width: 2rem;
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
    filter: brightness(1.06);
  }

  .send:active:not(:disabled) {
    transform: scale(0.96);
  }

  .send:disabled {
    opacity: 0.38;
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

  @keyframes hero-in {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  @keyframes sug-in {
    from {
      opacity: 0;
      transform: translateY(5px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .live-dot,
    .hero,
    .sug,
    .perm {
      animation: none;
    }
    .hist-scrim,
    .hist,
    .perm-btn {
      transition: none;
    }
    .chip:active:not(:disabled),
    .sug:active:not(:disabled),
    .send:active:not(:disabled),
    .icon-btn:active,
    .hist-row:active,
    .hist-del:active,
    .perm-btn:active,
    .model :global(.pm-chip:active) {
      transform: none;
    }
  }
</style>
