# Editor de grabaciones: pulido (2026-11-01)

> **Estado: hecho en macOS.** Los textos nuevos ya están fusionados en
> `apps/atic/assets/i18n/pill.*.json` (los fragmentos temporales se borraron). Lo que
> sigue es el contrato con que se repartió el trabajo; sirve de mapa de los módulos.

Contrato común de los agentes que trabajan en esto. Parte del repaso de
`docs/EDITOR_GRABACIONES.md` y de lo que se encontró al revisarlo. Lo que ya
existe está descrito en ese documento; aquí solo va lo nuevo y las reglas.

## Reglas para todos los agentes

1. **Español de Chile (tuteo)** en comentarios, mensajes y textos de la interfaz.
   Los identificadores van en inglés, como ya está en el código.
2. **No compiles el pill.** El Mac tiene 8 GB y una compilación en paralelo ya
   tumbó el equipo. Solo los agentes que tocan `crates/capture` pueden correr
   `cargo check -p atic-capture`, y con candado:
   `mkdir /tmp/atic-cargo.lock` (si falla, otro lo tiene: espera 30 s y
   reintenta; **siempre** `rmdir /tmp/atic-cargo.lock` al terminar, aunque falle),
   `--locked -j 2 --target-dir "$HOME/Library/Caches/atic-ct"`. Los errores en
   archivos que no son tuyos no son tuyos: anótalos en el informe.
   Nadie corre `cargo test`, `cargo build` ni `pill-dev-mac.sh`.
3. **Solo editas tus archivos** (los de tu encargo). Si necesitas algo de otro,
   anótalo en el informe; no lo toques.
4. Hay cambios ajenos sin commit en el árbol. **Prohibido** `git checkout`,
   `restore`, `stash`, `reset`, `clean`, `cargo fmt` y borrar lo que no creaste.
   No hagas commit ni push.
5. **i18n:** no toques `apps/atic/assets/i18n/*.json`. Escribe tus claves en
   `docs/i18n-fragments/<tu-nombre>.json` con la forma
   `{ "es": { "pill.editor.clave": "…" }, "en": { "pill.editor.clave": "…" } }`.
   El español es de Chile con tuteo; el inglés, natural y corto.
6. **Estilo:** imita el archivo que tocas (densidad de comentarios, nombres,
   idioma). No reformatees lo que no cambias. Sin `unwrap()` donde algo puede
   fallar. En objc2, un selector inválido es un **pánico** en debug: usa solo
   selectores que conozcas con certeza y sigue el patrón de lo que ya hay.
7. **Pruebas:** la lógica pura lleva tests `#[cfg(test)]` (no se correrán ahora;
   escríbelos para que pasen y verifica los números a mano). Para lo visual no
   inventes tests que copien la implementación.
8. **Revisa tu propio diff como si fueras `rustc`** antes de terminar
   (`git diff -- <tus archivos>`): tipos, préstamos, imports, campos nuevos en
   literales de struct, `match` exhaustivos.
9. **Informe final** (máx. 40 líneas): archivos tocados; qué quedó hecho; qué NO
   quedó y por qué; dudas y riesgos; claves i18n; tests escritos.
10. Casos borde que importan: grabaciones de horas (memoria), un solo cuadro,
    video sin audio, sin registro del mouse (`track == None`), proyectos
    `.edit.json` viejos (todo campo nuevo lleva `#[serde(default)]`).

## Quién es dueño de qué (ola 1)

| Agente | Archivos |
| --- | --- |
| A1 exportador | `crates/capture/src/stream/export_macos.rs` |
| A2 audio y texto del exportador | `crates/capture/src/stream/export_audio_macos.rs`, `export_text_macos.rs` |
| B análisis, GIF y reproductor | `crates/capture/src/stream/export_gif_macos.rs`, `analysis_macos.rs`, `player_macos.rs` |
| C lógica del proyecto | `apps/atic/src/edit_project.rs` |
| D grabación | `apps/atic/src/screen_cursor.rs`, `apps/atic/src/screen_recording.rs`, `apps/atic/src/platform/macos/screen_record.rs`, `apps/atic/src/screen_audio.rs` |

