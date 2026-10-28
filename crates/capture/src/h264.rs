//! H.264 a cuadros BGRA: la pantalla del celular en el PC (la manda la app de Android
//! por el canal de video, `atic_sync::video`).
//!
//! Decodifica OpenH264 (de Cisco), el mismo en macOS y Windows; los cuadros llegan en
//! Annex-B (lo que entrega MediaCodec), con SPS/PPS antes de cada keyframe. La
//! conversión a BGRA (lo que pinta GPUI) va aquí: en una dependencia se compila
//! optimizada también en desarrollo.

use openh264::decoder::Decoder as H264;
use openh264::formats::YUVSource;

type Result<T> = std::result::Result<T, String>;

/// Un cuadro listo para pintar (`gpui::RenderImage` lo quiere en BGRA).
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

pub struct Decoder {
    h264: H264,
    rgba: Vec<u8>,
}

impl Decoder {
    pub fn new() -> Result<Self> {
        Ok(Self { h264: H264::new().map_err(|e| format!("no se pudo iniciar el decodificador H.264: {e}"))?, rgba: Vec::new() })
    }

    /// Un cuadro del canal (una o más unidades NAL en Annex-B). `None` mientras no hay
    /// imagen completa (solo SPS/PPS, o falta el keyframe tras un hueco).
    pub fn decode(&mut self, annex_b: &[u8]) -> Result<Option<Frame>> {
        let mut last = None;
        for nal in openh264::nal_units(annex_b) {
            if let Some(yuv) = self.h264.decode(nal).map_err(|e| format!("cuadro H.264 dañado: {e}"))? {
                let (w, h) = yuv.dimensions();
                self.rgba.resize(w * h * 4, 0);
                yuv.write_rgba8(&mut self.rgba);
                last = Some(Frame { width: w as u32, height: h as u32, bgra: rgba_to_bgra(&self.rgba) });
            }
        }
        Ok(last)
    }
}

// --- Codificar: la pantalla del PC en el celular -----------------------------------

use openh264::encoder::{BitRate, Encoder as H264Encoder, EncoderConfig, FrameRate, FrameType, IntraFramePeriod, UsageType};
use openh264::formats::{BgraSliceU8, YUVBuffer};
use openh264::OpenH264API;

/// Lo que sale de codificar un cuadro, como lo lleva el canal de video
/// (`atic_sync::video::flags`): FORMAT (el JSON con las medidas), config (SPS/PPS) y
/// keyframe.
pub mod out {
    pub const KEY: u8 = 1;
    pub const CONFIG: u8 = 1 << 1;
    pub const FORMAT: u8 = 1 << 2;
}

/// Pantallas del PC a H.264 para el celular: escaladas a `max_width` como mucho (con
/// su proporción, medidas pares), en modo «contenido de pantalla» de OpenH264.
pub struct ScreenEncoder {
    encoder: Option<H264Encoder>,
    max_width: u32,
    bitrate: u32,
    fps: f32,
    size: (u32, u32),
    force_key: bool,
    scaled: Vec<u8>,
}

impl ScreenEncoder {
    pub fn new(max_width: u32, bitrate: u32, fps: f32) -> Self {
        Self { encoder: None, max_width: max_width.clamp(320, 3840), bitrate, fps, size: (0, 0), force_key: true, scaled: Vec::new() }
    }

    /// El próximo cuadro sale completo (el celular lo pidió).
    pub fn request_keyframe(&mut self) {
        self.force_key = true;
    }

    /// Bitrate nuevo: el codificador se rehace en el próximo cuadro.
    pub fn set_bitrate(&mut self, bps: u32) {
        if bps != self.bitrate {
            self.bitrate = bps.clamp(250_000, 20_000_000);
            self.encoder = None;
        }
    }

