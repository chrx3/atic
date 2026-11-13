//! El borrador de cada conversación: lo escrito en la caja (texto, cursor y adjuntos) se
//! queda con su conversación al cambiar a otra y vuelve al volver a ella. Se guarda en esta
//! máquina, en `code-drafts.json` junto a `code-marks.json`, con las imágenes como archivos
//! en `code-drafts/` (no en base64 dentro del JSON): así sigue ahí después de cerrar Atic.
//! Un borrador que no se edita en `MAX_AGE` se borra, con sus imágenes.
//!
//! La clave de un borrador es la sesión de la conversación (la misma de sus marcas, ver
//! `Chat::flag_session`). Antes de su primera sesión es la clave de la conversación, que no
//! sobrevive a reiniciar la pill: esos quedan solo en memoria y pasan a la sesión cuando
//! llega. La conversación nueva sin empezar de cada espacio tiene el suyo (`new_slot`).

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use gpui::Context;
use serde::{Deserialize, Serialize};

use super::{chat, Attachment, CodeView};

/// Un borrador que no se edita en este tiempo se borra (al abrir Atic Code o al guardar).
pub const MAX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);
/// Cuánto se espera sin cambios en la caja antes de escribir a disco.
const SAVE_DELAY: Duration = Duration::from_millis(1200);
const FILE: &str = "code-drafts.json";
/// La carpeta de las imágenes de los borradores, junto a `FILE`.
const IMAGES: &str = "code-drafts";

/// Lo que hay en la caja: texto, dónde estaba el cursor (byte) y los adjuntos.
#[derive(Clone, Default)]
pub struct Content {
    pub text: String,
    pub cursor: usize,
    pub attachments: Vec<Attachment>,
}

impl Content {
    fn is_empty(&self) -> bool {
        self.text.trim().is_empty() && self.attachments.is_empty()
    }

    /// El mismo texto y los mismos adjuntos (el cursor no cuenta como edición).
    fn same_as(&self, other: &Content) -> bool {
        self.text == other.text
            && self.attachments.len() == other.attachments.len()
            && self.attachments.iter().zip(&other.attachments).all(|pair| match pair {
                (Attachment::Image { name: a, data: x, .. }, Attachment::Image { name: b, data: y, .. }) => a == b && x == y,
                (Attachment::File(a), Attachment::File(b)) => a == b,
                (Attachment::Pasted { text: a, .. }, Attachment::Pasted { text: b, .. }) => a == b,
                _ => false,
            })
    }
}

struct Draft {
    content: Content,
    /// Cuándo se editó por última vez (segundos desde 1970).
    edited: u64,
}

/// A quién pertenece lo que está en la caja: una conversación (por su clave) o la nueva sin
/// empezar del espacio a la vista.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Owner {
    Chat(String),
    New(Option<u64>),
}

/// La clave del borrador de la conversación nueva de un espacio (o sin espacio).
pub fn new_slot(workspace: Option<u64>) -> String {
    match workspace {
        Some(id) => format!("new:{id}"),
        None => "new".into(),
    }
}

/// ¿Pasó más de `MAX_AGE` desde `edited`?
fn is_expired(edited: u64, now: u64) -> bool {
    now.saturating_sub(edited) > MAX_AGE.as_secs()
}

#[derive(Default)]
pub struct Drafts {
    slots: HashMap<String, Draft>,
    /// Las claves que son de una conversación sin sesión todavía: no se guardan en disco.
    transient: HashSet<String>,
    /// Dónde se guardan; `None`, solo en memoria (pruebas y ventanas de captura).
    dir: Option<PathBuf>,
    /// Hay algo que todavía no se escribió.
    dirty: bool,
}

impl Drafts {
    pub fn load() -> Self {
        // Una ventana abierta para capturas no lee ni guarda nada (como `stash.rs`).
        if std::env::var_os("CODE_OPEN").is_some() {
            return Self::default();
        }
        crate::paths::data_dir().map_or_else(Self::default, |dir| Self::load_from(dir, super::marks::now_secs()))
    }

