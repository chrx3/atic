//! El compositor del editor (Windows): lo que pinta la vista previa y la exportación sobre cada
//! cuadro del video, con Direct2D en la misma GPU que el reproductor (`player_win.rs`).
//!
//! El mismo contrato que `Composer` de macOS (`export_macos.rs`): `set_scene` fija lo que no
//! cambia entre cuadros (el marco, el fondo con su sombra y el tamaño de la salida) y `render`
//! dibuja un cuadro según su [`Decor`]: la cámara (recorte y zoom) con las esquinas del marco, las
//! zonas difuminadas, el desenfoque de movimiento, el cursor, las ondas de los clics y los
//! textos. Las formas y los números son los de macOS (`types.rs` y `sprite.rs`).

use std::collections::HashMap;
use std::mem::ManuallyDrop;
use std::path::Path;

use windows::core::{w, Interface};
use windows_numerics::{Matrix3x2, Vector2};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_IGNORE, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_BORDER_MODE_HARD, D2D1_COLOR_F, D2D1_COMPOSITE_MODE_SOURCE_OVER, D2D1_GRADIENT_STOP,
    D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    CLSID_D2D1DirectionalBlur, CLSID_D2D1GaussianBlur, CLSID_D2D1Scale, D2D1_SCALE_INTERPOLATION_MODE, D2D1_SCALE_INTERPOLATION_MODE_LINEAR, D2D1_SCALE_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_SCALE_PROP_INTERPOLATION_MODE, D2D1_SCALE_PROP_SCALE, D2D1CreateDevice, ID2D1Bitmap1, ID2D1CommandList, ID2D1Device, ID2D1DeviceContext, ID2D1Effect,
    ID2D1Geometry, ID2D1Image, ID2D1RoundedRectangleGeometry, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BUFFER_PRECISION_8BPC_UNORM, D2D1_COLOR_INTERPOLATION_MODE_PREMULTIPLIED, D2D1_COLOR_SPACE_SRGB, D2D1_BITMAP_OPTIONS_NONE, D2D1_BITMAP_OPTIONS_TARGET,
    D2D1_BITMAP_PROPERTIES1, D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC,
    D2D1_INTERPOLATION_MODE_LINEAR, D2D1_LAYER_OPTIONS1_NONE, D2D1_LAYER_PARAMETERS1, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_PROPERTY_TYPE_UNKNOWN,
    D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::Direct3D11::{ID3D11Texture2D, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteFontCollection, IDWriteTextFormat, IDWriteTextLayout, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_METRICS,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{IDXGIDevice, IDXGISurface};

use super::gfx_win::Gfx;
use super::info_win::win;
use super::player_win::HardwareFrame;
use super::sprite;
use super::types::{
    blur_plan, click_draw, content_rect, frame_margin, shadow_params, Backdrop, ClickDraw, ClickKind, Decor, FrameSpec, Rgb, Scene, TextAt, TextSpec,
};

/// El color de las píldoras de los textos (como `PILL` de macOS).
const PILL: [f32; 4] = [0.07, 0.07, 0.07, 0.82];

/// Un cuadro compuesto: la textura BGRA de la GPU con las medidas de `Scene::out`.
#[derive(Clone)]
pub struct Composed {
    texture: ID3D11Texture2D,
    serial: u64,
    size: (u32, u32),
}

impl PartialEq for Composed {
    fn eq(&self, other: &Self) -> bool {
        self.serial == other.serial && self.texture == other.texture
    }
}

impl Composed {
    pub fn width(&self) -> u32 {
        self.size.0
    }

    pub fn height(&self) -> u32 {
        self.size.1
    }

    /// Los píxeles BGRA (alfa premultiplicado, opacos) arriba a la izquierda primero.
    pub fn bgra(&self) -> Result<crate::Frame, String> {
        let gfx = Gfx::shared()?;
        let (w, h, bytes) = gfx.read(&self.texture)?;
        Ok(crate::Frame::new(crate::Rect::new(0, 0, w, h), bytes))
    }

    /// La textura de la GPU (para el exportador).
    pub(super) fn texture(&self) -> &ID3D11Texture2D {
        &self.texture
    }
}

/// La salida: una textura con su mapa de bits de Direct2D, de las medidas de la escena.
struct Target {
    texture: ID3D11Texture2D,
    bitmap: ID2D1Bitmap1,
    size: (u32, u32),
}

pub struct Composer {
    gfx: Gfx,
    _device: ID2D1Device,
    ctx: ID2D1DeviceContext,
    text: IDWriteFactory,
    scene: Option<Scene>,
    target: Option<Target>,
    /// El fondo con su sombra, ya pintado (de la geometría de la escena).
    base: Option<ID2D1Bitmap1>,
    /// Dónde va el video en el cuadro: `[x, y, ancho, alto]`, origen arriba a la izquierda.
    rect: [f64; 4],
    /// Radio de las esquinas del video, en píxeles.
    radius: f64,
    /// El mapa de bits del cuadro del reproductor (se arma una vez por textura).
    source: Option<(ID3D11Texture2D, ID2D1Bitmap1)>,
    /// Intermedio para el desenfoque de movimiento (del tamaño del video en el cuadro).
    scratch: Option<(ID2D1Bitmap1, (u32, u32))>,
    /// El video ya con el desenfoque de movimiento, para el de zoom.
    scratch2: Option<(ID2D1Bitmap1, (u32, u32))>,
    sprites: HashMap<(u8, u8), ID2D1Bitmap1>,
    serial: u64,
}

fn rect(left: f64, top: f64, right: f64, bottom: f64) -> D2D_RECT_F {
    D2D_RECT_F { left: left as f32, top: top as f32, right: right as f32, bottom: bottom as f32 }
}

fn color(rgb: Rgb, alpha: f64) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: rgb[0] as f32, g: rgb[1] as f32, b: rgb[2] as f32, a: alpha as f32 }
}

fn matrix(sx: f64, sy: f64, tx: f64, ty: f64) -> Matrix3x2 {
    Matrix3x2 { M11: sx as f32, M12: 0.0, M21: 0.0, M22: sy as f32, M31: tx as f32, M32: ty as f32 }
}

