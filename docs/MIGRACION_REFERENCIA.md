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
| Miniaturas de las imágenes adjuntas en el mensaje del usuario (`Thread.tsx:105`) | Hecho (`c820b5d`): `Item::User` guarda las imágenes | `ImageThumb` | |
| Copiar y Marcar en el mensaje del usuario (`Thread.tsx:112`) | Hecho (tanda 4): Copiar con «Copiado» y Marcar, al pasar el cursor (`message`, `view.rs`) | `Button` (`confirm`, `label_on_hover`) | |
| Markdown del asistente (`Thread.tsx:116`) | Hecho (tanda 6-8): `view::markdown` con `gpui_m3::Markdown` en respuestas, razonamiento, plan de ExitPlanMode y resultado de Task, en los tres estilos | `Markdown` | Formal y Glass le pasan a gpui-m3 un esquema armado con sus tokens (`style::scheme_of`). El razonamiento va con opacidad 0,75 en vez de gris y con la fuente de gpui-m3 |
| Copiar la respuesta («Copiado» durante 1,4 s) (`Thread.tsx:69`) | Hecho `view.rs:1130` | `Button::confirm` | Solo cuando la respuesta terminó |
| Marcar mensaje (flag por sesión) (`Thread.tsx:82`, `src/lib/flags.ts:21`) | Hecho (tanda 2, `1c6a237`): `Marks::toggle_flag` y `CodeView::toggle_flag` (`marks.rs`); el botón Marcar está en `message` (`view.rs`) | existe `Button::selected` | Se guarda en `code-marks.json` (ver Decisiones) |
| Fondo de mensaje marcado (`Thread.tsx:103,123`) | Hecho (tanda 2): `message` (`view.rs`) pinta `accent_soft` al 50 % si `is_flagged` | — | |
| «Siguiente marcado» con scroll y destello (`flags.ts:44-54`) | Hecho (tanda 2): `next_flagged` (`marks.rs`) lleva el hilo a la marca y `flash` la destella un momento (`message`, `view.rs`) | no existe `FlashHighlight`: el destello es de Atic | |
| Contador de marcas (`flags.ts:29`) | Hecho (tanda 2): `CodeView::flag_count` (`marks.rs`) alimenta la fila «Siguiente mensaje marcado» del menú del agente (`agent_menu.rs`) | — | |
| Ruta en `código` en línea que se puede abrir con un clic (`Thread.tsx:44,124`) | Hecho (tanda 6-8): `Markdown::on_path` → `CodeView::open_ref` → `files::resolve_ref` (con test) | `Markdown::on_path` | Relativa a las carpetas del espacio de la conversación (o `carpeta/ruta`); si no existe, un aviso «No encontré…» |
| Razonamiento plegable (`Thread.tsx:18`) | Hecho `view.rs:1142` | `ExpandableCard::plain` | |
| «Razonando…» en vivo, con la última línea (`Thread.tsx:21,29`) | Hecho (tanda 4): `Chat::is_streaming` + `ExpandableCard::preview` con `last_line` (con test). Tanda 21: también en Formal/Glass (`other_item`, `view.rs`: encabezado con la última línea y cuerpo en `markdown`) | `ExpandableCard::preview` | |
| Ocultar el razonamiento vacío o cifrado (`Thread.tsx:137`) | Hecho (tanda 4): `Chat::hidden`; el hilo se lo salta | — | |
| Aviso informativo (`Thread.tsx:146`) | Hecho `view.rs:1103` | `Bubble` Notice | |
| Aviso de error (`Thread.tsx:146`) | Hecho (tanda 4): `BubbleKind::Error` en Expressive | `BubbleKind::Error` | |
| Separador «Contexto compactado» (`Thread.tsx:152`, `src/lib/agent.ts:450`) | Hecho (tanda 2 y `c820b5d`): `compact_boundary` y `DividerLabel` | `DividerLabel` | |
| Marca «Cambiado a X» (`Thread.tsx:158`, `agent.ts:808`) | Hecho (`b1a3efc`, `Item::Model`, `chat.rs`) | `DividerLabel` cuando exista | Hoy dibujado a mano |
| Marca «X en el próximo mensaje» (`Thread.tsx:170,234`) | Hecho (`b1a3efc`, `Chat::pending_model`) | ídem | |
| Marcas de modelo al cargar el historial (`agent.ts:697-737`) | Hecho (tanda 23): `history_model_marks` y `Chat::load_history(list, …)` (`chat.rs`), con test; también en la rama de un Rewind y en el resync | — | La marca va antes del mensaje cuyo turno respondió otro modelo; ignora subagentes y modelos `<synthetic>` |
| Aviso «responde otro modelo» y reaplicarlo (`agent.ts:782`) | Hecho (`b1a3efc`, `Chat::check_model`) | — | |
| Seguir el final, soltarlo al subir y retomarlo cerca del fondo (`Thread.tsx:16,194,206`) | Hecho `view.rs:967`, `mod.rs:471` | — | |
| Ir al final al cambiar de conversación (`Thread.tsx:220`) | Hecho `mod.rs:682` | — | |
| Animación al cambiar de conversación (`Thread.tsx:227`) | Hecho (tanda 20): `Enter::on_change` en el hilo, sube 14 px (`view.rs`: `center`, `key_hash`) | no hay `swap`: se hace con `motion::replay` (`enter.rs`) | Solo Expressive |
| Entrada animada de cada parte nueva (`src/lib/motion.ts:43-57`) | Hecho (tanda 4): `Bubble::entrance` en el mensaje del usuario y los avisos, solo las últimas 3 partes (`ENTER_MAX`) | `Bubble::entrance` | Texto, razonamiento y herramientas no son burbujas: sin entrada |
| Indicador «Trabajando…» (`Thread.tsx:241,253`) | Hecho (tanda 4): `Chat::working` lo oculta mientras llega texto o razonamiento (con test) | `LoadingIndicator` | |
| Permisos apilados al final del hilo (`Thread.tsx:261`) | Hecho `view.rs:904` | — | Formal y Glass también apilan todos (tanda 21, `permission_cards` en `permissions.rs`) |
| Streaming start/delta/stop (`agent.ts:215`) | Hecho `chat.rs:282` | — | |
| Cambiar el texto del stream por el del mensaje final (`agent.ts:285-291`) | Parcial `chat.rs:366` | — | Si llegó por stream, el texto final se ignora |
| Ignorar ecos y mensajes internos (`agent.ts:332`) | Hecho `chat.rs:148,402` | — | |
| Mensaje desde otro cliente (Remote Control) (`agent.ts:348`) | Hecho `chat.rs:408` | — | Usa `isReplay`; la referencia deduplica por uuid |
| uuid y cadena de mensajes para Rewind (`agent.ts:168-175,339`) | Hecho (tanda 2): `Chat::chain`, `uuid_before` y `pending_echo` (`chat.rs`) | — | |
| Rewind/fork de conversación y código (`agent.ts:504`, `src/components/Agent.tsx:390-405`) | Hecho (tanda 2): `open_rewind`, `rewind` y `fork_at` (`rewind.rs`); el código vuelve con `rewindFiles` del sidecar | — | |
| Título tomado del primer mensaje (`agent.ts:653`) | Hecho `chat.rs:205` | — | 60 caracteres (la referencia usa 80) |
| «Detenido.» al interrumpir (`agent.ts:441`) | Hecho (tanda 2): `Chat::interrupted` + `Chat::result` (`chat.rs`), con test; lo marca `interrupt` (`mod.rs`) | — | |
| Error del resultado (`agent.ts:442`) | Hecho `chat.rs:421` | — | Solo el primer error |
| Herramientas que siguen `running` pasan a error si el turno falla (`agent.ts:439`) | Hecho (`Chat::result`, con `subtype != "success"`) | — | |
| Herramientas sin resultado en el historial se dan por hechas (`agent.ts:743`) | Hecho (`Chat::load_history`) | — | |
| `closed` con error (`agent.ts:480`) | Hecho `chat.rs:243` | — | |
| Error de start o de send (`agent.ts:636,657`) | Hecho `mod.rs:764,816` | — | |
| Resync al seguir en otro cliente (evento `external`) (`agent.ts:415,541`) | Hecho (tanda 2): `CodeView::resync` (`rewind.rs`), disparado desde el evento `external` (`mod.rs`) | — | |
| Costo y tokens acumulados (`agent.ts:425`) | Hecho `src/code/usage.rs:113` | — | |
| Fila de duración y costo del turno | Extra de Atic: `Item::Turn` (`chat.rs:431`, `view.rs:1237`) | — | Se conserva (Decisiones) |
| Tareas y subagentes (`agent.ts:454-478`) | Hecho `usage.rs:124-160` | — | |
| No leído al terminar (`agent.ts:675`) | Hecho `mod.rs:466` | — | |
| Recargar el archivo abierto cuando Claude lo edita (`agent.ts:327,356`) | Hecho (tanda 2 y 11): `files_edited` (`editor.rs`) recarga el editor y `doc.reload()` (`mod.rs`) el visor de diffs | — | |
| Refrescar git al terminar (`src/App.tsx:16`) | Hecho (tanda 3): `refresh_changes_now` en cada `result` (`mod.rs`), además del sondeo cada 3 s | — | |
| Notificación del sistema: terminó, falló, pide permiso (`src/lib/notify.ts:33`, `agent.ts:413,446`) | Hecho (tanda 4): `src/code/notify.rs` avisa con el globo del ícono de la pill (`tray_icon::notify`, `NIF_INFO`) cuando la ventana no está al frente o la conversación no es la visible; un clic abre Atic Code | No aplica | Sin la pill (`CODE_ALONE=1`) no hay ícono y no se avisa. Sin probar en Windows |
| Renombrar desde el título de la barra superior (`src/components/Chat.tsx:42-75`) | Hecho (tanda 3): `start_header_rename` (`sidebar.rs`), título con lápiz en `view.rs` | `TextField::inline` | Solo con la sesión creada; guarda al perder el foco |
| Barra superior: proyecto, pestañas Cambios (badge) y Archivos (`Chat.tsx:76-111`) | Hecho `view.rs:830-858` | `Button` + `Badge` | Falta el botón Terminal |
| Pantalla de inicio: formas, «¿Qué construimos hoy?», proyecto, sugerencias (`Chat.tsx:180-222`) | Hecho `view.rs` (`hero`) | `Shape`, `Chip` | «Chat sin proyecto» en el selector (tanda 3) |
| Subida escalonada de los hijos de la pantalla de inicio (`src/styles/motion.css:391-408`) | Hecho (tanda 20): `rise` en `hero` (`view.rs`), retardos 0 / 0,06 / 0,12 / 0,18 s | `motion::entrance` vía `enter::Enter` | la referencia solo retrasa 2 y 3; aquí se escalona todo (Decisiones) |
| Conversación de demo (`agent.ts:873`) | Hecho `src/code/demo.rs:11` | — | |
| Simulación de Tauri para el navegador (`src/lib/preview.ts`) | No aplica | — | |

### Markdown (`src/components/chat/Markdown.tsx`)

| Función de la referencia | Estado en Atic Code | gpui-m3 | Nota |
| --- | --- | --- | --- |
| GFM con `marked` (`Markdown.tsx:141`) | Hecho (tanda 6-8) | `Markdown` | Listas numeradas y anidadas, citas, `---`, cursiva y tachado |
| Tablas | Hecho (tanda 6-8) | `Markdown` | |
| Enlaces que abren el navegador (`Markdown.tsx:160`) | Hecho (tanda 6-8): `Markdown::on_link` + `cx.open_url`, solo `http`, `https` y `mailto` (`files::is_safe_link`, con test) | `Markdown::on_link` | |
| Imágenes | Parcial: sin `on_image`, gpui-m3 muestra una fila con la descripción que abre el enlace | `Markdown::on_image` | |
| Bloque de código con lenguaje y «Copiar» (`Markdown.tsx:144-155`) | Hecho (tanda 6-8): `CodeBlock` dentro de `Markdown` | `CodeBlock` | |
| Resaltado de sintaxis (rust/ts/js/json/md/css/html/py) (`Markdown.tsx:14-37,97`) | Hecho (tanda 6-8): `SyntaxHighlighter` (syntect) dentro de `CodeBlock` | `SyntaxHighlighter` | Colores de `SyntaxPalette::from_scheme` con el esquema del estilo; no se llamó a `SyntaxPalette::set` |
| Alias de lenguaje a nombre legible (`Markdown.tsx:43-88`) | Hecho (tanda 6-8) | dentro de `CodeBlock` | |
| `código` en línea y **negrita** | Hecho `view.rs:356,385` | — | Con tests |
| Títulos por nivel | Hecho (tanda 6-8) | `Markdown` | |
| Saneado de HTML (`Markdown.tsx:159`) | No aplica | — | |
| Caché durante el streaming (`Markdown.tsx:94,190`) | Hecho (tanda 6-8): `parse_markdown_cached` por texto y `highlight_cached` por bloque | `Markdown` | |
| Seleccionar texto | Falta (GPUI no la trae) | falta: `SelectableText` | Se compensa con «Copiar» |

