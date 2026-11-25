//! El perfil de quien usa Atic Code, como `Profile.tsx` y `profile.ts` de
//! la referencia: un nombre y una foto, de esta máquina. Se guardan en
//! `code-profile.json` y la foto, ya recortada, en un PNG
//! `code-profile-<n>.png` junto a él (cada foto nueva tiene otro nombre: el
//! cargador de imágenes de GPUI guarda por ruta y no vería el cambio).
//!
//! La tarjeta (nombre con contador y estado de guardado, «Cambiar foto» y
//! «Quitar» con deshacer) se abre desde el pie de la barra; la foto elegida
//! se encuadra con el `ImageCropper` de gpui-m3 en un diálogo.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{
    anchored, deferred, div, point, prelude::*, px, AnyElement, ClickEvent, Context, Corner, Entity, MouseButton, PathPromptOptions, Pixels, Point,
    RenderImage, SharedString, Window,
};
use gpui_m3::{Avatar, Button, ButtonSize, Dialog, IconButton, ImageCropper, ImageCropperEvent, Kbd, Popover, ShapeName};
use serde::{Deserialize, Serialize};

use super::style::t;
use crate::i18n::t as tr;
use super::CodeView;

const FILE: &str = "code-profile.json";
/// El lado de la foto guardada (px), como `OUT` de la referencia.
const OUT: u32 = 256;
/// El lado del visor de recorte (px).
const VIEW: f32 = 260.;
/// Una foto más grande que esto se reduce antes de encuadrarla.
const MAX_SIDE: u32 = 2048;
/// Cuánto espera el nombre tras la última tecla antes de guardarse (ms).
const SAVE_MS: u64 = 600;
/// Cuánto se puede deshacer el quitar la foto (ms): el archivo solo se borra
/// al vencer el plazo o al elegir otra foto.
const UNDO_MS: u64 = 8000;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    /// El archivo de la foto dentro de la carpeta de datos.
    pub photo: Option<String>,
    /// Cuántas veces se editó el nombre (en memoria): el guardado a disco va
    /// con 600 ms de retraso para no escribir en cada tecla.
    #[serde(skip)]
    name_gen: u64,
    /// La generación que ya quedó guardada: si coincide con `name_gen`, el
    /// nombre está al día y se muestra «Guardado».
    #[serde(skip)]
    name_flushed_gen: u64,
    /// La foto quitada hace poco, con sus bytes en memoria: se borra de disco
    /// cuando vence el plazo (u otra foto la reemplaza), salvo deshacer.
    #[serde(skip)]
    removed: Option<RemovedPhoto>,
}

/// Una foto quitada que aún se puede deshacer: su nombre de archivo, sus bytes
/// y la generación del aviso (para que el temporizador no pise un aviso nuevo).
#[derive(Clone, Debug, PartialEq)]
struct RemovedPhoto {
    file: String,
    bytes: Vec<u8>,
    gen: u64,
}

impl Profile {
    pub fn load() -> Self {
        crate::paths::data_dir().map(|dir| Self::load_in(&dir)).unwrap_or_default()
    }

    pub fn load_in(dir: &Path) -> Self {
        std::fs::read_to_string(dir.join(FILE)).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        if let Some(dir) = crate::paths::data_dir() {
            let _ = self.save_in(&dir);
        }
    }