const IDENTITY: Matrix3x2 = Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: 0.0, M32: 0.0 };

fn bitmap_properties(options: windows::Win32::Graphics::Direct2D::D2D1_BITMAP_OPTIONS, ignore_alpha: bool) -> D2D1_BITMAP_PROPERTIES1 {
    D2D1_BITMAP_PROPERTIES1 {
        pixelFormat: D2D1_PIXEL_FORMAT {
            format: DXGI_FORMAT_B8G8R8A8_UNORM,
            alphaMode: if ignore_alpha { D2D1_ALPHA_MODE_IGNORE } else { D2D1_ALPHA_MODE_PREMULTIPLIED },
        },
        dpiX: 96.0,
        dpiY: 96.0,
        bitmapOptions: options,
        colorContext: ManuallyDrop::new(None),
    }
}

/// Una textura de la GPU para dibujar encima y leer después.
fn make_texture(gfx: &Gfx, (w, h): (u32, u32)) -> Result<ID3D11Texture2D, String> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: w,
        Height: h,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut texture: Option<ID3D11Texture2D> = None;
    // SAFETY: crea una textura con el dispositivo compartido.
    unsafe { gfx.device.CreateTexture2D(&desc, None, Some(&mut texture)) }.map_err(win)?;
    texture.ok_or_else(|| "Direct3D no creó la textura".into())
}

