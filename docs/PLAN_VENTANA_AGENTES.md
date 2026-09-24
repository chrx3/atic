# Ventana de agentes como hogar, pill como aviso

**Estado:** en curso (2026-09-23).

## Por qué

Casi todos los problemas del chat y de las consolas venían de dónde vivían:
el overlay es una ventana transparente de pantalla completa pensada para la
pill. Popovers que se cortan, toasts fuera del float, modo texto para
escribir, la isla de 440 px, y toda la maquinaria de traspaso isla ⇄ float ⇄
ventana (claims, `consoleTransfer`, acks) para mover fichas entre tres
anfitriones. Una ventana normal del sistema lo resuelve de una vez.

Decisión del dueño (2026-09-23): desde la pill solo se mira que un agente
terminó; se quiere **enfocar** ese agente, y **aprobar permisos y contestar
preguntas** desde la pill, con varios agentes a la vez.

## Qué se reutiliza y qué se rehace

| Se reutiliza | Se rehace |
|---|---|
| Backend entero: sesiones, adaptadores, PTY en Rust, hub MCP | El contenedor: `AgentsWorkspace` |
| El chat (`AgentChatPanel` y sus piezas) | Navegación: barra lateral de sesiones + historial |
| xterm, extraído a `TerminalView` + `consoleBus` | El traspaso entre anfitriones: se retira |

## Fases

### A — La ventana
- A1 `TerminalView` + `consoleBus` (una PTY por vista, un oyente por ventana).
- A2 Sesiones de la ventana (`agentWorkspace`): chats y terminales en una
  lista, guardada para retomar tras recargar.
- A3 `AgentsWorkspace`: barra lateral (nueva sesión, sesiones vivas con su
  estado, historial) y la sesión elegida al centro.
- A4 La ruta `agents` monta `AgentsWorkspace`.
- A5 «Enfocar sesión X» desde otra webview: `emitTo("agents", …)` tras
  asegurar la ventana, con reintento por si recién se crea.

### B — La pill
- B1 Un chip por sesión de chat (con su id), y clic = enfocar en la ventana.
- B2 Pendientes de todas las sesiones en la pill: permisos, planes y
  preguntas, paginados entre agentes.
- B3 Sincronía entre webviews: lo leído y lo contestado en una ventana se
  refleja en la otra.

### C — Retiro
- Consolas del overlay (float, isla), `AgentLauncher`, `ConsolePanel`,
  traspaso. El botón de agentes de la pill abre la ventana. Recién cuando A y
  B estén probados.

## Bitácora
- **2026-09-23** — A y B implementadas y probadas con la app:
  - Ventana: chat y terminal reales, estados en la barra lateral (lista →
    trabajando → respondió), `agents-focus` desde el overlay selecciona la
    sesión, retomar tras recargar.
  - Pill: chips por sesión, tarjeta de pendientes paginada; el dueño aprobó
    un permiso real desde el notch.
  - Bugs encontrados: `agents_ensure_window` síncrono dejaba la ventana en
    `about:blank` (ahora `async`); el chat sin carpeta corría en la carpeta
    del proceso (ahora la del usuario); «arrancando» se mostraba como
    «trabajando» (`isWorking`).
  - Falta: C (retiro de consolas del overlay y del traspaso).
