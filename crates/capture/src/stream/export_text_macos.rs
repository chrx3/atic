//! Los textos del exportado: un `CIImage` ya ubicado en el cuadro (con su píldora, si
//! lleva). El texto se parte en líneas con CoreText (`CTTypesetter`, la capa sobre la
//! que corre `CTFramesetter`), se mide y se dibuja en un `CGBitmapContext` del tamaño
//! justo del bloque —no de un cuadro entero—, porque pueden ser cientos de subtítulos.

use std::ffi::c_void;

use core_foundation::base::{CFType, TCFType};
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::{CFString, CFStringRef};
use objc2::encode::{Encode, Encoding, RefEncode};
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};

use super::export_macos::{TextAt, TextSpec};

#[link(name = "CoreText", kind = "framework")]
extern "C" {
    static kCTFontAttributeName: CFStringRef;
    static kCTForegroundColorAttributeName: CFStringRef;
    fn CTFontCreateUIFontForLanguage(ui_type: i32, size: f64, language: *const c_void) -> *const c_void;
    fn CTFontCreateWithName(name: CFStringRef, size: f64, matrix: *const c_void) -> *const c_void;
    fn CTTypesetterCreateWithAttributedString(string: *const c_void) -> *const c_void;
    fn CTTypesetterSuggestLineBreak(typesetter: *const c_void, start: isize, width: f64) -> isize;
    fn CTTypesetterCreateLine(typesetter: *const c_void, range: CFRange) -> *const c_void;
    fn CTLineGetTypographicBounds(line: *const c_void, ascent: *mut f64, descent: *mut f64, leading: *mut f64) -> f64;
    fn CTLineGetTrailingWhitespaceWidth(line: *const c_void) -> f64;
    fn CTLineDraw(line: *const c_void, context: *mut c_void);
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: *const c_void);
    fn CFAttributedStringCreate(allocator: *const c_void, string: CFStringRef, attributes: *const c_void) -> *const c_void;
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    static kCGColorSpaceSRGB: CFStringRef;
    fn CGColorSpaceCreateWithName(name: CFStringRef) -> *mut c_void;
    fn CGColorCreate(space: *const c_void, components: *const f64) -> *const c_void;
    fn CGBitmapContextCreate(data: *mut c_void, width: usize, height: usize, bits: usize, bytes_per_row: usize, space: *const c_void, info: u32) -> *mut c_void;
    fn CGBitmapContextCreateImage(context: *mut c_void) -> *mut CGImage;
    fn CGContextSetRGBFillColor(context: *mut c_void, r: f64, g: f64, b: f64, a: f64);
    fn CGContextSetShouldSmoothFonts(context: *mut c_void, should: bool);
    fn CGContextSetShadowWithColor(context: *mut c_void, offset: CGSize, blur: f64, color: *const c_void);
    fn CGContextSetTextPosition(context: *mut c_void, x: f64, y: f64);
    fn CGContextAddPath(context: *mut c_void, path: *const c_void);
    fn CGContextFillPath(context: *mut c_void);
    fn CGPathCreateWithRoundedRect(rect: CGRect, corner_width: f64, corner_height: f64, transform: *const c_void) -> *const c_void;
}

#[link(name = "CoreImage", kind = "framework")]
extern "C" {}

/// `CFRange`.
#[repr(C)]
#[derive(Clone, Copy)]
struct CFRange {
    location: isize,
    length: isize,
}

/// Un `CGImage` (solo para pasarlo a Core Image).
#[repr(C)]
struct CGImage {
    _private: [u8; 0],
}
// SAFETY: la codificación del puntero de C (`^{CGImage=}`).
unsafe impl RefEncode for CGImage {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("CGImage", &[]));
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGSize {
    width: f64,
    height: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct Affine {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    tx: f64,
    ty: f64,
}
// SAFETY: la misma disposición que `CGAffineTransform`.
unsafe impl Encode for Affine {
    const ENCODING: Encoding = Encoding::Struct(
        "CGAffineTransform",
        &[f64::ENCODING, f64::ENCODING, f64::ENCODING, f64::ENCODING, f64::ENCODING, f64::ENCODING],
    );
}

impl Affine {
    fn translate(tx: f64, ty: f64) -> Self {
        Self { a: 1.0, b: 0.0, c: 0.0, d: 1.0, tx, ty }
    }
}

/// `kCGImageAlphaPremultipliedFirst | kCGBitmapByteOrder32Little`: BGRA premultiplicado.
const BGRA_PREMULTIPLIED: u32 = 2 | (2 << 12);
/// `kCTFontUIFontSystem` y `kCTFontUIFontEmphasizedSystem`.
const UI_FONT_SYSTEM: i32 = 2;
const UI_FONT_EMPHASIZED: i32 = 3;

/// La píldora: oscura y casi opaca.
const PILL: [f64; 4] = [0.07, 0.07, 0.07, 0.82];

/// Suelta un objeto de CoreFoundation retenido al salir del alcance.
struct Released(*const c_void);
impl Drop for Released {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: se creó o copió con +1 y se suelta una vez.
            unsafe { CFRelease(self.0) };
        }
    }
}

