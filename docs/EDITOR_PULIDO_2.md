# Editor de grabaciones: segunda vuelta (2026-11-01)

Lo que pidió el usuario después de probar la primera vuelta (`EDITOR_PULIDO.md`):

1. **El desenfoque de movimiento de la vista previa tiene que verse como el de la exportación.**
   Hoy no se parece. Causa: la vista previa dibuja el video con GPUI y le pinta encima
   (con elementos de GPUI) el cursor, las ondas, los textos, las esquinas y el fondo, mientras
   que la exportación los compone con Core Image (`compose` en `export_macos.rs`): son **dos
   códigos distintos** y nunca van a coincidir. La solución es que la vista previa pase cada
   cuadro por **el mismo `compose`** y muestre el resultado.
2. **Más personalización por botón, como OpenScreen**: varios botones (pestañas), cada uno con
   varias opciones: suavizado, tipos, tamaño del mouse, efectos de clic, profundidad y
   duración del zoom, fondo, etc.
3. **Seguimiento de la cámara más fluido**: que el mouse quede cerca del centro y nunca llegue
   tan al borde.
4. **Pulir la interfaz, las transiciones y las animaciones** con criterios de Impeccable.

## Línea base medida (puntos 1 y 3)

Con tus 6 registros reales y el Autofocus de hoy, mientras hay zoom: qué tan lejos del centro
de lo que se ve queda el cursor (0 centro, 1 borde):
mediana 0,45 · **p95 0,92** · p99 0,97 · **máx 1,03** · **17,4 % de los cuadros pasan de 0,80**.
El cursor llega al borde e incluso sale del encuadre. Causa: caja de seguimiento de 0,45 de la
media vista + el suavizado de la cámara que llega tarde.

## Metas medibles de la cámara (agente C2)

Mismo cálculo, con `Follow::Balanced` (por defecto): **p95 ≤ 0,55 · p99 ≤ 0,70 · máx ≤ 0,80 · 0 %
sobre 0,80**; `Follow::Centered`: p95 ≤ 0,25 y máx ≤ 0,45; `Follow::Free`: parecido a hoy pero
nunca sobre 0,90. Sin empeorar lo demás: la cámara sigue continua (segunda diferencia máxima por
cuadro de x/y/tamaño, medida igual, no peor que hoy: ~0,0090 en x/y y 0,0020 en el tamaño), sin saltos,
y los zooms siguen entrando y saliendo con su rampa. Se excluyen del cálculo los cuadros en que
la cámara está pegada al borde del video y el cursor cerca de ese mismo borde (ahí es inevitable).
Hay un crate mínimo para medir sin compilar la pill: `/tmp/atic-scratch-edit` (ver abajo).

## Reglas de esta vuelta

Valen las de `EDITOR_PULIDO.md` (español de Chile, no compilar el pill, no tocar archivos ajenos,
sin `cargo fmt`, sin commit, revisar el diff como `rustc`, informe corto). Excepciones:
- Quien toque `crates/capture` puede correr `cargo test --locked -j 2 -p atic-capture --features h264
  --target-dir "$HOME/Library/Caches/atic-ct"` con el candado `/tmp/atic-cargo.lock` (`mkdir`, y `rmdir`
  siempre al terminar).
- Quien toque `apps/atic/src/edit_project.rs` puede correr `/tmp/atic-scratch-edit/sync.sh [filtro]
  [-- --nocapture]`: copia `edit_project.rs` y las definiciones de `Track` a un crate mínimo y corre
  sus tests en segundos (no es el pill). Ya trae sondas con los registros reales de
  `~/Movies/Atic/.atic` (`probe`, `edge_probe`); agrega las tuyas ahí (ese crate es descartable).
- i18n: ya no se usan fragmentos: escribe las claves nuevas directamente en
  `apps/atic/assets/i18n/pill.es.json` y `pill.en.json` (al final de cada archivo, mismo formato:
  sangría de 1 espacio, sin ordenar), solo los agentes de interfaz y de uno en uno.
  `las_claves_del_codigo_existen_en_los_dos_idiomas` exige que cada clave usada exista en ambos.

## Fases

