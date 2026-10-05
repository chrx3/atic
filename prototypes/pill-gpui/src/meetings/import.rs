//! Importar audio de afuera como reunión nueva: soltando archivos en la
//! ventana o con «Importar audio…».
//!
//! Cada archivo se convierte a `mic.wav` mono 16 kHz en su carpeta y queda en
//! la base como `recorded`, igual que una grabación propia: de ahí en adelante
//! se transcribe y resume con el mismo flujo. Portado de `import.rs` de Atic;
//! la conversión (`atic_transcribe::import_audio_to_wav`) va en un hilo.
//!
//! El velo de «Suelta para importar» se funde al entrar y al salir con
//! animaciones de una vez: quieto no pide cuadros.

use std::path::{Path, PathBuf};

use atic_core::{Db, Recording, RecordingStatus};
use chrono::{DateTime, Utc};
use gpui::{
    canvas, div, prelude::*, px, svg, Animation, AnimationExt, AnyElement, ClickEvent, Context,
    DispatchPhase, ElementId, ExternalPaths, FileDropEvent, FontWeight, PathPromptOptions,
};

use super::data::Paths;
use super::{hsla, MeetingsView, FAINT, MUTED, RED, R_ITEM, R_PANEL, SURFACE_ON, TEXT};
use crate::hover::{self, HoverExt};

/// Lo que se acepta (lo que decodifica `atic-transcribe` con las funciones de
/// symphonia que el prototipo enciende en su `Cargo.toml`).
pub(super) const EXTENSIONS: [&str; 5] = ["wav", "mp3", "m4a", "ogg", "flac"];
const KINDS: &str = "wav, mp3, m4a, ogg o flac";

pub(super) fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// El nombre del archivo sin extensión, si dice algo.
pub(super) fn title_for(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?.trim();
    (!stem.is_empty()).then(|| stem.to_string())
}

/// Cuándo fue la reunión: la fecha del archivo (lo más cercano que hay) o,
/// si no se sabe, ahora.
pub(super) fn started_at(path: &Path) -> DateTime<Utc> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map(DateTime::<Utc>::from)
        .unwrap_or_else(|_| Utc::now())
}

fn file_name(path: &Path) -> String {
    path.file_name().and_then(|n| n.to_str()).unwrap_or("el archivo").to_string()
}

/// Convierte `src` en una grabación nueva dentro de `recordings_dir` y la
/// guarda en `db`. Si algo falla no queda ni la carpeta ni la fila.
pub(super) fn import_into(src: &Path, recordings_dir: &Path, db: &Db) -> Result<Recording, String> {
    let name = file_name(src);
    if !src.is_file() {
        return Err(format!("No se encontró «{name}»."));
    }
    let mut rec = Recording::new(started_at(src));
    if let Some(title) = title_for(src) {
        rec.title = title;
    }
    let dir = recordings_dir.join(&rec.id);
    std::fs::create_dir_all(&dir).map_err(|e| format!("No se pudo crear la carpeta: {e}"))?;
    let undo = |error: String| {
        let _ = std::fs::remove_dir_all(&dir);
        error
    };
    let secs = atic_transcribe::import_audio_to_wav(src, &dir.join("mic.wav"))
        .map_err(|e| undo(format!("No se pudo importar «{name}»: {e}")))?;
    rec.duration_secs = secs.max(0);
    rec.mic_path = Some("mic.wav".into());
    rec.system_path = None;
    rec.status = RecordingStatus::Recorded;
    db.insert_recording(&rec)
        .map_err(|e| undo(format!("No se pudo guardar «{name}» en la lista: {e}")))?;
    Ok(rec)
}

fn import(paths: &Paths, src: &Path) -> Result<Recording, String> {
    let db = paths.db().map_err(|e| format!("No se pudo abrir la base: {e}"))?;
    import_into(src, &paths.recordings_dir(), &db)
}

// --- Estado ------------------------------------------------------------------------

