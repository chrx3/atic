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
];