    pub fn save_in(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join(FILE), serde_json::to_string_pretty(self).unwrap_or_default())
    }

    /// El nombre que se ve: el guardado o, si no hay, el de git (el punto de
    /// partida de la referencia).
    pub fn shown_name(&self, git: Option<&str>) -> String {
        let name = self.name.trim();
        if !name.is_empty() {
            name.to_string()
        } else {
            git.unwrap_or(tr("pill.code.profile.yourName")).to_string()
        }
    }

    /// La ruta de la foto, si hay una y existe.
    pub fn photo_in(&self, dir: &Path) -> Option<PathBuf> {
        let path = dir.join(self.photo.as_deref()?);
        path.is_file().then_some(path)
    }

    pub fn photo_path(&self) -> Option<PathBuf> {
        crate::paths::data_dir().and_then(|dir| self.photo_in(&dir))
    }

    /// Guarda `png` como la foto nueva (con otro nombre que la anterior, que se borra).
    pub fn set_photo_in(&mut self, dir: &Path, png: &[u8]) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        let file = format!("code-profile-{stamp}.png");
        std::fs::write(dir.join(&file), png)?;
        self.remove_photo_in(dir);
        self.photo = Some(file);
        self.save_in(dir)
    }

    /// Quita la foto (y su archivo).
    pub fn remove_photo_in(&mut self, dir: &Path) {
        if let Some(file) = self.photo.take() {
            let _ = std::fs::remove_file(dir.join(file));
        }
    }

    /// Saca la foto quitada pendiente solo si sigue siendo la de `gen` (el
    /// temporizador de un aviso viejo no toca al nuevo).
    fn take_removed(&mut self, gen: u64) -> Option<RemovedPhoto> {
        if self.removed.as_ref().is_some_and(|pending| pending.gen == gen) {
            self.removed.take()
        } else {
            None
        }
    }
}

/// Decodifica una foto para encuadrarla (reducida si es enorme). GPUI guarda
/// los píxeles en orden BGRA.
pub fn decode_photo(bytes: &[u8]) -> Option<Arc<RenderImage>> {
    let mut image = image::load_from_memory(bytes).ok()?;
    if image.width().max(image.height()) > MAX_SIDE {
        image = image.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Triangle);
    }
    let mut rgba = image.to_rgba8();
    for pixel in rgba.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Some(Arc::new(RenderImage::new([image::Frame::new(rgba)])))
}

