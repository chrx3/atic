# Editor de grabaciones con agentes de IA

Estado: implementado en macOS (2026-11-01), compila y con tests de la lógica; falta probarlo en la app con un agente de verdad. Que un agente (Claude, OpenCode, Codex, Cursor, Grok…)
**vea la grabación y la edite** con las mismas herramientas del editor, desde un chat dentro
del editor o desde cualquier agente que ya tenga el MCP de Atic.

## Idea

```text
Agente (Claude / OpenCode / Codex / …)
   │  MCP stdio
atic-mcp  (herramientas atic_editor_*)
   │  TCP localhost, una línea JSON por pedido  (editor-bridge.json: puerto + token)
pill GPUI  ── recording_editor/agent_bridge.rs ──► RecordingEditor (hilo de GPUI)
                                                     └─ edit_ops.rs (lógica pura, con tests)
```

- El proyecto del editor (`Project`) ya es JSON, no destructivo y se valida solo
  (`ProjectRaw` acota los números). El agente edita **ese** proyecto: nunca el video.
- Todo lo que hace un agente en un pedido `edit` es **un solo paso de deshacer**
  («Cambios del asistente»): Cmd+Z lo revierte entero.
- El usuario ve los cambios en vivo en la línea de tiempo y la vista previa.
- Los tiempos son siempre **segundos del video original** (los mismos del proyecto).

## Puente (pill ⇄ atic-mcp)

Archivo de descubrimiento: `<data_dir>/editor-bridge.json` con
`{ "port": 51234, "token": "<hex>", "pid": 123 }`, donde `<data_dir>` es el mismo de `hub.json`
(`ProjectDirs::from("com", "ciat", "atic").data_dir()`). En tests, `ATIC_EDITOR_BRIDGE_JSON`
apunta a otro archivo. La pill lo crea al abrir el primer editor y lo borra al cerrar el último
(y al salir); no se borra uno que sea de otro pid.

Un pedido por conexión TCP a `127.0.0.1:<port>`: el cliente manda **una línea** JSON y lee
**una línea** de respuesta; después cierra.

```json
{ "token": "…", "id": 1, "tool": "state", "args": { "video": "/ruta/opcional.mp4" } }
```

Respuesta buena (`images` es opcional; cada imagen va con su texto):

```json
{ "id": 1, "ok": true, "data": { … }, "images": [ { "mime": "image/jpeg", "data": "<base64>", "caption": "0:12.3" } ] }
```

Respuesta mala: `{ "id": 1, "ok": false, "error": { "code": "no_editor", "message": "…" } }`.
Códigos: `no_editor` (no hay editor abierto), `bad_token`, `bad_args`, `bad_op`,
`busy` (una exportación o transcripción en curso impide el cambio), `unavailable`, `internal`.

`args.video` es opcional en todas: sin él se usa el editor que tuvo el foco al último.
La pill responde en **menos de 45 s** (los hosts cortan las herramientas a los 60 s): lo que tarda
más (transcripción, exportación) vuelve con `status: "running"` y `progress`, y se consulta de nuevo.
El cliente tiene un timeout de 60 s.

Sin Atic o sin editor abierto, las herramientas contestan (como error de herramienta, no de
protocolo) con un texto claro: «Abre una grabación en el editor de Atic para poder editarla».

## Herramientas MCP (`atic_editor_*`)

| Herramienta | Pedido del puente | Qué hace |
| --- | --- | --- |
| `atic_editor_list` | `list` | Editores abiertos: `video`, `title`, `duration`, `focused`. |
| `atic_editor_state` | `state` | Duración, tamaño, cabezal, selección, qué hay en el proyecto (recorte, cortes, zooms, textos, velocidades, difuminados, estilo, cursor, exportación) con **índices**, tiempo resultante tras cortes y velocidades, `can_undo`/`can_redo`, estado de exportación y de transcripción. |
| `atic_editor_frames` | `frames` | **Ver el video.** Args: `times: [s…]` (hasta 8) **o** `start`, `end`, `count` (hasta 8, repartidos); `height` (px, 240–720, por defecto 480); `composed` (bool: el cuadro como queda con zoom, fondo y textos; por defecto false). Devuelve imágenes con su tiempo. |
| `atic_editor_transcript` | `transcript` | Lo que se dice, con tiempos: `segments: [{start,end,text}]`. La primera vez lanza la transcripción local (`status: running` + `progress`). |
| `atic_editor_activity` | `activity` | Lo que pasó en pantalla: clics (`t`, `x`, `y` en 0–1), rachas de escritura, tramos quietos y silencios del audio, picos de volumen. Para decidir dónde acercar o cortar sin mirar todo. |
| `atic_editor_edit` | `edit` | Aplica una lista de operaciones (abajo) como un solo paso de deshacer. Devuelve por operación `ok` o `error`, más el `state` resumido. Args: `ops: [...]`, `label` opcional. Con un `error` en una operación **no se aplica ninguna** (todo o nada). |
| `atic_editor_export` | `export` | Exporta con la configuración del proyecto. Args: `wait` (s, hasta 40). Devuelve `status` (`running`/`done`/`failed`), `progress` y la ruta del archivo. |