### Extras de Atic Code

| Función | Estado | gpui-m3 | Nota |
| --- | --- | --- | --- |
| Dos conversaciones lado a lado arrastrando desde la barra | Hecho (`28f209d`): `src/code/split.rs` (`ChatDrag`, `open_split`, `focus_split`, borradores por conversación), `chat_area` y `split_pane` en `view.rs` | `ReorderList::grab_height` (solo el encabezado del proyecto reordena) | La otra se ve entera con sus permisos; un clic en su caja la vuelve la activa. Máximo dos. Sin probar en la app |
| Correos tapados a medias en la cuenta | Hecho (`8dd44de`): `config::mask_emails` (con test) | — | `ca•••@dominio` |
| Formas del inicio blandas que se vuelven M3 al pasar el cursor | Hecho (`fa6f860` en gpui-m3, `52ee62f`) | `ShapeName::Blob`, `Blob2`, `Pebble` | |
| Fila del turno con tok/s en vez del costo | Hecho (`bb12662`): `chat::turn_summary` (con test) | — | El sidecar manda `durationApiMs` |
| Sesión de sondeo para modelos y uso sin conversación | Hecho (`573a4d5`): `PROBE`, `ensure_probe`, `info_key` | — | El precalentado de la referencia |

## 2. Composer

| Función de la referencia | Estado en Atic Code | gpui-m3 | Nota |
| --- | --- | --- | --- |
| Caja que crece hasta 240 px (`Agent.tsx:1098,1311`) | Hecho (tanda 21, `9348cb1`): `composer_box` (`view.rs`) usa `content_height(3).min(240)` en los tres estilos; Expressive, `composer_m3` | existe `TextArea` | Atic usa su propio `crate::text_area` |
| Placeholder «Pregunta… · @ · /» (`Agent.tsx:1314`) | Hecho (tanda 4): la «@» solo se anuncia con un proyecto abierto (`HERO_PLACEHOLDER_LOOSE`) | — | |
| «/» solo abre el menú (`Agent.tsx:1319`) | Hecho `mod.rs:1000` | — | |
| Enter envía y Mayús+Enter baja de línea (`Agent.tsx:1352`) | Hecho `mod.rs:67` | — | |
| Esc interrumpe la respuesta en curso (`Agent.tsx:1336`) | Hecho `mod.rs:952` | — | Antes cierra los menús abiertos |
| Esc Esc abre Rewind (`Agent.tsx:1342`) | Hecho (tanda 2): `close_menu` (`mod.rs`, `DOUBLE_ESC`) llama `open_rewind` | — | |
| Adjuntar con el diálogo (`Agent.tsx:86,1360`) | Hecho `mod.rs:883` | `IconButton` | |
| La imagen va como imagen y el resto como ruta (`Agent.tsx:93-112`) | Hecho `mod.rs:863` | — | |
| Máximo 10 imágenes (`Agent.tsx:1110`) | Hecho: `MAX_IMAGES`, `attach_paths` y `take_files` con el aviso «Como mucho 10 imágenes por mensaje» (`mod.rs`); verificado en la tanda 23 | — | |
| Archivos sin duplicar (`Agent.tsx:1113`) | Hecho `mod.rs:875` | — | |
| Ctrl+U adjunta (`Agent.tsx:1140`) | Hecho `mod.rs:78` | — | |
| Pegar una imagen (`Agent.tsx:1202`) | Hecho `mod.rs:904` | — | |
| Pegar archivos que no son imagen (`savePasted`) (`Agent.tsx:1189`) | Hecho (tanda 3): `paste` lee `CF_HDROP` (`clip_image::read_files`) | — | En Windows el Explorador da rutas: se adjuntan sin copiar (Decisiones) |
| Arrastrar desde el Explorador, con «Suelta para adjuntar» (`Agent.tsx:1127,1208,1257`) | Hecho (`c820b5d`): `drop_zone` | `DropZone` | |
| Miniaturas de imágenes, con quitar (`Agent.tsx:1295`) | Hecho (`c820b5d`) | `ImageThumb` | |
| Chips de archivos, con quitar (`Agent.tsx:1280`) | Hecho `view.rs:1464` | `Chip::input` | |
| Chip de contexto del editor (archivo y líneas) (`Agent.tsx:1117,1264`) | Hecho (tanda 11): `editor_context` + `file_chip` (`view.rs`); la X lo quita hasta cambiar de pestaña. Sin probar en la app | `Chip::input` | Con solo el cursor cuenta su línea (`archivo:12`); la referencia solo ponía líneas con selección (Decisiones) |
| Sufijo «(Archivos adjuntos: @rel)» (`Agent.tsx:1149`) | Hecho: `compose_message` usa `relative_to`, la ruta relativa a la carpeta del proyecto que la contiene (con `/`); fuera de los proyectos, la absoluta. Verificado en la tanda 23 (la fila decía Parcial) | — | Con test (`relative_to`) |
| «(Contexto: @rel, líneas N-M)» en el mensaje (`Agent.tsx:1149`) | Hecho (tanda 11): `editor::context_note` + `compose_message` (con tests) | — | Solo si hay texto escrito y no menciona ya el archivo |
| Texto por defecto «Mira la imagen adjunta.» (`Agent.tsx:1150`) | Hecho: `compose_message` (`mod.rs`), con test; verificado en la tanda 23 | — | |
| Enviar mientras responde (cola) (`Agent.tsx:1377-1388`) | Hecho (tanda 2): `send` (`mod.rs`) ya no descarta el mensaje; queda en la cola del sidecar y `Chat::push_user`/`pending_echo` esperan su eco | — | |
| Detener (`Agent.tsx:1385`) | Hecho `view.rs:1635` | `IconButton` | |
| Despegue del botón al enviar (`Agent.tsx:1156`) | Hecho `view.rs:1646` | `IconButton::launch` | |
| @-menciones: índice, ranking difuso, popover, ↑↓ Enter Tab Esc (`Agent.tsx:990-1025,1161-1186,1330`) | Hecho (tanda 4): `src/code/mention.rs` (`mention_at`, `apply_mention`, con tests) + `SuggestionList`, `rank_mentions`; teclas por el contexto `suggesting` de `crate::text_area::TextArea` | `SuggestionList`, `SuggestionNav`, `rank_mentions` | Con varias carpetas la ruta lleva la carpeta (`raíz/ruta`) como en la referencia. Sin probar en la app |
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
| Salida animada de los popovers (`Presence`, `src/components/ui.tsx:10`) | Hecho (tanda 4): `overlay::Last` + `motion::presence` en el menú del agente, «Cuenta y uso», menús de sesión y espacio, tarjeta de perfil, @-menciones y toasts | `Presence`/`motion::presence`, `Exit` | |
| Oferta del Artifact publicado (`Agent.tsx:1438-1492`) | Hecho (`c820b5d`): `banners` | `Banner` | |
| Aviso de conversación larga (`Agent.tsx:1494-1545`) | Hecho (`c820b5d`): `banners` y `Chat::long_level` | `Banner` | |
| Entrada de los chips (`m3-chip-in`, `motion.css:706`) | Hecho (tanda 20): `chip_in` (`view.rs`) | `Enter` | Sube 8 px y se funde; sin escala |

## 3. Menú del agente (`AgentMenu`, `Agent.tsx:225-667`)

| Función de la referencia | Estado en Atic Code | gpui-m3 | Nota |
| --- | --- | --- | --- |
| Filtro «acciones o comandos» (`Agent.tsx:450`) | Hecho `mod.rs:304`, `agent_menu.rs:195` | `TextField` | |
| Seis pestañas con ícono que recuerdan la última (`Agent.tsx:225-234,461`) | Hecho `agent_menu.rs:18,207`, `mod.rs:172` | `IconTabs` | |
| Filtrar en todas las secciones (`Agent.tsx:357,501`) | Hecho `agent_menu.rs:239` | `Menu::section` | |
| Comandos «/» que coinciden (hasta 8) y «Sin resultados» (`Agent.tsx:379,480,498`) | Hecho `agent_menu.rs:214-254` | — | |
| Adjuntar archivo… ⌘U (`Agent.tsx:258`) | Hecho `agent_menu.rs:120` | `MenuItem::shortcut` | |
| Mencionar archivo… @ (`Agent.tsx:259`) | Hecho (tanda 4): `Act::Mention` («Mencionar archivo del proyecto…»), también en el menú de Formal/Glass | — | |
| Limpiar conversación (`Agent.tsx:260`) | Hecho `agent_menu.rs:121` | — | |
| Rewind: Código, Conversación o Ambos (`Agent.tsx:261,389-409,636`) | Hecho (tanda 2): `Sub::Rewind` (`agent_menu.rs`) con tres botones; `rewind` (`rewind.rs`) | `Chip` | |
| Marcador (`Agent.tsx:262`) | Hecho (tanda 2): `Act::Bookmark` (`agent_menu.rs`), `toggle_bookmark` (`marks.rs`) | — | |
| Siguiente mensaje marcado (`Agent.tsx:270`) | Hecho (tanda 2): `Act::NextFlag` (`agent_menu.rs`) → `next_flagged` (`marks.rs`) | — | |
| Exportar conversación (`Agent.tsx:120-137,281`) | Hecho (tanda 4): `export_markdown` incluye las herramientas (`> **Read** \`{…}\``, 200 caracteres; con test) | — | |
| Copiar enlace de Remote Control (`Agent.tsx:282`) | Hecho `agent_menu.rs:123` | — | |
| Cambiar modelo…, con el valor actual (`Agent.tsx:293`) | Hecho (`b1a3efc`) | — | |
| Esfuerzo en línea (`Agent.tsx:294,542`) | Hecho `agent_menu.rs:161` | `StopSlider` | Sin elegir dice «Predeterminado» (ver Decisiones) |
| Ultracode / Thinking / Modo rápido (`Agent.tsx:295-298`) | Hecho `agent_menu.rs:128-131` | `Switch` | |
| Cambiar de modelo al marcar (`Agent.tsx:297`) | Hecho (el interruptor) `agent_menu.rs:130` | `Switch` | Tendrá efecto cuando lleguen las marcas (tanda 2) |
| Cuenta y uso… (`Agent.tsx:299`) | Hecho (tanda 3): `Act::Usage` en la pestaña Modelo | `Popover` | |
| Estilo de salida, Permisos, MCP, Hooks (`Agent.tsx:300-303`) | Hecho `agent_menu.rs:134-137` | — | Hooks abre `settings.json` con el programa del sistema |
| Subagentes y Comandos con contador (`Agent.tsx:304-305`) | Hecho `agent_menu.rs:138-139` | — | |
| Memoria · CLAUDE.md, Sandbox, Línea de estado, Instrucciones, Configuración general (`Agent.tsx:306-310`) | Hecho `agent_menu.rs:140-144` | `Switch` | |
| Claude in Chrome (`Agent.tsx:311`, `src/lib/store.ts:405`) | Hecho (tanda 23): `Act::Chrome` (`agent_menu.rs`) llama `restart_idle_chat` (`mod.rs`), que cierra el proceso de la conversación vacía y avisa «activado/desactivado»; con mensajes, «se activará en la próxima conversación» | `Switch` | Atic no precalienta: una conversación vacía solo tiene proceso si se abrió sola (Rewind, resync) |
| Remote Control en la terminal, Claude Design, Abrir Claude en la terminal (`Agent.tsx:312-334`) | Hecho `agent_menu.rs:148-153` | — | Usa `wt.exe` externo. Claude Design agrega `/design ` al final (tanda 3) |
| Reanudar conversación… (`Agent.tsx:326`) | Hecho (tanda 3): `Act::Resume` en la pestaña Sesión | — | |
| Compactar contexto (`Agent.tsx:327`) | Hecho `agent_menu.rs:152` | — | Pide una sesión viva |
| Administrar/recargar plugins, cambiar de cuenta, cerrar sesión (`Agent.tsx:335-344`) | Hecho `agent_menu.rs:154-157` | — | |
| Submenú con «atrás» (`Agent.tsx:441,562`) | Hecho `agent_menu.rs:265` | `Menu::back` | |
| Submenú Modelo: radios con descripción y «Más modelos» (`Agent.tsx:413-421,569`) | Hecho (`b1a3efc` agregó `same_model`) | `MenuItem::radio` | «Más modelos» queda al final, no entre los grupos |
| Submenús Permisos y Estilo de salida (`Agent.tsx:422-431`) | Hecho `agent_menu.rs:309-330` | — | |
| Submenús Subagentes y Comandos (`Agent.tsx:432-435`) | Hecho (tanda 3): agregan con `insert()` | — | |
| Submenú MCP: estado, Reconectar, Activar/Desactivar (`Agent.tsx:347,608`) | Hecho `agent_menu.rs:361-442` | `Chip` | |
| Menú en Formal y Glass | Hecho (tanda 21, `f2793f0`): `menu_entries` (`agent_menu.rs`) arma filas neutras (`Line`, `Entry`) y `flat_entries`/`flat_line` (`menus.rs`) las dibujan con filtro y pestañas; Expressive las pasa a `MenuItem` | — | |
| Ítems de menú escalonados de a 25 ms (`motion.css:628`) | Hecho (tanda 20): `Menu` lo hace solo (`stagger` por defecto) y ningún menú de Atic usa `grouped` | `Menu::stagger` | Sin cambios en Atic Code |

