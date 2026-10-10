# Migración de la referencia a Atic Code

Este documento sirve para traspasar el trabajo entre agentes. Con él se puede retomar sin contexto previo.

**Qué es.** la referencia es un IDE con Tauri y SolidJS. Está pensado alrededor del chat con Claude Code (Claude Agent SDK). Atic Code es su port a GPUI (Rust) y vive dentro de la pill de Atic. Se abre desde la bandeja (entrada «Consolas», `src/main.rs:2059`) o, solo, con `CODE_ALONE=1`. `CODE_DEMO=1` agrega una conversación de ejemplo.

**Dónde está cada repo.**

| Repo | Ruta | Qué mirar |
| --- | --- | --- |
| la referencia (origen) | `C:\Users\Lenovo\Documents\referencia` | UI en `src/` (`components`, `components/chat`, `lib`, `styles`), núcleo en `src-tauri/src/`, agente en `sidecar/agent.mjs` |
| Atic Code (destino) | `C:\Users\Lenovo\Documents\atic\prototypes\pill-gpui` | `src/code/*.rs` y `sidecar/agent.mjs`. Reutiliza `src/space/` (espacios, árbol, visor, consolas) |
| gpui-m3 (componentes) | `C:\Users\Lenovo\Documents\gpui-m3` | `src/components/*.rs`, `src/motion.rs`, `src/shapes.rs`, `src/theme.rs`, README, CHANGELOG y `docs/INTEGRATING.md`. Atic lo usa como dependencia por ruta (`Cargo.toml:72`) |

**Hasta dónde se portó.**
- El port inicial (commit `c4eac77` de Atic) llega hasta el commit `dc11fe8` de la referencia.
- El hotfix `3cd87bb` de la referencia («hotfix modelos») ya está portado en `b1a3efc` (rama `dev` de Atic). Con él, cada conversación tiene su modelo y su esfuerzo, existen `sessionSettings`/`lastSettings`, se compara con `same_model`, aparecen las marcas «Cambiado a X» y «X en el próximo mensaje», y hay un aviso cuando responde otro modelo.
- El HEAD de la referencia es `3cd87bb`. Antes de cada tanda, revisa `git -C ..\referencia log dc11fe8..HEAD`.

**Flujo de trabajo.**
1. Un agente *revisor* lee la referencia y pide una tanda concreta. Indica qué función, en qué `archivo:línea` de la referencia y cuál es el criterio para darla por terminada.
2. Un agente *implementador* hace la tanda:
   - Los componentes genéricos van a **gpui-m3**, en un worktree o rama propia, con su ejemplo en la galería y su entrada en el CHANGELOG.
   - La lógica de la app va a **Atic Code** (`src/code/`).
3. Cada tanda se prueba sola. Para el sidecar, `node sidecar/build.mjs`. Para Rust, `cargo check`/`test -p` del crate. Después se ve en la app con `CODE_ALONE=1`. Luego se actualiza este documento: cambia el estado de la fila y su `archivo:línea`.

**Cómo leer las tablas.**
- Estados: **Hecho** · **Parcial** · **Falta** · **No aplica** · **En curso** (otro agente la está implementando ahora).
- Las rutas de la referencia son relativas a su raíz. Las de Atic Code son relativas a `prototypes/pill-gpui`. Las de gpui-m3 llevan el prefijo `gpui-m3/`.
- Los `archivo:línea` de Atic Code corresponden a `c4eac77`. Después de `b1a3efc` (+484/−61 líneas en `mod.rs`, `chat.rs`, `config.rs`, `view.rs`, etc.) pueden estar desplazados unas decenas de líneas. Busca por el nombre de la función.
- Casi todo lo «Hecho» lo está en el estilo **Expressive** (M3). Formal y Liquid Glass usan funciones propias más simples en `view.rs` y `menus.rs`.

---

## 1. Chat / hilo

| Función de la referencia | Estado en Atic Code | gpui-m3 | Nota |
| --- | --- | --- | --- |
| Burbuja del usuario (`src/components/chat/Thread.tsx:100`) | Hecho `src/code/view.rs:1098` | existe `Bubble` (`gpui-m3/src/components/surfaces.rs:1039`) | |
| Miniaturas de las imágenes adjuntas en el mensaje del usuario (`Thread.tsx:105`) | Falta (`Item::User(String)` no guarda imágenes, `chat.rs:53`) | **En curso**: `ImageThumb` (gpui-m3, tanda 1) | Las imágenes se mandan (`mod.rs:775`), pero no se ven |
| Copiar y Marcar en el mensaje del usuario (`Thread.tsx:112`) | **En curso** (tanda 2: marcas) | existe `Button` (`confirm`, `label_on_hover`) | |
| Markdown del asistente (`Thread.tsx:116`) | Parcial `view.rs:405` | falta crearlo: `Markdown` | Solo títulos, viñetas, código, `código` en línea y negrita |
| Copiar la respuesta («Copiado» durante 1,4 s) (`Thread.tsx:69`) | Hecho `view.rs:1130` | `Button::confirm` | Solo cuando la respuesta terminó |
| Marcar mensaje (flag por sesión) (`Thread.tsx:82`, `src/lib/flags.ts:21`) | **En curso** (tanda 2) | existe `Button::selected` | Se guarda en un json propio (ver Decisiones) |
| Fondo de mensaje marcado (`Thread.tsx:103,123`) | **En curso** (tanda 2) | — | |
| «Siguiente marcado» con scroll y destello (`flags.ts:44-54`) | **En curso** (tanda 2) | falta: `FlashHighlight` | |
| Contador de marcas (`flags.ts:29`) | **En curso** (tanda 2) | — | |
| Ruta en `código` en línea que se puede abrir con un clic (`Thread.tsx:44,124`) | Falta | falta: rangos clicables en StyledText (`InteractiveText`/`LinkText`) | `open_doc` ya existe (`mod.rs:1109`) |
| Razonamiento plegable (`Thread.tsx:18`) | Hecho `view.rs:1142` | `ExpandableCard::plain` | |
| «Razonando…» en vivo, con la última línea (`Thread.tsx:21,29`) | Falta | existe `ExpandableCard::preview` (`disclosure.rs:84`) | `Item::Thinking` no sabe si sigue llegando |
| Ocultar el razonamiento vacío o cifrado (`Thread.tsx:137`) | Parcial `chat.rs:372` | — | El razonamiento por streaming que queda vacío se dibuja igual |
| Aviso informativo (`Thread.tsx:146`) | Hecho `view.rs:1103` | `Bubble` Notice | |
| Aviso de error (`Thread.tsx:146`) | Parcial `view.rs:1246` | existe `BubbleKind::Error` | Expressive no lo usa |
| Separador «Contexto compactado» (`Thread.tsx:152`, `src/lib/agent.ts:450`) | **En curso** (tanda 2: compactación) | **En curso**: `DividerLabel` | Falta `compact_boundary` |
| Marca «Cambiado a X» (`Thread.tsx:158`, `agent.ts:808`) | Hecho (`b1a3efc`, `Item::Model`, `chat.rs`) | `DividerLabel` cuando exista | Hoy dibujado a mano |
| Marca «X en el próximo mensaje» (`Thread.tsx:170,234`) | Hecho (`b1a3efc`, `Chat::pending_model`) | ídem | |
| Marcas de modelo al cargar el historial (`agent.ts:697-737`) | Falta | — | Solo se generan al enviar |
| Aviso «responde otro modelo» y reaplicarlo (`agent.ts:782`) | Hecho (`b1a3efc`, `Chat::check_model`) | — | |
| Seguir el final, soltarlo al subir y retomarlo cerca del fondo (`Thread.tsx:16,194,206`) | Hecho `view.rs:967`, `mod.rs:471` | — | |
| Ir al final al cambiar de conversación (`Thread.tsx:220`) | Hecho `mod.rs:682` | — | |
| Animación al cambiar de conversación (`Thread.tsx:227`) | Falta | falta: `motion::swap`/FadeThrough | |
| Entrada animada de cada parte nueva (`src/lib/motion.ts:43-57`) | Falta | existe `Bubble::entrance` (`surfaces.rs:1083`) | Basta con llamarlo, con un máximo de 3 a la vez |
| Indicador «Trabajando…» (`Thread.tsx:241,253`) | Parcial `view.rs:907` | `LoadingIndicator` | la referencia lo oculta mientras llega texto o razonamiento |
| Permisos apilados al final del hilo (`Thread.tsx:261`) | Hecho `view.rs:904` | — | Formal y Glass muestran solo el primero |
| Streaming start/delta/stop (`agent.ts:215`) | Hecho `chat.rs:282` | — | |
| Cambiar el texto del stream por el del mensaje final (`agent.ts:285-291`) | Parcial `chat.rs:366` | — | Si llegó por stream, el texto final se ignora |
| Ignorar ecos y mensajes internos (`agent.ts:332`) | Hecho `chat.rs:148,402` | — | |
| Mensaje desde otro cliente (Remote Control) (`agent.ts:348`) | Hecho `chat.rs:408` | — | Usa `isReplay`; la referencia deduplica por uuid |
| uuid y cadena de mensajes para Rewind (`agent.ts:168-175,339`) | **En curso** (tanda 2) | — | |
| Rewind/fork de conversación y código (`agent.ts:504`, `src/components/Agent.tsx:390-405`) | **En curso** (tanda 2) | — | El sidecar ya tiene `rewindFiles`/`resumeSessionAt`/`forkSession` |
| Título tomado del primer mensaje (`agent.ts:653`) | Hecho `chat.rs:205` | — | 60 caracteres (la referencia usa 80) |
| «Detenido.» al interrumpir (`agent.ts:441`) | **En curso** (tanda 2) | — | Hoy muestra el subtype como error |
| Error del resultado (`agent.ts:442`) | Hecho `chat.rs:421` | — | Solo el primer error |
| Herramientas que siguen `running` pasan a error si el turno falla (`agent.ts:439`) | Falta | — | Quedan girando para siempre |
| Herramientas sin resultado en el historial se dan por hechas (`agent.ts:743`) | Falta | — | |
| `closed` con error (`agent.ts:480`) | Hecho `chat.rs:243` | — | |
| Error de start o de send (`agent.ts:636,657`) | Hecho `mod.rs:764,816` | — | |
| Resync al seguir en otro cliente (evento `external`) (`agent.ts:415,541`) | **En curso** (tanda 2) | — | El sidecar ya lo emite (`sidecar/agent.mjs:411`) |
| Costo y tokens acumulados (`agent.ts:425`) | Hecho `src/code/usage.rs:113` | — | |
| Fila de duración y costo del turno | Extra de Atic: `Item::Turn` (`chat.rs:431`, `view.rs:1237`) | — | Se conserva (Decisiones) |
| Tareas y subagentes (`agent.ts:454-478`) | Hecho `usage.rs:124-160` | — | |
| No leído al terminar (`agent.ts:675`) | Hecho `mod.rs:466` | — | |
| Recargar el archivo abierto cuando Claude lo edita (`agent.ts:327,356`) | **En curso** (tanda 2: recarga del visor) | — | `viewer.rs:81 reload()` existe, pero no se llama |
| Refrescar git al terminar (`src/App.tsx:16`) | Hecho (tanda 3): `refresh_changes_now` en cada `result` (`mod.rs`), además del sondeo cada 3 s | — | |
| Notificación del sistema: terminó, falló, pide permiso (`src/lib/notify.ts:33`, `agent.ts:413,446`) | Falta | No aplica | La pill/notch de Atic puede mostrarla |
| Renombrar desde el título de la barra superior (`src/components/Chat.tsx:42-75`) | Hecho (tanda 3): `start_header_rename` (`sidebar.rs`), título con lápiz en `view.rs` | `TextField::inline` | Solo con la sesión creada; guarda al perder el foco |
| Barra superior: proyecto, pestañas Cambios (badge) y Archivos (`Chat.tsx:76-111`) | Hecho `view.rs:830-858` | `Button` + `Badge` | Falta el botón Terminal |
| Pantalla de inicio: formas, «¿Qué construimos hoy?», proyecto, sugerencias (`Chat.tsx:180-222`) | Hecho `view.rs` (`hero`) | `Shape`, `Chip` | «Chat sin proyecto» en el selector (tanda 3) |
| Subida escalonada de los hijos de la pantalla de inicio (`src/styles/motion.css:391-408`) | Falta | `entrance` con retardo | |
| Conversación de demo (`agent.ts:873`) | Hecho `src/code/demo.rs:11` | — | |
| Simulación de Tauri para el navegador (`src/lib/preview.ts`) | No aplica | — | |