| Fase | Agente | Archivos |
| --- | --- | --- |
| 1 | A3 composición compartida y opciones de aspecto | `crates/capture/src/stream/export_macos.rs`, `export_text_macos.rs`, `mod.rs` |
| 1 | C2 opciones del proyecto y cámara | `apps/atic/src/edit_project.rs` |
| 2 | E5 vista previa con el mismo compose | `recording_editor/mod.rs` (la vista previa), `export_panel.rs` (solo `job`/escena compartida), archivo nuevo `recording_editor/scene.rs` |
| 2 | E6 pestañas y ajustes | `recording_editor/style_panel.rs`, `palette.rs`, íconos |
| 3 | E7 crítica y pulido de interfaz y animación | `recording_editor/*`, `anim.rs` si hace falta |

## Contrato de la fase 1 (A3)

`atic_capture::stream` (reexportado en `mod.rs`) gana:

```rust
/// Lo fijo de lo que se pinta: lo mismo para exportar y para la vista previa.
#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    pub src: (f64, f64),            // medidas del video original (px)
    pub out: (u32, u32),            // medidas del cuadro de salida (px, pares)
    pub frame: Option<FrameSpec>,   // marco/fondo
    pub texts: Vec<TextSpec>,
    pub cursor_px: f64,             // alto de la flecha en px del original
    pub look: LookSpec,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LookSpec {
    pub cursor_style: CursorStyle,  // System | Dot | Ring | Dark
    pub click: ClickSpec,
    pub blur_amount: f64,           // multiplicador del desenfoque de movimiento: 0 apagado, 1 normal, hasta 2
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClickSpec { pub kind: ClickKind /* None | Ripple | Ring | Pulse */, pub size: f64 /* 0,5–2,5 */, pub color: Rgb }
pub enum CursorStyle { System, Dot, Ring, Dark }
// FrameSpec.shadow pasa de bool a f64 (0 sin sombra, 0,5 = la de hoy, hasta 1).
// Export gana `look: LookSpec` (Default = el aspecto de hoy: System, Ripple blanca 1×, blur 1.0).

pub struct Composer { /* CIContext, sprites, textos, marco, pool de salida */ }   // solo el hilo de la interfaz
impl Composer {
    pub fn new() -> Result<Self, String>;
    pub fn set_scene(&mut self, scene: &Scene) -> Result<(), String>;      // rearma lo caro solo si cambió
    pub fn render(&mut self, source: &core_video::pixel_buffer::CVPixelBuffer, decor: &Decor)
        -> Result<core_video::pixel_buffer::CVPixelBuffer, String>;        // NV12 de rango completo, `scene.out`, listo para `gpui::surface`
}
```

El tipo del cuadro de entrada es el que ya entrega `VideoPlayer::frame()` (`HardwareFrame.0`).
La exportación usa **el mismo** `Composer` (y su `compose`): lo que se ve en la vista previa
es lo que sale. Con el aspecto por defecto la exportación no cambia (se verifica con el test
manual `exporta_de_verdad` y `ffmpeg -lavfi psnr` antes y después).

## Rúbrica de interfaz y animación (E7), de Impeccable

- Movimiento con propósito: **retroalimentación** (qué pasó), **continuidad** (seguir un elemento al
  cambiar de vista), **atención** (un momento importante) y **tiempo y control**. No se anima todo.
- La retroalimentación rutinaria es rápida; el énfasis es para momentos que lo merecen.
- Se puede **interrumpir** y **repetir**: repetir la acción rápido o interrumpirla no la rompe ni la
  atrasa; el estado manda y la animación lo sigue (ver la skill `transiciones-fluidas`: un solo dueño
  por elemento, progreso desde el estado, revisar los cuadros intermedios).
- Estados completos: hover, foco visible (teclado), presionado, apagado, cargando, vacío y error.
- Movimiento reducido: se mantiene el cambio de estado y se quita el desplazamiento.
- Pulido: espaciado y alineación consistentes, componentes iguales en pantallas vecinas, textos
  coherentes, jerarquía clara.
- Revisión tipo crítica: puntaje de 0 a 4 con razones por criterio (jerarquía, claridad de la tarea,
  retroalimentación, consistencia, estados), fortalezas a conservar, problemas por prioridad y
  preguntas abiertas.

