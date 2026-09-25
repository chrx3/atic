<script lang="ts">
  /**
   * Una ficha de chat del rail de consolas: la sesión estructurada de un
   * agente dibujada en lugar del xterm.
   *
   * Sin mueble propio (PLAN_CONSOLAS §2): rail, split, zoom y atajos son de
   * `ConsolePanel`. Acá va lo de adentro del pane: una columna legible con el
   * hilo, el permiso pendiente donde el agente se detuvo, y el composer con un
   * solo selector de agente y modelo.
   *
   * `readOnly` es la sesión que abrió otro agente por MCP: se mira y no se le
   * escribe. Escribirle a un hijo abre preguntas que no se responden a medias
   * —quién queda como interlocutor, qué pasa si el padre espera ese mismo
   * turno—, así que ahí no hay composer ni permiso.
   */
  import { onMount, tick } from "svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import AgentConversation from "$lib/AgentConversation.svelte";
  import {
    PERMISSION_MODES,
    effortShortLabel,
    modeShortLabel,
    rememberEffort,
    rememberMode,
    rememberModel,
  } from "$lib/agentModels";
  import { agents, nombrePadre, sessionAnswering } from "$lib/agentSessions.svelte";
  import type { PermissionDecision } from "$core/types";
  import type { AgentModel, AgentOrigin, SlashCommand, StoredThread } from "$lib/types";
  import { t } from "$domain/i18n.svelte";
  import { formatListWhen } from "$core/format";
  import Icon from "$ui/Icon.svelte";
  import { ArrowUp, Paperclip, Square, X } from "$lib/icons";
  import { agentListModels, agentStageImage, agentThreads } from "$ipc/agents";
  import { readClipboardDragText } from "$ipc/clipboard";
  import { pickAgentFiles } from "$ipc/dialogs";
  import { withAgentsDismissSuppressed } from "$surfaces/overlay/agents/dismissGuard";
  import AgentLogo from "./AgentLogo.svelte";
  import AgentModelPicker from "./AgentModelPicker.svelte";
  import ChatPermission from "./ChatPermission.svelte";
  import ChatQuestion from "./ChatQuestion.svelte";
  import ChatPlan from "./ChatPlan.svelte";
  import {
    QUESTION_TOOL,
    answersAsMessage,
    openToolQuestion,
    parseQuestions,
    withAnswers,
  } from "./chatQuestions";
  import SlashPalette from "./SlashPalette.svelte";
  import type { AgentDef } from "./agentCatalog";
  import type { ChatInsert } from "./chatInsert";
  import { matchModel, toBlocks } from "./chatThread";
  import { isWorking } from "./chatStatus";
  import { RESUMABLE, canResume } from "./chatResume";
  import ChatHistory from "./ChatHistory.svelte";
  import ChatMcp from "./ChatMcp.svelte";
  import ChatSelect from "./ChatSelect.svelte";
  import EffortSlider from "./EffortSlider.svelte";
  import { resolveSlashCommands, skillsAsCommands } from "./slashCatalog";

  let {
    sessionId,
    readOnly = false,
    decides = false,
    choices = [],
    onOpenThread,
    onSwitchAgent,
    onRegister,
  }: {
    sessionId: string;
    readOnly?: boolean;
    /**
     * Contestar sus permisos aunque sea de solo lectura. En la pizarra un
     * sub-agente no tiene otro lugar donde pedirlos: sin esto se quedaba
     * esperando hasta que el hub cortaba la espera.
     */
    decides?: boolean;
    /** Agentes instalados que se pueden elegir en el selector. */
    choices?: AgentDef[];
    /** Una conversación guardada: el panel de consolas decide dónde se abre. */
    onOpenThread?: (thread: StoredThread) => void;
    /** Otro agente: el panel de consolas decide si reemplaza o abre ficha. */
    onSwitchAgent?: (agent: AgentDef, modelId?: string) => void;
    /**
     * Por dónde el panel de consolas le pasa lo que llega del historial del
     * portapapeles. `null` al desmontar.
     */
    onRegister?: (api: ChatInsert | null) => void;
  } = $props();

  const session = $derived(agents.byId(sessionId));
  // Arrancar no es trabajar: sin esto, el primer mensaje quedaba bloqueado
  // detrás del turno de los hooks de arranque.
  const working = $derived(!!session && isWorking(session));
  /**
   * Los permisos pendientes se pasan de a uno: con varios, la tarjeta pagina
   * («1 / 3») en vez de esconder los que vienen detrás del primero.
   */
  let permissionIndex = $state(0);
  const pendingCount = $derived(
    readOnly && !decides ? 0 : (session?.pending.length ?? 0),
  );
  const permission = $derived(
    pendingCount > 0
      ? (session?.pending[Math.min(permissionIndex, pendingCount - 1)] ?? null)
      : null,
  );
  /** Si el pendiente es una pregunta del agente, sus preguntas. */
  const questions = $derived(
    permission?.tool === QUESTION_TOOL ? parseQuestions(permission.input) : null,
  );
  /**
   * Una pregunta que el agente no pudo hacer (OpenCode por ACP) y sigue sin
   * respuesta. Saltarla la esconde solo acá: el agente ya siguió sin ella.
   */
  let skippedToolQuestion = $state<string | null>(null);
  /** El plan que el agente pide aprobar: `ExitPlanMode` de Claude y el de Cursor. */
  const PLAN_TOOL = "ExitPlanMode";
  const toolQuestion = $derived.by(() => {
    if (readOnly || working || permission) return null;
    const open = openToolQuestion(session?.turns ?? []);
    return open && open.id !== skippedToolQuestion ? open : null;
  });

  async function answerToolQuestion(picked: string[][], written: string[]) {
    if (!toolQuestion) return;
    const text = answersAsMessage(toolQuestion.questions, picked, written);
    if (!text) return;
    error = null;
    pinnedToEnd = true;
    try {
      await agents.send(sessionId, text);
    } catch (err) {
      error = String(err);
    }
  }
  const requestedBy = $derived(nombrePadre(session?.parent));
  const blocks = $derived(toBlocks(session?.turns ?? []));
  const agentDef = $derived(choices.find((a) => a.backend === session?.backendId));

  let draft = $state("");
  let sending = $state(false);
  let deciding = $state(false);
  let error = $state<string | null>(null);
  let scroller = $state<HTMLElement | null>(null);
  let input = $state<HTMLTextAreaElement | null>(null);
  /** Solo se sigue el final si el usuario ya estaba ahí: no le robamos la lectura. */
  let pinnedToEnd = true;
  let menu = $state<"agent" | "effort" | "mode" | "history" | "mcp" | null>(null);

  // ── Modelo, esfuerzo, modo ─────────────────────────────────────────────

  /**
   * Los modelos que informó la sesión o, si no informa ninguno, el catálogo
   * del agente. Claude Code no los manda por stream-json: sin el catálogo no
   * había de dónde sacar sus niveles de esfuerzo y el selector no aparecía.
   */
  let catalogModels = $state<AgentModel[]>([]);
  const models = $derived(session?.models.length ? session.models : catalogModels);

  $effect(() => {
    const backend = session?.backendId;
    if (!backend || readOnly || (session?.models.length ?? 0) > 0) return;
    let alive = true;
    void agentListModels(backend)
      .then((list) => alive && (catalogModels = list))
      .catch(() => alive && (catalogModels = []));
    return () => {
      alive = false;
    };
  });
  const currentModel = $derived(matchModel(models, session?.model));
  const effortOptions = $derived(
    (currentModel?.efforts ?? []).map((e) => ({
      id: e.id,
      label: effortShortLabel(e.id),
      note: e.description || undefined,
    })),
  );

  /**
   * El esfuerzo en uso. Claude y Codex no lo informan hasta que se cambia: sin
   * esto el selector decía «Esfuerzo» en vez del nivel con el que corre.
   */
  const effectiveEffort = $derived(
    session?.effort || currentModel?.defaultEffort || "",
  );
  /** Cursor ofrece variante rápida del mismo modelo, aparte del esfuerzo. */
  const offersFast = $derived(
    session?.fast != null || currentModel?.supportsFast === true,
  );

  async function toggleFast() {
    if (!session?.model) return;
    error = null;
    try {
      await agents.setModel(
        sessionId,
        session.model,
        session.effort ?? undefined,
        !session.fast,
      );
    } catch (err) {
      error = String(err);
    }
  }

  const isClaude = $derived(session?.backendId === "claude-code");
  /** Modos de permiso: solo Claude Code los cambia en caliente (`/permissions`). */
  const modeOptions = PERMISSION_MODES.map((m) => ({
    id: m.id,
    label: m.label,
    note: m.note,
  }));
  /** El CLI informa `default` para lo que la UI llama «Manual». */
  const currentMode = $derived(
    !session?.mode || session.mode === "default" ? "manual" : session.mode,
  );

  async function pickModel(id: string) {
    const next = models.find((m) => m.id === id);
    // El esfuerzo vivo puede no existir en el modelo nuevo: ahí va su default.
    const effort = next?.efforts.some((e) => e.id === session?.effort)
      ? (session?.effort ?? undefined)
      : next?.defaultEffort;
    error = null;
    try {
      await agents.setModel(sessionId, id, effort, session?.fast ?? undefined);
      // La próxima sesión de este agente arranca con lo mismo.
      if (session) {
        rememberModel(session.backendId, id);
        if (effort) rememberEffort(session.backendId, id, effort);
      }
    } catch (err) {
      error = String(err);
    }
  }

  /** El slider queda abierto al elegir: se ajusta mirando el resultado. */
  async function pickEffort(id: string) {
    if (!session?.model) return;
    error = null;
    try {
      await agents.setModel(sessionId, session.model, id, session.fast ?? undefined);
      rememberEffort(session.backendId, currentModel?.id ?? session.model, id);
    } catch (err) {
      error = String(err);
    }
  }

  /**
   * Los modos del agente, si los informa (Cursor y OpenCode por ACP: agent /
   * plan / ask). Claude no: sus modos de permiso los conoce la vista.
   */
  const agentModes = $derived(
    (session?.modes ?? []).map((m) => ({
      id: m.id,
      label: m.name || m.id,
      note: m.description || undefined,
    })),
  );
  const currentAgentMode = $derived(
    agentModes.find((m) => m.id === session?.mode) ?? agentModes[0] ?? null,
  );

  async function pickAgentMode(id: string) {
    menu = null;
    error = null;
    try {
      await agents.setMode(sessionId, id);
    } catch (err) {
      error = String(err);
    }
  }

  async function pickMode(id: string) {
    menu = null;
    if (!session) return;
    rememberMode(session.backendId, id);
    error = null;
    try {
      // El adaptador intercepta `/permissions <modo>` y lo pasa por el canal
      // de control del CLI: no es un mensaje al modelo.
      await agents.send(sessionId, `/permissions ${id}`);
    } catch (err) {
      error = String(err);
    }
  }

  // ── Comandos `/` ───────────────────────────────────────────────────────

  /**
   * El catálogo del handshake si ya llegó; si no, el guardado de la sesión
   * anterior del mismo backend. El fallback y las skills de disco son de
   * Claude Code: a otro agente no se le ofrecen comandos que no tiene.
   */
  const slashCommands = $derived(
    session
      ? resolveSlashCommands(
          session.commands,
          agents.catalog[session.backendId],
          isClaude ? skillsAsCommands(agents.skills) : null,
          isClaude ? undefined : [],
        )
      : [],
  );

  /** `/algo` sin espacio todavía: el menú está abierto. Con espacio ya son args. */
  const slashQuery = $derived.by((): string | null => {
    const match = draft.match(/^\/([^\s]*)$/);
    return match ? match[1] : null;
  });

  const slashFiltered = $derived.by((): SlashCommand[] => {
    if (slashQuery === null) return [];
    const q = slashQuery.toLowerCase();
    if (!q) return slashCommands;
    return slashCommands.filter(
      (c) =>
        c.name.toLowerCase().startsWith(q) ||
        `${c.description} ${c.argumentHint}`.toLowerCase().includes(q),
    );
  });

  /**
   * Abierto solo si el agente tiene comandos: Codex y Antigravity no los
   * exponen, y un menú que siempre dice «Sin coincidencias» es ruido.
   */
  const slashOpen = $derived(slashQuery !== null && slashCommands.length > 0);

  let slashIndex = $state(0);
  const slashActive = $derived(
    slashFiltered.length === 0 ? 0 : Math.min(slashIndex, slashFiltered.length - 1),
  );

  $effect(() => {
    // Las skills solo describen el catálogo de Claude; se releen por carpeta.
    if (!isClaude || readOnly) return;
    void agents.loadSkills(session?.cwd || undefined);
  });

  /** Los que tienen selector propio lo abren en vez de escribirse. */
  function pickSlash(cmd: SlashCommand) {
    slashIndex = 0;
    const opens: Record<string, typeof menu> = {
      model: "agent",
      effort: effortOptions.length > 0 ? "effort" : null,
      permissions: isClaude ? "mode" : null,
    };
    if (opens[cmd.name]) {
      draft = "";
      menu = opens[cmd.name];
      return;
    }
    draft = cmd.argumentHint ? `/${cmd.name} ` : `/${cmd.name}`;
    input?.focus();
  }

  function onSlashKey(event: KeyboardEvent): boolean {
    const n = slashFiltered.length;
    if (n > 0 && event.key === "ArrowDown") {
      slashIndex = (slashActive + 1) % n;
    } else if (n > 0 && event.key === "ArrowUp") {
      slashIndex = (slashActive - 1 + n) % n;
    } else if (event.key === "Escape") {
      draft = "";
    } else if (
      n > 0 &&
      ((event.key === "Enter" && !event.shiftKey) || event.key === "Tab")
    ) {
      pickSlash(slashFiltered[slashActive]);
    } else {
      return false;
    }
    event.preventDefault();
    return true;
  }

  // ── Continuar una conversación ─────────────────────────────────────────

  const RECENT_MAX = 4;
  let recent = $state<StoredThread[]>([]);
  const isEmpty = $derived(blocks.length === 0);
  const offerResume = $derived(
    !readOnly && !!session && RESUMABLE.has(session.backendId) && isEmpty,
  );

  $effect(() => {
    if (!offerResume || !session) {
      recent = [];
      return;
    }
    const backendId = session.backendId;
    const cwd = session.cwd;
    void agentThreads()
      .then((list) => {
        // Sin texto del usuario no hay nada que reconocer: son chats que se
        // abrieron y no se usaron.
        const mine = list.filter(
          (th) =>
            th.backendId === backendId &&
            !th.parent &&
            th.preview.trim() &&
            canResume(th),
        );
        // Primero los de esta carpeta: es lo que más probablemente se quiere seguir.
        const here = mine.filter((th) => !!cwd && th.cwd === cwd);
        const rest = mine.filter((th) => !here.includes(th));
        recent = [...here, ...rest].slice(0, RECENT_MAX);
      })
      .catch(() => (recent = []));
  });

  // ── Trabajando ─────────────────────────────────────────────────────────

  /**
   * Cuánto lleva el turno. Se muestra solo cuando no hay otra señal de vida
   * (ni texto llegando ni actividad en vivo): es la respuesta a «¿se colgó?».
   */
  let workingSince = $state<number | null>(null);
  let now = $state(Date.now());
  const answering = $derived(!!session && sessionAnswering(session));
  const liveActivity = $derived(
    blocks.length > 0 &&
      blocks[blocks.length - 1].kind === "activity" &&
      (blocks[blocks.length - 1] as { live: boolean }).live,
  );
  // Con el hilo vacío la sesión está arrancando, no pensando.
  const showWorking = $derived(
    working && !isEmpty && !permission && !answering && !liveActivity,
  );
  /** En décimas: con segundos enteros el contador parece trabado entre saltos. */
  const elapsed = $derived(
    workingSince ? (Math.floor((now - workingSince) / 100) / 10).toFixed(1) : "0.0",
  );

  $effect(() => {
    if (!working) {
      workingSince = null;
      return;
    }
    workingSince = Date.now();
    now = Date.now();
    const timer = window.setInterval(() => (now = Date.now()), 100);
    return () => window.clearInterval(timer);
  });

  // ── Scroll y foco ──────────────────────────────────────────────────────

  function onScroll() {
    if (!scroller) return;
    pinnedToEnd =
      scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 48;
    selection = null;
  }

  // ── Selección: citar o copiar ──────────────────────────────────────────

  /**
   * Un trozo seleccionado del hilo, con dónde mostrar sus acciones. Citar
   * es la forma corta de «sobre esto que dijiste…»: sin esto había que copiar,
   * bajar al composer, pegar y armar la cita a mano.
   */
  let selection = $state<{ text: string; x: number; y: number } | null>(null);
  let rootEl = $state<HTMLElement | null>(null);

  function readSelection() {
    const sel = window.getSelection();
    const text = sel?.toString().trim() ?? "";
    if (!sel || sel.isCollapsed || !text || !scroller || !rootEl) {
      selection = null;
      return;
    }
    const range = sel.getRangeAt(0);
    if (!scroller.contains(range.commonAncestorContainer)) {
      selection = null;
      return;
    }
    const rect = range.getBoundingClientRect();
    const root = rootEl.getBoundingClientRect();
    selection = {
      text,
      x: Math.min(
        Math.max(rect.left + rect.width / 2 - root.left, 70),
        root.width - 70,
      ),
      y: Math.max(rect.top - root.top - 8, 8),
    };
  }

  $effect(() => {
    // Oyentes y no atributos: el hilo no es un control, solo se mira qué
    // quedó seleccionado al soltar el mouse o mover la selección con teclado.
    const el = scroller;
    if (!el) return;
    const onUp = () => requestAnimationFrame(readSelection);
    el.addEventListener("pointerup", onUp);
    el.addEventListener("keyup", readSelection);
    return () => {
      el.removeEventListener("pointerup", onUp);
      el.removeEventListener("keyup", readSelection);
    };
  });

  function quoteSelection() {
    if (!selection) return;
    const quoted = selection.text
      .split("\n")
      .map((line) => `> ${line}`)
      .join("\n");
    draft = draft.trim() ? `${draft.trimEnd()}\n\n${quoted}\n\n` : `${quoted}\n\n`;
    selection = null;
    window.getSelection()?.removeAllRanges();
    input?.focus();
  }

  async function copySelection() {
    if (!selection) return;
    try {
      await navigator.clipboard.writeText(selection.text);
    } catch {
      /* sin permiso de portapapeles: la selección sigue ahí para Ctrl+C */
    }
    selection = null;
  }

  $effect(() => {
    // Crecer en su sitio también cuenta: el texto que llega o una acción más.
    const last = blocks[blocks.length - 1];
    void blocks.length;
    void (last?.kind === "activity"
      ? last.items.length
      : last && "item" in last && "text" in last.item
        ? last.item.text.length
        : 0);
    void permission;
    void showWorking;
    if (!pinnedToEnd) return;
    void tick().then(() => {
      if (scroller) scroller.scrollTop = scroller.scrollHeight;
    });
  });

  $effect(() => {
    // Abrir un chat es para escribirle.
    if (readOnly) return;
    void tick().then(() => input?.focus());
  });

  // ── Adjuntos ───────────────────────────────────────────────────────────

  /**
   * Imágenes que viajan embebidas en el próximo turno. Otro archivo no: el
   * puente solo embebe imágenes, así que el resto entra como ruta en el texto.
   */
  let attaches = $state<{ path: string; name: string }[]>([]);
  /** Por dónde entraron: se dibuja junto al mensaje («portapapeles», «archivo»). */
  let attachVia = $state("");
  const IMAGE_EXT = /\.(png|jpe?g|gif|webp)$/i;

  function appendText(text: string) {
    const clean = text.trimEnd();
    if (!clean) return;
    draft = draft.trim() ? `${draft.trimEnd()}\n${clean}` : clean;
  }

  function addPath(path: string, via: string) {
    const clean = path.trim();
    if (!clean) return;
    if (!IMAGE_EXT.test(clean)) {
      appendText(clean);
      return;
    }
    if (attaches.some((a) => a.path === clean)) return;
    attachVia = via;
    attaches = [
      ...attaches,
      { path: clean, name: clean.split(/[/\\]/).pop() ?? clean },
    ];
  }

  async function stageImage(file: Blob, via: string) {
    const mime = (file.type || "image/png").toLowerCase();
    if (!mime.startsWith("image/")) return;
    const bytes = new Uint8Array(await file.arrayBuffer());
    let binary = "";
    for (let i = 0; i < bytes.length; i += 0x8000) {
      binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
    }
    addPath(await agentStageImage(btoa(binary), mime), via);
  }

  async function onPaste(event: ClipboardEvent) {
    const images = [...(event.clipboardData?.items ?? [])].filter(
      (item) => item.kind === "file" && item.type.startsWith("image/"),
    );
    if (images.length === 0) return;
    event.preventDefault();
    try {
      for (const item of images) {
        const file = item.getAsFile();
        if (file) await stageImage(file, t("page.agents.chat.viaClipboard"));
      }
    } catch (err) {
      error = String(err);
    }
  }

  async function pickFiles() {
    try {
      // El diálogo nativo le quita el foco al overlay: sin la guarda, el
      // float se achica justo cuando uno está eligiendo.
      const paths = await withAgentsDismissSuppressed(() => pickAgentFiles());
      for (const path of paths) addPath(path, t("page.agents.chat.viaFile"));
      input?.focus();
    } catch (err) {
      error = String(err);
    }
  }

  /** El historial de Atic arrastra texto como `.atic-drag-*.txt`: se lee, no se adjunta. */
  async function dropPath(path: string) {
    const name = path.split(/[/\\]/).pop() ?? "";
    if (name.startsWith(".atic-drag-") && name.endsWith(".txt")) {
      appendText((await readClipboardDragText(path)) ?? "");
      return;
    }
    addPath(path, t("page.agents.chat.viaFile"));
  }

  function onDragOver(event: DragEvent) {
    if (readOnly || !event.dataTransfer) return;
    // Propio: sin esto, el panel de consolas lo toma como destino de una PTY.
    event.preventDefault();
    event.stopPropagation();
    event.dataTransfer.dropEffect = "copy";
  }

  async function onDrop(event: DragEvent) {
    if (readOnly) return;
    event.preventDefault();
    event.stopPropagation();
    const dt = event.dataTransfer;
    if (!dt) return;
    try {
      let added = false;
      for (const file of dt.files ?? []) {
        const path = (file as File & { path?: string }).path?.trim();
        if (path) await dropPath(path);
        else if (file.type.startsWith("image/"))
          await stageImage(file, t("page.agents.chat.viaFile"));
        else continue;
        added = true;
      }
      if (!added) {
        for (const line of (dt.getData("text/uri-list") || "").split(/\r?\n/)) {
          const uri = line.trim();
          if (!uri.startsWith("file:")) continue;
          let local = decodeURIComponent(uri.replace(/^file:\/\//, ""));
          if (/^\/[A-Za-z]:/.test(local)) local = local.slice(1);
          await dropPath(local);
          added = true;
        }
      }
      if (!added) appendText(dt.getData("text/plain") || "");
      input?.focus();
    } catch (err) {
      error = String(err);
    }
  }

  onMount(() => {
    if (readOnly) return;
    onRegister?.({
      insertText: (text) => {
        appendText(text);
        input?.focus();
      },
      attachImage: (path) => {
        addPath(path, t("page.agents.chat.viaClipboard"));
        input?.focus();
      },
    });
    return () => onRegister?.(null);
  });

  // ── Enviar, cortar, decidir ────────────────────────────────────────────

  const shownError = $derived(error ?? session?.error ?? null);
  /** Credenciales del proveedor: no se arregla reintentando igual. */
  const authError = $derived(
    !!shownError && /auth|401|unauthori[sz]ed|credential/i.test(shownError),
  );

  const canSend = $derived((!!draft.trim() || attaches.length > 0) && !sending);

  async function send() {
    const text = draft.trim();
    const pending = attaches;
    if ((!text && pending.length === 0) || sending || working) return;
    const origin: AgentOrigin | undefined =
      pending.length > 0
        ? {
            via: attachVia,
            file:
              pending.length === 1
                ? pending[0].name
                : t("page.agents.chat.images", { n: pending.length }),
            files: pending.map((a) => a.path),
          }
        : undefined;
    sending = true;
    error = null;
    draft = "";
    attaches = [];
    pinnedToEnd = true;
    try {
      await agents.send(sessionId, text, origin);
    } catch (err) {
      error = String(err);
      draft = text;
      attaches = pending;
    } finally {
      sending = false;
    }
  }

  async function stop() {
    error = null;
    try {
      await agents.interrupt(sessionId);
    } catch (err) {
      error = String(err);
    }
  }

  async function answer(picked: string[][], written: string[]) {
    if (!permission || !questions || deciding) return;
    deciding = true;
    error = null;
    try {
      await agents.answer(
        sessionId,
        permission.id,
        withAnswers(permission.input, questions, picked, written),
      );
    } catch (err) {
      error = String(err);
    } finally {
      deciding = false;
    }
  }

  async function decide(decision: PermissionDecision) {
    if (!permission || deciding) return;
    deciding = true;
    error = null;
    try {
      await agents.decide(sessionId, permission.id, decision);
    } catch (err) {
      error = String(err);
    } finally {
      deciding = false;
    }
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.isComposing) return;
    if (slashOpen && onSlashKey(event)) return;
    if (event.key !== "Enter" || event.shiftKey) return;
    event.preventDefault();
    void send();
  }

  function folderName(path: string | undefined): string {
    if (!path) return "";
    return (
      path
        .replace(/[/\\]+$/, "")
        .split(/[/\\]/)
        .pop() ?? path
    );
  }
</script>

<!-- Soltar archivos sobre el chat entero los adjunta al composer. -->
<div
  class="chat"
  data-agent={session?.backendId}
  role="presentation"
  bind:this={rootEl}
  ondragover={onDragOver}
  ondrop={(e) => void onDrop(e)}
>
  {#if selection}
    <div
      class="sel-actions"
      style:left={`${selection.x}px`}
      style:top={`${selection.y}px`}
      role="toolbar"
      aria-label={t("page.agents.chat.selection")}
    >
      {#if !readOnly}
        <button
          type="button"
          onpointerdown={(e) => e.preventDefault()}
          onclick={quoteSelection}
        >
          {t("page.agents.chat.quote")}
        </button>
      {/if}
      <button
        type="button"
        onpointerdown={(e) => e.preventDefault()}
        onclick={() => void copySelection()}
      >
        {t("page.agents.chat.copy")}
      </button>
    </div>
  {/if}
  {#if !session}
    <p class="gone">{t("page.agents.chat.gone")}</p>
  {:else}
    <div class="scroll" bind:this={scroller} onscroll={onScroll}>
      <div class="column thread">
        {#if readOnly}
          <p class="origin">
            {requestedBy
              ? t("page.agents.chat.requestedBy", {
                  name: session.label?.trim() || session.backendName,
                  parent: requestedBy,
                })
              : session.label?.trim() || session.backendName}
          </p>
        {/if}

        {#if isEmpty && !readOnly}
          <div class="hello">
            <AgentLogo agent={agentDef?.cli ?? session.backendId} size={30} />
            <p class="hello-title">{t("page.agents.chat.hello")}</p>
            <p class="hello-sub">
              {session.backendName}{#if session.cwd}
                · {folderName(session.cwd)}{/if}
            </p>
          </div>
          {#if recent.length > 0}
            <section class="recent" aria-label={t("page.agents.chat.continue")}>
              <p class="recent-h">{t("page.agents.chat.continue")}</p>
              {#each recent as thread (thread.id)}
                <button
                  type="button"
                  class="recent-item"
                  onclick={() => onOpenThread?.(thread)}
                >
                  <span class="recent-preview">{thread.preview}</span>
                  <span class="recent-when">{formatListWhen(thread.updatedAt)}</span>
                </button>
              {/each}
            </section>
          {/if}
        {:else if isEmpty}
          <p class="quiet">{t("page.agents.chat.silent")}</p>
        {:else}
          <AgentConversation {blocks} />
        {/if}

        {#if showWorking}
          <p class="working" role="status">
            <span class="shimmer">{t("page.agents.chat.working")}</span>
            <span class="working-time">{elapsed}s</span>
          </p>
        {/if}

        {#if toolQuestion}
          {#key toolQuestion.id}
            <ChatQuestion
              questions={toolQuestion.questions}
              agentName={session.backendName}
              onAnswer={(picked, written) => void answerToolQuestion(picked, written)}
              onSkip={() => (skippedToolQuestion = toolQuestion?.id ?? null)}
            />
          {/key}
        {:else if permission && questions}
          {#key permission.id}
            <ChatQuestion
              {questions}
              agentName={session.backendName}
              busy={deciding}
              allowOwn={session.backendId !== "cursor"}
              onAnswer={(picked, written) => void answer(picked, written)}
              onSkip={() => void decide("deny")}
            />
          {/key}
        {:else if permission && permission.tool === PLAN_TOOL}
          {#key permission.id}
            <ChatPlan
              input={permission.input}
              agentName={session.backendName}
              busy={deciding}
              onDecide={(decision) => void decide(decision)}
            />
          {/key}
        {:else if permission}
          <ChatPermission
            {permission}
            agentName={session.backendName}
            busy={deciding}
            position={pendingCount > 1
              ? {
                  index: Math.min(permissionIndex, pendingCount - 1),
                  total: pendingCount,
                }
              : null}
            onMove={(step) =>
              (permissionIndex =
                (Math.min(permissionIndex, pendingCount - 1) + step + pendingCount) %
                pendingCount)}
            onDecide={(decision) => void decide(decision)}
          />
        {/if}
      </div>
    </div>

    {#if !readOnly}
      <div class="column dock">
        {#if shownError}
          <div class="error" role="alert">
            <p class="error-text">{shownError}</p>
            {#if authError}
              <p class="error-hint">
                {t("page.agents.chat.authHint", { name: session.backendName })}
              </p>
            {/if}
          </div>
        {/if}
        <form
          class="composer"
          onsubmit={(event) => {
            event.preventDefault();
            void send();
          }}
        >
          {#if slashOpen}
            <SlashPalette
              commands={slashFiltered}
              activeIndex={slashActive}
              emptyHint={t("page.agents.chat.noCommands")}
              onPick={pickSlash}
              onHover={(i) => (slashIndex = i)}
            />
          {/if}
          {#if attaches.length > 0}
            <div class="attaches">
              {#each attaches as attach (attach.path)}
                <span class="attach" title={attach.name}>
                  <img src={convertFileSrc(attach.path)} alt="" draggable="false" />
                  <button
                    type="button"
                    class="attach-x"
                    aria-label={t("page.agents.chat.removeAttach", {
                      name: attach.name,
                    })}
                    onclick={() =>
                      (attaches = attaches.filter((a) => a.path !== attach.path))}
                  >
                    <Icon icon={X} size={10} />
                  </button>
                </span>
              {/each}
            </div>
          {/if}
          <textarea
            bind:this={input}
            bind:value={draft}
            rows="1"
            placeholder={t("page.agents.chat.placeholder", {
              name: session.backendName,
            })}
            oninput={() => (slashIndex = 0)}
            onpaste={(e) => void onPaste(e)}
            onkeydown={onKeydown}></textarea>
          <div class="tools">
            <button
              type="button"
              class="tool"
              aria-label={t("page.agents.chat.attach")}
              onclick={() => void pickFiles()}
            >
              <Icon icon={Paperclip} size={14} />
            </button>
            <ChatHistory
              open={menu === "history"}
              onToggle={(open) => (menu = open ? "history" : null)}
              onPick={(thread) => onOpenThread?.(thread)}
            />
            <!-- Agente, modelo y esfuerzo son una sola elección: van juntos. -->
            <span class="model-group">
              <AgentModelPicker
                {choices}
                backendId={session.backendId}
                modelId={session.model}
                sessionModels={models}
                open={menu === "agent"}
                onToggle={(open) => (menu = open ? "agent" : null)}
                onPickModel={(id) => void pickModel(id)}
                onPickAgent={(agent, modelId) => onSwitchAgent?.(agent, modelId)}
              />
              {#if effortOptions.length > 0}
                <span class="sep" aria-hidden="true"></span>
                <EffortSlider
                  levels={effortOptions}
                  value={effectiveEffort}
                  defaultValue={currentModel?.defaultEffort ?? ""}
                  modelName={currentModel?.name ?? session.model ?? ""}
                  open={menu === "effort"}
                  onToggle={(open) => (menu = open ? "effort" : null)}
                  onPick={(id) => void pickEffort(id)}
                />
              {/if}
            </span>
            {#if offersFast}
              <button
                type="button"
                class="toggle"
                aria-pressed={session.fast === true}
                title={t("page.agents.chat.fastHint")}
                onclick={() => void toggleFast()}
              >
                {t("page.agents.chat.fast")}
              </button>
            {/if}
            {#if agentModes.length > 0 && currentAgentMode}
              <ChatSelect
                label={currentAgentMode.label}
                ariaLabel={t("page.agents.chat.agentMode")}
                options={agentModes}
                value={currentAgentMode.id}
                open={menu === "mode"}
                onToggle={(open) => (menu = open ? "mode" : null)}
                onPick={(id) => void pickAgentMode(id)}
              />
            {:else if isClaude}
              <ChatSelect
                label={modeShortLabel(currentMode)}
                ariaLabel={t("page.agents.chat.mode")}
                options={modeOptions}
                value={currentMode}
                open={menu === "mode"}
                onToggle={(open) => (menu = open ? "mode" : null)}
                onPick={(id) => void pickMode(id)}
              />
            {/if}
            {#if session.mcpServers.length > 0}
              <span class="spacer"></span>
              <ChatMcp
                servers={session.mcpServers}
                tools={session.tools}
                open={menu === "mcp"}
                onToggle={(open) => (menu = open ? "mcp" : null)}
              />
            {/if}
            {#if working}
              <button
                type="button"
                class="send is-stop"
                aria-label={t("page.agents.chat.stop")}
                onclick={() => void stop()}
              >
                <Icon icon={Square} size={11} />
              </button>
            {:else}
              <button
                type="submit"
                class="send"
                aria-label={t("page.agents.chat.send")}
                disabled={!canSend}
              >
                <Icon icon={ArrowUp} size={15} />
              </button>
            {/if}
          </div>
        </form>
      </div>
    {/if}
  {/if}
</div>

<style>
  .chat {
    /* Tokens que heredan AgentMessage, ToolCard y PickerMenu. */
    --coral: var(--accent);
    --text: var(--rb-text);
    --dim: var(--rb-muted);
    --faint: var(--rb-faint);
    --line: var(--rb-border);
    --card: var(--rb-surface-2);
    --code: var(--rb-surface-2);
    --hover: color-mix(in sRGB, var(--rb-text) 6%, transparent);
    --add: var(--rb-ok);
    --del: var(--rb-record);

    position: relative;
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    color: var(--rb-text);
    font-family: var(--rb-font);
    font-size: 13px;
    -webkit-font-smoothing: antialiased;
    cursor: auto;
  }

  /* Una columna de lectura: a todo el ancho de un pane grande las líneas se
     vuelven imposibles de seguir. Hilo y composer comparten el mismo eje. */
  .column {
    box-sizing: border-box;
    width: 100%;
    max-width: 760px;
    margin: 0 auto;
    padding: 0 16px;
  }

  /* Sobre la selección, centrada: la acción queda donde está la vista. */
  .sel-actions {
    position: absolute;
    z-index: 30;
    display: flex;
    gap: 2px;
    border-radius: 9px;
    padding: 3px;
    background: var(--rb-surface-elevated, var(--rb-surface));
    box-shadow:
      0 0 0 1px color-mix(in sRGB, var(--rb-text) 12%, transparent),
      0 8px 20px -8px rgb(0 0 0 / 50%);
    transform: translate(-50%, -100%);
  }

  .sel-actions button {
    border: 0;
    border-radius: 6px;
    padding: 4px 10px;
    background: transparent;
    color: var(--rb-text);
    font: inherit;
    font-size: 12px;
    font-weight: 560;
    cursor: pointer;
  }

  .sel-actions button:hover {
    background: color-mix(in sRGB, var(--rb-text) 9%, transparent);
  }

  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    scrollbar-gutter: stable both-edges;
  }

  .thread {
    display: flex;
    flex-direction: column;
    gap: 12px;
    min-height: 100%;
    padding-top: 16px;
    padding-bottom: 16px;
    line-height: 1.55;
  }

  .origin {
    margin: 0;
    color: var(--rb-faint);
    font-size: 11.5px;
  }

  .gone,
  .quiet {
    margin: auto;
    color: var(--rb-muted);
    font-size: 12px;
  }

  /* Vacío: centrado, con lo que hace falta para empezar y nada más. */
  .hello {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    margin-top: auto;
    padding-bottom: 8px;
    text-align: center;
  }

  .hello-title {
    margin: 6px 0 0;
    font-size: 17px;
    font-weight: 600;
    text-wrap: balance;
  }

  .hello-sub {
    margin: 0;
    color: var(--rb-muted);
    font-size: 12px;
  }

  .recent {
    display: flex;
    flex-direction: column;
    gap: 2px;
    width: 100%;
    max-width: 440px;
    margin: 8px auto auto;
  }

  .recent-h {
    margin: 0 0 4px 8px;
    color: var(--rb-faint);
    font-size: 11px;
    font-weight: 600;
  }

  .recent-item {
    display: flex;
    align-items: baseline;
    gap: 10px;
    border: 0;
    border-radius: 8px;
    padding: 7px 8px;
    background: transparent;
    color: var(--rb-text);
    font: inherit;
    font-size: 12.5px;
    text-align: left;
    cursor: pointer;
    transition: background-color 120ms ease;
  }

  .recent-item:hover:not(:disabled) {
    background: color-mix(in sRGB, var(--rb-text) 7%, transparent);
  }

  .recent-item:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .recent-preview {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .recent-when {
    flex-shrink: 0;
    color: var(--rb-faint);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }

  .working {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0;
    color: var(--rb-muted);
    font-size: 12px;
  }

  /* Un brillo que barre el texto: se lee «vivo» sin un punto más que mirar. */
  .shimmer {
    background: linear-gradient(
        100deg,
        var(--rb-muted) 35%,
        var(--rb-text) 50%,
        var(--rb-muted) 65%
      )
      0 0 / 250% 100%;
    background-clip: text;
    color: transparent;
    animation: shimmer 1.6s linear infinite;
  }

  @keyframes shimmer {
    from {
      background-position: 100% 0;
    }

    to {
      background-position: 0% 0;
    }
  }

  .working-time {
    color: var(--rb-faint);
    font-variant-numeric: tabular-nums;
  }

  .dock {
    flex-shrink: 0;
    padding-bottom: 12px;
  }

  .error {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-bottom: 8px;
    border-radius: 12px;
    padding: 10px 12px;
    background: color-mix(in sRGB, var(--rb-record) 10%, var(--rb-surface));
    box-shadow: inset 0 0 0 1px color-mix(in sRGB, var(--rb-record) 30%, transparent);
  }

  .error-text {
    display: -webkit-box;
    margin: 0;
    overflow: hidden;
    color: var(--rb-text);
    font-size: 12px;
    line-height: 1.45;
    overflow-wrap: anywhere;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 4;
    line-clamp: 4;
  }

  .error-hint {
    margin: 0;
    color: var(--rb-muted);
    font-size: 11.5px;
  }

  .toggle {
    flex-shrink: 0;
    height: 28px;
    border: 0;
    border-radius: 8px;
    padding: 0 8px;
    background: transparent;
    color: var(--rb-muted);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
    transition:
      background-color 120ms ease,
      color 120ms ease;
  }

  .toggle:hover {
    background: color-mix(in sRGB, var(--rb-text) 8%, transparent);
  }

  .toggle[aria-pressed="true"] {
    background: color-mix(in sRGB, var(--accent) 16%, transparent);
    color: var(--rb-text);
  }

  .composer {
    /* Ancla del menú `/` y del selector, que abren hacia arriba. */
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 6px;
    border-radius: 16px;
    padding: 10px 10px 8px 14px;
    background: var(--rb-surface-2);
    box-shadow:
      0 0 0 1px color-mix(in sRGB, var(--rb-text) 9%, transparent),
      0 10px 28px -18px rgb(0 0 0 / 55%);
    transition: box-shadow 140ms ease;
  }

  .composer:focus-within {
    box-shadow:
      0 0 0 1px color-mix(in sRGB, var(--accent) 45%, transparent),
      0 10px 28px -18px rgb(0 0 0 / 55%);
  }

  textarea {
    width: 100%;
    max-height: 200px;
    field-sizing: content;
    min-height: 22px;
    resize: none;
    border: 0;
    outline: 0;
    padding: 2px 0;
    background: transparent;
    color: inherit;
    font: inherit;
    font-size: 13.5px;
    line-height: 1.5;
  }

  textarea::placeholder {
    color: var(--rb-faint);
  }

  .tools {
    display: flex;
    align-items: center;
    gap: 2px;
    min-width: 0;
    margin-left: -6px;
  }

  .tool {
    display: grid;
    flex-shrink: 0;
    place-items: center;
    width: 28px;
    height: 28px;
    border: 0;
    border-radius: 8px;
    background: transparent;
    color: var(--rb-muted);
    cursor: pointer;
    transition:
      background-color 120ms ease,
      color 120ms ease;
  }

  .tool:hover {
    background: color-mix(in sRGB, var(--rb-text) 8%, transparent);
    color: var(--rb-text);
  }

  .spacer {
    flex: 1;
  }

  .model-group {
    display: flex;
    align-items: center;
    min-width: 0;
    margin-left: 2px;
    border-radius: 9px;
    background: color-mix(in sRGB, var(--rb-text) 5%, transparent);
  }

  .model-group :global(.trigger) {
    color: var(--rb-text);
  }

  .sep {
    flex-shrink: 0;
    width: 1px;
    height: 14px;
    background: color-mix(in sRGB, var(--rb-text) 12%, transparent);
  }

  /* Radio concéntrico: 16 del composer − 8 de padding = 8. */
  .send {
    display: grid;
    flex-shrink: 0;
    place-items: center;
    width: 30px;
    height: 30px;
    margin-left: auto;
    border: 0;
    border-radius: 8px;
    background: var(--accent);
    color: var(--rb-on-accent, var(--rb-bg0));
    cursor: pointer;
    transition:
      background-color 120ms ease,
      opacity 120ms ease,
      scale 120ms ease;
  }

  .send:active:not(:disabled) {
    scale: 0.96;
  }

  .send:disabled {
    background: color-mix(in sRGB, var(--rb-text) 12%, transparent);
    color: var(--rb-muted);
    cursor: default;
  }

  .send.is-stop {
    background: color-mix(in sRGB, var(--rb-text) 14%, transparent);
    color: var(--rb-text);
  }

  .attaches {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    padding-top: 2px;
  }

  .attach {
    position: relative;
    width: 48px;
    height: 48px;
  }

  .attach img {
    width: 100%;
    height: 100%;
    border-radius: 8px;
    object-fit: cover;
    outline: 1px solid rgb(255 255 255 / 10%);
    outline-offset: -1px;
  }

  :global([data-theme-base="light"]) .attach img {
    outline: 1px solid rgb(0 0 0 / 10%);
  }

  .attach-x {
    position: absolute;
    top: -6px;
    right: -6px;
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    border: 0;
    border-radius: 999px;
    background: var(--rb-text);
    color: var(--rb-bg0);
    cursor: pointer;
  }

  @media (prefers-reduced-motion: reduce) {
    .shimmer {
      animation: none;
      color: var(--rb-muted);
    }
  }
</style>
