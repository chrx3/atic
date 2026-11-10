# Subagentes delegados en Atic Code

Un agente de Atic Code (Claude, o Codex, Cursor, OpenCode o Grok por ACP) le puede
encargar trabajo a otro agente con la herramienta MCP `atic_delegate`. Atic abre una
conversación hija con el agente pedido y, cuando el hijo termina su turno, le manda el
resultado al padre. La experiencia copia la de los subagentes de t3code.

## Cómo funciona

- **El hub** (`apps/atic/src/code/hub.rs`): al abrir Atic Code se levanta un servidor
  HTTP en `127.0.0.1` (puerto libre) y se escribe `hub.json` en los datos de Atic
  (`%APPDATA%\ciat\atic\data\hub.json` en Windows) con el puerto, un token y el pid.
  Al cerrar Atic Code se borra si es de ese proceso. El archivo es solo del usuario: `0600`
  en Unix y, en Windows, una lista de permisos propia con él como única entrada
  (`restrict_to_user`). Lo mismo el `code-agent.json` del daemon (`writePrivate` en
  `agent.mjs`, con `icacls`).
- **`atic-mcp --delegate`** (`crates/atic-mcp`): cada conversación de Atic Code lo
  registra como servidor MCP `atic` (en Claude, `mcp__atic__…`, y queda permitido sin
  preguntar). En cada llamada lee `hub.json` y le pasa el pedido al hub. Va con cuatro
  variables de entorno:
  - `ATIC_HUB_TOKEN`: el token de esa conversación ante el hub (ver «Quién pide»);
  - `ATIC_SESSION`: la clave de la conversación (`c2`…);
  - `ATIC_DELEGATE_DEPTH`: 0 en una conversación normal, 1 en un hijo, 2 en un nieto;
  - `ATIC_ROOT`: la conversación con que empezó la cadena.
- **El hijo** (`code/delegate.rs`, `create_child`): se abre en la misma carpeta y espacio
  que el padre, con el agente y el modelo pedidos, y con `Chat.delegation` (padre, rol,
  agente, `started`, `wake`…). El encargo le llega con el rol delante.
- **El despertar**: por defecto `atic_delegate` contesta `running` al tiro. Cuando el hijo
  termina, su última respuesta va al padre como un mensaje `<atic-subagent>`
  (`wake_message`) con `session`, `title`, `agent`, `role`, `status`, `duration_ms` y
  `result`, más una línea legible para el modelo. Entra en el turno en curso del padre si
  se puede (Claude con la sesión abierta), queda en su cola si no, o abre un turno nuevo.
  Con `mode: "wait"` la llamada espera el resultado en vez de despertar.
- **Un hijo pegado**: cada turno de un hijo se vigila (`watch_stall`). Si pasa 15 minutos
  (`STALL_AFTER`) sin mandar ningún evento y no está esperando un permiso del usuario, quien
  lo espera recibe un aviso con estado `stalled` y un `hint`: como mensaje `<atic-subagent>`
  al padre (la tarjeta dice «Codex avisó: …») o como respuesta del `wait`. Es un aviso por
  turno y no detiene al hijo: si termina después, su resultado llega como siempre.
- **Límites**: profundidad 2 (hijo y nieto; un nieto no puede delegar) y 4 hijos trabajando
  a la vez por padre. Las instrucciones para los agentes están en `DELEGATION_GUIDE`.

## Quién pide y qué puede

- **Dos tokens.** El de `hub.json` lo puede leer cualquier programa del usuario: quien lo
  trae pide como alguien de fuera de Atic Code (`Caller::External`). Cada conversación
  recibe además el suyo (`hub::session_token`, al azar; no se escribe en ningún archivo), y
  con él el hub sabe qué conversación pide, a qué profundidad está y cuál es su raíz. Lo que
  diga el cuerpo del pedido (`parent`, `from`, `depth`) solo se usa para quien pide desde
  fuera, y nunca lo hace pasar por una conversación.
- **Cada uno maneja lo suyo.** `atic_prompt`, `atic_wait`, `atic_cancel`, `atic_close`,
  `atic_rename` y `atic_permission` solo valen sobre las tareas que delegó quien pide (o
  sus hijos); desde fuera, solo sobre lo que se pidió desde fuera (`owns`).
