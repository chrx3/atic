# Editor de grabaciones: plan

Estado: hecho en macOS (2026-10-31), salvo lo de «Pendiente» al final.

- Barra de la mira (Captura / Grabar) con las islas de la Pizarra.
- Registro del mouse y los clics al grabar (`screen_cursor.rs`).
- «Cursor suave» en Grabar: graba sin cursor, guarda la copia limpia en
  `.atic/<nombre>.master.mp4` y al guardar dibuja el cursor suavizado
  (`screen_recording.rs`, `bake_cursor`).
- Editor en ventana propia (`recording_editor.rs`): vista previa con AVPlayer,
  miniaturas, recorte, cortes, zooms (a mano o con Autofocus), textos, cursor
  dibujado (tamaño y suavizado) y exportar «(editada)» con progreso.
- Lógica pura y con tests en `edit_project.rs`; motor de exportación en
  `crates/capture/src/stream/export_macos.rs` (Core Image + AVAssetWriter).
- `PILL_OPEN=editor:<ruta del video>` abre el editor al arrancar (para revisarlo).

- Interfaz de editor de video (2026-10-31): sigue el estilo de Apariencia
  (`recording_editor/palette.rs`; vidrio de verdad con Liquid Glass por piezas),
  barra lateral con lo elegido, Fondo (degradados, margen, esquinas, sombra),
  Zoom (Autofocus) y Cursor (tamaño, suavizado, onda al hacer clic), y línea de
  tiempo con regla, marcas, líneas guía y nombres de pistas
  (`recording_editor/timeline.rs`, hecha por un subagente Sonnet).
- Cámara del autofocus: sigue al cursor con margen y suavizado hacia los dos
  lados (sin retraso); test `la_camara_sigue_al_cursor_sin_atrasarse_ni_saltar`.

- Zoom al escribir: al grabar se cuentan las teclas (sin guardar cuáles) y se
  lee con Accesibilidad dónde está el cursor de texto; el Autofocus hace un zoom
  por racha de escritura y la cámara sigue al texto mientras se escribe.
- Desenfoque de movimiento al exportar (`CIMotionBlur` y `CIZoomBlur`), suave,
  según la velocidad de la cámara; se apaga en la sección Zoom.

Pulido (2026-11-01, ver `EDITOR_PULIDO.md`): exportar con proporción (9:16, 1:1, 4:5),
GIF, HEVC, resolución y calidad; deshacer y rehacer; línea de tiempo con zoom, imán y
onda de audio; velocidades y «acelerar silencios»; difuminar o pixelar zonas; textos con
estilo; subtítulos automáticos; fondo propio o con imagen; estilos de cámara; cursor con
forma (mano, texto, cruz) que se esconde quieto.

Asistente de IA (2026-11-01): pestaña «Asistente» con un chat con Claude, Codex, OpenCode, Cursor o Grok que ve la
grabación y la edita con las herramientas del editor; ver `EDITOR_AGENTE.md`. El chip de al lado del agente elige
el modelo (y el esfuerzo, si el agente lo ofrece): viaja en `want.config` al abrir la sesión o se aplica con
`setConfig` en caliente. No se recuerda entre ventanas a propósito (un modelo retirado dejaría la sesión sin abrir).

Peso y seguridad (2026-11-01): en Apple Silicon la grabación va en **HEVC** (`record_macos.rs`, ~40–55 % menos
bits; en Intel o si el escritor no lo acepta, H.264) y el cursor horneado se rehace en HEVC con tope de 1080. El
export por defecto usa alto máximo 1080 (nunca agranda) en vez del alto del original. Exportar **no puede pisar**
la grabación ni el máster (se comprueba al elegir el destino y otra vez al publicar), y un archivo existente se
reemplaza solo tras «Reemplazar». Un error del lector del video es un error, no un fin de archivo.

