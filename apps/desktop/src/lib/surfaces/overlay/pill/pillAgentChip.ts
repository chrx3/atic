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

function fromChat(
  chat: {
    unread: number;
    working: boolean;
    waiting: number;
    readyLabel: string | null;
    readyBackendId?: string | null;
    /** El stream del assistant está vivo: contesta, no solo trabaja. */
    answering?: boolean;
  },
  labels: ChipLabels,
): AgentChip {
  const target: ChipTarget = { kind: "console" };
  const logoId = agentLogoKey(chat.readyBackendId);
  if (chat.waiting > 0) {
    return { id: "chat", tone: "waiting", label: "permiso", target, logoId };
  }
  if (chat.working) {
    return {
      id: "chat",
      tone: "working",
      // Sin texto todavía, el chip igual dice en qué anda: contestando (deltas
      // llegando) o trabajando (herramientas, pensando).
      label: chat.readyLabel ?? (chat.answering ? labels.answering : labels.working),
      target,
      logoId,
    };
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

function fromPresence(p: PresenceView, labels: ChipLabels): AgentChip {
  const target = presenceTarget(p);
  const logoId = agentLogoKey(p.backendId);
  if (p.status === "waiting") {
    return { id: p.id, tone: "waiting", label: "permiso", target, logoId };
  }
  if (p.status === "working") {
    return {
      id: p.id,
      tone: "working",
      // Una TUI solo dice «trabajando»: no hay stream que distinguir.
      label: p.preview?.trim() ? clipChipPreview(p.preview) : labels.working,
      target,
      logoId,
    };
  }
  if (p.status === "ready") {
    // Ya visto (cerraste la consola, o el clic del aviso): no sigue en la
    // pill. El preview solo acompaña al aviso sin leer.
    if (p.unread <= 0) return OFF;
    return {
      id: p.id,
      tone: "ready",
      label: p.preview?.trim() ? clipChipPreview(p.preview) : null,
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

const STALE_WORKING_MS = 90_000;

function skipStalePresence(
  p: PresenceView,
  liveLogos: Set<string>,
  now?: number,
): boolean {
  if (p.window?.hwnd) return false;
  const logo = agentLogoKey(p.backendId);
  if (logo && liveLogos.has(logo)) return false;
  // Ready is already pruned by the watcher. Waiting is an explicit hook
  // signal, so only an unbound working presence gets a frontend age guard.
  // Tests can omit `now` to exercise the presence pipeline without a clock.
  if (p.status !== "working" || now == null) return false;
  return now - p.updatedAt * 1000 > STALE_WORKING_MS;
}

function chatForLiveConsoles(
  chat: {
    unread: number;
    working: boolean;
    waiting: number;
    readyLabel: string | null;
    readyBackendId?: string | null;
    answering?: boolean;
  },
  liveLogos: Set<string>,
  labels: ChipLabels,
): AgentChip {
  if (liveLogos.size === 0) return fromChat(chat, labels);
  const chatLogo = agentLogoKey(chat.readyBackendId);
  if (chatLogo && liveLogos.has(chatLogo)) return fromChat(chat, labels);
  return OFF;
}

type ChipInput = {
  chat: {
    unread: number;
    working: boolean;
    waiting: number;
    readyLabel: string | null;
    readyBackendId?: string | null;
    /** El stream del assistant está vivo: contesta, no solo trabaja. */
    answering?: boolean;
    updatedAt?: number;
    providerSessions?: Array<string | null | undefined>;
  };
  presence: PresenceView[];
  chatEnabled: boolean;
  pagerEnabled: boolean;
  consoles?: Array<string | null | undefined>;
  /** Epoch milliseconds used only to discard genuinely old unbound work. */
  now?: number;
  /** Textos del chip cuando no hay preview. La UI pasa los de i18n. */
  workingLabel?: string;
  answeringLabel?: string;
};

/** Textos por defecto del chip ocupado; la UI los pisa con su i18n. */
type ChipLabels = { working: string; answering: string };

const DEFAULT_LABELS: ChipLabels = {
  working: "Trabajando…",
  answering: "Contestando…",
};

const CHIP_STACK_MAX = 4;

/**
 * Todos los avisos activos, urgentes arriba. Uno por consola/TUI, no un
 * «mejor» que esconda al resto.
 */
export function agentChips(state: ChipInput): AgentChip[] {
  const liveLogos = liveLogosFromConsoles(state.consoles);
  const labels: ChipLabels = {
    working: state.workingLabel ?? DEFAULT_LABELS.working,
    answering: state.answeringLabel ?? DEFAULT_LABELS.answering,
  };
  const chatResult = state.chatEnabled
    ? chatForLiveConsoles(state.chat, liveLogos, labels)
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
      if (skipStalePresence(p, liveLogos, state.now)) continue;
      const next = fromPresence(p, labels);
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

type CueAgentState = {
  sessions: Array<{ backendId: string; status: string }>;
  presence: Array<{ backendId: string; status: string }>;
  consoles?: Array<string | null | undefined>;
  /** Si el chip ya eligió un agente, solo ese logo: no mezclar marcas. */
  chipLogoId?: string | null;
};

/**
 * Logos de la pestaña: agentes ocupados (chat/TUI) y consolas con CLI conocida.
 * Un id por marca, en el orden en que aparecen.
 */
export function cueAgentIds(state: CueAgentState): string[] {
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

export function agentChipLogos(
  chip: Pick<AgentChip, "tone" | "logoId">,
  state: Omit<CueAgentState, "chipLogoId">,
  dockMinimized: boolean,
): string[] {
  if (chip.tone !== "off") return chip.logoId ? [chip.logoId] : [];
  if (!dockMinimized) return [];
  return cueAgentIds(state);
}

/** Cuántos logos caben en un aviso antes de pasar a contador. */
export const LOGO_SLOTS_MAX = 3;

/**
 * Qué logos se ven en un aviso y cuántos quedan contados.
 *
 * Con la consola minimizada, el aviso junta las marcas de todos los agentes
 * vivos. Sin tope, cinco logos de 18 px se salían de la pestaña: el botón
 * crece con su contenido, pero la pestaña se mide antes. Hasta tres se ven
 * enteros; con más, dos logos y un "+N" en la tercera celda — la lista
 * completa sigue en el globo.
 */
export function logoSlots(
  logos: string[],
  max = LOGO_SLOTS_MAX,
): { shown: string[]; extra: number; cells: number } {
  if (logos.length <= max) {
    return { shown: logos, extra: 0, cells: Math.max(1, logos.length) };
  }
  const shown = logos.slice(0, max - 1);
  return { shown, extra: logos.length - shown.length, cells: max };
}

/**
 * Al cerrar o achicar la consola de Atic, estos avisos ya no tienen dónde
 * volver. Incluye la TUI propia, el JSONL de un CLI que seguía adentro y el
 * de uno que acabas de cerrar. La terminal externa (HWND ajeno) se queda:
 * esa ventana no la cerraste desde Atic.
 */
export function presenceIdsToDismissOnAticHide(
  presence: Array<{
    id: string;
    backendId: string;
    window?: { hwnd?: number | null; own?: boolean } | null;
  }>,
): string[] {
  const ids: string[] = [];
  for (const p of presence) {
    if (p.window?.hwnd && !p.window.own) continue;
    ids.push(p.id);
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
