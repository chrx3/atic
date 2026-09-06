# Orquestación de agentes (MCP)

**Estado:** `parcial` (fases 0 y 1 codificadas y validadas; demo §5.10
hecha el 2026-09-06 con Atic en dev, Claude Code y Codex reales: listar,
delegar, `already_running`, `atic_prompt`/`atic_wait`/`atic_cancel`,
`permission_timeout` y cadena hijo Claude → MCP; faltan `pnpm tauri build`
con el sidecar y la fase 2 con Cursor IDE / Codex TUI)

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

Fases 0 y 1 codificadas y validadas con tests, clippy y `svelte-check`
(2026-09-05). Pendientes: la demo manual del §5.10 del traspaso y las
correcciones del §10. Plan:
[`docs/PLAN_ORQUESTACION_MCP.md`](../docs/PLAN_ORQUESTACION_MCP.md).
Tareas para implementar (rutas, contratos, tests, criterios):
[`docs/TAREAS_ORQUESTACION_MCP.md`](../docs/TAREAS_ORQUESTACION_MCP.md).

Nuevo:

- [`agents/hub/`](../apps/desktop/src-tauri/src/agents/hub/) — `api` (contrato),
  `graph` (reglas puras), `wait` (`TurnWatch`), `server` (HTTP localhost),
  `mod` (estado, merge de `mcp_config`, snippets)
- [`crates/atic-mcp/`](../../crates/atic-mcp/) — sidecar MCP stdio (`rmcp` solo acá)
- Ajustes → Agentes → «Desde otras apps» (estado del hub + snippets por host)

El harness que se reusa:

- [`bridge.rs`](../apps/desktop/src-tauri/src/agents/bridge.rs) — start / send
- [`exe.rs`](../apps/desktop/src-tauri/src/agents/exe.rs) — spawn en Windows
- [`McpServersModal.svelte`](../apps/desktop/src/lib/McpServersModal.svelte) —
  MCP *para* el agente, distinto de este servidor

## Pendiente / siguiente

- [x] Cerrar las preguntas de revisión del plan (Fable 5, 2026-08-30)
- [x] Re-validar el plan contra la spec MCP 2026-07-28, `rmcp` 3.2 y los
      timeouts de los CLIs (Fable 5.1, 2026-09-05): enmiendas E1–E5 en el plan
- [ ] Fase 0: grafo (profundidad, ciclos, ruteo por `kind`, espera por host)
      con tests, sin CLIs — codificado, falta correr `cargo test -p atic-desktop hub::`
- [ ] Hub localhost + binario `atic-mcp` (fase 1) — codificado, falta la demo
      manual del §5.10 (hub.json, delegar a Codex, `already_running`, permiso,
      `atic_wait`, error con Atic cerrado, `cargo test --workspace`, NSIS)
- [ ] Snippets para Cursor IDE y Codex CLI (fase 2: probar en las apps originales)
- [ ] Pulido (fase 3): `atic_cancel` contra los cuatro backends, tope de 8 KB
      por item en hilos hijos (revisar `store::apply` antes de tocar), i18n final

## Relacionado

- [agentes.md](agentes.md)
- [`docs/PLAN_AGENTES.md`](../docs/PLAN_AGENTES.md)
- [`docs/PLAN_ORQUESTACION_MCP.md`](../docs/PLAN_ORQUESTACION_MCP.md)