Windows (2026-11-01): el editor completo corre también en Windows con Media Foundation y Direct2D (`crates/capture/src/stream/*_win.rs`): `player_win` (motor de medios con audio), `compose_win` (el mismo contrato que el `Composer` de macOS, en la GPU), `export_win` (Source Reader, compositor y Sink Writer H.264/HEVC + AAC), `export_audio_win`, `export_gif_win`, `info_win`/`analysis_win` (miniaturas, onda, WAV de 16 kHz) y `sprite` (cursores). Los tipos y las cuentas comunes están en `types.rs`. Faltan: «Cursor suave» (hornear el cursor al grabar), pixelar con bloques de verdad (hoy es un difuminado fuerte) y el desenfoque de zoom; los subtítulos usan solo el modelo local. Ver `GRABACION_WINDOWS.md` para el costo de grabar.

Pendiente: teclas en pantalla (hoy se guarda solo
cuántas teclas, a propósito) y cámara web.

Decidido (2026-10-31):

- El editor va en **ventana propia**.
- **Cursor:** hoy queda grabado en el video y además se registra su posición.
  Cuando el editor dibuje el cursor suavizado, la grabación pasará a hacerse
  sin cursor (`showsCursor = false`) y, al guardar, se generará el MP4 con el
  cursor dibujado encima (por hardware, segundos). Así una grabación que no se
  edita igual tiene cursor, y una que se edita lo tiene limpio para suavizarlo.
- **Exportar siempre aparte** («(editada)»). Más formatos (GIF, vertical)
  en una segunda etapa.

## Qué queremos

Al terminar una grabación, en el estante aparece **Editar** (en vez de
Pizarra, que es para fotos). Abre un editor liviano, con el lenguaje visual de
la Pizarra, donde puedes:

- **Recortar** el inicio y el final, y **cortar** tramos del medio.
- **Zoom por tramos** en la línea de tiempo: acercar a una zona durante un rato
  y volver.
- **Autofocus**: la cámara sigue al mouse y se acerca sola a los clics, como
  Screen Studio.
- **Textos por tiempo**: un título o una nota que aparece entre dos momentos.
- **Exportar** a MP4 (y después GIF) sin perder la fluidez de la grabación.

## Principio: no destructivo y sin re-grabar

La grabación queda tal cual. El editor guarda un **proyecto** (JSON) al lado
del video, y solo al exportar se genera un archivo nuevo. Abrir, cambiar y
deshacer es instantáneo, y nunca se pierde el original.

```
~/Movies/Atic/Grabación 2026-10-31 17-38-46.mp4         ← el original
~/Movies/Atic/.atic/Grabación 2026-10-31 17-38-46.json  ← mouse, clics y proyecto
~/Movies/Atic/Grabación 2026-10-31 17-38-46 (editada).mp4
```

## Las piezas

### 1. Grabar también el mouse (la base del autofocus)

Sin esto, el autofocus tendría que adivinar dónde está el cursor mirando los
píxeles. Hay que hacerlo **antes** que el editor: así las grabaciones de hoy ya
se pueden editar después.

- Mientras graba, un hilo muestrea la posición del cursor a 60 Hz
  (`CGEventGetLocation` de un evento vacío, o `NSEvent.mouseLocation`) y la
  marca con el mismo `RecordClock` del video. Así las pausas quedan fuera solas
  y todo calza al milisegundo.
- Los **clics** salen de un `CGEventTap` de solo escucha. Pide Monitoreo de
  entrada, que la pill ya maneja en Ajustes → Permisos. Sin ese permiso, solo se
  pierden los clics: el autofocus sigue al cursor igual.
- Se guarda en coordenadas de la zona grabada (0–1), no de la pantalla: así no
  importan la escala ni el monitor.
- Opcional, en una segunda vuelta: grabar **sin** cursor (`showsCursor =
  false`) y dibujarlo en el editor. Permite suavizarlo, agrandarlo y esconderlo
  quieto, como Screen Studio. Por ahora conviene dejarlo grabado en el video:
  es lo seguro.

Costo: un hilo que lee un punto 60 veces por segundo, nada al lado del video.
Va en `platform/macos/screen_record.rs` (y en el de Windows, con
`GetCursorPos` y un hook de bajo nivel).

### 2. El proyecto

