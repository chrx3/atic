/**
 * Las sesiones de agente, vistas desde el frontend.
 *
 * El proceso lo tiene Rust: sigue corriendo con el panel cerrado, con la
 * ventana escondida, y con la pill mostrando otra cosa. Este módulo es solo la
 * lectura de ese proceso, y existe por dos motivos que el componente no puede
 * resolver solo:
 *
 *  1. Un componente se desmonta al cerrar el panel. Si la conversación viviera
 *     ahí, cerrar la pill borraría lo que el agente respondió mientras tanto —
 *     que es justo el caso de uso: mandarle algo largo y seguir trabajando.
 *  2. Los eventos son globales. Una sesión que arrancó la ventana principal
 *     también llega a la pill, así que cualquiera de las dos puede adoptarla.
 *     Por eso las sesiones se crean al VER el evento, no al pedirlas.
 *
 * Hay una instancia por webview (pill y ventana principal son contextos JS
 * distintos). El estado compartido de verdad es el de Rust; esto se pone al día
 * con `agentSessions()` al montar y después sigue el flujo de eventos.
 */
import {
  agentInterrupt,
  agentPermission,
  agentSend,
  agentSessions,
  agentSetModel,
  agentSkills,
  agentStart,
  agentStop,
  onAgentDelta,
} from "$lib/api";
import type {
  AgentDeltaPayload,
  AgentItem,
  AgentModel,
  AgentOrigin,
  AgentTurn,
  AgentSkill,
  AgentStartOptions,
  McpServerState,
  PermissionDecision,
  SlashCommand,
} from "$lib/types";
import { clipChipPreview } from "$core/agentChipPreview";

/** Un permiso pendiente: el agente está detenido esperando la respuesta. */
export interface PendingPermission {
  id: string;
  tool: string;
  description: string;
  input: unknown;
}

/** Catálogo `/` de la última sesión, para el primer `/` en frío. */
const CATALOG_KEY = "atic.agents.slashCatalog";

