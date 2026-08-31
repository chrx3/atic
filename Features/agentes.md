# Agentes multi-proveedor

**Estado:** `chat Claude Code` (resume del CLI + historial Atic)

## Resumen

UI en ventana principal y pill que spawnea **Claude Code** con las
credenciales locales del usuario: no lee tokens; el CLI hereda el login.

- [`AgentsDemo.svelte`](../apps/desktop/src/lib/features/agents/AgentsDemo.svelte)
  — chat real + historial Atic + sesiones del CLI (`--resume`)
- [`AgentConversation.svelte`](../apps/desktop/src/lib/AgentConversation.svelte)
  — mensajes (markdown), tools, thinking, plan, collab
- [`claude_sessions.rs`](../apps/desktop/src-tauri/src/agents/claude_sessions.rs)
  — índice de `~/.claude/projects/<cwd>/…jsonl` (sin importar el transcript)
- [`AgentsTool.svelte`](../apps/desktop/src/lib/features/agents/AgentsTool.svelte) /
  [`AgentsFloat.svelte`](../apps/desktop/src/lib/surfaces/overlay/agents/AgentsFloat.svelte)
  — hosts

## Hecho

- [x] Transcript rico (`AgentConversation`)
- [x] Historial Atic + continuar con `providerSession`
- [x] Picker tipo `/resume`: sesiones del CLI en la carpeta
- [x] Diálogo al importar: desde resumen (`/compact`), completa, o solo contexto
- [x] Compactar contexto en sesión viva (aviso + resumen + recorte UI)
- [x] Autocomplete `/`: catálogo del handshake (`name`, `description`, `argumentHint`)
- [x] Sesión al enfocar/`/`: catálogo completo sin mandar mensaje
- [x] Select de `/effort` en composer (low…auto)
- [x] Misma UI en main y pill
- [x] Atajo global para abrir/cerrar la consola (`agents_shortcut`, default
  `CmdOrCtrl+Shift+A`; Ajustes → Atajos)

## Pendiente

- [ ] Modos de permiso en composer
- [ ] Otros backends en UI (Codex, OpenCode, Cursor)
- [ ] Dictado → composer
- [ ] MCP / skills visibles
- [x] Hosts SSH / agente remoto MVP (Claude Code vía `ssh`; plan: [ssh-remote-hosts.md](ssh-remote-hosts.md))

## Relacionado

- [dictado.md](dictado.md)
- [liquid.md](liquid.md)
- [ssh-remote-hosts.md](ssh-remote-hosts.md)
- [`docs/PLAN_AGENTES.md`](../docs/PLAN_AGENTES.md)
- Orquestación entre agentes (idea): [orquestacion-agentes.md](orquestacion-agentes.md) · [`docs/PLAN_ORQUESTACION_MCP.md`](../docs/PLAN_ORQUESTACION_MCP.md)