### Markdown (`src/components/chat/Markdown.tsx`)

| Función de la referencia | Estado en Atic Code | gpui-m3 | Nota |
| --- | --- | --- | --- |
| GFM con `marked` (`Markdown.tsx:141`) | Parcial `view.rs:405` | falta: `Markdown` | Faltan listas numeradas o anidadas, citas, `---`, cursiva y tachado |
| Tablas | Falta | falta: `Markdown` (tablas) | |
| Enlaces que abren el navegador (`Markdown.tsx:160`) | Falta | falta: rangos clicables | |
| Imágenes | Falta | falta | |
| Bloque de código con lenguaje y «Copiar» (`Markdown.tsx:144-155`) | Parcial `view.rs:469` | falta: `CodeBlock` | Muestra la etiqueta cruda y no cambia a «Copiado» |
| Resaltado de sintaxis (rust/ts/js/json/md/css/html/py) (`Markdown.tsx:14-37,97`) | Falta | falta: `SyntaxHighlighter` (lo comparte con `CodeEditor`) | No hay syntect ni tree-sitter en `Cargo.toml` |
| Alias de lenguaje a nombre legible (`Markdown.tsx:43-88`) | Falta | dentro de `CodeBlock` | |
| `código` en línea y **negrita** | Hecho `view.rs:356,385` | — | Con tests |
| Títulos por nivel | Parcial `view.rs:453` | — | Todos de 15 px |
| Saneado de HTML (`Markdown.tsx:159`) | No aplica | — | |
| Caché durante el streaming (`Markdown.tsx:94,190`) | No aplica | — | |
| Seleccionar texto | Falta (GPUI no la trae) | falta: `SelectableText` | Se compensa con «Copiar» |

## 2. Composer

| Función de la referencia | Estado en Atic Code | gpui-m3 | Nota |
| --- | --- | --- | --- |
| Caja que crece hasta 240 px (`Agent.tsx:1098,1311`) | Hecho en Expressive `view.rs:1625`; Parcial en Formal/Glass (fija en 60 px, `view.rs:1524`) | existe `TextArea` | Atic usa su propio `crate::text_area` |
| Placeholder «Pregunta… · @ · /» (`Agent.tsx:1314`) | Parcial `view.rs:37,160` | — | Menciona la «@», que todavía no funciona |
| «/» solo abre el menú (`Agent.tsx:1319`) | Hecho `mod.rs:1000` | — | |
| Enter envía y Mayús+Enter baja de línea (`Agent.tsx:1352`) | Hecho `mod.rs:67` | — | |
| Esc interrumpe la respuesta en curso (`Agent.tsx:1336`) | Hecho `mod.rs:952` | — | Antes cierra los menús abiertos |
| Esc Esc abre Rewind (`Agent.tsx:1342`) | **En curso** (tanda 2, con Rewind) | — | |
| Adjuntar con el diálogo (`Agent.tsx:86,1360`) | Hecho `mod.rs:883` | `IconButton` | |
| La imagen va como imagen y el resto como ruta (`Agent.tsx:93-112`) | Hecho `mod.rs:863` | — | |
| Máximo 10 imágenes (`Agent.tsx:1110`) | Falta | — | |
| Archivos sin duplicar (`Agent.tsx:1113`) | Hecho `mod.rs:875` | — | |
| Ctrl+U adjunta (`Agent.tsx:1140`) | Hecho `mod.rs:78` | — | |
| Pegar una imagen (`Agent.tsx:1202`) | Hecho `mod.rs:904` | — | |
| Pegar archivos que no son imagen (`savePasted`) (`Agent.tsx:1189`) | Hecho (tanda 3): `paste` lee `CF_HDROP` (`clip_image::read_files`) | — | En Windows el Explorador da rutas: se adjuntan sin copiar (Decisiones) |
| Arrastrar desde el Explorador, con «Suelta para adjuntar» (`Agent.tsx:1127,1208,1257`) | Falta | **En curso**: `DropZone` | `on_drop::<ExternalPaths>` |
| Miniaturas de imágenes, con quitar (`Agent.tsx:1295`) | Parcial: ícono, no la miniatura (`view.rs:1461`) | **En curso**: `ImageThumb` | |
| Chips de archivos, con quitar (`Agent.tsx:1280`) | Hecho `view.rs:1464` | `Chip::input` | |
| Chip de contexto del editor (archivo y líneas) (`Agent.tsx:1117,1264`) | Falta | `Chip::input` | Depende del editor |
| Sufijo «(Archivos adjuntos: @rel)» (`Agent.tsx:1149`) | Parcial `mod.rs:114` | — | Manda la ruta absoluta |
| Texto por defecto «Mira la imagen adjunta.» (`Agent.tsx:1150`) | Falta | — | |
| Enviar mientras responde (cola) (`Agent.tsx:1377-1388`) | **En curso** (tanda 2) | — | Hoy `mod.rs:784` lo descarta; el sidecar lo encolaría |
| Detener (`Agent.tsx:1385`) | Hecho `view.rs:1635` | `IconButton` | |
| Despegue del botón al enviar (`Agent.tsx:1156`) | Hecho `view.rs:1646` | `IconButton::launch` | |
| @-menciones: índice, ranking difuso, popover, ↑↓ Enter Tab Esc (`Agent.tsx:990-1025,1161-1186,1330`) | Falta | **En curso**: `Autocomplete` | El índice puede salir de `space::explorer` con `.gitignore` |
| `insert()` agrega al final del borrador (`Agent.tsx:1102`) | Hecho (tanda 3): `insert` + `append_draft` (`mod.rs`, con test) | — | Las sugerencias del inicio siguen reemplazando (`prefill`) |
| Pedir el contexto al cambiar de chat (`Agent.tsx:1075`) | Parcial (después de `result`, `mod.rs:469`) | — | |
| Botón de modelo con esfuerzo (`Agent.tsx:1366`) | Hecho (`b1a3efc`, `chat_model`/`chat_effort`) | `Button::sublabel` (podría ser `SplitButton`) | |
| Botón «/» (`Agent.tsx:1363`) | Hecho `view.rs:1665` | `IconButton` | |
| Estado «Conectando…» (`Agent.tsx:1371`) | Hecho (tanda 2, `Chat::starting`) | — | |
| Anillo de contexto que abre «Cuenta y uso» (`Agent.tsx:1374`) | Hecho `usage.rs:234` | `Ring` | Solo en Expressive |
| Chip de modo que cicla (`Agent.tsx:1094,1393`) | Hecho `mod.rs:985` | `Chip` + `MorphDot` | |
| Chip de duración (`Agent.tsx:1047,1396`) | Hecho `view.rs:1717` | `Chip` | No tiene reloj de 30 s |
| Chip «N agentes» (`Agent.tsx:1059,1402`) | Hecho `usage.rs:255` | `Chip::pulse_icon` | |
| Chip Remote Control (`Agent.tsx:1411`) | Hecho `usage.rs:269` | `Chip` + `MorphDot` | Atic lo aplica solo a las conversaciones del espacio |
| Línea de estado: modelo · contexto % · $ · s (`Agent.tsx:1427`) | Hecho (tanda 3): `status_line` (`mod.rs`, con test), en los tres estilos | — | `Chat::last_duration_ms` sale de `durationMs` del `result` |
| Cerrar al hacer clic fuera (`Agent.tsx:1084`) | Hecho `menus.rs:124` | — | |
| Popover hacia el lado con más espacio, con tope (`Agent.tsx:193-217`) | Parcial: siempre arriba (`menus.rs:114`) | — | |
| Salida animada de los popovers (`Presence`, `src/components/ui.tsx:10`) | Falta | **En curso**: `Presence` | |
| Oferta del Artifact publicado (`Agent.tsx:1438-1492`) | Falta | **En curso**: `Banner` | No se detecta la herramienta Artifact |
| Aviso de conversación larga (`Agent.tsx:1494-1545`) | Falta | **En curso**: `Banner` | Va junto con la compactación |
| Entrada de los chips (`m3-chip-in`, `motion.css:706`) | Falta | parcial: `Chip::input` sin entrada | |

## 3. Menú del agente (`AgentMenu`, `Agent.tsx:225-667`)

