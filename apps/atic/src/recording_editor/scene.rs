//! La escena que comparten la vista previa del editor y la exportación: un solo camino.
//!
//! `SceneInputs::scene_for` y `SceneInputs::decor_at` arman lo mismo que `Composer`
//! (`atic_capture::stream`) pinta, y `job` (`export_panel.rs`) y el escenario (`mod.rs`) los usan
//! por igual: lo que se ve al editar es lo que sale (desenfoque de movimiento, cursor, ondas,
//! textos, zonas difuminadas, marco, esquinas, sombra y fondo los dibuja Core Image).
//!
//! El escenario pasa cada cuadro por `Composer::render` y muestra el resultado con
//! `gpui::surface`. Eso cuesta (unos 35 ms en una prueba), así que:
//! - `Governor` mide cada render mientras suena y baja la escala de la vista previa (1 → 0,75 →
//!   0,5) si no llega a ~14 ms, y la sube de vuelta con holgura (con histéresis);
//! - con la vista previa quieta no se vuelve a componer: `Shot` guarda la última salida con la
//!   llave del cuadro del reproductor, la escena y el `Decor`;
//! - el tamaño de la salida se acota y se redondea (`preview_dims`), así que arrastrar una
//!   ventana no recompone a cada píxel.

use std::fmt::{self, Write as _};
use std::hash::{DefaultHasher, Hasher};
use std::time::Instant;

use atic_capture::stream as cap;
use atic_capture::stream::{Composer, HardwareFrame};

use super::pic;

use super::export_panel::{backdrop_of, rgb_of, text_spec};
use super::RecordingEditor;
use crate::edit_project::{ClickKind, CursorStyle, Motion, Project};
use crate::screen_cursor::Track;

/// Lo más largo que mide la salida de la vista previa, en píxeles.
pub(super) const PREVIEW_MAX_SIDE: u32 = 1280;
/// El lado largo de la salida se redondea hacia arriba a un múltiplo de esto: al cambiar el
/// tamaño de la ventana la escena (y el cuadro) cambian cada tanto, no a cada píxel.
const SIZE_STEP: f64 = 64.0;
/// Lo que puede tardar un render mientras suena (ms).
const RENDER_BUDGET_MS: f64 = 14.0;
/// Las escalas de la vista previa, de la mejor a la más barata.
const SCALES: [f64; 3] = [1.0, 0.75, 0.5];
/// Cuánto tiene que pasar de largo el promedio para bajar de escala (segundos).
const SLOW_SECS: f64 = 0.5;
/// Cuánto tiene que sobrar para subir de escala (segundos).
const SPARE_SECS: f64 = 2.0;
/// Para subir, el costo previsto a la escala mejor tiene que caber en esta parte del presupuesto
/// (la brecha con 1 es la histéresis: así no oscila).
const SPARE_FACTOR: f64 = 0.8;
/// Cuánto pesa el último render en el promedio móvil.
const AVG_WEIGHT: f64 = 0.2;
/// Un hueco más largo que esto entre dos renders no cuenta como tiempo corrido (segundos).
const MAX_STEP_SECS: f64 = 0.1;

// --- La escena y el Decor (lo comparten la vista previa y `job`) ---------------------------------

/// Todo lo que hace falta para armar la `Scene` y el `Decor` de un cuadro. Son referencias: el
/// escenario lo arma a cada render sin copiar el proyecto; `job` guarda las suyas en el cierre
/// que corre en otro hilo.
#[derive(Clone, Copy)]
pub(super) struct SceneInputs<'a> {
    pub project: &'a Project,
    pub motion: &'a Motion,
    /// El editor dibuja el cursor (la grabación no lo tiene).
    pub cursor_drawn: bool,
    /// El alto de la flecha en píxeles del original (sin zoom).
    pub cursor_px: f64,
    /// Las medidas del video original.
    pub src: (u32, u32),
}

/// El alto del cursor en píxeles del original: 0 si no hay registro del mouse.
pub(super) fn cursor_px_of(track: Option<&Track>, src_w: u32, size: f64) -> f64 {
    track.map_or(0.0, |track| crate::screen_recording::cursor_px(track, src_w as f64, size))
}