function readStoredCatalog(): Record<string, SlashCommand[]> {
  try {
    const raw = localStorage.getItem(CATALOG_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as unknown;
    if (!parsed || typeof parsed !== "object") return {};
    return parsed as Record<string, SlashCommand[]>;
  } catch {
    return {};
  }
}

function writeStoredCatalog(catalog: Record<string, SlashCommand[]>): void {
  try {
    localStorage.setItem(CATALOG_KEY, JSON.stringify(catalog));
  } catch {
    // Quota / modo privado: el menú sigue con fallback + skills.
  }
}

function lastAssistantText(s: AgentSessionView): string | null {
  for (let i = s.turns.length - 1; i >= 0; i--) {
    const items = s.turns[i].items;
    for (let j = items.length - 1; j >= 0; j--) {
      const it = items[j];
      if (it.kind !== "message" || it.role !== "assistant") continue;
      if (it.text.trim()) return it.text;
    }
  }
  return null;
}

/**
 * En qué anda una sesión.
 *
 * `working` no lo dice el backend: se deduce de tener un turno abierto. Es lo
 * que permite mostrar «pensando» sin preguntar.
 */
export type AgentStatus = "ready" | "working" | "waiting" | "failed";

export interface AgentSessionView {
  id: string;
  backendId: string;
  backendName: string;
  status: AgentStatus;
  /**
   * La conversación, en turnos.
   *
   * Reemplaza al `log: AgentEventPayload[]` plano. La diferencia práctica: acá
   * los items tienen identidad, así que una tarjeta de herramienta pasa de
   * «ejecutando» a «listo» **en su sitio**, y ya no hace falta un `streaming`
   * suelto en paralelo para saber dónde poner el texto que va llegando.
   */
  turns: AgentTurn[];
  /** Respuestas llegadas mientras nadie miraba esta sesión. */
  unread: number;
  /** Última línea del agente: el resumen que cabe en la pill. */
  lastText: string | null;
  /** Epoch secs de `lastText`, para no perderle a una TUI más nueva. */
  lastTextAt: number;
  error: string | null;
  /** Permisos esperando respuesta. Mientras haya uno, el agente no avanza. */
  pending: PendingPermission[];
  /** Contexto consumido, en tokens. */
  contextTokens: number;
  /**
   * Tamaño de la ventana, cuando el agente lo informa.
   *
   * `null` mientras no lo diga: ahí la vista cae en su valor por defecto, que
   * es lo único que se puede hacer sin saberlo.
   */
  contextSize: number | null;
  /** Costo acumulado de la sesión. */
  costUsd: number;
  cwd: string;
  model: string;
  /** Modo de permisos efectivo informado por el backend. */
  mode: string;
  /** Id con el que el backend reanuda esta conversación. */
  providerSession: string | null;
  /**
   * Los modelos que ofrece este agente, tal como los informó él.
   *
   * Vacío mientras no llegue el aviso, y también para siempre en OpenCode y
   * Cursor: ACP no nombra los modelos en su protocolo, así que ahí el modelo lo
   * elige su propia configuración y no hay nada que ofrecer.
   */
  models: AgentModel[];
  /** Esfuerzo en curso, si el backend lo maneja. */
  effort: string | null;
  /** Variante rápida (Cursor). */
  fast: boolean | null;
  /** Con descripción, del canal de control. Es la que sirve para ofrecerlos. */
  commands: SlashCommand[];
  mcpServers: McpServerState[];
  tools: string[];
}

class AgentSessionStore {
  sessions = $state<AgentSessionView[]>([]);
  /** Sesión que se está mirando ahora mismo; sus respuestas no son «nuevas». */
  watching = $state<string | null>(null);

  /**
   * Comandos vistos por backend, entre sesiones.
   *
   * Llegan al abrir una sesión, pero se ofrecen ANTES de tener una: escribir
   * `/` en el compositor vacío no mostraba nada, que es lo mismo que no tener
   * autocompletado. Son estables para un backend dado, así que los de la última
   * sesión sirven perfectamente para el primer `/` de la siguiente.
   */
  catalog = $state<Record<string, SlashCommand[]>>(readStoredCatalog());

  /**
   * Skills leídas del disco, con descripción.
   *
   * El agente ya las ofrece entre sus comandos de barra, pero con el nombre
   * pelado. Un nombre suelto sirve para invocar la que ya conocés y no para
   * encontrar la que no; la descripción, que es lo único que dice para qué es,
   * solo está en el archivo.
   */
  skills = $state<AgentSkill[]>([]);

  #started = false;
  #unlisten: Promise<() => void> | null = null;
  /**
   * Quién avisa al sistema operativo.
   *
   * Solo una ventana lo hace: los eventos llegan a todas, y sin esto una misma
   * respuesta produciría un toast por ventana abierta. La pill se ofrece porque
   * es la que siempre está viva.
   */
  #notifier = false;
  /**
   * Si un turno no cierra (`turn.end` / `failed`), el composer queda bloqueado.
   * Tras 45s sin deltas, liberamos la sesión.
   */
  static readonly WORKING_TIMEOUT_MS = 45_000;
  #workingTimers = new Map<string, ReturnType<typeof setTimeout>>();
  /** Sesiones que el usuario pidió cortar: un delta tardío no las revive. */
  #stopped = new Set<string>();

  /** Hay algo pendiente de leer: lo que enciende el aviso en la pill. */
  get unread(): number {
    return this.sessions.reduce((total, s) => total + s.unread, 0);
  }

  /** Algún agente está trabajando ahora mismo. */
  get working(): boolean {
    return this.sessions.some((s) => s.status === "working");
  }

  /**
   * Alguien está esperando una decisión tuya.
   *
   * Se separa de `unread` porque es la única señal con urgencia real: el agente
   * está detenido y no avanza hasta que contestes.
   */
  get waiting(): number {
    return this.sessions.reduce((total, s) => total + s.pending.length, 0);
  }

  /** Primer permiso pendiente de cualquier sesión (el que muestra el diálogo). */
  get primaryPending(): {
    sessionId: string;
    permission: PendingPermission;
  } | null {
    for (const s of this.sessions) {
      const p = s.pending[0];
      if (p) return { sessionId: s.id, permission: p };
    }
    return null;
  }

  /**
   * Texto corto para el chip de la pill.
   *
   * En «listo» es el último mensaje sin leer. Mientras escribe, el mismo
   * recorte: si no, el chip se queda en el saludo y no sigue la respuesta.
   */
  get #chipSession(): AgentSessionView | undefined {
    const busy = this.sessions.find((x) => x.status === "working");
    if (busy) return busy;
    const unread = this.sessions.filter((x) => x.unread > 0);
    if (unread.length === 0) return undefined;
    return unread.reduce((a, b) => (a.lastTextAt >= b.lastTextAt ? a : b));
  }

  get readyLabel(): string | null {
    if (this.waiting > 0) return null;
    const s = this.#chipSession;
    if (!s) return null;
    const text = s.lastText?.trim();
    if (!text) return this.working ? null : "Listo";
    return clipChipPreview(text);
  }

  get readyBackendId(): string | null {
    return this.#chipSession?.backendId ?? null;
  }

  get readyUpdatedAt(): number {
    return this.#chipSession?.lastTextAt ?? 0;
  }

  /**
   * Empieza a escuchar y adopta lo que ya estuviera corriendo. Idempotente:
   * cualquier vista puede llamarlo al montar sin coordinarse con las demás.
   */
  async init({ notify = false } = {}): Promise<void> {
    if (notify) this.#notifier = true;
    if (this.#started) return;
    this.#started = true;
    this.#unlisten = onAgentDelta((payload) => this.#receive(payload));
    try {
      const live = await agentSessions();
      for (const info of live) this.#ensure(info.id, info.backendId, info.backendName);
    } catch (err) {
      console.warn("adoptar sesiones de agente", err);
    }
  }

  /** Solo para tests y recargas: en la app el listener vive lo que la ventana. */
  async dispose(): Promise<void> {
    const un = this.#unlisten;
    this.#started = false;
    this.#unlisten = null;
    if (un) (await un)();
  }

  async start(
    backendId: string,
    options?: AgentStartOptions,
  ): Promise<string> {
    const id = await agentStart(backendId, options);
    this.#stopped.delete(id);
    // El primer evento la crearía igual; crearla acá evita el hueco en el que
    // la UI ya tiene el id pero todavía no tiene nada que mostrar.
    this.#ensure(id, backendId, backendId);
    return id;
  }

  /**
   * Rellena una sesión viva con un hilo guardado (p. ej. tras `resume`).
   * Sin esto el transcript queda vacío hasta el próximo turno.
   */
  hydrate(
    id: string,
    patch: {
      turns?: AgentTurn[];
      cwd?: string;
      model?: string;
      providerSession?: string | null;
      costUsd?: number;
    },
  ): void {
    const s = this.byId(id);
    if (!s) return;
    if (patch.turns) s.turns = patch.turns;
    if (patch.cwd !== undefined) s.cwd = patch.cwd;
    if (patch.model !== undefined) s.model = patch.model;
    if (patch.providerSession !== undefined) {
      s.providerSession = patch.providerSession;
    }
    if (patch.costUsd !== undefined) s.costUsd = patch.costUsd;
  }

  async send(id: string, text: string, origin?: AgentOrigin): Promise<void> {
    const session = this.byId(id);
    if (session) {
      session.status = "working";
      session.error = null;
      this.#armWorkingTimeout(id);
    }
    try {
      await agentSend(id, text, origin);
    } catch (err) {
      this.#clearWorkingTimeout(id);
      if (session) {
        session.status = "failed";
        session.error = String(err);
      }
      throw err;
    }
  }

  /**
   * Compacta el contexto del CLI (`/compact`), opcionalmente con instrucciones
   * de qué conservar. El adaptador pinta aviso + resumen; acá se recorta el
   * historial de Atic para que coincida con el contexto vivo.
   */
  async compact(id: string, keep?: string): Promise<void> {
    const hint = keep?.trim();
    const cmd = hint ? `/compact ${hint}` : "/compact";
    await this.send(id, cmd);
  }

  /**
   * Tras un resumen de compactación, deja solo desde el aviso de compactar.
   * El JSONL del CLI conserva todo; la UI refleja el contexto activo.
   */
  collapseAfterCompact(id: string): void {
    const s = this.byId(id);
    if (!s || s.turns.length === 0) return;
    let start = -1;
    for (let i = 0; i < s.turns.length; i++) {
      for (const it of s.turns[i].items) {
        if (
          it.kind === "notice" &&
          (it.text.startsWith("Compactando el contexto") ||
            it.text.startsWith("Contexto compactado") ||
            it.text.startsWith("Resumen del contexto"))
        ) {
          if (start < 0 || it.text.startsWith("Compactando el contexto")) {
            start = i;
          }
          break;
        }
      }
    }
    if (start < 0) return;
    // El aviso “Compactando…” ya cumplió; quedan boundary + resumen.
    s.turns = s.turns
      .slice(start)
      .map((t) => ({
        ...t,
        items: t.items.filter(
          (it) =>
            !(
              it.kind === "notice" &&
              it.text.startsWith("Compactando el contexto")
            ),
        ),
      }))
      .filter((t) => t.items.length > 0);
  }

  /** Cambia el modelo, el esfuerzo y (si aplica) la variante rápida. */
  async setModel(
    id: string,
    model: string,
    effort?: string,
    fast?: boolean,
  ): Promise<void> {
    const session = this.byId(id);
    if (session) {
      session.model = model;
      session.effort = effort ?? null;
      session.fast = fast ?? null;
    }
    await agentSetModel(id, model, effort, fast);
  }

  /**
   * Corta el turno en curso. La sesión (historial, cwd, modelo) sigue viva.
   * El estado `working` baja cuando llega `turn.end` del backend.
   */
  async interrupt(id: string): Promise<void> {
    this.#clearWorkingTimeout(id);
    await agentInterrupt(id);
  }

  /** Cierra la sesión por completo y saca el proceso del agente. */
  async stop(id: string): Promise<void> {
    // Sacarla de la lista antes de esperar: `stop` puede tardar en matar el
    // proceso, y la UI no debería quedarse mostrando una sesión que el usuario
    // ya cerró. `#stopped` evita que un delta en vuelo la recree con `#ensure`.
    this.#stopped.add(id);
    this.#clearWorkingTimeout(id);
    this.sessions = this.sessions.filter((s) => s.id !== id);
    if (this.watching === id) this.watching = null;
    try {
      await agentStop(id);
    } finally {
      // Liberar tras un rato: el id es un UUID, no se reutiliza, pero el set
      // no tiene por qué crecer sin techo a lo largo de la sesión de la app.
      window.setTimeout(() => this.#stopped.delete(id), 30_000);
    }
  }

  byId(id: string | null): AgentSessionView | undefined {
    if (!id) return undefined;
    return this.sessions.find((s) => s.id === id);
  }

  /** Marca cuál se está mirando; la deja leída. */
  watch(id: string | null): void {
    this.watching = id;
    const session = this.byId(id);
    if (session) session.unread = 0;
  }

  /**
   * Watch ligado a la visibilidad del float.
   * Cerrado → `null` para que unread/avisos de la pill vuelvan a funcionar.
   */
  watchVisible(shown: boolean, id: string | null): void {
    this.watch(shown ? id : null);
  }

  #armWorkingTimeout(id: string): void {
    this.#clearWorkingTimeout(id);
    const t = setTimeout(() => {
      this.#workingTimers.delete(id);
      const s = this.byId(id);
      if (!s || s.status !== "working") return;
      s.status = "failed";
      s.error =
        "El agente no respondió a tiempo. Puedes reintentar o pulsar Detener.";
      const last = s.turns.at(-1);
      if (last?.status === "running") last.status = "failed";
    }, AgentSessionStore.WORKING_TIMEOUT_MS);
    this.#workingTimers.set(id, t);
  }

  #clearWorkingTimeout(id: string): void {
    const t = this.#workingTimers.get(id);
    if (t !== undefined) {
      clearTimeout(t);
      this.#workingTimers.delete(id);
    }
  }

  /** Cualquier delta de la sesión reinicia el reloj de “working”. */
  #touchWorking(id: string, status: AgentStatus): void {
    if (status === "working" || status === "waiting") {
      this.#armWorkingTimeout(id);
    } else {
      this.#clearWorkingTimeout(id);
    }
  }

  #ensure(id: string, backendId: string, backendName: string): AgentSessionView {
    const found = this.byId(id);
    if (found) return found;
    const session: AgentSessionView = {
      id,
      backendId,
      backendName,
      status: "ready",
      turns: [],
      unread: 0,
      lastText: null,
      lastTextAt: 0,
      error: null,
      pending: [],
      contextTokens: 0,
      contextSize: null,
      models: [],
      effort: null,
      fast: null,
      costUsd: 0,
      cwd: "",
      model: "",
      mode: "",
      providerSession: null,
      commands: [],
      mcpServers: [],
      tools: [],
    };
    this.sessions = [...this.sessions, session];
    return session;
  }

  /**
   * Avisa por el sistema operativo.
   *
   * Es lo que hace que valga la pena dejar al agente trabajando: sin esto
   * habría que volver a mirar la ventana cada tanto, que es exactamente el
   * trabajo que la sesión en segundo plano venía a evitar.
   */
  #notify(title: string, body: string): void {
    if (!this.#notifier) return;
    void (async () => {
      try {
        const api = await import("@tauri-apps/plugin-notification");
        let allowed = await api.isPermissionGranted();
        if (!allowed) allowed = (await api.requestPermission()) === "granted";
        if (allowed) api.sendNotification({ title, body });
      } catch (err) {
        // Sin notificaciones se sigue: el aviso de la pill ya está puesto.
        console.warn("notificar agente", err);
      }
    })();
  }

  /**
   * Aplica un delta.
   *
   * Es la misma operación que hace `Thread::apply` en Rust, y vive dos veces a
   * propósito: el store del backend reconstruye el hilo sin frontend, y la
   * vista lo aplica sin ida y vuelta. Lo que las mantiene juntas es que el
   * delta es el mismo tipo, no una copia paralela.
   */
  #receive(payload: AgentDeltaPayload): void {
    if (this.#stopped.has(payload.session)) return;
    const s = this.#ensure(
      payload.session,
      payload.backendId,
      payload.backendName,
    );
    // Nombre real en cuanto llega: `start` la crea con el id como etiqueta
    // provisional para no depender de que el backend responda.
    s.backendName = payload.backendName;

    const unseen = this.watching !== s.id;

    switch (payload.t) {
      case "turn.start":
        s.turns = [
          ...s.turns,
          { id: payload.turn, items: [], status: "running", costUsd: null },
        ];
        s.status = "working";
        break;

      case "item.add": {
        // Un item cuyo turno no existe no se descarta: se le abre uno. Perderlo
        // dejaría un hueco en la conversación, que es peor que un turno de más.
        let turn = s.turns.find((t) => t.id === payload.turn);
        if (!turn) {
          turn = { id: payload.turn, items: [], status: "running", costUsd: null };
          s.turns = [...s.turns, turn];
        }
        turn.items = [...turn.items, payload.item];

        const it = payload.item;
        if (it.kind === "permission") {
          s.pending = [
            ...s.pending,
            {
              id: it.id,
              tool: it.tool,
              description: it.description,
              input: it.input,
            },
          ];
          s.status = "waiting";
          // Un permiso siempre cuenta como no leído, incluso mirando la sesión:
          // si la ventana está detrás de otra app, «estar mirando» no es cierto,
          // y este es el evento que no se puede perder.
          s.unread += 1;
          this.#notify(
            `${s.backendName} pide permiso`,
            `${it.tool}${it.description ? `: ${it.description}` : ""}`,
          );
        } else if (it.kind === "message" && it.role === "assistant") {
          this.#rememberAssistant(s, it);
          if (!it.streaming && unseen) s.unread += 1;
        } else if (
          it.kind === "notice" &&
          it.text.startsWith("Resumen del contexto")
        ) {
          // El chat de Atic pasa a reflejar el contexto compactado.
          this.collapseAfterCompact(s.id);
          this.#setLastText(s, "Contexto compactado");
        }
        break;
      }

      case "item.chunk": {
        const it = this.#item(s, payload.item);
        if (it && (it.kind === "message" || it.kind === "reasoning")) {
          it.text += payload.text;
          this.#rememberAssistant(s, it);
        }
        break;
      }

      case "item.patch": {
        const it = this.#item(s, payload.item);
        if (!it) break;
        // `Object.assign` y no un `switch` por variante: el parche ya trae solo
        // las claves que cambian, y enumerarlas acá sería repetir el tipo.
        Object.assign(it, payload.patch);
        if (it.kind === "message" && it.role === "assistant") {
          // Lo que marca «te contestó» es que DEJE de escribirse, no que venga
          // texto en el parche: ACP acumula por trozos y al cerrar solo apaga
          // la señal, sin repetir la respuesta entera. Colgando el aviso del
          // texto, la pill se quedaba muda en OpenCode y Cursor.
          if (payload.patch.text !== undefined || payload.patch.streaming === false) {
            this.#setLastText(s, it.text);
          }
          if (unseen && payload.patch.streaming === false) s.unread += 1;
        }
        if (it.kind === "permission" && it.status !== "pending") {
          s.pending = s.pending.filter((p) => p.id !== it.id);
        }
        break;
      }

      case "thread.patch": {
        const p = payload.patch;
        if (p.providerSession !== undefined) s.providerSession = p.providerSession;
        if (p.cwd !== undefined) s.cwd = p.cwd;
        if (p.model !== undefined) s.model = p.model;
        if (p.mode !== undefined) s.mode = p.mode;
        if (p.tokens !== undefined) s.contextTokens = p.tokens;
        if (p.contextSize !== undefined) s.contextSize = p.contextSize;
        if (p.models !== undefined) s.models = p.models;
        if (p.effort !== undefined) s.effort = p.effort;
        if (p.fast !== undefined) s.fast = p.fast;
        if (p.tools !== undefined) s.tools = p.tools;
        if (p.mcpServers !== undefined) s.mcpServers = p.mcpServers;
        if (p.commands !== undefined) {
          s.commands = p.commands;
          this.catalog = { ...this.catalog, [s.backendId]: p.commands };
          writeStoredCatalog(this.catalog);
        }
        break;
      }

      case "turn.end": {
        const turn = s.turns.find((t) => t.id === payload.turn);
        if (turn) {
          turn.status = payload.status;
          turn.costUsd = payload.costUsd;
        }
        if (payload.costUsd !== null) s.costUsd += payload.costUsd;
        s.status = payload.status === "failed" ? "failed" : "ready";
        const closing = lastAssistantText(s);
        if (closing) this.#setLastText(s, closing);
        // Solo si no lo estás mirando: avisar de algo que está en pantalla es
        // ruido, y el fin de turno es el evento más frecuente de todos.
        if (unseen) {
          this.#notify(
            `${s.backendName} terminó`,
            s.lastText?.slice(0, 140) ?? "",
          );
        }
        break;
      }

      case "failed": {
        s.status = "failed";
        s.error = payload.message;
        const last = s.turns.at(-1);
        if (last?.status === "running") last.status = "failed";
        if (unseen) s.unread += 1;
        this.#notify(`${s.backendName} falló`, payload.message);
        break;
      }
    }

    this.#touchWorking(s.id, s.status);
  }

  #setLastText(s: AgentSessionView, text: string): void {
    s.lastText = text;
    s.lastTextAt = Math.floor(Date.now() / 1000);
  }

  #rememberAssistant(s: AgentSessionView, it: AgentItem): void {
    if (it.kind === "message" && it.role === "assistant" && it.text.trim()) {
      this.#setLastText(s, it.text);
    }
  }

  /**
   * Busca un item por id.
   *
   * De atrás para adelante: lo que se parchea es casi siempre lo último que
   * llegó, y una conversación larga tiene miles de items.
   */
  #item(s: AgentSessionView, id: string): AgentItem | undefined {
    for (let i = s.turns.length - 1; i >= 0; i--) {
      const items = s.turns[i].items;
      for (let j = items.length - 1; j >= 0; j--) {
        if (items[j].id === id) return items[j];
      }
    }
    return undefined;
  }

  /**
   * Contesta un permiso y devuelve la sesión a «trabajando».
   *
   * El estado se corrige acá y no al recibir el siguiente evento porque entre
   * la respuesta y el próximo mensaje puede pasar bastante: dejarla en
   * «esperando» haría parecer que el clic no hizo nada.
   */
  async decide(
    sessionId: string,
    permissionId: string,
    decision: PermissionDecision,
  ) {
    const session = this.byId(sessionId);
    if (!session) return;
    session.pending = session.pending.filter((p) => p.id !== permissionId);
    if (session.pending.length === 0) {
      session.status = "working";
      this.#armWorkingTimeout(sessionId);
    }
    await agentPermission(sessionId, permissionId, decision);
  }

  /**
   * Relee las skills del disco para una carpeta.
   *
   * No se cachean entre llamadas: son archivos que el usuario edita con su
   * editor abierto al lado, y una lista guardada sería una lista vieja justo
   * en el momento en que acaba de escribir una skill nueva y la quiere probar.
   */
  async loadSkills(cwd?: string): Promise<void> {
    try {
      this.skills = await agentSkills(cwd);
    } catch (err) {
      // Sin skills se sigue: son un atajo para descubrirlas, no la única vía
      // de invocarlas — escribir el comando a mano funciona igual.
      console.warn("leer skills", err);
    }
  }
}

export const agents = new AgentSessionStore();
