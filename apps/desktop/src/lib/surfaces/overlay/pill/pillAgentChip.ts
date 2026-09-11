import { clipChipPreview } from "$core/agentChipPreview";
import type { PresenceView } from "$lib/agentPresenceReduce";

export type ChipTone = "waiting" | "working" | "ready" | "count" | "off";
export type ChipTarget =
  | { kind: "console"; presenceId?: string }
  | { kind: "focus"; presenceId: string }
  | { kind: "none"; presenceId?: string };

export type AgentChip = {
  /** `chat` o el id de la presencia TUI. */
  id: string;
  tone: ChipTone;
  label: string | null;
  target: ChipTarget;
  logoId: string | null;
};

const OFF: AgentChip = {
  id: "",
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
    return { id: "chat", tone: "waiting", label: "permiso", target, logoId };
  }
  if (chat.working) {
    return { id: "chat", tone: "working", label: chat.readyLabel, target, logoId };
  }
  if (chat.unread > 0) {
    return {
      id: "chat",
      tone: "ready",
      label: chat.readyLabel ?? "Listo",
      target,
      logoId,
    };
  }
  return OFF;
}

function presenceTarget(p: PresenceView): ChipTarget {
  if (p.window?.own) return { kind: "console", presenceId: p.id };
  if (p.window?.hwnd) return { kind: "focus", presenceId: p.id };
  return { kind: "none", presenceId: p.id };
}

function fromPresence(p: PresenceView): AgentChip {
  const target = presenceTarget(p);
  const logoId = agentLogoKey(p.backendId);
  if (p.status === "waiting") {
    return { id: p.id, tone: "waiting", label: "permiso", target, logoId };
  }
  if (p.status === "working") {
    return {
      id: p.id,
      tone: "working",
      label: p.preview ? clipChipPreview(p.preview) : null,
      target,
      logoId,
    };
  }
  if (p.status === "ready" && p.unread > 0) {
    return {
      id: p.id,
      tone: "ready",
      label: clipChipPreview(p.preview),
      target,
      logoId,
    };
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

function skipStalePresence(p: PresenceView, liveLogos: Set<string>): boolean {
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

type ChipInput = {
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
};

const CHIP_STACK_MAX = 4;

/**
 * Todos los avisos activos, urgentes arriba. Uno por consola/TUI, no un
 * «mejor» que esconda al resto.
 */
export function agentChips(state: ChipInput): AgentChip[] {
  const liveLogos = liveLogosFromConsoles(state.consoles);
  const chatResult = state.chatEnabled
    ? chatForLiveConsoles(state.chat, liveLogos)
    : OFF;
  const chatAt = state.chat.updatedAt ?? 0;
  const ranked: { chip: AgentChip; at: number }[] = [];
  if (chatResult.tone !== "off") {
    ranked.push({ chip: chatResult, at: chatAt });
  }
  if (state.pagerEnabled) {
    const live = new Set(
      (state.chat.providerSessions ?? []).filter((id): id is string => !!id),
    );
    for (const p of state.presence) {
      if (live.has(p.id)) continue;
      if (skipStalePresence(p, liveLogos)) continue;
      const next = fromPresence(p);
      if (next.tone === "off") continue;
      ranked.push({ chip: next, at: p.updatedAt });
    }
  }
  ranked.sort((a, b) => {
    if (better(a.chip, a.at, b.chip, b.at)) return -1;
    if (better(b.chip, b.at, a.chip, a.at)) return 1;
    if (a.chip.id === "chat" && b.chip.id !== "chat") return -1;
    if (b.chip.id === "chat" && a.chip.id !== "chat") return 1;
    return a.chip.id.localeCompare(b.chip.id);
  });
  return ranked.slice(0, CHIP_STACK_MAX).map((x) => x.chip);
}

export function agentChip(state: ChipInput): AgentChip {
  return agentChips(state)[0] ?? OFF;
}

const BUSY: Record<string, true> = { working: true, waiting: true };

/**
 * Marca conocida para `AgentLogo`. Acepta backendId, CLI o path (`codex.exe`).
 */
export function agentLogoKey(id: string | null | undefined): string | null {
  if (!id) return null;
  const raw = id.trim().toLowerCase().replace(/\\/g, "/");
  const base = (raw.split("/").pop() ?? raw).replace(/\.(exe|cmd|bat|ps1|com)$/i, "");
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
    case "agy":
    case "antigravity":
      return "agy";
    case "grok":
    case "xai":
      return "grok";
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

/**
 * Al achicar o cerrar el globo de Atic, estos avisos TUI ya no tienen
 * consola que mostrar: la ventana propia, o el JSONL de un CLI que sigue
 * vivo adentro.
 */
export function presenceIdsToDismissOnAticHide(
  presence: Array<{
    id: string;
    backendId: string;
    window?: { hwnd?: number | null; own?: boolean } | null;
  }>,
  consoles: Array<string | null | undefined> | undefined,
): string[] {
  const logos = liveLogosFromConsoles(consoles);
  const ids: string[] = [];
  for (const p of presence) {
    if (p.window?.own) {
      ids.push(p.id);
      continue;
    }
    if (p.window?.hwnd) continue;
    const logo = agentLogoKey(p.backendId);
    if (logo && logos.has(logo)) ids.push(p.id);
  }
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
