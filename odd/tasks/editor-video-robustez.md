# Editor de video: robustez, peso y selector de modelo

Objetivo: cerrar los dos defectos que borran datos antes de publicar el WIP del
grabador/editor, arreglar los defectos verificados del exportador, bajar el peso
de las grabaciones y dejar elegir modelo de IA en el editor. Rama: `dev` (sin
commits: hay trabajo ajeno en el mismo árbol). Test runner: `cargo test
--manifest-path apps/atic/Cargo.toml --target-dir "$HOME/Library/Caches/atic-gpui-target" <filtro>`
y `cargo test --locked -p atic-capture --features h264 --target-dir "$HOME/Library/Caches/atic-ct" <filtro>`.

## Specs

- S1. Exportar nunca puede reemplazar el video original ni el máster: si el
  destino es el mismo archivo, el editor lo rechaza con un aviso y no exporta.
  Cualquier otro archivo existente se reemplaza solo tras una confirmación
  explícita (el botón cambia a «Reemplazar» y hace falta un segundo clic).
- S2. Si al guardar una grabación falla el traslado desde el temporal, el
  temporal **no se borra**: se conserva y el error dice la ruta donde quedó.
  Descartar sí borra.
- S3. Exportar a 30 fps desde una fuente de 60 fps produce un archivo de 30 fps
  (hoy sale a 60). A la misma cadencia de la fuente no se descarta ningún cuadro.
- S4. Un error del lector de video (`AVAssetReader` fallido) es un error de la
  exportación, no un fin de archivo: no se publica un MP4 truncado como éxito.
  Lo mismo al juntar audio en la grabación.
- S5. Solo se auto-aprueban las herramientas del MCP del editor
  (`mcp__atic-editor__atic_editor_*`), no cualquier tool cuyo nombre contenga
  «atic-editor».
- S6. En macOS con Apple Silicon la grabación se escribe en HEVC (≈40–55 % menos
  bits a igual calidad) y el cursor horneado se re-encodea en HEVC con tope de
  1080p; si el codificador no acepta HEVC, se cae a H.264 sin perder la toma.
- S7. El export por defecto no agranda: alto máximo 1080 (nunca agranda una
  fuente menor) y H.264 Standard, en vez del alto del original.
- S8. La pestaña «Asistente» deja elegir el modelo del agente (y el esfuerzo si
  el agente lo ofrece). La elección se recuerda por agente y se aplica al abrir
  la sesión (`want.config`) o en caliente (`setConfig`).

## Tasks

| ID | Qué | Specs | Ruta | Estado |
| --- | --- | --- | --- | --- |
| T1 | Destino de exportación seguro (editor + hilo) | S1 | inline | hecho |
| T2 | Temporal de grabación a salvo si falla el guardado | S2 | inline | hecho |
| T3 | Cadencia de salida y errores del lector | S3, S4 | inline | hecho |
| T4 | Auto-aprobación exacta del MCP del editor | S5 | inline | hecho |
| T5 | HEVC al grabar y al hornear el cursor | S6 | inline | hecho |
| T6 | Defaults de exportación sin agrandar | S7 | inline | hecho |
| T7 | Selector de modelo en el Asistente | S8 | inline | hecho |

## Log

**L1** (2026-11-01, verbatim): «Revisa este proyecto. Me gustaría subir cosas,
pero primero hazle code revieva las cosas y para tener una segunda opinión
rápida, puedes preguntarle a algún modelo de Codex que sea bueno. Estuvimos
trabajando en la parte del editor de video y el grabador de video. Una de las
cosas que me preocupa es el peso de los videos. no sé si eso se pueda optimizar
de alguna forma que pesen menos ya grabados, pero que después de que se exporten
ya estén en un formato más pesado. Y también ver qué otras opciones podemos
robar de Open screen y cosas así para añadirlo, como por ejemplo, sé que también
hay ángulos en 3D, sombras, mayor cantidad de variantes de cursores. También veo
que te deja usar la IA, pero no te deja, por ejemplo seleccionar qué modelo
quieres usar si te deja usar el proveedor, pero no el modelo. Así que revisemos
todas esas cosas.»

**L2** (2026-11-01, verbatim): «aaranta con todo lo que puedas, trata de no usar
el proveedor claude. a menos que sea sonnet en medium. tambie tengo poca cuota en
codex asi que puedes usar luna supongo. tengo a otro proyecto haciendo cmabios
aqui asi que revisa que no lo choques»

**L3** Revisión previa: Codex `gpt-6-astra` (segunda opinión) + revisión propia
estática. Verificado en código: S1 (export_panel:821/534), S2
(screen_record:154), S3 (export_macos:710), S4 (export_macos:1032), S5
(assistant:370), S6 (record_macos:523, screen_recording:437). Descartado por
medición: los «huecos VFR» no se reproducen (hueco máximo 72–105 ms en los
másters reales), así que no se toca.

**L4** Medición real de los másters (evidencia para S6/S7): 1080p ≈ 7,0 Mbps
(60 MB en 69 s; 130 MB en 149 s); 2880×1800 ≈ 7,4–7,55 Mbps (96 MB en 103 s;
117 MB en 124 s). El tope de bitrate no se alcanza: pesa la resolución.

