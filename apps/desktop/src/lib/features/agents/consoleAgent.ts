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

/** Más largo que esto no es un nombre, es una frase. */
const NOMBRE_MAX = 40;

/**
 * El nombre propio que el TUI puso en su título, si puso alguno.
 *
 * Los CLIs escriben el título por OSC, y ahí es donde asoma un `/rename`: el
 * título pasa de «Grok» a «Grok · agy». Se descartan los trozos que son el
 * nombre de un CLI —eso es el título por defecto, no un rename— y los que son
 * rutas, que son el cwd: la pestaña ya lo muestra aparte y cambiaría con cada
 * `cd`.
 *
 * `null` = el título no dice nada que no supiéramos, y la pestaña se queda
 * como está en vez de bailar con cada cambio del TUI.
 */
export function tabNameFromTitle(title: string): string | null {
  const trozos = title
    .split(/[|·—–]|\s-\s/)
    .map((t) => t.trim())
    // Fuera el cwd (`~/Downloads`, `C:\repo`) y las frases: ni uno ni
    // otro son un nombre, y el cwd cambiaría con cada `cd`.
    .filter(
      (t) => t && t.length <= NOMBRE_MAX && !/[/\\]/.test(t) && !t.startsWith("~"),
    );
  if (trozos.length === 0) return null;
  // Un solo trozo y es la marca: el título por defecto, no un rename.
  if (trozos.length === 1) return canonicalAgentCli(trozos[0]) ? null : trozos[0];
  // Con varios, el nombre es el primero que no sea una marca; si todos lo
  // parecen —renombrar a «agy» es legítimo— vale el último, que es el que el
  // TUI agregó detrás de la suya.
  return trozos.find((t) => !canonicalAgentCli(t)) ?? trozos[trozos.length - 1];
}