| Función de la referencia | Estado en Atic Code | gpui-m3 | Nota |
| --- | --- | --- | --- |
| Filtro «acciones o comandos» (`Agent.tsx:450`) | Hecho `mod.rs:304`, `agent_menu.rs:195` | `TextField` | |
| Seis pestañas con ícono que recuerdan la última (`Agent.tsx:225-234,461`) | Hecho `agent_menu.rs:18,207`, `mod.rs:172` | `IconTabs` | |
| Filtrar en todas las secciones (`Agent.tsx:357,501`) | Hecho `agent_menu.rs:239` | `Menu::section` | |
| Comandos «/» que coinciden (hasta 8) y «Sin resultados» (`Agent.tsx:379,480,498`) | Hecho `agent_menu.rs:214-254` | — | |
| Adjuntar archivo… ⌘U (`Agent.tsx:258`) | Hecho `agent_menu.rs:120` | `MenuItem::shortcut` | |
| Mencionar archivo… @ (`Agent.tsx:259`) | Falta | depende de `Autocomplete` | |
| Limpiar conversación (`Agent.tsx:260`) | Hecho `agent_menu.rs:121` | — | |
| Rewind: Código, Conversación o Ambos (`Agent.tsx:261,389-409,636`) | **En curso** (tanda 2) | `Chip` | Falta `Sub::Rewind` |
| Marcador (`Agent.tsx:262`) | **En curso** (tanda 2) | — | |
| Siguiente mensaje marcado (`Agent.tsx:270`) | **En curso** (tanda 2) | — | |
| Exportar conversación (`Agent.tsx:120-137,281`) | Parcial `agent_menu.rs:583` | — | No incluye las herramientas |
| Copiar enlace de Remote Control (`Agent.tsx:282`) | Hecho `agent_menu.rs:123` | — | |
| Cambiar modelo…, con el valor actual (`Agent.tsx:293`) | Hecho (`b1a3efc`) | — | |
| Esfuerzo en línea (`Agent.tsx:294,542`) | Hecho `agent_menu.rs:161` | `StopSlider` | Sin elegir dice «Predeterminado» (ver Decisiones) |
| Ultracode / Thinking / Modo rápido (`Agent.tsx:295-298`) | Hecho `agent_menu.rs:128-131` | `Switch` | |
| Cambiar de modelo al marcar (`Agent.tsx:297`) | Hecho (el interruptor) `agent_menu.rs:130` | `Switch` | Tendrá efecto cuando lleguen las marcas (tanda 2) |
| Cuenta y uso… (`Agent.tsx:299`) | Hecho (tanda 3): `Act::Usage` en la pestaña Modelo | `Popover` | |
| Estilo de salida, Permisos, MCP, Hooks (`Agent.tsx:300-303`) | Hecho `agent_menu.rs:134-137` | — | Hooks abre `settings.json` con el programa del sistema |
| Subagentes y Comandos con contador (`Agent.tsx:304-305`) | Hecho `agent_menu.rs:138-139` | — | |
| Memoria · CLAUDE.md, Sandbox, Línea de estado, Instrucciones, Configuración general (`Agent.tsx:306-310`) | Hecho `agent_menu.rs:140-144` | `Switch` | |
| Claude in Chrome (`Agent.tsx:311`, `src/lib/store.ts:405`) | Parcial `agent_menu.rs:147` | `Switch` | No reinicia el chat vacío |
| Remote Control en la terminal, Claude Design, Abrir Claude en la terminal (`Agent.tsx:312-334`) | Hecho `agent_menu.rs:148-153` | — | Usa `wt.exe` externo. Claude Design agrega `/design ` al final (tanda 3) |
| Reanudar conversación… (`Agent.tsx:326`) | Hecho (tanda 3): `Act::Resume` en la pestaña Sesión | — | |
| Compactar contexto (`Agent.tsx:327`) | Hecho `agent_menu.rs:152` | — | Pide una sesión viva |
| Administrar/recargar plugins, cambiar de cuenta, cerrar sesión (`Agent.tsx:335-344`) | Hecho `agent_menu.rs:154-157` | — | |
| Submenú con «atrás» (`Agent.tsx:441,562`) | Hecho `agent_menu.rs:265` | `Menu::back` | |
| Submenú Modelo: radios con descripción y «Más modelos» (`Agent.tsx:413-421,569`) | Hecho (`b1a3efc` agregó `same_model`) | `MenuItem::radio` | «Más modelos» queda al final, no entre los grupos |
| Submenús Permisos y Estilo de salida (`Agent.tsx:422-431`) | Hecho `agent_menu.rs:309-330` | — | |
| Submenús Subagentes y Comandos (`Agent.tsx:432-435`) | Hecho (tanda 3): agregan con `insert()` | — | |
| Submenú MCP: estado, Reconectar, Activar/Desactivar (`Agent.tsx:347,608`) | Hecho `agent_menu.rs:361-442` | `Chip` | |
| Menú en Formal y Glass | Parcial: versión reducida (`menus.rs:282-361`) | — | |
| Ítems de menú escalonados de a 25 ms (`motion.css:628`) | Parcial (por grupo, 35 ms) | `surfaces.rs:386`; falta escalonar por ítem | |

## 4. Modelos y esfuerzo

| Función de la referencia | Estado en Atic Code | gpui-m3 | Nota |
| --- | --- | --- | --- |
| Modelos de respaldo y `models()` desde meta (`Agent.tsx:37-43`) | Hecho `config.rs:37`, `mod.rs:508` | — | |
| `splitModels` (`Agent.tsx:46`) | Hecho `config.rs:42` (con test) | — | |
| `modelLabel` / `modelName` (`Agent.tsx:61`, `agent.ts:799`) | Hecho (`b1a3efc`, `model_name` en `config.rs`) | — | Revisa que «Default (recommended)» pase a «Predeterminado» (`mod.rs:521`) |
| `sameModel` (`agent.ts:816`) | Hecho (`b1a3efc`) | — | |
| `want`, `setChatWant`, `chatModel`/`chatEffort` (`agent.ts:130,819`, `store.ts:368`) | Hecho (`b1a3efc`, `Chat::want_*`, `chosen_model`/`chosen_effort`) | — | |
| `adoptSessionSettings` / `sessionSettings` (`agent.ts:826`, `sidecar/agent.mjs:330,514`) | Hecho (`b1a3efc`) | — | |
| `syncChatSettings` + `applied` (`agent.ts:762`, `store.ts:427`) | Hecho (`b1a3efc`, `Applied`) | — | Falta la reversión si falla (`agent.ts:770`) |
| `checkModel` (`agent.ts:782`) | Hecho (`b1a3efc`) | — | |
| `noteModelForSend` / `sentModel` (`agent.ts:808`) | Hecho (`b1a3efc`) | — | |
| `reviveFor`/`forkAt` con `want` (`store.ts:567,689`) | `reviveFor`: Hecho (`b1a3efc`). `forkAt`: **En curso** (Rewind) | — | |
| PERMISSIONS y EFFORTS (`Agent.tsx:66`, `ui.tsx:90`) | Hecho `config.rs:12,20` | — | |
| `EffortControl` por estilo: puntos, paradas, perilla (`ui.tsx:101`) | Parcial: `StopSlider` en M3 y puntos en Formal (`menus.rs:165`) | `StopSlider` | Glass no tiene la perilla |
| `ClaudeSettings` global o por proyecto (`src/lib/api.ts:11`, `store.ts:348`) | Hecho por espacio (`config.rs:155`) | — | Sin espacio activo no se guarda nada |
| Valores por defecto: opus / medium / thinking / acceptEdits (`store.ts:334`, `src-tauri/src/workspace.rs:97`) | No aplica: se siguen los de Atic (Decisiones) | — | |

## 5. Permisos y preguntas

| Función de la referencia | Estado en Atic Code | gpui-m3 | Nota |
| --- | --- | --- | --- |
| Elegir la tarjeta: general, preguntas o plan (`src/components/chat/Permission.tsx:55`) | Hecho `permissions.rs:423` | `Card` | |
| Título por herramienta (`Permission.tsx:14`) | Hecho `permissions.rs:337` | — | |
| Cuerpo Bash `$ comando` (`Permission.tsx:115`) | Hecho `view.rs:2362` | — | |
| Cuerpo NotebookEdit `new_source` (`Permission.tsx:121`) | Falta (muestra JSON) | — | |
| Cuerpo Edit/MultiEdit/Write con diff (`Permission.tsx:124`) | Parcial `permissions.rs:523` | falta: `Diff` | El diff es falso; MultiEdit muestra JSON |
| Notas: descripción, motivo, ruta bloqueada (`Permission.tsx:129`) | Hecho `permissions.rs:463` | — | |
| «Decirle a Claude qué hacer en cambio» (`Permission.tsx:140`) | Hecho `permissions.rs:525` | `TextField` | Solo en la primera tarjeta |
| Permitir / Permitir siempre / Permitir todo / Rechazar y responder (`Permission.tsx:74-160`) | Hecho `permissions.rs:461-514` | `Button` | |
| Tooltip con las reglas en «Permitir siempre» (`Permission.tsx:152`) | Parcial | existe `Tooltip` | |
| Foco inicial según `defaultToNo` (`Permission.tsx:90`) | **En curso** (tanda 2: teclado en permisos) | falta: foco inicial en `Button` | `chat.rs` descarta `defaultToNo` |
| Teclado: Enter permite, Esc rechaza, Enter en el campo rechaza con texto (`Permission.tsx:92`) | **En curso** (tanda 2) | — | Esc rechaza (Decisiones) |
| Preguntas: chip de encabezado, opciones radio/checkbox, «Otro…» (`Permission.tsx:224-272`) | Hecho `permissions.rs:589-701` | `ChoiceRow`, `Badge`, `TextField` | |
| Enfocar «Otro» lo selecciona (`Permission.tsx:269`) | Falta | — | |
| Enviar desactivado hasta completar y Cancelar como rechazo (`Permission.tsx:183-205`) | Hecho `permissions.rs:586-666` | — | |
| Teclado en preguntas: Esc, Enter/Ctrl+Enter, foco en la primera (`Permission.tsx:207`) | **En curso** (tanda 2) | — | |
| Plan: «Claude terminó de planificar» y sus tres opciones con campo (`Permission.tsx:316-337`) | Hecho `permissions.rs:533-568` | `Card`, `Button` | |
| Teclado en el plan (`Permission.tsx:303`) | **En curso** (tanda 2) | — | |
| `permission_cancel` y responder (`agent.ts:421,670`) | Hecho `chat.rs:233`, `permissions.rs:401` | — | |
| Tarjeta en Formal/Glass (`view.rs:1343`) | Parcial | — | Sin «Permitir todo», sin campo y solo la primera |

## 6. Herramientas y diffs (`src/components/chat/Tools.tsx`, `Diff.tsx`)