`crates/capture/src/stream/mod.rs` ya declara los módulos y reexporta los
tipos; no lo edites. Los tipos nuevos del exportador (`ExportOptions`, `Codec`,
`Quality`, `SpeedSpan`, `CursorShape`, `Redaction`, `Decor::default()`,
`TextSpec::plain`, `Backdrop::Image`…) **ya están escritos** al principio de
`export_macos.rs`; úsalos tal cual (A1 puede ajustarlos si hace falta y lo
anota). Los archivos `export_audio_macos.rs`, `export_text_macos.rs`,
`export_gif_macos.rs` y `analysis_macos.rs` son stubs con `todo!()` y las firmas
del contrato: el dueño reemplaza el cuerpo y **no cambia las firmas** sin avisar.

## Contrato del exportador (A1, A2, B)

- **Tiempo de salida.** El segundo del original `src` queda en el exportado en
  `out = Σ (tramo kept anterior, dividido por su velocidad) + (src − inicio del tramo) / velocidad`.
  `Export.segments` son los tramos que quedan; `Export.speeds` los que van a
  otra velocidad (dentro de `segments`; lo que no está en `speeds` va a 1×).
  La duración total de salida es `Σ (b − a) / rate`.
- **Cuadros.** A velocidad > 1 llegan más cuadros por segundo de salida de los
  que caben: se descartan los que quedarían a menos de `0.9 / fps` del anterior.
- **Audio.** Tramos a 1×: el PCM del original con fundido de 15 ms en cada
  empalme. Tramos con `rate != 1`: silencio de `(b − a) / rate`. Se lee por
  bloques, nunca el audio entero en memoria (una hora de audio son ~690 MB).
- **Cuadro de salida.** `out_h = min(alto del original, max_height)` y
  `out_w = out_h × aspecto` (par, `& !1`); sin `aspect`, el del original. Si el
  aspecto es otro que el del original, el video entra entero y centrado (con el
  margen de `FrameSpec.padding`; sin `frame`, margen 0 y fondo negro) sobre el
  fondo. El recorte de la cámara (`Decor.crop`) sigue siendo relativo al original.
- **Bitrate.** `pixeles × 30 × factor` con factor Low 0,07 · Standard 0,12 (lo de
  hoy) · High 0,20 · Max 0,35; HEVC × 0,6; tope entre 2 y 60 Mbps. Se lee
  `options.fps` solo como `ExpectedSourceFrameRate` y para el ritmo de cuadros.
- **Texto.** `text_image` parte el texto en líneas (tope 90 % del ancho; 80 %
  si `caption`), centrado, con la píldora solo si `boxed`; `size` multiplica el
  tamaño base `out_h × 0,045` (con tope de 18–96 px antes de multiplicar).
  `caption` se ubica sobre el borde de abajo con margen mayor.
- **Zonas tapadas (`Redaction`).** Sobre el cuadro original, antes del recorte de
  la cámara: difuminado (`CIGaussianBlur`) o pixelado (`CIPixellate`) recortado al
  rectángulo, mezclado con `alpha`.
- **Cursor.** `cursor_shape` elige el sprite; `cursor_alpha` lo desvanece. Las
  formas que no tengan sprite propio se dibujan con `drawn_*` como `drawn_arrow`
  (mano e I-beam hechas a mano con CoreGraphics, igual que la flecha).
- **GIF (`export_gif`).** Lee el MP4 exportado (AVAssetImageGenerator con
  tiempos fijos a `fps`), `max_width` y `fps` como tope, paleta global razonable,
  bucle infinito; cancela y limpia. Memoria acotada: un cuadro a la vez.
- **Análisis.** `audio_levels` lee PCM con AVAssetReader por bloques (nunca todo
  en memoria); `extract_wav_16k` deja un WAV mono 16 kHz de 16 bits (cabecera
  RIFF correcta) por bloques.
- **Reproductor.** `VideoPlayer::set_rate(&self, f64)` (afecta a `play`; 1.0 por
  defecto) y `set_volume(&self, f32)`.

## Contrato del proyecto (C) y del registro del mouse (D)

`Track` (en `screen_cursor.rs`, D) gana, con `#[serde(default)]`:

```rust
/// La forma del cursor del sistema en cada cambio: (ms, código). 0 flecha, 1 mano,
/// 2 I-beam, 3 cruz. Solo se anota cuando cambia.
pub shapes: Vec<(u32, u8)>,
```

y `Track::shape_at(&self, ms: u32) -> u8` (0 si no hay datos). C lee solo
`track.shapes`/`shape_at`; los literales de `Track` en los tests de C y de D
llevan `shapes: Vec::new()`.

`edit_project.rs` (C) expone, además de lo que ya hay (sin romper su API):

