//! Los fondos de imagen que trae el editor (los de OpenScreen; ver `assets/wallpapers/AVISO.md`).
//!
//! El proyecto guarda un fondo así como `wallpaper:NN` en `backdrop_image` (en vez de una ruta):
//! sigue valiendo si se mueve el proyecto a otro Mac o si se limpia la carpeta temporal. El
//! compositor necesita un archivo, así que `resolve` lo saca de los bytes incluidos en la app la
//! primera vez que se usa.

use std::path::{Path, PathBuf};

struct Paper {
    id: u8,
    full: &'static [u8],
    thumb: &'static [u8],
}

macro_rules! paper {
    ($id:literal, $file:literal) => {
        Paper {
            id: $id,
            full: include_bytes!(concat!("../../assets/wallpapers/", $file, ".jpg")),
            thumb: include_bytes!(concat!("../../assets/wallpapers/thumbs/", $file, ".jpg")),
        }
    };
}

/// En el orden del selector: primero los de aire propio y al final los que se parecen a los
/// fondos de macOS (así, si hay que quitarlos, es el final de la lista).
const PAPERS: [Paper; 18] = [
    paper!(11, "11"),
    paper!(2, "02"),
    paper!(12, "12"),
    paper!(14, "14"),
    paper!(15, "15"),
    paper!(16, "16"),
    paper!(17, "17"),
    paper!(18, "18"),
    paper!(13, "13"),
    paper!(1, "01"),
    paper!(3, "03"),
    paper!(5, "05"),
    paper!(4, "04"),
    paper!(7, "07"),
    paper!(9, "09"),
    paper!(6, "06"),
    paper!(8, "08"),
    paper!(10, "10"),
];

const PREFIX: &str = "wallpaper:";

/// Los números de los fondos, en el orden del selector.
pub fn ids() -> impl Iterator<Item = u8> {
    PAPERS.iter().map(|p| p.id)
}

/// Lo que se guarda en `backdrop_image` al elegir el fondo `id`.
pub fn reference(id: u8) -> PathBuf {
    PathBuf::from(format!("{PREFIX}{id}"))
}

/// El número del fondo, si `path` es la referencia a uno de los que trae el editor.
pub fn id_of(path: &Path) -> Option<u8> {
    let id: u8 = path.to_str()?.strip_prefix(PREFIX)?.parse().ok()?;
    PAPERS.iter().any(|p| p.id == id).then_some(id)
}

/// La miniatura del selector, como ruta de asset de GPUI.
pub fn thumb_asset(id: u8) -> String {
    format!("wallpapers/thumbs/{id:02}.jpg")
}

/// Los bytes de un asset `wallpapers/…` (para `AssetSource`).
pub fn asset(path: &str) -> Option<&'static [u8]> {
    let rest = path.strip_prefix("wallpapers/")?;
    let (thumb, name) = match rest.strip_prefix("thumbs/") {
        Some(name) => (true, name),
        None => (false, rest),
    };
    let id: u8 = name.strip_suffix(".jpg")?.parse().ok()?;
    let paper = PAPERS.iter().find(|p| p.id == id)?;
    Some(if thumb { paper.thumb } else { paper.full })
}

/// El archivo que el compositor puede abrir: el de la imagen propia si existe, o el fondo del
/// editor ya sacado a la carpeta temporal. `None` si la imagen se movió o se borró.
pub fn resolve(path: &Path) -> Option<PathBuf> {
    match id_of(path) {
        Some(id) => extract(id),
        None => path.exists().then(|| path.to_path_buf()),
    }
}

fn extract(id: u8) -> Option<PathBuf> {
    let paper = PAPERS.iter().find(|p| p.id == id)?;
    let dir = std::env::temp_dir().join("atic-wallpapers");
    let file = dir.join(format!("{id:02}.jpg"));
    // Si macOS limpió la carpeta temporal, se vuelve a sacar.
    if file.metadata().map(|m| m.len() == paper.full.len() as u64).unwrap_or(false) {
        return Some(file);
    }
    std::fs::create_dir_all(&dir).ok()?;
    // Se escribe aparte y se renombra: el compositor nunca ve un archivo a medias.
    let part = dir.join(format!(".{id:02}-{}.part", std::process::id()));
    std::fs::write(&part, paper.full).ok()?;
    std::fs::rename(&part, &file).ok()?;
    Some(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_referencia_se_reconoce_y_lo_ajeno_no() {
        for id in ids() {
            assert_eq!(id_of(&reference(id)), Some(id));
        }
        assert_eq!(id_of(Path::new("wallpaper:99")), None);
        assert_eq!(id_of(Path::new("wallpaper:x")), None);
        assert_eq!(id_of(Path::new("/Users/yo/foto.png")), None);
    }

    #[test]
    fn los_assets_existen_y_son_jpeg() {
        for id in ids() {
            for bytes in [asset(&format!("wallpapers/{id:02}.jpg")), asset(&thumb_asset(id))] {
                let bytes = bytes.expect("falta el asset");
                assert_eq!(&bytes[..2], &[0xff, 0xd8], "no es JPEG");
            }
        }
        assert!(asset("wallpapers/99.jpg").is_none());
        assert!(asset("icons/x.svg").is_none());
    }

    #[test]
    fn resolve_saca_el_fondo_a_un_archivo_y_respeta_lo_perdido() {
        let file = resolve(&reference(11)).expect("no se pudo sacar el fondo");
        assert!(file.exists());
        assert_eq!(resolve(&reference(11)), Some(file));
        assert_eq!(resolve(Path::new("/no/existe/esta/imagen.png")), None);
    }

    #[test]
    fn los_ids_no_se_repiten() {
        let mut all: Vec<u8> = ids().collect();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), PAPERS.len());
    }
}
