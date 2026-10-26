# Pendientes — 26 de octubre de 2026

Lo que quedó por hacer tras la sesión de la pill en macOS (25 y 26 de
octubre). Lo hecho está en `dev` (de `542066d` a `d0a90be`).

## Release

- [ ] **Publicar la 0.5.1.** La versión ya está en `5b9ba09`, pero no hay tag
  ni release. El build quedó a medias por falta de memoria (Mac de 8 GB).
  Antes de lanzarlo, cerrar la pill de dev y las apps pesadas, y después:
  `bash scripts/release-macos.sh --publish`.
- [ ] **No repartir el `.dmg` de `target/pill-mac`.** Es una 0.5.1 anterior,
  sin tareas ni los últimos arreglos.
- [ ] **Windows.** Nada de esto se compiló ni se probó allá. Correr
  `scripts/pill-dev.ps1` en el PC y publicar desde
  `scripts/release-windows.ps1` (suma Windows al mismo release).
- [ ] **`media/lyrics.rs`** tiene cambios de otra sesión sin commitear.
  Revisar de quién son antes de publicar desde este árbol.

## Por confirmar en la app

Hecho, pero sin haberlo visto funcionar:

- [ ] El notch que quedaba abierto y sin responder después de una captura.
- [ ] La mira y el cuentagotas en la pantalla que no tiene la pill: la lupa,
  la cruz y el oscurecido. Si falla, ver las líneas `espejo:` en
  `pill-salida.log`.
- [ ] El flip con la ventana del frente en la otra pantalla.
- [ ] Arrastrar del Portapapeles al chat de Agentes, con los dos lado a lado.
- [ ] El panel de Agentes y su chat en los tres estilos de Apariencia.
- [ ] La tira con «Más»: que no se recoja y que el espacio vacío al final no
  se vea raro (si se ve, centrar o achicar después del cambio de página).
- [ ] Las tareas: escribir, marcar, «Traer de Reuniones» y las que llegan al
  generar un resumen.

## Ideas conversadas, sin empezar

- [ ] **Zona para soltar sobre el notch:** al acercar un archivo arrastrado,
  destinos para Celular, Atic Code, Portapapeles, Textos y Pizarra. Primero
  archivos; el texto arrastrado necesita tocar `vendor/gpui`.
- [ ] **Documentos:** fijar archivos importantes, mandarlos al celular y
  convertirlos de formato.
- [ ] **Chat de Atic Code:**
  - Pegar imágenes con ⌘V.
  - Un campo de varias líneas (Enter envía, ⇧Enter hace salto de línea).
  - El estilo Liquid Glass.
- [ ] **Pomodoro:** escribir en qué te vas a enfocar en cada ronda, y guardar
  la ronda en curso al cerrar la pill.
- [ ] **Dictado:** mostrar «Escuchando…» dentro del panel cuando el notch
  está abierto (hoy no se ve).

## Deuda técnica

- [ ] Sacar el código de los vistazos viejos del Portapapeles (`peek` en
  `main.rs`) y de Agentes (`usage_peek`, salvo `PILL_OPEN=uso`). Ya no se
  abren: son páginas del carrusel.
- [ ] Algunos commits del 25 no compilan solos (se separaron por tema). Solo
  importa si se hace `git bisect`.
- [ ] Arrastrar al chat y la zona del arrastre dentro del notch no están
  probados en Windows (`drag.rs` de Windows no cambió).
