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
};

const OFF: AgentChip = { tone: "off", label: null, target: { kind: "none" } };

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

function fromChat(chat: {
  unread: number;
  working: boolean;
  waiting: number;
  readyLabel: string | null;
}): AgentChip {
  if (chat.waiting > 0) {
    return { tone: "waiting", label: "permiso", target: { kind: "console" } };
  }
  if (chat.working && chat.unread === 0) {
    return { tone: "working", label: null, target: { kind: "console" } };
  }
  if (chat.unread > 0 && !chat.working) {
    return {
      tone: "ready",
      label: chat.readyLabel ?? "Listo",
      target: { kind: "console" },
    };
  }
  if (chat.unread > 0) {
    return { tone: "count", label: String(chat.unread), target: { kind: "console" } };
  }
  if (chat.working) {
    return { tone: "working", label: null, target: { kind: "console" } };
  }
  return OFF;
}

function presenceTarget(p: PresenceView): ChipTarget {
  if (p.window?.hwnd) return { kind: "focus", presenceId: p.id };
  return { kind: "none", presenceId: p.id };
}

function fromPresence(p: PresenceView): AgentChip {
  const target = presenceTarget(p);
  if (p.status === "waiting") {
    return { tone: "waiting", label: "permiso", target };
  }
  if (p.status === "working") {
    return { tone: "working", label: null, target };
  }
  if (p.status === "ready" && p.unread > 0) {
    return { tone: "ready", label: clipChipPreview(p.preview), target };
  }
  return OFF;
}

export function agentChip(state: {
  chat: {
    unread: number;
    working: boolean;
    waiting: number;
    readyLabel: string | null;
    providerSessions?: Array<string | null | undefined>;
  };
  presence: PresenceView[];
  chatEnabled: boolean;
  pagerEnabled: boolean;
}): AgentChip {
  const chatResult = state.chatEnabled ? fromChat(state.chat) : OFF;
  if (!state.pagerEnabled) return chatResult;

  const live = new Set(
    (state.chat.providerSessions ?? []).filter((id): id is string => !!id),
  );
  let best = OFF;
  for (const p of state.presence) {
    if (live.has(p.id)) continue;
    const next = fromPresence(p);
    if (rank(next.tone) > rank(best.tone)) best = next;
  }

  if (chatResult.tone !== "off" && rank(chatResult.tone) >= rank(best.tone)) {
    return chatResult;
  }
  return best;
}
