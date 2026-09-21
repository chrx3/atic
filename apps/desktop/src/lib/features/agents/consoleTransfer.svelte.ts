/**
 * Buzón de traspasos por webview.
 *
 * Dos webviews = dos contextos JS: este estado NO cruza ventanas (el overlay
 * y la principal tienen perfiles distintos). Quien escucha el evento ofrece,
 * el lanzador consume. Lo puro (tipos + árboles) vive en `consoleTransfer`.
 */
import type { TransferPayload } from "./consoleTransfer";

export type {
  SessionTree,
  KeyTree,
  TransferBody,
  TransferPayload,
  TransferTabDescriptor,
  TransferAck,
} from "./consoleTransfer";

export { AGENTS_WINDOW_LABEL } from "./consoleTransfer";

/** Buzón por webview: quien escucha el evento ofrece, el lanzador consume. */
class TransferInbox {
  current = $state<TransferPayload | null>(null);

  offer(payload: TransferPayload): void {
    this.current = payload;
  }

  take(): TransferPayload | null {
    const payload = this.current;
    this.current = null;
    return payload;
  }
}

export const transferInbox = new TransferInbox();

/**
 * Traspaso intra-overlay (isla ⇄ float): las dos instancias del lanzador
 * viven en el MISMO webview, así que el buzón compartido no alcanza — las
 * dos lo mirarían y la receptora sería una carrera. Este bus lleva destino
 * explícito y cada instancia solo toma lo suyo.
 *
 * Y como las dos están en el mismo contexto JS, la confirmación **no da la
 * vuelta por Rust**: la receptora resuelve la promesa de la emisora en el
 * acto. Antes esto viajaba como evento `agents-transfer-ack`, con marca de
 * traspaso en Rust y espera de 4 s; ese camino queda solo para la ventana
 * dedicada, que sí es otro webview.
 */
export type OverlayTransferRole = "island" | "float";

export type OverlayHandoff = {
  to: OverlayTransferRole;
  payload: TransferPayload;
  /** La receptora contesta con lo que adoptó. */
  done: (adopted: string[]) => void;
};

/**
 * Si nadie toma la oferta, la emisora se queda esperando para siempre y la
 * consola queda inservible. Esto NO protege PTYs (de eso se encarga el
 * registro de vistas en Rust): solo destraba la UI.
 */
const HANDOFF_TIMEOUT_MS = 2000;

class OverlayTransferBus {
  current = $state<OverlayHandoff | null>(null);

  /** Ofrece y espera la adopción. Lista vacía = nadie la tomó. */
  offer(to: OverlayTransferRole, payload: TransferPayload): Promise<string[]> {
    return new Promise<string[]>((resolve) => {
      let listo = false;
      const done = (adopted: string[]) => {
        if (listo) return;
        listo = true;
        window.clearTimeout(timer);
        resolve(adopted);
      };
      const timer = window.setTimeout(() => {
        if (this.current?.done === done) this.current = null;
        done([]);
      }, HANDOFF_TIMEOUT_MS);
      this.current = { to, payload, done };
    });
  }

  take(): OverlayHandoff | null {
    const pending = this.current;
    this.current = null;
    return pending;
  }
}

export const overlayTransferBus = new OverlayTransferBus();

/** La pill pide al lanzador que MUDE sus consolas a la otra instancia. */
export const AGENTS_OVERLAY_DETACH = "agents-overlay-detach";
export type AgentsOverlayDetachDetail = {
  from: OverlayTransferRole;
  to: OverlayTransferRole;
};

/** La mudanza terminó (adoptadas > 0): la pill cierra/abre superficies. */
export const AGENTS_OVERLAY_DETACHED = "agents-overlay-detached";
export type AgentsOverlayDetachedDetail = {
  to: OverlayTransferRole;
  /** Retach sin sesiones: cerrar el float vacío y abrir la cara igual. */
  empty?: boolean;
};

/** Hojas por clave → hojas por sesión; las hojas sin sesión se podan. */
