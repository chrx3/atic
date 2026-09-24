/**
 * Cómo se lee un hilo de chat: de turnos con items a bloques para dibujar.
 *
 * El agente emite una herramienta por item, y un turno normal son diez o
 * veinte. Dibujadas una debajo de otra tapan lo que el agente dice. Acá las
 * seguidas se juntan en un bloque de «actividad» que se lee de un vistazo
 * («leyó 3, editó 2») y se abre solo si hace falta.
 */
import type {
  AgentItem,
  AgentModel,
  AgentTurn,
  ToolKind,
  TurnStatus,
} from "$lib/types";
import { editDiff } from "$core/agentMarkdown";
import { isChatStatusNoise } from "./chatNotifications";

type Of<K extends AgentItem["kind"]> = Extract<AgentItem, { kind: K }>;

/** Lo que va dentro de un bloque de actividad. */
export type ActivityItem = Of<"tool"> | Of<"collab"> | Of<"reasoning">;

export type ChatBlock =
  | { kind: "user"; item: Of<"message"> }
  | { kind: "text"; item: Of<"message"> }
  | {
      kind: "activity";
      /** El id del primer item: estable mientras el bloque crece. */
      id: string;
      items: ActivityItem[];
      /** Sigue pasando: es el tramo en curso del turno vivo. */
      live: boolean;
    }
  | { kind: "plan"; item: Of<"plan"> }
  | { kind: "notice"; item: Of<"notice"> }
  | {
      /** Lo que un turno terminado hizo antes de su respuesta, plegado. */
      kind: "work";
      id: string;
      durationMs: number;
      status: TurnStatus;
      costUsd: number | null;
      blocks: ChatBlock[];
      /** Lo que cambió en todo el tramo: se ve aunque esté plegado. */
      files: EditedFile[];
    };

function isActivity(item: AgentItem): item is ActivityItem {
  return item.kind === "tool" || item.kind === "collab" || item.kind === "reasoning";
}

/**
 * Turnos → bloques. Los permisos no se dibujan acá: el pendiente tiene su
 * tarjeta propia y uno ya contestado no dice nada que la herramienta no diga.
 */
export function toBlocks(turns: AgentTurn[]): ChatBlock[] {
  const blocks: ChatBlock[] = [];
  for (const turn of turns) {
    blocks.push(...foldTurn(turn, turnBlocks(turn)));
  }
  return blocks;
}

function turnBlocks(turn: AgentTurn): ChatBlock[] {
  const blocks: ChatBlock[] = [];
  let activity: Extract<ChatBlock, { kind: "activity" }> | null = null;
  for (const item of turn.items) {
    if (isChatStatusNoise(item)) continue;
    if (isActivity(item)) {
      if (!activity) {
        activity = { kind: "activity", id: item.id, items: [], live: false };
        blocks.push(activity);
      }
      activity.items.push(item);
      continue;
    }
    if (item.kind === "permission") continue;
    activity = null;
    if (item.kind === "message") {
      blocks.push({ kind: item.role === "user" ? "user" : "text", item });
    } else if (item.kind === "plan") {
      blocks.push({ kind: "plan", item });
    } else {
      blocks.push({ kind: "notice", item });
    }
  }
  // Solo el último tramo de un turno vivo está pasando: los de antes ya
  // terminaron aunque el turno siga.
  if (activity && turn.status === "running") activity.live = true;
  return blocks;
}

/**
 * Un turno terminado se lee como «pregunta → cuánto trabajó → respuesta».
 * Lo de en medio (herramientas y textos de paso) se pliega en un bloque
 * `work`; la respuesta final queda a la vista. Sin duración medida (hilos
 * viejos) o sin herramientas, el turno se deja como está.
 */
function foldTurn(turn: AgentTurn, blocks: ChatBlock[]): ChatBlock[] {
  if (turn.status === "running" || turn.durationMs === undefined) return blocks;
  let start = 0;
  while (start < blocks.length && blocks[start].kind === "user") start += 1;
  // La respuesta es el último texto, si después no quedó trabajo colgando.
  let end = blocks.length;
  for (let i = blocks.length - 1; i >= start; i -= 1) {
    if (blocks[i].kind === "activity") break;
    if (blocks[i].kind === "text") {
      end = i;
      break;
    }
  }
  const inner = blocks.slice(start, end);
  const activity = inner.flatMap((b) => (b.kind === "activity" ? b.items : []));
  if (activity.length === 0) return blocks;
  return [
    ...blocks.slice(0, start),
    {
      kind: "work",
      id: `work-${turn.id}`,
      durationMs: turn.durationMs,
      status: turn.status,
      costUsd: turn.costUsd,
      blocks: inner,
      files: editedFiles(activity),
    },
    ...blocks.slice(end),
  ];
}

