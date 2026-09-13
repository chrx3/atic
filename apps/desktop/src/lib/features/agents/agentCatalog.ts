/**
 * Catálogo de CLIs de agentes que Atic sabe lanzar en una consola local.
 * Lo comparten el lanzador (grilla de selección) y el menú "+" del rail
 * de consolas: una sola lista, un solo orden.
 */
import { shortcutOs } from "$core/hotkeys";

export type AgentDef = {
  cli: string;
  name: string;
  /**
   * Línea oficial de instalación por sistema.
   *
   * `windows` usa `irm | iex` (los instaladores nativos) o npm cuando el
   * vendor no publica uno; el resto son los `curl | bash` oficiales, que
   * también sirven en Linux.
   */
  install: { windows: string; macos: string };
};

/** La línea de instalación del SO en el que corre Atic. */
export function installCommand(agent: AgentDef): string {
  // `shortcutOs` lo setea el layout al arrancar con el userAgent real; en
  // tests queda "other", que comparte el camino de bash con macOS.
  return shortcutOs() === "windows" ? agent.install.windows : agent.install.macos;
}

export const AGENTS: AgentDef[] = [
  {
    cli: "claude",
    name: "Claude Code",
    install: {
      windows: "irm https://claude.ai/install.ps1 | iex",
      macos: "curl -fsSL https://claude.ai/install.sh | bash",
    },
  },
  {
    cli: "opencode",
    name: "OpenCode",
    install: {
      windows: "npm install -g opencode-ai",
      // v2 se instala standalone en ~/.opencode/bin (y le gana al shim npm).
      macos: "curl -fsSL https://opencode.ai/install | bash",
    },
  },
  {
    cli: "codex",
    name: "Codex",
    install: {
      windows: "npm install -g @openai/codex",
      macos: "npm install -g @openai/codex",
    },
  },
  {
    cli: "cursor-agent",
    name: "Cursor",
    install: {
      windows: "irm 'https://cursor.com/install?win32=true' | iex",
      macos: "curl https://cursor.com/install -fsS | bash",
    },
  },
  {
    // Sucesor del Gemini CLI clásico, que Google retiró en jun-2026.
    cli: "agy",
    name: "Antigravity",
    install: {
      windows: "irm https://antigravity.google/cli/install.ps1 | iex",
      macos: "curl -fsSL https://antigravity.google/cli/install.sh | bash",
    },
  },
  {
    // El instalador deja `grok` en `~/.grok/bin` y lo suma al PATH.
    cli: "grok",
    name: "Grok",
    install: {
      windows: "irm https://x.ai/cli/install.ps1 | iex",
      macos: "curl -fsSL https://x.ai/cli/install.sh | bash",
    },
  },
];

/**
 * Los agentes a la vista, en el orden del catalogo.
 *
 * `shown` vacia significa «sin configurar», y sin configurar se ven todos: es
 * lo que hace que sumar un agente al catalogo aparezca sin tocarle la config a
 * nadie. Una lista que deja fuera a todos tambien devuelve todos —quedarse sin
 * agentes no es un estado util, es una pantalla vacia sin forma de salir—.
 *
 * El orden nunca sale de `shown`: es el del catalogo, para que la grilla no
 * baile segun el orden en que se marcaron las casillas.
 */
export function shownAgents(shown: readonly string[]): AgentDef[] {
  const picked = AGENTS.filter((agent) => shown.includes(agent.cli));
  return picked.length > 0 ? picked : AGENTS;
}

/** Si un cli concreto esta a la vista. Misma regla que `shownAgents`. */
export function isAgentShown(cli: string, shown: readonly string[]): boolean {
  return shownAgents(shown).some((agent) => agent.cli === cli);
}