| Función de la referencia | Estado en Atic Code | gpui-m3 | Nota |
| --- | --- | --- | --- |
| Armazón: estado, verbo, objetivo, meta, chevron (`Tools.tsx:124`) | Hecho `tools.rs:162-238` | `ExpandableCard` | |
| Girando / ✓ / ✗ (`Tools.tsx:102`) | Hecho `tools.rs:170` | `LoadingIndicator` | |
| Ícono de «terminado» que salta (`motion.ts:87`) | Falta | falta: `Icon::pop` (base en `Badge::pop`) | |
| El objetivo abre el archivo (`Tools.tsx:176`) | Falta | — | `open_doc` |
| Tooltip con la ruta, el comando o la URL completos (`Tools.tsx:171`) | Falta | existe `Tooltip` | |
| Enter/Espacio despliega la tarjeta (`Tools.tsx:158`) | Falta | **En curso**: teclado en `ExpandableCard` | |
| Despliegue con resorte en altura (`motion.ts:68-83`) | Parcial: solo se funde (`tools.rs:298`) | falta: altura animada en `ExpandableCard` | |
| Tarjeta fija y abierta de entrada (`Tools.tsx:136,143`) | Hecho `tools.rs:290` | `.fixed()`/`.open()` | |
| Badges y +/− (`Tools.tsx:207`) | Hecho `tools.rs:190` | `Badge` | |
| Quitar `<system-reminder>` (`Tools.tsx:28`) | Hecho `tools.rs:16` | — | |
| Salida recortada con «Mostrar todo» (`Tools.tsx:79`) | Hecho `tools.rs:111` | falta: `CodeOutput` | Atic recorta a 6000 caracteres (`chat.rs:18`) |
| Read con rango L… (`Tools.tsx:396`) | Parcial `tools.rs:178` | — | Error de uno en `from+limit` |
| Edit con «todas» y diff (`Tools.tsx:420`) | Parcial `tools.rs:188`; diff falso (`view.rs:2376`) | falta: `Diff` | Se corta en 40 líneas sin avisar |
| MultiEdit: un diff por edición (`Tools.tsx:67`) | Parcial: muestra JSON | `Diff` | |
| Write: todo agregado, abierta si tiene ≤ 40 líneas (`Tools.tsx:64,424`) | Parcial `tools.rs:36` | `Diff` | |
| NotebookEdit (`Tools.tsx:313`) | Parcial: muestra JSON | — | |
| Bash/PowerShell: descripción, en segundo plano, `$`, salida en rojo (`Tools.tsx:454`) | Hecho `tools.rs:42,203,251` | `Badge` | |
| Grep/Glob, WebFetch, WebSearch (`Tools.tsx:328-364`) | Hecho `tools.rs:47-54,257-264` | — | |
| Task/Agent con las últimas 5 herramientas hijas y «N herramientas» (`Tools.tsx:479`, `agent.ts:196,224,273`) | Parcial: falta registrar los hijos (`chat.rs:288` los descarta) | existe `ExpandableCard::under` | |
| TodoWrite (`Tools.tsx:512`) | Hecho `tools.rs:220`, `view.rs:2316` | — | |
| ExitPlanMode y AskUserQuestion como tarjetas (`Tools.tsx:375,380`) | Hecho `tools.rs:270,274` | — | |
| Artifact: verbo, título, «Abrir» (`Tools.tsx:261`, `src/lib/artifacts.ts`) | Falta | `Button` | |
| MCP «servidor · acción» y genérica JSON (`Tools.tsx:538-554`) | Hecho `tools.rs:70,275` | — | |
| Diff: números de línea viejo/nuevo (`Diff.tsx:14,116`) | Falta en el chat (el visor git sí, `view.rs:1974`) | falta: `Diff` | Reutilizar `space::viewer::parse_diff` |
| Diff: «⋯ N líneas sin cambios», «Mostrar todo» pasadas 400 filas, encabezado, archivo nuevo (`Diff.tsx:42,97-128`) | Falta | `Diff` | |
| Diff: estadísticas con un diff de líneas real (`Diff.tsx:31`) | Parcial: aproxima (`tools.rs:102`) | — | Sumar el crate `similar` |

## 7. Sidebar, espacios y sesiones (`src/components/Sidebar.tsx`)

| Función de la referencia | Estado en Atic Code | gpui-m3 | Nota |
| --- | --- | --- | --- |
| Barra abierta/cerrada y botón para ocultarla (`App.tsx:88`, `Sidebar.tsx:366`) | Hecho `sidebar.rs:68,83` | `NavItem`/`RailItem` | |
| Botón buscar/comandos (`Sidebar.tsx:369`) | Hecho `sidebar.rs:80` | `IconButton` | |
| «Nueva conversación» con ⌘N a la vista (`Sidebar.tsx:375`) | Parcial: no muestra el atajo (`sidebar.rs:96`) | `Fab` | |
| Historial (`Sidebar.tsx:380`) | Hecho `sidebar.rs:97` | `NavItem` | |
| Menú «Proyectos +»: nuevo, abrir, carpeta, importar VS Code (`Sidebar.tsx:387`, `src/components/Overlays.tsx:37-63`) | Parcial: el botón abre «Nuevo espacio» (tanda 3); abrir carpeta va por Ctrl+O y la paleta | `Menu` | Falta importar VS Code. Ver Decisiones (espacios) |
| Diálogo «Nuevo workspace» con nombre y carpetas (`Overlays.tsx:411-483`) | Hecho (tanda 3): `new_space_dialog` (`sidebar.rs`) | `Dialog`, `TextField`, `Button` | Las carpetas con el diálogo nativo, no `space/picker.rs` (Decisiones) |
| Orden: favoritos primero y luego el del usuario (`store.ts:130-144`) | Hecho (tanda 3): `config::sidebar_order` (con test) | — | «El del usuario» es el orden de la lista hasta que exista reordenar |
| Reordenar arrastrando (`Sidebar.tsx:313-351`) | Pendiente: `TODO(gpui-m3)` en `sidebar.rs` | falta: `ReorderList`/`DragHandle` | |
| Estrella de favorito animada (`Sidebar.tsx:152-176`) | Hecho (tanda 3), en la fila y en el menú contextual del espacio | `FavStar` | Favoritos en `code-claude.json` |
| Plegar el proyecto y recordarlo (`store.ts:145`) | Hecho `sidebar.rs:156`, `src/space/workspaces.rs:184` | `NavItem` | |
| Desplegar al abrir el proyecto la primera vez (`store.ts:153`) | Hecho (tanda 3): `expand_first_open` + `Configs::opened` | — | |
| Avatar del proyecto que gira 40° al hover (`Sidebar.tsx:212`, `motion.css:770`) | Parcial: avatar sin giro (`sidebar.rs:152`) | falta: `Avatar::hover_spin` | |
| «+» al pasar el cursor, carga diferida, últimas 3 y «Ver todas (n)» (`Sidebar.tsx:184-258`) | Hecho `sidebar.rs:21,143-187` | `IconButton` | |
| Conversaciones vivas primero; fila activa; indicador «respondiendo»; punto de no leído; «hace X» (`Sidebar.tsx:55-137`) | Hecho `sidebar.rs:172-233` | `LoadingIndicator`, `Badge::dot` | |
| No leído cuando otro cliente retoma la conversación (`store.ts:508`) | **En curso** (tanda 2: resync) | — | |
| Entrada escalonada de las conversaciones (`motion.css:727`) | Falta | `entrance` | |
| Ícono de marcador en la fila (`Sidebar.tsx:125`) | **En curso** (tanda 2: marcadores) | — | |
| Menú contextual: Abrir, Renombrar en el sitio, Eliminar con confirmación (`Sidebar.tsx:89-115`) | Hecho; guarda al perder el foco (tanda 3, `commit_renames_on_blur`) | `MenuItem::danger().confirm()`, `TextField::inline` | |
| Menú contextual: Agregar/Quitar marcador (`Sidebar.tsx:91`) | **En curso** (tanda 2) | — | |
| Chats sueltos, sin proyecto (`Sidebar.tsx:277-305`, `store.ts:453,575`) | Hecho (tanda 3): sección «Chats» (`loose_chats`, cinco y «Ver todos») | — | Solo en la barra de Expressive (Decisiones) |
| Pie: perfil, Apariencia, Configuración (`Sidebar.tsx:423`) | Hecho (tanda 3): Apariencia abre su pestaña (`open_appearance`) | `IconButton` | El perfil sigue sin tarjeta (tanda 16) |
| Riel M3 con su avatar, que abre la barra (`Sidebar.tsx:434-467`) | Hecho (el avatar abre la barra desde la tanda 3) | `RailItem`, `Fab`, `Avatar` | |
| Página Historial con búsqueda (`Chat.tsx:316-346`) | Hecho `sidebar.rs:345` | `TextField` | |
| Historial: filtro «Marcadores» (`Chat.tsx:335`) | **En curso** (tanda 2) | `Chip` (filtro) | |
| Historial: rama git de cada sesión (`Chat.tsx:274`) | Hecho (tanda 3): `SessionInfo::branch` de `gitBranch` | — | «hace X · rama» |
| Página y filas del historial escalonadas (`motion.css:784`) | Falta | `entrance` | |
| Máximo 4 procesos vivos; cierra los inactivos (`store.ts:462`) | Hecho (tanda 3): `limit_live` + `idle_to_close` (con test) | — | Cierra el proceso, no la conversación (Decisiones) |
| Cerrar las conversaciones precalentadas vacías (`store.ts:599`) | No aplica (Atic no precalienta) | — | |
| Título de la ventana «proyecto — Atic Code» (`store.ts:101`) | Hecho (tanda 3), en `render`; «Chats — Atic Code» en un chat suelto | — | |
| Avisos al abrir: importado, carpetas que faltan (`store.ts:102`) | Falta | `Toast` | |
| Quitar un espacio y agregarle carpetas | Hecho (tanda 3): menú contextual del espacio en Expressive (`space_menu_layer`) | `Menu`, `MenuItem::confirm` | También Renombrar y Favorito |
| `listSessions`, `renameSession`, `deleteSession` (`agent.ts:833`) | Hecho `mod.rs:556`, `sidebar.rs:537,559` | — | Límite de 40 (la referencia usa 50) |

## 8. Panel de contexto, archivos y git (`src/components/ContextPanel.tsx`)