impl Composer {
    pub fn new() -> Result<Self, String> {
        let gfx = Gfx::shared()?;
        // SAFETY: crea el dispositivo y el contexto de Direct2D sobre el de Direct3D.
        unsafe {
            let dxgi: IDXGIDevice = gfx.device.cast().map_err(win)?;
            let device = D2D1CreateDevice(&dxgi, None).map_err(|e| format!("Direct2D no arrancó: {}", win(e)))?;
            let ctx = device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE).map_err(win)?;
            let text: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).map_err(|e| format!("DirectWrite no arrancó: {}", win(e)))?;
            Ok(Self {
                gfx,
                _device: device,
                ctx,
                text,
                scene: None,
                target: None,
                base: None,
                rect: [0.0; 4],
                radius: 0.0,
                source: None,
                scratch: None,
                scratch2: None,
                sprites: HashMap::new(),
                serial: 0,
            })
        }
    }

    /// La escena que se pinta de ahora en adelante: rearma la salida, el marco y el fondo solo si
    /// cambiaron; con la misma escena no hace nada.
    pub fn set_scene(&mut self, scene: &Scene) -> Result<(), String> {
        if self.scene.as_ref() == Some(scene) {
            return Ok(());
        }
        let valid = |v: f64| v.is_finite() && v >= 1.0;
        if scene.out.0 < 2 || scene.out.1 < 2 || !valid(scene.src.0) || !valid(scene.src.1) {
            return Err(format!("escena inválida: video {:?}, cuadro {:?}", scene.src, scene.out));
        }
        let same_geometry = self.base.is_some() && self.scene.as_ref().is_some_and(|old| old.src == scene.src && old.out == scene.out && old.frame == scene.frame);
        if self.target.as_ref().is_none_or(|t| t.size != scene.out) {
            let texture = make_texture(&self.gfx, scene.out)?;
            // SAFETY: mapa de bits de Direct2D sobre la superficie de la textura.
            let bitmap = unsafe {
                let surface: IDXGISurface = texture.cast().map_err(win)?;
                self.ctx
                    .CreateBitmapFromDxgiSurface(&surface, Some(&bitmap_properties(D2D1_BITMAP_OPTIONS_TARGET, false)))
                    .map_err(|e| format!("no se pudo usar la textura de salida: {}", win(e)))?
            };
            self.target = Some(Target { texture, bitmap, size: scene.out });
        }
        if !same_geometry {
            self.build_base(scene)?;
        }
        self.scene = Some(scene.clone());
        Ok(())
    }

    /// El cuadro `source` (lo que entrega `VideoPlayer::frame()`) compuesto según `decor`.
    pub fn render(&mut self, source: &HardwareFrame, decor: &Decor) -> Result<Composed, String> {
        let scene = self.scene.clone().ok_or("sin escena")?;
        let src_bitmap = self.source_bitmap(source)?;
        let target = self.target.as_ref().ok_or("sin salida")?;
        let (target_bitmap, target_texture) = (target.bitmap.clone(), target.texture.clone());
        // SAFETY: dibuja en la textura de salida; `BeginDraw`/`EndDraw` enmarcan todo.
        let drawn = unsafe {
            self.ctx.SetTarget(&target_bitmap);
            self.ctx.BeginDraw();
            let result = self.draw(&scene, decor, &src_bitmap, &target_bitmap);
            let end = self.ctx.EndDraw(None, None);
            self.ctx.SetTarget(None::<&ID2D1Image>);
            result.and(end.map_err(|e| format!("Direct2D no terminó el cuadro: {}", win(e))))
        };
        drawn?;
        self.serial += 1;
        Ok(Composed { texture: target_texture, serial: self.serial, size: scene.out })
    }

    /// El mapa de bits de Direct2D del cuadro del reproductor.
    fn source_bitmap(&mut self, frame: &HardwareFrame) -> Result<ID2D1Bitmap1, String> {
        if let Some((texture, bitmap)) = self.source.as_ref() {
            if *texture == frame.texture {
                return Ok(bitmap.clone());
            }
        }
        // SAFETY: mapa de bits sobre la textura del reproductor (su alfa no vale: se ignora).
        let bitmap = unsafe {
            let surface: IDXGISurface = frame.texture.cast().map_err(win)?;
            self.ctx
                .CreateBitmapFromDxgiSurface(&surface, Some(&bitmap_properties(D2D1_BITMAP_OPTIONS_NONE, true)))
                .map_err(|e| format!("no se pudo leer el cuadro del video: {}", win(e)))?
        };
        self.source = Some((frame.texture.clone(), bitmap.clone()));
        Ok(bitmap)
    }

    /// El fondo, el marco y la sombra de `scene`, pintados una vez en `self.base`.
    fn build_base(&mut self, scene: &Scene) -> Result<(), String> {
        let (out_w, out_h) = (scene.out.0 as f64, scene.out.1 as f64);
        let margin = scene.frame.as_ref().map_or(0.0, |f| frame_margin(f.padding, out_w, out_h));
        // `content_rect` cuenta desde abajo (como Core Image): aquí se cuenta desde arriba.
        let [x, y_up, w, h] = content_rect((out_w, out_h), scene.src, margin);
        self.rect = [x, out_h - y_up - h, w, h];
        self.radius = scene.frame.as_ref().map_or(0.0, |f| f.radius.clamp(0.0, 0.1) * out_h);
        // SAFETY: pinta en un mapa de bits propio.
        unsafe {
            let base = self
                .ctx
                .CreateBitmap(D2D_SIZE_U { width: scene.out.0, height: scene.out.1 }, None, 0, &bitmap_properties(D2D1_BITMAP_OPTIONS_TARGET, false))
                .map_err(win)?;
            // La sombra se arma antes, en su lista de dibujo.
            let shadow = match scene.frame.as_ref().and_then(|f| shadow_params(f.shadow, out_h)) {
                Some((alpha, blur, drop)) => Some((self.shadow_effect(alpha, blur)?, drop)),
                None => None,
            };
            self.ctx.SetTarget(&base);
            self.ctx.BeginDraw();
            self.ctx.Clear(Some(&color([0.0; 3], 1.0)));
            let painted = match scene.frame.as_ref() {
                Some(frame) => self.paint_backdrop(frame, out_w, out_h),
                None => Ok(()),
            };
            if let Some((effect, drop)) = shadow.as_ref() {
                let image: ID2D1Image = effect.cast().map_err(win)?;
                let offset = Vector2 { X: 0.0, Y: *drop as f32 };
                self.ctx.DrawImage(&image, Some(&offset), None, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_COMPOSITE_MODE_SOURCE_OVER);
            }
            let end = self.ctx.EndDraw(None, None);
            self.ctx.SetTarget(None::<&ID2D1Image>);
            painted?;
            end.map_err(|e| format!("Direct2D no pintó el fondo: {}", win(e)))?;
            self.base = Some(base);
        }
        Ok(())
    }

    /// La sombra del video: su silueta difuminada, lista para dibujar (`blur` es la desviación).
    unsafe fn shadow_effect(&self, alpha: f64, blur: f64) -> Result<ID2D1Effect, String> {
        unsafe {
            let list: ID2D1CommandList = self.ctx.CreateCommandList().map_err(win)?;
            self.ctx.SetTarget(&list);
            self.ctx.BeginDraw();
            let brush = self.ctx.CreateSolidColorBrush(&color([0.0; 3], alpha), None).map_err(win)?;
            let shape = D2D1_ROUNDED_RECT {
                rect: rect(self.rect[0], self.rect[1], self.rect[0] + self.rect[2], self.rect[1] + self.rect[3]),
                radiusX: self.radius as f32,
                radiusY: self.radius as f32,
            };
            self.ctx.FillRoundedRectangle(&shape, &brush);
            let end = self.ctx.EndDraw(None, None);
            self.ctx.SetTarget(None::<&ID2D1Image>);
            end.map_err(win)?;
            list.Close().map_err(win)?;
            let effect = self.ctx.CreateEffect(&CLSID_D2D1GaussianBlur).map_err(win)?;
            effect.SetInput(0, &list, true);
            effect.SetValue(0, D2D1_PROPERTY_TYPE_UNKNOWN, &(blur as f32).to_ne_bytes()).map_err(win)?;
            Ok(effect)
        }
    }

    /// El fondo del marco, sobre todo el cuadro.
    unsafe fn paint_backdrop(&self, frame: &FrameSpec, out_w: f64, out_h: f64) -> Result<(), String> {
        unsafe {
            let all = rect(0.0, 0.0, out_w, out_h);
            match &frame.backdrop {
                Backdrop::Solid(rgb) => self.ctx.Clear(Some(&color(*rgb, 1.0))),
                Backdrop::Gradient(a, b) => {
                    let stops = [D2D1_GRADIENT_STOP { position: 0.0, color: color(*a, 1.0) }, D2D1_GRADIENT_STOP { position: 1.0, color: color(*b, 1.0) }];
                    let collection = self.ctx.CreateGradientStopCollection(&stops, D2D1_COLOR_SPACE_SRGB, D2D1_COLOR_SPACE_SRGB, D2D1_BUFFER_PRECISION_8BPC_UNORM, D2D1_EXTEND_MODE_CLAMP, D2D1_COLOR_INTERPOLATION_MODE_PREMULTIPLIED).map_err(win)?;
                    let line = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                        startPoint: Vector2 { X: 0.0, Y: 0.0 },
                        endPoint: Vector2 { X: out_w as f32, Y: out_h as f32 },
                    };
                    let brush = self.ctx.CreateLinearGradientBrush(&line, None, &collection).map_err(win)?;
                    self.ctx.FillRectangle(&all, &brush);
                }
                Backdrop::Image(path) => match load_backdrop(path) {
                    Some((w, h, bgra)) => {
                        let props = bitmap_properties(D2D1_BITMAP_OPTIONS_NONE, false);
                        let bitmap = self
                            .ctx
                            .CreateBitmap(D2D_SIZE_U { width: w, height: h }, Some(bgra.as_ptr() as *const _), w * 4, &props)
                            .map_err(win)?;
                        // Llena el cuadro, recortada a su proporción y centrada.
                        let scale = (out_w / w as f64).max(out_h / h as f64);
                        let (dw, dh) = (w as f64 * scale, h as f64 * scale);
                        let dest = rect((out_w - dw) / 2.0, (out_h - dh) / 2.0, (out_w + dw) / 2.0, (out_h + dh) / 2.0);
                        self.ctx.DrawBitmap(&bitmap, Some(&dest), 1.0, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, None, None);
                    }
                    None => {
                        tracing::warn!(path = %path.display(), "no se pudo leer la imagen de fondo, va un gris oscuro");
                        self.ctx.Clear(Some(&color([0.1, 0.1, 0.11], 1.0)));
                    }
                },
            }
        }
        Ok(())
    }

    /// Un cuadro, ya con el contexto apuntando a la salida y `BeginDraw` hecho.
    unsafe fn draw(&mut self, scene: &Scene, decor: &Decor, frame: &ID2D1Bitmap1, target: &ID2D1Bitmap1) -> Result<(), String> {
        let (src_w, src_h) = scene.src;
        let (out_w, out_h) = (scene.out.0 as f64, scene.out.1 as f64);
        let [rx, ry, cw, ch] = self.rect;
        let look = scene.look;
        // SAFETY: todas son llamadas de dibujo del contexto, entre `BeginDraw` y `EndDraw`.
        unsafe {
            // El fondo ya pintado.
            match self.base.as_ref() {
                Some(base) => self.ctx.DrawBitmap(base, None, 1.0, D2D1_INTERPOLATION_MODE_LINEAR, None, None),
                None => self.ctx.Clear(Some(&color([0.0; 3], 1.0))),
            }

            // La cámara: qué parte del video llena el rectángulo del video.
            let [cx, cy, cwf, chf] = decor.crop;
            let (cwf, chf) = (cwf.clamp(0.05, 1.0), chf.clamp(0.05, 1.0));
            let (left, top) = (cx * src_w, cy * src_h);
            let (sx, sy) = (cw / (cwf * src_w), ch / (chf * src_h));
            let place = |x: f64, y: f64| -> (f64, f64) { (rx + (x - left) * sx, ry + (y - top) * sy) };
            let source_rect = rect(left, top, left + cwf * src_w, top + chf * src_h);
            let content = rect(rx, ry, rx + cw, ry + ch);

            // Las esquinas del marco: todo lo del video va dentro de este recorte.
            let mask: Option<ID2D1RoundedRectangleGeometry> = if self.radius > 0.5 {
                let factory = self.ctx.GetFactory().map_err(win)?;
                let shape = D2D1_ROUNDED_RECT { rect: content, radiusX: self.radius as f32, radiusY: self.radius as f32 };
                Some(factory.CreateRoundedRectangleGeometry(&shape).map_err(win)?)
            } else {
                None
            };
            let clipped = match mask.as_ref() {
                Some(geometry) => {
                    let params = D2D1_LAYER_PARAMETERS1 {
                        contentBounds: rect(f32::MIN as f64, f32::MIN as f64, f32::MAX as f64, f32::MAX as f64),
                        geometricMask: ManuallyDrop::new(Some(geometry.cast::<ID2D1Geometry>().map_err(win)?)),
                        maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                        maskTransform: IDENTITY,
                        opacity: 1.0,
                        opacityBrush: ManuallyDrop::new(None),
                        layerOptions: D2D1_LAYER_OPTIONS1_NONE,
                    };
                    self.ctx.PushLayer(&params, None);
                    drop(ManuallyDrop::into_inner(params.geometricMask));
                    true
                }
                None => {
                    self.ctx.PushAxisAlignedClip(&content, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                    false
                }
            };

            // El video, con desenfoque de movimiento si la cámara se movió lo bastante.
            let plan = decor.blur.and_then(|blur| blur_plan(look.blur_amount, (cw, ch), blur));
            match plan {
                Some(plan) => self.draw_blurred(frame, &source_rect, (cw, ch), plan.pan, plan.zoom, &content, target)?,
                None => self.ctx.DrawBitmap(frame, Some(&content), 1.0, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, Some(&source_rect), None),
            }

            // Las zonas tapadas, sobre el video con su misma cámara.
            for redaction in &decor.redactions {
                self.draw_redaction(frame, redaction.rect, redaction.pixelate, redaction.alpha, (src_w, src_h), (left, top, sx, sy, rx, ry))?;
            }

            // El cursor, con el zoom como el resto.
            if let Some((x, y, size)) = decor.cursor {
                if decor.cursor_alpha > 0.003 {
                    let sprite = sprite::pick(look.cursor_style, decor.cursor_shape);
                    let bitmap = self.sprite_bitmap(look.cursor_style as u8, decor.cursor_shape as u8, sprite)?;
                    let k = scene.cursor_px * size / sprite::reference_height();
                    let (px, py) = place(x * src_w, y * src_h);
                    let scale = k * sx;
                    let dest = rect(
                        px - sprite.hot.0 * scale,
                        py - sprite.hot.1 * scale,
                        px - sprite.hot.0 * scale + sprite.size.0 * scale,
                        py - sprite.hot.1 * scale + sprite.size.1 * scale,
                    );
                    self.ctx.DrawBitmap(&bitmap, Some(&dest), decor.cursor_alpha.clamp(0.0, 1.0) as f32, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, None, None);
                }
            }

            // Los clics, encima del cursor.
            for &(x, y, p) in &decor.ripples {
                let (cxp, cyp) = place(x * src_w, y * src_h);
                if let Some(draw) = click_draw(look.click.kind, look.click.size, p, src_w * sx) {
                    self.draw_click((cxp, cyp), draw, look.click.color)?;
                }
            }
            let _ = ClickKind::None;

            if clipped {
                self.ctx.PopLayer();
            } else {
                self.ctx.PopAxisAlignedClip();
            }

            // Los textos, sin zoom y sin recorte.
            for (index, alpha) in &decor.texts {
                if let Some(spec) = scene.texts.get(*index) {
                    self.draw_text(spec, *alpha, (out_w, out_h))?;
                }
            }
        }
        let _ = target;
        Ok(())
    }

    /// El video con desenfoque de movimiento y de zoom (solo el video: el cursor va nítido). `pan`
    /// es el radio y el ángulo del desenfoque direccional; `zoom`, cuántos píxeles se corre el borde
    /// del cuadro con el zoom (la cantidad de `CIZoomBlur`).
    unsafe fn draw_blurred(
        &mut self,
        frame: &ID2D1Bitmap1,
        source: &D2D_RECT_F,
        (cw, ch): (f64, f64),
        pan: Option<(f64, f64)>,
        zoom: Option<f64>,
        content: &D2D_RECT_F,
        target: &ID2D1Bitmap1,
    ) -> Result<(), String> {
        let size = (cw.round().max(1.0) as u32, ch.round().max(1.0) as u32);
        let make = |ctx: &ID2D1DeviceContext| {
            // SAFETY: crea un mapa de bits de dibujo propio.
            unsafe { ctx.CreateBitmap(D2D_SIZE_U { width: size.0, height: size.1 }, None, 0, &bitmap_properties(D2D1_BITMAP_OPTIONS_TARGET, false)).map_err(win) }
        };
        // SAFETY: dibuja el video en un intermedio, lo difumina y lo pone en su lugar.
        unsafe {
            if self.scratch.as_ref().is_none_or(|(_, s)| *s != size) {
                self.scratch = Some((make(&self.ctx)?, size));
            }
            let scratch = self.scratch.as_ref().map(|(b, _)| b.clone()).ok_or("sin intermedio")?;
            self.ctx.SetTarget(&scratch);
            self.ctx.Clear(Some(&color([0.0; 3], 1.0)));
            let whole = rect(0.0, 0.0, size.0 as f64, size.1 as f64);
            self.ctx.DrawBitmap(frame, Some(&whole), 1.0, D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC, Some(source), None);

            // El desenfoque de movimiento: una desviación un poco menos que el radio de Core Image;
            // el ángulo cuenta hacia abajo.
            let motion: Option<ID2D1Image> = match pan {
                Some((radius, angle)) => {
                    let effect = self.ctx.CreateEffect(&CLSID_D2D1DirectionalBlur).map_err(win)?;
                    effect.SetInput(0, &scratch, true);
                    effect.SetValue(0, D2D1_PROPERTY_TYPE_UNKNOWN, &((radius * 0.45) as f32).to_ne_bytes()).map_err(win)?;
                    effect.SetValue(1, D2D1_PROPERTY_TYPE_UNKNOWN, &((-angle).to_degrees() as f32).to_ne_bytes()).map_err(win)?;
                    Some(effect.cast().map_err(win)?)
                }
                None => None,
            };

            let Some(amount) = zoom else {
                // Solo movimiento: el efecto va directo a su lugar.
                self.ctx.SetTarget(target);
                match motion.as_ref() {
                    Some(image) => {
                        let at = Vector2 { X: content.left, Y: content.top };
                        self.ctx.DrawImage(image, Some(&at), None, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_COMPOSITE_MODE_SOURCE_OVER);
                    }
                    None => self.ctx.DrawBitmap(&scratch, Some(content), 1.0, D2D1_INTERPOLATION_MODE_LINEAR, None, None),
                }
                return Ok(());
            };

            // Con zoom hace falta el cuadro (ya con el movimiento) como mapa de bits: se dibuja
            // varias veces, a escalas apenas distintas alrededor del centro, y cada copia con una
            // opacidad de 1/(n+1) para que el resultado sea el promedio (un desenfoque radial).
            let base = match motion.as_ref() {
                Some(image) => {
                    if self.scratch2.as_ref().is_none_or(|(_, s)| *s != size) {
                        self.scratch2 = Some((make(&self.ctx)?, size));
                    }
                    let blurred = self.scratch2.as_ref().map(|(b, _)| b.clone()).ok_or("sin intermedio")?;
                    self.ctx.SetTarget(&blurred);
                    self.ctx.Clear(Some(&color([0.0; 3], 1.0)));
                    self.ctx.DrawImage(image, None, None, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_COMPOSITE_MODE_SOURCE_OVER);
                    blurred
                }
                None => scratch.clone(),
            };
            self.ctx.SetTarget(target);
            const COPIES: usize = 8;
            // Cuánto cambia la escala para correr `amount` píxeles en la esquina (el punto más lejos del centro).
            let corner = 0.5 * (cw * cw + ch * ch).sqrt().max(1.0);
            let spread = (amount / corner).clamp(0.0, 0.2);
            let (mid_x, mid_y) = (((content.left + content.right) / 2.0) as f64, ((content.top + content.bottom) / 2.0) as f64);
            for i in 0..COPIES {
                // De la más grande a la más chica: la primera cubre el rectángulo entero.
                let t = i as f64 / (COPIES - 1) as f64;
                let k = 1.0 + spread * (0.5 - t);
                let dest = rect(mid_x - cw / 2.0 * k, mid_y - ch / 2.0 * k, mid_x + cw / 2.0 * k, mid_y + ch / 2.0 * k);
                self.ctx.DrawBitmap(&base, Some(&dest), 1.0 / (i as f32 + 1.0), D2D1_INTERPOLATION_MODE_LINEAR, None, None);
            }
        }
        Ok(())
    }

    /// Una zona tapada (difuminada o pixelada) con su fundido.
    unsafe fn draw_redaction(
        &self,
        frame: &ID2D1Bitmap1,
        area: [f64; 4],
        pixelate: bool,
        alpha: f64,
        (src_w, src_h): (f64, f64),
        (left, top, sx, sy, rx, ry): (f64, f64, f64, f64, f64, f64),
    ) -> Result<(), String> {
        if alpha <= 0.003 {
            return Ok(());
        }
        let [x, y, w, h] = area;
        let (x0, y0) = (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0));
        let (x1, y1) = ((x + w).clamp(0.0, 1.0), (y + h).clamp(0.0, 1.0));
        if (x1 - x0) * src_w < 1.0 || (y1 - y0) * src_h < 1.0 {
            return Ok(());
        }
        // SAFETY: dibuja con la cámara del cuadro como transformación y deja el contexto como estaba.
        unsafe {
            let image: ID2D1Image = if pixelate {
                // Bloques: se achica con promedio y se vuelve a agrandar sin suavizar, así cada
                // bloque queda de un solo color (como `CIPixellate` de macOS).
                let block = (src_h * 0.012).max(6.0) as f32;
                let scale = |input: &ID2D1Image, factor: f32, mode: D2D1_SCALE_INTERPOLATION_MODE| -> Result<ID2D1Effect, String> {
                    let effect = self.ctx.CreateEffect(&CLSID_D2D1Scale).map_err(win)?;
                    effect.SetInput(0, input, true);
                    let mut factors = [0u8; 8];
                    factors[..4].copy_from_slice(&factor.to_ne_bytes());
                    factors[4..].copy_from_slice(&factor.to_ne_bytes());
                    effect.SetValue(D2D1_SCALE_PROP_SCALE.0 as u32, D2D1_PROPERTY_TYPE_UNKNOWN, &factors).map_err(win)?;
                    effect.SetValue(D2D1_SCALE_PROP_INTERPOLATION_MODE.0 as u32, D2D1_PROPERTY_TYPE_UNKNOWN, &(mode.0 as u32).to_ne_bytes()).map_err(win)?;
                    Ok(effect)
                };
                let whole: ID2D1Image = frame.cast().map_err(win)?;
                let small = scale(&whole, 1.0 / block, D2D1_SCALE_INTERPOLATION_MODE_LINEAR)?;
                let small_image = small.GetOutput().map_err(win)?;
                scale(&small_image, block, D2D1_SCALE_INTERPOLATION_MODE_NEAREST_NEIGHBOR)?.cast().map_err(win)?
            } else {
                let effect = self.ctx.CreateEffect(&CLSID_D2D1GaussianBlur).map_err(win)?;
                effect.SetInput(0, frame, true);
                effect.SetValue(0, D2D1_PROPERTY_TYPE_UNKNOWN, &((src_h * 0.015).max(8.0) as f32).to_ne_bytes()).map_err(win)?;
                effect.SetValue(2, D2D1_PROPERTY_TYPE_UNKNOWN, &(D2D1_BORDER_MODE_HARD.0 as u32).to_ne_bytes()).map_err(win)?;
                effect.cast().map_err(win)?
            };
            // Del original al cuadro: la misma cámara que el video.
            self.ctx.SetTransform(&matrix(sx, sy, rx - left * sx, ry - top * sy));
            let region = rect(x0 * src_w, y0 * src_h, x1 * src_w, y1 * src_h);
            let params = D2D1_LAYER_PARAMETERS1 {
                contentBounds: region,
                geometricMask: ManuallyDrop::new(None),
                maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                maskTransform: IDENTITY,
                opacity: alpha.clamp(0.0, 1.0) as f32,
                opacityBrush: ManuallyDrop::new(None),
                layerOptions: D2D1_LAYER_OPTIONS1_NONE,
            };
            self.ctx.PushLayer(&params, None);
            self.ctx.PushAxisAlignedClip(&region, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            self.ctx.DrawImage(&image, None, None, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_COMPOSITE_MODE_SOURCE_OVER);
            self.ctx.PopAxisAlignedClip();
            self.ctx.PopLayer();
            self.ctx.SetTransform(&IDENTITY);
        }
        Ok(())
    }

    /// El sprite del cursor como mapa de bits (se arma una vez por estilo y forma).
    unsafe fn sprite_bitmap(&mut self, style: u8, shape: u8, sprite: &sprite::Sprite) -> Result<ID2D1Bitmap1, String> {
        if let Some(bitmap) = self.sprites.get(&(style, shape)) {
            return Ok(bitmap.clone());
        }
        // SAFETY: copia los bytes del sprite a un mapa de bits de la GPU.
        let bitmap = unsafe {
            self.ctx
                .CreateBitmap(
                    D2D_SIZE_U { width: sprite.width, height: sprite.height },
                    Some(sprite.bgra.as_ptr() as *const _),
                    sprite.width * 4,
                    &bitmap_properties(D2D1_BITMAP_OPTIONS_NONE, false),
                )
                .map_err(win)?
        };
        self.sprites.insert((style, shape), bitmap.clone());
        Ok(bitmap)
    }

    /// El efecto de un clic, centrado en `at` del cuadro de salida.
    unsafe fn draw_click(&self, (x, y): (f64, f64), draw: ClickDraw, rgb: Rgb) -> Result<(), String> {
        // SAFETY: dibujo con pinceles creados aquí.
        unsafe {
            match draw {
                ClickDraw::Disk { radius, solid, alpha } => {
                    let stops = [
                        D2D1_GRADIENT_STOP { position: 0.0, color: color(rgb, alpha) },
                        D2D1_GRADIENT_STOP { position: (solid / radius).clamp(0.0, 0.999) as f32, color: color(rgb, alpha) },
                        D2D1_GRADIENT_STOP { position: 1.0, color: color(rgb, 0.0) },
                    ];
                    let collection = self.ctx.CreateGradientStopCollection(&stops, D2D1_COLOR_SPACE_SRGB, D2D1_COLOR_SPACE_SRGB, D2D1_BUFFER_PRECISION_8BPC_UNORM, D2D1_EXTEND_MODE_CLAMP, D2D1_COLOR_INTERPOLATION_MODE_PREMULTIPLIED).map_err(win)?;
                    let props = D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
                        center: Vector2 { X: x as f32, Y: y as f32 },
                        gradientOriginOffset: Vector2 { X: 0.0, Y: 0.0 },
                        radiusX: radius as f32,
                        radiusY: radius as f32,
                    };
                    let brush = self.ctx.CreateRadialGradientBrush(&props, None, &collection).map_err(win)?;
                    let shape = D2D1_ELLIPSE { point: Vector2 { X: x as f32, Y: y as f32 }, radiusX: radius as f32, radiusY: radius as f32 };
                    self.ctx.FillEllipse(&shape, &brush);
                }
                ClickDraw::Ring { radius, thickness, alpha } => {
                    let brush = self.ctx.CreateSolidColorBrush(&color(rgb, alpha), None).map_err(win)?;
                    let middle = (radius - thickness / 2.0).max(0.5) as f32;
                    let shape = D2D1_ELLIPSE { point: Vector2 { X: x as f32, Y: y as f32 }, radiusX: middle, radiusY: middle };
                    self.ctx.DrawEllipse(&shape, &brush, thickness as f32, None);
                }
            }
        }
        Ok(())
    }

    /// Un texto (con su píldora, si lleva) en el cuadro de `out`, con opacidad `alpha`.
    unsafe fn draw_text(&self, spec: &TextSpec, alpha: f64, (out_w, out_h): (f64, f64)) -> Result<(), String> {
        let alpha = alpha.clamp(0.0, 1.0);
        if alpha <= 0.003 || spec.text.is_empty() {
            return Ok(());
        }
        let font_px = ((out_h * 0.045).clamp(18.0, 96.0) * if spec.size.is_finite() && spec.size > 0.0 { spec.size } else { 1.0 }).max(4.0);
        let (pad_x, pad_y) = if spec.boxed { (font_px * 0.7, font_px * 0.4) } else { (0.0, 0.0) };
        let block = out_w * if spec.caption { 0.8 } else { 0.9 };
        let text_max = (if spec.boxed { block - pad_x * 2.0 } else { block }).max(font_px * 2.0);
        let wide: Vec<u16> = spec.text.encode_utf16().collect();
        // SAFETY: DirectWrite mide y el contexto dibuja; los objetos viven hasta el final.
        unsafe {
            let format: IDWriteTextFormat = self
                .text
                .CreateTextFormat(
                    w!("Segoe UI"),
                    None::<&IDWriteFontCollection>,
                    if spec.bold { DWRITE_FONT_WEIGHT_BOLD } else { DWRITE_FONT_WEIGHT_NORMAL },
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    font_px as f32,
                    w!("es-CL"),
                )
                .map_err(win)?;
            format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER).map_err(win)?;
            format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR).map_err(win)?;
            let layout: IDWriteTextLayout = self.text.CreateTextLayout(&wide, &format, text_max as f32, out_h as f32).map_err(win)?;
            let mut metrics = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut metrics).map_err(win)?;
            let (text_w, text_h) = (metrics.width as f64, metrics.height as f64);
            // Centradas dentro del bloque más angosto que las contiene.
            layout.SetMaxWidth(text_w.ceil() as f32).map_err(win)?;
            let (box_w, box_h) = (text_w + pad_x * 2.0, text_h + pad_y * 2.0);
            let x = ((out_w - box_w) / 2.0).round();
            // Como `block_y` de macOS (contado desde abajo): aquí desde arriba.
            let margin = out_h * 0.07;
            let top = match spec.at {
                TextAt::Top => margin,
                TextAt::Center => (out_h - box_h) / 2.0,
                TextAt::Bottom if spec.caption => out_h - out_h * 0.09 - box_h,
                TextAt::Bottom => out_h - margin - box_h,
            }
            .max(0.0)
            .round();
            if spec.boxed {
                let pill = self.ctx.CreateSolidColorBrush(&D2D1_COLOR_F { r: PILL[0], g: PILL[1], b: PILL[2], a: PILL[3] * alpha as f32 }, None).map_err(win)?;
                let lines = (text_h / (font_px * 1.25)).round().max(1.0) as usize;
                let radius = if lines <= 1 { box_h / 2.0 } else { (font_px * 0.8).min(box_h / 2.0) }.min(box_w / 2.0).min(box_h / 2.0);
                let shape = D2D1_ROUNDED_RECT { rect: rect(x, top, x + box_w, top + box_h), radiusX: radius as f32, radiusY: radius as f32 };
                self.ctx.FillRoundedRectangle(&shape, &pill);
            } else {
                // Sin píldora, una sombra suave detrás para que se lean.
                let shade = self.ctx.CreateSolidColorBrush(&color([0.0; 3], 0.6 * alpha), None).map_err(win)?;
                let at = Vector2 { X: (x + pad_x) as f32, Y: (top + pad_y + font_px * 0.06) as f32 };
                self.ctx.DrawTextLayout(at, &layout, &shade, windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_NONE);
            }
            let ink = self.ctx.CreateSolidColorBrush(&color(spec.color, alpha), None).map_err(win)?;
            let at = Vector2 { X: (x + pad_x) as f32, Y: (top + pad_y) as f32 };
            self.ctx.DrawTextLayout(at, &layout, &ink, windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_NONE);
        }
        Ok(())
    }
}