**L5** Restricción de convivencia: otro agente está editando el mismo checkout.
Solo se tocan archivos del editor/grabador/exportador; no se hacen commits; se
revisa `git status` antes de cada tanda de cambios.

**L6** Hecho y comprobado (checks): `cargo check` de la pill sin errores; `cargo test`
de la pill **966 pasan, 0 fallan, 7 ignorados**; `cargo test -p atic-capture
--features h264` **99 pasan, 0 fallan, 5 ignorados**. Tests nuevos/ajustados:
`same_file` (`el_mismo_archivo_se_reconoce_aunque_la_ruta_se_escriba_distinto`),
`keep_at` a 30 fps (`exportar_a_menos_cuadros_por_segundo_los_descarta`),
`is_editor_tool` con prefijos exactos y modelo largo en el chip, `CursorStyle::Arrow`
en `Sprites::pick`, y los dos defaults (`ExportSettings`, `render_dims`).

**L7** Decisiones de peso: la grabación en HEVC se limita a `aarch64` (todos los Apple
Silicon tienen codificador HEVC; en Intel no alcanza a ser seguro). El horneado del
cursor usa HEVC + tope 1080 y cae a H.264 si falla. Windows sigue en H.264 (el
codificador HEVC de Media Foundation depende de una extensión de la Store).

**L8** Lo que **no** se hizo: 3D/orbita, click impact, marcos de dispositivo, cortar por
transcripción y flechas/imágenes como anotación. Son funciones nuevas sobre el
`Composer` y quedan fuera de esta vuelta de robustez; el detalle está en la revisión
de la sesión (2026-11-01).

**L9** Sin commits: otro agente trabaja en el mismo checkout. Verificado que
`git status` no cambió por fuera de este trabajo (solo se agregó `odd/`). No se
relanzó la pill de dev: hay una corriendo (`~/Library/Caches/atic-gpui-target/debug/Atic
Pill.app`) de la otra sesión y `pill-dev-mac.sh` la cerraría.

**L10** (2026-11-01, después de compilar) Dos bugs nuevos del usuario con dos pantallas:
(a) grabar tomaba las dos y solo mostraba una, con el mouse desincronizado; (b) al
arrastrar desde el borde de la pantalla el mouse "agarraba la interfaz y la movía".
Causa de (a): la mira congela el escritorio entero (`whole_desktop`) y el codificador
es de un solo monitor (`display_for`), mientras el `CursorTrack` se mide sobre la zona
pedida. Arreglo: grabar es de **una** pantalla —`Session::pick_rect` usa la pantalla
del cursor para «pantalla entera» y recorta el arrastre a la pantalla donde empezó
(`clip_to_screen`), y `screen_recording::one_screen` recorta por si algo pide cruzar
(línea de log `grabar: la zona cruzaba pantallas`). De (b) no hubo reproducción: el
único camino que movía la ventana desde fuera es `refresh_screen` (`overlay.move_to`);
se corta mientras `capture`/`capture_pending` estén activos.

**L11** Compilado y corriendo: `scripts/pill-dev-mac.sh` (174 s + relanzado dos veces).
Tests: 967 pasan, 0 fallan, 7 ignorados; capture 99 pasan.

**L12** (2026-11-01) «Copiar texto» fallaba con `CRImageReaderError 1`. Investigado con
Sonnet 5.5 high (delegado, con el log unificado de macOS): **no es el tamaño** —el
éxito de 251 palabras y los dos fallos fueron de 1920×1080—. El modo preciso de
Vision queda marcado como listo aunque su precalentamiento falle (`ocr.rs:99-101`
ignoraba el resultado) y después toda lectura precisa muere (E5RT Code 13). Arreglo en
`ocr.rs`: `ACCURATE_BROKEN` + solo marcar listo si el precalentamiento leyó de verdad +
`with_fallback` que repite con el rápido y apaga el preciso para el proceso +
`recognize_in_background` sin forzar el preciso. El error de `text_select` ahora dice
el tamaño de la imagen. Verificado: `cargo check` limpio, 968 tests en verde, pill
recompilada y abierta.

**L13** (2026-11-01) Calidad de «Copiar texto». Investigado con gpt-6.1-sol: los dos
rectángulos punteados de la captura son **figuras guardadas** de la Pizarra
(`figuras.json`: dos `Rect` blancos con `dashed: true` y esos límites), no un fallo de
exportación; y la barra «Copiar / Copiar todo» ya estaba en la **foto original** de esa
edición, o sea que la captura incluía la propia Pizarra. Al medir la calidad apareció lo
importante: el nivel rápido de Vision leía basura el texto chico. Arreglos:
`ocr::prewarm()` al arrancar (antes de ventanas y GPU: así el preciso compila bien; en
marcha fallaba con E5RT) y `ocr_zoom` en macOS sube ×2 las capturas de ≤1600 px (antes
solo Windows). Verificado en la foto real: de «l Revlsar edltory grobadorde TldBo» a
«Revisar editor y grabador de video». Sin la advertencia `ocr: el modo preciso no
arrancó` en el log tras relanzar → el preciso arrancó.
