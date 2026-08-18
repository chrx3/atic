/**
 * Presencias de TUI, vistas desde el frontend.
 *
 * El snapshot lo tiene Rust. Este módulo solo guarda la lista y el `unread`
 * de quien mira: dos ventanas de Atic pueden tener contadores distintos.
 */
import { agentPresences, onAgentPresence } from "$lib/api";
import type { AgentPresence } from "$core/types";
import {
  applyPresenceSnapshot,
  markPresenceSeen,
  type PresenceView,
} from "./agentPresenceReduce";

class AgentPresenceStore {
  list = $state<AgentPresence[]>([]);
  unread = $state<Record<string, number>>({});
  watching = $state(false);

  #started = false;
  #unlisten: Promise<() => void> | null = null;

  get view(): PresenceView[] {
    return this.list.map((p) => ({ ...p, unread: this.unread[p.id] ?? 0 }));
  }

  applySnapshot(snapshot: AgentPresence[]): void {
    const next = applyPresenceSnapshot(
      { list: this.list, unread: this.unread, watching: this.watching },
      snapshot,
    );
    this.list = next.list;
    this.unread = next.unread;
  }

  markSeen(id: string): void {
    this.unread = markPresenceSeen(this.unread, id);
  }

  async init(): Promise<void> {
    if (this.#started) return;
    this.#started = true;
    this.#unlisten = onAgentPresence((payload) => this.applySnapshot(payload));
    try {
      this.applySnapshot(await agentPresences());
    } catch (err) {
      console.warn("adoptar presencias de agente", err);
    }
  }

  async dispose(): Promise<void> {
    const un = this.#unlisten;
    this.#started = false;
    this.#unlisten = null;
    if (un) (await un)();
  }
}

export const presence = new AgentPresenceStore();
