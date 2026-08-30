/**
 * Los cupos de los agentes, cacheados del lado del webview.
 *
 * Rust ya cachea un minuto, así que esto no está para ahorrar red: está para
 * que el panel tenga algo que pintar en el primer cuadro. Un hover que abre en
 * blanco y se llena medio segundo después se lee como lento aunque la consulta
 * haya sido instantánea, y el punto de esta herramienta es justamente no tener
 * que esperar nada.
 *
 * De ahí que `ensure()` no vacíe lo que ya tiene mientras recarga: se muestra
 * el dato anterior, y cuando llega el nuevo se reemplaza sin parpadeo.
 */
import { agentQuotaOverview } from "$ipc/agents";
import type { QuotaOverview } from "$core/types";

/** Cuánto vale el snapshot local antes de volver a pedirlo. */
const FRESH_MS = 45_000;

class AgentQuotas {
  overview = $state<QuotaOverview | null>(null);
  /** Solo para el primer llenado: después se recarga en silencio. */
  loading = $state(false);
  /** Falla del comando entero, no de un agente suelto (eso va en su fila). */
  error = $state<string | null>(null);

  #at = 0;
  #inflight: Promise<void> | null = null;

  /**
   * Trae los cupos si hace falta. Varias llamadas seguidas comparten la misma
   * consulta: el puntero entrando y saliendo del disco no puede disparar una
   * ráfaga de comandos.
   */
  ensure(force = false): Promise<void> {
    if (!force && this.overview && Date.now() - this.#at < FRESH_MS) {
      return Promise.resolve();
    }
    if (this.#inflight) return this.#inflight;

    this.loading = this.overview === null;
    this.#inflight = agentQuotaOverview(force)
      .then((next) => {
        this.overview = next;
        this.error = null;
        this.#at = Date.now();
      })
      .catch((err: unknown) => {
        this.error = err instanceof Error ? err.message : String(err);
      })
      .finally(() => {
        this.loading = false;
        this.#inflight = null;
      });
    return this.#inflight;
  }
}

export const agentQuotas = new AgentQuotas();
