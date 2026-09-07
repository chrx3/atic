//! Notas por app: documentos con páginas y bloques.
//!
//! Reemplaza al `window_flip_notes.json` de una línea por clave. El motivo del
//! cambio no es el formato: es la clave. Antes era `exe|título`, y el título de
//! una ventana cambia todo el tiempo —una pestaña, un documento, un archivo sin
//! guardar—, así que las notas se fragmentaban solas y el usuario perdía lo que
//! había escrito sin entender por qué.
//!
//! Acá la nota es **de la app**, y adentro tiene páginas. El título de la
//! ventana elige la página, no la nota: si cambia, aparece otra página, pero
//! las demás siguen a un clic de distancia en vez de desaparecer.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const NOTE_FILE: &str = "note.json";
const ASSETS_DIR: &str = "assets";
/// El JSON viejo, de cuando la nota era un `String` por `exe|título`.
const LEGACY_FILE: &str = "window_flip_notes.json";
const LEGACY_DONE: &str = "window_flip_notes.migrado.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: String,
    /// Nombre del ejecutable, tal como lo ve `window_flip`.
    pub app: String,
    pub pages: Vec<Page>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page {
    pub id: String,
    /// Título de la ventana con la que nació. Es la etiqueta y el criterio de
    /// búsqueda al voltear; vacío = la página suelta de esa app.
    pub title: String,
    pub blocks: Vec<Block>,
    pub updated_at: i64,
}