// --- Medidas (puro) ------------------------------------------------------------------------

/// El tamaño de la letra en píxeles: la base (`alto × 0,045`, entre 18 y 96) por `size`.
fn font_px(out_h: f64, size: f64) -> f64 {
    let size = if size.is_finite() && size > 0.0 { size } else { 1.0 };
    ((out_h * 0.045).clamp(18.0, 96.0) * size).max(4.0)
}

/// Cuánto ancho del cuadro puede ocupar el bloque entero: 90 %; 80 % en un subtítulo.
fn max_block_width(out_w: f64, caption: bool) -> f64 {
    out_w * if caption { 0.8 } else { 0.9 }
}

/// El relleno de la píldora a cada lado y arriba/abajo.
fn pill_padding(font_px: f64) -> (f64, f64) {
    (font_px * 0.7, font_px * 0.4)
}

/// Cuánto ancho le queda al texto dentro del bloque (sin el relleno de la píldora).
fn max_text_width(out_w: f64, caption: bool, boxed: bool, font_px: f64) -> f64 {
    let block = max_block_width(out_w, caption);
    let width = if boxed { block - pill_padding(font_px).0 * 2.0 } else { block };
    width.max(font_px * 2.0)
}

/// El radio de las esquinas: píldora entera con una línea; más suave con varias.
fn pill_radius(h: f64, font_px: f64, lines: usize) -> f64 {
    if lines <= 1 {
        h / 2.0
    } else {
        (font_px * 0.8).min(h / 2.0)
    }
}

/// Dónde cae el borde de abajo de un bloque de alto `h` en un cuadro de `out_h` (el origen
/// de Core Image es abajo): el margen de siempre; un subtítulo abajo lleva un margen
/// mayor (sobre el borde) y arriba o al centro va como cualquier texto.
fn block_y(at: TextAt, caption: bool, out_h: f64, h: f64) -> f64 {
    let margin = out_h * 0.07;
    let y = match at {
        TextAt::Top => out_h - margin - h,
        TextAt::Center => (out_h - h) / 2.0,
        TextAt::Bottom if caption => out_h * 0.09,
        TextAt::Bottom => margin,
    };
    y.max(0.0)
}

/// Una línea ya partida y medida.
struct Line {
    line: Released,
    /// Lo que mide sin el espacio con el que termina (al partir en un espacio).
    width: f64,
    ascent: f64,
    height: f64,
}