/** «33s», «1m 12s», «1h 5m»: lo justo para saber cuánto tardó. */
export function formatDuration(ms: number): string {
  const seconds = Math.max(0, Math.round(ms / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ${seconds % 60}s`;
  return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}

/** Cuántas acciones de cada clase hubo, para el resumen del bloque. */
export type ActivityCounts = {
  read: number;
  edit: number;
  run: number;
  search: number;
  agent: number;
  other: number;
  failed: number;
  thought: boolean;
};

const KIND_GROUP: Partial<Record<ToolKind, keyof ActivityCounts>> = {
  read: "read",
  edit: "edit",
  delete: "edit",
  move: "edit",
  execute: "run",
  search: "search",
  fetch: "search",
};

export function countActivity(items: ActivityItem[]): ActivityCounts {
  const counts: ActivityCounts = {
    read: 0,
    edit: 0,
    run: 0,
    search: 0,
    agent: 0,
    other: 0,
    failed: 0,
    thought: false,
  };
  for (const item of items) {
    if (item.kind === "reasoning") {
      counts.thought = true;
      continue;
    }
    if (item.status === "failed") counts.failed += 1;
    if (item.kind === "collab") {
      counts.agent += 1;
      continue;
    }
    const group = KIND_GROUP[item.toolKind] ?? "other";
    (counts[group] as number) += 1;
  }
  return counts;
}

/** Las pestañas del bloque de actividad desplegado. */
export type ActivityTab = "all" | "thought" | "read" | "edit" | "run" | "other";

export function activityTabOf(item: ActivityItem): Exclude<ActivityTab, "all"> {
  if (item.kind === "reasoning") return "thought";
  if (item.kind === "collab") return "other";
  const group = KIND_GROUP[item.toolKind];
  if (group === "read" || group === "search") return "read";
  if (group === "edit") return "edit";
  if (group === "run") return "run";
  return "other";
}

/**
 * Las pestañas que tienen algo, con cuántos. Con una sola clase no hay nada
 * que separar: se devuelve vacío y el bloque se muestra sin pestañas.
 */
export function activityTabs(
  items: ActivityItem[],
): { id: ActivityTab; count: number }[] {
  const order: Exclude<ActivityTab, "all">[] = [
    "thought",
    "read",
    "edit",
    "run",
    "other",
  ];
  const counts = new Map<ActivityTab, number>();
  for (const item of items) {
    const tab = activityTabOf(item);
    counts.set(tab, (counts.get(tab) ?? 0) + 1);
  }
  const present = order.filter((tab) => counts.has(tab));
  if (present.length < 2) return [];
  return [
    { id: "all", count: items.length },
    ...present.map((id) => ({ id, count: counts.get(id) ?? 0 })),
  ];
}

/** Un archivo que el tramo cambió, con cuántas líneas entraron y salieron. */
export type EditedFile = { path: string; add: number; del: number };

function editedPath(item: Of<"tool">): string | null {
  if (item.locations[0]) return item.locations[0];
  const input = item.input;
  if (!input || typeof input !== "object") return null;
  const o = input as Record<string, unknown>;
  for (const key of ["file_path", "path", "notebook_path"]) {
    if (typeof o[key] === "string" && o[key]) return o[key];
  }
  return null;
}

/**
 * Los archivos que cambió un tramo de actividad, uno por archivo aunque se
 * haya editado varias veces, en el orden en que se tocaron. Lo que falló no
 * cambió nada y no cuenta.
 */
export function editedFiles(items: ActivityItem[]): EditedFile[] {
  const byPath = new Map<string, EditedFile>();
  for (const item of items) {
    if (item.kind !== "tool" || item.status === "failed") continue;
    if (KIND_GROUP[item.toolKind] !== "edit") continue;
    const path = editedPath(item);
    if (!path) continue;
    const file = byPath.get(path) ?? { path, add: 0, del: 0 };
    for (const line of editDiff(item.input) ?? []) {
      if (line.sign === "+") file.add += 1;
      else if (line.sign === "-") file.del += 1;
    }
    byPath.set(path, file);
  }
  return [...byPath.values()];
}

/**
 * El nombre de un modelo, sin el proveedor delante.
 *
 * OpenCode los nombra `proveedor/modelo` (`opencode-go/Space Bunny Fast`):
 * en el botón del composer lo que distingue es lo de después de la barra.
 */
export function shortModelName(name: string): string {
  const trimmed = name.trim();
  const tail = trimmed.slice(trimmed.lastIndexOf("/") + 1).trim();
  return tail || trimmed;
}

/** El proveedor de un id `proveedor/modelo`, o "" si no lo trae. */
export function modelProvider(id: string): string {
  const slash = id.lastIndexOf("/");
  return slash > 0 ? id.slice(0, slash) : "";
}

/**
 * Modelos agrupados por proveedor, en el orden en que llegaron, con el
 * proveedor del modelo en uso primero.
 */
export function modelGroups<T extends { id: string }>(
  models: T[],
  currentId: string,
): { provider: string; models: T[] }[] {
  const byProvider = new Map<string, T[]>();
  for (const model of models) {
    const provider = modelProvider(model.id);
    const list = byProvider.get(provider);
    if (list) list.push(model);
    else byProvider.set(provider, [model]);
  }
  const first = modelProvider(currentId);
  return [...byProvider.entries()]
    .map(([provider, list]) => ({ provider, models: list }))
    .sort((a, b) => Number(b.provider === first) - Number(a.provider === first));
}

/**
 * El modelo de la lista que corresponde al id en uso.
 *
 * Claude Code se lista por alias (`opus`) y la sesión informa el id resuelto
 * (`claude-opus-5-5`): sin esto el selector mostraba el id crudo. Gana el
 * exacto; si no, el alias más largo contenido en el id.
 */
export function matchModel(models: AgentModel[], id: string | null | undefined) {
  if (!id) return undefined;
  const exact = models.find((m) => m.id === id);
  if (exact) return exact;
  const lower = id.toLowerCase();
  return [...models]
    .sort((a, b) => b.id.length - a.id.length)
    .find((m) => m.id.length > 2 && lower.includes(m.id.toLowerCase()));
}