    /// Lee los borradores de `dir`; los vencidos se borran, con sus imágenes.
    fn load_from(dir: PathBuf, now: u64) -> Self {
        let file: DiskFile = std::fs::read_to_string(dir.join(FILE)).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
        let images = dir.join(IMAGES);
        let mut slots = HashMap::new();
        let mut dropped = false;
        for (id, disk) in file.drafts {
            if is_expired(disk.edited, now) {
                dropped = true;
                continue;
            }
            let edited = disk.edited;
            let content = disk.content(&images);
            if content.is_empty() {
                dropped = true;
                continue;
            }
            slots.insert(id, Draft { content, edited });
        }
        let mut drafts = Self { slots, transient: HashSet::new(), dir: Some(dir), dirty: dropped };
        // Escribir también limpia las imágenes que ya no usa nadie.
        drafts.write(now);
        drafts
    }

    /// El borrador de `id`, si hay.
    pub fn get(&self, id: &str) -> Option<&Content> {
        self.slots.get(id).map(|draft| &draft.content)
    }

    pub fn has_transient(&self) -> bool {
        !self.transient.is_empty()
    }

    fn is_transient(&self, id: &str) -> bool {
        self.transient.contains(id)
    }

    /// Deja `content` como el borrador de `id` (vacío, lo borra). `durable`: se guarda en
    /// disco. La fecha de edición cambia solo si cambió el texto o los adjuntos.
    pub fn put(&mut self, id: &str, content: Content, durable: bool, now: u64) {
        if content.is_empty() {
            self.remove(id);
            return;
        }
        if durable {
            self.transient.remove(id);
        } else {
            self.transient.insert(id.to_string());
        }
        match self.slots.get_mut(id) {
            Some(draft) if draft.content.same_as(&content) => {
                if draft.content.cursor != content.cursor {
                    draft.content.cursor = content.cursor;
                    self.dirty |= durable;
                }
            }
            _ => {
                self.slots.insert(id.to_string(), Draft { content, edited: now });
                self.dirty |= durable;
            }
        }
    }

    pub fn remove(&mut self, id: &str) {
        if self.slots.remove(id).is_some() && !self.transient.remove(id) {
            self.dirty = true;
        }
    }

    /// El borrador de `from` pasa a `to` (la conversación recibió su sesión, o otra la
    /// reemplazó). Si `to` ya tiene uno, se queda con el suyo.
    pub fn rename(&mut self, from: &str, to: &str, durable: bool) {
        if from == to || self.slots.contains_key(to) {
            return;
        }
        let Some(draft) = self.slots.remove(from) else {
            return;
        };
        self.transient.remove(from);
        if !durable {
            self.transient.insert(to.to_string());
        }
        self.slots.insert(to.to_string(), draft);
        self.dirty = true;
    }

    /// La caja pasa de `from` a `to`: lo que tenía (`current`) queda como borrador de `from`
    /// y vuelve el de `to` (o una caja vacía).
    pub fn switch(&mut self, from: &str, from_durable: bool, current: Content, to: &str, now: u64) -> Content {
        self.put(from, current, from_durable, now);
        self.get(to).cloned().unwrap_or_default()
    }

    /// Escribe a disco si algo cambió.
    pub fn write_if_dirty(&mut self, now: u64) {
        if self.dirty {
            self.write(now);
        }
    }