/// El texto de `spec` dibujado en un cuadro de `out_w × out_h` (el origen de Core Image
/// es abajo a la izquierda). Parte el texto en líneas con tope de ancho.
pub(super) unsafe fn text_image(spec: &TextSpec, out_w: f64, out_h: f64) -> Result<Retained<AnyObject>, String> {
    let font_px = font_px(out_h, spec.size);
    let text_max = max_text_width(out_w, spec.caption, spec.boxed, font_px);

    // La fuente y el color del texto.
    let font = unsafe { CTFontCreateUIFontForLanguage(if spec.bold { UI_FONT_EMPHASIZED } else { UI_FONT_SYSTEM }, font_px, std::ptr::null()) };
    let font = if font.is_null() {
        let name = CFString::new(if spec.bold { "Helvetica-Bold" } else { "Helvetica" });
        unsafe { CTFontCreateWithName(name.as_concrete_TypeRef(), font_px, std::ptr::null()) }
    } else {
        font
    };
    if font.is_null() {
        return Err("no se encontró una fuente para el texto".to_string());
    }
    let _font = Released(font);
    let space = unsafe { CGColorSpaceCreateWithName(kCGColorSpaceSRGB) };
    if space.is_null() {
        return Err("sin espacio de color sRGB".to_string());
    }
    let _space = Released(space as *const c_void);
    let [r, g, b] = spec.color;
    let components = [r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0), 1.0];
    let color = unsafe { CGColorCreate(space, components.as_ptr()) };
    if color.is_null() {
        return Err("no se armó el color del texto".to_string());
    }
    let _color = Released(color);
    // SAFETY: constantes de CoreText; `font` y `color` viven hasta el final de la función.
    let attributes = CFDictionary::from_CFType_pairs(&[
        (unsafe { CFString::wrap_under_get_rule(kCTFontAttributeName) }, unsafe { CFType::wrap_under_get_rule(font) }),
        (unsafe { CFString::wrap_under_get_rule(kCTForegroundColorAttributeName) }, unsafe { CFType::wrap_under_get_rule(color) }),
    ] as &[(CFString, CFType)]);

    // Las líneas: cada párrafo se parte al ancho tope.
    let mut lines: Vec<Line> = Vec::new();
    for paragraph in spec.text.split('\n') {
        let paragraph = paragraph.trim_end_matches('\r');
        if paragraph.trim().is_empty() {
            continue;
        }
        let string = CFString::new(paragraph);
        let attributed = unsafe { CFAttributedStringCreate(std::ptr::null(), string.as_concrete_TypeRef(), attributes.as_concrete_TypeRef() as *const c_void) };
        if attributed.is_null() {
            return Err("no se armó el texto".to_string());
        }
        let _attributed = Released(attributed);
        let typesetter = unsafe { CTTypesetterCreateWithAttributedString(attributed) };
        if typesetter.is_null() {
            return Err("CoreText no armó el texto".to_string());
        }
        let _typesetter = Released(typesetter);
        let length = string.char_len();
        let mut start = 0isize;
        while start < length {
            let count = unsafe { CTTypesetterSuggestLineBreak(typesetter, start, text_max) };
            // Un corte inválido no puede dejar el bucle sin avanzar.
            let count = if count <= 0 { length - start } else { count.min(length - start) };
            let line = unsafe { CTTypesetterCreateLine(typesetter, CFRange { location: start, length: count }) };
            if line.is_null() {
                return Err("CoreText no armó una línea".to_string());
            }
            let line = Released(line);
            let (mut ascent, mut descent, mut leading) = (0.0f64, 0.0f64, 0.0f64);
            let full = unsafe { CTLineGetTypographicBounds(line.0, &mut ascent, &mut descent, &mut leading) };
            let width = (full - unsafe { CTLineGetTrailingWhitespaceWidth(line.0) }).max(0.0);
            lines.push(Line { line, width, ascent: ascent.ceil(), height: (ascent + descent + leading).ceil() });
            start += count;
        }
    }
    if lines.is_empty() {
        // Sin texto no hay nada que dibujar.
        let empty: Retained<AnyObject> = unsafe { msg_send![class(c"CIImage")?, emptyImage] };
        return Ok(empty);
    }

    // El bloque: el texto más el relleno de la píldora. Sin píldora, un margen para la sombra.
    let text_w = lines.iter().map(|l| l.width).fold(0.0, f64::max);
    let text_h: f64 = lines.iter().map(|l| l.height).sum();
    let (pad_x, pad_y) = if spec.boxed { pill_padding(font_px) } else { (0.0, 0.0) };
    let (box_w, box_h) = (text_w + pad_x * 2.0, text_h + pad_y * 2.0);
    let halo = if spec.boxed { 2.0 } else { (font_px * 0.25).ceil() };
    let img_w = (box_w + halo * 2.0).ceil().clamp(1.0, 16_384.0);
    let img_h = (box_h + halo * 2.0).ceil().clamp(1.0, 16_384.0);

    let context = unsafe { CGBitmapContextCreate(std::ptr::null_mut(), img_w as usize, img_h as usize, 8, 0, space, BGRA_PREMULTIPLIED) };
    if context.is_null() {
        return Err("no se pudo crear el lienzo del texto".to_string());
    }
    let _context = Released(context as *const c_void);
    unsafe { CGContextSetShouldSmoothFonts(context, false) };

    // Origen de las medidas dentro del lienzo: el bloque centrado.
    let (bx, by) = ((img_w - box_w) / 2.0, (img_h - box_h) / 2.0);
    if spec.boxed {
        let radius = pill_radius(box_h, font_px, lines.len()).min(box_w / 2.0).min(box_h / 2.0);
        let rect = CGRect { origin: CGPoint { x: bx, y: by }, size: CGSize { width: box_w, height: box_h } };
        let path = unsafe { CGPathCreateWithRoundedRect(rect, radius, radius, std::ptr::null()) };
        if !path.is_null() {
            let _path = Released(path);
            unsafe {
                CGContextSetRGBFillColor(context, PILL[0], PILL[1], PILL[2], PILL[3]);
                CGContextAddPath(context, path);
                CGContextFillPath(context);
            }
        }
    } else {
        // Sin píldora, una sombra suave detrás de las letras para que se lean.
        let shadow = [0.0, 0.0, 0.0, 0.6];
        let shadow_color = unsafe { CGColorCreate(space, shadow.as_ptr()) };
        if !shadow_color.is_null() {
            let _shadow_color = Released(shadow_color);
            unsafe { CGContextSetShadowWithColor(context, CGSize { width: 0.0, height: -font_px * 0.06 }, font_px * 0.2, shadow_color) };
        }
    }

    // Las líneas, de arriba abajo, cada una centrada.
    let mut top = by + box_h - pad_y;
    for line in &lines {
        let x = bx + pad_x + (text_w - line.width) / 2.0;
        unsafe {
            CGContextSetTextPosition(context, x, top - line.ascent);
            CTLineDraw(line.line.0, context);
        }
        top -= line.height;
    }

    let image = unsafe { CGBitmapContextCreateImage(context) };
    if image.is_null() {
        return Err("no se pudo crear la imagen del texto".to_string());
    }
    let _image = Released(image as *const c_void);
    let text: Option<Retained<AnyObject>> = unsafe { msg_send![class(c"CIImage")?, imageWithCGImage: image] };
    let text = text.ok_or("Core Image no tomó la imagen del texto")?;

    // En el cuadro: centrado, con la altura que toca; en píxeles enteros para no desenfocar.
    let x = ((out_w - img_w) / 2.0).round();
    let y = (block_y(spec.at, spec.caption, out_h, box_h) - halo).round();
    let placed: Retained<AnyObject> = unsafe { msg_send![&text, imageByApplyingTransform: Affine::translate(x, y)] };
    Ok(placed)
}