## 4. Modelos y esfuerzo

| Función de la referencia | Estado en Atic Code | gpui-m3 | Nota |
| --- | --- | --- | --- |
| Modelos de respaldo y `models()` desde meta (`Agent.tsx:37-43`) | Hecho `config.rs:37`, `mod.rs:508` | — | |
| `splitModels` (`Agent.tsx:46`) | Hecho `config.rs:42` (con test) | — | |
| `modelLabel` / `modelName` (`Agent.tsx:61`, `agent.ts:799`) | Hecho (`b1a3efc`, `model_name` en `config.rs`) | — | Revisa que «Default (recommended)» pase a «Predeterminado» (`mod.rs:521`) |
| `sameModel` (`agent.ts:816`) | Hecho (`b1a3efc`) | — | |
| `want`, `setChatWant`, `chatModel`/`chatEffort` (`agent.ts:130,819`, `store.ts:368`) | Hecho (`b1a3efc`, `Chat::want_*`, `chosen_model`/`chosen_effort`) | — | |
| `adoptSessionSettings` / `sessionSettings` (`agent.ts:826`, `sidecar/agent.mjs:330,514`) | Hecho (`b1a3efc`) | — | |
| `syncChatSettings` + `applied` (`agent.ts:762`, `store.ts:427`) | Hecho (`b1a3efc`, `Applied`) | — | La reversión si falla (`agent.ts:770`) está hecha (tanda 23): `sync_chat_settings` revierte solo la parte que falló (`Setting::revert`, `mod.rs`, con test); la referencia revierte las dos |
| `checkModel` (`agent.ts:782`) | Hecho (`b1a3efc`) | — | |
| `noteModelForSend` / `sentModel` (`agent.ts:808`) | Hecho (`b1a3efc`) | — | |
| `reviveFor`/`forkAt` con `want` (`store.ts:567,689`) | `reviveFor`: Hecho (`b1a3efc`). `forkAt`: Hecho (tanda 2), `fork_at` (`rewind.rs`) hereda `want_model`, `want_effort` y `sent_model` | — | |
| PERMISSIONS y EFFORTS (`Agent.tsx:66`, `ui.tsx:90`) | Hecho `config.rs:12,20` | — | |
| `EffortControl` por estilo: puntos, paradas, perilla (`ui.tsx:101`) | Hecho (tanda 21): `StopSlider` en M3, puntos en Formal (`effort_dots`) y riel con perilla blanca y paradas en Glass (`effort_knob`, `menus.rs`) | `StopSlider` | La perilla no es circular ni se arrastra libre: la referencia tampoco; se mueve por paradas (clic o arrastre) y no se anima el desplazamiento |
| `ClaudeSettings` global o por proyecto (`src/lib/api.ts:11`, `store.ts:348`) | Hecho por espacio (`config.rs:155`) | — | Sin espacio activo no se guarda nada |
| Valores por defecto: opus / medium / thinking / acceptEdits (`store.ts:334`, `src-tauri/src/workspace.rs:97`) | No aplica: se siguen los de Atic (Decisiones) | — | |

## 5. Permisos y preguntas

| Función de la referencia | Estado en Atic Code | gpui-m3 | Nota |
| --- | --- | --- | --- |
| Elegir la tarjeta: general, preguntas o plan (`src/components/chat/Permission.tsx:55`) | Hecho `permissions.rs:423` | `Card` | |
| Título por herramienta (`Permission.tsx:14`) | Hecho `permissions.rs:337` | — | |
| Cuerpo Bash `$ comando` (`Permission.tsx:115`) | Hecho `view.rs:2362` | — | |
| Cuerpo NotebookEdit `new_source` (`Permission.tsx:121`) | Hecho (tanda 4): `tool_input` muestra `new_source` | — | |
| Cuerpo Edit/MultiEdit/Write con diff (`Permission.tsx:124`) | Hecho (tanda 6-8): `tool_input` usa `DiffView` (mismo cuerpo que la tarjeta de la herramienta) | `DiffView` | Los números de línea son los del fragmento, no los del archivo |
| Notas: descripción, motivo, ruta bloqueada (`Permission.tsx:129`) | Hecho `permissions.rs:463` | — | |
| «Decirle a Claude qué hacer en cambio» (`Permission.tsx:140`) | Hecho `permissions.rs:525` | `TextField` | Solo en la primera tarjeta |
| Permitir / Permitir siempre / Permitir todo / Rechazar y responder (`Permission.tsx:74-160`) | Hecho `permissions.rs:461-514` | `Button` | |
| Tooltip con las reglas en «Permitir siempre» (`Permission.tsx:152`) | Parcial | existe `Tooltip` | |
| Foco inicial según `defaultToNo` (`Permission.tsx:90`) | Parcial (tanda 2): `Permission::default_to_no` se guarda (`chat.rs`); con él, «Permitir» pasa a tonal y Enter no permite (`permission_key`, `permissions.rs`). No hay foco real en el botón | falta: foco inicial en `Button` | |
| Teclado: Enter permite, Esc rechaza, Enter en el campo rechaza con texto (`Permission.tsx:92`) | Hecho (tanda 2): `permission_key` y `permission_keypress` (`permissions.rs`, `mod.rs`) | — | Esc rechaza (Decisiones) |
| Preguntas: chip de encabezado, opciones radio/checkbox, «Otro…» (`Permission.tsx:224-272`) | Hecho `permissions.rs:589-701` | `ChoiceRow`, `Badge`, `TextField` | |
| Enfocar «Otro» lo selecciona (`Permission.tsx:269`) | Hecho (tanda 2; verificado en la tanda 23): `pick_focused_other` (`permissions.rs`), llamado desde `render` (`view.rs`) | — | |
| Enviar desactivado hasta completar y Cancelar como rechazo (`Permission.tsx:183-205`) | Hecho `permissions.rs:586-666` | — | |
| Teclado en preguntas: Esc, Enter/Ctrl+Enter, foco en la primera (`Permission.tsx:207`) | Parcial (tanda 2): Esc cancela y Enter/Ctrl+Enter envían si está completa (`permission_key`, `permissions.rs`) | — | No se enfoca la primera opción |
| Plan: «Claude terminó de planificar» y sus tres opciones con campo (`Permission.tsx:316-337`) | Hecho `permissions.rs:533-568` | `Card`, `Button` | |
| Teclado en el plan (`Permission.tsx:303`) | Hecho (tanda 2): `permission_key` (`ExitPlanMode`: Esc o Enter con texto siguen planificando) | — | |
| `permission_cancel` y responder (`agent.ts:421,670`) | Hecho `chat.rs:233`, `permissions.rs:401` | — | |
| Tarjeta en Formal/Glass (`view.rs:1343`) | Hecho (tanda 21, `9348cb1`): `permission_cards`, `general_card`, `plan_card`, `ask_card` (`permissions.rs`) con «Permitir todo», campo de respuesta, diffs, preguntas y plan, todas apiladas | — | |

## 6. Herramientas y diffs (`src/components/chat/Tools.tsx`, `Diff.tsx`)

| Función de la referencia | Estado en Atic Code | gpui-m3 | Nota |
| --- | --- | --- | --- |
| Armazón: estado, verbo, objetivo, meta, chevron (`Tools.tsx:124`) | Hecho `tools.rs:162-238` | `ExpandableCard` | |
| Girando / ✓ / ✗ (`Tools.tsx:102`) | Hecho `tools.rs:170` | `LoadingIndicator` | |
| Ícono de «terminado» que salta (`motion.ts:87`) | Hecho (tanda 20): `Icon::pop` en `tool_m3` (`tools.rs`), solo en las últimas 3 partes | `Icon::pop` | |
| El objetivo abre el archivo (`Tools.tsx:176`) | Hecho (tanda 4): `target_file` + `open_doc` (Read, Edit, MultiEdit, Write, NotebookEdit) | — | |
| Tooltip con la ruta, el comando o la URL completos (`Tools.tsx:171`) | Hecho (tanda 4): `target_tip` (con test) | `hover::tip_text` | |
| Enter/Espacio despliega la tarjeta (`Tools.tsx:158`) | Hecho en Expressive (verificado en la tanda 23): la cabecera de `ExpandableCard` (`gpui-m3/src/components/disclosure.rs`) es una parada de Tab con anillo y Enter/Espacio la despliegan, así que `tool_m3` y el razonamiento no necesitan más. Formal y Glass (`tool`, `view.rs`) siguen solo con el ratón | `ExpandableCard` (foco en `777021d`) | |
| Despliegue con resorte en altura (`motion.ts:68-83`) | Hecho (tanda 20): lo da `ExpandableCard` de gpui-m3 sin cambios en Atic (`tools.rs`, razonamiento, grupos de cambios) | `ExpandableCard` | Sin ver en la app |
| Tarjeta fija y abierta de entrada (`Tools.tsx:136,143`) | Hecho `tools.rs:290` | `.fixed()`/`.open()` | |
| Badges y +/− (`Tools.tsx:207`) | Hecho `tools.rs:190` | `Badge` | |
| Quitar `<system-reminder>` (`Tools.tsx:28`) | Hecho `tools.rs:16` | — | |
| Salida recortada con «Mostrar todo» (`Tools.tsx:79`) | Hecho (tanda 6-8): `tools::output` con `CodeOutput`; los errores en rojo | `CodeOutput` | Atic recorta a 40 000 caracteres y lo avisa (`MAX_RESULT`, antes 6000); ver Decisiones |
| Read con rango L… (`Tools.tsx:396`) | Hecho (tanda 4): `read_range`, `desde + limit − 1`, también con solo `limit` (con test) | — | |
| Edit con «todas» y diff (`Tools.tsx:420`) | Hecho (tanda 6-8): `DiffView` | `DiffView` | |
| MultiEdit: un diff por edición (`Tools.tsx:67`) | Hecho (tanda 6-8): `edits::edit_diffs`, un `DiffView` por edición con «Cambio N de M» (con test) | `DiffView` | |
| Write: todo agregado, abierta si tiene ≤ 40 líneas (`Tools.tsx:64,424`) | Hecho (tanda 6-8): `DiffView` con `max_rows(40)` y «Mostrar todo» | `DiffView` | |
| NotebookEdit (`Tools.tsx:313`) | Hecho (tanda 4): badge del modo y `new_source` con «Mostrar todo» | — | |
| Bash/PowerShell: descripción, en segundo plano, `$`, salida en rojo (`Tools.tsx:454`) | Hecho `tools.rs:42,203,251` | `Badge` | |
| Grep/Glob, WebFetch, WebSearch (`Tools.tsx:328-364`) | Hecho `tools.rs:47-54,257-264` | — | |
| Task/Agent con las últimas 5 herramientas hijas y «N herramientas» (`Tools.tsx:479`, `agent.ts:196,224,273`) | Hecho (tanda 4): `ToolCall::children`/`child_total` (`Chat::note_child`, sin duplicar por id, con test) + `ExpandableCard::under` | existe `ExpandableCard::under` | la referencia muestra «N» con tope 20; aquí es el total real |
| TodoWrite (`Tools.tsx:512`) | Hecho `tools.rs:220`, `view.rs:2316` | — | |
| ExitPlanMode y AskUserQuestion como tarjetas (`Tools.tsx:375,380`) | Hecho `tools.rs:270,274` | — | |
| Artifact: verbo, título, «Abrir» (`Tools.tsx:261`, `src/lib/artifacts.ts`) | Hecho (tanda 23): `describe` y `tool_m3` (`tools.rs`) con `artifact_verb` (`chat.rs`, con test), `artifact_title` y `artifact_url`; «Abrir» abre el navegador sin desplegar la tarjeta. Formal y Glass: solo el verbo y el resumen (`tool_label`) | `Button` | |
| MCP «servidor · acción» y genérica JSON (`Tools.tsx:538-554`) | Hecho `tools.rs:70,275` | — | |
| Diff: números de línea viejo/nuevo (`Diff.tsx:14,116`) | Hecho (tanda 6-8): en el chat y en el visor de cambios (`doc_diff` con `Diff::from_unified`) | `DiffView` | El visor de cambios ya no es virtual: `DiffView` dibuja 400 filas y «Mostrar todo» las demás |
| Diff: «⋯ N líneas sin cambios», «Mostrar todo» pasadas 400 filas, encabezado, archivo nuevo (`Diff.tsx:42,97-128`) | Hecho (tanda 6-8) | `DiffView` | |
| Diff: estadísticas con un diff de líneas real (`Diff.tsx:31`) | Hecho (tanda 6-8): `edits::edit_stats` (con test) usa el diff de líneas de gpui-m3 | `diff_cached(..).stats()` | Es el mismo conteo que `diff_stats`, pero reutiliza el diff que dibuja `DiffView` |

## 7. Sidebar, espacios y sesiones (`src/components/Sidebar.tsx`)