```jsonc
{
  "source": "Grabación ….mp4",
  "trim": [0.0, 42.3],                     // segundos
  "cuts": [[12.0, 15.5]],
  "zooms": [
    { "start": 3.2, "end": 7.9, "scale": 2.0, "focus": "auto" },
    { "start": 20.0, "end": 24.0, "scale": 1.6, "focus": [0.30, 0.42] }
  ],
  "texts": [
    { "start": 0.0, "end": 3.0, "text": "Cómo crear un proyecto", "at": "top", "style": "title" }
  ],
  "cursor": { "track": true },
  "output": { "height": 1080, "fps": 60 }
}
```

### 3. El autofocus (sin IA, determinista)

No hace falta ningún modelo: es geometría con el registro del mouse.

1. **Dónde acercar.** Los clics que caen a menos de ~1,5 s entre sí forman un
   grupo. Cada grupo propone un tramo de zoom desde 0,4 s antes del primero
   hasta 1,2 s después del último, a ×1,8. Los tramos se ven en la línea de
   tiempo y se pueden mover, alargar o borrar.
2. **Hacia dónde mira la cámara.** Dentro de un tramo, la cámara sigue al
   cursor con un **resorte críticamente amortiguado** (sin rebote; ω ≈ 6). Tiene
   una zona muerta: mientras el cursor está en el 50 % central del cuadro, la
   cámara no se mueve. Así no "marea".
3. **Entrar y salir.** El zoom entra y sale en ~0,6 s con `ease_smooth_out`
   (`anim.rs`), y nunca muestra fuera del video: la cámara se acota a los bordes.
4. **Todo sale del tiempo** (regla 3 de `transiciones-fluidas`): la cámara en el
   segundo `t` se calcula desde el proyecto y el registro, no desde el cuadro
   anterior. Por eso adelantar, retroceder o exportar dan exactamente lo mismo
   que la vista previa. El resorte se integra una sola vez al cargar (un arreglo
   de cuadros de cámara a 60 Hz) y después solo se lee.

Lo mismo sirve para un zoom fijo: `focus` con un punto en vez de `auto`.

### 4. La vista previa (liviana)

- **AVPlayer** reproduce el original con audio. Un `AVPlayerItemVideoOutput`
  entrega el `CVPixelBuffer` del momento y se dibuja con `gpui::surface`, igual
  que la pantalla del celular (`atic_capture::stream::HardwareFrame`). La
  imagen nunca pasa por la CPU.
- El **zoom de la vista previa no re-renderiza nada**: es la misma superficie
  dibujada más grande y corrida dentro de un contenedor recortado. Cuesta lo
  mismo que no tener zoom.
- Los **textos** son elementos de GPUI encima, con su entrada y salida.
- Los cortes se respetan saltando el reproductor (`seek`) al llegar a uno.

### 5. La línea de tiempo

En el mismo lenguaje de la Pizarra (`board::island`, `INK`, `CARD`, `BUTTON`):

```
┌───────────────────────── vista previa ─────────────────────────┐
│                                                                │
└────────────────────────────────────────────────────────────────┘
 ( ▶  0:07 / 0:42 )  ( ✂ Cortar · 🔍 Zoom · T Texto · ✨ Autofocus )  ( ⬇ Exportar  ✓ Listo  ✕ )
 ├──0:00────0:05────0:10────0:15────0:20────0:25────0:30────0:35───┤
 ▕▓▓▓▓▓▓▓▓▓ miniaturas del video ▓▓▓▓▓▓▓▓│░░ corte ░░│▓▓▓▓▓▓▓▓▓▓▓▓▏  video
 ▕     ▐██ zoom ×2 ██▌                 ▐█ zoom ×1.6 █▌             ▏  zoom
 ▕▐▒ Título ▒▌                                  ▐▒ Nota ▒▌          ▏  texto
                    ▲ cabezal
```

- **Miniaturas** con `AVAssetImageGenerator`, en segundo plano, ~1 cada 2 s.
- **Cabezal guiado por gesto** (regla de la skill): arrastrar mueve la vista
  previa 1:1. Soltar no anima.
- Bloques con los **bordes arrastrables** para alargar o acortar. Clic: elegir
  y editar (escala del zoom, texto y estilo). Supr: borrar.