    /// Codifica una captura BGRA de `width × height`. Devuelve lo que hay que mandar,
    /// en orden: el FORMAT si cambiaron las medidas y el cuadro (nada si OpenH264 lo
    /// saltó porque no cambió nada).
    pub fn encode(&mut self, bgra: &[u8], width: u32, height: u32) -> Result<Vec<(u8, Vec<u8>)>> {
        let (tw, th) = fit_even(width, height, self.max_width);
        let mut out = Vec::new();
        if (tw, th) != self.size {
            self.size = (tw, th);
            self.encoder = None;
            out.push((out::FORMAT, format!(r#"{{"width":{tw},"height":{th},"rotation":0}}"#).into_bytes()));
        }
        if self.encoder.is_none() {
            let config = EncoderConfig::new()
                .bitrate(BitRate::from_bps(self.bitrate))
                .max_frame_rate(FrameRate::from_hz(self.fps))
                .usage_type(UsageType::ScreenContentRealTime)
                .intra_frame_period(IntraFramePeriod::from_num_frames((self.fps * 2.) as u32))
                .skip_frames(true);
            let encoder = H264Encoder::with_api_config(OpenH264API::from_source(), config).map_err(|e| format!("no se pudo iniciar el codificador H.264: {e}"))?;
            self.encoder = Some(encoder);
            self.force_key = true;
        }
        scale_bgra(bgra, (width, height), (tw, th), &mut self.scaled);
        let yuv = YUVBuffer::from_rgb_source(BgraSliceU8::new(&self.scaled, (tw as usize, th as usize)));
        let encoder = self.encoder.as_mut().expect("recién creado");
        if std::mem::take(&mut self.force_key) {
            encoder.force_intra_frame();
        }
        let stream = encoder.encode(&yuv).map_err(|e| format!("no se pudo codificar: {e}"))?;
        let flags = match stream.frame_type() {
            FrameType::Skip | FrameType::Invalid => return Ok(out),
            // OpenH264 pone SPS/PPS antes de cada IDR.
            FrameType::IDR => out::KEY | out::CONFIG,
            FrameType::I => out::KEY,
            _ => 0,
        };
        out.push((flags, stream.to_vec()));
        Ok(out)
    }
}

/// Las medidas para mandar: como mucho `max_width` de ancho, con la proporción, pares.
pub fn fit_even(width: u32, height: u32, max_width: u32) -> (u32, u32) {
    let w = width.min(max_width).max(2);
    let h = ((height as u64 * w as u64) / width.max(1) as u64) as u32;
    (w & !1, h.max(2) & !1)
}

/// Achica BGRA promediando los píxeles de cada celda (el texto queda legible, no
/// entrecortado como al saltear píxeles).
pub fn scale_bgra(src: &[u8], (sw, sh): (u32, u32), (tw, th): (u32, u32), out: &mut Vec<u8>) {
    out.resize((tw * th * 4) as usize, 0);
    if (sw, sh) == (tw, th) {
        let len = out.len();
        out.copy_from_slice(&src[..len]);
        return;
    }
    let (sw, sh, tw, th) = (sw as usize, sh as usize, tw as usize, th as usize);
    for y in 0..th {
        let y0 = y * sh / th;
        let y1 = ((y + 1) * sh / th).max(y0 + 1).min(sh);
        for x in 0..tw {
            let x0 = x * sw / tw;
            let x1 = ((x + 1) * sw / tw).max(x0 + 1).min(sw);
            // Hasta 2×2 muestras por celda: barato y suficiente para achicar ~2×.
            let xs = [x0, (x0 + x1 - 1) / 2 + if x1 - x0 > 1 { 1 } else { 0 }];
            let ys = [y0, (y0 + y1 - 1) / 2 + if y1 - y0 > 1 { 1 } else { 0 }];
            let mut acc = [0u32; 4];
            for &sy in &ys {
                for &sx in &xs {
                    let i = (sy.min(sh - 1) * sw + sx.min(sw - 1)) * 4;
                    for c in 0..4 {
                        acc[c] += src[i + c] as u32;
                    }
                }
            }
            let o = (y * tw + x) * 4;
            for c in 0..4 {
                out[o + c] = (acc[c] / 4) as u8;
            }
        }
    }
}

fn rgba_to_bgra(rgba: &[u8]) -> Vec<u8> {
    let mut out = rgba.to_vec();
    for px in out.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_pantalla_se_achica_con_su_proporcion_y_medidas_pares() {
        assert_eq!(fit_even(2940, 1912, 1600), (1600, 1040));
        assert_eq!(fit_even(1280, 800, 1600), (1280, 800));
        assert_eq!(fit_even(1921, 1081, 1600), (1600, 900));
    }

    #[test]
    fn achicar_promedia_cada_celda() {
        // 4×2 blanco y negro alternado → 2×1 gris.
        let w = [255u8, 255, 255, 255];
        let b = [0u8, 0, 0, 255];
        let src: Vec<u8> = [w, b, w, b, b, w, b, w].concat();
        let mut out = Vec::new();
        scale_bgra(&src, (4, 2), (2, 1), &mut out);
        assert_eq!(out, vec![127, 127, 127, 255, 127, 127, 127, 255]);
    }

    /// La pantalla principal de verdad, capturada y codificada como la comparte la pill:
    /// a mano (pide el permiso de grabar pantalla).
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    #[ignore]
    fn captura_y_codifica_la_pantalla_principal() {
        let monitor = crate::monitors::enumerate().into_iter().find(|m| m.is_primary).unwrap();
        let mut encoder = ScreenEncoder::new(1600, 4_000_000, 24.);
        let (mut capture, mut encode, mut bytes) = (0f32, 0f32, 0usize);
        let n = 24;
        for _ in 0..n {
            let t = std::time::Instant::now();
            let frame = crate::engine::capture_monitor_for_stream(&monitor).expect("captura");
            capture += t.elapsed().as_secs_f32();
            let t = std::time::Instant::now();
            for (_, data) in encoder.encode(&frame.bgra, frame.width(), frame.height()).unwrap() {
                bytes += data.len();
            }
            encode += t.elapsed().as_secs_f32();
        }
        // Lo mismo con la captura completa (resolución nativa), para comparar con la misma carga.
        let t = std::time::Instant::now();
        for _ in 0..n {
            let _ = crate::engine::freeze_monitors(std::slice::from_ref(&monitor), false);
        }
        println!("captura completa: {:.1} ms por cuadro", t.elapsed().as_secs_f32() * 1000. / n as f32);
        println!(
            "{}×{} → {:?}: capturar {:.1} ms, codificar {:.1} ms por cuadro; {} KB/s a 24 fps",
            monitor.bounds.width, monitor.bounds.height, encoder.size,
            capture * 1000. / n as f32, encode * 1000. / n as f32, bytes / 1024,
        );
    }

    #[test]
    fn una_pantalla_codificada_se_vuelve_a_ver() {
        // Una «pantalla» con un degradado, codificada y decodificada de vuelta.
        let (w, h) = (640u32, 400u32);
        let mut bgra = vec![0u8; (w * h * 4) as usize];
        for (i, px) in bgra.chunks_exact_mut(4).enumerate() {
            let x = (i as u32 % w) as u8;
            px.copy_from_slice(&[x, 128, 255 - x, 255]);
        }
        let mut encoder = ScreenEncoder::new(320, 1_000_000, 30.);
        let first = encoder.encode(&bgra, w, h).unwrap();
        assert_eq!(first[0].0, out::FORMAT);
        assert_eq!(std::str::from_utf8(&first[0].1).unwrap(), r#"{"width":320,"height":200,"rotation":0}"#);
        assert_eq!(first[1].0, out::KEY | out::CONFIG);
        let mut decoder = Decoder::new().unwrap();
        let frame = decoder.decode(&first[1].1).unwrap().expect("un cuadro");
        assert_eq!((frame.width, frame.height), (320, 200));
    }

    #[test]
    fn rojo_queda_en_su_lugar_en_bgra() {
        assert_eq!(rgba_to_bgra(&[255, 10, 20, 255]), vec![20, 10, 255, 255]);
    }

    /// Un H.264 de verdad (`ATIC_TEST_H264`, Annex-B; p. ej. de ffmpeg con
    /// `-profile:v baseline -f h264`): a mano.
    #[test]
    #[ignore]
    fn decodifica_un_video_annex_b() {
        let path = std::env::var("ATIC_TEST_H264").expect("ATIC_TEST_H264");
        let bytes = std::fs::read(path).unwrap();
        let mut decoder = Decoder::new().unwrap();
        let started = std::time::Instant::now();
        let mut frames = 0;
        let mut size = (0, 0);
        // Como llegan por el canal: de a una unidad NAL.
        for nal in openh264::nal_units(&bytes) {
            if let Some(frame) = decoder.decode(nal).unwrap() {
                frames += 1;
                size = (frame.width, frame.height);
                assert_eq!(frame.bgra.len(), (frame.width * frame.height * 4) as usize);
            }
        }
        let per_frame = started.elapsed().as_secs_f32() * 1000. / frames.max(1) as f32;
        println!("{frames} cuadros de {}×{}, {per_frame:.1} ms cada uno", size.0, size.1);
        assert!(frames >= 55, "{frames} cuadros");
    }
}