/// La imagen de fondo como BGRA premultiplicado `(ancho, alto, bytes)`; `None` si no se lee.
fn load_backdrop(path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    let bytes = std::fs::read(path).ok()?;
    let (w, h, rgba) = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        let mut decoder = png::Decoder::new(std::io::Cursor::new(&bytes));
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info().ok()?;
        let mut buffer = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buffer).ok()?;
        buffer.truncate(info.buffer_size());
        let rgba: Vec<u8> = match info.color_type {
            png::ColorType::Rgba => buffer,
            png::ColorType::Rgb => buffer.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
            png::ColorType::GrayscaleAlpha => buffer.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
            png::ColorType::Grayscale => buffer.iter().flat_map(|&g| [g, g, g, 255]).collect(),
            png::ColorType::Indexed => return None,
        };
        (info.width, info.height, rgba)
    } else {
        let image = image::load_from_memory(&bytes).ok()?.to_rgba8();
        (image.width(), image.height(), image.into_raw())
    };
    if w == 0 || h == 0 {
        return None;
    }
    let mut bgra = Vec::with_capacity(rgba.len());
    for p in rgba.chunks_exact(4) {
        let a = p[3] as u32;
        let premul = |c: u8| ((c as u32 * a + 127) / 255) as u8;
        bgra.extend_from_slice(&[premul(p[2]), premul(p[1]), premul(p[0]), p[3]]);
    }
    Some((w, h, bgra))
}