```rust
pub enum Aspect { Source, Wide /*16:9*/, Vertical /*9:16*/, Square /*1:1*/, Portrait /*4:5*/ }
impl Aspect { pub const ALL: [Aspect; 5]; pub fn ratio(self, source: (u32, u32)) -> (u32, u32); }
pub enum Format { Mp4, Gif }
pub enum Quality { Low, Standard, High, Max }
pub enum Codec { H264, Hevc }
pub struct ExportSettings { pub format: Format, pub aspect: Aspect, pub height: Option<u32>,
                            pub quality: Quality, pub codec: Codec, pub fps: u32 } // serde(default); Mp4, Source, None, Standard, H264, 60
pub enum CameraStyle { Calm, Normal, Dynamic }   // Default: Normal
pub struct Speed { pub start: f64, pub end: f64, pub rate: f64 }
pub struct Blur  { pub start: f64, pub end: f64, pub rect: [f64; 4], pub pixelate: bool }
// Project gana: camera: CameraStyle, speeds: Vec<Speed>, blurs: Vec<Blur>,
//               backdrop_image: Option<PathBuf>, export: ExportSettings
// Backdrop gana la variante unitaria `Image` (la ruta va en Project.backdrop_image; Look sigue siendo Copy).
// Text gana (serde default): size: f64 (1.0), color: u32 (0xffffff), boxed: bool (true), bold: bool (true), caption: bool (false)
// CursorOpts gana: hide_idle: bool (true)

impl Project {
    pub fn add_zoom(&mut self, at: f64, len: f64, duration: f64) -> Option<usize>;      // None si no cabe; nunca duplica ni apila
    pub fn tidy(&mut self, duration: f64);                                              // junta/limpia zooms, ordena
    pub fn add_speed(&mut self, start: f64, end: f64, rate: f64) -> Option<usize>;
    pub fn speed_spans(&self, duration: f64) -> Vec<(f64, f64, f64)>;                   // (a, b, rate) dentro de los tramos que quedan
    pub fn out_time(&self, src: f64, duration: f64) -> Option<f64>;                     // el exportado, con cortes y velocidades
    pub fn out_length(&self, duration: f64) -> f64;
    pub fn suggest_idle(&self, levels: &[f32], per_second: u32, track: Option<&Track>, duration: f64, min_len: f64) -> Vec<(f64, f64)>;
    pub fn blurs_at(&self, t: f64) -> Vec<(usize, f64)>;                                 // índice y opacidad (con fundido)
    pub fn load_checked(video: &Path) -> (Self, Option<String>);                        // el texto del error si el JSON estaba dañado
    pub fn save(&self, video: &Path) -> Result<(), String>;                             // atómico (tmp + rename) y deja `.bak`
}
impl Motion { pub fn cursor_alpha(&self, t: f64) -> f64; pub fn cursor_shape(&self, t: f64) -> u8; }
pub struct History { /* undo/redo de `Project` con agrupación por clave */ }
impl History {
    pub fn new(initial: &Project) -> Self;
    pub fn record(&mut self, project: &Project, key: Option<&'static str>);  // misma clave dentro de 600 ms: reemplaza el último paso
    pub fn undo(&mut self, current: &Project) -> Option<Project>;
    pub fn redo(&mut self, current: &Project) -> Option<Project>;
    pub fn can_undo(&self) -> bool; pub fn can_redo(&self) -> bool;
}
```

`Project::length` y `segments` siguen igual (tiempo del original sin velocidades);
`out_length` es la duración real del exportado. `ZOOM_RAMP` sigue siendo `pub const`.

## Lo que NO entra en esta vuelta (necesita decisión del usuario)

- **Teclas en pantalla:** hoy el registro guarda solo *cuántas* teclas, a propósito
  (privacidad); guardar cuáles es una decisión de producto.
- **Cámara web (PiP):** es una captura nueva (permiso de cámara, sincronía con
  `RecordClock`, otra pista).
- **Windows:** el editor es de macOS; el plan está en `GRABACION_WINDOWS.md`.

---

# Ola 2: la interfaz del editor

La ola 1 (exportador, audio, GIF, reproductor, proyecto, grabación) ya está hecha y
verificada: el export de prueba salió 9:16 con fondo, texto, mano, zona pixelada y
audio sin clics. Ahora la interfaz (`apps/atic/src/recording_editor/`). **Nadie
compila el pill** salvo el integrador (ver abajo): revisa tu diff como `rustc`.

## Fases y dueños