| Función de la referencia | Estado en Atic Code | gpui-m3 | Nota |
| --- | --- | --- | --- |
| Panel derecho con Cambios/Archivos, título, contador y cerrar (`ContextPanel.tsx:347-383`) | Hecho `view.rs:1782-1841` | `Badge` | |
| Actualizar con spinner (`ContextPanel.tsx:361`) | Hecho (tanda 3): `refreshing` + `LoadingIndicator` | `LoadingIndicator` | Al pedirlo y al terminar cada respuesta, no en el sondeo |
| Árbol con las raíces como carpetas, carga diferida (`ContextPanel.tsx:27-69`) | Hecho `view.rs` (`file_tree`), `src/space/explorer.rs:80` | `TreeRow` en Expressive (tanda 3) | |
| Color del nombre según el tipo; dotfiles atenuados; archivo activo resaltado (`ContextPanel.tsx:18-60`) | Hecho (tanda 3): `files::kind_of` + `kind_color`; el activo es el último abierto (`active_file`) | `TreeRow` | Como la referencia, se pinta el ícono. Formal/Glass: solo los dotfiles atenuados |
| Ocultar node_modules, target y dist (`src-tauri/src/fs.rs`) | Parcial: solo oculta `.git` (`explorer.rs:47`) | — | |
| Buscar archivos con índice (120 resultados) (`ContextPanel.tsx:71-131`) | Hecho (tanda 3): `src/code/files.rs` (`index`, `search`, con tests) | `TextField`, `TreeRow` | El índice se reutilizará en las @-menciones; Esc limpia |
| Cambios agrupados por repo, con rama (`ContextPanel.tsx:267-305`) | Hecho `view.rs:2218,2242`, `src/code/git.rs:55` | `Card`/`ListGroup` | |
| Grupo plegable; «Al día»; «No es un repositorio git» (`ContextPanel.tsx:268-297`) | Hecho (tanda 3): `repo_group` (`view.rs`), `Repo::is_repo` (`git.rs`) | `ExpandableCard` en Expressive | Formal/Glass: encabezado propio que se pliega |
| «Todo al día» con ícono (`ContextPanel.tsx:318`) | Hecho (tanda 3) | — | Solo si alguna carpeta es un repo (Decisiones) |
| Resumen n archivos +a −r (`ContextPanel.tsx:330`) | Hecho `view.rs:1946` | — | |
| Fila de cambio: estado, nombre, carpeta, +/− (`ContextPanel.tsx:239`) | Hecho `view.rs` (`change_row`) | `ListItem` | Los no seguidos salen como `N` (tanda 3) |
| Barra de 5 bloques del diff (`ContextPanel.tsx:225`) | Falta | falta: `DiffBar` | |
| Ver el diff de un cambio y «Abrir» el archivo (`ContextPanel.tsx:190-222`) | Hecho `view.rs:1820,1974,2285` (`space::viewer`) | `Diff` cuando exista | |
| Volver del diff a la lista (`ContextPanel.tsx:153,202`) | Parcial: la X cierra el archivo | `IconButton` | |
| Panel más ancho al editar (`ContextPanel.tsx:351`) | Hecho `view.rs:1784` | — | |
| Commit, ramas, stage, descartar | No aplica (la referencia tampoco los tiene) | — | |

## 9. Terminal (`src/components/Terminal.tsx`, `src/lib/terminals.ts`, `src/styles/terminal.css`)

En Atic Code no hay terminal integrada: «Abrir Claude en la terminal» lanza `wt.exe` (`agent_menu.rs:569`). Las consolas de `src/space/console.rs` (alacritty + ConPTY, teclado con AltGr y tildes, paleta ANSI, scroll, título y «terminó») ya cubren casi todo. Se decidió reutilizarlas (Decisiones).

