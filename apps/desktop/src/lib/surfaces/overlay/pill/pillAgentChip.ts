import { clipChipPreview } from "$core/agentChipPreview";
import type { PresenceView } from "$lib/agentPresenceReduce";

export type ChipTone = "waiting" | "working" | "ready" | "count" | "off";
export type ChipTarget =
  | { kind: "console" }
  | { kind: "focus"; presenceId: string }
  | { kind: "none"; presenceId?: string };

export type AgentChip = {
  tone: ChipTone;
  label: string | null;
  target: ChipTarget;
  logoId: string | null;
};

const OFF: AgentChip = {
  tone: "off",
  label: null,
  target: { kind: "none" },
  logoId: null,
};

function rank(tone: ChipTone): number {
  switch (tone) {
    case "waiting":
      return 3;
    case "working":
    case "count":
      return 2;
    case "ready":
      return 1;
    default:
      return 0;
  }
}

function better(
  next: AgentChip,
  nextAt: number,
  best: AgentChip,
  bestAt: number,
): boolean {
  const nr = rank(next.tone);
  const br = rank(best.tone);
  if (nr !== br) return nr > br;
  return nr > 0 && nextAt > bestAt;
}

function fromChat(chat: {
  unread: number;
  working: boolean;
  waiting: number;
  readyLabel: string | null;
  readyBackendId?: string | null;
}): AgentChip {
  const target: ChipTarget = { kind: "console" };
  const logoId = agentLogoKey(chat.readyBackendId);
  if (chat.waiting > 0) {
    return { tone: "waiting", label: "permiso", target, logoId };
  }
  if (chat.working) {
    return { tone: "working", label: chat.readyLabel, target, logoId };
  }
  if (chat.unread > 0) {
    return {
      tone: "ready",
      label: chat.readyLabel ?? "Listo",
      target,
      logoId,
    };
  }
  return OFF;
}

function presenceTarget(p: PresenceView): ChipTarget {
  if (p.window?.hwnd) return { kind: "focus", presenceId: p.id };
  return { kind: "none", presenceId: p.id };
}

function fromPresence(p: PresenceView): AgentChip {
  const target = presenceTarget(p);
  const logoId = agentLogoKey(p.backendId);
  if (p.status === "waiting") {
    return { tone: "waiting", label: "permiso", target, logoId };
  }
  if (p.status === "working") {
    return {
      tone: "working",
      label: p.preview ? clipChipPreview(p.preview) : null,
      target,
      logoId,
    };
  }
  if (p.status === "ready" && p.unread > 0) {
    return { tone: "ready", label: clipChipPreview(p.preview), target, logoId };
  }
  return OFF;
}

function liveLogosFromConsoles(
  consoles: Array<string | null | undefined> | undefined,
): Set<string> {
  const logos = new Set<string>();
  for (const c of consoles ?? []) {
    const key = agentLogoKey(c);
    if (key) logos.add(key);
  }
  return logos;
}

function skipStalePresence(
  p: PresenceView,
  liveLogos: Set<string>,
): boolean {
  if (liveLogos.size === 0) return false;
  if (p.window?.hwnd) return false;
  const logo = agentLogoKey(p.backendId);
  return !logo || !liveLogos.has(logo);
}

function chatForLiveConsoles(
  chat: {
    unread: number;
    working: boolean;
    waiting: number;
    readyLabel: string | null;
    readyBackendId?: string | null;
  },
  liveLogos: Set<string>,
): AgentChip {
  if (liveLogos.size === 0) return fromChat(chat);
  const chatLogo = agentLogoKey(chat.readyBackendId);
  if (chatLogo && liveLogos.has(chatLogo)) return fromChat(chat);
  return OFF;
}

export function agentChip(state: {
  chat: {
    unread: number;
    working: boolean;
    waiting: number;
    readyLabel: string | null;
    readyBackendId?: string | null;
    updatedAt?: number;
    providerSessions?: Array<string | null | undefined>;
  };
  presence: PresenceView[];
  chatEnabled: boolean;
  pagerEnabled: boolean;
  consoles?: Array<string | null | undefined>;
}): AgentChip {
  const liveLogos = liveLogosFromConsoles(state.consoles);
  const chatResult = state.chatEnabled
    ? chatForLiveConsoles(state.chat, liveLogos)
    : OFF;
  const chatAt = state.chat.updatedAt ?? 0;
  if (!state.pagerEnabled) return chatResult;

  const live = new Set(
    (state.chat.providerSessions ?? []).filter((id): id is string => !!id),
  );
  let best = OFF;
  let bestAt = -1;
  for (const p of state.presence) {
    if (live.has(p.id)) continue;
    if (skipStalePresence(p, liveLogos)) continue;
    const next = fromPresence(p);
    if (better(next, p.updatedAt, best, bestAt)) {
      best = next;
      bestAt = p.updatedAt;
    }
  }

  if (chatResult.tone !== "off") {
    const cr = rank(chatResult.tone);
    const br = rank(best.tone);
    if (cr > br || (cr === br && chatAt >= bestAt)) return chatResult;
  }
  return best;
}

const BUSY: Record<string, true> = { working: true, waiting: true };

/**
 * Marca conocida para `AgentLogo`. Acepta backendId, CLI o path (`codex.exe`).
 */
export function agentLogoKey(id: string | null | undefined): string | null {
  if (!id) return null;
  const raw = id.trim().toLowerCase().replace(/\\/g, "/");
  const base = (raw.split("/").pop() ?? raw).replace(
    /\.(exe|cmd|bat|ps1|com)$/i,
    "",
  );
  switch (base) {
    case "claude":
    case "claude-code":
      return "claude-code";
    case "codex":
    case "openai":
      return "codex";
    case "opencode":
      return "opencode";
    case "cursor":
    case "cursor-agent":
      return "cursor-agent";
    default:
      return null;
  }
}

/**
 * Logos de la pestaña: agentes ocupados (chat/TUI) y consolas con CLI conocida.
 * Un id por marca, en el orden en que aparecen.
 */
export function cueAgentIds(state: {
  sessions: Array<{ backendId: string; status: string }>;
  presence: Array<{ backendId: string; status: string }>;
  consoles?: Array<string | null | undefined>;
  /** Si el chip ya eligió un agente, solo ese logo: no mezclar marcas. */
  chipLogoId?: string | null;
}): string[] {
  if (state.chipLogoId) return [state.chipLogoId];
  const ids: string[] = [];
  const add = (raw: string | null | undefined) => {
    const key = agentLogoKey(raw);
    if (key && !ids.includes(key)) ids.push(key);
  };
  for (const s of state.sessions) {
    if (BUSY[s.status]) add(s.backendId);
  }
  for (const p of state.presence) {
    if (BUSY[p.status]) add(p.backendId);
  }
  for (const c of state.consoles ?? []) add(c);
  return ids;
}

/** Compat: el primer logo, o `null` si solo hay consola genérica. */
export function cueAgentId(state: {
  sessions: Array<{ backendId: string; status: string }>;
  presence: Array<{ backendId: string; status: string }>;
  consoles?: Array<string | null | undefined>;
  chipLogoId?: string | null;
}): string | null {
  return cueAgentIds(state)[0] ?? null;
}