    /// Escribe el JSON y las imágenes que faltan, y borra las que ya no usa ningún borrador.
    fn write(&mut self, now: u64) {
        self.dirty = false;
        let Some(dir) = &self.dir else {
            return;
        };
        let images = dir.join(IMAGES);
        let mut file = DiskFile::default();
        let mut keep = HashSet::new();
        for (id, draft) in &self.slots {
            if self.transient.contains(id) || is_expired(draft.edited, now) {
                continue;
            }
            file.drafts.insert(id.clone(), DiskDraft::of(draft, &images, &mut keep));
        }
        match serde_json::to_string(&file) {
            Ok(text) => {
                if let Err(error) = std::fs::write(dir.join(FILE), text) {
                    tracing::warn!(%error, "atic code: no se guardaron los borradores de las conversaciones");
                }
            }
            Err(error) => tracing::warn!(%error, "atic code: borradores de las conversaciones sin serializar"),
        }
        let Ok(entries) = std::fs::read_dir(&images) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !keep.contains(&name) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

// --- En disco -------------------------------------------------------------------------

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct DiskFile {
    drafts: HashMap<String, DiskDraft>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct DiskDraft {
    text: String,
    cursor: usize,
    edited: u64,
    attachments: Vec<DiskAttachment>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum DiskAttachment {
    /// `file` es el nombre del archivo en `code-drafts/`.
    Image { name: String, media_type: String, file: String },
    File { path: String },
    Pasted { text: String },
}

impl DiskDraft {
    /// Lo que se escribe de `draft`; sus imágenes quedan en `images` (las que falten se
    /// escriben) y sus nombres en `keep`.
    fn of(draft: &Draft, images: &Path, keep: &mut HashSet<String>) -> Self {
        let mut attachments = Vec::new();
        for attachment in &draft.content.attachments {
            match attachment {
                Attachment::Image { name, media_type, data, .. } => {
                    let Some(file) = image_file(data, media_type, images) else {
                        continue;
                    };
                    keep.insert(file.clone());
                    attachments.push(DiskAttachment::Image { name: name.clone(), media_type: (*media_type).into(), file });
                }
                Attachment::File(path) => attachments.push(DiskAttachment::File { path: path.to_string_lossy().into_owned() }),
                Attachment::Pasted { text, .. } => attachments.push(DiskAttachment::Pasted { text: text.clone() }),
            }
        }
        Self { text: draft.content.text.clone(), cursor: draft.content.cursor, edited: draft.edited, attachments }
    }

    /// El borrador listo para la caja. Las imágenes que ya no se pueden leer se omiten.
    fn content(self, images: &Path) -> Content {
        let mut attachments = Vec::new();
        for attachment in self.attachments {
            match attachment {
                DiskAttachment::Image { name, media_type, file } => {
                    let Some(media_type) = known_media_type(&media_type) else {
                        continue;
                    };
                    // Solo un nombre de archivo, dentro de la carpeta.
                    if file.contains(['/', '\\']) || file.starts_with('.') {
                        continue;
                    }
                    let Ok(bytes) = std::fs::read(images.join(&file)) else {
                        continue;
                    };
                    let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
                    let Some(image) = chat::image_from(media_type, bytes) else {
                        continue;
                    };
                    let size = chat::image_size(&image).unwrap_or((1., 1.));
                    attachments.push(Attachment::Image { name, media_type, data, image, size });
                }
                DiskAttachment::File { path } => attachments.push(Attachment::File(PathBuf::from(path))),
                DiskAttachment::Pasted { text } => {
                    let lines = text.lines().count();
                    attachments.push(Attachment::Pasted { text, lines });
                }
            }
        }
        Content { text: self.text, cursor: self.cursor, attachments }
    }
}

fn known_media_type(media_type: &str) -> Option<&'static str> {
    match media_type {
        "image/png" => Some("image/png"),
        "image/jpeg" => Some("image/jpeg"),
        "image/gif" => Some("image/gif"),
        "image/webp" => Some("image/webp"),
        _ => None,
    }
}

/// El archivo de una imagen (base64 en `data`) en `images`, nombrado por su contenido: la
/// misma imagen no se vuelve a escribir. `None` si no se pudo escribir.
fn image_file(data: &str, media_type: &str, images: &Path) -> Option<String> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut hasher);
    let ext = media_type.strip_prefix("image/").filter(|ext| ext.chars().all(|c| c.is_ascii_alphanumeric())).unwrap_or("img");
    let name = format!("{:016x}.{ext}", hasher.finish());
    let path = images.join(&name);
    if !path.is_file() {
        let bytes = base64::engine::general_purpose::STANDARD.decode(data).ok()?;
        std::fs::create_dir_all(images).ok()?;
        if let Err(error) = std::fs::write(&path, bytes) {
            tracing::warn!(%error, "atic code: no se guardó una imagen de un borrador");
            return None;
        }
    }
    Some(name)
}

// --- En la ventana --------------------------------------------------------------------

impl CodeView {
    /// A quién pertenece ahora la caja.
    fn draft_owner_now(&self) -> Owner {
        match &self.active {
            Some(key) => Owner::Chat(key.clone()),
            None => Owner::New(self.active_workspace()),
        }
    }