| Función de la referencia | Estado en Atic Code | gpui-m3 | Nota |
| --- | --- | --- | --- |
| Panel bajo el chat (`Terminal.tsx:184-288`, `Chat.tsx:359`) | Falta (existe `space::console::Console`, `console.rs:102`) | No aplica (la consola es de Atic) | |
| Pestañas: nueva «+», cerrar/matar, número (`terminals.ts:11-35`, `Terminal.tsx:217-273`) | Falta | existe `TabStrip` (`on_close`) | |
| Renombrar la pestaña con doble clic (máx. 40) (`Terminal.tsx:226-264`) | Falta | `TabStrip` (`editing`/`on_rename`) | |
| «[proceso terminado]» y título según el shell (`Terminal.tsx:126-131`) | Falta (existe `console.rs:236,240`) | — | |
| Ejecutar un comando al abrir (`claude --resume`) (`terminals.ts:18`) | Parcial: `wt.exe` externo | — | |
| Mostrar/ocultar con ⌃\` y ⌘J (`terminals.ts:50`, `App.tsx:21,37`) | Falta | — | |
| Botón Terminal con contador en la barra superior (`Chat.tsx:82`) | Falta | `Button` + `Badge` | |
| Alto ajustable y recordado (`Terminal.tsx:182-209`) | Falta | falta: `ResizeHandle`/`Splitter` | `space::panes` tiene divisores |
| Paleta ANSI por estilo y modo (`Terminal.tsx:19-88`) | Falta en code (existe `console.rs:310-369`) | — | |
| Cursor, scrollback de 5000, reajuste y foco (`Terminal.tsx:94-157`) | Falta en code (existe `console.rs:218,227`) | — | |
| Entrada por estilo: `m3-rise`, `glass-condense` (`terminal.css:176,217`) | Falta | `entrance` | |

## 10. Editor de código (`src/components/CodeEditor.tsx`, `src/lib/editor.ts`)

Hoy hay solo un visor de solo lectura (`src/space/viewer.rs`). Se decidió construir el editor como `CodeEditor` en gpui-m3, con resaltado (Decisiones).

| Función de la referencia | Estado en Atic Code | gpui-m3 | Nota |
| --- | --- | --- | --- |
| Abrir desde el árbol (`store.ts:744`) | Parcial: un archivo a la vez (`mod.rs:1109`) | — | |
| Pestañas de archivos abiertos (`ContextPanel.tsx:149-175`) | Falta | existe `TabStrip` (con `dirty`) | |
| Confirmar al cerrar si hay cambios sin guardar (`ContextPanel.tsx:135`) | Falta | `Dialog` | |
| Editar (CodeMirror) (`CodeEditor.tsx:86-191`) | Falta | falta: `CodeEditor` | Es lo más grande |
| Números de línea (`CodeEditor.tsx:113`) | Hecho en el visor (`view.rs:2012`) | — | |
| Resaltado por lenguaje (`CodeEditor.tsx:40-84`) y colores `--sx-*` (`src/styles/tokens.css:55`) | Falta | falta: `SyntaxHighlighter` | |
| Guardar con Mod-S (`CodeEditor.tsx:96`) | Falta | dentro de `CodeEditor` | |
| Deshacer, plegado, multicursor, corchetes, sangría (`CodeEditor.tsx:113-129`) | Falta | dentro de `CodeEditor` | |
| Recargar si Claude editó y no hay cambios propios (`editor.ts:28`) | **En curso** (tanda 2: recarga del visor) | — | |
| Cursor y selección como contexto para Claude (`editor.ts:10`, `CodeEditor.tsx:132`) | Falta | `CodeEditor` emite la selección | Alimenta el chip de contexto del composer |
| Archivos grandes, borrados o ilegibles | Hecho (mejora de Atic, `viewer.rs:84`) | — | |

## 11. Overlays y paleta (`src/components/Overlays.tsx`, `src/components/ContextMenu.tsx`)

| Función de la referencia | Estado en Atic Code | gpui-m3 | Nota |
| --- | --- | --- | --- |
| Paleta ⌘K/⌘P con flechas, Enter, Esc y filtro (`Overlays.tsx:528-599`) | Hecho `src/code/palette.rs:24-71` | `CommandPalette` (**En curso**: ratón) | |
| Comandos: nueva conversación, historial, configuración, actualizar Claude, ver cambios/archivos, 30 conversaciones, estilo y modo (`Overlays.tsx:496-523`) | Hecho `palette.rs:43-61` | — | |
| Comando: nuevo chat sin proyecto (`Overlays.tsx:497`) | Hecho (tanda 3): `PaletteAct::NewLoose` | — | |
| Comandos: nuevo, abrir o importar workspace (`Overlays.tsx:498-501`) | Parcial: «Nuevo espacio…» y «Abrir carpeta…» (tanda 3) | — | Falta importar VS Code |
| Comandos: guardar, guardar como, cerrar workspace (`Overlays.tsx:510-512`) | No aplica (los espacios se guardan solos) | — | |
| Comando: abrir pestañas de archivos (`Overlays.tsx:513`) | Falta | — | Depende del editor |
| Toast de 3,2 s (`store.ts:39`) | Hecho `agent_menu.rs:613`, `view.rs:193` | `Toast` | |
| Menú contextual ajustado a la ventana, que cierra con Esc, clic fuera o blur (`ContextMenu.tsx:17-84`) | Hecho `sidebar.rs:468` | `context_menu()` (Atic usa `anchored` a mano) | No cierra cuando la ventana pierde el foco |
| Salidas animadas de menús, diálogos y toasts (`motion.css:548-595`) | Falta | **En curso**: `Presence` | |
| Diálogo: velo y crecimiento desde el centro con radio 56 (`motion.css:598`) | Parcial `settings_m3.rs:147` | `Dialog` (solo `pop_in` de 12 px) | Falta la escala con cambio de forma |
| Snackbar que sube y crece (`motion.css:615`) | Parcial `view.rs:202` | `Toast` | |

## 12. Perfil y uso (`src/components/Profile.tsx`, `src/lib/profile.ts`, `Agent.tsx:676-979`)

| Función de la referencia | Estado en Atic Code | gpui-m3 | Nota |
| --- | --- | --- | --- |
| Perfil local con nombre y foto (`profile.ts:5-33`) | Falta | — | Se construye editable (Decisiones) |
| Nombre de git como valor inicial (`profile.ts:36`) | Parcial: siempre el de git (`sidebar.rs:585`) | — | |
| Iniciales (`profile.ts:43`) | Hecho `sidebar.rs:579` | `Avatar` | |
| Avatar galleta que cambia de forma al hover (`Profile.tsx:15-28`, `src/styles/app.css:4749`) | Parcial: Cookie9 fijo (`sidebar.rs:275`) | **En curso**: `Avatar` con imagen y hover-morph | |
| Avatar con foto (`Profile.tsx:17`) | Falta | **En curso** (ídem) | |
| Tarjeta de perfil: editar el nombre, cambiar o quitar la foto (`Profile.tsx:207-254`) | Falta | `Popover`, `TextField`, `Button` | |
| Recortar la foto: arrastrar, zoom, máscara, Enter/Esc (`Profile.tsx:55-182`) | Falta | falta: `ImageCropper` | |
| Popover «Cuenta y uso»: tokens, plan, entrada/salida/caché, costo (`Agent.tsx:713-818`) | Hecho (tanda 3): `usage_windows` lee `rate_limits.limits` (con tests) y si no, las ventanas viejas | `Popover`, `Ring`, `WavyProgress` | |
| Mapa de agentes (`Agent.tsx:851-979`) | Hecho (tanda 3): `agent_map` + `task_row` (`usage.rs`), `task_kind` y `Chat::task_label` con tests | `MorphDot`, `LoadingIndicator`, `IconButton` | Conserva la fila de la conversación principal (extra de Atic) |
| Plan de la cuenta (`Overlays.tsx:246`) | Hecho `settings_m3.rs:25`, `usage.rs:536` | — | |

## 13. Ajustes y apariencia

| Función de la referencia | Estado en Atic Code | gpui-m3 | Nota |
| --- | --- | --- | --- |
| Elegir el estilo Formal, M3 o Glass (`src/lib/theme.ts:160`) | Hecho `style.rs:18`, `mod.rs:1016` | — | Atic parte en Expressive |
| Modo claro/oscuro/sistema (`theme.ts:166`) | Hecho (tanda 3): «Sistema» sigue a Windows (`style::set_system_light`) | — | |
| Seguir en vivo el cambio del sistema (`theme.ts:118`) | Hecho (tanda 3): `observe_window_appearance` | — | Sin probar en la app |
| Tarjetas de estilo con miniatura (`Overlays.tsx:68`) | Hecho (tanda 3) en Expressive | `SelectCard` | Formal/Glass siguen con chips |
| Segmentado «Modo de color» (`Overlays.tsx:219`) | Hecho en M3 (`settings_m3.rs:359`); chips en Formal y Glass | `SegmentedButtons` | |
| Acento: 9 sugeridos, propio, restablecer (`Overlays.tsx:97-136`, `theme.ts:46`) | Hecho (tanda 3): `accent_picker_ui` (`settings_m3.rs`), también en la Configuración de Formal/Glass | `ColorSwatches`, `HsvPicker`, `Button` | |
| Acento por proyecto (`Overlays.tsx:138`, `store.ts:316`) | Hecho (tanda 3): `Configs::accents` por espacio y estilo | — | No hay acento global de la app (Decisiones) |
| Esquema M3 desde la semilla (`theme.ts:72`) | Hecho (tanda 3): `style::with_accent` y `apply_m3` | `Scheme::from_seed` | gpui-m3 usa la especificación de color de 2021 y la referencia la de 2025 |
| Acento simple de Formal/Glass: tono HCT 62/52, `accent-soft`, `sel` (`theme.ts:57`) | Hecho (tanda 3): `style::simple_accent` (con test, crate `material-colors`) | — | |
| Menú rápido de Apariencia (`Overlays.tsx:157`) | Parcial: abre Configuración en la pestaña Apariencia (tanda 3) | `Popover` | Falta el popover propio |
| Configuración con pestañas Claude y Apariencia (`Overlays.tsx:173-229`) | Hecho en M3 (`settings_m3.rs:86`); Parcial en Formal/Glass (otra pantalla, `view.rs:2043`) | `Dialog`, `NavItem::large` | |
| Tarjeta de Claude Code: versión, estado, actualizar, registro (`Overlays.tsx:249-356`) | Hecho `settings_m3.rs:159-290` | `Shape`, `Card` | Usa `v != l` en vez de `newer(a,b)`; no reinicia el chat vacío después de actualizar |
| Instalación y Cuenta (`Overlays.tsx:358-398`) | Hecho `settings_m3.rs:257,294` | `ListGroup` | |
| Movimiento reducido según el sistema (`motion.ts:11`, `motion.css:1374`) | Hecho (tanda 3): `set_reduced_motion` con `UISettings.AnimationsEnabled` | `MotionSettings.reduced` | Global de la app (Decisiones) |
| Fuentes y tamaño por estilo (`tokens.css:17-19,96,191`) | Parcial `style.rs:100,141,215` | `theme::FONT_FAMILY` | Atic usa 13,5 en todos y otras fuentes |
| Línea de estado (`Agent.tsx:164`) | Parcial (ver Composer) | — | |
| Guardar el tema (`theme.ts:12`) | Hecho en otro formato (`code-claude.json`) | — | |
| Vista previa `?style=&mode=` (`theme.ts:24`) | No aplica | — | |

## 14. Estilos M3 / motion (`src/styles/expressive.css`, `motion.css`, `glass.css`, `app.css`)

| Función de la referencia | Estado en Atic Code | gpui-m3 | Nota |
| --- | --- | --- | --- |
| Colores de Formal y M3, claro y oscuro (`tokens.css:34-151`) | Hecho `style.rs:105-219` | `theme.rs:85,118` | |
| Colores de Glass (`tokens.css:209,254`) | Parcial `style.rs:257,295` | — | `raised` más denso; faltan `--hi-edge`, `--spec`, `--wall-*` |
| Radios, `--gap`, paneles flotantes (`tokens.css:22-27`, `expressive.css:31-41`) | Hecho `style.rs:133`, `view.rs:184,954` | `theme::corner` | |
| `--pad`, `--stroke`, `--fs-code` (`tokens.css:28,31`) | Falta | — | |
| `--shadow-pop` por estilo (`tokens.css:54,141,244`) | Parcial: una sola sombra (`view.rs:260`) | `shadow_pop` | |
| Resortes M3 (`expressive.css:12-21`) | Hecho (vía gpui-m3) | `Spring::*` (`motion.rs:26`) | |
| Ripple M3 (`motion.css:313`) | Hecho | `interaction.rs:52` | |
| Indicador activo con resorte, pestañas de ícono, rail, FAB (`motion.css:357`, `expressive.css:627-909`) | Hecho `sidebar.rs:215,312`, `agent_menu.rs:207` | `nav.rs`, `button.rs:691` | Falta la entrada del rail |
| Formas que respiran, indicador de carga, progreso ondulado (`expressive.css:111-228`) | Hecho `view.rs:1018`, `usage.rs:387` | `visual.rs` | |
| Despegue al enviar; radio del botón 18→12→8 (`motion.css:468`, `expressive.css:373`) | Hecho `view.rs:1641` | `button.rs:468,506` | |
| Burbujas con cola; superficies de 28 px; menú en una sola superficie; botón de modelo que se cuadra (`expressive.css:497-708`) | Hecho | `Bubble`, `Menu`, `Button::open` | |
| Botón dividido del modelo (`expressive.css:311-370`) | Parcial: usa `Button::open` | existe `SplitButton` | |
| Configuración: cajón, lista segmentada, tarjeta de Claude (`expressive.css:914-1131`) | Hecho `settings_m3.rs` | `ListGroup`, `NavItem::large` | Falta el contenido escalonado (`expressive.css:989`) |
| Punto de no leído que salta; pulgar del switch (`motion.css:694,809`) | Hecho | `Badge::pop`, `Switch` | |
| Interpolación de colores al cambiar tema o acento (`motion.css:9-84`) | Falta | falta: `motion::animate_scheme` | |
| Transición de tema: revelado circular M3 (650 ms) y fundidos de Formal y Glass (`theme.ts:137`, `app.css:4842-4902`) | Falta | falta: `theme_reveal` (en la hoja de ruta) | |
| Composer que «respira» al enfocar (`motion.css:411`, `expressive.css:502-560`) | Parcial: cambia el fondo sin animar (`view.rs:1680`) | `animate_color` | |
| Burbujas con resorte desde su esquina (`motion.css:425`) | Falta | `Bubble::entrance` | Basta con llamarlo |
| Panel y barra lateral con resorte (`motion.css:444`) | Falta | falta: `slide_in(dir)` | |
| Muestras de color que giran a rombo; pulso de la tarjeta de estilo (`motion.css:495-532`) | Hecho con los componentes (tanda 3) | `ColorSwatches`/`SelectCard` | Lo que animen viene de gpui-m3 |
| Separador de compactación (`motion.css:673`) | **En curso** | `DividerLabel` | |
| Filas que se encogen al presionar (`motion.css:752`) | Parcial: solo redondean | — | GPUI no escala |
| Título del hero con peso y anchura animados (`motion.css:378`) | No aplica (GPUI no anima `font-stretch`) | — | |
| Estrella de favorito M3 (giro + estallido) (`app.css:3931`) | Hecho (tanda 3) | `FavStar` | |
| Zona para soltar (`app.css:4430`) | Falta | **En curso**: `DropZone` | |
| Formal: barra de acento en lo activo, anillo de foco, fade-in/fade-up, spinner (`motion.css:105-126,289`, `app.css:156`) | Falta | — | Al final (Decisiones) |
| Glass: luz ambiental, reflejo que sigue al cursor, gota de selección, luz interior, gelatina, materializar, cápsula de la barra, borde especular, bordes que se desvanecen, resortes propios (`glass.css`, `motion.ts:140-252`, `motion.css:821-1362`) | Falta | No aplica: va en un módulo aparte de Atic | Al final (Decisiones) |
| Glass: `backdrop-filter` dentro de la ventana (`glass.css:63`) | No aplica (GPUI no desenfoca dentro de la ventana) | — | La ventana usa Acrylic (`mod.rs:262`) |
| Indicador deslizante del segmentado en Formal y Glass (`motion.css:252-286`) | Falta | — | Módulo de estilos de Atic |

## 15. Sidecar (`sidecar/agent.mjs`)

Sin contar `3cd87bb`, los dos sidecars son idénticos línea a línea, comentarios incluidos. `b1a3efc` ya copió `lastSettings`/`sessionSettings`. Lo que falta está en el **cliente Rust**, que no usa todo lo que el sidecar ya ofrece.

| Función de la referencia | Estado en Atic Code | gpui-m3 | Nota |
| --- | --- | --- | --- |
| Protocolo JSON por stdin/stdout, `stream` cada 30 ms, `streamingInput` (`sidecar/agent.mjs:29-84`) | Hecho (mismo código; `src/code/sidecar.rs:115`) | No aplica | |
| `start`/`startSession` (`agent.mjs:88-186`) | Parcial `mod.rs` (`ensure_live`), `config.rs:111` | No aplica | Rust no manda `title`. `createCwd` va en los chats sueltos (tanda 3); `resumeSessionAt` y `forkSession`, en el Rewind |
| `canUseTool` → `permission` (`agent.mjs:105-130`) | Hecho `chat.rs:77` | — | Descarta `toolUseID` y `defaultToNo` |
| Eventos `meta`, `closed`, `error`, `session`, `assistant`, `user`, `result`, `init` (`agent.mjs:151-282`) | Hecho `mod.rs:432`, `chat.rs:218-416` | — | `init` no toma `permissionMode` |
| Evento `remote` (`agent.mjs:191`) | Parcial: solo guarda `url` (`mod.rs:455`) | — | |
| Evento `rate_limit` (`agent.mjs:266`) | Falta: se ignora; se usa el método `usage` | — | |
| Evento `system` → `compact_boundary` (`agent.mjs:283`) | **En curso** (tanda 2) | — | |
| Vigilar la sesión desde fuera → `external` (`agent.mjs:377-444`) | Sidecar Hecho; cliente **En curso** (tanda 2) | — | |
| `send`, `close`, `permission`, `setPermissionMode`, `applyFlags`, `setThinking` (`agent.mjs:457-497`) | Hecho `mod.rs` | — | |
| `interrupt` (`agent.mjs:466`) | Parcial: sin «Detenido.» (**En curso**) | — | |
| `setModel` (`agent.mjs:493`) | Hecho (`b1a3efc`, por conversación) | — | |
| `rewindFiles` (`agent.mjs:498`) | **En curso** (tanda 2) | — | |
| `usage`, `context`, `account`, `mcp*`, `reloadPlugins`, `remoteControl`, `stopTask`, `backgroundTasks` (`agent.mjs:499-508`) | Hecho `usage.rs`, `agent_menu.rs` | — | |
| `listSessions`, `deleteSession`, `sessionMessages`, `renameSession` (`agent.mjs:509-513`) | Hecho | — | |
| `sessionSettings` + `lastSettings` (`agent.mjs:330-359,514`) | Hecho (`b1a3efc`) | — | |
| `claudeInfo`, `claudeUpdate` (`agent.mjs:515-562`) | Hecho `settings_m3.rs:46-84` | — | |
| `ping`, `sessionInfo`, `other`, `ready` | No aplica (la referencia tampoco los usa) | — | |
| `fatal` (`agent.mjs:589`) | Hecho `mod.rs:442` | — | |

## 16. Rust / núcleo (`src-tauri/src/`)

| Función de la referencia | Estado en Atic Code | gpui-m3 | Nota |
| --- | --- | --- | --- |
| `agent_call` + proceso Node, reinicio, `sidecar_exit`, stderr (`src-tauri/src/agent.rs:67-147`) | Hecho `src/code/sidecar.rs:67-173` | No aplica | Revisar la ruta empaquetada: `sidecar.rs:67` busca `exe/sidecar/agent.mjs` y `build.mjs` genera `dist/agent.mjs` |
| `login_env` (PATH en macOS) (`agent.rs:33`) | No aplica (Windows) | — | |
| `workspace_open`/`read`/`open_folder`/`create`/`save` (`lib.rs:24-50`, `workspace.rs`) | Hecho de otra forma: `src/space/workspaces.rs` (`space-workspaces.json`) | — | Ver Decisiones |
| `from_vscode` (importar `.code-workspace`) (`workspace.rs:138`) | Falta | — | Baja prioridad |
| `recent_list`/`recent_remove` (`lib.rs:57`, `recent.rs`) | No aplica (la lista de espacios hace de recientes) | — | |
| `fs_list_dir` (`lib.rs:67`) | Hecho `src/space/explorer.rs:39` | — | |
| `fs_read_text` (`lib.rs:72`) | Hecho `src/space/viewer.rs:64` (límite de 4 MB) | — | |
| `fs_write_text` (`lib.rs:92`) | Parcial: exportar y CLAUDE.md (`agent_menu.rs:549,587`) | — | El editor lo necesita |
| `fs_read_base64` (`lib.rs:87`) | Hecho `mod.rs:863` | — | El perfil también lo necesita |
| `fs_save_pasted` (`lib.rs:82`) | Parcial (solo imágenes) | — | |
| `fs_index` con `.gitignore` (`lib.rs:108`) | Hecho (tanda 3): `src/code/files.rs` (crate `ignore`) | — | Para las @-menciones y la búsqueda de archivos |
| `user_name` (`lib.rs:77`) | Hecho `sidebar.rs:585` | — | |
| `git_status` (`lib.rs:97`) | Hecho `src/code/git.rs:55` | — | |
| `git_at_head` (`lib.rs:114`) | Hecho de otra forma: `git diff HEAD` (`viewer.rs:115`) | — | |
| `terminal_spawn`/`write`/`resize`/`kill`, `kill_all` (`lib.rs:119-182`, `terminal.rs`) | Falta en code; existe `space::console` | — | |
| Chats sueltos: `createCwd`, `~/.referencia/chats` (`store.ts:453-583`) | Hecho (tanda 3): `LOOSE`, `loose_dir` (`<datos de Atic>\pill\code-chats`), `LOOSE_CONTEXT` | — | |

## 17. Atajos

| Función de la referencia | Estado en Atic Code | gpui-m3 | Nota |
| --- | --- | --- | --- |
| ⌘K y ⌘P paleta, ⌘N nueva, ⌘B barra, ⌘U adjuntar, ⌘, configuración (`App.tsx:30-40`) | Hecho con Ctrl (`mod.rs:73-78`) | — | |
| ⌘L nueva conversación, salvo en el editor (`App.tsx:39`) | Hecho (tanda 3) con Ctrl+L | — | Cuando haya terminal y editor, no debe actuar dentro de ellos |
| ⌘O abrir workspace (`App.tsx:33`) | Hecho (tanda 3) con Ctrl+O: abre carpetas como espacio nuevo | — | |
| ⌘E Archivos / ⌘G Cambios (`App.tsx:35-36`) | Hecho (tanda 3) con Ctrl+E y Ctrl+G | — | |
| ⌘J y ⌃\` terminal (`App.tsx:21,37`) | Falta | — | Junto con la terminal |
| Esc cierra la paleta, la apariencia y la configuración (`App.tsx:26`) | Hecho `mod.rs:952` | — | |
| Esc con un permiso pendiente lo rechaza (`Permission.tsx:92`) | **En curso** (tanda 2) | — | Decisión tomada |
| Esc Esc abre Rewind (`Agent.tsx:1342`) | **En curso** (tanda 2) | — | |
| @: ↑↓ Enter Tab Esc (`Agent.tsx:1330`) | Falta | `Autocomplete` | |
| Enter y Esc al renombrar una sesión (`Sidebar.tsx:103`) | Hecho (`restore_on_cancel`) | `TextField` | |
| Enter y Esc al renombrar una terminal; Esc en la búsqueda de archivos; Enter y Esc en el recorte y la tarjeta de perfil (`Terminal.tsx:251`, `ContextPanel.tsx:96`, `Profile.tsx:127,215`) | Parcial: Esc en la búsqueda de archivos (tanda 3) | — | Lo demás, junto con cada función |
| Enter o Espacio despliega la fila del proyecto (`Sidebar.tsx:207`) | Falta | — | |
| Editor: Mod-S, Tab, deshacer, plegar (`CodeEditor.tsx:129`) | Falta | `CodeEditor` | |

---

## Siguiente

Prioridad de arriba hacia abajo. Cada tanda es chica y se prueba sola. **[m3]** es un componente nuevo en gpui-m3 (en su worktree, con ejemplo en la galería). **[code]** es lógica de Atic Code.

**Hecho:** `3cd87bb` (hotfix modelos), portado en `b1a3efc`.

**Hecho, tanda 3 [code]** (rama `dev`, un commit por letra): A composer, B chats sueltos, C barra lateral, D panel de cambios y archivos, E apariencia y F cuenta y uso con el mapa de agentes. Cubre las tandas 12, 14, 15 y 17 de abajo, casi toda la 13 y parte de la 5 y la 19. Validado con `cargo check` y `cargo test code::` (51 tests); **no se probó en la app** (`CODE_ALONE=1`). Lo que quedó de esas tandas:
- Tanda 5: el popover hacia donde haya más espacio.
- Tanda 13: reordenar arrastrando (`TODO(gpui-m3)`: `ReorderList`/`DragHandle`).
- Tanda 14: **[m3]** `DiffBar`.
- Tanda 15: el menú rápido de Apariencia como popover propio; `SelectCard` y el árbol con `TreeRow` solo en Expressive.
- Tanda 17: el evento `rate_limit` en vivo (sigue en la tanda 23).
- Tanda 19: el teclado en las filas de proyecto.
- Chats sueltos en Formal y Glass: sin sección en su barra (se abren desde la paleta).

**En curso ahora:**
- **Tanda 2 [code]:** uuids y Rewind (Código/Conversación/Ambos, Esc Esc, `forkAt` con `want`); marcas y marcadores (json propio de Atic Code, fila, historial, «Siguiente marcado»); resync `external` (con no leído); «Detenido.»; cola de envío; teclado en permisos, preguntas y plan (Esc rechaza); compactación (`compact_boundary` + separador); «Conectando…»; recarga del visor cuando Claude edita.
- **Tanda m3-1 [m3]:** teclado en `ExpandableCard`, ratón en `CommandPalette`, `DividerLabel`, `Banner`, `Presence`, `DropZone`, `Avatar` con imagen y hover-morph, `ImageThumb` y `Autocomplete`.

**Pendientes:**

3. **[code] Usar la tanda m3-1** (cuando se integre):
   - `DividerLabel` en compactación y en «Cambiado a X».
   - `Presence` en menús, popovers, diálogos y toasts.
   - `DropZone` en el composer (`on_drop::<ExternalPaths>`).
   - `ImageThumb` en las miniaturas del composer y de la burbuja (que `Item::User` guarde las imágenes).
   - `Banner` en el aviso de conversación larga y en la oferta de Artifact (detectar la herramienta Artifact y su tarjeta).
4. **[code] Pulido del hilo y las herramientas:**
   - Herramientas `running` pasan a error si el turno falla; las que no tienen resultado en el historial se dan por hechas.
   - Ocultar el razonamiento vacío y mostrar «Razonando…» con `preview()`.
   - Usar `BubbleKind::Error`.
   - Corregir el error de uno en el rango de Read.
   - Tooltips con la ruta, el comando o la URL; el objetivo de la herramienta abre el archivo.
   - Llamar a `Bubble::entrance` en cada parte nueva (máximo 3).
   - Registrar los hijos de un subagente para Task (las últimas 5 y «N herramientas»).
5. **[code] Detalles del composer:**
   - Máximo 10 imágenes; «Mira la imagen adjunta.» por defecto; rutas relativas en el sufijo.
   - Pegar archivos que no son imagen.
   - Que `insert()` agregue al final (subagentes, comandos y Claude Design).
   - Línea de estado con contexto % y $.
   - «Cuenta y uso…» y «Reanudar…» en el menú; el popover hacia donde haya más espacio.
6. **[m3] `Diff` + `CodeOutput`**: números de línea viejo/nuevo, tramos plegados, «Mostrar todo» y encabezado con +/−. Partir de `space::viewer::parse_diff`.
   **[code]** Usarlo en Edit, MultiEdit, Write y en las tarjetas de permiso; estadísticas con el crate `similar`; NotebookEdit.
7. **[m3] `Markdown` + `CodeBlock`**: listas numeradas y anidadas, citas, tablas, enlaces y rangos clicables, cursiva y tachado; bloque con el nombre del lenguaje y «Copiado».
   **[code]** Reemplazar `view.rs:405`; las rutas en `código` abren el archivo.
8. **[m3] `SyntaxHighlighter`** (syntect o tree-sitter, con los colores `--sx-*` por estilo). Usarlo primero en `CodeBlock` y en el visor de solo lectura.
9. **[code] @-menciones y búsqueda de archivos**: índice que respete `.gitignore` (equivalente a `fs_index`) + `Autocomplete`; «Mencionar archivo… @» en el menú; búsqueda en el panel Archivos; ocultar node_modules, target y dist.
10. **[code] Terminal integrada**: panel bajo el chat con `space::console` + `TabStrip` (nueva, cerrar, renombrar, «terminado», título); Ctrl+J y Ctrl+\`; botón con contador en la barra superior; «Abrir Claude en la terminal» y Remote Control dentro de ella.
    **[m3]** `ResizeHandle`/`Splitter` para el alto.
11. **[m3] `CodeEditor`** (sobre `SyntaxHighlighter`): edición, deshacer, Tab, Mod-S, plegado, selección.
    **[code]** Pestañas de archivos con `dirty` y confirmación al cerrar; guardar con `fs_write_text`; recargar si no hay cambios propios; enviar la selección como chip de contexto del composer.
12. **[code] Chats sueltos**: carpeta propia (equivalente a `~/.referencia/chats`), su prompt, «Chat sin proyecto» en el inicio, la paleta y la barra lateral.
13. **[code] Sidebar y espacios:**
    - Favoritos con `FavStar`, orden y reordenar.
    - Desplegar al abrir; renombrar al perder el foco y desde el título.
    - Quitar espacios y agregarles carpetas en Expressive.
    - Rama en el historial; máximo 4 procesos vivos.
    - Título de la ventana con el proyecto.
    - Diálogo «Nuevo espacio» con nombre y `space::picker`.
    **[m3]** `ReorderList`/`DragHandle`.
14. **[code] Panel de cambios:** grupos plegables, «Al día» y «No es un repositorio git», `N` para lo no seguido, spinner al actualizar, árbol con `TreeRow`, colores por tipo y archivo activo.
    **[m3]** `DiffBar`.
15. **[code] Apariencia:**
    - «Sistema» sigue al sistema operativo, también en vivo.
    - Acento por espacio en `code-claude.json`, con `ColorSwatches` y `HsvPicker` y `Scheme::from_seed` en M3.
    - Acento HCT en Formal y Glass.
    - `SelectCard` en las tarjetas de estilo; Apariencia abre su pestaña.
    - `MotionSettings.reduced` según el sistema.
16. **[code] Perfil editable**: nombre y foto (en un json propio), tarjeta de perfil, `Avatar` con imagen.
    **[m3]** `ImageCropper`.
17. **[code] Cuenta y uso, mapa de agentes**: formato `rate_limits.limits`, evento `rate_limit`, secciones «Trabajando ahora» y «Terminados», contadores, `TASK_KINDS`, reloj de 1 s, Detener en la fila.
18. **[code] Notificaciones del sistema** (terminó, falló, pide permiso) a través de la pill/notch cuando la ventana no está al frente.
19. **[code] Atajos que faltan:** Ctrl+L (sin efecto en la terminal y el editor), Ctrl+O, Ctrl+E, Ctrl+G; teclado en las filas de proyecto.
20. **[m3] Motion M3 que falta:** `theme_reveal` + `animate_scheme`, altura animada en `ExpandableCard`, ítems de menú escalonados uno por uno, `Icon::pop`, `Avatar::hover_spin`, `slide_in`, entrada fiel de `Dialog` y `Toast`.
    **[code]** Usarlos, junto con las entradas escalonadas del hero, la barra lateral, el historial y la configuración.
21. **[code] Formal y Glass, primero la funcionalidad:**
    - Tarjeta de permiso completa.
    - Menú del agente completo.
    - La Configuración de la referencia.
    - Composer que crece.
    - Perilla de esfuerzo en Glass.
22. **[code] Motion propio de Formal y Glass**, al final: el de Glass en un módulo aparte de Atic, no en gpui-m3 (luz ambiental, reflejo que sigue al cursor, gota de selección, gelatina, materializar, borde especular, `ScrollFade`, segmentado deslizante).
23. **[code] Baja prioridad:** importar `.code-workspace`, `rate_limit` en vivo, reversión si falla `syncChatSettings`, marcas de modelo al cargar el historial, exportar con las herramientas, Chrome que reinicia el chat vacío, reinicio después de actualizar Claude.

## Decisiones / no aplica

Decisiones tomadas sin el usuario (2026-10-09). Hay que confirmarlas con él cuando vuelva:

- **Estilo prioritario: M3 Expressive.** Formal y Liquid Glass reciben la funcionalidad (tanda 21), pero su motion propio va al final (tanda 22).
- **Animaciones de Glass en un módulo aparte de Atic**, no en gpui-m3. gpui-m3 queda solo para M3 Expressive.
- **Esc con un permiso pendiente lo rechaza**, como en la referencia. Si no hay permiso, Esc cierra lo abierto o interrumpe.
- **Se conserva `Item::Turn`** (la fila de duración y costo del turno), aunque la referencia no la tiene.
- **Se construyen** el editor de código (`CodeEditor` en gpui-m3, con resaltado), la terminal integrada (con `space::console` y `TabStrip`), los chats sueltos y el perfil editable con foto.
- **«Sistema» sigue al sistema operativo**, no al tema de Atic.
- **El acento va por espacio** en `code-claude.json`, no en el archivo de espacios.
- **Los atajos ⌘ pasan a Ctrl**, salvo los que chocan dentro de la terminal y el editor (Ctrl+L, Ctrl+J, Ctrl+W…). Esos no actúan cuando el foco está en la terminal o el editor.
- **Valores por defecto de Claude: los de Atic.** El modelo vacío es el de Claude Code, igual que el esfuerzo vacío («Predeterminado»). Además `thinking: false` y el modo `default`. No se copian opus/medium/acceptEdits de la referencia.
- **Marcadores y flags en un json propio de Atic Code**, aparte de `space-workspaces.json`.

Decisiones de la tanda 3 (2026-10-09), también por confirmar:

- **Chats sueltos** en `<datos de Atic>\pill\code-chats` (con `paths.rs`), no en `~/.referencia`. Van con un espacio ficticio `LOOSE` (`u64::MAX`) que tiene su propia configuración de Claude y su propio acento en `code-claude.json`. Solo la barra de Expressive tiene la sección «Chats»; en Formal y Glass se abren desde la paleta.
- **Favoritos y «ya se abrió»** se guardan en `code-claude.json`, no en `space-workspaces.json`, que comparte el Mando.
- **Máximo de 4 procesos:** se cierra el proceso de Claude, no la conversación. Queda en la lista y se retoma al escribirle. Solo se cierran las que tienen sesión y no trabajan ni esperan un permiso.
- **Pegar archivos:** en Windows el Explorador copia rutas (`CF_HDROP`). Se adjuntan por ruta, sin copiarlos aparte como `savePasted`.
- **«Nuevo espacio»** elige las carpetas con el diálogo nativo. `space/picker.rs` está atado a `SpaceView` y habría que separarlo.
- **Acento** por espacio y por estilo, sin acento global de la app. M3 usa la especificación de 2021 de gpui-m3.
- **«Sistema»** sigue la apariencia de la ventana de GPUI (claro/oscuro de Windows) en vez de `crate::theme`.
- **Movimiento reducido:** `MotionSettings` es global de la app, así que también afecta a la pill.
- **Índice de archivos:** respeta `.gitignore` también fuera de un repo (`require_git(false)`). Se rehace en la búsqueda siguiente a cada respuesta.
- **Panel de cambios:** «Todo al día» solo si alguna carpeta es un repo. Si ninguna lo es, se listan con «No es un repositorio git».
- **Ctrl+L** abre siempre una conversación nueva, porque todavía no hay editor ni terminal.

Cosas que no aplican o que Atic ya tiene de otra forma:

- **Workspaces.** Los `.referencia-workspace` (JSONC con rutas relativas, «guardar», «guardar como», «cerrar», «recientes») no se portan. Atic usa los espacios de `src/space/workspaces.rs`: una lista en `space-workspaces.json` con nombre automático y varias carpetas, que se guardan solos. Las carpetas extra van como `additionalDirectories` y la configuración de Claude va por espacio en `code-claude.json`. Solo podría portarse más adelante la importación de VS Code.
- **Espacio de consolas.** `src/space/` (Mando, pizarra, paneles, `console.rs` con alacritty + ConPTY, `explorer.rs`, `viewer.rs`, `changes.rs`, `picker.rs`) ya existe. Atic Code reutiliza el árbol, el visor y los espacios, y reutilizará las consolas para la terminal. En la bandeja, Atic Code ocupa la entrada «Consolas».
- **Recientes** (`recent.rs`): la lista de espacios cumple esa función.
- **Precalentar conversaciones vacías:** Atic abre la sesión al enviar el primer mensaje.
- **`login_env`** (PATH en macOS), `error.rs`, la vista previa en el navegador (`preview.ts`, `?demo`, `?style=`) y el saneado de HTML del markdown.
- **Commit, ramas y stage:** la referencia tampoco los tiene.
- **`backdrop-filter` dentro de la ventana** (GPUI no desenfoca). Glass usa Acrylic para toda la ventana (`mod.rs:262`).
- **Animar `font-stretch` y escalar elementos:** GPUI no los tiene. Se aproximan con radio, opacidad y tamaño.

Dudas abiertas para el usuario:

1. La ruta del sidecar empaquetado: `sidecar.rs:67` busca `<exe>/sidecar/agent.mjs` y `build.mjs` deja `dist/agent.mjs`.
2. El `pnpm-workspace.yaml` sin seguimiento en `prototypes/pill-gpui/sidecar/` dice `allowBuilds: esbuild` sin decidir.
3. El recorte a 6000 caracteres de los resultados de herramientas (`chat.rs:18`): la referencia no recorta.
4. Remote Control: la referencia lo activa en todas las conversaciones vivas y Atic solo en las del espacio activo.
5. Glass tiene `raised` en 0.92 (la referencia usa 0.82). ¿Se hizo más denso a propósito porque no hay desenfoque?
6. La especificación de color: gpui-m3 usa la de 2021 y la referencia la de 2025. Con semillas propias, los colores no van a coincidir del todo.