- **Y ve lo suyo.** `atic_list_sessions` (`/v1/sessions`) le muestra a una conversación la
  suya y las que delegó (hijos y nietos), y a quien pide desde fuera, solo lo pedido desde
  fuera (`sees`): no las demás conversaciones del usuario.
- **La carpeta.** Un hijo trabaja en la carpeta del padre o en otra de su mismo proyecto.
  Desde fuera se puede pedir cualquier carpeta.
- **Desde fuera nunca sin preguntar.** Lo que se pide con el token de `hub.json` lleva de
  techo el modo guardado, pero a lo más `default` (`external_ceiling`): un hijo Claude
  pregunta y uno de Codex va en solo lectura, aunque el usuario tenga guardado «sin
  preguntar». Así, leer `hub.json` no le sirve a un subagente para saltarse el techo de su
  padre. Si el usuario quiere que ese hijo no pregunte, se lo dice en su conversación
  («Permitir todo»). Queda fuera lo que Atic no controla: OpenCode, Cursor y Grok preguntan
  según su propia configuración, pidan desde dentro o desde fuera.
- **El techo de permisos.** Ningún hijo deja hacer más que su padre. El modo del padre es
  el de su conversación (el guardado, el de su delegación o «Permitir todo»); un hijo
  Claude lleva a lo más ese (`capped_mode`), y uno de Codex, su equivalente
  (`capped_acp_mode`): acceso total solo con un padre que no pregunta, escribir en el
  proyecto con uno que acepta ediciones, y solo lectura (pide permiso para escribir) con
  los demás. OpenCode, Cursor y Grok no tienen un modo que Atic conozca como «sin
  preguntar»: lo que preguntan lo decide su configuración; con un padre en modo plan se
  abren en su modo de solo lectura. Si el usuario cambia el modo de permisos, a un hijo
  Claude abierto solo se le baja (`cap_children`, `child_mode`): nunca sube sobre el techo
  que heredó.
- **Los permisos del hijo los contesta el usuario**, en la conversación del hijo. Quien
  delegó puede rechazarlos con `atic_permission`, y aprobarlos solo si su propio modo ya
  no pregunta (`parent_may_allow`); si no, recibe `needs_user`. Las preguntas al usuario y
  la salida del modo plan son siempre del usuario, y «permitir siempre» también.
- **Si la pill se reinicia** y la conversación siguió viva en el daemon, conserva su
  token: la pill lo deja en el `tag` de la conversación (`sync_tag`, campo `hubToken`), que
  el daemon guarda en memoria y solo le devuelve a quien trae su token, y al retomarla lo
  vuelve a registrar (`hub::adopt_session_token`). Con un daemon de antes, que no lo
  guardó, `atic-mcp` repite el pedido con el de `hub.json` y delega como alguien de fuera
  (espera el resultado, sin despertar y sin pasar de `default`) hasta que la conversación
  se abra de nuevo.
- **Los mensajes de otra sesión** (`<cross-session-message>`) se dibujan como tarjeta solo
  si Claude Code dice que llegaron por ese canal (`origin.kind == "peer"`, que viaja con el
  mensaje en vivo y en el historial; `chat::peer_messages`). El mismo texto escrito o pegado
  por el usuario es una burbuja suya.

## Cómo se ve

- **En el padre**, la llamada a `atic_delegate` (o `atic_prompt`) no es la fila genérica
  con el JSON: es la tarjeta del hijo (`code/subagents.rs`), una línea del alto de las
  herramientas con el logo y el nombre del agente, el título y el estado en vivo
  (trabajando con su tiempo, pide permiso, terminó, falló, detenido), y un ícono que abre
  al hijo. Viene plegada; al abrirla muestra el rol, la duración y el encargo. Si pide un
  permiso o la llamada falló, eso se ve aunque esté plegada, con «Abrir para contestar».
  No entra en las tandas de herramientas.
- **El resultado** (`<atic-subagent>`) es otra tarjeta plegable de una línea (`Item::Task`
  con `agent`), no una burbuja del usuario: «Codex terminó: …», duración, agente y rol, y
  el ícono para abrir al hijo; al abrirla, la respuesta en Markdown. No cambia el título, no cuenta para el Rewind,
  la edición ni las flechas de la caja. Funciona en vivo, con el eco del sidecar y al
  recargar el historial.
