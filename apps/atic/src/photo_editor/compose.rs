//! La foto editada, armada en CPU con `image`: el fondo, la sombra y la
//! captura con sus esquinas redondeadas, sobre un lienzo con la proporción
//! elegida. Lo mismo arma la vista previa (con la captura achicada) y lo que
//! se copia o guarda (a tamaño completo): todo se mide como parte del lado
//! corto de la captura, así que sale igual a cualquier escala.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use image::{imageops, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

/// Lo que va detrás de la captura.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Background {
    /// Transparente.
    None,
    Solid(u32),
    /// De arriba a la izquierda a abajo a la derecha.
    Gradient(u32, u32),
    /// Uno de los fondos del editor de video (`recording_editor::wallpapers`).
    Wallpaper(u8),
}

/// La proporción del lienzo. El lienzo crece hacia el lado que falta: la
/// captura nunca se recorta.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ratio {
    /// La de la captura más el relleno.
    #[default]
    Auto,
    /// 16:9.
    Wide,
    /// 4:3.
    Classic,
    /// 1:1.
    Square,
    /// 4:5.
    Portrait,
    /// 9:16.
    Vertical,
}

impl Ratio {
    pub const ALL: [Ratio; 6] = [Ratio::Auto, Ratio::Wide, Ratio::Classic, Ratio::Square, Ratio::Portrait, Ratio::Vertical];

    /// Ancho y alto relativos; `None` con `Auto`.
    pub fn dims(self) -> Option<(u32, u32)> {
        match self {
            Self::Auto => None,
            Self::Wide => Some((16, 9)),
            Self::Classic => Some((4, 3)),
            Self::Square => Some((1, 1)),
            Self::Portrait => Some((4, 5)),
            Self::Vertical => Some((9, 16)),
        }
    }
}

/// El relleno más grande, como parte del lado corto de la captura.
pub const PADDING_MAX: f32 = 0.3;
/// El radio más grande, como parte del lado corto de la captura.
pub const RADIUS_MAX: f32 = 0.12;
/// El radio de siempre: que las esquinas se noten también en la vista previa.
pub const RADIUS_DEFAULT: f32 = 0.035;

/// Cómo queda la foto. Se recuerda entre una captura y otra.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Style {
    pub background: Background,
    /// El relleno alrededor de la captura (parte del lado corto, 0–`PADDING_MAX`).
    pub padding: f32,
    /// El radio de las esquinas (parte del lado corto, 0–`RADIUS_MAX`).
    pub radius: f32,
    /// La intensidad de la sombra: 0 sin sombra, hasta 1.
    pub shadow: f32,
    pub ratio: Ratio,
}

impl Default for Style {
    fn default() -> Self {
        Self { background: Background::Gradient(0x8e2de2, 0x4a00e0), padding: 0.08, radius: RADIUS_DEFAULT, shadow: 0.5, ratio: Ratio::Auto }
    }
}

impl Style {
    /// Los mismos números dentro de sus límites (un archivo editado a mano no rompe nada).
    pub fn clamped(self) -> Self {
        let bound = |v: f32, max: f32, fallback: f32| if v.is_finite() { v.clamp(0.0, max) } else { fallback };
        let d = Self::default();
        Self {
            padding: bound(self.padding, PADDING_MAX, d.padding),
            radius: bound(self.radius, RADIUS_MAX, d.radius),
            shadow: bound(self.shadow, 1.0, d.shadow),
            ..self
        }
    }
}

/// Dónde va la captura en el lienzo, en píxeles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub canvas: (u32, u32),
    /// La esquina de arriba a la izquierda de la captura.
    pub at: (u32, u32),
    pub radius: f32,
}

/// El lienzo para una captura de `src` píxeles: la captura, el relleno a
/// cada lado y, con una proporción fija, lo que falte para llegar a ella.
pub fn layout(src: (u32, u32), style: &Style) -> Layout {
    let style = style.clamped();
    let (w, h) = (src.0.max(1), src.1.max(1));
    let short = w.min(h) as f32;
    let pad = (style.padding * short).round() as u32;
    let (mut cw, mut ch) = (w + 2 * pad, h + 2 * pad);
    if let Some((rw, rh)) = style.ratio.dims() {
        let wide_enough = (ch as u64 * rw as u64).div_ceil(rh as u64) as u32;
        if wide_enough >= cw {
            cw = wide_enough;
        } else {
            ch = (cw as u64 * rh as u64).div_ceil(rw as u64) as u32;
        }
    }
    // El radio no pasa de la mitad del lado corto (más sería un óvalo deforme).
    let radius = (style.radius * short).min(short / 2.0);
    Layout { canvas: (cw, ch), at: ((cw - w) / 2, (ch - h) / 2), radius }
}