---

# Informes de la fase 1 (hecha y verificada)

## A3 (capture): `Composer` y opciones de aspecto
- `Scene { src, out, frame, texts, cursor_px, look }`, `LookSpec { cursor_style, click, blur_amount }`, `ClickSpec { kind, size, color }`,
  `ClickKind { None, Ripple, Ring, Pulse }`, `CursorStyle { System, Dot, Ring, Dark }` (Default en los tres primeros; todos `Clone, Debug, PartialEq`).
  `FrameSpec.shadow` es ahora `f64` (0 sin sombra, 0,5 = la de antes, hasta 1). `Export` gana `look: LookSpec`.
- `Composer::new()`, `set_scene(&Scene)` (barato si no cambió), `render(&CVPixelBuffer, &Decor) -> Result<CVPixelBuffer, String>` (NV12 `420f`, tamaño `scene.out`,
  sale de un pool de ≥3 buffers; **no es `Send`**: solo el hilo de la interfaz), y `render_bgra`. `output_size((w, h), max_height, aspect)` es pública.
- La exportación usa el MISMO `Composer` (regresión dorada: PSNR 87,8 dB promedio contra el export de antes). NV12 sale directo de Core Image con la misma matriz BT.601 de rango
  completo que el shader de GPUI (±1 de error). Un `render` tardó ~35 ms en la prueba (cuadro vertical con desenfoque, sin medir en vivo): **la vista previa debe medir y adaptarse**.
- Prueba manual: `ATIC_TEST_COMPOSER=<video> cargo test --locked -j 2 -p atic-capture --features h264 --target-dir "$HOME/Library/Caches/atic-ct" prueba_del_composer -- --nocapture --test-threads=1`
  deja /tmp/atic-composer-prueba.png (NV12), -bgra.png y -estilos.png.
- Construir la `Scene`: `src` de `video_info`; `out` de `output_size(...)` (o el tamaño de la vista previa con la misma proporción); `frame/texts/cursor_px/look` del proyecto.

## C2 (edit_project.rs): modelo y cámara
- Seguimiento nuevo con banda de guarda: con `Follow::Balanced` el cursor ya no pasa de 0,70 del camino al borde de lo que se ve (p95 0,44) salvo cuando la cámara está pegada al borde
  del video (ahí es inevitable: entre el 37 y el 54 % de los cuadros con zoom). `Centered`: máx 0,38; `Free`: máx 0,85. La cámara quedó más suave (segunda diferencia 0,002–0,003).
- Tipos nuevos: `Follow {Centered, Balanced, Free}`, `ZoomSpeed {Fast, Normal, Slow}` (`secs()` 0,5/0,8/1,2), `Ease {Smooth, Snappy, Linear}`, `CameraOpts {depth, speed, ease}`
  (`DEPTH_RANGE`, `clamped()`, `ramp_secs()`, `depth_text()`, `ramp_text()`), `MotionOpts {blur, follow, smoothing}` (`BLUR_RANGE`, `SMOOTHING_RANGE`, `blur_text()`, `smoothing_text()`),
  `CursorStyle {System, Dot, Ring, Dark}`, `ClickKind {None, Ripple, Ring, Pulse}` (ambos con `ALL`), `CursorOpts {size, smooth_level, style, click, click_size, click_color, press_shrink,
  hide_idle, hide_delay}` (`SIZE_RANGE`, `CLICK_SIZE_RANGE`, `HIDE_DELAY_RANGE`, `size_text()`, `smooth_text()`, `hide_delay_text()`, `clamped()`), `Look {backdrop, padding, radius, shadow_amount}`
  (`PADDING_RANGE`, `RADIUS_RANGE`, `has_shadow()`, `clamped()`). `Project` gana `camera_opts` y `motion`; `Project::apply_style(style)`, `active_style() -> Option<CameraStyle>`, `clamp_options()`;
  `Motion::followed(t)`; `smooth_omega(level) -> Option<f64>`.
- ELIMINADOS (el JSON viejo se lee y se migra): `Project.sharp` → `motion.blur`; `Look.shadow` → `shadow_amount`; `CursorOpts.smooth` → `smooth_level`; `CursorOpts.ripple` → `click`.
  `CameraStyle::sigma()` y `follow_box()` ya no existen.

