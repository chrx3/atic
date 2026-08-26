/**
 * Catálogo de CLIs de agentes que Atic sabe lanzar en una consola local.
 * Lo comparten el lanzador (grilla de selección) y el menú "+" del rail
 * de consolas: una sola lista, un solo orden.
 */
export type AgentDef = { cli: string; name: string };

export const AGENTS: AgentDef[] = [
  { cli: "claude", name: "Claude Code" },
  { cli: "opencode", name: "OpenCode" },
  { cli: "codex", name: "Codex" },
  { cli: "cursor-agent", name: "Cursor" },
];
