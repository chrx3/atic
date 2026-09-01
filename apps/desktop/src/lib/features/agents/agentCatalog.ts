/**
 * Catálogo de CLIs de agentes que Atic sabe lanzar en una consola local.
 * Lo comparten el lanzador (grilla de selección) y el menú "+" del rail
 * de consolas: una sola lista, un solo orden.
 */
export type AgentDef = { cli: string; name: string; install: string };

/**
 * `install` es la línea oficial para Windows (la app hoy solo corre ahí) y se
 * ejecuta en una consola nueva, dentro de la shell del usuario: `irm | iex`
 * son los instaladores nativos publicados por cada vendor; el resto va por
 * npm porque no publican instalador de Windows.
 */
export const AGENTS: AgentDef[] = [
  {
    cli: "claude",
    name: "Claude Code",
    install: "irm https://claude.ai/install.ps1 | iex",
  },
  {
    cli: "opencode",
    name: "OpenCode",
    install: "npm install -g opencode-ai",
  },
  {
    cli: "codex",
    name: "Codex",
    install: "npm install -g @openai/codex",
  },
  {
    cli: "cursor-agent",
    name: "Cursor",
    install: "irm 'https://cursor.com/install?win32=true' | iex",
  },
  {
    // Sucesor del Gemini CLI clásico, que Google retiró en jun-2026.
    cli: "agy",
    name: "Antigravity",
    install: "irm https://antigravity.google/cli/install.ps1 | iex",
  },
  {
    // El instalador deja `grok.exe` en `~/.grok/bin` y lo suma al PATH.
    cli: "grok",
    name: "Grok",
    install: "irm https://x.ai/cli/install.ps1 | iex",
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