#[cfg(test)]
mod tests {
    use super::super::player_win::VideoPlayer;
    use super::super::types::{ClickSpec, CursorShape, CursorStyle, LookSpec, Redaction, TextSpec};
    use super::*;
    use std::time::{Duration, Instant};

    /// Un video de verdad, a mano: `ATIC_TEST_VIDEO=<video>`. Compone un cuadro con todo lo que
    /// dibuja el editor y lo guarda como PNG (`atic-compose-*.png` en la carpeta temporal).
    #[test]
    #[ignore]
    fn compone_un_cuadro_con_todo() {
        let Ok(source) = std::env::var("ATIC_TEST_VIDEO") else { return };
        let mut player = VideoPlayer::open(Path::new(&source)).expect("abrir");
        let (sw, sh) = player.size();
        player.seek((player.duration() / 2.0).min(2.0));
        let started = Instant::now();
        let mut frame = None;
        while started.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(40));
            frame = player.frame();
            if frame.is_some() && started.elapsed() > Duration::from_millis(400) {
                break;
            }
        }
        let frame = frame.expect("un cuadro");
        let mut composer = Composer::new().expect("compositor");
        let scene = |framed: bool| Scene {
            src: (sw as f64, sh as f64),
            out: (1280, 720),
            frame: framed.then(|| FrameSpec { backdrop: Backdrop::Gradient([0.35, 0.2, 0.8], [0.95, 0.4, 0.5]), padding: 0.08, radius: 0.03, shadow: 0.6 }),
            texts: vec![TextSpec::plain("Hola desde Windows", TextAt::Bottom), TextSpec { caption: true, boxed: false, ..TextSpec::plain("Un subtítulo largo que tiene que partirse en varias líneas porque no cabe", TextAt::Bottom) }],
            cursor_px: 32.0,
            look: LookSpec { cursor_style: CursorStyle::System, click: ClickSpec::default(), blur_amount: 1.0 },
        };
        let decor = |zoom: bool| Decor {
            crop: if zoom { [0.3, 0.25, 0.4, 0.4] } else { [0.0, 0.0, 1.0, 1.0] },
            cursor: Some((0.5, 0.45, 1.0)),
            cursor_shape: CursorShape::Arrow,
            cursor_alpha: 1.0,
            texts: vec![(0, 1.0)],
            ripples: vec![(0.5, 0.45, 0.3)],
            blur: None,
            redactions: vec![Redaction { rect: [0.6, 0.1, 0.25, 0.12], pixelate: false, alpha: 1.0 }, Redaction { rect: [0.3, 0.42, 0.3, 0.12], pixelate: true, alpha: 1.0 }],
        };
        for (name, framed, zoom) in [("marco", true, false), ("zoom", true, true), ("sin-marco", false, false)] {
            composer.set_scene(&scene(framed)).expect("escena");
            let started = Instant::now();
            let out = composer.render(&frame, &decor(zoom)).expect("render");
            let ms = started.elapsed().as_secs_f64() * 1000.0;
            let image = out.bgra().expect("leer");
            let png = image.to_png().expect("png");
            let path = std::env::temp_dir().join(format!("atic-compose-{name}.png"));
            std::fs::write(&path, png).expect("escribir");
            println!("{name}: {}x{} en {ms:.1} ms -> {}", out.width(), out.height(), path.display());
        }
        // Con desenfoque de movimiento y de zoom (más fuerte que lo normal para que se note).
        for (name, blur) in [("blur-movimiento", (0.04, 0.02, 1.0)), ("blur-zoom", (0.0, 0.0, 0.7)), ("blur-los-dos", (0.04, 0.02, 0.7))] {
            let mut scene = scene(true);
            scene.look.blur_amount = 2.0;
            composer.set_scene(&scene).expect("escena");
            let out = composer.render(&frame, &Decor { blur: Some(blur), redactions: Vec::new(), ..decor(true) }).expect("render con desenfoque");
            std::fs::write(std::env::temp_dir().join(format!("atic-compose-{name}.png")), out.bgra().unwrap().to_png().unwrap()).unwrap();
        }
        // El desenfoque de zoom cambia el cuadro (sin otra diferencia entre los dos), más en las esquinas
        // que en el centro, y deja el centro casi intacto.
        {
            let mut scene = scene(true);
            scene.look.blur_amount = 2.0;
            composer.set_scene(&scene).expect("escena");
            let plain = Decor { redactions: Vec::new(), cursor: None, ripples: Vec::new(), texts: Vec::new(), ..decor(true) };
            let sharp = composer.render(&frame, &plain).expect("nítido").bgra().unwrap().bgra;
            let zoomed = composer.render(&frame, &Decor { blur: Some((0.0, 0.0, 0.5)), ..plain.clone() }).expect("zoom").bgra().unwrap().bgra;
            let (w, h) = (1280usize, 720usize);
            let region = |data: &[u8], x0: usize, y0: usize, x1: usize, y1: usize| -> Vec<u8> { (y0..y1).flat_map(|y| data[(y * w + x0) * 4..(y * w + x1) * 4].to_vec()).collect() };
            let diff = |a: &[u8], b: &[u8]| a.iter().zip(b).map(|(x, y)| (*x as i32 - *y as i32).unsigned_abs() as f64).sum::<f64>() / a.len() as f64;
            let corner = diff(&region(&sharp, 70, 60, 330, 260), &region(&zoomed, 70, 60, 330, 260));
            let center = diff(&region(&sharp, 540, 310, 740, 410), &region(&zoomed, 540, 310, 740, 410));
            println!("zoom blur: esquina {corner:.2}, centro {center:.2}");
            assert!(corner > 0.2, "el desenfoque de zoom no hizo nada en la esquina: {corner}");
            assert!(corner > center, "tendría que notarse más lejos del centro: esquina {corner}, centro {center}");
        }
        // Con subtítulo y sin píldora.
        composer.set_scene(&scene(true)).expect("escena");
        let out = composer.render(&frame, &Decor { texts: vec![(1, 1.0)], ..decor(false) }).expect("render");
        std::fs::write(std::env::temp_dir().join("atic-compose-subtitulo.png"), out.bgra().unwrap().to_png().unwrap()).unwrap();
    }
}