| Fase | Agente | Archivos (solo estos) |
| --- | --- | --- |
| A (en paralelo) | E1a línea de tiempo | `recording_editor/timeline.rs` y, en `mod.rs`, **solo** lo de la línea de tiempo (ver su encargo) |
| A | E2 exportar | `recording_editor/export_panel.rs` (nuevo) |
| A | E3 barra lateral | `recording_editor/style_panel.rs` (nuevo) |
| A | E4 subtítulos | `recording_editor/captions.rs` (nuevo) |
| B (después de A) | E1b núcleo | `recording_editor/mod.rs` (todo lo demás), `palette.rs`, íconos nuevos; y ajusta los archivos de la fase A para que calcen |
| C | integrador | compila (`cargo check`), arregla, prueba en la app |

E2, E3 y E4 **no editan `mod.rs`**: son módulos hijos (`mod export_panel;` etc. los
declara E1b), así que ven los campos y métodos privados de `RecordingEditor` y de
`mod.rs` (`self.project`, `self.track`, `self.button(..)`, `Item`, `Ed`…).
Escriben `impl RecordingEditor { pub(super) fn … }` en su archivo.

## Lo que el núcleo (`mod.rs`, E1b) ofrece a los paneles

E2, E3 y E4 programan contra esto (E1b lo deja exacto):

```rust
enum Item { Zoom(usize), Text(usize), Cut(usize), Speed(usize), Blur(usize) }   // Speed y Blur son nuevos
impl RecordingEditor {
    fn changed(&mut self, cx: &mut Context<Self>);                             // registra un paso de deshacer, recalcula la cámara y guarda
    fn changed_keyed(&mut self, key: &'static str, cx: &mut Context<Self>);    // igual, pero una racha con la misma clave es un solo paso (escribir, arrastrar)
    fn select(&mut self, item: Option<Item>, window: &mut Window, cx: &mut Context<Self>);
    fn now(&self) -> f64;  fn seek(&mut self, t: f64, cx: &mut Context<Self>);
    fn add_cut / add_zoom / add_text / add_speed / add_blur(&mut self, window: &mut Window, cx: &mut Context<Self>);
    fn autofocus(&mut self, window: &mut Window, cx: &mut Context<Self>);      // usa Project::autofocus_track
    fn speed_up_idle(&mut self, window: &mut Window, cx: &mut Context<Self>);  // silencios/inactividad -> velocidad (Project::suggest_idle + add_speed)
    fn cut_idle(&mut self, window: &mut Window, cx: &mut Context<Self>);       // lo mismo, pero como cortes
    fn delete_selected(&mut self, window: &mut Window, cx: &mut Context<Self>);
    fn undo(&mut self, window: &mut Window, cx: &mut Context<Self>);  fn redo(..);
    fn reset_project(&mut self, window: &mut Window, cx: &mut Context<Self>);  // Project::default() conservando nada
    // UI ya existente (se quedan): button, primary, chip, switch_row, section, row, TOP_H…, y los campos:
    // project, track, clicks, duration, source, path, player, selected, text_field, levels: Vec<f32> (nivel del audio, LEVELS_PER_SEC por segundo), …
}
pub(super) const LEVELS_PER_SEC: u32 = 20;
```

Los paneles que E1b llama (los **escriben** E2, E3 y E4; E1b los engancha):

```rust
// export_panel.rs (E2)
pub(super) struct ExportUi { /* diálogo abierto, nombre del archivo (TextInput), exportación en curso/terminada… */ }
impl ExportUi { pub(super) fn new(pal: &Ed, cx: &mut Context<RecordingEditor>) -> Self; }   // crea los TextInput y se suscribe a sus cambios
impl RecordingEditor {
    pub(super) fn export_button(&self, pal: &Ed, cx: &mut Context<Self>) -> gpui::AnyElement;       // el botón de arriba a la derecha (abre el diálogo / muestra el avance / revela lo exportado)
    pub(super) fn export_overlay(&mut self, pal: &Ed, cx: &mut Context<Self>) -> Option<gpui::AnyElement>; // el diálogo como capa absoluta; None si está cerrado
    pub(super) fn open_export(&mut self, cx: &mut Context<Self>);                                    // ⌘E
    pub(super) fn export_busy(&self) -> bool;                                                        // ¿hay una exportación corriendo? (pide cuadros de animación)
    pub(super) fn job(&self, segments: Vec<(f64, f64)>, progress: Arc<AtomicU64>, cancel: Arc<AtomicBool>, preview: bool) -> Export;   // el trabajo del exportador (también para «Ver cómo queda»; `preview`: más chico y rápido)
}
// style_panel.rs (E3)
pub(super) struct StyleUi { /* campos de texto del panel (colores en hex…) */ }
impl StyleUi { pub(super) fn new(pal: &Ed, cx: &mut Context<RecordingEditor>) -> Self; }
impl RecordingEditor {
    pub(super) fn sidebar_panel(&self, pal: &Ed, cx: &mut Context<Self>) -> impl IntoElement;        // TODA la columna derecha, selección incluida
}
// captions.rs (E4)
#[derive(Default)] pub(super) struct CaptionsUi { /* generando, avance, error… */ }
impl RecordingEditor {
    pub(super) fn captions_section(&self, pal: &Ed, cx: &mut Context<Self>) -> gpui::AnyElement;     // la sección «Subtítulos» (E3 la incluye en la barra lateral)
    pub(super) fn start_captions(&mut self, cx: &mut Context<Self>);
}
```