/// Un archivo que se está convirtiendo.
struct Pending {
    key: u64,
    title: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Veil {
    #[default]
    Hidden,
    /// Hay archivos encima; `audio`: alguno se puede importar.
    Over { audio: bool },
    /// Se soltaron o se fueron: se está desvaneciendo.
    Leaving { audio: bool },
}

#[derive(Default)]
pub(super) struct Imports {
    pending: Vec<Pending>,
    error: Option<String>,
    next: u64,
    veil: Veil,
    /// Sube con cada entrada y salida: las animaciones y el temporizador de
    /// salida son de una pasada en particular.
    pass: u64,
}

impl Imports {
    pub(super) fn busy(&self) -> bool {
        !self.pending.is_empty()
    }
}

impl MeetingsView {
    fn import_paths(&self) -> Option<Paths> {
        self.source.as_ref().and_then(|s| s.paths()).cloned()
    }

    /// Los datos de prueba no se escriben: ahí no se importa.
    pub(super) fn can_import(&self) -> bool {
        self.import_paths().is_some()
    }

    /// Importa los archivos de a uno, en orden, fuera del hilo de UI.
    pub(super) fn import_files(&mut self, files: Vec<PathBuf>, cx: &mut Context<Self>) {
        let Some(paths) = self.import_paths() else {
            return;
        };
        let (audio, other): (Vec<_>, Vec<_>) = files.into_iter().partition(|p| is_audio(p));
        self.imports.error = other
            .first()
            .map(|p| format!("«{}» no es un audio ({KINDS}).", file_name(p)));
        if audio.is_empty() {
            cx.notify();
            return;
        }
        // Lo importado tiene que verse: una búsqueda a medias lo escondería.
        self.clear_search_text(cx);
        let mut jobs = Vec::with_capacity(audio.len());
        for file in audio {
            let key = self.imports.next;
            self.imports.next += 1;
            self.imports.pending.push(Pending {
                key,
                title: title_for(&file).unwrap_or_else(|| file_name(&file)),
            });
            jobs.push((key, file));
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            for (key, file) in jobs {
                let paths = paths.clone();
                let result = cx.background_spawn(async move { import(&paths, &file) }).await;
                if this.update(cx, |v, cx| v.import_done(key, result, cx)).is_err() {
                    return;
                }
            }
        })
        .detach();
    }

    fn import_done(&mut self, key: u64, result: Result<Recording, String>, cx: &mut Context<Self>) {
        self.imports.pending.retain(|p| p.key != key);
        match result {
            Ok(rec) => {
                self.reload(cx);
                // Se elige y, si así está configurado, se transcribe al tiro:
                // lo mismo que al terminar de grabar.
                self.after_recording(&rec.id, cx);
                if let Some(ix) = self.items.iter().position(|r| r.id == rec.id) {
                    self.reveal_row(ix, true);
                }
            }
            Err(error) => self.imports.error = Some(error),
        }
        cx.notify();
    }