impl SceneInputs<'_> {
    /// El aspecto del cursor, los clics y el desenfoque, del proyecto.
    fn look_spec(&self) -> cap::LookSpec {
        let cursor = self.project.cursor.clamped();
        cap::LookSpec {
            cursor_style: match cursor.style {
                CursorStyle::System => cap::CursorStyle::System,
                CursorStyle::Dot => cap::CursorStyle::Dot,
                CursorStyle::Ring => cap::CursorStyle::Ring,
                CursorStyle::Dark => cap::CursorStyle::Dark,
                CursorStyle::Arrow => cap::CursorStyle::Arrow,
            },
            click: cap::ClickSpec {
                kind: match cursor.click {
                    ClickKind::None => cap::ClickKind::None,
                    ClickKind::Ripple => cap::ClickKind::Ripple,
                    ClickKind::Ring => cap::ClickKind::Ring,
                    ClickKind::Pulse => cap::ClickKind::Pulse,
                },
                size: cursor.click_size,
                color: rgb_of(cursor.click_color),
            },
            blur_amount: self.project.motion.clamped().blur,
        }
    }

    /// Lo fijo de lo que se pinta, para un cuadro de salida de `out` píxeles. Con `Backdrop::Image`
    /// y el archivo perdido, el degradado de reemplazo (`backdrop_of`).
    pub(super) fn scene_for(&self, out: (u32, u32)) -> cap::Scene {
        let look = self.project.look.clamped();
        cap::Scene {
            src: (self.src.0 as f64, self.src.1 as f64),
            out,
            frame: look.framed().then(|| cap::FrameSpec {
                backdrop: backdrop_of(look.backdrop, self.project.backdrop_image.as_deref()),
                padding: look.padding,
                radius: look.radius,
                shadow: look.shadow_amount,
            }),
            texts: self.project.texts.iter().map(text_spec).collect(),
            cursor_px: self.cursor_px,
            look: self.look_spec(),
        }
    }

    /// Lo que se ve en el segundo `src` del original.
    pub(super) fn decor_at(&self, src: f64) -> cap::Decor {
        let (project, motion) = (self.project, self.motion);
        let cursor = if self.cursor_drawn { motion.cursor(src).map(|(x, y)| (x, y, motion.cursor_scale(src))) } else { None };
        // La forma y la opacidad solo cuentan si se dibuja el cursor (así no mueven la llave).
        let (cursor_shape, cursor_alpha) = match cursor {
            Some(_) => (crate::screen_recording::cursor_shape_of(motion.cursor_shape(src)), motion.cursor_alpha(src)),
            None => (cap::CursorShape::default(), 1.0),
        };
        let click = self.project.cursor.click != ClickKind::None;
        let blur = self.project.motion.clamped().blur > 0.0;
        cap::Decor {
            crop: motion.crop(src),
            cursor,
            cursor_shape,
            cursor_alpha,
            texts: project.text_alpha(src),
            ripples: if click { motion.ripples(src) } else { Vec::new() },
            blur: blur.then(|| motion.camera_motion(src)),
            redactions: project
                .blurs_at(src)
                .into_iter()
                .filter_map(|(i, alpha)| project.blurs.get(i).map(|b| cap::Redaction { rect: b.rect, pixelate: b.pixelate, alpha }))
                .collect(),
        }
    }
}

// --- El tamaño de la vista previa ------------------------------------------------------------

fn even(value: f64) -> u32 {
    (value.round().clamp(2.0, u32::MAX as f64) as u32 & !1).max(2)
}

/// El cuadro de salida de la vista previa, en píxeles. `full` es el que exportaría; `frame_px`,
/// el cuadro en el escenario en píxeles físicos (0 si aún no se mide: sale `full`); `scale`, la
/// del `Governor`. Tiene la proporción de `full`, sus lados son pares, el lado largo no pasa de
/// `PREVIEW_MAX_SIDE` ni de `full`, y se redondea hacia arriba a un múltiplo de `SIZE_STEP`.
pub(super) fn preview_dims(full: (u32, u32), frame_px: (f32, f32), scale: f64) -> (u32, u32) {
    let (fw, fh) = (full.0.max(2) as f64, full.1.max(2) as f64);
    let long_full = fw.max(fh);
    let wanted = frame_px.0.max(frame_px.1) as f64 * if scale.is_finite() { scale.clamp(0.1, 1.0) } else { 1.0 };
    let long = if wanted.is_finite() && wanted > 0.0 { (wanted / SIZE_STEP).ceil() * SIZE_STEP } else { long_full };
    let long = long.min(PREVIEW_MAX_SIDE as f64).min(long_full).max(2.0);
    let k = long / long_full;
    (even(fw * k), even(fh * k))
}