- **En el hijo**, una barra arriba: «Subagente de «…» · Codex · revisión» con «Ir a la
  conversación principal». En vez de la caja de texto, el aviso «Los subagentes reciben
  mensajes de la conversación principal». Sus permisos se contestan en su hilo, como
  siempre; «Permitir todo» vale solo para esa conversación y no se guarda.
- **En la barra lateral**, los hijos van bajo su padre (sangría, línea guía y logo del
  agente). Si el padre no se ve, el hijo se muestra como cualquier conversación.
- **En el mapa de agentes** (el chip o la píldora «N agentes», en los tres estilos), los
  subagentes delegados cuentan en el número y en el encabezado, y se listan con su
  estado; un clic los abre.
- **Detener** el padre detiene a sus hijos que trabajan y ninguno lo despierta después.

## Probarlo en dev

La pill de dev (`scripts/pill-dev.ps1`) usa las copias de `atic-mcp.exe` y de
`agent.mjs` que están junto a su ejecutable: si no se actualizan, corre con las viejas.
Un `atic-mcp.exe` anterior a los tokens por conversación no manda `ATIC_HUB_TOKEN`: el
hub lo trata como alguien de fuera y los hijos no quedan bajo su padre ni lo despiertan.

1. Compila lo que cambió:
   - `cargo build -p atic-mcp --release`
   - `pnpm --dir apps/atic/sidecar run build`
2. Copia los dos a `%LOCALAPPDATA%\atic-gpui\debug\`:

   ```powershell
   Copy-Item target\release\atic-mcp.exe "$env:LOCALAPPDATA\atic-gpui\debug\" -Force
   Copy-Item apps\atic\sidecar\dist\agent.mjs "$env:LOCALAPPDATA\atic-gpui\debug\" -Force
   ```

   Si `atic-mcp.exe` está en uso (una CLI lo tiene abierto), renombra el viejo en vez de
   cerrar las CLI.
3. Abre la pill con `scripts/pill-dev.ps1` y, en Atic Code, pídele a Claude algo como
   «delega en Codex revisar los tests de payload.rs». Mira la tarjeta en el padre, el
   hijo anidado en la barra, la barra del hijo, el resultado como tarjeta y «Detener».
4. Revisa en tema claro y oscuro y con los tres estilos de ventana.

## En el celular

- **El protocolo marca las sesiones hijas.** `ChatLink` (`code/link.rs`) trae
  `subagent`, `parent` (la primera sesión de la conversación padre, que es el `id` de su
  tarjeta) y `role`, y `phone::set_agents` los pasa a `AgentCard` (`proto.rs`) como campos
  opcionales con `#[serde(default)]`: un PC o un celular viejos siguen entendiéndose.
- **Anidadas.** En Agentes del celular (`AgentsScreen.kt`, `AgentGroups.of`) la hija va
  bajo su padre, con sangría y «Subagente · revisión»; el padre sube a «En curso» si un
  hijo trabaja o espera. Si el padre no está en la lista, la hija va suelta. Con varios
  PCs, `publishAgentRoutes` traduce `parent` al id visible del mismo PC.
- **Sin caja en el hijo.** En la pantalla en vivo (`AgentLiveActivity.kt`) un subagente
  muestra «Los subagentes reciben mensajes de la conversación principal» y «Ir a la
  conversación principal». `SyncLink.prompt`/`promptWith` y `SyncFeed.canPrompt` (isla y
  voz) no le escriben, y el PC rechaza un `Prompt` del celular a un subagente
  (`phone::is_subagent`).
- **Sus permisos** se siguen contestando desde el celular, en su tarjeta, y la del padre
  dice «Te espera» y «Codex pide permiso» mientras un hijo espera.
- **El resultado en el historial.** `agents::claude_entries` y `codex_entries` separan el
  `<atic-subagent>` con `chat::split_subagent_notes` y lo mandan como una respuesta
  (`EntryKind::Assistant`) con el encabezado en negrita («Codex terminó: …») y el
  resultado en Markdown, sin cambiar el protocolo.
- **Detener** desde el celular (`AgentAction::Stop`) pasa por `interrupt_chat`, así que
  detiene también a los hijos.
- Probarlo sin PC: `cargo run -p atic-sync --example fake_desktop` publica a Codex como
  subagente de Claude.
