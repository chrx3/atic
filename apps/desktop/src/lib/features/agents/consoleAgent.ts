/**
 * Cómo se reconoce un CLI de agente dentro de una consola: output, título
 * del TUI o nombre de proceso. Una sola lista, la del catálogo.
 */
import { AGENTS } from "./agentCatalog";

const ALIAS: Record<string, string> = {
  claude: "claude",
  "claude-code": "claude",
  openai: "codex",
  codex: "codex",
  opencode: "opencode",
  cursor: "cursor-agent",
  "cursor-agent": "cursor-agent",
  grok: "grok",
  xai: "grok",
  agy: "agy",
  antigravity: "agy",
};

const RESTART_NAME =
  /please restart\s+(claude(?:\s+code)?|codex|opencode|cursor(?:-agent)?|grok|agy|antigravity)/i;
const UPDATE_OK = /update ran successfully|\bupdated successfully\b/i;

function stripAnsi(chunk: string): string {
  return chunk.replace(/\x1b\[[0-9;?]*[ -/]*[@-~]/g, "").replace(/\r/g, "");
}

function stemOf(raw: string): string {
  const key = raw.trim().toLowerCase().replace(/\\/g, "/");
  return (key.split("/").pop() ?? key).replace(/\.(exe|cmd|bat|ps1|com)$/i, "");
}

/** CLI canónico (`codex`, `claude`…) o `null` si no es un agente conocido. */
export function canonicalAgentCli(raw: string | null | undefined): string | null {
  if (!raw) return null;
  const stem = stemOf(raw);
  if (ALIAS[stem]) return ALIAS[stem];
  const named = AGENTS.find(
    (agent) =>
      agent.cli === stem || agent.name.toLowerCase() === raw.trim().toLowerCase(),
  );
  return named?.cli ?? null;
}

export function agentDisplayName(cli: string): string {
  return AGENTS.find((agent) => agent.cli === cli)?.name ?? cli;
}

/**
 * El updater del CLI pide reinicio y deja el prompt de la shell (`cmd /K`).
 * Devuelve el CLI a relanzar, o `null` si el chunk no es ese aviso.
 */
export function restartCliFromOutput(
  chunk: string,
  fallback: string | null | undefined,
): string | null {
  const text = stripAnsi(chunk);
  const named = text.match(RESTART_NAME);
  if (named?.[1]) {
    const token = named[1].trim().replace(/\s+/g, "-");
    return canonicalAgentCli(token) ?? canonicalAgentCli(named[1]);
  }
  if (UPDATE_OK.test(text)) {
    return canonicalAgentCli(fallback) ?? (fallback?.trim() || null);
  }
  return null;
}

/** Título OSC del TUI (`Codex`, `Claude Code · …`). */
export function cliFromTitle(title: string): string | null {
  const trimmed = title.trim();
  if (!trimmed) return null;
  return (
    canonicalAgentCli(trimmed) ??
    canonicalAgentCli(trimmed.split(/[|·\u2014\-]/)[0] ?? "")
  );
}
