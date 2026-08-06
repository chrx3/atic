import type { AgentSkill, SlashCommand } from "$lib/types";

/**
 * Comandos built-in de Claude Code cuando todavía no llegó el handshake.
 * El catálogo vivo del CLI los reemplaza (incluye skills y custom del repo).
 */
export const CLAUDE_CODE_FALLBACK_COMMANDS: SlashCommand[] = [
  {
    name: "compact",
    description: "Compacta el contexto en un resumen",
    argumentHint: "[instrucciones]",
  },
  {
    name: "clear",
    description: "Limpia la conversación actual",
    argumentHint: "",
  },
  {
    name: "context",
    description: "Muestra el uso de contexto",
    argumentHint: "",
  },
  {
    name: "cost",
    description: "Muestra el costo de la sesión",
    argumentHint: "",
  },
  {
    name: "usage",
    description: "Uso y límites de la cuenta",
    argumentHint: "",
  },
  {
    name: "model",
    description: "Cambia el modelo de esta sesión",
    argumentHint: "[alias]",
  },
  {
    name: "effort",
    description: "Cambia cuánto piensa el modelo",
    argumentHint: "[low|medium|high|xhigh|max|auto]",
  },
  {
    name: "permissions",
    description: "Configura el modo de permisos",
    argumentHint: "[manual|acceptEdits|plan|bypassPermissions]",
  },
  {
    name: "plan",
    description: "Activa el modo plan (solo lectura)",
    argumentHint: "",
  },
  {
    name: "help",
    description: "Lista ayuda de comandos",
    argumentHint: "",
  },
  {
    name: "resume",
    description: "Reanuda una sesión anterior",
    argumentHint: "",
  },
  {
    name: "doctor",
    description: "Diagnostica la instalación del CLI",
    argumentHint: "",
  },
  {
    name: "memory",
    description: "Edita la memoria del proyecto",
    argumentHint: "",
  },
  {
    name: "init",
    description: "Inicializa CLAUDE.md en el proyecto",
    argumentHint: "",
  },
  {
    name: "review",
    description: "Revisa cambios del código",
    argumentHint: "",
  },
];

/** Skills de disco → misma forma que un slash del CLI. */
export function skillsAsCommands(skills: AgentSkill[] | undefined | null): SlashCommand[] {
  if (!skills?.length) return [];
  return skills.map((s) => ({
    name: s.name,
    description: s.description,
    argumentHint: "",
  }));
}

/**
 * Une listas por `name`. Las posteriores pisan campos vacíos / ganan si
 * traen texto (así skills enriquecen un handshake pelado, y el cache pisa
 * el fallback).
 */
export function mergeSlashCommands(
  ...lists: Array<SlashCommand[] | undefined | null>
): SlashCommand[] {
  const byName = new Map<string, SlashCommand>();
  for (const list of lists) {
    if (!list) continue;
    for (const cmd of list) {
      const prev = byName.get(cmd.name);
      if (!prev) {
        byName.set(cmd.name, {
          name: cmd.name,
          description: cmd.description || "",
          argumentHint: cmd.argumentHint || "",
        });
        continue;
      }
      byName.set(cmd.name, {
        name: cmd.name,
        description: cmd.description || prev.description,
        argumentHint: cmd.argumentHint || prev.argumentHint,
      });
    }
  }
  return [...byName.values()].sort((a, b) => a.name.localeCompare(b.name));
}

/**
 * Catálogo vivo del CLI si hay; si no, fallback + skills de disco + cache.
 * Así `/` no espera el spawn del CLI para mostrar skills.
 */
export function resolveSlashCommands(
  live: SlashCommand[] | undefined | null,
  cached: SlashCommand[] | undefined | null,
  skills: SlashCommand[] | undefined | null = null,
  fallback: SlashCommand[] = CLAUDE_CODE_FALLBACK_COMMANDS,
): SlashCommand[] {
  if (live && live.length > 0) {
    // El CLI manda el catálogo oficial; skills rellenan descripciones vacías.
    return mergeSlashCommands(live, skills);
  }
  return mergeSlashCommands(fallback, skills, cached);
}