Campos nuevos de `RecordingEditor` que declara E1b: `export_ui: ExportUi` (`ExportUi::new(&pal, cx)`),
`style_ui: StyleUi` (`StyleUi::new(&pal, cx)`), `captions_ui: CaptionsUi` (`Default`), `history: History`, `levels: Vec<f32>` (E1a lo carga), `notice: Option<…>`.
**Sustituyen** a `Exporting`, `fn export`, `fn export_path`, `fn job`, `fn sidebar` y `fn selection`
de `mod.rs` (que E1b borra; E2 y E3 reescriben su equivalente mejorado).

## Reglas de diseño de la interfaz

- Mismo lenguaje visual de lo que ya hay (`Ed` en `palette.rs`; Atic, Material 3 Expressive
  y Liquid Glass). Cada texto con `t("pill.editor.…")` (es y en, fragmento por agente);
  tooltips con `crate::hover::tip`. Sin números ni textos mágicos sueltos: constantes con nombre.
- Todo se ve en claro y oscuro; opciones no elegidas y textos secundarios deben leerse.
- Íconos: los de `apps/atic/assets/icons/` (lista con `ls`); si falta uno, agrégalo
  (SVG 24×24, trazo `currentColor`, como los demás) y anótalo en el informe.
- Nada de colores fijos nuevos salvo los de datos (fondos del marco, color del texto elegido).
- No inventes tests de interfaz; sí para lógica pura nueva (tamaños, mapeos, agrupaciones).


---

# Cierre (2026-11-01)

**Módulos:** `crates/capture/src/stream/` (`export_macos` el exportador, `export_audio_macos`
el audio por bloques con fundidos, `export_text_macos` los textos con CoreText,
`export_gif_macos`, `analysis_macos` niveles de audio y WAV, `player_macos` con velocidad y
volumen); `apps/atic/src/edit_project.rs` (lógica pura con tests, `History`);
`apps/atic/src/recording_editor/` (`mod.rs` el núcleo, `timeline.rs`, `export_panel.rs`,
`style_panel.rs`, `captions.rs`); `screen_cursor.rs` (forma del cursor), `screen_recording.rs`
y `platform/macos/screen_record.rs` (avisos, horneado del cursor seguro).

**Decisiones que conviene recordar**
- Los subtítulos no usan Whisper: la pill compila `atic-transcribe` sin la función `local`, así
  que usan el motor de voz que ya usa Reuniones (modelo local o voz de macOS), nunca la nube.
- A velocidad mayor que 1× el audio del tramo va en silencio.
- Los cuadros se descartan por velocidad solo por encima de 1× (a 1× llegan todos).
- Zooms automáticos separados por menos de 2 rampas se juntan: eso quitó la «respiración»,
  pero la actividad continua da zooms largos (mira el estilo de cámara y los cortes a mano).

**Verificado:** 853 tests del pill y los del crate de captura; export real (9:16, fondo, texto,
mano, pixelado, velocidad 2×, audio sin clics), GIF, niveles de audio y WAV con videos reales;
el editor abierto con una grabación real, el diálogo de exportar y una exportación MP4 y GIF
desde la interfaz, sin pánicos en el log.

**Sin verificar en la app:** la forma del cursor grabada de verdad (necesita una grabación
nueva), los avisos de audio perdido y de cursor no dibujado, subtítulos con un motor de voz
real, fondo con imagen, y tema claro/oscuro con los tres estilos de ventana.