/// La distancia con signo del punto `(x, y)` al rectángulo `w`×`h` de
/// esquinas redondeadas `r` (negativa adentro).
fn rounded_sdf(x: f32, y: f32, w: f32, h: f32, r: f32) -> f32 {
    let (hx, hy) = (w / 2.0, h / 2.0);
    let qx = (x - hx).abs() - (hx - r);
    let qy = (y - hy).abs() - (hy - r);
    let (ox, oy) = (qx.max(0.0), qy.max(0.0));
    (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0) - r
}

/// Cuánto del píxel `(x, y)` de la captura queda dentro de sus esquinas
/// redondeadas (0–1, con el borde suavizado). Fuera de las esquinas, 1.
pub fn corner_coverage(x: u32, y: u32, w: u32, h: u32, r: f32) -> f32 {
    if r <= 0.0 {
        return 1.0;
    }
    let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
    let near_x = fx < r || fx > w as f32 - r;
    let near_y = fy < r || fy > h as f32 - r;
    if !(near_x && near_y) {
        return 1.0;
    }
    (0.5 - rounded_sdf(fx, fy, w as f32, h as f32, r)).clamp(0.0, 1.0)
}

/// La sombra: cuánto se difumina, cuánto baja y qué tan oscura es, en
/// píxeles para una captura de lado corto `short`. `None` sin sombra.
fn shadow_params(amount: f32, short: f32) -> Option<(f32, f32, f32)> {
    (amount > 0.0).then(|| {
        let blur = short * (0.015 + 0.05 * amount);
        let drop = short * 0.025 * amount;
        let alpha = 0.18 + 0.32 * amount;
        (blur.max(1.0), drop, alpha)
    })
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn rgb(c: u32) -> [f32; 3] {
    [((c >> 16) & 0xff) as f32, ((c >> 8) & 0xff) as f32, (c & 0xff) as f32]
}

/// Pinta `color` (alfa directo, canales 0–255) con opacidad `a` (0–1) sobre `dst`.
fn blend(dst: &mut Rgba<u8>, color: [f32; 3], a: f32) {
    if a <= 0.0 {
        return;
    }
    let da = dst.0[3] as f32 / 255.0;
    let out_a = a + da * (1.0 - a);
    if out_a <= 0.0 {
        return;
    }
    for (i, c) in color.iter().enumerate() {
        let d = dst.0[i] as f32;
        dst.0[i] = ((c * a + d * da * (1.0 - a)) / out_a).round().clamp(0.0, 255.0) as u8;
    }
    dst.0[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
}

/// Los fondos ya decodificados: el último que se usó (decodificar el JPEG a
/// cada cambio de la vista previa sería lento).
static WALLPAPER: Mutex<Option<(u8, Arc<RgbaImage>)>> = Mutex::new(None);

fn wallpaper(id: u8) -> Option<Arc<RgbaImage>> {
    if let Ok(slot) = WALLPAPER.lock() {
        if let Some((cached, image)) = slot.as_ref() {
            if *cached == id {
                return Some(image.clone());
            }
        }
    }
    let bytes = crate::recording_editor::wallpapers::asset(&format!("wallpapers/{id:02}.jpg"))?;
    let image = Arc::new(image::load_from_memory(bytes).ok()?.to_rgba8());
    if let Ok(mut slot) = WALLPAPER.lock() {
        *slot = Some((id, image.clone()));
    }
    Some(image)
}

/// `picture` cubriendo `w`×`h` (recortada al centro, como `object-fit: cover`).
fn cover(picture: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    let (pw, ph) = (picture.width().max(1) as f32, picture.height().max(1) as f32);
    let scale = (w as f32 / pw).max(h as f32 / ph);
    let (cw, ch) = (((w as f32 / scale).round() as u32).clamp(1, picture.width()), ((h as f32 / scale).round() as u32).clamp(1, picture.height()));
    let (x, y) = ((picture.width() - cw) / 2, (picture.height() - ch) / 2);
    let crop = imageops::crop_imm(picture, x, y, cw, ch).to_image();
    if scale < 1.0 {
        // Achicar: `thumbnail` es rápido y promedia bien.
        imageops::thumbnail(&crop, w, h)
    } else {
        imageops::resize(&crop, w, h, imageops::FilterType::Triangle)
    }
}

fn paint_background(canvas: &mut RgbaImage, background: Background) {
    let (w, h) = canvas.dimensions();
    match background {
        Background::None => {}
        Background::Solid(c) => {
            let [r, g, b] = rgb(c);
            let pixel = Rgba([r as u8, g as u8, b as u8, 255]);
            canvas.pixels_mut().for_each(|p| *p = pixel);
        }
        Background::Gradient(a, b) => {
            let (a, b) = (rgb(a), rgb(b));
            let (fw, fh) = (w as f32, h as f32);
            let span = (fw * fw + fh * fh).max(1.0);
            for (x, y, p) in canvas.enumerate_pixels_mut() {
                // La proyección sobre la diagonal: 0 arriba a la izquierda, 1 abajo a la derecha.
                let t = (((x as f32 + 0.5) * fw + (y as f32 + 0.5) * fh) / span).clamp(0.0, 1.0);
                let mix = |i: usize| (a[i] + (b[i] - a[i]) * t).round() as u8;
                *p = Rgba([mix(0), mix(1), mix(2), 255]);
            }
        }
        Background::Wallpaper(id) => match wallpaper(id) {
            Some(picture) => *canvas = cover(&picture, w, h),
            // Sin el fondo (no debería pasar: va dentro de la app), el degradado de siempre.
            None => paint_background(canvas, Style::default().background),
        },
    }
}

fn paint_shadow(canvas: &mut RgbaImage, layout: &Layout, src: (u32, u32), amount: f32) {
    let Some((blur, drop, alpha)) = shadow_params(amount, src.0.min(src.1) as f32) else {
        return;
    };
    let (cw, ch) = canvas.dimensions();
    let (ax, ay) = (layout.at.0 as f32, layout.at.1 as f32 + drop);
    let (sw, sh) = (src.0 as f32, src.1 as f32);
    // Solo el rectángulo de la captura más lo que se difumina.
    let x0 = (ax - blur).floor().max(0.0) as u32;
    let y0 = (ay - blur).floor().max(0.0) as u32;
    let x1 = ((ax + sw + blur).ceil() as u32).min(cw);
    let y1 = ((ay + sh + blur).ceil() as u32).min(ch);
    let r = layout.radius.max(blur * 0.5).min(sw.min(sh) / 2.0);
    for y in y0..y1 {
        for x in x0..x1 {
            let d = rounded_sdf(x as f32 + 0.5 - ax, y as f32 + 0.5 - ay, sw, sh, r);
            let a = alpha * (1.0 - smoothstep(-blur, blur, d));
            blend(canvas.get_pixel_mut(x, y), [0.0; 3], a);
        }
    }
}

/// La foto editada: `src` sobre su fondo, con relleno, esquinas y sombra.
pub fn compose(src: &RgbaImage, style: &Style) -> RgbaImage {
    let style = style.clamped();
    let (w, h) = src.dimensions();
    let layout = layout((w, h), &style);
    let mut canvas = RgbaImage::new(layout.canvas.0, layout.canvas.1);
    paint_background(&mut canvas, style.background);
    paint_shadow(&mut canvas, &layout, (w, h), style.shadow);
    let (ax, ay) = layout.at;
    for (x, y, pixel) in src.enumerate_pixels() {
        let coverage = corner_coverage(x, y, w, h, layout.radius);
        let dst = canvas.get_pixel_mut(ax + x, ay + y);
        if coverage >= 1.0 && pixel.0[3] == 255 {
            *dst = *pixel;
        } else {
            let a = pixel.0[3] as f32 / 255.0 * coverage;
            blend(dst, [pixel.0[0] as f32, pixel.0[1] as f32, pixel.0[2] as f32], a);
        }
    }
    canvas
}

/// Solo para la vista previa: un damero de `cell` píxeles (colores `a` y `b`)
/// detrás de lo transparente, para que se note que no hay fondo.
pub fn checker_under(image: &mut RgbaImage, cell: u32, a: [u8; 3], b: [u8; 3]) {
    let cell = cell.max(1);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        let alpha = pixel.0[3] as f32 / 255.0;
        if alpha >= 1.0 {
            continue;
        }
        let under = if (x / cell + y / cell) % 2 == 0 { a } else { b };
        for i in 0..3 {
            let top = pixel.0[i] as f32 * alpha;
            pixel.0[i] = (top + under[i] as f32 * (1.0 - alpha)).round().clamp(0.0, 255.0) as u8;
        }
        pixel.0[3] = 255;
    }
}

/// `src` achicada para que su lado largo no pase de `max` (igual si ya cabe).
pub fn shrink(src: &RgbaImage, max: u32) -> RgbaImage {
    let (w, h) = src.dimensions();
    let long = w.max(h).max(1);
    if long <= max {
        return src.clone();
    }
    let fit = max as f32 / long as f32;
    let (tw, th) = (((w as f32 * fit).round() as u32).max(1), ((h as f32 * fit).round() as u32).max(1));
    imageops::thumbnail(src, tw, th)
}

/// Dónde se guarda la foto editada: junto a la original, con `suffix`
/// («(editada)») y un número si ya hay una con ese nombre.
pub fn edited_path(original: &Path, dir: &Path, suffix: &str) -> PathBuf {
    let stem = original.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "captura".into());
    let first = dir.join(format!("{stem} {suffix}.png"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| dir.join(format!("{stem} {suffix} {n}.png")))
        .find(|path| !path.exists())
        .unwrap_or(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(padding: f32, ratio: Ratio) -> Style {
        Style { padding, ratio, ..Style::default() }
    }

    #[test]
    fn sin_relleno_ni_proporcion_el_lienzo_es_la_captura() {
        let l = layout((800, 600), &style(0.0, Ratio::Auto));
        assert_eq!(l.canvas, (800, 600));
        assert_eq!(l.at, (0, 0));
    }

    #[test]
    fn el_relleno_sale_del_lado_corto_y_va_a_los_cuatro_lados() {
        let l = layout((800, 600), &style(0.1, Ratio::Auto));
        assert_eq!(l.canvas, (920, 720));
        assert_eq!(l.at, (60, 60));
    }

    #[test]
    fn la_proporcion_agranda_el_lado_que_falta_sin_recortar() {
        // 920×720 a 16:9: falta ancho.
        let l = layout((800, 600), &style(0.1, Ratio::Wide));
        assert_eq!(l.canvas, (1280, 720));
        assert_eq!(l.at, (240, 60));
        // A 9:16: falta alto.
        let l = layout((800, 600), &style(0.1, Ratio::Vertical));
        assert_eq!(l.canvas, (920, 1636));
        assert!(l.at.1 > 60 && l.at.0 == 60);
        // Cuadrado.
        let l = layout((800, 600), &style(0.0, Ratio::Square));
        assert_eq!(l.canvas, (800, 800));
        assert_eq!(l.at, (0, 100));
    }

    #[test]
    fn cada_proporcion_se_cumple_y_la_captura_cabe() {
        for ratio in Ratio::ALL {
            for src in [(1920, 1080), (300, 1200), (640, 640), (1234, 567)] {
                let l = layout(src, &style(0.07, ratio));
                assert!(l.canvas.0 >= src.0 && l.canvas.1 >= src.1);
                assert!(l.at.0 + src.0 <= l.canvas.0 && l.at.1 + src.1 <= l.canvas.1);
                if let Some((rw, rh)) = ratio.dims() {
                    let want = rw as f32 / rh as f32;
                    let got = l.canvas.0 as f32 / l.canvas.1 as f32;
                    assert!((want - got).abs() < 0.01, "{ratio:?} {src:?}: {got}");
                }
            }
        }
    }

    #[test]
    fn la_mascara_de_esquinas_corta_las_puntas_y_deja_el_centro() {
        let (w, h, r) = (100, 80, 20.0);
        assert_eq!(corner_coverage(0, 0, w, h, r), 0.0);
        assert_eq!(corner_coverage(w - 1, h - 1, w, h, r), 0.0);
        assert_eq!(corner_coverage(50, 40, w, h, r), 1.0);
        // En los bordes rectos, entera.
        assert_eq!(corner_coverage(50, 0, w, h, r), 1.0);
        assert_eq!(corner_coverage(0, 40, w, h, r), 1.0);
        // Dentro de la curva, entera; en el borde de la curva, parcial.
        assert_eq!(corner_coverage(15, 15, w, h, r), 1.0);
        let edge = corner_coverage(5, 5, w, h, r);
        assert!(edge >= 0.0 && edge < 1.0);
        // Sin radio, todo entero.
        assert_eq!(corner_coverage(0, 0, w, h, 0.0), 1.0);
    }

    #[test]
    fn compone_con_fondo_transparente_y_esquinas() {
        let src = RgbaImage::from_pixel(40, 30, Rgba([10, 200, 30, 255]));
        let s = Style { background: Background::None, padding: 0.0, radius: 0.1, shadow: 0.0, ratio: Ratio::Auto };
        let out = compose(&src, &s);
        assert_eq!(out.dimensions(), (40, 30));
        assert_eq!(out.get_pixel(0, 0).0[3], 0, "la punta queda transparente");
        assert_eq!(*out.get_pixel(20, 15), Rgba([10, 200, 30, 255]));
    }

    #[test]
    fn compone_con_fondo_solido_relleno_y_sombra() {
        let src = RgbaImage::from_pixel(100, 100, Rgba([255, 255, 255, 255]));
        let s = Style { background: Background::Solid(0x336699), padding: 0.2, radius: 0.0, shadow: 1.0, ratio: Ratio::Auto };
        let out = compose(&src, &s);
        assert_eq!(out.dimensions(), (140, 140));
        assert_eq!(*out.get_pixel(0, 0), Rgba([0x33, 0x66, 0x99, 255]), "la esquina del lienzo es el fondo");
        assert_eq!(*out.get_pixel(70, 70), Rgba([255, 255, 255, 255]), "la captura encima");
        // Bajo la captura, la sombra oscurece el fondo.
        let under = out.get_pixel(70, 122);
        assert!(under.0[2] < 0x99, "sin sombra: {under:?}");
    }

    #[test]
    fn el_degradado_va_de_un_color_al_otro() {
        let src = RgbaImage::from_pixel(10, 10, Rgba([0, 0, 0, 255]));
        let s = Style { background: Background::Gradient(0x000000, 0xffffff), padding: 0.3, radius: 0.0, shadow: 0.0, ratio: Ratio::Auto };
        let out = compose(&src, &s);
        let (w, h) = out.dimensions();
        assert!(out.get_pixel(0, 0).0[0] < 30);
        assert!(out.get_pixel(w - 1, h - 1).0[0] > 225);
    }

    #[test]
    fn el_fondo_de_imagen_cubre_el_lienzo() {
        let src = RgbaImage::from_pixel(64, 48, Rgba([0, 0, 0, 255]));
        let id = crate::recording_editor::wallpapers::ids().next().unwrap();
        let s = Style { background: Background::Wallpaper(id), padding: 0.25, radius: 0.0, shadow: 0.0, ratio: Ratio::Square };
        let out = compose(&src, &s);
        assert_eq!(out.dimensions(), (88, 88));
        assert_eq!(out.get_pixel(0, 0).0[3], 255);
    }

    #[test]
    fn el_estilo_se_guarda_y_se_lee_y_se_acota() {
        let s = Style { background: Background::Wallpaper(11), padding: 0.12, radius: 0.05, shadow: 0.3, ratio: Ratio::Classic };
        let back: Style = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        // Un archivo viejo o a medias toma lo de siempre en lo que falta.
        let partial: Style = serde_json::from_str(r#"{"padding": 9.0}"#).unwrap();
        assert_eq!(partial.clamped().padding, PADDING_MAX);
        assert_eq!(partial.ratio, Ratio::Auto);
    }

    #[test]
    fn el_nombre_de_la_editada_no_pisa_otra() {
        let dir = std::env::temp_dir().join(format!("atic-photo-editor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let original = dir.join("Captura 1.png");
        let first = edited_path(&original, &dir, "(editada)");
        assert_eq!(first, dir.join("Captura 1 (editada).png"));
        std::fs::write(&first, b"x").unwrap();
        assert_eq!(edited_path(&original, &dir, "(editada)"), dir.join("Captura 1 (editada) 2.png"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn el_damero_queda_solo_detras_de_lo_transparente() {
        let mut image = RgbaImage::new(4, 2);
        image.put_pixel(3, 1, Rgba([10, 20, 30, 255]));
        checker_under(&mut image, 2, [200; 3], [100; 3]);
        assert_eq!(*image.get_pixel(0, 0), Rgba([200, 200, 200, 255]));
        assert_eq!(*image.get_pixel(2, 0), Rgba([100, 100, 100, 255]));
        assert_eq!(*image.get_pixel(3, 1), Rgba([10, 20, 30, 255]), "lo opaco no cambia");
    }

    #[test]
    fn achicar_respeta_la_proporcion() {
        let src = RgbaImage::new(2000, 1000);
        assert_eq!(shrink(&src, 500).dimensions(), (500, 250));
        assert_eq!(shrink(&src, 4000).dimensions(), (2000, 1000));
    }
}