/// El recorte como PNG (el inverso de [`decode_photo`]).
pub fn encode_png(image: &RenderImage) -> Option<Vec<u8>> {
    let size = image.size(0);
    let (width, height) = (size.width.0 as u32, size.height.0 as u32);
    let mut bytes = image.as_bytes(0)?.to_vec();
    if bytes.len() < (width * height * 4) as usize {
        return None;
    }
    for pixel in bytes.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let buffer = image::RgbaImage::from_raw(width, height, bytes)?;
    let mut out = Cursor::new(Vec::new());
    buffer.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

impl CodeView {
    /// El nombre del perfil (o el de git mientras no se haya escrito uno).
    pub(super) fn profile_name(&self) -> String {
        self.profile.shown_name(self.user_name.as_deref())
    }

    /// El avatar del perfil: la galleta con su foto o sus iniciales. La cookie
    /// es la única forma del perfil, sin morph al pasar el cursor.
    pub(super) fn profile_avatar(&self, _id: &'static str, size: Pixels, cx: &mut Context<Self>) -> AnyElement {
        let name = self.profile_name();
        let scheme = *gpui_m3::Theme::of(cx);
        match self.profile.photo_path() {
            Some(path) => Avatar::image(path).name(name).size(size).into_any_element(),
            None => gpui_m3::Shape::new(ShapeName::Cookie9)
                .size(size)
                .color(scheme.tertiary_container)
                .child(
                    div()
                        .text_size(px((f32::from(size) * 0.38).max(11.)))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(scheme.on_tertiary_container)
                        .child(super::sidebar::initials(&name)),
                )
                .into_any_element(),
        }
    }

    pub(super) fn toggle_profile(&mut self, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        if self.was_dismissed(super::Dismissed::Profile) {
            return;
        }
        if self.profile_card.take().is_some() {
            cx.notify();
            return;
        }
        let name = self.profile_name();
        self.profile_field.update(cx, |field, cx| field.set_text(name, cx));
        self.sync_name_supporting(cx);
        self.profile_field.read(cx).focus(window);
        self.profile_card = Some(at);
        cx.notify();
    }

    /// La foto elegida con el diálogo del sistema: se lee, se decodifica en
    /// segundo plano y se abre el recorte.
    pub(super) fn pick_profile_photo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some(tr("pill.code.profile.photoPrompt").into()) });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let decoded = cx.background_executor().spawn(async move { std::fs::read(&path).ok().and_then(|bytes| decode_photo(&bytes)) }).await;
            let _ = this.update_in(cx, |view, window, cx| match decoded {
                Some(image) => view.start_crop(image, window, cx),
                None => view.show_toast(tr("pill.code.profile.readFailed"), cx),
            });
        })
        .detach();
    }

    fn start_crop(&mut self, image: Arc<RenderImage>, window: &mut Window, cx: &mut Context<Self>) {
        // La foto entera (hasta 2048 px) sale de la GPU al terminar de recortar.
        let source = image.clone();
        let cropper = cx.new(|cx| ImageCropper::new(image, cx).view_size(px(VIEW)).output_size(OUT).shape(ShapeName::Cookie9));
        cx.subscribe(&cropper, move |view, _, event: &ImageCropperEvent, cx| {
            match event {
                ImageCropperEvent::Cropped(image) => view.save_profile_photo(image, cx),
                ImageCropperEvent::Cancelled => {}
            }
            crate::release::image(source.clone());
            view.cropper = None;
            cx.notify();
        })
        .detach();
        cropper.read(cx).focus(window);
        self.cropper = Some(cropper);
        cx.notify();
    }

    fn save_profile_photo(&mut self, image: &RenderImage, cx: &mut Context<Self>) {
        let Some(dir) = crate::paths::data_dir() else {
            return;
        };
        // La foto nueva reemplaza a la quitada pendiente: su archivo se borra
        // ahora y el aviso de deshacer desaparece.
        self.discard_removed_photo(&dir);
        match encode_png(image).map(|png| self.profile.set_photo_in(&dir, &png)) {
            Some(Ok(())) => {}
            _ => self.show_toast(tr("pill.code.profile.saveFailed"), cx),
        }
    }

    /// Quita la foto dejando sus bytes en memoria unos segundos, por si se
    /// deshace. El archivo solo se borra al vencer el plazo o al elegir otra.
    pub(super) fn remove_profile_photo(&mut self, cx: &mut Context<Self>) {
        let Some(dir) = crate::paths::data_dir() else {
            return;
        };
        let Some(file) = self.profile.photo.clone() else {
            return;
        };
        let Ok(bytes) = std::fs::read(dir.join(&file)) else {
            self.profile.photo = None;
            self.profile.save();
            cx.notify();
            return;
        };
        self.profile.photo = None;
        self.profile.save();
        // La generación ata el temporizador a este aviso: quitar otra foto
        // después reinicia el plazo en vez de borrar la nueva.
        let gen = self.profile.removed.as_ref().map_or(0, |pending| pending.gen).wrapping_add(1);
        self.profile.removed = Some(RemovedPhoto { file, bytes, gen });
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(UNDO_MS)).await;
            let _ = this.update(cx, |view, cx| view.expire_removed_photo(gen, cx));
        })
        .detach();
    }

    /// Devuelve la foto quitada pendiente (si el archivo falta, lo reescribe
    /// con los bytes guardados) y la deja como estaba.
    pub(super) fn undo_remove_profile_photo(&mut self, cx: &mut Context<Self>) {
        let Some(dir) = crate::paths::data_dir() else {
            return;
        };
        let Some(pending) = self.profile.removed.take() else {
            return;
        };
        let path = dir.join(&pending.file);
        if !path.is_file() {
            let _ = std::fs::write(&path, &pending.bytes);
        }
        self.profile.photo = Some(pending.file);
        self.profile.save();
        cx.notify();
    }

    /// Borra de disco la foto quitada pendiente, si la hay (al vencer el plazo
    /// o al reemplazarla con otra foto).
    fn discard_removed_photo(&mut self, dir: &Path) {
        if let Some(pending) = self.profile.removed.take() {
            let _ = std::fs::remove_file(dir.join(pending.file));
        }
    }

    /// Vence el aviso de deshacer: borra el archivo solo si el aviso vigente
    /// sigue siendo el de `gen`.
    fn expire_removed_photo(&mut self, gen: u64, cx: &mut Context<Self>) {
        let Some(pending) = self.profile.take_removed(gen) else {
            return;
        };
        if let Some(dir) = crate::paths::data_dir() {
            let _ = std::fs::remove_file(dir.join(pending.file));
        }
        cx.notify();
    }

    /// El nombre se guarda 600 ms después de dejar de escribir: mientras, el
    /// campo muestra «Editando…» y al guardar, «Guardado».
    pub(super) fn profile_name_changed(&mut self, text: &str, cx: &mut Context<Self>) {
        if text == self.profile.name && self.profile.name_gen == self.profile.name_flushed_gen {
            return;
        }
        self.profile.name = text.to_string();
        self.profile.name_gen = self.profile.name_gen.wrapping_add(1);
        let gen = self.profile.name_gen;
        self.sync_name_supporting(cx);
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(SAVE_MS)).await;
            let _ = this.update(cx, |view, cx| {
                if view.profile.name_gen == gen {
                    view.profile.save();
                    view.profile.name_flushed_gen = gen;
                    view.sync_name_supporting(cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// El estado bajo el campo del nombre: «Editando…» o «Guardado», junto al
    /// contador de caracteres que ya pone el campo.
    fn sync_name_supporting(&self, cx: &mut Context<Self>) {
        let saved = self.profile.name_gen == self.profile.name_flushed_gen;
        let text: SharedString = if saved { tr("pill.code.profile.saved") } else { tr("pill.code.profile.editing") }.into();
        self.profile_field.update(cx, |field, cx| field.set_supporting_text(Some(text), cx));
    }

    /// La tarjeta de perfil sobre el pie de la barra; sale animada.
    pub(super) fn profile_layer(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let shown = self.profile_last.show("profile-presence", self.profile_card, window, cx)?;
        let at = shown.value;
        let leaving = shown.leaving;
        let has_photo = self.profile.photo_path().is_some();
        let scheme = *gpui_m3::Theme::of(cx);
        let t = t();
        // El botón del lápiz dice qué foto cambia: la que hay o agregar una.
        let edit_tip: SharedString = if has_photo { tr("pill.code.profile.changePhoto") } else { tr("pill.code.profile.addPhoto") }.into();
        let mut card = Popover::new("profile-card")
            .origin(Corner::BottomLeft)
            .width(px(300.))
            .padding(px(18.))
            .radius(px(28.))
            .gap(px(14.))
            .child(
                div().flex().justify_center().child(
                    div()
                        .relative()
                        .child(self.profile_avatar("profile-card-avatar", px(96.), cx))
                        .child(
                            div().absolute().bottom(px(-4.)).right(px(-4.)).child(
                                IconButton::new("profile-photo-edit", "pen")
                                    .filled()
                                    .size(px(40.))
                                    .tooltip(edit_tip)
                                    .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.pick_profile_photo(window, cx))),
                            ),
                        ),
                ),
            )
            .child(div().flex().flex_col().gap(px(4.)).child(div().px(px(4.)).text_size(px(12.)).text_color(t.muted).child(tr("pill.code.profile.name"))).child(self.profile_field.clone()));
        if self.profile.removed.is_some() {
            card = card.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(14.))
                    .bg(scheme.secondary_container)
                    .text_color(scheme.on_secondary_container)
                    .pl(px(14.))
                    .pr(px(8.))
                    .py(px(4.))
                    .min_h(px(44.))
                    .child(div().flex_1().child(tr("pill.code.profile.photoRemoved")))
                    .child(
                        Button::new("profile-photo-undo", tr("pill.code.sidebar.undo"))
                            .text()
                            .size(ButtonSize::Comfortable)
                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.undo_remove_profile_photo(cx))),
                    ),
            );
        }
        let card = card.child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(
                    Button::new("profile-photo-pick", if has_photo { tr("pill.code.profile.changePhoto") } else { tr("pill.code.profile.addPhoto") })
                        .tonal()
                        .size(ButtonSize::Comfortable)
                        .icon("folder-open")
                        .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.pick_profile_photo(window, cx))),
                )
                .when(has_photo, |el| {
                    el.child(
                        Button::new("profile-photo-remove", tr("pill.code.profile.remove"))
                            .text()
                            .size(ButtonSize::Comfortable)
                            .icon("trash")
                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.remove_profile_photo(cx))),
                    )
                }),
        );
        Some(
            deferred(
                div()
                    .absolute()
                    .occlude()
                    .inset_0()
                    .when(!leaving, |el| {
                        el.on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|view, _, _, cx| {
                                view.profile_card = None;
                                view.note_dismiss(super::Dismissed::Profile);
                                cx.notify();
                            }),
                        )
                    })
                    .child(
                        anchored()
                            .position(point(at.x - px(24.), at.y - px(22.)))
                            .anchor(Corner::BottomLeft)
                            .snap_to_window_with_margin(px(8.))
                            .child(
                                div()
                                    .id("profile-card-wrap")
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                    .child(shown.shrink(Corner::BottomLeft, card)),
                            ),
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// El diálogo para encuadrar la foto; sale animado. El `ImageCropper` ya
    /// trae el zoom visible (deslizador), el arrastre y la máscara con la forma
    /// final; aquí solo se explica y se muestran los atajos.
    pub(super) fn crop_dialog(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let shown = self.cropper_last.show("crop-presence", self.cropper.clone(), window, cx)?;
        let cropper = shown.value.clone();
        let (save, cancel) = (cropper.clone(), cropper.clone());
        let muted = t().muted;
        Some(
            Dialog::new("profile-crop")
                .exit(shown.progress)
                .title(tr("pill.code.profile.cropTitle"))
                .width(px(VIEW + 48.))
                .on_dismiss(cx.listener(|view, _: &ClickEvent, _, cx| {
                    view.cropper = None;
                    cx.notify();
                }))
                .child(div().flex().justify_center().child(cropper))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(6.))
                        .child(div().text_size(px(12.)).text_color(muted).child(tr("pill.code.profile.cropHelp")))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .text_size(px(12.))
                                .text_color(muted)
                                .child(Kbd::new("Enter").small(true))
                                .child(div().child(tr("pill.code.profile.save")))
                                .child(div().child("·"))
                                .child(Kbd::new("Esc").small(true))
                                .child(div().child(tr("pill.settings.cancel"))),
                        ),
                )
                .action(Button::new("crop-cancel", tr("pill.settings.cancel")).text().on_click(move |_, _, cx| cancel.update(cx, |c, cx| c.cancel(cx))))
                .action(Button::new("crop-save", tr("pill.code.profile.save")).filled().on_click(move |_, _, cx| save.update(cx, |c, cx| c.confirm(cx))))
                .into_any_element(),
        )
    }
}

