//! Arma las páginas que recibe `flip_export.rs`, como `armarExport` de Atic
//! (`flipExport.ts`).
//!
//! PDF, Word y PowerPoint llevan contenido nativo: el texto como texto, las
//! fotos con sus bytes originales y la tinta como una imagen transparente
//! aparte, porque un trazo no puede volverse texto. PNG y JPEG son la página
//! entera ya dibujada (`flip_board.rs` la captura de la pantalla).

use std::path::Path;

use base64::Engine;
use image::ImageEncoder;

use crate::flip_board::{Block, Body, PAGE_H, PAGE_W};
use crate::flip_export::{
    FlipExportFoto, FlipExportItem, FlipExportLista, FlipExportPage, FlipExportTexto,
};

/// Escala de la tinta rasterizada (`escala = 2` en Atic).
const INK_SCALE: f32 = 2.0;

/// La parte de un marco que cae en la celda `index`, en coordenadas de la
/// celda (`marcoEnPagina`).
pub fn in_page(x: f32, y: f32, w: f32, h: f32, index: usize) -> Option<(f64, f64, f64, f64)> {
    let x0 = index as f32 * PAGE_W;
    let left = x.max(x0);
    let right = (x + w).min(x0 + PAGE_W);
    let top = y.max(0.0);
    let bottom = (y + h).min(PAGE_H);
    if right - left < 1.0 || bottom - top < 1.0 {
        return None;
    }
    Some((
        (left - x0) as f64,
        top as f64,
        (right - left) as f64,
        (bottom - top) as f64,
    ))
}

fn mime_of(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        _ => "image/png",
    }
}

/// Páginas de contenido nativo, y los assets que no se pudieron leer.
pub fn native_pages(
    blocks: &[Block],
    count: usize,
    assets: &Path,
) -> (Vec<FlipExportPage>, Vec<String>) {
    let b64 = base64::engine::general_purpose::STANDARD;
    let mut failed = Vec::new();
    let mut pages = Vec::with_capacity(count);
    for index in 0..count {
        let mut page = FlipExportPage {
            preview_base64: String::new(),
            preview_mime: "image/png".into(),
            fotos: Vec::new(),
            textos: Vec::new(),
            checks: Vec::new(),
            ink_base64: ink_png(blocks, index).unwrap_or_default(),
        };
        for block in blocks {
            let Some((x, y, w, h)) = in_page(block.x, block.y, block.w, block.h, index) else {
                continue;
            };
            match &block.body {
                Body::Text { body } => page.textos.push(FlipExportTexto {
                    body: body.clone(),
                    x,
                    y,
                    w,
                    h,
                }),
                Body::Check { items } => page.checks.push(FlipExportLista {
                    items: items
                        .iter()
                        .map(|item| FlipExportItem {
                            text: item.text.clone(),
                            done: item.done,
                        })
                        .collect(),
                    x,
                    y,
                    w,
                    h,
                }),
                Body::Image { asset, .. } => {
                    let path = assets.join(asset);
                    match std::fs::read(&path) {
                        Ok(bytes) => page.fotos.push(FlipExportFoto {
                            image_base64: b64.encode(bytes),
                            mime: mime_of(&path).into(),
                            x,
                            y,
                            w,
                            h,
                        }),
                        Err(_) => {
                            if !failed.contains(asset) {
                                failed.push(asset.clone());
                            }
                        }
                    }
                }
                Body::Ink { .. } => {}
            }
        }
        pages.push(page);
    }
    (pages, failed)
}

/// Los trazos de la página `index` en una capa transparente de 2400×1800,
/// en base64; `None` si no hay nada que dibujar (`rasterTinta`).
pub fn ink_png(blocks: &[Block], index: usize) -> Option<String> {
    use tiny_skia::{LineCap, LineJoin, Paint, PathBuilder, Pixmap, Stroke, Transform};

    let strokes: Vec<_> = blocks
        .iter()
        .filter_map(|b| match &b.body {
            Body::Ink { strokes, .. } => Some(strokes),
            _ => None,
        })
        .flatten()
        .filter(|s| s.points.len() >= 2)
        .collect();
    if strokes.is_empty() {
        return None;
    }
    let mut pixmap = Pixmap::new((PAGE_W * INK_SCALE) as u32, (PAGE_H * INK_SCALE) as u32)?;
    let page_x = index as f32 * PAGE_W;
    let transform = Transform::from_scale(INK_SCALE, INK_SCALE).pre_translate(-page_x, 0.0);
    let mut drew = false;
    for stroke in strokes {
        let color = crate::flip_board::parse_rgba(&stroke.color);
        let mut builder = PathBuilder::new();
        builder.move_to(stroke.points[0][0], stroke.points[0][1]);
        for p in &stroke.points[1..] {
            builder.line_to(p[0], p[1]);
        }
        let Some(path) = builder.finish() else {
            continue;
        };
        let mut paint = Paint::default();
        paint.set_color_rgba8(color.0, color.1, color.2, color.3);
        paint.anti_alias = true;
        let line = Stroke {
            width: stroke.width,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Stroke::default()
        };
        pixmap.stroke_path(&path, &paint, &line, transform, None);
        drew = true;
    }
    if !drew {
        return None;
    }
    let png = pixmap.encode_png().ok()?;
    Some(base64::engine::general_purpose::STANDARD.encode(png))
}

