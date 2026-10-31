//! El portapapeles de macOS para lo que GPUI no escribe ni lee: una imagen
//! como PNG (`public.png`, que entienden todas las apps; las que piden TIFF
//! lo convierten solas) y archivos como URLs de archivo.

use std::path::PathBuf;

use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSPasteboard, NSPasteboardTypeFileURL, NSPasteboardTypePNG, NSPasteboardWriting};
use objc2_foundation::{NSArray, NSData, NSString, NSURL};

pub fn write(width: u32, height: u32, bgra: &[u8], png: &[u8]) -> Result<(), String> {
    let board = NSPasteboard::generalPasteboard();
    board.clearContents();
    let data = NSData::with_bytes(png);
    // SAFETY: constante de AppKit, viva todo el proceso.
    let kind = unsafe { NSPasteboardTypePNG };
    if board.setData_forType(Some(&data), kind) {
        atic_clipboard::copied_own_image(width as usize, height as usize, bgra);
        Ok(())
    } else {
        Err("el portapapeles no aceptó la imagen".into())
    }
}

/// Archivos al portapapeles: pegarlos en Finder, un chat o un correo los
/// adjunta.
pub fn write_files(paths: &[String]) -> Result<(), String> {
    let urls: Vec<_> = paths
        .iter()
        .map(|path| ProtocolObject::<dyn NSPasteboardWriting>::from_retained(NSURL::fileURLWithPath(&NSString::from_str(path))))
        .collect();
    let board = NSPasteboard::generalPasteboard();
    board.clearContents();
    if board.writeObjects(&NSArray::from_retained_slice(&urls)) {
        Ok(())
    } else {
        Err("el portapapeles no aceptó los archivos".into())
    }
}

/// Los archivos copiados en Finder, si los hay. GPUI solo lee texto e
/// imágenes del portapapeles.
pub fn read_files() -> Vec<PathBuf> {
    let board = NSPasteboard::generalPasteboard();
    let Some(items) = board.pasteboardItems() else {
        return Vec::new();
    };
    // SAFETY: constante de AppKit, viva todo el proceso.
    let kind = unsafe { NSPasteboardTypeFileURL };
    items
        .iter()
        .filter_map(|item| item.stringForType(kind))
        .filter_map(|text| NSURL::URLWithString(&text))
        .filter_map(|url| url.path())
        .map(|path| PathBuf::from(path.to_string()))
        .collect()
}