    /// «Importar audio…»: el selector de archivos del sistema.
    pub(super) fn pick_audio(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Importar".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(files))) = picked.await else {
                return;
            };
            let _ = this.update(cx, |v, cx| v.import_files(files, cx));
        })
        .detach();
    }

    // --- Soltar archivos -----------------------------------------------------------

    /// Escucha archivos arrastrados sobre el cuerpo de la ventana.
    pub(super) fn drop_target(el: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        el.on_drag_move::<ExternalPaths>(cx.listener(|v, event: &gpui::DragMoveEvent<ExternalPaths>, _, cx| {
            if !v.can_import() || v.settings.is_some() {
                return;
            }
            let audio = event.drag(cx).paths().iter().any(|p| is_audio(p));
            v.veil_to(Veil::Over { audio }, cx);
        }))
        .on_drop(cx.listener(|v, paths: &ExternalPaths, _, cx| {
            if !v.can_import() || v.settings.is_some() {
                return;
            }
            v.veil_leave(cx);
            v.import_files(paths.paths().to_vec(), cx);
        }))
    }

    fn veil_to(&mut self, veil: Veil, cx: &mut Context<Self>) {
        if self.imports.veil != veil {
            self.imports.veil = veil;
            self.imports.pass += 1;
            cx.notify();
        }
    }

    fn veil_leave(&mut self, cx: &mut Context<Self>) {
        let Veil::Over { audio } = self.imports.veil else {
            return;
        };
        self.veil_to(Veil::Leaving { audio }, cx);
        let pass = self.imports.pass;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(hover::LEAVE).await;
            let _ = this.update(cx, |v, cx| {
                if v.imports.pass == pass {
                    v.imports.veil = Veil::Hidden;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// El velo sobre la lista mientras hay archivos encima. Lleva también el
    /// oído para cuando se van sin soltar (GPUI solo avisa con un
    /// `FileDropEvent::Exited` a nivel de ventana).
    pub(super) fn drop_veil(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.can_import() {
            return None;
        }
        let view = cx.entity().downgrade();
        let ear = canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                window.on_mouse_event(move |event: &FileDropEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble && matches!(event, FileDropEvent::Exited) {
                        let _ = view.update(cx, |v, cx| v.veil_leave(cx));
                    }
                });
            },
        )
        .absolute()
        .size_0();
        let (audio, entering) = match self.imports.veil {
            Veil::Hidden => return Some(ear.into_any_element()),
            Veil::Over { audio } => (audio, true),
            Veil::Leaving { audio } => (audio, false),
        };
        let (title, hint) = if audio {
            ("Suelta para importar", "Cada archivo, una reunión nueva")
        } else {
            ("Eso no es audio", "Se importan solo estos:")
        };
        let duration = if entering { hover::ENTER } else { hover::LEAVE };
        let veil = div()
            .absolute()
            .inset_0()
            .rounded(px(R_PANEL))
            .bg(hsla(SURFACE_ON).opacity(0.96))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .child(
                svg()
                    .path("icons/import.svg")
                    .size(px(22.))
                    .mb(px(4.))
                    .text_color(hsla(if audio { TEXT } else { MUTED })),
            )
            .child(div().text_size(px(14.)).font_weight(FontWeight::MEDIUM).child(title))
            .child(div().text_size(px(12.)).text_color(hsla(MUTED)).child(hint))
            .child(div().text_size(px(12.)).text_color(hsla(FAINT)).child(KINDS))
            .child(ear)
            .with_animation(
                ElementId::NamedInteger("import-veil".into(), self.imports.pass),
                Animation::new(duration).with_easing(gpui::ease_out_quint()),
                move |el, t| el.opacity(if entering { t } else { 1.0 - t }),
            );
        Some(veil.into_any_element())
    }

    // --- Filas -------------------------------------------------------------------

    /// Lo que se está importando y el último error, arriba de la lista.
    pub(super) fn import_rows(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.imports.pending.is_empty() && self.imports.error.is_none() {
            return None;
        }
        let mut column = div().flex_none().px(px(8.)).pt(px(8.)).flex().flex_col().gap(px(2.));
        for pending in &self.imports.pending {
            column = column.child(
                div()
                    .px(px(12.))
                    .py(px(9.))
                    .rounded(px(R_ITEM))
                    .bg(hsla(SURFACE_ON).opacity(0.5))
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(
                        div()
                            .truncate()
                            .text_size(px(13.))
                            .font_weight(FontWeight::MEDIUM)
                            .child(pending.title.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(12.))
                            .text_color(hsla(MUTED))
                            .child("Importando…")
                            .child(div().flex_1())
                            .child(super::actions::pulse_dot("import-pulse", super::AMBER)),
                    ),
            );
        }
        if let Some(error) = &self.imports.error {
            column = column.child(
                div()
                    .pl(px(12.))
                    .pr(px(4.))
                    .py(px(4.))
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .pt(px(5.))
                            .text_size(px(12.))
                            .line_height(px(17.))
                            .text_color(hsla(RED))
                            .child(error.clone()),
                    )
                    .child(hover::round_button(
                        "import-error-close",
                        "icons/x.svg",
                        "Cerrar",
                        false,
                        hsla(TEXT),
                        hsla(FAINT),
                        cx.listener(|v, _: &ClickEvent, _, cx| {
                            v.imports.error = None;
                            cx.notify();
                        }),
                    )),
            );
        }
        Some(column.into_any_element())
    }

    /// «Importar audio…» bajo la invitación de la lista vacía.
    pub(super) fn import_link(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.can_import() {
            return None;
        }
        Some(
            div()
                .flex_none()
                .pb(px(18.))
                .flex()
                .justify_center()
                .child(
                    div()
                        .id("import-link")
                        .h(px(28.))
                        .px(px(12.))
                        .flex()
                        .items_center()
                        .gap(px(7.))
                        .rounded(px(14.))
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .cursor_pointer()
                        .on_click(cx.listener(|v, _: &ClickEvent, _, cx| v.pick_audio(cx)))
                        .child(svg().path("icons/import.svg").size(px(13.)).text_color(hsla(MUTED)))
                        .child("Importar audio…")
                        .fx("import-link", |el, h| {
                            el.bg(h.mix(hsla(TEXT).opacity(0.0), hsla(TEXT).opacity(0.08)))
                                .text_color(h.mix(hsla(MUTED), hsla(TEXT)))
                        }),
                )
                .into_any_element(),
        )
    }

    /// El botón redondo junto al buscador.
    pub(super) fn import_button(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        self.can_import().then(|| {
            hover::round_button(
                "meetings-import",
                "icons/import.svg",
                "Importar audio…",
                false,
                hsla(TEXT),
                hsla(MUTED),
                cx.listener(|v, _: &ClickEvent, _, cx| v.pick_audio(cx)),
            )
            .into_any_element()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("atic-import-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_tone(path: &Path, secs: u32) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for n in 0..16_000 * secs {
            let s = (n as f32 * 440.0 * std::f32::consts::TAU / 16_000.0).sin();
            writer.write_sample((s * 8_000.0) as i16).unwrap();
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn reconoce_audio_por_extension() {
        assert!(is_audio(Path::new("a/Reunión.MP3")));
        assert!(is_audio(Path::new("x.flac")));
        assert!(is_audio(Path::new("x.m4a")));
        assert!(!is_audio(Path::new("x.mp4")));
        assert!(!is_audio(Path::new("notas.txt")));
        assert!(!is_audio(Path::new("sin_extension")));
    }

    #[test]
    fn el_titulo_es_el_nombre_sin_extension() {
        assert_eq!(title_for(Path::new("C:/x/Llamada con Ana.m4a")).as_deref(), Some("Llamada con Ana"));
        assert_eq!(title_for(Path::new("x/   .mp3")), None);
    }

    #[test]
    fn importa_un_wav_como_reunion_nueva() {
        let dir = scratch("ok");
        let src = dir.join("Llamada de prueba.wav");
        write_tone(&src, 2);
        let recordings = dir.join("recordings");
        let db = Db::open(&dir.join("atic.db3")).unwrap();

        let rec = import_into(&src, &recordings, &db).unwrap();
        assert_eq!(rec.title, "Llamada de prueba");
        assert_eq!(rec.duration_secs, 2);
        assert_eq!(rec.status, RecordingStatus::Recorded);
        assert_eq!(rec.mic_path.as_deref(), Some("mic.wav"));
        assert!(recordings.join(&rec.id).join("mic.wav").is_file());
        // La fecha es la del archivo, no la de hoy a la fuerza.
        assert_eq!(rec.started_at, started_at(&src));

        let listed = db.list_recordings().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, rec.id);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn un_archivo_roto_no_deja_rastro() {
        let dir = scratch("roto");
        let src = dir.join("roto.mp3");
        std::fs::write(&src, b"esto no es un mp3").unwrap();
        let recordings = dir.join("recordings");
        let db = Db::open(&dir.join("atic.db3")).unwrap();

        let error = import_into(&src, &recordings, &db).unwrap_err();
        assert!(error.contains("roto.mp3"), "{error}");
        assert!(db.list_recordings().unwrap().is_empty());
        let left = std::fs::read_dir(&recordings).map(|d| d.count()).unwrap_or(0);
        assert_eq!(left, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
