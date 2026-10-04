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
- `crates/sync/examples/fake_desktop.rs` — PC de prueba para el celular.
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

## Relacionado

- [sync-dispositivos.md](sync-dispositivos.md)
- [`docs/MOBILE.md`](../docs/MOBILE.md)
- [clipboard-historial.md](clipboard-historial.md)
