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
  agentPermission,
  agentSend,
  agentSessions,
  agentStart,
  agentStop,
  onAgentEvent,
} from "$lib/api";
import type {
  AgentEventPayload,
  AgentStartOptions,
  McpServerState,
} from "$lib/types";

/** Un permiso pendiente: el agente está detenido esperando la respuesta. */
export interface PendingPermission {
  id: string;
  tool: string;
  description: string;
  input: unknown;
}

/**
 * En qué anda una sesión.
 *
 * `working` no lo dice el backend: se deduce de haber mandado algo y no haber
 * visto el fin del turno. Es lo que permite mostrar «pensando» sin preguntar.
 */
export type AgentStatus = "ready" | "working" | "waiting" | "failed";

export interface AgentSessionView {
  id: string;
  backendId: string;
  backendName: string;
  status: AgentStatus;
  /** Todo lo recibido, en orden. */
  log: AgentEventPayload[];
  /** Respuestas llegadas mientras nadie miraba esta sesión. */
  unread: number;
  /** Última línea del agente: el resumen que cabe en la pill. */
  lastText: string | null;
  error: string | null;
  /** Permisos esperando respuesta. Mientras haya uno, el agente no avanza. */
  pending: PendingPermission[];
  /** Contexto consumido, en tokens. */
  contextTokens: number;
  /** Costo acumulado de la sesión. */
  costUsd: number;
  cwd: string;
  model: string;
  slashCommands: string[];
  mcpServers: McpServerState[];
}

class AgentSessionStore {
  sessions = $state<AgentSessionView[]>([]);
  /** Sesión que se está mirando ahora mismo; sus respuestas no son «nuevas». */
  watching = $state<string | null>(null);

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

  /**
   * Empieza a escuchar y adopta lo que ya estuviera corriendo. Idempotente:
   * cualquier vista puede llamarlo al montar sin coordinarse con las demás.
   */
  async init({ notify = false } = {}): Promise<void> {
    if (notify) this.#notifier = true;
    if (this.#started) return;
    this.#started = true;
    this.#unlisten = onAgentEvent((payload) => this.#receive(payload));
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
    // El primer evento la crearía igual; crearla acá evita el hueco en el que
    // la UI ya tiene el id pero todavía no tiene nada que mostrar.
    this.#ensure(id, backendId, backendId);
    return id;
  }

  async send(id: string, text: string): Promise<void> {
    const session = this.byId(id);
    if (session) {
      session.status = "working";
      session.error = null;
    }
    try {
      await agentSend(id, text);
    } catch (err) {
      if (session) {
        session.status = "failed";
        session.error = String(err);
      }
      throw err;
    }
  }

  async stop(id: string): Promise<void> {
    // Sacarla de la lista antes de esperar: `stop` bloquea hasta que el proceso
    // vacía sus buffers, y la UI no debería quedarse mostrando una sesión que
    // el usuario ya cerró.
    this.sessions = this.sessions.filter((s) => s.id !== id);
    if (this.watching === id) this.watching = null;
    await agentStop(id);
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

  #ensure(id: string, backendId: string, backendName: string): AgentSessionView {
    const found = this.byId(id);
    if (found) return found;
    const session: AgentSessionView = {
      id,
      backendId,
      backendName,
      status: "ready",
      log: [],
      unread: 0,
      lastText: null,
      error: null,
      pending: [],
      contextTokens: 0,
      costUsd: 0,
      cwd: "",
      model: "",
      slashCommands: [],
      mcpServers: [],
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

  #receive(payload: AgentEventPayload): void {
    const session = this.#ensure(
      payload.session,
      payload.backendId,
      payload.backendName,
    );
    // Nombre real en cuanto llega: `start` la crea con el id como etiqueta
    // provisional para no depender de que el backend responda.
    session.backendName = payload.backendName;
    session.log = [...session.log, payload];

    const unseen = this.watching !== session.id;
    switch (payload.kind) {
      case "started":
        session.cwd = payload.cwd;
        session.model = payload.model;
        session.slashCommands = payload.slashCommands;
        session.mcpServers = payload.mcpServers;
        break;
      case "message":
        session.lastText = payload.text;
        if (unseen) session.unread += 1;
        break;
      case "context":
        session.contextTokens = payload.tokens;
        break;
      case "permission":
        session.pending = [
          ...session.pending,
          {
            id: payload.id,
            tool: payload.tool,
            description: payload.description,
            input: payload.input,
          },
        ];
        session.status = "waiting";
        // Un permiso siempre cuenta como no leído, incluso mirando la sesión:
        // si la ventana está detrás de otra app, «estar mirando» no es cierto,
        // y este es el evento que no se puede perder.
        session.unread += 1;
        this.#notify(
          `${session.backendName} pide permiso`,
          `${payload.tool}${payload.description ? `: ${payload.description}` : ""}`,
        );
        break;
      case "finished":
        session.status = payload.isError ? "failed" : "ready";
        if (payload.costUsd !== null) session.costUsd += payload.costUsd;
        // Solo si no lo estás mirando: avisar de algo que está en pantalla es
        // ruido, y el fin de turno es el evento más frecuente de todos.
        if (unseen) {
          this.#notify(
            `${session.backendName} terminó`,
            session.lastText?.slice(0, 140) ?? "",
          );
        }
        break;
      case "failed":
        session.status = "failed";
        session.error = payload.message;
        if (unseen) session.unread += 1;
        this.#notify(`${session.backendName} falló`, payload.message);
        break;
      default:
        break;
    }
  }

  /**
   * Contesta un permiso y devuelve la sesión a «trabajando».
   *
   * El estado se corrige acá y no al recibir el siguiente evento porque entre
   * la respuesta y el próximo mensaje puede pasar bastante: dejarla en
   * «esperando» haría parecer que el clic no hizo nada.
   */
  async decide(sessionId: string, permissionId: string, allow: boolean) {
    const session = this.byId(sessionId);
    if (!session) return;
    session.pending = session.pending.filter((p) => p.id !== permissionId);
    if (session.pending.length === 0) session.status = "working";
    await agentPermission(sessionId, permissionId, allow);
  }
}

export const agents = new AgentSessionStore();
