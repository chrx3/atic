# Orquestación de agentes (MCP)

**Estado:** `idea`

## Resumen

Un servidor MCP de Atic para que cualquier agente (Claude Code, Codex, Cursor
IDE / Agent, OpenCode) liste, elija, levante o le encargue un turno a
cualquier otro. Atic es el directorio; MCP es la puerta; por dentro se reusa
el harness que ya existe.

## Cómo se usa

- Atic abierto en la bandeja (v1: el hub vive ahí).
- El MCP `atic` se carga en Claude / Cursor / Codex / OpenCode (snippet en
  Ajustes, o merge automático si Atic arranca Claude Code).
- El modelo llama `atic_list_agents`, `atic_delegate`, etc.

## Código

Aún no. Plan: [`docs/PLAN_ORQUESTACION_MCP.md`](../docs/PLAN_ORQUESTACION_MCP.md).

El harness que se reusa:

- [`bridge.rs`](../apps/desktop/src-tauri/src/agents/bridge.rs) — start / send
- [`exe.rs`](../apps/desktop/src-tauri/src/agents/exe.rs) — spawn en Windows
- [`McpServersModal.svelte`](../apps/desktop/src/lib/McpServersModal.svelte) —
  MCP *para* el agente, distinto de este servidor

## Pendiente / siguiente

- [x] Cerrar las preguntas de revisión del plan (Fable 5, 2026-08-30)
- [ ] Fase 0: grafo (profundidad, ciclos, ruteo por `kind`) con tests, sin CLIs
- [ ] Hub localhost + binario `atic-mcp` (fase 1)
- [ ] Snippets para Cursor IDE y Codex CLI

## Relacionado

- [agentes.md](agentes.md)
- [`docs/PLAN_AGENTES.md`](../docs/PLAN_AGENTES.md)
- [`docs/PLAN_ORQUESTACION_MCP.md`](../docs/PLAN_ORQUESTACION_MCP.md)
