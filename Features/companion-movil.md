# Companion móvil

**Estado:** `en curso`

## Resumen

App Android (Kotlin + Compose, Android 16+) en un repo aparte, `atic-android`.
Muestra en una isla dinámica lo que hacen tus agentes en el PC y te deja
contestar sus permisos. También comparte el portapapeles de texto y muestra lo
que suena y si el PC está grabando. Se distribuye por Play Store y como APK.

## Cómo se usa

- En el PC: Ajustes → Celular → «Mostrar QR».
- En el celular: tarjeta «PC» → «Escanear QR». El PC pregunta «¿Vincular …?» y
  hay que aceptar ahí: tener el QR no basta.
- La isla rodea la cámara y muestra al protagonista: un permiso pendiente, la
  grabación del PC, un agente trabajando o la música, en ese orden.
- Tocarla la abre. «Permitir» pide huella o PIN; «Denegar» no.
- Portapapeles (Ajustes → Celular → Compartir, apagado por defecto): lo que
  copias en el PC llega al celular. Del celular al PC se manda desde el tile
  de ajustes rápidos «Enviar al PC», desde «Compartir → Enviar al PC» o desde la
  isla abierta.
- Varios PCs: un celular se puede parear con todos los que quieras (Ajustes →
  PC vinculado → «Agregar otro PC») y queda conectado a todos a la vez. Es el
  centro: lo nuevo del historial de un PC pasa a los otros conectados (al
  historial, sin pisar su portapapeles), y a los que estaban apagados les llega
  al conectarse. Los borrados viajan igual. Lo que se manda sin ningún PC
  conectado queda en el celular y sale al conectarse. Los permisos de cada
  agente se contestan solo a su PC.
- Desde la isla: detener la grabación del PC y controlar la música. No se puede
  empezar a grabar desde el celular, a propósito.

## Seguridad

- Canal: `atic-sync` sobre iroh (QUIC + TLS 1.3, claves ed25519). Sin cuentas
  ni nube de Atic. El relay de n0 solo ve tráfico cifrado.
- Pareo: token de 128 bits, 5 minutos, un solo uso, y confirmación en el PC.
- Desvincular corta en el acto la sesión abierta del celular.
- Clave del PC en el llavero del sistema; la del celular, cifrada con el
  Android Keystore.
- Aprobar desde la isla pide huella o PIN. Los botones del Live Update exigen
  desbloquear, y la pantalla de bloqueo muestra una versión sin proyecto ni
  comando.
- El portapapeles reusa el filtro de sensibles del historial: lo que no entra
  al historial no sale.

## Código

- `crates/sync` — protocolo, pareo, sesión, portapapeles y estado del PC
  (escritorio y celular). `crates/sync/ffi` — bindings UniFFI para Kotlin.
- `crates/sync/examples/fake_desktop.rs` — PC de prueba para el celular. Con
  `ATIC_FAKE_NAME="Otro PC"` se levanta un segundo PC con otra identidad.
- `apps/desktop/src-tauri/src/phone_sync.rs` — une el canal con el hub de
  agentes, la presencia, el portapapeles, los medios y la grabación.
- `apps/desktop/src/lib/features/settings/PhoneSection.svelte` — Ajustes → Celular.
- Repo `atic-android` — isla, Live Update, pareo y portapapeles.

## Pendiente / siguiente

- [ ] Probar fuera de la LAN (datos móviles) y medir si queda directo o por relay.
- [ ] Relay propio opcional en vez de los públicos de n0.
- [ ] Imágenes en el portapapeles (con tope de tamaño).
- [ ] Descubrimiento mDNS en la LAN (`iroh-mdns-address-lookup`).
- [ ] Revisión de política de Play para el servicio de accesibilidad de la isla.
- [ ] Archivos en los dos sentidos: una corriente QUIC aparte para lo grande
      (hoy todo va en una corriente de mensajes JSON de hasta 8 MB).
- [ ] Reuniones grabadas en el celular → al PC (y al revés).
- [ ] Notificaciones del celular en la pill, y contestarlas desde el PC.
- [ ] SMS y llamadas en el PC (permisos sensibles: quizás solo fuera de Play).
- [ ] Hablarle a los agentes desde el celular con texto libre o voz.
- [ ] El celular como control del PC: música, volumen, bloquear, diapositivas.
- [ ] Ver en vivo la terminal o lo que hace un agente.
- [ ] Registro de cambios con secuencia y que fijar/desfijar viaje entre PCs.
- Para después (pesado: casi obliga a conexión directa o relay propio):
  compartir la pantalla del PC y el celular como cámara web.

## Relacionado

- [sync-dispositivos.md](sync-dispositivos.md)
- [`docs/MOBILE.md`](../docs/MOBILE.md)
- [clipboard-historial.md](clipboard-historial.md)
