# Agentes multi-proveedor

**Estado:** consolas PTY + fichas de chat en el mismo rail (Fase 1 de
[`PLAN_CONSOLAS.md`](../docs/PLAN_CONSOLAS.md))

## Resumen

Cada agente se abre como **terminal** (el TUI del CLI en una PTY) o como
**chat** (la sesión estructurada del puente: stream-json en Claude Code,
app-server en Codex, ACP en OpenCode/Cursor/Grok). Las dos son fichas del
mismo rail: split, zoom, atajos y traspaso isla ⇄ float ⇄ ventana son
comunes. El CLI hereda el login local: Atic no lee tokens.

- [`ConsolePanel.svelte`](../apps/desktop/src/lib/features/agents/ConsolePanel.svelte)
  — rail de fichas; el `+` ofrece terminal o chat por agente
- [`AgentChatPanel.svelte`](../apps/desktop/src/lib/features/agents/AgentChatPanel.svelte)
  — ficha de chat: transcript, permisos, composer; en sólo lectura para las
  sesiones que abre otro agente por MCP
- [`AgentConversation.svelte`](../apps/desktop/src/lib/AgentConversation.svelte)
  — mensajes (markdown), tools, thinking, plan, collab
- [`AgentLauncher.svelte`](../apps/desktop/src/lib/features/agents/AgentLauncher.svelte) /
  [`AgentsFloat.svelte`](../apps/desktop/src/lib/surfaces/overlay/agents/AgentsFloat.svelte)
  — lanzador y hosts

## Hecho

- [x] Transcript rico (`AgentConversation`)
- [x] Chat como ficha del rail, sin chrome propio (menú `+` y lanzador)
- [x] Permiso como tarjeta en el hilo, con el comando o el diff a aprobar (`ChatPermission`)
- [x] Autocomplete `/`: catálogo del handshake + skills de disco (Claude)
- [x] Selector único agente · modelo (`AgentModelPicker`): cambia de agente en el chat vacío o abre ficha nueva
- [x] Herramientas agrupadas en bloques de actividad (`chatThread.ts`, con tests)
- [x] Esfuerzo y modo de permisos (este último solo Claude) en el composer
- [x] Continuar una conversación guardada (Claude Code y Codex)
- [x] Adjuntar imágenes: pegar, soltar, o desde el historial del portapapeles
- [x] Sesiones del hub (MCP) en el mismo panel, sólo lectura
- [x] Atajo global para abrir/cerrar la consola (`agents_shortcut`, default
  `CmdOrCtrl+Shift+A`; Ajustes → Atajos)

## Instalación de CLIs

Cuando un agente no está en el PATH, el lanzador y el menú «+» de la consola
siembran una consola nueva con su **instalador oficial** (`agentCatalog.ts`,
línea por SO: `curl | bash` en macOS/Linux, `irm | iex` o npm en Windows). El
usuario ve correr la instalación y Atic re-detecta el CLI al volver.

- [x] macOS: al arrancar se arma el PATH de la app (Finder hereda el mínimo)
      con Homebrew, `~/.local/bin`, `~/.opencode/bin`, cargo/bun/volta/pnpm y
      nvm; la consola usa `$SHELL` o zsh.
- [x] OpenCode v2: el binario standalone de `~/.opencode/bin` le gana al shim
      npm de v1, y su MCP se registra con `opencode mcp add` (formato
      `mcp.servers`); v1 sigue por el JSON de siempre.

## Pendiente (chat)

**Verificar en la app** (nada de esto se probó a mano todavía)

- [ ] Prompt real con tool-calls: bloques de actividad, en vivo y plegados
- [ ] Permiso desde la tarjeta: Rechazar / Aprobar / Aprobar siempre
- [ ] Cambiar modelo, esfuerzo y modo en caliente (Claude y Codex)
- [ ] Cambiar de agente: chat vacío se reemplaza; con conversación abre ficha
- [ ] Historial y «Continuar»: retoma con el hilo en pantalla
- [ ] Adjuntos: pegar, soltar, clip y desde el historial del portapapeles
- [ ] Popovers en la isla (440 px): sin cortes
- [ ] Minimizar/restaurar y despegar isla ⇄ float ⇄ ventana con un chat vivo

