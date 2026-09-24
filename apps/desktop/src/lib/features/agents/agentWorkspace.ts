/**
 * Las sesiones de la ventana de agentes, como se guardan.
 *
 * Chats y terminales en una sola lista: lo que el usuario ve a la izquierda
 * es «mis agentes», no «mis consolas» por un lado y «mis chats» por otro. La
 * sesión vive en Rust y sobrevive a recargar la ventana; la lista se guarda
 * para volver a mostrarla.
 */
import { parseRect, type Rect } from "./agentBoard";
import type { ChatTabStatus } from "./chatStatus";

export type WorkspaceItem =
  | {
      key: string;
      kind: "chat";
      /** Sesión estructurada (`agents`). */
      session: string;
      label: string;
      /** CLI del agente (`claude`, `codex`…), para el logo. */
      cli: string | null;
    }
  | {
      key: string;
      kind: "terminal";
      /** PTY viva; `null` mientras se abre. */
      session: string | null;
      label: string;
      cli: string | null;
      /** Con qué se abrió: `null` = la shell del sistema. */
      command: string | null;
      cwd: string | null;
      /** Dónde está en la pizarra; sin esto, la pizarra le busca lugar. */
      rect?: Rect;
    };

/** Lo que la barra lateral dice de cada sesión. */
export type ItemStatus = ChatTabStatus | "terminal" | "ended";

export type WorkspaceState = { items: WorkspaceItem[]; active: string | null };

export const WORKSPACE_KEY = "atic.agents.workspace";

const text = (v: unknown) => (typeof v === "string" ? v : null);

function rectOf(value: unknown): { rect?: Rect } {
  const rect = parseRect(value);
  return rect ? { rect } : {};
}

/** Lo guardado, o vacío si falta o está roto: retomar es un extra. */
export function parseWorkspace(raw: string | null): WorkspaceState {
  const empty: WorkspaceState = { items: [], active: null };
  if (!raw) return empty;
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    return empty;
  }
  if (!value || typeof value !== "object") return empty;
  const { items, active } = value as Record<string, unknown>;
  if (!Array.isArray(items)) return empty;
  const parsed = items.flatMap((item): WorkspaceItem[] => {
    if (!item || typeof item !== "object") return [];
    const o = item as Record<string, unknown>;
    const key = text(o.key);
    const session = text(o.session);
    const label = text(o.label) ?? "";
    const cli = text(o.cli);
    if (!key) return [];
    if (o.kind === "chat" && session)
      return [{ key, kind: "chat", session, label, cli }];
    // Una terminal sin sesión no se abrió nunca: no hay nada que retomar.
    if (o.kind === "terminal" && session)
      return [
        {
          key,
          kind: "terminal",
          session,
          label,
          cli,
          command: text(o.command),
          cwd: text(o.cwd),
          ...rectOf(o.rect),
        },
      ];
    return [];
  });
  const activeKey = text(active);
  return {
    items: parsed,
    active: parsed.some((i) => i.key === activeKey)
      ? activeKey
      : (parsed[0]?.key ?? null),
  };
}

/** El elegido después de cerrar `closed`: el de al lado, como en un navegador. */
export function neighbourAfterClose(
  items: WorkspaceItem[],
  closed: string,
  active: string | null,
): string | null {
  if (active !== closed) return active;
  const index = items.findIndex((i) => i.key === closed);
  const rest = items.filter((i) => i.key !== closed);
  if (rest.length === 0) return null;
  return rest[Math.min(index, rest.length - 1)].key;
}

/** En qué tramo de la lista de recientes cae una fecha. */
export type DateBucket = "today" | "yesterday" | "thisWeek" | "older";

const BUCKETS: DateBucket[] = ["today", "yesterday", "thisWeek", "older"];

/** Por días de calendario locales, no por horas: anoche a las 23:50 es «ayer». */
export function dateBucket(epochSeconds: number, now: Date = new Date()): DateBucket {
  const startOfToday = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const then = new Date(epochSeconds * 1000);
  const startOfThen = new Date(then.getFullYear(), then.getMonth(), then.getDate());
  const days = Math.round(
    (startOfToday.getTime() - startOfThen.getTime()) / 86_400_000,
  );
  if (days <= 0) return "today";
  if (days === 1) return "yesterday";
  if (days < 7) return "thisWeek";
  return "older";
}

/** Agrupados por tramo, en orden, sin tramos vacíos. Respeta el orden de entrada. */
export function groupByDate<T extends { updatedAt: number }>(
  items: T[],
  now: Date = new Date(),
): { bucket: DateBucket; items: T[] }[] {
  const groups = new Map<DateBucket, T[]>();
  for (const item of items) {
    const bucket = dateBucket(item.updatedAt, now);
    groups.set(bucket, [...(groups.get(bucket) ?? []), item]);
  }
  return BUCKETS.filter((b) => groups.has(b)).map((bucket) => ({
    bucket,
    items: groups.get(bucket) ?? [],
  }));
}