    /// La clave del borrador de `owner`: su sesión o, sin sesión todavía, su clave.
    fn draft_slot_of(&self, owner: &Owner) -> String {
        match owner {
            Owner::Chat(key) => match self.chats.iter().find(|c| c.key == *key) {
                Some(chat) => chat.listed_session().unwrap_or(&chat.key).to_string(),
                // Ya se cerró: su borrador sigue donde estaba.
                None => self.draft_slot.clone(),
            },
            Owner::New(workspace) => new_slot(*workspace),
        }
    }

    /// La clave del borrador de una conversación, para mostrarlo en la barra.
    pub(super) fn draft_of(&self, chat: &super::Chat) -> Option<&Content> {
        self.drafts.get(chat.listed_session().unwrap_or(&chat.key))
    }

    /// La clave del borrador de la caja se guarda en disco (no es la de una conversación
    /// sin sesión).
    fn draft_slot_durable(&self) -> bool {
        !matches!(&self.draft_owner, Owner::Chat(key) if *key == self.draft_slot)
    }

    /// Lo que hay en la caja. Mientras se recorren los mensajes enviados con ↑ / ↓, el
    /// borrador es lo que se estaba escribiendo antes.
    fn composer_content(&self, cx: &gpui::App) -> Content {
        let area = self.composer.read(cx);
        match &self.prompt_nav {
            Some(nav) => Content { text: nav.draft.clone(), cursor: nav.draft.len(), attachments: self.attachments.clone() },
            None => Content { text: area.text().to_string(), cursor: area.cursor(), attachments: self.attachments.clone() },
        }
    }

    /// La caja sigue a la conversación a la vista: si cambió, lo escrito queda como borrador
    /// de la anterior y aparece el de esta. Se llama donde cambia la conversación y en cada
    /// cuadro (por si se cambió por otro lado); no avisa (quien la cambió ya avisó).
    pub(super) fn sync_draft(&mut self, cx: &mut Context<Self>) {
        // Lo guardado con la clave de una conversación pasa a su sesión cuando la recibe.
        if self.drafts.has_transient() {
            let sessions: Vec<(String, String)> = self
                .chats
                .iter()
                .filter(|c| self.drafts.is_transient(&c.key))
                .filter_map(|c| c.listed_session().map(|s| (c.key.clone(), s.to_string())))
                .collect();
            for (key, session) in sessions {
                self.drafts.rename(&key, &session, true);
            }
        }
        let owner = self.draft_owner_now();
        let slot = self.draft_slot_of(&owner);
        if owner == self.draft_owner {
            if slot != self.draft_slot {
                self.drafts.rename(&self.draft_slot, &slot, true);
                self.draft_slot = slot;
            }
            return;
        }
        let current = self.composer_content(cx);
        let durable = self.draft_slot_durable();
        let from = std::mem::take(&mut self.draft_slot);
        let next = self.drafts.switch(&from, durable, current, &slot, super::marks::now_secs());
        self.draft_owner = owner;
        self.draft_slot = slot;
        self.prompt_nav = None;
        self.attachments = next.attachments;
        let (text, cursor) = (next.text, next.cursor);
        self.composer.update(cx, |area, cx| area.set_text_at(&text, cursor, cx));
        self.draft_seen = (text.len(), self.attachments.len());
        self.close_mention(cx);
        self.save_drafts_soon(cx);
    }