// --- Medir y adaptar --------------------------------------------------------------------------

/// Mide lo que tarda cada render mientras suena y elige la escala de la vista previa: baja si el
/// promedio pasa de `RENDER_BUDGET_MS` durante `SLOW_SECS`, sube si sobra holgura durante
/// `SPARE_SECS`. Sin reloj propio (le dicen cuánto pasó): se prueba con números.
#[derive(Debug)]
pub(super) struct Governor {
    level: usize,
    /// El promedio móvil de lo que tarda un render a la escala de ahora (ms).
    avg_ms: Option<f64>,
    slow_secs: f64,
    spare_secs: f64,
}

impl Governor {
    pub(super) fn new() -> Self {
        Self { level: 0, avg_ms: None, slow_secs: 0.0, spare_secs: 0.0 }
    }

    /// La escala de ahora (1, 0,75 o 0,5).
    pub(super) fn scale(&self) -> f64 {
        SCALES[self.level]
    }

    pub(super) fn avg_ms(&self) -> Option<f64> {
        self.avg_ms
    }

    /// Anota un render de `ms` milisegundos, `dt` segundos después del anterior. Devuelve la escala
    /// nueva si cambió.
    pub(super) fn observe(&mut self, ms: f64, dt: f64) -> Option<f64> {
        if !ms.is_finite() || ms < 0.0 {
            return None;
        }
        let dt = if dt.is_finite() { dt.clamp(0.0, MAX_STEP_SECS) } else { 0.0 };
        let avg = self.avg_ms.map_or(ms, |avg| avg + (ms - avg) * AVG_WEIGHT);
        self.avg_ms = Some(avg);
        if avg > RENDER_BUDGET_MS {
            self.spare_secs = 0.0;
            self.slow_secs += dt;
            if self.slow_secs >= SLOW_SECS && self.level + 1 < SCALES.len() {
                return Some(self.shift(self.level + 1));
            }
            return None;
        }
        self.slow_secs = 0.0;
        if self.level == 0 {
            return None;
        }
        // El costo es casi proporcional a los píxeles: lo que tardaría a la escala mejor.
        let predicted = avg * (SCALES[self.level - 1] / SCALES[self.level]).powi(2);
        if predicted <= RENDER_BUDGET_MS * SPARE_FACTOR {
            self.spare_secs += dt;
            if self.spare_secs >= SPARE_SECS {
                return Some(self.shift(self.level - 1));
            }
        } else {
            self.spare_secs = 0.0;
        }
        None
    }

    /// Pasa al nivel `level`; el promedio se lleva a lo que se espera allí.
    fn shift(&mut self, level: usize) -> f64 {
        let ratio = SCALES[level] / SCALES[self.level];
        self.avg_ms = self.avg_ms.map(|avg| avg * ratio * ratio);
        self.level = level;
        self.slow_secs = 0.0;
        self.spare_secs = 0.0;
        SCALES[level]
    }
}

// --- La salida guardada ------------------------------------------------------------------------

/// Con qué se compuso una salida: el cuadro del reproductor (el mismo búfer, no uno parecido),
/// la escena y el `Decor` (hashes de su `Debug`).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ShotKey<F> {
    pub frame: F,
    pub scene: u64,
    pub decor: u64,
}

/// La salida de antes sirve si se hizo con la misma llave.
pub(super) fn reusable<F: PartialEq>(cached: Option<&ShotKey<F>>, key: &ShotKey<F>) -> bool {
    cached == Some(key)
}

/// Escribe en un hasher lo que un `Debug` imprime, sin armar el texto.
struct HashWriter(DefaultHasher);

impl fmt::Write for HashWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.0.write(s.as_bytes());
        Ok(())
    }
}

/// El hash de lo que imprime el `Debug` de `value`: igual para valores iguales (todo lo que entra
/// en una `Scene` o un `Decor` está en su `Debug`).
pub(super) fn debug_hash(value: &impl fmt::Debug) -> u64 {
    let mut writer = HashWriter(DefaultHasher::new());
    let _ = write!(writer, "{value:?}");
    writer.0.finish()
}

/// La última imagen compuesta, con su llave. El cuadro de entrada va dentro de la llave: mientras
/// se guarde, nadie le reusa el búfer y no se confunde con otro.
pub(super) struct Shot {
    key: ShotKey<pic::Key>,
    out: pic::Shown,
}

