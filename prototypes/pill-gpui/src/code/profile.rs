//! El perfil de quien usa Atic Code, como `Profile.tsx` y `profile.ts` de
//! la referencia: un nombre y una foto, de esta máquina. Se guardan en
//! `code-profile.json` y la foto, ya recortada, en un PNG
//! `code-profile-<n>.png` junto a él (cada foto nueva tiene otro nombre: el
//! cargador de imágenes de GPUI guarda por ruta y no vería el cambio).
//!
//! La tarjeta (nombre, «Cambiar foto» y «Quitar») se abre desde el pie de la
//! barra; la foto elegida se encuadra con el `ImageCropper` de gpui-m3 en un
//! diálogo.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::{
    anchored, deferred, div, point, prelude::*, px, AnyElement, ClickEvent, Context, Corner, Entity, MouseButton, PathPromptOptions, Pixels, Point,
    RenderImage, SharedString, Window,
};
use gpui_m3::{Avatar, Button, Dialog, Exit, IconButton, ImageCropper, ImageCropperEvent, Popover, ShapeName};
use serde::{Deserialize, Serialize};

use super::style::t;
use super::CodeView;

const FILE: &str = "code-profile.json";
/// El lado de la foto guardada (px), como `OUT` de la referencia.
const OUT: u32 = 256;
/// El lado del visor de recorte (px).
const VIEW: f32 = 260.;
/// Una foto más grande que esto se reduce antes de encuadrarla.
const MAX_SIDE: u32 = 2048;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    /// El archivo de la foto dentro de la carpeta de datos.
    pub photo: Option<String>,
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
            git.unwrap_or("Tu nombre").to_string()
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

    /// El avatar del perfil: la galleta con su foto o sus iniciales, que se
    /// transforma en trébol al pasar el cursor.
    pub(super) fn profile_avatar(&self, id: &'static str, size: Pixels, cx: &mut Context<Self>) -> AnyElement {
        let name = self.profile_name();
        let scheme = *gpui_m3::Theme::of(cx);
        match self.profile.photo_path() {
            Some(path) => Avatar::image(path).name(name).size(size).breathe_on_hover(id, ShapeName::Clover8).into_any_element(),
            None => gpui_m3::Shape::new(ShapeName::Cookie9)
                .size(size)
                .color(scheme.tertiary_container)
                .breathe_on_hover(id, ShapeName::Clover8)
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
        self.profile_field.read(cx).focus(window);
        self.profile_card = Some(at);
        cx.notify();
    }

    /// La foto elegida con el diálogo del sistema: se lee, se decodifica en
    /// segundo plano y se abre el recorte.
    pub(super) fn pick_profile_photo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some("Foto de perfil".into()) });
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
                None => view.show_toast("No se pudo leer esa imagen", cx),
            });
        })
        .detach();
    }

    fn start_crop(&mut self, image: Arc<RenderImage>, window: &mut Window, cx: &mut Context<Self>) {
        let cropper = cx.new(|cx| ImageCropper::new(image, cx).view_size(px(VIEW)).output_size(OUT).shape(ShapeName::Cookie9));
        cx.subscribe(&cropper, |view, _, event: &ImageCropperEvent, cx| {
            match event {
                ImageCropperEvent::Cropped(image) => view.save_profile_photo(image, cx),
                ImageCropperEvent::Cancelled => {}
            }
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
        match encode_png(image).map(|png| self.profile.set_photo_in(&dir, &png)) {
            Some(Ok(())) => {}
            _ => self.show_toast("No se pudo guardar la foto", cx),
        }
    }

    pub(super) fn remove_profile_photo(&mut self, cx: &mut Context<Self>) {
        if let Some(dir) = crate::paths::data_dir() {
            self.profile.remove_photo_in(&dir);
            self.profile.save();
        }
        cx.notify();
    }

    /// El nombre se guarda mientras se escribe, como en la referencia.
    pub(super) fn profile_name_changed(&mut self, text: &str, cx: &mut Context<Self>) {
        self.profile.name = text.to_string();
        self.profile.save();
        cx.notify();
    }

    /// La tarjeta de perfil sobre el pie de la barra; sale animada.
    pub(super) fn profile_layer(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let shown = self.profile_last.show("profile-presence", self.profile_card, window, cx)?;
        let at = shown.value;
        let leaving = shown.leaving;
        let has_photo = self.profile.photo_path().is_some();
        let t = t();
        let card = Popover::new("profile-card")
            .width(px(300.))
            .padding(px(18.))
            .radius(px(28.))
            .gap(px(14.))
            .child(
                div().flex().justify_center().child(
                    div()
                        .id("profile-photo")
                        .cursor_pointer()
                        .tooltip(crate::hover::tip("Cambiar foto"))
                        .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.pick_profile_photo(window, cx)))
                        .child(self.profile_avatar("profile-card-avatar", px(84.), cx)),
                ),
            )
            .child(div().flex().flex_col().gap(px(4.)).child(div().px(px(4.)).text_size(px(12.)).text_color(t.muted).child("Nombre")).child(self.profile_field.clone()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        Button::new("profile-photo-pick", if has_photo { "Cambiar foto" } else { "Agregar foto" })
                            .tonal()
                            .icon("folder-open")
                            .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.pick_profile_photo(window, cx))),
                    )
                    .when(has_photo, |el| {
                        el.child(IconButton::new("profile-photo-remove", "trash").size(px(36.)).tooltip("Quitar foto").on_click(cx.listener(
                            |view, _: &ClickEvent, _, cx| view.remove_profile_photo(cx),
                        )))
                    }),
            );
        Some(
            deferred(
                div()
                    .absolute()
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
                                    .child(shown.wrap(Exit::Rise, card)),
                            ),
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    /// El diálogo para encuadrar la foto; sale animado.
    pub(super) fn crop_dialog(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let shown = self.cropper_last.show("crop-presence", self.cropper.clone(), window, cx)?;
        let cropper = shown.value.clone();
        let (save, cancel) = (cropper.clone(), cropper.clone());
        let hint: SharedString = "Arrastra para mover y usa la rueda para acercar.".into();
        Some(
            Dialog::new("profile-crop")
                .exit(shown.progress)
                .title("Encuadra tu foto")
                .width(px(VIEW + 48.))
                .on_dismiss(cx.listener(|view, _: &ClickEvent, _, cx| {
                    view.cropper = None;
                    cx.notify();
                }))
                .child(div().flex().justify_center().child(cropper))
                .child(div().text_size(px(12.)).text_color(t().muted).child(hint))
                .action(Button::new("crop-cancel", "Cancelar").text().on_click(move |_, _, cx| cancel.update(cx, |c, cx| c.cancel(cx))))
                .action(Button::new("crop-save", "Guardar").filled().on_click(move |_, _, cx| save.update(cx, |c, cx| c.confirm(cx))))
                .into_any_element(),
        )
    }
}

/// El campo del nombre de la tarjeta.
pub(super) fn name_field(cx: &mut Context<CodeView>) -> Entity<gpui_m3::TextField> {
    let field = cx.new(|cx| gpui_m3::TextField::new(cx).placeholder("Tu nombre"));
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
        let mut profile = Profile { name: "Ana Pérez".into(), photo: None };
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