fn class(name: &std::ffi::CStr) -> Result<&'static AnyClass, String> {
    AnyClass::get(name).ok_or_else(|| format!("sin {}", name.to_string_lossy()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn tamano_de_letra_acotado_antes_de_multiplicar() {
        assert!(close(font_px(1080.0, 1.0), 48.6));
        // Cuadros chicos y enormes se acotan a 18–96 antes de aplicar `size`.
        assert!(close(font_px(300.0, 1.0), 18.0));
        assert!(close(font_px(3000.0, 1.0), 96.0));
        assert!(close(font_px(300.0, 2.0), 36.0));
        assert!(close(font_px(3000.0, 2.0), 192.0));
        assert!(close(font_px(1080.0, 0.5), 24.3));
    }

    #[test]
    fn tamano_invalido_vuelve_a_uno() {
        assert!(close(font_px(1080.0, 0.0), 48.6));
        assert!(close(font_px(1080.0, -3.0), 48.6));
        assert!(close(font_px(1080.0, f64::NAN), 48.6));
        // Nunca queda ilegible de chico.
        assert!(close(font_px(100.0, 0.01), 4.0));
    }

    #[test]
    fn ancho_maximo_del_bloque_y_del_texto() {
        assert!(close(max_block_width(1920.0, false), 1728.0));
        assert!(close(max_block_width(1920.0, true), 1536.0));
        // Con píldora, el relleno de los dos lados sale del ancho del texto.
        let f = 48.6;
        assert!(close(max_text_width(1920.0, false, true, f), 1728.0 - 2.0 * 0.7 * f));
        assert!(close(max_text_width(1920.0, true, false, f), 1536.0));
        // Un cuadro diminuto no deja un ancho de cero.
        assert!(close(max_text_width(10.0, false, true, 20.0), 40.0));
    }

    #[test]
    fn ubicacion_arriba_centro_abajo_y_subtitulo() {
        let (out_h, h) = (1080.0, 100.0);
        assert!(close(block_y(TextAt::Bottom, false, out_h, h), 75.6));
        assert!(close(block_y(TextAt::Top, false, out_h, h), 1080.0 - 75.6 - 100.0));
        assert!(close(block_y(TextAt::Center, false, out_h, h), 490.0));
        // El subtítulo abajo lleva un margen mayor; arriba o al centro, como cualquier texto.
        assert!(close(block_y(TextAt::Bottom, true, out_h, h), 97.2));
        assert!(close(block_y(TextAt::Top, true, out_h, h), 1080.0 - 75.6 - 100.0));
        assert!(close(block_y(TextAt::Center, true, out_h, h), 490.0));
        // Un bloque más alto que el cuadro no sale con y negativa.
        assert!(close(block_y(TextAt::Top, false, 100.0, 500.0), 0.0));
    }

    #[test]
    fn radio_de_la_pildora() {
        assert!(close(pill_radius(80.0, 40.0, 1), 40.0));
        // Con varias líneas, un radio menor que la mitad del alto.
        assert!(close(pill_radius(200.0, 40.0, 3), 32.0));
        assert!(close(pill_radius(50.0, 40.0, 2), 25.0));
    }

    #[test]
    fn relleno_de_la_pildora_como_siempre() {
        let (x, y) = pill_padding(50.0);
        assert!(close(x, 35.0));
        assert!(close(y, 20.0));
    }
}