| Función de la referencia | Estado en Atic Code | gpui-m3 | Nota |
| --- | --- | --- | --- |
| Barra abierta/cerrada y botón para ocultarla (`App.tsx:88`, `Sidebar.tsx:366`) | Hecho `sidebar.rs:68,83` | `NavItem`/`RailItem` | |
| Botón buscar/comandos (`Sidebar.tsx:369`) | Hecho `sidebar.rs:80` | `IconButton` | |
| «Nueva conversación» con ⌘N a la vista (`Sidebar.tsx:375`) | Parcial: no muestra el atajo (`sidebar.rs:96`) | `Fab` | |
| Historial (`Sidebar.tsx:380`) | Hecho `sidebar.rs:97` | `NavItem` | |
| Menú «Proyectos +»: nuevo, abrir, carpeta, importar VS Code (`Sidebar.tsx:387`, `src/components/Overlays.tsx:37-63`) | Hecho: el botón abre «Nuevo espacio» (tanda 3; tanda 21: también en Formal y Glass, `new_space_dialog`); abrir carpeta va por Ctrl+O y la paleta | `Menu` | Importar VS Code: tanda 23, botón «Importar de VS Code…» del diálogo (`import_vscode`, `sidebar.rs`). Ver Decisiones (espacios) |
| Diálogo «Nuevo workspace» con nombre y carpetas (`Overlays.tsx:411-483`) | Hecho (tanda 3): `new_space_dialog` (`sidebar.rs`) | `Dialog`, `TextField`, `Button` | Las carpetas con el diálogo nativo, no `space/picker.rs` (Decisiones) |
| Orden: favoritos primero y luego el del usuario (`store.ts:130-144`) | Hecho (tanda 3 y 13): `config::sidebar_order(ids, favoritos, orden)` (con tests) | — | «El del usuario» es `Configs::order` (tanda 13), el orden de la lista para los espacios sin lugar guardado |
| Reordenar arrastrando (`Sidebar.tsx:313-351`) | Hecho (tanda 13): `ReorderList` en `sidebar_m3`, `reorder_spaces` (`sidebar.rs`); el orden va a `Configs::order` (`code-claude.json`, no al Mando) con `set_order` (`config.rs`) | `ReorderList` (no hay `DragHandle`: se arrastra desde toda la fila) | Dos grupos: favoritos (0) y el resto (1). **No se probó en la app.** Solo en la barra de Expressive |
| Estrella de favorito animada (`Sidebar.tsx:152-176`) | Hecho (tanda 3), en la fila y en el menú contextual del espacio | `FavStar` | Favoritos en `code-claude.json` |
| Plegar el proyecto y recordarlo (`store.ts:145`) | Hecho `sidebar.rs:156`, `src/space/workspaces.rs:184` | `NavItem` | |
| Desplegar al abrir el proyecto la primera vez (`store.ts:153`) | Hecho (tanda 3): `expand_first_open` + `Configs::opened` | — | |
| Avatar del proyecto que gira 40° al hover (`Sidebar.tsx:212`, `motion.css:770`) | Hecho (tanda 20): `project_m3` (`sidebar.rs`) con `hover_spin` + `hovered` según toda la fila | `Avatar::hover_spin` | |
| «+» al pasar el cursor, carga diferida, últimas 3 y «Ver todas (n)» (`Sidebar.tsx:184-258`) | Hecho `sidebar.rs:21,143-187` | `IconButton` | |
| Conversaciones vivas primero; fila activa; indicador «respondiendo»; punto de no leído; «hace X» (`Sidebar.tsx:55-137`) | Hecho `sidebar.rs:172-233` | `LoadingIndicator`, `Badge::dot` | |
| No leído cuando otro cliente retoma la conversación (`store.ts:508`) | Hecho (tanda 2): `resync` (`rewind.rs`) marca `unread` si no es la visible | — | |
| Entrada escalonada de las conversaciones (`motion.css:727`) | Hecho (tanda 20): `session_row` (`sidebar.rs`), 30 ms entre filas, desde la izquierda | `Enter` | |
| Ícono de marcador en la fila (`Sidebar.tsx:125`) | Hecho (tanda 2): `session_row` y `history_row` (`sidebar.rs`) | `NavItem::icon` | Solo en Expressive |
| Menú contextual: Abrir, Renombrar en el sitio, Eliminar con confirmación (`Sidebar.tsx:89-115`) | Hecho; guarda al perder el foco (tanda 3, `commit_renames_on_blur`). Tanda 23: también en Formal y Glass (`chat_rows`, `view.rs`: clic derecho en las conversaciones abiertas y guardadas, con el campo de renombrar en la fila) | `MenuItem::danger().confirm()`, `TextField::inline` | |
| Menú contextual: Agregar/Quitar marcador (`Sidebar.tsx:91`) | Hecho (tanda 2): `session_menu_layer` (`sidebar.rs`); tanda 23: también en Formal y Glass | `MenuItem` | |
| Chats sueltos, sin proyecto (`Sidebar.tsx:277-305`, `store.ts:453,575`) | Hecho (tanda 3): sección «Chats» (`loose_chats`, cinco y «Ver todos») | — | Tanda 21: también en Formal y Glass (`chat_rows(LOOSE, …)` en `sidebar`, `view.rs`) |
| Pie: perfil, Apariencia, Configuración (`Sidebar.tsx:423`) | Hecho (tanda 3 y 4): Apariencia abre su pestaña; el perfil abre su tarjeta | `IconButton` | Tanda 21: Formal y Glass tienen la fila de perfil (avatar y nombre, `toggle_profile`) sobre Configuración; el recorte de la foto es el mismo diálogo de gpui-m3 |
| Riel M3 con su avatar, que abre la barra (`Sidebar.tsx:434-467`) | Hecho (el avatar abre la barra desde la tanda 3) | `RailItem`, `Fab`, `Avatar` | |
| Página Historial con búsqueda (`Chat.tsx:316-346`) | Hecho `sidebar.rs:345` | `TextField` | |
| Historial: filtro «Marcadores» (`Chat.tsx:335`) | Hecho (tanda 2): `Chip` «history-marked» (`history_view`, `sidebar.rs`) | `Chip` (filtro) | |
| Historial: rama git de cada sesión (`Chat.tsx:274`) | Hecho (tanda 3): `SessionInfo::branch` de `gitBranch` | — | «hace X · rama» |
| Página y filas del historial escalonadas (`motion.css:784`) | Hecho (tanda 20): `history_view` (`sidebar.rs`), 25 ms entre filas, tope 8 | `Enter` | |
| Máximo 4 procesos vivos; cierra los inactivos (`store.ts:462`) | Hecho (tanda 3): `limit_live` + `idle_to_close` (con test) | — | Cierra el proceso, no la conversación (Decisiones) |
| Cerrar las conversaciones precalentadas vacías (`store.ts:599`) | No aplica (Atic no precalienta) | — | |
| Título de la ventana «proyecto — Atic Code» (`store.ts:101`) | Hecho (tanda 3), en `render`; «Chats — Atic Code» en un chat suelto | — | |
| Avisos al abrir: importado, carpetas que faltan (`store.ts:102`) | Hecho (tanda 23): `warn_missing_folders` (`mod.rs`) al arrancar y en `select_workspace` («No se encontró: a, b», con `vscode::missing_note`, con test); al importar de VS Code, «Importado desde VS Code» | `Toast` | la referencia avisa «Guárdalo como .referencia-workspace»: no aplica |
| Quitar un espacio y agregarle carpetas | Hecho (tanda 3): menú contextual del espacio (`space_menu_layer`); tanda 21: también con el clic derecho en la barra de Formal y Glass | `Menu`, `MenuItem::confirm` | También Renombrar (campo en la fila) y Favorito |
| `listSessions`, `renameSession`, `deleteSession` (`agent.ts:833`) | Hecho `mod.rs:556`, `sidebar.rs:537,559` | — | Límite de 40 (la referencia usa 50) |

## 8. Panel de contexto, archivos y git (`src/components/ContextPanel.tsx`)

| Función de la referencia | Estado en Atic Code | gpui-m3 | Nota |
| --- | --- | --- | --- |
| Panel derecho con Cambios/Archivos, título, contador y cerrar (`ContextPanel.tsx:347-383`) | Hecho `view.rs:1782-1841` | `Badge` | |
| Actualizar con spinner (`ContextPanel.tsx:361`) | Hecho (tanda 3): `refreshing` + `LoadingIndicator` | `LoadingIndicator` | Al pedirlo y al terminar cada respuesta, no en el sondeo |
| Árbol con las raíces como carpetas, carga diferida (`ContextPanel.tsx:27-69`) | Hecho `view.rs` (`file_tree`), `src/space/explorer.rs:80` | `TreeRow` en Expressive (tanda 3) | |
| Color del nombre según el tipo; dotfiles atenuados; archivo activo resaltado (`ContextPanel.tsx:18-60`) | Hecho (tanda 3): `files::kind_of` + `kind_color`; el activo es el último abierto (`active_file`) | `TreeRow` | Como la referencia, se pinta el ícono. Formal/Glass (tanda 21): ícono por tipo, dotfiles atenuados y archivo abierto resaltado en `file_tree` y `file_results` (`view.rs`) |
| Ocultar node_modules, target y dist (`src-tauri/src/fs.rs`) | Hecho (tanda 6-8): `Explorer::for_code` (con test) | — | Solo el árbol de Atic Code; el del Mando sigue ocultando solo `.git` |
| Buscar archivos con índice (120 resultados) (`ContextPanel.tsx:71-131`) | Hecho (tanda 3): `src/code/files.rs` (`index`, `search`, con tests) | `TextField`, `TreeRow` | El índice se reutilizará en las @-menciones; Esc limpia |
| Cambios agrupados por repo, con rama (`ContextPanel.tsx:267-305`) | Hecho `view.rs:2218,2242`, `src/code/git.rs:55` | `Card`/`ListGroup` | |
| Grupo plegable; «Al día»; «No es un repositorio git» (`ContextPanel.tsx:268-297`) | Hecho (tanda 3): `repo_group` (`view.rs`), `Repo::is_repo` (`git.rs`) | `ExpandableCard` en Expressive | Formal/Glass: encabezado propio que se pliega |
| «Todo al día» con ícono (`ContextPanel.tsx:318`) | Hecho (tanda 3) | — | Solo si alguna carpeta es un repo (Decisiones) |
| Resumen n archivos +a −r (`ContextPanel.tsx:330`) | Hecho `view.rs:1946` | — | |
| Fila de cambio: estado, nombre, carpeta, +/− (`ContextPanel.tsx:239`) | Hecho `view.rs` (`change_row`) | `ListItem` | Los no seguidos salen como `N` (tanda 3) |
| Barra de 5 bloques del diff (`ContextPanel.tsx:225`) | Hecho (tanda 6-8): `DiffBar` en `change_row` | `DiffBar` | |
| Ver el diff de un cambio y «Abrir» el archivo (`ContextPanel.tsx:190-222`) | Hecho `view.rs` (`doc_body`, `doc_diff`) y `space::viewer` | `DiffView` | |
| Volver del diff a la lista (`ContextPanel.tsx:153,202`) | Parcial: la X cierra el archivo | `IconButton` | |
| Panel más ancho al editar (`ContextPanel.tsx:351`) | Hecho `view.rs:1784` | — | |
| Commit, ramas, stage, descartar | No aplica (la referencia tampoco los tiene) | — | |

## 9. Terminal (`src/components/Terminal.tsx`, `src/lib/terminals.ts`, `src/styles/terminal.css`)

Hecho (tanda 10): `src/code/terminal.rs` (`Terminals`, una entidad aparte con su foco y su manejador de texto) reutiliza las consolas del Mando: `space::console` (alacritty + ConPTY) con `Palette`, `space::grid_of`/`paint_grid` con `GridLook` y `space::input::layer` genérico. Validado con `cargo check` y `cargo test code::`/`space::`; **no se probó en la app**.

