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
  agentSend,
  agentSessions,
  agentStart,
  agentStop,
  onAgentEvent,
} from "$lib/api";
import type { AgentEventPayload } from "$lib/types";

/**
 * En qué anda una sesión.
 *
 * `working` no lo dice el backend: se deduce de haber mandado algo y no haber
 * visto el fin del turno. Es lo que permite mostrar «pensando» sin preguntar.
 */
export type AgentStatus = "ready" | "working" | "failed";

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
}

class AgentSessionStore {
  sessions = $state<AgentSessionView[]>([]);
  /** Sesión que se está mirando ahora mismo; sus respuestas no son «nuevas». */
  watching = $state<string | null>(null);

  #started = false;
  #unlisten: Promise<() => void> | null = null;

  /** Hay algo pendiente de leer: lo que enciende el aviso en la pill. */
  get unread(): number {
    return this.sessions.reduce((total, s) => total + s.unread, 0);
  }

  /** Algún agente está trabajando ahora mismo. */
  get working(): boolean {
    return this.sessions.some((s) => s.status === "working");
  }

  /**
   * Empieza a escuchar y adopta lo que ya estuviera corriendo. Idempotente:
   * cualquier vista puede llamarlo al montar sin coordinarse con las demás.
   */
  async init(): Promise<void> {
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

  async start(backendId: string): Promise<string> {
    const id = await agentStart(backendId);
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
    };
    this.sessions = [...this.sessions, session];
    return session;
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
      case "message":
        session.lastText = payload.text;
        if (unseen) session.unread += 1;
        break;
      case "finished":
        session.status = payload.isError ? "failed" : "ready";
        break;
      case "failed":
        session.status = "failed";
        session.error = payload.message;
        if (unseen) session.unread += 1;
        break;
      default:
        break;
    }
  }
}

export const agents = new AgentSessionStore();