/// Lo del escenario que compone: el `Composer` (creado perezosamente, solo en el hilo de la
/// interfaz), la última salida y la medida de lo que tarda.
pub(super) struct Compose {
    composer: Option<Composer>,
    /// No se pudo crear el `Composer`: se muestra el cuadro tal cual.
    failed: bool,
    shot: Option<Shot>,
    governor: Governor,
    /// La escena del último render y cuándo fue: sirve para no medir el primer render de una escena
    /// (compila y arma cosas) y para saber cuánto pasó entre dos.
    last_scene: u64,
    last_render: Option<Instant>,
    /// El último error que se registró, para no repetirlo a cada cuadro.
    last_error: Option<String>,
    /// Las salidas que ya no se muestran (en Windows son imágenes que GPUI guarda en su atlas hasta
    /// que se le avisa: `Window::drop_image`).
    pub(super) retired: Vec<pic::Shown>,
}

impl Compose {
    pub(super) fn new() -> Self {
        Self { composer: None, failed: false, shot: None, governor: Governor::new(), last_scene: 0, last_render: None, last_error: None, retired: Vec::new() }
    }

    /// La escala de la vista previa mientras suena o se arrastra el cabezal.
    pub(super) fn scale(&self) -> f64 {
        self.governor.scale()
    }

    fn complain(&mut self, what: &str, error: String) {
        if self.last_error.as_deref() != Some(error.as_str()) {
            tracing::warn!(%error, "editor: {what}");
            self.last_error = Some(error);
        }
    }
}