    /// La caja cambió (se escribió, se adjuntó o se quitó algo): se guarda en un rato.
    /// En cada cuadro se mira si cambió el largo del texto o los adjuntos, así también
    /// cuentan los textos puestos desde afuera (citar, comandos, la paleta).
    pub(super) fn note_draft_change(&mut self, cx: &mut Context<Self>) {
        let seen = (self.composer.read(cx).text().len(), self.attachments.len());
        if seen != self.draft_seen {
            self.draft_seen = seen;
            self.save_drafts_soon(cx);
        }
    }

    /// Guarda los borradores después de `SAVE_DELAY` sin otro cambio (no en cada tecla).
    pub(super) fn save_drafts_soon(&mut self, cx: &mut Context<Self>) {
        self.draft_gen += 1;
        let generation = self.draft_gen;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            let _ = this.update(cx, |view, cx| {
                if view.draft_gen == generation {
                    view.save_drafts(cx);
                }
            });
        })
        .detach();
    }

    /// Lo que hay en la caja pasa a su borrador y se escribe a disco ahora (al enviar, para
    /// que un borrador ya enviado no vuelva si Atic se cierra enseguida).
    pub(super) fn save_drafts(&mut self, cx: &mut Context<Self>) {
        self.sync_draft(cx);
        let current = self.composer_content(cx);
        let durable = self.draft_slot_durable();
        let now = super::marks::now_secs();
        self.drafts.put(&self.draft_slot.clone(), current, durable, now);
        self.drafts.write_if_dirty(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(text: &str) -> Content {
        Content { text: text.into(), cursor: text.len(), attachments: Vec::new() }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("atic-drafts-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn png() -> Vec<u8> {
        let mut bytes = Vec::new();
        image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]))
            .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        bytes
    }

    fn image_attachment(bytes: Vec<u8>) -> Attachment {
        let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let image = chat::image_from("image/png", bytes).unwrap();
        Attachment::Image { name: "captura.png".into(), media_type: "image/png", data, image, size: (2., 2.) }
    }

    #[test]
    fn cambiar_de_chat_no_arrastra_el_texto() {
        let mut drafts = Drafts::default();
        // En A se escribe algo y se pasa a B: B empieza vacía.
        let b = drafts.switch("a", true, text("hola desde A"), "b", 100);
        assert!(b.text.is_empty() && b.attachments.is_empty());
        // En B se escribe otra cosa con un adjunto y se vuelve a A.
        let mut in_b = text("lo de B");
        in_b.cursor = 3;
        in_b.attachments.push(Attachment::File(PathBuf::from("/tmp/a.rs")));
        let a = drafts.switch("b", true, in_b, "a", 101);
        assert_eq!(a.text, "hola desde A");
        let back = drafts.switch("a", true, a, "b", 102);
        assert_eq!(back.text, "lo de B");
        assert_eq!(back.cursor, 3);
        assert!(matches!(back.attachments.as_slice(), [Attachment::File(p)] if p == Path::new("/tmp/a.rs")));
    }

    #[test]
    fn una_caja_vacia_borra_el_borrador() {
        let mut drafts = Drafts::default();
        drafts.put("a", text("algo"), true, 1);
        assert!(drafts.get("a").is_some());
        // Enviar deja la caja vacía: el borrador se va.
        drafts.put("a", text("  "), true, 2);
        assert!(drafts.get("a").is_none());
    }

    #[test]
    fn el_borrador_sin_sesion_pasa_a_la_sesion() {
        let mut drafts = Drafts::default();
        drafts.put("c1-x", text("antes de la sesión"), false, 1);
        assert!(drafts.has_transient());
        drafts.rename("c1-x", "sesion-1", true);
        assert!(!drafts.has_transient());
        assert_eq!(drafts.get("sesion-1").map(|c| c.text.as_str()), Some("antes de la sesión"));
        assert!(drafts.get("c1-x").is_none());
    }

    #[test]
    fn se_guarda_y_se_lee_con_sus_imagenes_como_archivos() {
        let dir = temp_dir("guardar");
        let mut drafts = Drafts { dir: Some(dir.clone()), ..Default::default() };
        let mut content = text("mira esta captura");
        content.cursor = 4;
        content.attachments.push(image_attachment(png()));
        content.attachments.push(Attachment::Pasted { text: "uno\ndos".into(), lines: 2 });
        drafts.put("sesion-1", content, true, 1_000);
        // Uno sin sesión no se escribe.
        drafts.put("c9-x", text("solo en memoria"), false, 1_000);
        drafts.write_if_dirty(1_000);
        let json = std::fs::read_to_string(dir.join(FILE)).unwrap();
        assert!(!json.contains("iVBOR"), "la imagen no va en base64 dentro del JSON");
        assert!(!json.contains("solo en memoria"));
        assert_eq!(std::fs::read_dir(dir.join(IMAGES)).unwrap().count(), 1);

        let back = Drafts::load_from(dir.clone(), 1_000 + 60);
        let content = back.get("sesion-1").expect("el borrador vuelve");
        assert_eq!(content.text, "mira esta captura");
        assert_eq!(content.cursor, 4);
        assert!(matches!(&content.attachments[0], Attachment::Image { name, media_type: "image/png", .. } if name == "captura.png"));
        assert!(matches!(&content.attachments[1], Attachment::Pasted { lines: 2, .. }));
        assert!(back.get("c9-x").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn enviar_borra_el_borrador_y_su_imagen() {
        let dir = temp_dir("enviar");
        let mut drafts = Drafts { dir: Some(dir.clone()), ..Default::default() };
        let mut content = text("con imagen");
        content.attachments.push(image_attachment(png()));
        drafts.put("sesion-1", content, true, 1_000);
        drafts.write_if_dirty(1_000);
        assert_eq!(std::fs::read_dir(dir.join(IMAGES)).unwrap().count(), 1);
        drafts.put("sesion-1", Content::default(), true, 1_001);
        drafts.write_if_dirty(1_001);
        assert_eq!(std::fs::read_dir(dir.join(IMAGES)).unwrap().count(), 0);
        assert!(Drafts::load_from(dir.clone(), 1_002).get("sesion-1").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn los_borradores_viejos_vencen() {
        let dir = temp_dir("vencer");
        let mut drafts = Drafts { dir: Some(dir.clone()), ..Default::default() };
        let mut old = text("viejo");
        old.attachments.push(image_attachment(png()));
        drafts.put("vieja", old, true, 1_000);
        drafts.put("nueva", text("reciente"), true, 1_000 + MAX_AGE.as_secs());
        drafts.write_if_dirty(1_000);
        // Ocho días después del primero y uno después del segundo.
        let now = 1_000 + MAX_AGE.as_secs() + 24 * 3600;
        let back = Drafts::load_from(dir.clone(), now);
        assert!(back.get("vieja").is_none());
        assert_eq!(back.get("nueva").map(|c| c.text.as_str()), Some("reciente"));
        // La imagen del vencido también se fue.
        assert_eq!(std::fs::read_dir(dir.join(IMAGES)).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn mover_el_cursor_no_cuenta_como_edicion() {
        let mut drafts = Drafts::default();
        drafts.put("a", text("hola"), true, 1);
        let mut moved = text("hola");
        moved.cursor = 1;
        drafts.put("a", moved, true, 999_999_999);
        assert_eq!(drafts.slots["a"].edited, 1);
        assert_eq!(drafts.get("a").map(|c| c.cursor), Some(1));
    }

    #[test]
    fn la_conversacion_nueva_tiene_un_borrador_por_espacio() {
        assert_eq!(new_slot(Some(7)), "new:7");
        assert_eq!(new_slot(None), "new");
        assert_ne!(new_slot(Some(7)), new_slot(Some(8)));
    }
}