- Teclas: Espacio reproduce; S corta en el cabezal; Z agrega un zoom; T agrega
  un texto; ⌘Z deshace (como `UNDO_MAX` de la Pizarra).
- La barra de herramientas reutiliza `board::island` y la píldora `glide` para
  la herramienta elegida.

### 6. Exportar (rápido, por hardware)

En macOS: `AVMutableComposition` para el recorte y los cortes (el audio va con
ellos), más una `AVMutableVideoComposition` con
`applyingCIFiltersWithHandler`. En el bloque de cada cuadro:

1. Se lee la cámara del momento (paso 3) y se recorta y escala con Core Image
   (`cropped` + `transformed`, con buen filtro de escalado). Todo en GPU.
2. Se encima cada texto activo, dibujado una sola vez con Core Text a una
   imagen y desvanecido según su tramo.
3. `AVAssetExportSession` (o `AVAssetWriter`, que ya usamos en
   `record_macos.rs`) codifica en H.264/HEVC por hardware.

Un video de 1 minuto a 1080p60 debería exportarse en unos segundos en un Mac
con Apple Silicon. Lo mismo, con otras piezas, en Windows: Media Foundation y
`ID3D11VideoProcessor` para escalar y recortar (ver `GRABACION_WINDOWS.md`).

GIF (después): exportar a 15 fps y ~800 px de ancho con `CGImageDestination`.

## Orden propuesto

| Paso | Qué | Para probar | Riesgo |
|---|---|---|---|
| 1 | Registrar mouse y clics al grabar (`.atic/….json`) | Grabar y mirar el JSON | Bajo |
| 2 | **Editar** en el estante → ventana con vista previa y reproducir/pausar | Abrir una grabación | Medio (AVPlayer + surface) |
| 3 | Línea de tiempo: cabezal, miniaturas, recorte y cortes | Cortar y reproducir | Medio |
| 4 | Zoom por tramos manual + exportar con zoom | Exportar y comparar con la vista previa | Medio |
| 5 | Autofocus (tramos desde los clics + cámara con resorte) | Grabar un recorrido con clics | Bajo (es matemática, con tests) |
| 6 | Textos por tiempo | Título al inicio | Bajo |
| 7 | Pulido: cursor propio, fondo y bordes redondeados, GIF, Windows | — | — |

Cada paso deja algo usable. Se pueden juntar 1 y 2 en una sola compilación.

## Con qué modelo hacerlo (barato y rápido)

**Dentro de la app no hace falta IA.** El autofocus, los zooms y la exportación
son geometría y AVFoundation: corren offline y gratis. Si después quieres
extras:

- **Subtítulos automáticos:** con el Whisper local que ya usa Atic
  (`crates/transcribe`, `local_stt.rs`). Gratis y sin red.
- **Títulos o capítulos sugeridos** a partir de esos subtítulos: **Claude Haiku
  5.5**, el modelo rápido y barato. Es una llamada corta por video.

**Para construirlo** (los agentes que escriben el código):

- **Opus**, para el núcleo delicado: FFI de AVFoundation (vista previa y
  exportación), la cámara del autofocus y la coreografía del editor. Ahí un
  error cuesta caro (pánicos de objc2, cuadros que no calzan).
- **Sonnet 5.5 en paralelo**, para lo mecánico y bien acotado: la UI de cada
  pista de la línea de tiempo con un brief común, i18n en/es, tests del
  algoritmo de cámara y el port de Windows a partir de `GRABACION_WINDOWS.md`.
  Es más rápido y barato, y así lo prefieres para trabajo repartible.
- **Haiku 5.5**, para tareas triviales: revisar strings y JSON, o resumir logs.

## Preguntas para decidir

1. ¿El editor como **ventana propia** (redimensionable, como Flip) o encima de
   la pantalla como la Pizarra? Propuesta: ventana propia; una línea de tiempo
   necesita espacio y quedarse abierta.
2. ¿Grabar sin cursor y dibujarlo en el editor (paso 7) o dejarlo en el video?
3. ¿Exportar reemplazando el original o siempre como «(editada)»? Propuesta:
   siempre aparte.
