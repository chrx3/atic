/**
 * Mudanza de consolas vivas entre ventanas (float del overlay ⇄ principal).
 *
 * Las PTY viven en Rust y cualquier webview puede operarlas por id; lo que se
 * mueve acá es la *vista*: descriptores de pestaña + forma de la división.
 * El scrollback viaja por `console_tail`, no en este JSON.
 *
 * Dos webviews = dos contextos JS: este módulo NO comparte estado entre
 * ventanas (el overlay y la principal tienen perfiles distintos). El buzón es
 * por webview; el transporte es el evento `agents-transfer` vía Rust.
 */

import type { ConsoleKind } from "$core/types";

export type TransferTabDescriptor = {
  /** PTY viva, o null si es ficha del hub (sin terminal). */
  session: string | null;
  kind: ConsoleKind;
  hostId: string | null;
  label: string | null;
  command: string | null;
  cwd: string | null;
  hubSession: string | null;
};

/** División con hojas nombradas por sesión (`hub:<id>` para fichas del hub). */
export type SessionTree =
  | { kind: "leaf"; session: string }
  | {
      kind: "split";
      direction: "right" | "down";
      ratio?: number;
      first: SessionTree;
      second: SessionTree;
    };

/** Lo mismo, con hojas nombradas por clave local de pestaña. */
export type KeyTree =
  | { kind: "leaf"; key: string }
  | {
      kind: "split";
      direction: "right" | "down";
      ratio?: number;
      first: KeyTree;
      second: KeyTree;
    };

export type TransferBody = {
  tabs: TransferTabDescriptor[];
  /** Null = sin división que conservar (una ficha o nada visible). */
  tree: SessionTree | null;
  /** Sesión de la pestaña activa, o null. */
  activeSession: string | null;
  /** PTYs que viajan (para la protección `begin/end_transfer`). */
  sessions: string[];
};

export type TransferPayload = TransferBody & {
  transferId: string;
  /** Etiqueta de ventana (`main` | `overlay`). */
  from: string;
  to: string;
  /**
   * Traspaso coreografiado por la pill (detach/retach intra-overlay): la
   * receptora no avisa con toast, la emisora ya confirmó.
   */
  quiet?: boolean;
};

/** Etiqueta de la ventana dedicada de consolas. Debe coincidir con Rust. */
export const AGENTS_WINDOW_LABEL = "agents";

export type TransferAck = {
  transferId: string;
  /** Sesiones que la receptora adoptó de verdad. */
  adopted: string[];
};

export function treeToSessions(
  node: KeyTree | null,
  sessionOf: (key: string) => string | null,
): SessionTree | null {
  if (!node) return null;
  if (node.kind === "leaf") {
    const session = sessionOf(node.key);
    return session ? { kind: "leaf", session } : null;
  }
  const first = treeToSessions(node.first, sessionOf);
  const second = treeToSessions(node.second, sessionOf);
  if (first && second) {
    return {
      kind: "split",
      direction: node.direction,
      ratio: node.ratio,
      first,
      second,
    };
  }
  return first ?? second;
}

/** Hojas por sesión → hojas por clave; lo no adoptado se poda. */
export function treeToKeys(
  node: SessionTree | null,
  keyOf: (session: string) => string | null,
): KeyTree | null {
  if (!node) return null;
  if (node.kind === "leaf") {
    const key = keyOf(node.session);
    return key ? { kind: "leaf", key } : null;
  }
  const first = treeToKeys(node.first, keyOf);
  const second = treeToKeys(node.second, keyOf);
  if (first && second) {
    return {
      kind: "split",
      direction: node.direction,
      ratio: node.ratio,
      first,
      second,
    };
  }
  return first ?? second;
}
