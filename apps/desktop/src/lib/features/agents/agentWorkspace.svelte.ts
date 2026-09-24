/**
 * Las sesiones que muestra la ventana de agentes, y cuál está elegida.
 *
 * Solo el orden y la elección: el estado de cada chat vive en el store de
 * sesiones (`agents`) y el de cada terminal en Rust. Se guarda en el storage
 * de la ventana para retomarla tras recargar.
 */
import {
  WORKSPACE_KEY,
  neighbourAfterClose,
  parseWorkspace,
  type WorkspaceItem,
} from "./agentWorkspace";

type NewItem =
  | Omit<Extract<WorkspaceItem, { kind: "chat" }>, "key">
  | Omit<Extract<WorkspaceItem, { kind: "terminal" }>, "key">;

class AgentWorkspace {
  items = $state<WorkspaceItem[]>([]);
  active = $state<string | null>(null);
  #seq = 0;

  /** Lo guardado, sin comprobar si sigue vivo: eso lo hace quien monta. */
  load(): WorkspaceItem[] {
    let raw: string | null = null;
    try {
      raw = localStorage.getItem(WORKSPACE_KEY);
    } catch {
      /* sin storage se arranca vacío */
    }
    const state = parseWorkspace(raw);
    this.items = state.items;
    this.active = state.active;
    this.#seq = state.items.length;
    return state.items;
  }

  save(): void {
    try {
      localStorage.setItem(
        WORKSPACE_KEY,
        JSON.stringify({ items: this.items, active: this.active }),
      );
    } catch {
      /* se sigue trabajando; solo no se retoma */
    }
  }

  get current(): WorkspaceItem | null {
    return this.items.find((i) => i.key === this.active) ?? null;
  }

  add(item: NewItem, select = true): string {
    const key = `w${Date.now().toString(36)}${++this.#seq}`;
    this.items = [...this.items, { ...item, key }];
    if (select) this.active = key;
    this.save();
    return key;
  }

  update(key: string, patch: Partial<WorkspaceItem>): void {
    this.items = this.items.map((i) =>
      i.key === key ? ({ ...i, ...patch } as WorkspaceItem) : i,
    );
    this.save();
  }

  remove(key: string): void {
    this.active = neighbourAfterClose(this.items, key, this.active);
    this.items = this.items.filter((i) => i.key !== key);
    this.save();
  }

  /** Saca las que ya no existen, sin tocar la elección si sigue en pie. */
  keep(alive: (item: WorkspaceItem) => boolean): void {
    const next = this.items.filter(alive);
    if (next.length === this.items.length) return;
    this.items = next;
    if (!next.some((i) => i.key === this.active)) this.active = next[0]?.key ?? null;
    this.save();
  }

  select(key: string): void {
    if (this.active === key) return;
    this.active = key;
    this.save();
  }

  findSession(kind: WorkspaceItem["kind"], session: string): WorkspaceItem | undefined {
    return this.items.find((i) => i.kind === kind && i.session === session);
  }
}

export const workspace = new AgentWorkspace();