| Función de la referencia | Estado en Atic Code | gpui-m3 | Nota |
| --- | --- | --- | --- |
| Panel bajo el chat (`Terminal.tsx:184-288`, `Chat.tsx:359`) | Hecho (tanda 10): `Terminals` como último hijo de `center` | No aplica (la consola es de Atic) | Expressive: contenedor tonal con margen; Glass: tarjeta; Formal: borde arriba |
| Pestañas: nueva «+», cerrar/matar, número (`terminals.ts:11-35`, `Terminal.tsx:217-273`) | Hecho (tanda 10): `Terminals::head` con `TabStrip`; cerrar suelta la consola (`Shutdown`) | `TabStrip` | El número va pegado al nombre cuando hay varias sin nombre propio |
| Renombrar la pestaña con doble clic (máx. 40) (`Terminal.tsx:226-264`) | Hecho (tanda 10): `start_rename`/`commit_rename`, `clean_name` (con test); guarda al perder el foco | `TabStrip` (`editing`/`on_rename`) | |
| «[proceso terminado]» y título según el shell (`Terminal.tsx:126-131`) | Hecho (tanda 10): `Console::note` escribe el aviso en la pantalla; la pestaña lleva el nombre del shell (`shell_name`) | — | |
| Ejecutar un comando al abrir (`claude --resume`) (`terminals.ts:18`) | Hecho (tanda 10): `run_in_terminal`; «Abrir Claude en la terminal», Remote Control, plugins y cuenta abren una pestaña con nombre en vez de `wt.exe` | — | PowerShell: la ruta de `claude` entre comillas va con `&` |
| Mostrar/ocultar con ⌃\` y ⌘J (`terminals.ts:50`, `App.tsx:21,37`) | Hecho (tanda 10): `ToggleTerminal` con Ctrl+J y Ctrl+\`; sin pestañas abre una | — | Dentro de la terminal Ctrl+J es del shell (`SHELL_KEYS`) |
| Botón Terminal con contador en la barra superior (`Chat.tsx:82`) | Hecho (tanda 10): `terminal_button` en los tres estilos | `Button` + `Badge` | |
| Alto ajustable y recordado (`Terminal.tsx:182-209`) | Hecho (tanda 10): `ResizeHandle` arriba del panel; el alto va en `code-claude.json` (`terminal_height`) | `ResizeHandle` | Entre 120 px y lo que deja 220 px al chat (`clamp_height`, con test) |
| Paleta ANSI por estilo y modo (`Terminal.tsx:19-88`) | Hecho (tanda 10): `terminal::ansi` (con test) + `console::Palette`; texto del estilo, cursor del acento | — | |
| Cursor, scrollback de 5000, reajuste y foco (`Terminal.tsx:94-157`) | Hecho (tanda 10): `Terminals::sync` ajusta las consolas al tamaño del panel; rueda, clic para enfocar, Ctrl+V pega | — | Cursor de bloque, no de barra; sin seleccionar con el ratón |
| Entrada por estilo: `m3-rise`, `glass-condense` (`terminal.css:176,217`) | Parcial (tanda 20): `m3-rise` en Expressive (`Terminals::render`); Glass no | `Enter` | |

## 10. Editor de código (`src/components/CodeEditor.tsx`, `src/lib/editor.ts`)

Hecho (tanda 11): `src/code/editor.rs` (estado de pestañas, guardar, recargar, contexto, con tests de la lógica pura) y el enganche en `mod.rs`/`view.rs` (`editor_body`, `right_panel`, `file_chip`). Un archivo abierto desde el árbol, una herramienta o una ruta del chat va a un `CodeEditor` de gpui-m3 con `SyntaxLines::for_path`; los diffs del panel de Cambios siguen en el visor (`DiffView`) y su botón «Abrir» pasa el archivo al editor. Validado con `cargo check` y `cargo test code::` (88 tests); **no se probó en la app**.

| Función de la referencia | Estado en Atic Code | gpui-m3 | Nota |
| --- | --- | --- | --- |
| Abrir desde el árbol (`store.ts:744`) | Hecho (tanda 11): `open_file` desde el árbol, las herramientas y las rutas del chat (`a.rs:42` deja el cursor en la línea) | — | Un archivo ya abierto reutiliza su pestaña |
| Pestañas de archivos abiertos (`ContextPanel.tsx:149-175`) | Hecho (tanda 11): `editor_body` con `TabStrip`; `Tab::dirty` para los sucios | `TabStrip` | La X del encabezado vuelve a Archivos sin cerrar nada (la flecha de la referencia) |
| Cerrar con la vecina (`closeTab`, `store.ts:753`) | Hecho (tanda 11): `after_close` (con test) | — | |
| Confirmar al cerrar si hay cambios sin guardar (`ContextPanel.tsx:135`) | Hecho (tanda 11): `close_dialog` con Guardar, Descartar y Cancelar | `Dialog` | la referencia solo pregunta «Descartar / Cancelar»; aquí se agregó Guardar |
| Editar (CodeMirror) (`CodeEditor.tsx:86-191`) | Hecho (tanda 11) | `CodeEditor` | Sin plegado, multicursor ni buscar/reemplazar (gpui-m3 no los trae). La tabulación dura se dibuja de una columna |
| Números de línea (`CodeEditor.tsx:113`) | Hecho (tanda 11): los del `CodeEditor` | `CodeEditor` | |
| Resaltado por lenguaje (`CodeEditor.tsx:40-84`) y colores `--sx-*` (`src/styles/tokens.css:55`) | Hecho (tanda 11): `SyntaxLines::for_path`; los colores se piden otra vez al cambiar de estilo, modo o acento (`refresh_editor_colors`) | `SyntaxLines` | Colores del esquema (no los `--sx-*`) |
| Guardar con Mod-S (`CodeEditor.tsx:96`) | Hecho (tanda 11): Ctrl+S → `save_tab` (`fs::write`); si falla, toast; después `refresh_changes_now` | `CodeEditorEvent::Save` | No es escritura atómica |
| Deshacer, sangría, corchetes (`CodeEditor.tsx:113-129`) | Hecho (tanda 11): los del `CodeEditor` | `CodeEditor` | Plegado y multicursor: faltan en gpui-m3 |
| Recargar si Claude editó y no hay cambios propios (`editor.ts:28`) | Hecho (tanda 11): `files_edited` tras Edit/Write/MultiEdit; conserva cursor o selección (`reload_decision`, con test) | `CodeEditor::set_text`/`select` | Con cambios propios no se pisa: aviso «El archivo cambió en el disco» con Recargar / Mantener lo mío. El desplazamiento vuelve arriba al recargar |
| Cursor y selección como contexto para Claude (`editor.ts:10`, `CodeEditor.tsx:132`) | Hecho (tanda 11): chip `archivo:L1-L2` en el composer (ver sección 2) | `CodeEditor::selection` | |
| Archivos grandes, borrados, binarios o no UTF-8 | Hecho (mejora de Atic): la pestaña muestra el motivo (`read_text`) | — | No UTF-8 no se edita para no dañarlo al guardar |

## 11. Overlays y paleta (`src/components/Overlays.tsx`, `src/components/ContextMenu.tsx`)

| Función de la referencia | Estado en Atic Code | gpui-m3 | Nota |
| --- | --- | --- | --- |
| Paleta ⌘K/⌘P con flechas, Enter, Esc y filtro (`Overlays.tsx:528-599`) | Hecho `src/code/palette.rs:24-71` | `CommandPalette` (ratón hecho en `777021d`) | |
| Comandos: nueva conversación, historial, configuración, actualizar Claude, ver cambios/archivos, 30 conversaciones, estilo y modo (`Overlays.tsx:496-523`) | Hecho `palette.rs:43-61` | — | |
| Comando: nuevo chat sin proyecto (`Overlays.tsx:497`) | Hecho (tanda 3): `PaletteAct::NewLoose` | — | |
| Comandos: nuevo, abrir o importar workspace (`Overlays.tsx:498-501`) | Hecho (tanda 23): «Nuevo espacio…», «Abrir carpeta…» y «Importar workspace de VS Code…» (`PaletteAct::ImportVscode`, `palette.rs`) | — | |
| Comandos: guardar, guardar como, cerrar workspace (`Overlays.tsx:510-512`) | No aplica (los espacios se guardan solos) | — | |
| Comando: abrir pestañas de archivos (`Overlays.tsx:513`) | Hecho (tanda 23): `PaletteAct::Tab` (`palette.rs`), un «Abrir <archivo>» por cada pestaña del editor | — | la referencia los lista igual: solo con un proyecto abierto |
| Toast de 3,2 s (`store.ts:39`) | Hecho `agent_menu.rs:613`, `view.rs:193` | `Toast` | |
| Menú contextual ajustado a la ventana, que cierra con Esc, clic fuera o blur (`ContextMenu.tsx:17-84`) | Hecho `sidebar.rs:468` | `context_menu()` (Atic usa `anchored` a mano) | No cierra cuando la ventana pierde el foco |
| Salidas animadas de menús, diálogos y toasts (`motion.css:548-595`) | Hecho (tanda 4): ver «Salida animada de los popovers»; los diálogos usan `Dialog::exit` (commit propio en gpui-m3, porque un `Presence` no atenúa lo diferido) | `Presence`, `Dialog::exit` | La paleta de comandos no sale animada |
| Diálogo: velo y crecimiento desde el centro con radio 56 (`motion.css:598`) | Hecho (tanda 20): los `Dialog` de Atic usan la entrada nueva de gpui-m3 sin cambios | `Dialog` | Sin ver en la app |
| Snackbar que sube y crece (`motion.css:615`) | Hecho (tanda 4): entra con `Toast` y sale con `Exit::Sink` | `Toast` | |

## 12. Perfil y uso (`src/components/Profile.tsx`, `src/lib/profile.ts`, `Agent.tsx:676-979`)

| Función de la referencia | Estado en Atic Code | gpui-m3 | Nota |
| --- | --- | --- | --- |
| Perfil local con nombre y foto (`profile.ts:5-33`) | Hecho (tanda 4): `src/code/profile.rs`, `code-profile.json` y el PNG recortado junto a él (con tests) | — | |
| Nombre de git como valor inicial (`profile.ts:36`) | Hecho (tanda 4): `Profile::shown_name` | — | Si se borra el nombre, vuelve a verse el de git |
| Iniciales (`profile.ts:43`) | Hecho `sidebar.rs:579` | `Avatar` | |
| Avatar galleta que cambia de forma al hover (`Profile.tsx:15-28`, `src/styles/app.css:4749`) | Hecho (tanda 4): `profile_avatar`, Cookie9 → Clover8 en la barra, el riel y la tarjeta | `Shape`/`Avatar::breathe_on_hover` | |
| Avatar con foto (`Profile.tsx:17`) | Hecho (tanda 4): `Avatar::image` | `Avatar::image` | |
| Tarjeta de perfil: editar el nombre, cambiar o quitar la foto (`Profile.tsx:207-254`) | Hecho (tanda 4): `profile_layer` (`Popover`, `TextField`, «Cambiar foto» y «Quitar») | `Popover`, `TextField`, `Button` | El nombre se guarda al escribir |
| Recortar la foto: arrastrar, zoom, máscara, Enter/Esc (`Profile.tsx:55-182`) | Hecho (tanda 4): `crop_dialog` con `ImageCropper` (Cookie9, 256 px) y guardado como PNG | `ImageCropper` | Sin probar en la app |
| Popover «Cuenta y uso»: tokens, plan, entrada/salida/caché, costo (`Agent.tsx:713-818`) | Hecho (tanda 3): `usage_windows` lee `rate_limits.limits` (con tests) y si no, las ventanas viejas | `Popover`, `Ring`, `WavyProgress` | |
| Mapa de agentes (`Agent.tsx:851-979`) | Hecho (tanda 3): `agent_map` + `task_row` (`usage.rs`), `task_kind` y `Chat::task_label` con tests | `MorphDot`, `LoadingIndicator`, `IconButton` | Conserva la fila de la conversación principal (extra de Atic) |
| Plan de la cuenta (`Overlays.tsx:246`) | Hecho `settings_m3.rs:25`, `usage.rs:536` | — | |

## 13. Ajustes y apariencia

| Función de la referencia | Estado en Atic Code | gpui-m3 | Nota |
| --- | --- | --- | --- |
| Elegir el estilo Formal, M3 o Glass (`src/lib/theme.ts:160`) | Hecho `style.rs:18`, `mod.rs:1016` | — | Atic parte en Expressive |
| Modo claro/oscuro/sistema (`theme.ts:166`) | Hecho (tanda 3): «Sistema» sigue a Windows (`style::set_system_light`) | — | |
| Seguir en vivo el cambio del sistema (`theme.ts:118`) | Hecho (tanda 3): `observe_window_appearance` | — | Sin probar en la app |
| Tarjetas de estilo con miniatura (`Overlays.tsx:68`) | Hecho (tanda 3) en Expressive | `SelectCard` | Formal/Glass: también tarjetas (`appearance_tab_flat`, `settings_flat.rs`, tanda 21) |
| Segmentado «Modo de color» (`Overlays.tsx:219`) | Hecho en M3 (`settings_m3.rs:359`); Formal y Glass (`appearance_tab_flat`) | `SegmentedButtons` | |
| Acento: 9 sugeridos, propio, restablecer (`Overlays.tsx:97-136`, `theme.ts:46`) | Hecho (tanda 3): `accent_picker_ui` (`settings_m3.rs`), también en la Configuración de Formal/Glass | `ColorSwatches`, `HsvPicker`, `Button` | |
| Acento por proyecto (`Overlays.tsx:138`, `store.ts:316`) | Hecho (tanda 3): `Configs::accents` por espacio y estilo | — | No hay acento global de la app (Decisiones) |
| Esquema M3 desde la semilla (`theme.ts:72`) | Hecho (tanda 3): `style::with_accent` y `apply_m3` | `Scheme::from_seed` | gpui-m3 usa la especificación de color de 2021 y la referencia la de 2025 |
| Acento simple de Formal/Glass: tono HCT 62/52, `accent-soft`, `sel` (`theme.ts:57`) | Hecho (tanda 3): `style::simple_accent` (con test, crate `material-colors`) | — | |
| Menú rápido de Apariencia (`Overlays.tsx:157`) | Hecho (tanda 23): `style_menu_layer` / `toggle_style_menu` (`sidebar.rs`), un popover junto al botón con el mismo contenido de la pestaña (`appearance_tab`, `appearance_tab_flat`); Esc lo cierra. Solo los botones de la barra de Expressive lo abren | `Popover` | Formal y Glass no tienen el botón en su barra |
| Configuración con pestañas Claude y Apariencia (`Overlays.tsx:173-229`) | Hecho en M3 (`settings_m3.rs:86`); Hecho en Formal/Glass (tanda 21, `9e2818d`): `settings_flat` con `claude_tab_flat`, `project_defaults_flat` y `appearance_tab_flat` (`settings_flat.rs`) | `Dialog`, `NavItem::large` | El estado de la instalación (`claude_health`) se comparte con Expressive y tiene test |
| Tarjeta de Claude Code: versión, estado, actualizar, registro (`Overlays.tsx:249-356`) | Hecho `settings_m3.rs:159-290` | `Shape`, `Card` | Usa `v != l` en vez de `newer(a,b)`; después de actualizar (`update_claude`) reinicia la conversación vacía con `restart_idle_chat` (tanda 23) |
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
| Configuración: cajón, lista segmentada, tarjeta de Claude (`expressive.css:914-1131`) | Hecho `settings_m3.rs` | `ListGroup`, `NavItem::large` | Contenido: sube la pestaña entera (tanda 20), no sección por sección |
| Punto de no leído que salta; pulgar del switch (`motion.css:694,809`) | Hecho | `Badge::pop`, `Switch` | |
| Interpolación de colores al cambiar tema o acento (`motion.css:9-84`) | Hecho (tanda 20), solo Expressive: `sync_style` (`mod.rs`) llama `animate_scheme` y `style::begin_blend`/`mix_tokens` mezcla los tokens propios (`style.rs`) 0,45 s | `animate_scheme` | Sin ver en la app |
| Transición de tema: revelado circular M3 (650 ms) y fundidos de Formal y Glass (`theme.ts:137`, `app.css:4842-4902`) | Parcial (tanda 20): revelado circular al cambiar estilo/modo hacia Expressive (`set_appearance`, `apply_pending_look`, `ThemeReveal` en `view.rs`); sin fundidos de Formal y Glass | `theme_reveal`, `ThemeReveal` | Atic cambia sus colores al cubrirse la ventana |
| Composer que «respira» al enfocar (`motion.css:411`, `expressive.css:502-560`) | Parcial: cambia el fondo sin animar (`view.rs:1680`) | `animate_color` | |
| Burbujas con resorte desde su esquina (`motion.css:425`) | Hecho (tanda 4): ver «Entrada animada de cada parte nueva» | `Bubble::entrance` | |
| Panel y barra lateral con resorte (`motion.css:444`) | Hecho (tanda 20): `Enter` desde la derecha (32 px) en `right_panel` y desde la izquierda (24 px) en `sidebar_m3` | no hay `slide_in`: `enter.rs` | Sin escala 0,97 |
| Muestras de color que giran a rombo; pulso de la tarjeta de estilo (`motion.css:495-532`) | Hecho con los componentes (tanda 3) | `ColorSwatches`/`SelectCard` | Lo que animen viene de gpui-m3 |
| Separador de compactación (`motion.css:673`) | Hecho (tanda 2 y `c820b5d`): `compact_mark` (`view.rs`) | `DividerLabel` | |
| Filas que se encogen al presionar (`motion.css:752`) | Parcial: solo redondean | — | GPUI no escala |
| Título del hero con peso y anchura animados (`motion.css:378`) | No aplica (GPUI no anima `font-stretch`) | — | |
| Estrella de favorito M3 (giro + estallido) (`app.css:3931`) | Hecho (tanda 3) | `FavStar` | |
| Zona para soltar (`app.css:4430`) | Hecho (`c820b5d`): `drop_zone` (`view.rs`) | `DropZone` (`777021d`) | |
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
| Evento `rate_limit` (`agent.mjs:266`) | Hecho (tanda 23): `read_rate_limit` y `apply_rate_limit` (`usage.rs`, con test) ponen el porcentaje y el reinicio de la ventana en «Cuenta y uso» y dejan un chip de aviso (`RateAlert`) cuando el servidor dice `allowed_warning` o `rejected` | — | `utilization` se lee como fracción (0 a 1), según el SDK; sin probar con el servidor real. Sin cambios en el sidecar |
| Evento `system` → `compact_boundary` (`agent.mjs:283`) | Hecho (tanda 2): `Chat::apply` (`chat.rs`) | — | |
| Vigilar la sesión desde fuera → `external` (`agent.mjs:377-444`) | Hecho: sidecar y cliente (tanda 2, `resync` en `rewind.rs`) | — | |
| `send`, `close`, `permission`, `setPermissionMode`, `applyFlags`, `setThinking` (`agent.mjs:457-497`) | Hecho `mod.rs` | — | |
| `interrupt` (`agent.mjs:466`) | Hecho (tanda 2): `interrupt` (`mod.rs`) y «Detenido.» (`Chat::result`) | — | |
| `setModel` (`agent.mjs:493`) | Hecho (`b1a3efc`, por conversación) | — | |
| `rewindFiles` (`agent.mjs:498`) | Hecho (tanda 2): `rewind` (`rewind.rs`) | — | |
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
| `from_vscode` (importar `.code-workspace`) (`workspace.rs:138`) | Hecho (tanda 23): `vscode::parse`/`read` (`src/code/vscode.rs`, con tests de JSONC y rutas relativas) | — | Solo lleva las carpetas y el nombre del archivo; no los ajustes ni el `name` de cada carpeta |
| `recent_list`/`recent_remove` (`lib.rs:57`, `recent.rs`) | No aplica (la lista de espacios hace de recientes) | — | |
| `fs_list_dir` (`lib.rs:67`) | Hecho `src/space/explorer.rs:39` | — | |
| `fs_read_text` (`lib.rs:72`) | Hecho `src/space/viewer.rs:64` (límite de 4 MB) | — | |
| `fs_write_text` (`lib.rs:92`) | Hecho (tanda 11): `editor::write_text` (`fs::write`) para el editor; exportar y CLAUDE.md escriben aparte (`agent_menu.rs:549,587`) | — | |
| `fs_read_base64` (`lib.rs:87`) | Hecho `mod.rs:863` | — | El perfil también lo necesita |
| `fs_save_pasted` (`lib.rs:82`) | Parcial (solo imágenes) | — | |
| `fs_index` con `.gitignore` (`lib.rs:108`) | Hecho (tanda 3): `src/code/files.rs` (crate `ignore`) | — | Para las @-menciones y la búsqueda de archivos |
| `user_name` (`lib.rs:77`) | Hecho `sidebar.rs:585` | — | |
| `git_status` (`lib.rs:97`) | Hecho `src/code/git.rs:55` | — | |
| `git_at_head` (`lib.rs:114`) | Hecho de otra forma: `git diff HEAD` (`viewer.rs:115`) | — | |
| `terminal_spawn`/`write`/`resize`/`kill`, `kill_all` (`lib.rs:119-182`, `terminal.rs`) | Hecho con `space::console` (tanda 10) | — | |
| Chats sueltos: `createCwd`, `~/.referencia/chats` (`store.ts:453-583`) | Hecho (tanda 3): `LOOSE`, `loose_dir` (`<datos de Atic>\pill\code-chats`), `LOOSE_CONTEXT` | — | |

## 17. Atajos

| Función de la referencia | Estado en Atic Code | gpui-m3 | Nota |
| --- | --- | --- | --- |
| ⌘K y ⌘P paleta, ⌘N nueva, ⌘B barra, ⌘U adjuntar, ⌘, configuración (`App.tsx:30-40`) | Hecho con Ctrl (`mod.rs:73-78`) | — | |
| ⌘L nueva conversación, salvo en el editor (`App.tsx:39`) | Hecho (tanda 3) con Ctrl+L | — | No actúa dentro de la terminal (tanda 10) ni del editor (tanda 11) |
| ⌘O abrir workspace (`App.tsx:33`) | Hecho (tanda 3) con Ctrl+O: abre carpetas como espacio nuevo | — | |
| ⌘E Archivos / ⌘G Cambios (`App.tsx:35-36`) | Hecho (tanda 3) con Ctrl+E y Ctrl+G | — | |
| ⌘J y ⌃\` terminal (`App.tsx:21,37`) | Hecho (tanda 10) con Ctrl+J y Ctrl+\` | — | |
| Esc cierra la paleta, la apariencia y la configuración (`App.tsx:26`) | Hecho `mod.rs:952` | — | |
| Esc con un permiso pendiente lo rechaza (`Permission.tsx:92`) | Hecho (tanda 2): `close_menu` y `permission_key` (Deny) | — | Decisión tomada |
| Esc Esc abre Rewind (`Agent.tsx:1342`) | Hecho (tanda 2): `close_menu` (`mod.rs`) | — | |
| @: ↑↓ Enter Tab Esc (`Agent.tsx:1330`) | Hecho (tanda 4): `bind_keys` en el contexto `CodeComposer > TextArea && suggesting` | `SuggestionList` | |
| Enter y Esc al renombrar una sesión (`Sidebar.tsx:103`) | Hecho (`restore_on_cancel`) | `TextField` | |
| Enter y Esc al renombrar una terminal; Esc en la búsqueda de archivos; Enter y Esc en el recorte y la tarjeta de perfil (`Terminal.tsx:251`, `ContextPanel.tsx:96`, `Profile.tsx:127,215`) | Parcial: Esc en la búsqueda de archivos (tanda 3); Enter y Esc en el recorte y la tarjeta de perfil (tanda 4) | — | Lo demás, junto con cada función |
| Enter o Espacio despliega la fila del proyecto (`Sidebar.tsx:207`) | Hecho (tanda 19): `project_m3` (`sidebar.rs`) envuelve el `NavItem` en un contenedor con foco (`focusable`, `use_focus`, `focus_ring`) y `toggle_project` pliega con Enter o Espacio | `gpui_m3::interaction` (`NavItem` no toma foco por sí mismo) | Tab llega a la fila con anillo de foco. **No se probó en la app** |
| Editor: Mod-S, Tab, deshacer, plegar (`CodeEditor.tsx:129`) | Hecho (tanda 11) salvo plegar: Ctrl+S, Tab, Ctrl+Z son del `CodeEditor` | `CodeEditor` | Dentro del editor, Ctrl+K, P, N, B, U, E, G, O, L, J y Ctrl+Enter no actúan (`editor::EDITOR_KEYS`, `NoAction` en el contexto `M3CodeEditor`). Ctrl+\` y Ctrl+, siguen siendo de Atic Code |
| Configuración de atajos (extra de Atic, la referencia no la tiene) | Hecho (tanda 24), **sin probar en la app**: pestaña «Atajos» en la Configuración de los tres estilos (`shortcuts_tab`, `shortcuts.rs`, llamada desde `settings_m3.rs` y `settings_flat.rs`). La tabla `shortcuts::TABLE` (acción, grupo, descripción, tecla, contexto) arma los `bind_keys` de la ventana, la terminal y el editor (`shortcuts::bind_keys`, `terminal_keys`, `editor_keys`); clic en la tecla captura la siguiente con `cx.intercept_keystrokes` (`start_capture`, `captured`), Esc cancela, `check` avisa de choques, teclas reservadas y letras sueltas; «Restablecer» por fila y «Restablecer todos»; se guarda en `Configs::shortcuts` (`code-claude.json`) y se aplica en vivo (`set_shortcuts`) | — | Las teclas viejas quedan en `NoAction` y se revincula todo (no se usa `clear_key_bindings`: vaciaría el teclado de las demás ventanas de la pill). Esc, pegar, la lista de @ y las teclas que pasan al shell o al editor solo se muestran |

---

## Siguiente

Prioridad de arriba hacia abajo. Cada tanda es chica y se prueba sola. **[m3]** es un componente nuevo en gpui-m3 (en su worktree, con ejemplo en la galería). **[code]** es lógica de Atic Code.

**Hecho:** `3cd87bb` (hotfix modelos), portado en `b1a3efc`.

**Hecho, tanda 3 [code]** (rama `dev`, un commit por letra): A composer, B chats sueltos, C barra lateral, D panel de cambios y archivos, E apariencia y F cuenta y uso con el mapa de agentes. Cubre las tandas 12, 14, 15 y 17 de abajo, casi toda la 13 y parte de la 5 y la 19. Validado con `cargo check` y `cargo test code::` (51 tests); **no se probó en la app** (`CODE_ALONE=1`). Lo que quedó de esas tandas:
- Tanda 5: el popover hacia donde haya más espacio.
- Tanda 13: hecha (`ReorderList`, ver sección 7); falta probarla en la app.
- Tanda 15: el menú rápido de Apariencia como popover propio (hecho en la tanda 23); `SelectCard` y el árbol con `TreeRow` solo en Expressive.
- Tanda 17: el evento `rate_limit` en vivo (hecho en la tanda 23).
- Tanda 19: el teclado en las filas de proyecto, hecho (Enter y Espacio, ver sección 17); falta probarlo en la app y las demás filas (conversaciones) siguen sin foco.
- Chats sueltos en Formal y Glass: resuelto en la tanda 21 (sección «Chats» en su barra).

**Hecho, tanda 23 [code]** (rama `dev`, un commit por grupo): pendientes chicos. Marcas «Cambiado a X» al cargar el historial (`history_model_marks`); evento `rate_limit` en vivo (`apply_rate_limit`, chip de aviso); reversión por campo si falla `sync_chat_settings`; tarjeta de Artifact con «Abrir»; importar `.code-workspace` desde la paleta y el diálogo «Nuevo espacio» (`src/code/vscode.rs`) y aviso de carpetas que faltan al abrir un espacio; «Abrir <archivo>» de las pestañas en la paleta; popover rápido de Apariencia; menú contextual de las conversaciones en Formal y Glass; Chrome y la actualización de Claude reinician la conversación vacía (`restart_idle_chat`). Ya estaban hechos y solo se verificaron: máximo de 10 imágenes, «Mira la imagen adjunta.», rutas relativas en los adjuntos, enfocar «Otro…» y Enter/Espacio en la cabecera de `ExpandableCard`. Se limpiaron las filas que seguían como **En curso** de la tanda 2 y las columnas gpui-m3 con «falta». Validado con `cargo check` y `cargo test code::` (99 tests, 9 nuevos); el sidecar no cambió; **no se probó en la app**. Lo que quedó:
- `utilization` del evento `rate_limit` se lee como fracción (0 a 1) según el tipo del SDK; falta verlo con el servidor real.
- Enter/Espacio en las tarjetas de herramienta de Formal y Glass (`tool`, `view.rs`): son un `div` con clic, sin foco.
- El menú rápido de Apariencia solo lo abren los botones de la barra de Expressive; Formal y Glass cambian el estilo y el modo por la paleta o Configuración.
- Importar de VS Code no lleva los ajustes ni el nombre de cada carpeta.
- Siguen pendientes: foco inicial real en los botones de permiso (`defaultToNo`), foco en la primera opción de las preguntas, `FlashHighlight` y `SelectableText` no existen en gpui-m3.

**Hecho, tanda 2 [code]** (`1c6a237`): uuids y Rewind, marcas y marcadores (`code-marks.json`), resync `external`, «Detenido.», cola de envío, teclado en permisos (Esc rechaza), compactación, «Conectando…» y recarga del visor.

**Hecho, gpui-m3 main** (sin push): tanda 1 (`777021d`: `DividerLabel`, `Banner`, `Presence`, `DropZone`, `ImageThumb`, `Avatar::image`, `SuggestionList`, foco en `ExpandableCard`, ratón en `CommandPalette`), tanda 2 (`936aded`: `Diff`/`DiffView`/`DiffBar`, `CodeOutput`, `SyntaxHighlighter` con syntect, `CodeBlock`, `Markdown`) y tanda 3 (`d7b6387`: `CodeEditor` + `SyntaxLines`, `ResizeHandle`/`Splitter`, `ReorderList`, `ImageCropper`, `theme_reveal`/`animate_theme`, altura con resorte en `ExpandableCard`, `Icon::pop`, `Avatar::hover_spin`, menú escalonado, entradas de `Dialog`/`Toast`, `MenuItem` de dos líneas). Con eso están hechos todos los **[m3]** de las tandas 6, 7, 8, 10, 11, 13, 14, 16 y 20.

**Hecho, tanda 4 [code]** (rama `dev`, un commit por grupo; gpui-m3 tiene un commit aparte, `Dialog::exit`, sin push): `c820b5d` (`DividerLabel`, `ImageThumb`, `DropZone`, `Banner`), salida animada con `Presence` y @-menciones (tanda 9), pulido del hilo y las herramientas (tanda 4), perfil editable con recorte (16) y avisos del sistema (18). Validado con `cargo check` y `cargo test code::` (71 tests) y `cargo test tray_icon`; **no se probó en la app**. Lo que quedó de la tanda 4:
- Formal y Glass: sus menús sí salen animados (comparten `menu_layer`); «Razonando…» en vivo resuelto en la tanda 21; siguen sin burbujas con entrada (motion, tanda 22).
- La paleta de comandos (`CommandPalette`) no sale animada.
- Los avisos del sistema usan el globo de la bandeja: no distinguen el tono con sonido ni agrupan. El clic abre Atic Code, no la conversación.
- «Nuevo espacio», la configuración y el recorte del perfil: en Formal y Glass resueltos en la tanda 21 (los diálogos de gpui-m3 siguen el esquema del estilo).

**Hecho, tandas 6, 7, 8 y 14 [code]** (rama `dev`, un commit por grupo): `Markdown` (con enlaces y rutas que se abren), `DiffView` en Edit, MultiEdit, Write, los permisos y el visor de cambios, `CodeOutput`, `DiffBar`, resaltado en el visor de solo lectura y árbol sin `node_modules`, `target` ni `dist`. Validado con `cargo check` y `cargo test code::`; **no se probó en la app**. Lo que quedó: imágenes del markdown (`Markdown::on_image`); `SyntaxPalette::set` con los `--sx-*` de la referencia; el visor del Mando (`space::mando`) sigue con su resaltado y diff antiguos; los números de línea del diff de Edit son los del fragmento.

**Para retomar, en este orden:** terminal (10, con `Splitter`); editor (11, `CodeEditor` + `SyntaxLines::for_path`); `ReorderList` en la barra (13); motion de la 20; luego 19, 21, 22 y 23. Ninguna de las tandas [code] desde la 2 se probó en la app.

**Hecho, tanda 20 [code]** (rama `dev`): motion M3 en Atic Code, solo Expressive. Nuevo `src/code/enter.rs` (`Enter`, `stagger`) sobre `motion::entrance`/`replay` para lo que gpui-m3 no trae (`slide_in`, `swap`). Revelado de tema, mezcla de acento/modo, `Icon::pop`, `hover_spin`, entradas del hero, barra, historial, chips, panel derecho, terminal, configuración y cambio de conversación. Menú escalonado, altura de `ExpandableCard`, `Dialog` y `Toast` ya llegan solos con gpui-m3. Validado con `cargo check` y `cargo test code::` (89 tests); **no se probó en la app**. Lo que quedó: fundidos de Formal y Glass; las secciones de la configuración no entran una por una; la paleta de comandos y Glass sin entradas; sin escala (GPUI no escala).

**Hecho, tanda 21 [code]** (rama `dev`; `9348cb1`, `f2793f0`, `9e2818d` y cuatro commits más): funcionalidad de la referencia en Formal y Glass, sin su motion. `9348cb1`: tarjetas de permiso completas y apiladas (`permissions.rs`) y composer que crece hasta 240 px (`composer_box`). `f2793f0`: menú del agente con filas neutras (`agent_menu.rs`) que cada estilo dibuja (`menus.rs`). `9e2818d`: Configuración con pestañas Claude y Apariencia (`settings_flat.rs`). Después: perilla de esfuerzo de Glass (`effort_knob`, `menus.rs`); «Razonando…» en vivo con última línea y markdown, y sección «Chats» (`other_item` y `sidebar`/`chat_rows`, `view.rs`); «Nuevo espacio», menú contextual del espacio (con renombrar en la fila), favoritos primero y fila de perfil con tarjeta y recorte en la barra de Formal y Glass (los diálogos y menús de gpui-m3 siguen el esquema del estilo, así que son los mismos que en Expressive); y árbol de archivos con ícono por tipo y archivo abierto resaltado. Validado con `cargo check` y `cargo test code::` (90 tests); **no se probó en la app**. Lo que quedó: la perilla no se anima ni se arrastra fuera de las paradas; el menú contextual de las conversaciones (hecho en la tanda 23) y el arrastre para reordenar proyectos no están en Formal y Glass; el diálogo de «Nuevo espacio», la tarjeta de perfil y el recorte salen con la forma de gpui-m3 (esquinas y botones M3), no con controles propios de Formal; el motion propio sigue en la tanda 22.

**Hecho, tanda 10 [code]** (rama `dev`): terminal integrada (ver sección 9). El refactor de `space` que estaba en el stash se retomó tal cual (paleta, grilla e `input::layer` compartidos con el Mando). Dentro de la terminal, los atajos de Atic Code con Ctrl+letra y Esc no actúan: son del shell (Decisiones). Falta: seleccionar y copiar texto con el ratón, la entrada animada del panel, y que una terminal sobreviva a cerrar la ventana de Atic Code (hoy muere con ella).

**Hecho, tanda 11 [code]** (rama `dev`): el editor de código (ver sección 10). `src/code/editor.rs` guarda las pestañas, guarda con Ctrl+S, recarga o avisa cuando Claude edita, y arma el contexto del composer; `mod.rs` y `view.rs` solo lo enganchan. Validado con `cargo check` y `cargo test code::` (88 tests, 8 nuevos); **no se probó en la app**. Lo que quedó:
- El comando «Abrir pestañas de archivos» de la paleta.
- Plegado, multicursor y buscar/reemplazar (faltan en `CodeEditor`); la tabulación dura se dibuja de una columna (el editor pone `hard_tabs` si el archivo se sangra con ellas).
- Guardado no atómico (`fs::write`). Cerrar la ventana con pestañas sucias no pregunta. Al cambiar de espacio se cierran las pestañas limpias y las sucias quedan ocultas (vuelven al abrir el archivo).
- Al recargar por una edición de Claude el cursor se conserva pero el desplazamiento vuelve arriba (`CodeEditor` no expone su scroll).
- Solo se detectan las ediciones de Claude (Edit, Write, MultiEdit); un cambio externo al archivo no se ve.
- El visor de solo lectura quedó solo para los diffs; su rama de archivo (`doc_body`, `highlight::doc_syntax`) ya no se usa salvo si el diff desaparece mientras está abierto.

**Pendientes:**

3. **[code] Usar la tanda m3-1:** **Hecho** (`DividerLabel`, `DropZone`, `ImageThumb`, `Banner` en `c820b5d`; `Presence` y `Dialog::exit` en la tanda 4).
4. **[code] Pulido del hilo y las herramientas:** **Hecho** (tanda 4). Las herramientas `running` que fallan y las del historial sin resultado ya se resolvían en `Chat`.
5. **[code] Detalles del composer:**
   - Máximo 10 imágenes; «Mira la imagen adjunta.» por defecto; rutas relativas en el sufijo (verificado en la tanda 23: ya estaba hecho).
   - Pegar archivos que no son imagen.
   - Que `insert()` agregue al final (subagentes, comandos y Claude Design).
   - Línea de estado con contexto % y $.
   - «Cuenta y uso…» y «Reanudar…» en el menú; el popover hacia donde haya más espacio.
6. **Hecho** (tanda 6-8, ver arriba). **[m3] `Diff` + `CodeOutput`**: números de línea viejo/nuevo, tramos plegados, «Mostrar todo» y encabezado con +/−. Partir de `space::viewer::parse_diff`.
   **[code]** Usarlo en Edit, MultiEdit, Write y en las tarjetas de permiso; estadísticas con el crate `similar`; NotebookEdit.
7. **Hecho** (tanda 6-8). **[m3] `Markdown` + `CodeBlock`**: listas numeradas y anidadas, citas, tablas, enlaces y rangos clicables, cursiva y tachado; bloque con el nombre del lenguaje y «Copiado».
   **[code]** Reemplazar `view.rs:405`; las rutas en `código` abren el archivo.
8. **Hecho** (tanda 6-8: `CodeBlock` y visor). **[m3] `SyntaxHighlighter`** (syntect o tree-sitter, con los colores `--sx-*` por estilo). Usarlo primero en `CodeBlock` y en el visor de solo lectura.
9. **[code] @-menciones y búsqueda de archivos:** **Hecho** (índice con `.gitignore` en la tanda 3; lista, teclas y «Mencionar archivo… @» en la tanda 4). El árbol de Archivos ya oculta node_modules, target y dist (tanda 6-8).
10. **[code] Terminal integrada**: panel bajo el chat con `space::console` + `TabStrip` (nueva, cerrar, renombrar, «terminado», título); Ctrl+J y Ctrl+\`; botón con contador en la barra superior; «Abrir Claude en la terminal» y Remote Control dentro de ella.
    **[m3]** `ResizeHandle`/`Splitter` para el alto.
11. **Hecho** (tanda 11, ver abajo y sección 10). **[m3] `CodeEditor`** (sobre `SyntaxHighlighter`): edición, deshacer, Tab, Mod-S, plegado, selección.
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
14. **[code] Panel de cambios:** **Hecho**, con `DiffBar` (tanda 6-8). grupos plegables, «Al día» y «No es un repositorio git», `N` para lo no seguido, spinner al actualizar, árbol con `TreeRow`, colores por tipo y archivo activo.
    **[m3]** `DiffBar`.
15. **[code] Apariencia:**
    - «Sistema» sigue al sistema operativo, también en vivo.
    - Acento por espacio en `code-claude.json`, con `ColorSwatches` y `HsvPicker` y `Scheme::from_seed` en M3.
    - Acento HCT en Formal y Glass.
    - `SelectCard` en las tarjetas de estilo; Apariencia abre su pestaña.
    - `MotionSettings.reduced` según el sistema.
16. **[code] Perfil editable:** **Hecho** (tanda 4): nombre y foto en `code-profile.json`, tarjeta, `Avatar` con imagen y `ImageCropper`.
17. **[code] Cuenta y uso, mapa de agentes**: formato `rate_limits.limits`, evento `rate_limit`, secciones «Trabajando ahora» y «Terminados», contadores, `TASK_KINDS`, reloj de 1 s, Detener en la fila.
18. **[code] Notificaciones del sistema** (terminó, falló, pide permiso) cuando la ventana no está al frente: **Hecho** (tanda 4) con el globo del ícono de la pill. Falta probarlo y, si se quiere, mandarlas también a la bandeja del notch (`tray::Inbox`, que ya avisa los turnos que terminan por las sesiones de `~/.claude`).
19. **[code] Atajos que faltan:** Ctrl+L (sin efecto en la terminal y el editor), Ctrl+O, Ctrl+E, Ctrl+G; teclado en las filas de proyecto.
20. **Hecho (tanda 20, ver arriba).** **[m3] Motion M3 que falta:** `theme_reveal` + `animate_scheme`, altura animada en `ExpandableCard`, ítems de menú escalonados uno por uno, `Icon::pop`, `Avatar::hover_spin`, `slide_in`, entrada fiel de `Dialog` y `Toast`.
    **[code]** Usarlos, junto con las entradas escalonadas del hero, la barra lateral, el historial y la configuración.
21. **Hecho, funcionalidad (tanda 21, ver arriba).** **[code] Formal y Glass, primero la funcionalidad:** tarjeta de permiso completa, menú del agente completo, Configuración de la referencia, composer que crece, perilla de esfuerzo en Glass, «Razonando…» en vivo, chats sueltos, «Nuevo espacio», perfil con recorte, menú contextual del espacio y árbol de archivos con colores. Falta: ver todo en la app (los menús contextuales de las conversaciones de Formal y Glass se hicieron en la tanda 23).
22. **[code] Motion propio de Formal y Glass**, al final: el de Glass en un módulo aparte de Atic, no en gpui-m3 (luz ambiental, reflejo que sigue al cursor, gota de selección, gelatina, materializar, borde especular, `ScrollFade`, segmentado deslizante).
23. **Hecho (tanda 23, ver arriba).** **[code] Baja prioridad y pendientes chicos:** importar `.code-workspace`, `rate_limit` en vivo, reversión si falla `syncChatSettings`, marcas de modelo al cargar el historial, Chrome que reinicia el chat vacío, reinicio después de actualizar Claude, tarjeta de Artifact, avisos de carpetas que faltan, comando de pestañas, popover de Apariencia y menú contextual de conversaciones en Formal y Glass.

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

Decisiones de la tanda 20 (2026-10-10), también por confirmar:

- **Revelado circular solo hacia Expressive** y solo si cambia el estilo o el claro/oscuro; el resto (acento, «Sistema» en vivo) se mezcla 0,45 s. Con movimiento reducido, todo es inmediato.
- **Hero escalonado 0 / 0,06 / 0,12 / 0,18 s** (la referencia solo retrasa dos hijos).
- **Entradas con desplazamiento y opacidad** (`Enter`), sin escala, porque GPUI no escala.
- **El ícono de «terminado» salta solo en las últimas 3 partes**, para que un historial abierto no salte entero.

Decisiones de la tanda 11 (2026-10-10), también por confirmar:

- **Diffs y editor son vistas distintas.** Un cambio del panel de Cambios abre su diff (`DiffView`); «Abrir» (en lugar de «Ver archivo») lo pasa al editor, donde los tres estilos usan el mismo `CodeEditor`. Un archivo sin diff abre directo en el editor.
- **Cerrar sin guardar:** el diálogo ofrece Guardar, Descartar y Cancelar (la referencia solo Descartar y Cancelar). Cerrar la activa deja la vecina que ocupa su lugar o, si era la última, la anterior.
- **La X del encabezado con el editor abierto vuelve a la lista de Archivos** sin cerrar pestañas (la flecha de la referencia); las pestañas se cierran con la X de cada una.
- **Contexto con solo el cursor:** el chip muestra `archivo:12` y el mensaje lleva «(Contexto: @ruta, línea 12)». la referencia solo agregaba líneas con una selección. El chip aparece solo con el editor a la vista (no con el panel cerrado ni con un diff) y quitarlo vale hasta cambiar de pestaña. Con el texto vacío (solo imágenes o archivos) no se agrega.
- **Recargar:** si el editor está limpio se carga el disco conservando cursor o selección; si está sucio no se pisa y sale un aviso con «Recargar» y «Mantener lo mío». Si guarda con el aviso puesto, escribe encima del disco.
- **Archivos que no son UTF-8** no se editan (la pestaña explica por qué): guardarlos los dañaría. El límite es el del visor, 4 MB.
- **Atajos dentro del editor:** los de Atic Code con Ctrl+letra y Ctrl+Enter no actúan (`EDITOR_KEYS`). Esc no se anula: lo atiende el editor.
- **Al cambiar de espacio** se cierran las pestañas sin cambios y las sucias quedan guardadas, ocultas.

Decisiones de la tanda 10 (2026-10-10), también por confirmar:

- **Shell:** PowerShell 7 si está, si no Windows PowerShell (`space::powershell`, el mismo del Mando). Abre en la carpeta de la conversación a la vista.
- **Teclas dentro de la terminal:** Ctrl+K, P, N, B, U, E, G, O, L, J, V, Ctrl+Enter, Esc y Tab van al shell (`NoAction` en `CodeTerminal`). Ctrl+\` y Ctrl+, siguen siendo de Atic Code.
- **Acciones del menú del agente** que antes abrían `wt.exe` (Claude, Remote Control, plugins, cuenta) ahora abren una pestaña con nombre, como en la referencia.

Decisiones de las tandas 6-8 y 14 (2026-10-10), también por confirmar:

- **Formal y Glass también usan los componentes de código de gpui-m3** (`Markdown`, `DiffView`, `CodeOutput`, `DiffBar`): `style::apply_m3` les fija un esquema hecho con sus tokens (`style::scheme_of`) en vez de dejar el último de Expressive. Efecto: el tema global de gpui-m3 sigue ahora a Formal/Glass cuando se eligen.
- **Estadísticas +/−** con `diff_cached(..).stats()` (el diff que ya usa `DiffView`) y no con `similar` directo.
- **Rutas del chat:** `código` que parece ruta se abre contra las carpetas del espacio de la conversación; si no existe, un toast.
- **Enlaces:** solo `http`, `https` y `mailto`.
- **Árbol:** solo Atic Code oculta `node_modules`, `target`, `dist` y `.DS_Store`.

Decisiones de la tanda 4 (2026-10-10), también por confirmar:

- **Salida animada de los diálogos:** `Dialog::exit(progress)` en gpui-m3 (commit aparte, sin push). Un `Presence` no atenúa lo que se dibuja con `deferred`, que es todo `Dialog`.
- **@-menciones:** solo con un proyecto abierto (no en chats sueltos). Con varias carpetas la mención lleva `carpeta/ruta` como en la referencia, aunque Claude Code resuelve las rutas desde la primera. Usa los atajos del contexto `suggesting` del `TextArea` propio de Atic, no los de gpui-m3 (`M3TextArea`).
- **Recorte de resultados:** 40 000 caracteres con aviso (antes 6000).
- **Hijos de un subagente:** se cuentan una sola vez por id (llegan por el stream y de nuevo completos) y `N herramientas` es el total real, no el tope de 20 de la referencia.
- **Perfil:** `code-profile.json` + `code-profile-<ms>.png` en `<datos de Atic>\pill`; cada foto tiene otro nombre porque GPUI guarda las imágenes por ruta. Un nombre borrado vuelve al de git.
- **Avisos del sistema:** el globo del ícono de la bandeja de la pill (`tray_icon::notify`), sin dependencias. Se avisa si la ventana no está al frente o si la conversación no es la visible; los turnos detenidos no se avisan. Sin la pill no hay aviso.

Decisiones de la tanda 3 (2026-10-09), también por confirmar:

- **Chats sueltos** en `<datos de Atic>\pill\code-chats` (con `paths.rs`), no en `~/.referencia`. Van con un espacio ficticio `LOOSE` (`u64::MAX`) que tiene su propia configuración de Claude y su propio acento en `code-claude.json`. La sección «Chats» está en la barra de los tres estilos (en Formal y Glass desde la tanda 21).
- **Favoritos y «ya se abrió»** se guardan en `code-claude.json`, no en `space-workspaces.json`, que comparte el Mando.
- **Máximo de 4 procesos:** se cierra el proceso de Claude, no la conversación. Queda en la lista y se retoma al escribirle. Solo se cierran las que tienen sesión y no trabajan ni esperan un permiso.
- **Pegar archivos:** en Windows el Explorador copia rutas (`CF_HDROP`). Se adjuntan por ruta, sin copiarlos aparte como `savePasted`.
- **«Nuevo espacio»** elige las carpetas con el diálogo nativo. `space/picker.rs` está atado a `SpaceView` y habría que separarlo.
- **Acento** por espacio y por estilo, sin acento global de la app. M3 usa la especificación de 2021 de gpui-m3.
- **«Sistema»** sigue la apariencia de la ventana de GPUI (claro/oscuro de Windows) en vez de `crate::theme`.
- **Movimiento reducido:** `MotionSettings` es global de la app, así que también afecta a la pill.
- **Índice de archivos:** respeta `.gitignore` también fuera de un repo (`require_git(false)`). Se rehace en la búsqueda siguiente a cada respuesta.
- **Panel de cambios:** «Todo al día» solo si alguna carpeta es un repo. Si ninguna lo es, se listan con «No es un repositorio git».
- **Ctrl+L** abre una conversación nueva, salvo con el foco en la terminal (ahí limpia la pantalla del shell).

Cosas que no aplican o que Atic ya tiene de otra forma:

- **Workspaces.** Los `.referencia-workspace` (JSONC con rutas relativas, «guardar», «guardar como», «cerrar», «recientes») no se portan. Atic usa los espacios de `src/space/workspaces.rs`: una lista en `space-workspaces.json` con nombre automático y varias carpetas, que se guardan solos. Las carpetas extra van como `additionalDirectories` y la configuración de Claude va por espacio en `code-claude.json`. La importación de VS Code se portó en la tanda 23 (`src/code/vscode.rs`): lee las carpetas y deja que se revisen en «Nuevo espacio».
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
3. El recorte de los resultados de herramientas (`chat.rs`, `MAX_RESULT`): la referencia no recorta. Se subió de 6000 a 40 000 caracteres y ahora avisa «(se recortó: N caracteres más)». ¿Se quita del todo?
4. Remote Control: la referencia lo activa en todas las conversaciones vivas y Atic solo en las del espacio activo.
5. Glass tiene `raised` en 0.92 (la referencia usa 0.82). ¿Se hizo más denso a propósito porque no hay desenfoque?
6. La especificación de color: gpui-m3 usa la de 2021 y la referencia la de 2025. Con semillas propias, los colores no van a coincidir del todo.

Decisiones de la tanda 23 (2026-10-10), por confirmar:

- **Importar de VS Code no crea el espacio de golpe:** deja las carpetas y el nombre del archivo en el diálogo «Nuevo espacio» para revisarlas y pulsar «Crear». Se descartan las carpetas remotas (`uri`) y los duplicados; las que no existen se avisan pero se conservan. No se llevan `settings` ni el `name` de cada carpeta.
- **`rate_limit` en vivo:** `utilization` se toma como fracción (0 a 1). Un aviso (`allowed_warning` o `rejected`) deja un chip bajo la caja que abre «Cuenta y uso»; vuelve a «allowed» y desaparece.
- **Reversión de `syncChatSettings` por campo:** si falla el modelo no se revierte el esfuerzo que sí se aplicó (la referencia revierte los dos).
- **«Reiniciar el chat vacío»** cierra el proceso (Atic no precalienta) y la conversación se reabre al escribir, con la configuración y la versión de Claude de ese momento.
- **Aviso de carpetas que faltan** cada vez que se elige un espacio, no solo al arrancar.

Decisiones de la tanda 21 (2026-10-10), por confirmar:

- **La perilla de Glass sigue a la referencia**: riel con relleno, perilla blanca de 20 px y una parada por nivel (no una perilla circular de ángulo). Sin nivel elegido («Predeterminado») no se dibuja la perilla; un clic en la parada actual vuelve al predeterminado, como los puntos de Formal.
- **Formal y Glass reutilizan los diálogos y menús de gpui-m3** («Nuevo espacio», menú del espacio, tarjeta de perfil, recorte) porque `apply_m3` ya les fija su esquema, en vez de rehacerlos con controles propios. Si se quiere una cara más «Formal», es trabajo de la tanda 22.
- **El menú de acciones se arma una vez** como filas neutras (`Line`/`Entry`) y cada estilo las dibuja, para no duplicar la lógica.