/// El campo del nombre de la tarjeta: con contador (máximo 40, solo avisa) y
/// una línea de ayuda donde [`CodeView::sync_name_supporting`] muestra el
/// estado del guardado agrupado.
pub(super) fn name_field(cx: &mut Context<CodeView>) -> Entity<gpui_m3::TextField> {
    let field = cx.new(|cx| gpui_m3::TextField::new(cx).placeholder(tr("pill.code.profile.yourName")).max_length(40));
    cx.subscribe(&field, |view: &mut CodeView, _, event: &gpui_m3::TextFieldEvent, cx| match event {
        gpui_m3::TextFieldEvent::Changed(text) => view.profile_name_changed(text, cx),
        gpui_m3::TextFieldEvent::Submitted(_) | gpui_m3::TextFieldEvent::Cancelled => {
            view.profile_card = None;
            cx.notify();
        }
    })
    .detach();
    field
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("atic-profile-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn el_perfil_se_guarda_y_se_vuelve_a_leer() {
        let dir = temp("save");
        assert_eq!(Profile::load_in(&dir), Profile::default());
        let mut profile = Profile { name: "Ana Pérez".into(), photo: None, ..Default::default() };
        profile.save_in(&dir).unwrap();
        assert_eq!(Profile::load_in(&dir), profile);
        // Un archivo dañado no rompe: queda el perfil vacío.
        std::fs::write(dir.join(FILE), "{ no es json").unwrap();
        assert_eq!(Profile::load_in(&dir), Profile::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn la_foto_nueva_reemplaza_a_la_vieja() {
        let dir = temp("photo");
        let mut profile = Profile::default();
        profile.set_photo_in(&dir, b"uno").unwrap();
        let first = profile.photo.clone().unwrap();
        assert_eq!(profile.photo_in(&dir).map(|p| std::fs::read(p).unwrap()), Some(b"uno".to_vec()));
        std::thread::sleep(std::time::Duration::from_millis(3));
        profile.set_photo_in(&dir, b"dos").unwrap();
        assert_ne!(profile.photo.as_deref(), Some(first.as_str()), "otra ruta, para que GPUI no use la foto guardada en caché");
        assert!(!dir.join(&first).exists());
        // Lo guardado en disco apunta a la nueva.
        assert_eq!(Profile::load_in(&dir), profile);
        profile.remove_photo_in(&dir);
        profile.save_in(&dir).unwrap();
        assert_eq!(profile.photo_in(&dir), None);
        assert_eq!(Profile::load_in(&dir).photo, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sin_nombre_se_ve_el_de_git() {
        let mut profile = Profile::default();
        assert_eq!(profile.shown_name(Some("chrx3")), "chrx3");
        assert_eq!(profile.shown_name(None), "Tu nombre");
        profile.name = "  Ana ".into();
        assert_eq!(profile.shown_name(Some("chrx3")), "Ana");
    }

    #[test]
    fn el_deshacer_de_la_foto_respeta_la_generacion() {
        let mut profile = Profile::default();
        profile.removed = Some(RemovedPhoto { file: "vieja.png".into(), bytes: vec![1, 2], gen: 3 });
        // Un temporizador viejo no toca el aviso nuevo.
        assert!(profile.take_removed(2).is_none());
        assert!(profile.removed.is_some());
        let pending = profile.take_removed(3).expect("la generación vigente sí sale");
        assert_eq!(pending.file, "vieja.png");
        assert_eq!(pending.bytes, vec![1, 2]);
        assert!(profile.removed.is_none());
    }

    #[test]
    fn los_campos_efimeros_no_quedan_en_disco() {
        let dir = temp("ephemeral");
        let mut profile = Profile { name: "Ana".into(), photo: None, ..Default::default() };
        profile.name_gen = 7;
        profile.name_flushed_gen = 3;
        profile.removed = Some(RemovedPhoto { file: "x.png".into(), bytes: vec![9], gen: 1 });
        profile.save_in(&dir).unwrap();
        let loaded = Profile::load_in(&dir);
        assert_eq!(loaded.name, "Ana");
        assert_eq!((loaded.name_gen, loaded.name_flushed_gen), (0, 0));
        assert_eq!(loaded.removed, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn la_foto_va_y_vuelve_por_png_sin_cambiar_los_colores() {
        // Un píxel rojo, uno verde, uno azul y uno translúcido (RGBA).
        let mut source = image::RgbaImage::new(2, 2);
        source.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        source.put_pixel(1, 0, image::Rgba([0, 255, 0, 255]));
        source.put_pixel(0, 1, image::Rgba([0, 0, 255, 255]));
        source.put_pixel(1, 1, image::Rgba([10, 20, 30, 128]));
        let mut png = Cursor::new(Vec::new());
        source.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let render = decode_photo(&png.into_inner()).unwrap();
        // GPUI guarda BGRA: el rojo queda con el azul al final.
        assert_eq!(&render.as_bytes(0).unwrap()[..4], &[0, 0, 255, 255]);
        let back = encode_png(&render).unwrap();
        let again = image::load_from_memory(&back).unwrap().to_rgba8();
        assert_eq!(again, source);
    }
}