**Falta para estar a la par de la terminal**

- [x] Estado de la ficha en el rail: trabajando / espera permiso / respondió (`chatStatus.ts`) — falta verlo en la app
- [x] Aviso al terminar fuera de vista: la ficha activa y visible marca la sesión como mirada, y el store avisa (notificación del sistema) solo si no lo estás mirando — falta verlo en la app
- [ ] El chat en la pill: chip y vistazo (Fase 2, `agentRoster.ts`)
- [x] Fichas de chat que sobreviven a recargar la vista: cada lanzador guarda
      las suyas (`chatTabs.ts`) y las retoma con su hilo; «Cerrar consolas» las
      termina — falta verlo en la app
- [ ] Contexto y costo de la sesión en el composer (`contextTokens`,
      `costUsd` ya llegan), y `/compact` con su aviso

**Hecho después de la revisión por proveedor (2026-09-23)**

- [x] Preguntas del agente (`AskUserQuestion`) con opciones, paginadas, y
      permisos pendientes paginados («1 / 3») — probado de punta a punta
- [x] Citar o copiar una selección del hilo
- [x] «Pensando…» con brillo y tiempo en décimas
- [x] Bloque de actividad con pestañas por clase
- [x] Esfuerzo en OpenCode y Grok (niveles de sesión ACP) y en Codex por defecto
- [x] Codex: `/compact` y `/review [rama]` por su protocolo
- [x] Errores visibles con pista de credenciales; modelos agrupados por proveedor

**Por agente**

- [ ] Modo de permisos fuera de Claude (Codex, ACP)
- [x] Cursor: `cursor/ask_question` usa la tarjeta de preguntas (ids de opción,
      sin respuesta escrita). Probado con test; en sesión real Cursor no llamó
      a la herramienta en modo agente — falta verlo en modo plan
- [ ] Cursor: `cursor/create_plan` todavía se acepta solo (acp.rs)
- [x] OpenCode: su `question` falla sola por ACP; el chat la ofrece igual y
      la respuesta va como mensaje
- [ ] Variante rápida de Cursor (`fast`) en el selector
- [ ] Modelos de Antigravity en `discover.rs` (Grok los informa la sesión)
- [ ] Servidores MCP en ACP: no los informa, el indicador no aparece
- [ ] Retomar en OpenCode/Cursor cuando ACP lo haga fiable
- [ ] Retomar sesiones del propio CLI (`~/.claude/projects`)
- [ ] Chat remoto por SSH (hoy lo remoto va por consola SSH; ver
      [ssh-remote-hosts.md](ssh-remote-hosts.md))

**Comodidad**

- [ ] Copiar respuesta y botón de copiar en bloques de código
- [ ] Rutas de archivos clicables en las herramientas
- [ ] Encolar un mensaje mientras el agente trabaja
- [ ] Editar y reenviar el último mensaje
- [ ] Dictado → composer
- [ ] Navegación con teclado en los popovers (selector, historial)
- [ ] Hilos largos: virtualizar si pesan

**Limpieza**

- [ ] Borrar `HubSessions.svelte` y `AgentModelsModal.svelte` (sin uso)
- [ ] Fase 0 · T4: quitar `consoleTransfer.*` y los comandos de traspaso
- [ ] Commit por partes: el árbol se comparte con otro trabajo en curso
      (media) y los dos tocan `es.ts` / `en.ts`

## Relacionado

- [dictado.md](dictado.md)
- [liquid.md](liquid.md)
- [ssh-remote-hosts.md](ssh-remote-hosts.md)
- [`docs/PLAN_AGENTES.md`](../docs/PLAN_AGENTES.md)
- Orquestación entre agentes (idea): [orquestacion-agentes.md](orquestacion-agentes.md) · [`docs/PLAN_ORQUESTACION_MCP.md`](../docs/PLAN_ORQUESTACION_MCP.md)