El texto de `atic_editor_edit` (descripción de la herramienta) lleva el catálogo de operaciones de
abajo, tal cual: está en `crates/atic-mcp/src/editor_ops.md` y se incluye con `include_str!`.

## Operaciones de `edit`

Cada operación es un objeto con `op`. Los tiempos son segundos del original; `rect` y los puntos
de foco van de 0 a 1 (origen arriba a la izquierda).

| `op` | Campos | Efecto |
| --- | --- | --- |
| `trim` | `start`, `end` | Deja solo ese tramo (recorte de inicio y final). |
| `clear_trim` | — | Quita el recorte. |
| `cut` | `start`, `end` | Quita un tramo del medio. |
| `zoom` | `start`, `end`, `scale?` (1,2–4, por defecto 2), `x?`, `y?` | Acerca la cámara. Sin `x`/`y` sigue al cursor; con ellos mira ese punto fijo. |
| `text` | `start`, `end`, `text`, `at?` (`top`/`center`/`bottom`), `size?` (0,5–3), `color?` (`#rrggbb`), `boxed?`, `bold?`, `caption?` | Texto o título por tiempo. |
| `speed` | `start`, `end`, `rate` (1,25–16) | Tramo más rápido. |
| `blur` | `start`, `end`, `rect:[x,y,w,h]`, `pixelate?` | Tapa una zona (datos privados). |
| `update` | `kind` (`cut`/`zoom`/`text`/`speed`/`blur`), `index`, `set:{…}` | Cambia campos de un bloque ya existente (los mismos nombres de arriba). |
| `remove` | `kind`, `index` | Borra un bloque. |
| `remove_all` | `kind` | Borra todos los de ese tipo (`zoom` con `auto_only: true` borra solo los automáticos). |
| `autofocus` | — | Propone zooms según clics y escritura (reemplaza los automáticos anteriores). |
| `speed_up_silences` | `min_len?` (s) | Acelera los tramos sin voz ni movimiento. |
| `cut_silences` | `min_len?` (s) | Corta esos tramos. |
| `camera_style` | `style` | Preajuste de cámara (los nombres salen de `state.camera.styles`). |
| `set` | `look?`, `cursor?`, `camera_opts?`, `motion?`, `export?` | Combina esos valores con los del proyecto (los números se acotan solos). Los nombres son los de `state`. |
| `generate_captions` | — | Subtítulos automáticos (lanza la transcripción si falta). |
| `seek` / `play` / `pause` | `time?` | Mueve o reproduce el cabezal (no cuenta como edición). |
| `undo` / `redo` | — | Deshace o rehace un paso (cuenta como un paso). |

Reglas de validación: tiempos dentro de `0..duración`, `start < end`, bloques de velocidad sin
pisarse, textos y zooms con largo mínimo. Un valor fuera de rango se rechaza con un mensaje que
dice el rango válido (no se acota en silencio, para que el agente aprenda).

## Chat dentro del editor

Un panel «Asistente» (pestaña de la barra lateral) con el campo de escribir, el selector de
agente (los instalados: Claude, Codex, OpenCode, Cursor, Grok) y la conversación resumida
(texto del agente y una línea por herramienta; permisos con Permitir/Rechazar). Reusa el motor de
Atic Code: Claude por el sidecar, el resto por ACP (`code/acp.rs`).

- La sesión corre en la carpeta de la grabación y arranca con un prompt de sistema corto que
  explica el editor y sus herramientas (ver `recording_editor/assistant.rs`).
- Al abrir la sesión se le entrega el servidor `atic-mcp` como `mcpServers` (Claude por el
  sidecar, ACP por `session/new`). Codex y los agentes con el MCP ya instalado en sus archivos
  de configuración (Ajustes → Agentes) lo usan desde ahí.
- Cada turno del agente que cambie el proyecto queda como un paso de deshacer.
- El usuario sigue editando a mano mientras tanto; el agente siempre ve el estado actual.

## Pendiente a futuro

- Cuadro «compuesto» (`composed`) en `frames`: reusa el `Composer` de la vista previa.
- Cámara web, teclas en pantalla y Windows (hoy el editor es de macOS).