## Lo que rompe la compilación del pill (lo arregla la fase 2)
- `export_panel.rs` (`job`): `cursor.ripple` → `cursor.click`; `!sharp` → `motion.blur`; `look.shadow` → `shadow_amount`; pasar estilo, clic (tipo, tamaño, color), `blur_amount` y sombra al exportador (`Export.look`, `FrameSpec.shadow: f64`).
- `style_panel.rs` (L565, 694–705, 724–725, 758–759, 765–766) y `mod.rs` (L396, 438, 1817 y los tests de L2435 y 2554): los campos renombrados de arriba.

## E7a (movimiento de la interfaz): `recording_editor/fluid.rs`
- `Fluid` (registro de animaciones con nombre, dentro de `RecordingEditor.fluid`) + `Anim` (valor que va a un destino y se puede dar vuelta a medias). Constantes de duración y curva
  (`ease_smooth_out`) solo en `fluid.rs`. `Fluid::begin_frame()` / `animating()`: el editor pide otro cuadro solo mientras algo corre.
- Movimiento reducido: `Fluid::reduced()` (caché de 1 s sobre `crate::code::style::system_reduced_motion`). Sin movimiento: fundido de ≤ 60 ms; sin ascenso, escala, deslizamiento ni
  manilla que crece; la píldora, la perilla y el valor de los controles saltan.
- No se anima: el cabezal, la cámara ni nada que mueva el layout de la vista previa (los avisos son una capa encima, no ocupan lugar). GPUI 0.2.2 no escala un `div` (el diálogo sube 8 px)
  y `paint_surface` ignora la opacidad (el video de «Ver cómo queda» no se cruza con la vista en vivo; solo la insignia se funde).
- Pruebas puras de la lógica (animación interrumpida, dirección del deslizamiento, geometría de la píldora): `cargo test` de `fluid::tests` (corren en un crate mínimo sin gpui).

## E7b (estructura y pulido según la crítica Impeccable) y fondos de imagen
- E7b: pestañas fijas con «Lo elegido» como tarjeta que se desplaza; anillo de foco solo al navegar con teclado (`kbd_nav`: Tab o flechas lo encienden, un clic lo apaga); diálogo de exportar anclado
  y con avance suavizado; acciones de panel con forma de botón; escala 11/12/13/14 y `Ed.sub` (≥ 4,5:1); degradados de scroll; pista de audio sin señal («Sin señal de audio»); «Guardado»,
  avisos con «Deshacer», hoja de atajos (`?`), zoom de la línea de tiempo con deslizamiento, eco de play/pausa e indicador «2,4×». No hecho: nombre en la pestaña activa, ícono de restablecer,
  dígitos tabulares, seguimiento del cabezal al reproducir, Tab dentro del diálogo de exportar. Claves nuevas en `pill.*.json` (38).
- Fondos de imagen: `recording_editor/wallpapers.rs` trae 18 fondos de OpenScreen (MIT; `assets/wallpapers/AVISO.md` con la licencia y la advertencia de procedencia: del 3 al 10 se parecen
  a los de macOS). El proyecto guarda `wallpaper:NN` en `backdrop_image`; `wallpapers::resolve` los saca a `$TMPDIR/atic-wallpapers/` y la usa `backdrop_of` (así la vista previa y el export
  leen el mismo archivo). `Project.backdrop_own` recuerda la última imagen del usuario para que siga en la grilla; los proyectos viejos la derivan de `backdrop_image` (`own_image()`).
- Lección de GPUI: `gpui::img(String)` busca un asset embebido; una ruta de disco hay que pasarla como `PathBuf`/`ImageSource::from(path)`. `truncate()` no dibuja «…» en la barra
  lateral: se recorta el nombre a mano (`short_name`).
- Verificado en la app (2026-11-01): grilla con «+», imagen propia y 18 fondos; elegir un fondo cambia la vista previa y la imagen propia sigue; el aro y los globitos se ven; sin pánicos.
  Sin verificar: tema claro, los tres estilos de ventana, exportar con un fondo del editor, deshacer tras elegir un fondo.