impl RecordingEditor {
    /// Lo que el editor le pasa a la escena: el proyecto, la cámara y el video de ahora.
    pub(super) fn scene_inputs(&self) -> SceneInputs<'_> {
        let src = self.player.size();
        SceneInputs {
            project: &self.project,
            motion: &self.motion,
            cursor_drawn: self.cursor_drawn,
            cursor_px: cursor_px_of(self.track.as_ref(), src.0, self.project.cursor.size),
            src,
        }
    }

    /// El cuadro `frame` del reproductor compuesto en el segundo de ahora para una salida de `out`
    /// píxeles: la misma imagen que exportaría. `full`: con el crop entero (apuntando un zoom o un
    /// difuminado). `interactive`: suena o se arrastra el cabezal (ahí se mide y se adapta la
    /// escala). Si nada cambió desde la última vez, devuelve la misma salida sin componer.
    /// `None` si el `Composer` no pudo.
    pub(super) fn composed(&mut self, frame: &HardwareFrame, out: (u32, u32), full: bool, interactive: bool) -> Option<pic::Shown> {
        let (scene, decor) = {
            let inputs = self.scene_inputs();
            let mut decor = inputs.decor_at(self.now());
            if full {
                decor.crop = [0.0, 0.0, 1.0, 1.0];
                decor.blur = None;
            }
            (inputs.scene_for(out), decor)
        };
        let compose = &mut self.compose;
        let key = ShotKey { frame: pic::key_of(frame), scene: debug_hash(&scene), decor: debug_hash(&decor) };
        if let Some(shot) = compose.shot.as_ref().filter(|shot| reusable(Some(&shot.key), &key)) {
            return Some(shot.out.clone());
        }
        if compose.composer.is_none() && !compose.failed {
            match Composer::new() {
                Ok(composer) => compose.composer = Some(composer),
                Err(error) => {
                    compose.failed = true;
                    compose.complain("no se pudo crear el compositor de la vista previa", error);
                }
            }
        }
        let composer = compose.composer.as_mut()?;
        if let Err(error) = composer.set_scene(&scene) {
            compose.complain("escena inválida en la vista previa", error);
            return None;
        }
        let started = Instant::now();
        let rendered = pic::render(composer, frame, &decor);
        let spent = started.elapsed();
        let buffer = match rendered {
            Ok(buffer) => buffer,
            Err(error) => {
                compose.complain("no se pudo componer el cuadro", error);
                return None;
            }
        };
        // Se mide solo lo que suena, y no el primer render de una escena (arma el marco, el fondo y los textos).
        if interactive && compose.last_scene == key.scene {
            let dt = compose.last_render.map_or(0.0, |at| started.saturating_duration_since(at).as_secs_f64());
            let ms = spent.as_secs_f64() * 1000.0;
            if let Some(scale) = compose.governor.observe(ms, dt) {
                tracing::debug!(avg_ms = compose.governor.avg_ms().unwrap_or(ms), scale, "editor: escala de la vista previa");
            }
        }
        compose.last_scene = key.scene;
        compose.last_render = Some(started);
        let shown = buffer;
        // La salida anterior ya no se muestra: su textura se suelta de GPUI en el próximo `render`.
        if let Some(old) = compose.shot.replace(Shot { key, out: shown.clone() }) {
            compose.retired.push(old.out);
        }
        Some(shown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit_project::{Backdrop, Blur, Look};

    /// Un proyecto y su cámara (sin registro del mouse) de 10 s.
    fn inputs_of(project: &Project, motion: &Motion, cursor_drawn: bool) -> (cap::Scene, cap::Decor) {
        let inputs = SceneInputs { project, motion, cursor_drawn, cursor_px: 30.0, src: (1920, 1080) };
        (inputs.scene_for((960, 540)), inputs.decor_at(1.0))
    }

    #[test]
    fn la_escena_lleva_el_marco_los_textos_y_el_aspecto_del_proyecto() {
        let mut project = Project::default();
        project.look = Look { backdrop: Backdrop::Solid(0xff8000), padding: 0.1, radius: 0.03, shadow_amount: 0.8 };
        project.cursor.style = CursorStyle::Ring;
        project.cursor.click = ClickKind::Pulse;
        project.cursor.click_size = 2.0;
        project.cursor.click_color = 0x0000ff;
        project.motion.blur = 1.5;
        project.texts.push(crate::edit_project::Text::new(0.0, 2.0, "hola", crate::edit_project::TextAt::Top));
        let motion = Motion::new(&project, None, 10.0);
        let (scene, _) = inputs_of(&project, &motion, true);
        assert_eq!((scene.src, scene.out, scene.cursor_px), ((1920.0, 1080.0), (960, 540), 30.0));
        let frame = scene.frame.expect("hay marco");
        assert_eq!(frame.backdrop, cap::Backdrop::Solid([1.0, 128.0 / 255.0, 0.0]));
        assert_eq!((frame.padding, frame.radius, frame.shadow), (0.1, 0.03, 0.8));
        assert_eq!(scene.texts.len(), 1);
        assert_eq!(scene.texts[0].text, "hola");
        assert_eq!(scene.look.cursor_style, cap::CursorStyle::Ring);
        assert_eq!((scene.look.click.kind, scene.look.click.size, scene.look.click.color), (cap::ClickKind::Pulse, 2.0, [0.0, 0.0, 1.0]));
        assert_eq!(scene.look.blur_amount, 1.5);
    }

    #[test]
    fn sin_marco_no_hay_marco_y_la_imagen_perdida_es_el_degradado_de_reemplazo() {
        let mut project = Project::default();
        project.look.backdrop = Backdrop::None;
        let motion = Motion::new(&project, None, 10.0);
        assert!(inputs_of(&project, &motion, false).0.frame.is_none());
        project.look.backdrop = Backdrop::Image;
        project.backdrop_image = Some("/no/existe/esta/imagen.png".into());
        let motion = Motion::new(&project, None, 10.0);
        let frame = inputs_of(&project, &motion, false).0.frame.expect("hay marco");
        assert_eq!(frame.backdrop, backdrop_of(Backdrop::Gradient(super::super::export_panel::FALLBACK_BACKDROP.0, super::super::export_panel::FALLBACK_BACKDROP.1), None));
        assert!(matches!(frame.backdrop, cap::Backdrop::Gradient(..)));
    }

    #[test]
    fn el_clic_ninguno_no_dibuja_ondas_y_el_desenfoque_cero_no_pide_movimiento() {
        let mut project = Project::default();
        project.cursor.click = ClickKind::None;
        project.motion.blur = 0.0;
        let motion = Motion::new(&project, None, 10.0);
        let (scene, decor) = inputs_of(&project, &motion, false);
        assert_eq!(scene.look.click.kind, cap::ClickKind::None);
        assert!(decor.ripples.is_empty());
        assert!(decor.blur.is_none());
        // Con el desenfoque normal, el movimiento de la cámara va (aunque esté quieta).
        project.motion.blur = 1.0;
        let motion = Motion::new(&project, None, 10.0);
        assert!(inputs_of(&project, &motion, false).1.blur.is_some());
    }

    #[test]
    fn el_cursor_solo_va_si_el_editor_lo_dibuja_y_las_zonas_tapadas_salen_del_proyecto() {
        let mut project = Project::default();
        project.blurs.push(Blur { start: 0.0, end: 3.0, rect: [0.1, 0.2, 0.3, 0.4], pixelate: true });
        let motion = Motion::new(&project, None, 10.0);
        let (_, decor) = inputs_of(&project, &motion, true);
        // Sin registro del mouse no hay cursor que dibujar.
        assert!(decor.cursor.is_none());
        assert_eq!((decor.cursor_shape, decor.cursor_alpha), (cap::CursorShape::default(), 1.0));
        assert_eq!(decor.redactions.len(), 1);
        assert_eq!(decor.redactions[0].rect, [0.1, 0.2, 0.3, 0.4]);
        assert!(decor.redactions[0].pixelate);
        assert!(decor.redactions[0].alpha > 0.0);
        assert_eq!(decor.crop, [0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn el_cuadro_de_la_vista_previa_se_acota_y_conserva_la_proporcion() {
        let close = |got: (u32, u32), want: (u32, u32)| assert_eq!(got, want, "{got:?}");
        // Un escenario chico: el lado largo se redondea a un múltiplo de 64.
        close(preview_dims((1920, 1080), (700.0, 393.75), 1.0), (704, 396));
        // Un escenario Retina grande: tope de 1280 por el lado largo.
        close(preview_dims((1920, 1080), (2400.0, 1350.0), 1.0), (1280, 720));
        // Nunca mayor que lo que exportaría.
        close(preview_dims((640, 360), (2400.0, 1350.0), 1.0), (640, 360));
        // Vertical: el lado largo es el alto.
        close(preview_dims((606, 1080), (500.0, 889.0), 1.0), (502, 896));
        // La escala del `Governor` achica.
        let half = preview_dims((1920, 1080), (1800.0, 1012.5), 0.5);
        assert_eq!(half, (960, 540));
        // Sin medida del escenario todavía: lo que exportaría, acotado.
        close(preview_dims((1920, 1080), (0.0, 0.0), 1.0), (1280, 720));
        close(preview_dims((800, 600), (0.0, 0.0), 1.0), (800, 600));
        // Siempre pares y sin cero, con lo que sea.
        for (full, frame, scale) in [((2, 2), (1.0, 1.0), 0.5), ((1921, 1081), (333.0, 777.0), 0.75), ((3412, 1920), (4000.0, 2250.0), 1.0), ((1920, 1080), (f32::NAN, 5.0), f64::NAN)] {
            let (w, h) = preview_dims(full, frame, scale);
            assert!(w >= 2 && h >= 2 && w % 2 == 0 && h % 2 == 0, "{w}x{h}");
            assert!(w <= full.0.max(2) && h <= full.1.max(2) && w.max(h) <= PREVIEW_MAX_SIDE.max(2), "{w}x{h} de {full:?}");
        }
    }

    #[test]
    fn el_tamano_cambia_solo_a_saltos_al_redimensionar() {
        let mut seen = std::collections::BTreeSet::new();
        for width in 500..1000 {
            seen.insert(preview_dims((1920, 1080), (width as f32, width as f32 * 9.0 / 16.0), 1.0));
        }
        // 500 anchos distintos dejan solo unos pocos cuadros distintos (8 múltiplos de 64).
        assert!(seen.len() <= 9, "{} tamaños", seen.len());
    }

    /// Un render que cuesta `k` ms a escala 1 y `k × escala²` a otra, a 60 cuadros por segundo.
    fn run(governor: &mut Governor, cost_at_full: f64, secs: f64) -> Vec<f64> {
        let mut changes = Vec::new();
        for _ in 0..(secs * 60.0) as usize {
            let scale = governor.scale();
            if let Some(new) = governor.observe(cost_at_full * scale * scale, 1.0 / 60.0) {
                changes.push(new);
            }
        }
        changes
    }

    #[test]
    fn un_render_dentro_del_presupuesto_no_cambia_la_escala() {
        let mut governor = Governor::new();
        assert!(run(&mut governor, 10.0, 10.0).is_empty());
        assert_eq!(governor.scale(), 1.0);
    }

    #[test]
    fn un_render_lento_baja_la_escala_tras_medio_segundo_y_no_antes() {
        let mut governor = Governor::new();
        assert!(run(&mut governor, 35.0, 0.4).is_empty());
        assert_eq!(governor.scale(), 1.0);
        let changes = run(&mut governor, 35.0, 0.3);
        assert_eq!(changes, vec![0.75]);
        // 35 × 0,5625 = 19,7 ms: sigue lento y baja otra vez; 35 × 0,25 = 8,75 ms: se queda en 0,5.
        let changes = run(&mut governor, 35.0, 3.0);
        assert_eq!(changes, vec![0.5]);
        assert_eq!(governor.scale(), 0.5);
        assert!(run(&mut governor, 35.0, 10.0).is_empty());
    }

    #[test]
    fn un_pico_suelto_no_baja_la_escala() {
        let mut governor = Governor::new();
        for i in 0..600 {
            let ms = if i % 100 == 50 { 120.0 } else { 8.0 };
            assert_eq!(governor.observe(ms, 1.0 / 60.0), None, "cuadro {i}");
        }
        assert_eq!(governor.scale(), 1.0);
    }

    #[test]
    fn con_holgura_sube_de_vuelta_pero_sin_oscilar() {
        let mut governor = Governor::new();
        run(&mut governor, 35.0, 1.0);
        assert!(governor.scale() < 1.0);
        // A 18 ms a escala 1 (10,1 a 0,75): a 1 volvería a pasar de 14: se queda donde está.
        let changes = run(&mut governor, 18.0, 30.0);
        assert!(changes.is_empty() || changes.iter().all(|s| *s < 1.0), "{changes:?}");
        assert!(governor.scale() < 1.0);
        // A 8 ms a escala 1 sobra de verdad: sube hasta 1 y se queda.
        let changes = run(&mut governor, 8.0, 20.0);
        assert_eq!(changes.last(), Some(&1.0), "{changes:?}");
        assert_eq!(governor.scale(), 1.0);
        assert!(run(&mut governor, 8.0, 20.0).is_empty());
    }

    #[test]
    fn el_gobernador_ignora_lo_roto_y_los_huecos_largos() {
        let mut governor = Governor::new();
        assert_eq!(governor.observe(f64::NAN, 0.016), None);
        assert_eq!(governor.observe(-1.0, 0.016), None);
        assert_eq!(governor.avg_ms(), None);
        // Un hueco de una hora entre dos renders cuenta como un cuadro, no como una hora lenta.
        assert_eq!(governor.observe(40.0, 3600.0), None);
        assert_eq!(governor.observe(40.0, f64::NAN), None);
        assert_eq!(governor.scale(), 1.0);
    }

    #[test]
    fn la_llave_distingue_el_cuadro_la_escena_y_el_decor() {
        let key = ShotKey { frame: 7u32, scene: 1, decor: 2 };
        assert!(reusable(Some(&key), &key.clone()));
        assert!(!reusable(None, &key));
        assert!(!reusable(Some(&key), &ShotKey { frame: 8, ..key.clone() }));
        assert!(!reusable(Some(&key), &ShotKey { scene: 9, ..key.clone() }));
        assert!(!reusable(Some(&key), &ShotKey { decor: 9, ..key.clone() }));
    }

    #[test]
    fn el_hash_del_debug_es_estable_y_distingue_lo_que_cambia() {
        let decor = cap::Decor::default();
        assert_eq!(debug_hash(&decor), debug_hash(&decor.clone()));
        assert_ne!(debug_hash(&decor), debug_hash(&cap::Decor { crop: [0.1, 0.1, 0.5, 0.5], ..decor.clone() }));
        assert_ne!(debug_hash(&decor), debug_hash(&cap::Decor { cursor: Some((0.5, 0.5, 1.0)), ..decor.clone() }));
        assert_ne!(debug_hash(&decor), debug_hash(&cap::Decor { texts: vec![(0, 1.0)], ..decor.clone() }));
        let project = Project::default();
        let motion = Motion::new(&project, None, 10.0);
        let inputs = SceneInputs { project: &project, motion: &motion, cursor_drawn: false, cursor_px: 30.0, src: (1920, 1080) };
        assert_eq!(debug_hash(&inputs.scene_for((960, 540))), debug_hash(&inputs.scene_for((960, 540))));
        assert_ne!(debug_hash(&inputs.scene_for((960, 540))), debug_hash(&inputs.scene_for((704, 396))));
    }
}
