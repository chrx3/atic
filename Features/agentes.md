# Agentes multi-proveedor

**Estado:** `parcial`

## Resumen

Conversaciones con Claude Code, Codex, OpenCode y Cursor dentro de Atic,
usando el CLI ya instalado y autenticado del usuario. Modelo canónico de
hilos/turnos/items; prefs de modelo, esfuerzo, Fast y modo de permisos
recordadas por backend.

## Cómo se usa

- Abrir agentes desde la pill (burbuja anclada).
- Elegir pestaña de proveedor, modelo, esfuerzo, Fast (Cursor) y escudo de
  permisos; se recuerdan al reabrir.
- Escribir, dictar o adjuntar capturas/archivos; Enter inicia o envía.
- Permisos de herramientas se aprueban en la UI cuando el agente pregunta.

## Código

- [`apps/desktop/src-tauri/src/agents/`](../apps/desktop/src-tauri/src/agents/) — adaptadores y bridge
- [`apps/desktop/src/routes/agents/+page.svelte`](../apps/desktop/src/routes/agents/+page.svelte)
- [`apps/desktop/src/lib/agentModels.ts`](../apps/desktop/src/lib/agentModels.ts) — recuerdo localStorage
- [`apps/desktop/src/lib/agentSessions.svelte.ts`](../apps/desktop/src/lib/agentSessions.svelte.ts)

## Pendiente / siguiente

- [ ] UI para `cursor/ask_question` y `cursor/create_plan` (hoy auto skip/accept)
- [ ] Mover `static SESSIONS` / pendientes del plan de agentes
- [ ] Paridad de UX entre backends (costos, modos, errores)

## Relacionado

- [dictado.md](dictado.md)
- [capturas.md](capturas.md)
- [`docs/PLAN_AGENTES.md`](../docs/PLAN_AGENTES.md)