/// Páginas de una sola imagen (PNG o JPEG), a partir de lo capturado:
/// `(ancho, alto, BGRA)` por página.
pub fn image_pages(
    frames: &[(u32, u32, Vec<u8>)],
    jpeg: bool,
) -> Result<Vec<FlipExportPage>, String> {
    let b64 = base64::engine::general_purpose::STANDARD;
    frames
        .iter()
        .map(|(w, h, bgra)| {
            let mut rgb = Vec::with_capacity(bgra.len() / 4 * 3);
            for px in bgra.chunks_exact(4) {
                rgb.extend_from_slice(&[px[2], px[1], px[0]]);
            }
            let mut out = Vec::new();
            if jpeg {
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 92)
                    .encode(&rgb, *w, *h, image::ExtendedColorType::Rgb8)
                    .map_err(|e| e.to_string())?;
            } else {
                image::codecs::png::PngEncoder::new(&mut out)
                    .write_image(&rgb, *w, *h, image::ExtendedColorType::Rgb8)
                    .map_err(|e| e.to_string())?;
            }
            Ok(FlipExportPage {
                preview_base64: b64.encode(out),
                preview_mime: if jpeg { "image/jpeg" } else { "image/png" }.into(),
                fotos: Vec::new(),
                textos: Vec::new(),
                checks: Vec::new(),
                ink_base64: String::new(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flip_board::{Block, Body, Item, Stroke};

    fn block(x: f32, w: f32, body: Body) -> Block {
        Block {
            id: "b".into(),
            x,
            y: 10.0,
            w,
            h: 50.0,
            body,
        }
    }

    #[test]
    fn a_frame_is_cut_to_the_page_it_falls_in() {
        // Cruza el borde entre la página 0 y la 1.
        let a = in_page(1150.0, 10.0, 100.0, 50.0, 0).unwrap();
        let b = in_page(1150.0, 10.0, 100.0, 50.0, 1).unwrap();
        assert_eq!((a.0, a.2), (1150.0, 50.0));
        assert_eq!((b.0, b.2), (0.0, 50.0));
        assert!(in_page(1150.0, 10.0, 100.0, 50.0, 2).is_none());
    }

    #[test]
    fn native_pages_split_text_lists_and_ink() {
        let blocks = vec![
            block(
                0.0,
                300.0,
                Body::Text {
                    body: "hola".into(),
                },
            ),
            block(
                1300.0,
                200.0,
                Body::Check {
                    items: vec![Item {
                        id: "i".into(),
                        text: "a".into(),
                        done: true,
                    }],
                },
            ),
            block(
                0.0,
                1200.0,
                Body::Ink {
                    strokes: vec![Stroke {
                        color: "#e5483f".into(),
                        width: 2.6,
                        points: vec![[10.0, 10.0], [100.0, 100.0]],
                    }],
                    height: 0,
                },
            ),
        ];
        let (pages, failed) = native_pages(&blocks, 2, Path::new("."));
        assert!(failed.is_empty());
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].textos.len(), 1);
        assert_eq!(pages[0].checks.len(), 0);
        assert_eq!(pages[1].checks.len(), 1);
        assert_eq!(pages[1].checks[0].x, 100.0);
        // La tinta de la hoja 0 se rasteriza; la hoja 1 no tiene.
        assert!(!pages[0].ink_base64.is_empty());
    }

    #[test]
    fn a_missing_asset_is_reported_not_swallowed() {
        let blocks = vec![block(
            0.0,
            100.0,
            Body::Image {
                asset: "no-existe.png".into(),
                width: 10,
                height: 10,
            },
        )];
        let (pages, failed) = native_pages(&blocks, 1, Path::new("."));
        assert_eq!(failed, ["no-existe.png"]);
        assert!(pages[0].fotos.is_empty());
    }

    #[test]
    fn image_pages_encode_png_and_jpeg() {
        let bgra = vec![255u8; 4 * 4 * 4];
        let png = image_pages(&[(4, 4, bgra.clone())], false).unwrap();
        let jpg = image_pages(&[(4, 4, bgra)], true).unwrap();
        assert_eq!(png[0].preview_mime, "image/png");
        assert_eq!(jpg[0].preview_mime, "image/jpeg");
        assert!(!png[0].preview_base64.is_empty() && !jpg[0].preview_base64.is_empty());
    }
}
