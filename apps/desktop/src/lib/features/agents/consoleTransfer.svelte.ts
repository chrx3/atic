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

/** Hojas por clave → hojas por sesión; las hojas sin sesión se podan. */
