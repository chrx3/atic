# Celular ↔ PC: lo que falta en la app de escritorio

**Estado:** el celular y `crates/sync` están listos (rama `feat/sync-hub`, worktree
`../atic-sync-hub`). Probado en un S24 contra `fake_desktop`. Falta que la app de
escritorio (`apps/desktop/src-tauri/src/phone_sync.rs` + UI) prenda y atienda lo nuevo.

## Cómo funciona ahora

- **Funciones negociadas.** El celular manda `features` en `Hello`/`Pair`; el PC
  contesta las suyas en `Welcome`. Un mensaje nuevo solo se manda a quien anunció su
  función: un PC viejo no lo conoce, falla al leerlo y corta la sesión. Constantes en
  `atic_sync::feature` (`FILES`, `NOTICES`, `CALLS`, `PROMPT`, `WATCH`, `CONTROL`;
  `feature::all()`).
- **El PC prende funciones** con `Desktop::enable(features, Some(inbox_dir))` antes de
  que conecten los celulares. Hoy la app no lo llama: para ella nada cambió (solo
  ignora `DesktopEvent::Phone` y los `PcCommand` nuevos; ver las dos líneas marcadas
  en `phone_sync.rs` y `prototypes/pill-gpui/src/phone.rs`).
- **El celular es el centro**: puede estar con varios PCs a la vez y les reparte el
  portapapeles con `ClipSync`/`ClipDelete` (protocolo de siempre).
- `DesktopEvent::Connected` trae ahora `features` del celular; `Desktop::phone_features(id)`.

## Qué implementar

1. **Prender** en `ensure_running`: `desktop.enable(feature::all(), Some(<dir de datos>/phone-inbox))`
   (o solo lo que ya esté hecho; cada función se puede prender por separado).
2. **`DesktopEvent::Phone { device_id, request }`** — `PhoneRequest`:
   - `Notice(PhoneNotice)` / `NoticeRemoved { key }`: mostrar la notificación del
     celular (pill/peek). `PhoneNotice { key, app, app_name, title, text, posted_at_ms,
     can_reply, icon: Option<PNG 64px> }`. Contestar con `desktop.notice_reply(device,
     key, text)` (solo si `can_reply`); descartar con `desktop.notice_dismiss(device, key)`.
     En el celular viene apagado de entrada (Ajustes → PC vinculado).
   - `Call(Option<CallState>)`: la llamada en curso (`name, incoming, since_ms,
     can_answer, can_hang_up`); `None` = terminó. Botones → `desktop.call_action(device,
     CallAction::Answer|HangUp)`. También apagado de entrada en el celular.
   - `Prompt { agent_id, text }`: escribir `text` en la consola del agente, como si el
     usuario lo tipeara (mismo camino que las respuestas a preguntas de consola,
     `console_prompts.rs`/`bridge.rs`). `agent_id` es el de `AgentCard.id` que publica
     `snapshot()`.
   - `Watch { agent_id: Option<String> }`: el celular mira en vivo a ese agente (`None` =
     deja de mirar; uno por celular). Mandar con `desktop.agent_output(device, agent_id,
     lines, reset)`: primero las últimas ~200 líneas con `reset: true`, después solo lo
     nuevo. Sin códigos ANSI de más (el celular los limpia igual). Cortar al recibir
     `None` o `Disconnected`.
   - `File { header, path }`: un archivo ya guardado en `inbox`. `header.kind`:
     - `FileKind::File`: moverlo a Descargas/Atic del PC y avisar (abrir / mostrar en carpeta).
     - `FileKind::Meeting`: es una reunión del celular; `header.meta` trae JSON
       `{"id","title","startedAt" (epoch ms),"durationSecs","language"}`. Llegan dos
       archivos con el mismo `id`: el audio (`.wav`) y la transcripción (`.md`).
       Importarla a reuniones del PC (o al menos guardarlos juntos).
3. **`PcCommand` nuevos** (solo llegan con `CONTROL` prendido): `VolumeUp`,
   `VolumeDown`, `Mute`, `LockScreen`, `SlideNext`, `SlidePrev`. En `run_command`:
   volumen/silencio del sistema, bloquear pantalla (Windows `LockWorkStation`, macOS
   `CGSession -suspend` o equivalente), diapositivas = teclas flecha derecha/izquierda
   (o AvPág/RePág) a la ventana con foco.
4. **Mandar archivos al celular** (opcional, UI): `desktop.send_file(device, path, mime,
   FileKind::File, None).await` — p. ej. desde el historial del portapapeles o arrastrando
   a la pill. En el celular llegan a Descargas/Atic con aviso para abrir.
5. **mDNS** ya está en `crates/sync` (`iroh-mdns-address-lookup`, servicio `atic`). En
   macOS 15+ hace falta en `Info.plist`: `NSLocalNetworkUsageDescription` y
   `NSBonjourServices` con `_atic._udp` (revisar el nombre exacto que publica
   `swarm-discovery` para el servicio `atic`), o el sistema lo bloquea en silencio.
6. **Pendiente de antes**: que fijar/desfijar un ítem del historial viaje entre PCs (hoy
   `merge_from_phone` ignora `pinned` de ítems que ya existen).

## Cómo probar

- `cargo test -p atic-sync` (24 pruebas; `con_las_funciones_nuevas_viaja_todo` es la guía
  de cada mensaje).
- `ATIC_FAKE_NAME="PC X" cargo run --example fake_desktop` es un PC de referencia que
  atiende todo (imprime lo que llega y responde a `Watch` con salida simulada).
- Celular: app Android de la rama `feat/sync-hub` de `atic-android`
  (`ATIC_REPO=../atic-sync-hub scripts/build-rust.sh` y luego instalar).

## Al integrar

`feat/sync-hub` sale de `c37dfae`. Toca `crates/sync/*`, `Features/*`, una línea en
`apps/desktop/src-tauri/src/phone_sync.rs` (ignorar `DesktopEvent::Phone` y
`PcCommand` nuevos) y otra en `prototypes/pill-gpui/src/phone.rs`. La rama de Android
del mismo nombre depende de esta: se integran juntas.