/// Un bloque de contenido. `kind` va en el JSON para que el front pueda hacer
/// un `switch` sin adivinar por las llaves presentes.
///
/// `x/y/w/h` son el marco en el tablero. Cero = nota vieja en columna: el
/// front los reparte al abrir y los guarda ya colocados.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Block {
    Text {
        id: String,
        body: String,
        #[serde(default)]
        x: f32,
        #[serde(default)]
        y: f32,
        #[serde(default)]
        w: f32,
        #[serde(default)]
        h: f32,
    },
    Image {
        id: String,
        /// Nombre del archivo dentro de `assets/`, no una ruta absoluta: la
        /// carpeta de datos cambia de lugar entre máquinas y la nota no.
        asset: String,
        width: u32,
        height: u32,
        #[serde(default)]
        x: f32,
        #[serde(default)]
        y: f32,
        #[serde(default)]
        w: f32,
        #[serde(default)]
        h: f32,
    },
    Check {
        id: String,
        items: Vec<CheckItem>,
        #[serde(default)]
        x: f32,
        #[serde(default)]
        y: f32,
        #[serde(default)]
        w: f32,
        #[serde(default)]
        h: f32,
    },
    Ink {
        id: String,
        strokes: Vec<Stroke>,
        height: u32,
        #[serde(default)]
        x: f32,
        #[serde(default)]
        y: f32,
        #[serde(default)]
        w: f32,
        #[serde(default)]
        h: f32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckItem {
    pub id: String,
    pub text: String,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stroke {
    pub color: String,
    pub width: f32,
    /// Pares `[x, y]` en coordenadas del bloque. Se guardan los puntos y no un
    /// PNG para poder rehacer el trazo al cambiar de tamaño o de tema.
    pub points: Vec<[f32; 2]>,
}

impl Note {
    fn new(app: &str) -> Self {
        Note {
            id: uuid::Uuid::new_v4().to_string(),
            app: app.to_string(),
            pages: Vec::new(),
            updated_at: now(),
        }
    }

    /// La página de ese título, o una nueva si no existe.
    ///
    /// Devuelve el id en vez de la página para no atar al llamador a un préstamo
    /// del documento entero mientras la edita.
    pub fn page_for_title(&mut self, title: &str) -> String {
        let title = title.trim();
        if let Some(page) = self.pages.iter().find(|p| p.title == title) {
            return page.id.clone();
        }
        let page = Page {
            id: uuid::Uuid::new_v4().to_string(),
            title: title.to_string(),
            blocks: Vec::new(),
            updated_at: now(),
        };
        let id = page.id.clone();
        self.pages.push(page);
        id
    }

    pub fn page_mut(&mut self, page_id: &str) -> Option<&mut Page> {
        self.pages.iter_mut().find(|p| p.id == page_id)
    }
}

impl Page {
    /// Todo el texto de la página en uno solo.
    ///
    /// Es el puente con la UI vieja de una sola caja de texto: mientras el
    /// reverso siga siendo un `textarea`, lee y escribe por acá.
    pub fn plain_text(&self) -> String {
        self.blocks
            .iter()
            .filter_map(|b| match b {
                Block::Text { body, .. } => Some(body.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Reemplaza el contenido de la página.
    pub fn set_blocks(&mut self, blocks: Vec<Block>) {
        self.blocks = blocks;
        self.updated_at = now();
    }

    /// Deja `body` como único bloque de texto, conservando imágenes y tinta.
    pub fn set_plain_text(&mut self, body: &str) {
        let mut primero = true;
        self.blocks.retain(|b| match b {
            Block::Text { .. } => {
                let quedarse = primero;
                primero = false;
                quedarse
            }
            _ => true,
        });
        match self.blocks.iter_mut().find_map(|b| match b {
            Block::Text { body: actual, .. } => Some(actual),
            _ => None,
        }) {
            Some(actual) => *actual = body.to_string(),
            None if !body.is_empty() => self.blocks.insert(
                0,
                Block::Text {
                    id: uuid::Uuid::new_v4().to_string(),
                    body: body.to_string(),
                    x: 0.0,
                    y: 0.0,
                    w: 0.0,
                    h: 0.0,
                },
            ),
            None => {}
        }
        self.updated_at = now();
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

/// Nombre de carpeta seguro para un ejecutable.
///
/// Los nombres reales (`chrome.exe`, `Code.exe`) ya son inofensivos, pero esto
/// viene de otro proceso: cualquier cosa que no sea alfanumérico, punto, guion
/// o guion bajo se reemplaza, y se corta el largo. Sin esto, un `..` en el
/// nombre escribiría fuera de la carpeta de notas.
fn app_slug(app: &str) -> String {
    let limpio: String = app
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .take(64)
        .collect();
    let limpio = limpio.trim_matches(['.', '-', '_']).to_string();
    if limpio.is_empty() {
        "app".to_string()
    } else {
        limpio
    }
}

pub fn note_dir(notes_dir: &Path, app: &str) -> PathBuf {
    notes_dir.join(app_slug(app))
}

/// Binarios que el usuario pegó en las notas de esa app.
pub fn assets_dir(notes_dir: &Path, app: &str) -> PathBuf {
    note_dir(notes_dir, app).join(ASSETS_DIR)
}

/// Guarda la imagen que hay ahora en el portapapeles del sistema dentro de
/// `assets/` y devuelve `(nombre, ancho, alto)`.
///
/// La imagen se lee acá y no en el webview a propósito: el `paste` del front
/// tiene los bytes, pero mandarlos por IPC significa serializarlos a JSON (una
/// captura de 3 MB se vuelve un array de treinta y pico de millones de
/// caracteres). El portapapeles del sistema ya los tiene de este lado.
pub fn add_clipboard_image(notes_dir: &Path, app: &str) -> Result<(String, u32, u32), String> {
    let img = arboard::Clipboard::new()
        .and_then(|mut c| c.get_image())
        .map_err(|e| e.to_string())?;
    let (w, h) = (img.width, img.height);
    if w == 0 || h == 0 {
        return Err("la imagen del portapapeles está vacía".into());
    }
    let png = crate::clipboard_history::encode_png_rgba(&img.bytes, w, h)?;
    if png.len() > crate::clipboard_history::MAX_IMAGE_BYTES {
        return Err(crate::ui_lang::msg(
            "PNG demasiado grande",
            "PNG is too large",
        ));
    }
    let dir = assets_dir(notes_dir, app);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let asset = format!("img-{}.png", uuid::Uuid::new_v4());
    std::fs::write(dir.join(&asset), png).map_err(|e| e.to_string())?;
    Ok((asset, w as u32, h as u32))
}

/// Copia un PNG que ya existe (una imagen del historial del portapapeles) a
/// los binarios de la nota.
///
/// Se decodifica y se vuelve a codificar en vez de copiar el archivo: valida
/// que sea una imagen de verdad y deja todo el `assets/` en un solo formato.
pub fn import_image(notes_dir: &Path, app: &str, origen: &Path) -> Result<(String, u32, u32), String> {
    let img = image::open(origen).map_err(|e| e.to_string())?.to_rgba8();
    let (w, h) = img.dimensions();
    let png = crate::clipboard_history::encode_png_rgba(&img, w as usize, h as usize)?;
    if png.len() > crate::clipboard_history::MAX_IMAGE_BYTES {
        return Err(crate::ui_lang::msg(
            "PNG demasiado grande",
            "PNG is too large",
        ));
    }
    let dir = assets_dir(notes_dir, app);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let asset = format!("img-{}.png", uuid::Uuid::new_v4());
    std::fs::write(dir.join(&asset), png).map_err(|e| e.to_string())?;
    Ok((asset, w, h))
}

/// Borra los binarios que ya no menciona ninguna página.
///
/// Se llama al guardar: si no, cada imagen borrada de una nota queda ocupando
/// disco para siempre, y son capturas de pantalla del usuario.
pub fn collect_garbage(notes_dir: &Path, note: &Note) {
    let dir = assets_dir(notes_dir, &note.app);
    let Ok(entradas) = std::fs::read_dir(&dir) else {
        return;
    };
    let vivos: std::collections::HashSet<&str> = note
        .pages
        .iter()
        .flat_map(|p| p.blocks.iter())
        .filter_map(|b| match b {
            Block::Image { asset, .. } => Some(asset.as_str()),
            _ => None,
        })
        .collect();
    for entrada in entradas.flatten() {
        let nombre = entrada.file_name();
        let Some(nombre) = nombre.to_str() else { continue };
        if !vivos.contains(nombre) {
            let _ = std::fs::remove_file(entrada.path());
        }
    }
}

/// Lee la nota de una app. Si no existe todavía, devuelve una vacía sin
/// tocar el disco: escribir es cosa de [`save`].
pub fn load(notes_dir: &Path, app: &str) -> Note {
    let path = note_dir(notes_dir, app).join(NOTE_FILE);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Note::new(app);
    };
    match serde_json::from_str::<Note>(&raw) {
        Ok(note) => note,
        Err(err) => {
            // Antes que perder lo que había, se conserva el archivo ilegible al
            // lado: es texto que el usuario escribió y no lo tiene en otro lado.
            tracing::error!(target: "notes", %err, app, "note.json ilegible; se guarda como .roto");
            let _ = std::fs::rename(&path, path.with_extension("roto.json"));
            Note::new(app)
        }
    }
}

/// Escribe la nota entera. Primero a un temporal y después `rename`: un corte
/// de luz a mitad de escritura deja la nota anterior intacta, no media nota.
pub fn save(notes_dir: &Path, note: &mut Note) -> Result<(), String> {
    note.updated_at = now();
    let dir = note_dir(notes_dir, &note.app);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let raw = serde_json::to_string_pretty(note).map_err(|e| e.to_string())?;
    let tmp = dir.join("note.json.tmp");
    std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, dir.join(NOTE_FILE)).map_err(|e| e.to_string())
}

/// Pasa el `window_flip_notes.json` viejo al modelo nuevo, una sola vez.
///
/// Cada clave `exe|título` se vuelve una página de la nota de `exe`, con el
/// título que tenía. Al terminar, el archivo viejo se renombra en vez de
/// borrarse: si algo salió mal, el texto original sigue ahí.
pub fn migrate_legacy(data_dir: &Path, notes_dir: &Path) {
    let legacy = data_dir.join(LEGACY_FILE);
    let Ok(raw) = std::fs::read_to_string(&legacy) else {
        return;
    };
    let Ok(viejas) = serde_json::from_str::<std::collections::HashMap<String, String>>(&raw) else {
        tracing::warn!(target: "notes", "el JSON de notas viejo no se pudo leer; se deja como está");
        return;
    };

    let mut migradas = 0usize;
    for (clave, cuerpo) in viejas {
        if cuerpo.trim().is_empty() {
            continue;
        }
        let (app, titulo) = match clave.split_once('|') {
            Some((app, titulo)) => (app, titulo),
            None => (clave.as_str(), ""),
        };
        let mut note = load(notes_dir, app);
        let page_id = note.page_for_title(titulo);
        if let Some(page) = note.page_mut(&page_id) {
            // Si ya había algo (dos claves para la misma página), no se pisa.
            if page.plain_text().is_empty() {
                page.set_plain_text(&cuerpo);
            }
        }
        if let Err(err) = save(notes_dir, &mut note) {
            tracing::error!(target: "notes", %err, app, "no se pudo migrar una nota");
            return; // Sin renombrar el original: se reintenta en el próximo arranque.
        }
        migradas += 1;
    }

    let _ = std::fs::rename(&legacy, data_dir.join(LEGACY_DONE));
    if migradas > 0 {
        tracing::info!(target: "notes", migradas, "notas migradas al modelo por app");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_slug_no_sale_de_la_carpeta() {
        assert_eq!(app_slug("../../etc/passwd"), "etc-passwd");
        assert_eq!(app_slug("chrome.exe"), "chrome.exe");
        assert_eq!(app_slug(""), "app");
        assert_eq!(app_slug("..."), "app");
    }

    #[test]
    fn la_pagina_se_reusa_por_titulo() {
        let mut note = Note::new("code.exe");
        let a = note.page_for_title("main.rs");
        let b = note.page_for_title("main.rs");
        let c = note.page_for_title("otro.rs");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(note.pages.len(), 2);
    }

    #[test]
    fn el_texto_plano_no_pisa_imagenes() {
        let mut page = Page {
            id: "p".into(),
            title: String::new(),
            blocks: vec![
                Block::Text {
                    id: "t".into(),
                    body: "hola".into(),
                    x: 0.0,
                    y: 0.0,
                    w: 0.0,
                    h: 0.0,
                },
                Block::Image {
                    id: "i".into(),
                    asset: "img-1.png".into(),
                    width: 10,
                    height: 10,
                    x: 0.0,
                    y: 0.0,
                    w: 0.0,
                    h: 0.0,
                },
            ],
            updated_at: 0,
        };
        page.set_plain_text("chau");
        assert_eq!(page.plain_text(), "chau");
        assert_eq!(page.blocks.len(), 2);
        assert!(matches!(page.blocks[1], Block::Image { .. }));
    }

    #[test]
    fn escribir_en_una_pagina_vacia_crea_el_bloque() {
        let mut page = Page {
            id: "p".into(),
            title: String::new(),
            blocks: Vec::new(),
            updated_at: 0,
        };
        page.set_plain_text("");
        assert!(page.blocks.is_empty());
        page.set_plain_text("algo");
        assert_eq!(page.plain_text(), "algo");
    }

    #[test]
    fn el_json_viejo_sin_marco_carga() {
        let texto: Block =
            serde_json::from_str(r#"{"kind":"text","id":"t","body":"hola"}"#).unwrap();
        match texto {
            Block::Text { x, y, w, h, body, .. } => {
                assert_eq!(body, "hola");
                assert_eq!((x, y, w, h), (0.0, 0.0, 0.0, 0.0));
            }
            _ => panic!("esperaba texto"),
        }
        let lista: Block = serde_json::from_str(
            r#"{"kind":"check","id":"c","items":[{"id":"i","text":"a","done":false}]}"#,
        )
        .unwrap();
        assert!(matches!(lista, Block::Check { .. }));
    }
}
