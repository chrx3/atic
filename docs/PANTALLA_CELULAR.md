# Pantalla compartida entre el celular y el PC: qué falta

Estado al 2026-10-29. El trabajo de fluidez (video v2, captura y codificación
por hardware en Mac, decodificación por hardware en Mac, bitrate adaptable,
BBR) está en `dev`. Lo que sigue es lo que no se ha visto funcionar y lo que
se puede mejorar.

Piezas: `crates/sync/src/video.rs` (transporte y bitrate),
`crates/capture/src/stream/` (captura y códec del sistema),
`crates/capture/src/engine/dxgi.rs` (captura en Windows),
`prototypes/pill-gpui/src/phone_screen.rs` (las dos ventanas) y
`ScreenShareService.kt` en `atic-android`.

## Por probar

### Con el celular (en Mac)

- [ ] **Celular → PC por hardware.** La ventana se ve igual que antes y la
  pill gasta poca CPU (con OpenH264 eran ~48 %). Solo se probó con un video
  de archivo y con la pantalla del Mac codificada y decodificada.
- [ ] **El cambio a OpenH264 con un stream real.** Si VideoToolbox no puede,
  el log dice `VideoToolbox no pudo; sigue OpenH264` y la imagen sigue. Está
  cubierto por la prueba `si_videotoolbox_falla_openh264_sigue` (atic-capture)
  con un video de OpenH264, no con el de MediaCodec.
- [ ] **PC → celular con BBR.** Que siga a 27–30 cuadros/s sin descartes en
  la red de la casa, como antes.
- [ ] **Red mala de verdad.** El celular con datos móviles, o forzando relay.
  La imagen tiene que seguir moviéndose aunque baje la calidad, y al cambiar de
  red recuperarse en segundos. Mirar en el log de la pill (`pantalla del`)
  la línea de cada 5 s: cuadros/s, kb/s, descartados, objetivo, cola, RTT y
  camino (directo o relay).
- [ ] **Keyframe con la cola llena (celular → PC).** Antes el PC podía quedar
  congelado hasta 10 s; ahora el celular pide otro. Llega con el próximo APK
  (no cambia Kotlin, solo la librería de Rust).
- [ ] **Mensajes y archivos con BBR.** El endpoint del PC usa BBR para todo
  (el PC no sabe el ALPN antes de aceptar). Las pruebas de atic-sync pasan;
  falta mandar un archivo grande en la app.

### En Windows (nada visto todavía)

- [ ] **Captura con DXGI** (`engine/dxgi.rs`): solo pasó el chequeo de tipos.
  Al compartir el PC, el log no debería mostrar que cae a `BitBlt`. Probar
  también con dos monitores, pantalla girada y tras bloquear la sesión (UAC,
  Ctrl+Alt+Supr): la duplicación se pierde y se tiene que rehacer.
- [ ] **PC → celular y celular → PC** con BBR y el bitrate nuevo, igual que
  en Mac.

### Sin el celular

- Red mala simulada: `cargo test -p atic-sync red_mala -- --ignored --nocapture`
  (~30 s). Un proxy UDP entre dos endpoints limita el ancho de banda, agrega
  demora y pierde paquetes; imprime cada segundo el bitrate, los cuadros y la
  demora. Sirve para comparar cualquier cambio al bitrate o al transporte.

## Por hacer y optimizar

1. **Windows: codificar por hardware** con Media Foundation (NVENC,
   QuickSync, AMF) al compartir el PC. Hoy es OpenH264 en la CPU.
2. **Windows: decodificar por hardware** la pantalla del celular (Media
   Foundation / D3D11) y pintarla sin pasar por una imagen nueva cada cuadro.
   Hoy es OpenH264 más una imagen por cuadro, como estaba el Mac antes.
3. **El puntero del PC aparte**, como mensaje liviano a ~60 Hz que el
   celular dibuja encima: se ve fluido aunque el video baje de cuadros.
4. **La caída brusca de ancho de banda.** En la simulación (de 8 a 1 Mb/s)
   hay 2–3 s con hasta ~800 ms de demora y varios keyframes seguidos antes de
   asentarse. Ideas: bajar el bitrate antes, mirando el crecimiento de la
   demora (no solo su valor); no pedir otro keyframe mientras uno sigue en
   camino; keyframes más chicos a bitrate bajo.
5. **Subir más rápido tras una caída.** Sube ×1,15 por segundo: de 500 kb/s a
   4 Mb/s tarda ~15 s. Se podría subir más rápido mientras la demora no crece.
6. **Log del lado del celular.** El celular no tiene un subscriber de
   `tracing`: las líneas de cada 5 s del que manda (objetivo, cola, RTT) no se
   ven en celular → PC.
7. **Disco.** El target de la pill de dev pasa de 50 GB (`debug/deps`).

## Publicación

- El celular y el PC tienen que salir juntos para usar v2 (cada uno sigue
  funcionando con la versión vieja del otro, por v1).
