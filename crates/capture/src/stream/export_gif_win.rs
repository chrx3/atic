//! De un MP4 ya exportado a GIF (Windows): Media Foundation entrega cada cuadro en BGRA y ya
//! escalado, y la crate `gif` lo cuantiza (NeuQuant, una paleta por cuadro) y lo va sumando al
//! archivo. El mismo contrato que `export_gif_macos.rs`: `fps` cuadros por segundo (con tope),
//! `max_width` de ancho como mucho, bucle infinito, avance de 0 a 1 y `Err("cancelada")` al
//! cancelar.
//!
//! El video se abre una vez y se recorre hacia adelante: cada cuadro del archivo se decodifica
//! una sola vez, por más cuadros de GIF que se pidan.

use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use gif::{Encoder, Frame as GifFrame, Repeat};
use windows::core::GUID;
use windows::Win32::Media::MediaFoundation::{
    IMFSourceReader, IMFSample, MFCreateMediaType, MFMediaType_Video, MFVideoFormat_ARGB32, MFVideoFormat_RGB32,
    MFVideoInterlace_Progressive, MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE,
    MF_MT_SUBTYPE, MF_SOURCE_READERF_ENDOFSTREAM, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
};

use super::info_win::{win, Session, Source, TICKS_PER_SECOND};

/// Más de esto no se gana nada en un GIF (y los navegadores lo recortan).
const MAX_FPS: u32 = 30;
/// El codificador guarda un cuadro a la vez, pero el archivo se hace enorme: este tope acota el
/// trabajo y el tamaño. Si el video es largo, baja el ritmo en vez de pasarse.
const MAX_FRAMES: usize = 1500;
/// Lo que recomienda la crate: el punto medio entre calidad y tiempo de NeuQuant.
const SPEED: i32 = 10;
/// La primera pista de video (el índice negativo que entiende Media Foundation).
const VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

/// Cada cuánto va un cuadro, en centésimas de segundo (lo único que sabe un GIF), y cuántos
/// salen: `fps` como mucho (con tope en [`MAX_FPS`]) y nunca más de [`MAX_FRAMES`].
fn plan(duration: f64, fps: u32) -> (u32, usize) {
    let fps = fps.clamp(1, MAX_FPS);
    let mut step = ((100.0 / fps as f64).round() as u32).max(1);
    let total = duration.max(0.0) * 100.0;
    if (total / step as f64).ceil() > MAX_FRAMES as f64 {
        step = (total / MAX_FRAMES as f64).ceil() as u32;
    }
    let frames = ((total / step as f64).ceil() as usize).max(1);
    (step, frames)
}

/// El retardo de cada cuadro, en centésimas (lo único que sabe un GIF): el paso del plan. Con
/// el tope de 30 fps nunca baja de 3, pero por si acaso no se deja en menos de 2, que es lo que
/// muchos visores muestran como si fueran 10.
fn delay_cs(step: u32) -> u16 {
    step.clamp(2, u16::MAX as u32) as u16
}

/// El segundo del cuadro `i` (base 0): cada `step` centésimas, sin pasarse del final (los
/// últimos cuadros se quedan con lo que se ve al final del video).
fn frame_time(i: usize, step: u32, duration: f64) -> f64 {
    (i as f64 * step as f64 / 100.0).min((duration - 0.05).max(0.0))
}

/// Las medidas del GIF: `max_width` de ancho como mucho (nunca agranda), con la proporción del
/// original, y las dos pares (como `info_win::scaled` con el ancho): el procesador de video de
/// Media Foundation trabaja más cómodo con medidas pares y un píxel de diferencia no se nota.
fn target_size((w, h): (u32, u32), max_width: u32) -> (u32, u32) {
    let (w, h) = (w.max(1), h.max(1));
    // Sin límite (o si ya cabe): el video tal cual, sin tocarle las medidas.
    if max_width == 0 || w <= max_width {
        return (w, h);
    }
    let out_w = (max_width & !1).max(2).min(w);
    let out_h = ((((h as f64 * out_w as f64 / w as f64).round() as u32) & !1).max(2)).min(h);
    (out_w, out_h)
}

/// `max_width` de ancho como mucho (con su proporción) y `fps` cuadros por segundo.
/// `progress` recibe de 0 a 1; si `cancel` se activa, borra lo escrito y devuelve `Err`.
pub fn export_gif(source: &Path, out: &Path, max_width: u32, fps: u32, progress: &dyn Fn(f64), cancel: &AtomicBool) -> Result<(), String> {
    let _ = std::fs::remove_file(out);
    let result = gif_inner(source, out, max_width, fps, progress, cancel);
    if result.is_err() {
        // Ni un archivo a medias ni uno viejo que se confunda con el nuevo. El archivo y el
        // codificador ya se soltaron al volver de `gif_inner` (Windows no borra uno abierto).
        let _ = std::fs::remove_file(out);
    }
    result
}

fn gif_inner(source: &Path, out: &Path, max_width: u32, fps: u32, progress: &dyn Fn(f64), cancel: &AtomicBool) -> Result<(), String> {
    let _session = Session::start()?;
    let opened = Source::open(source)?;
    if opened.duration <= 0.0 {
        return Err("el video no tiene duración".into());
    }
    let size = target_size(opened.size, max_width);
    let mut grab = Grab::open(opened, size)?;
    let (duration, (width, height)) = (grab.source.duration, grab.out);
    let (step, count) = plan(duration, fps);
    let delay = delay_cs(step);
    if width == 0 || height == 0 || width > u16::MAX as u32 || height > u16::MAX as u32 {
        return Err("el video no tiene un tamaño que quepa en un GIF".into());
    }
    let (w16, h16) = (width as u16, height as u16);

    let mut file = BufWriter::new(std::fs::File::create(out).map_err(|error| format!("no pude crear el GIF: {error}"))?);
    let mut encoder = Encoder::new(&mut file, w16, h16, &[]).map_err(|error| format!("no pude escribir el GIF: {error}"))?;
    // Bucle infinito, como el archivo de macOS (0 en ImageIO).
    encoder.set_repeat(Repeat::Infinite).map_err(|error| format!("no pude escribir el GIF: {error}"))?;

    // El búfer del cuadro se reusa: `frame_at` lo llena de nuevo en cada vuelta.
    let mut rgba: Vec<u8> = Vec::new();
    for i in 0..count {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelada".into());
        }
        let at = frame_time(i, step, duration);
        if !grab.frame_at(at, &mut rgba) {
            return Err("no pude leer el video".into());
        }
        if rgba.len() != width as usize * height as usize * 4 {
            return Err("Media Foundation no dio el cuadro del tamaño pedido".into());
        }
        let mut frame = GifFrame::from_rgba_speed(w16, h16, &mut rgba, SPEED);
        frame.delay = delay;
        encoder.write_frame(&frame).map_err(|error| format!("no pude escribir el GIF: {error}"))?;
        progress(((i + 1) as f64 / count as f64) * 0.98);
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("cancelada".into());
    }
    // Cierra el GIF (la marca final) y el archivo, avisando si algo falló al escribir.
    let file = encoder.into_inner().map_err(|error| format!("no pude escribir el GIF: {error}"))?;
    file.flush().map_err(|error| format!("no pude escribir el GIF: {error}"))?;
    progress(1.0);
    Ok(())
}

/// Un video abierto una sola vez, con los cuadros que entrega Media Foundation ya convertidos y
/// escalados a un tamaño fijo. `info_win::Video` hace lo mismo para las miniaturas, pero es
/// privado de su módulo; aquí se pide el mismo tipo de medio con [`configure`].
struct Grab {
    source: Source,
    /// Las medidas de lo que entrega Media Foundation (ya escalado).
    out: (u32, u32),
    /// Bytes de una fila; negativo si el cuadro viene de abajo hacia arriba.
    stride: i32,
    /// El último cuadro con marca de tiempo anterior o igual al instante pedido.
    last: Option<Vec<u8>>,
    /// El siguiente a `last`, ya leído, para cuando lo pida un instante mayor.
    ahead: Option<(f64, Vec<u8>)>,
    /// Ya se llegó al final del archivo.
    ended: bool,
}

impl Grab {
    /// Pide cuadros del tamaño `out` de un video ya abierto (la sesión de Media Foundation tiene
    /// que estar abierta).
    fn open(source: Source, out: (u32, u32)) -> Result<Self, String> {
        let (out, stride) = configure(&source.reader, out)?;
        Ok(Self { source, out, stride, last: None, ahead: None, ended: false })
    }

    /// Deja en `pixels` el cuadro que se ve en el segundo `at` (el último con marca de tiempo
    /// anterior o igual) y dice si lo hay. Al final del video repite el último, igual que macOS
    /// cuando un cuadro no se puede leer, para no desajustar la cuenta.
    fn frame_at(&mut self, at: f64, pixels: &mut Vec<u8>) -> bool {
        // Ya se leyó el cuadro que sigue: si todavía no llega su instante, el que se ve es
        // `last`; si ya llegó, pasa a ser el último y se sigue leyendo.
        let arrived = match &self.ahead {
            Some((time, _)) => *time <= at,
            None => false,
        };
        if arrived {
            self.last = self.ahead.take().map(|(_, bgra)| bgra);
        } else if self.ahead.is_some() {
            return self.take_last(pixels);
        }
        let (out, stride) = (self.out, self.stride);
        while !self.ended {
            match next_sample(&self.source.reader, out, stride) {
                Some((time, bgra)) if time <= at => self.last = Some(bgra),
                // Este ya pasa el objetivo: se guarda para la próxima vuelta. Si el video
                // empieza después del instante pedido, vale este (como en macOS, que se queda
                // con el primer cuadro que hay).
                Some((time, bgra)) => {
                    if self.last.is_none() {
                        self.last = Some(bgra);
                    } else {
                        self.ahead = Some((time, bgra));
                    }
                    break;
                }
                None => self.ended = true,
            }
        }
        self.take_last(pixels)
    }

    /// Copia el cuadro elegido a `pixels` (se copia porque `from_rgba_speed` trabaja sobre el
    /// búfer y el cuadro tiene que quedar para el siguiente instante).
    fn take_last(&self, pixels: &mut Vec<u8>) -> bool {
        let Some(last) = self.last.as_ref() else { return false };
        pixels.clear();
        pixels.extend_from_slice(last);
        true
    }
}

/// El siguiente cuadro del video, en RGBA, con su marca de tiempo en segundos. `None` al final
/// del archivo; los cuadros que no se pueden convertir se saltan.
fn next_sample(reader: &IMFSourceReader, out: (u32, u32), stride: i32) -> Option<(f64, Vec<u8>)> {
    loop {
        let (mut flags, mut time, mut sample) = (0u32, 0i64, None);
        // SAFETY: el lector vive en quien llama, se usa desde este hilo y se lee hacia adelante.
        unsafe { reader.ReadSample(VIDEO, 0, None, Some(&mut flags), Some(&mut time), Some(&mut sample)) }.ok()?;
        if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
            return None;
        }
        let Some(sample) = sample else { continue };
        let Some(rgba) = locked(&sample, |bytes| to_rgba(bytes, out, stride)).flatten() else { continue };
        return Some((time as f64 / TICKS_PER_SECOND as f64, rgba));
    }
}

/// Pide cuadros en BGRA del tamaño `out` (RGB32 y, si no se deja, ARGB32): Media Foundation
/// mete su procesador de video para convertir y escalar. Devuelve el tamaño real y los bytes
/// de una fila (negativo si el cuadro viene de abajo hacia arriba).
fn configure(reader: &IMFSourceReader, out: (u32, u32)) -> Result<((u32, u32), i32), String> {
    let mut last = String::new();
    for subtype in [MFVideoFormat_RGB32, MFVideoFormat_ARGB32] {
        match configure_as(reader, out, &subtype) {
            Ok(done) => return Ok(done),
            Err(error) => last = error,
        }
    }
    Err(last)
}

/// [`configure`] con un subtipo concreto de video (los dos son BGRA en memoria).
fn configure_as(reader: &IMFSourceReader, out: (u32, u32), subtype: &GUID) -> Result<((u32, u32), i32), String> {
    // SAFETY: el lector está vivo.
    unsafe {
        let media_type = MFCreateMediaType().map_err(win)?;
        media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).map_err(win)?;
        media_type.SetGUID(&MF_MT_SUBTYPE, subtype).map_err(win)?;
        media_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32).map_err(win)?;
        media_type.SetUINT64(&MF_MT_FRAME_SIZE, pack(out)).map_err(win)?;
        reader
            .SetCurrentMediaType(VIDEO, None, &media_type)
            .map_err(|error| format!("no pude preparar los cuadros: {}", win(error)))?;
        let current = reader.GetCurrentMediaType(VIDEO).map_err(win)?;
        let real = unpack(current.GetUINT64(&MF_MT_FRAME_SIZE).map_err(win)?);
        if real.0 == 0 || real.1 == 0 {
            return Err("Media Foundation no dio las medidas del cuadro".into());
        }
        // El paso lo decide el decodificador: en RGB32 suele venir de abajo hacia arriba.
        let stride = match current.GetUINT32(&MF_MT_DEFAULT_STRIDE) {
            Ok(0) | Err(_) => real.0 as i32 * 4,
            Ok(value) => value as i32,
        };
        Ok((real, stride))
    }
}

/// Las dos medidas de un `UINT64` de Media Foundation (el ancho arriba).
fn unpack(packed: u64) -> (u32, u32) {
    ((packed >> 32) as u32, packed as u32)
}

/// Las dos medidas en un `UINT64` como lo pide Media Foundation (el ancho arriba).
fn pack(size: (u32, u32)) -> u64 {
    ((size.0 as u64) << 32) | size.1 as u64
}

/// Corre `read` con los bytes de `sample`, ya seguidos.
fn locked<T>(sample: &IMFSample, read: impl FnOnce(&[u8]) -> T) -> Option<T> {
    // SAFETY: el buffer se bloquea y se desbloquea aquí; `read` no se guarda la rebanada.
    unsafe {
        let buffer = sample.ConvertToContiguousBuffer().ok()?;
        let mut data = std::ptr::null_mut();
        let mut len = 0u32;
        buffer.Lock(&mut data, None, Some(&mut len)).ok()?;
        let out = if data.is_null() || len == 0 {
            None
        } else {
            Some(read(std::slice::from_raw_parts(data, len as usize)))
        };
        let _ = buffer.Unlock();
        out
    }
}

/// El cuadro RGBA de un buffer de Media Foundation (BGRA en memoria): filas de `size.0 * 4`
/// bytes con `stride` de paso, de abajo hacia arriba si es negativo. El alfa queda opaco (en
/// RGB32 no significa nada y suele venir en cero).
fn to_rgba(bytes: &[u8], size: (u32, u32), stride: i32) -> Option<Vec<u8>> {
    let (w, h) = (size.0 as usize, size.1 as usize);
    let row = w * 4;
    let step = stride.unsigned_abs() as usize;
    if w == 0 || h == 0 || step < row || bytes.len() < (h - 1) * step + row {
        return None;
    }
    let mut rgba = vec![255u8; row * h];
    for y in 0..h {
        let from = if stride < 0 { h - 1 - y } else { y };
        let src = &bytes[from * step..from * step + row];
        let dst = &mut rgba[y * row..(y + 1) * row];
        for (pixel, out) in src.chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
            // BGRA → RGBA; el alfa ya viene opaco.
            out[0] = pixel[2];
            out[1] = pixel[1];
            out[2] = pixel[0];
        }
    }
    Some(rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_ritmo_se_redondea_a_centesimas() {
        // 15 fps → 6,67 cs → 7 cs; 10 s → ceil(1000 / 7) = 143 cuadros.
        assert_eq!(plan(10.0, 15), (7, 143));
        // 10 fps → 10 cs exactos.
        assert_eq!(plan(2.0, 10), (10, 20));
    }

    #[test]
    fn el_ritmo_tiene_tope_de_30_fps() {
        // 60 fps pedidos se tratan como 30 → 3 cs.
        assert_eq!(plan(1.0, 60).0, 3);
        assert_eq!(plan(1.0, 0).0, 100);
    }

    #[test]
    fn un_video_largo_baja_el_ritmo_para_no_pasar_de_1500_cuadros() {
        // 300 s a 30 fps serían 10000 cuadros: el paso sube a 20 cs → 1500 cuadros.
        assert_eq!(plan(300.0, 30), (20, 1500));
        let (step, frames) = plan(3600.0, 15);
        assert!(frames <= MAX_FRAMES && step > 7);
    }

    #[test]
    fn un_video_cortisimo_da_al_menos_un_cuadro() {
        assert_eq!(plan(0.0, 15).1, 1);
        assert_eq!(plan(0.01, 15).1, 1);
    }

    #[test]
    fn los_cuadros_caen_cada_paso_y_no_pasan_del_final() {
        // 1 s a 10 fps: diez cuadros, en 0,0 s, 0,1 s… 0,9 s.
        let (step, count) = plan(1.0, 10);
        assert_eq!((step, count), (10, 10));
        for i in 0..count {
            assert!((frame_time(i, step, 1.0) - i as f64 / 10.0).abs() < 1e-9);
        }
        // Uno de 0,3 s: no hay cuadro después del final; el primero es el 0.
        assert!(frame_time(9, step, 0.3) <= 0.3);
        assert_eq!(frame_time(0, step, 0.0), 0.0);
    }

    #[test]
    fn el_retardo_va_en_centesimas_y_no_baja_de_dos() {
        assert_eq!(delay_cs(3), 3);
        assert_eq!(delay_cs(7), 7);
        assert_eq!(delay_cs(1), 2);
        assert_eq!(delay_cs(100), 100);
    }

    #[test]
    fn las_medidas_respetan_la_proporcion_y_quedan_pares() {
        assert_eq!(target_size((1920, 1080), 800), (800, 450));
        // 1920×1080 a 400 de ancho daría 225 de alto: baja a par.
        assert_eq!(target_size((1920, 1080), 401), (400, 224));
        assert_eq!(target_size((1080, 1920), 800), (800, 1422));
        // Un ancho impar pedido se redondea al par de abajo.
        assert_eq!(target_size((1920, 1080), 801), (800, 450));
    }

    #[test]
    fn nunca_agranda_el_cuadro() {
        // Pedir más ancho que el video lo deja como está (aunque tenga medidas impares).
        assert_eq!(target_size((640, 480), 800), (640, 480));
        assert_eq!(target_size((641, 481), 800), (641, 481));
        // 0 quiere decir sin límite.
        assert_eq!(target_size((640, 480), 0), (640, 480));
    }

    #[test]
    fn el_cuadro_pasa_a_rgba_y_se_voltea_si_hace_falta() {
        // Filas de 2 píxeles BGRA, la de abajo primero (el paso negativo de RGB32).
        let (azul, blanco) = ([255u8, 0, 0, 0], [255u8, 255, 255, 0]);
        let (rojo, verde) = ([0u8, 0, 255, 0], [0u8, 255, 0, 0]);
        let mut bytes = Vec::new();
        for fila in [[rojo, verde], [azul, blanco]] {
            for pixel in fila {
                bytes.extend_from_slice(&pixel);
            }
        }
        let rgba = to_rgba(&bytes, (2, 2), -8).expect("un cuadro");
        assert_eq!(rgba, vec![0, 0, 255, 255, 255, 255, 255, 255, 255, 0, 0, 255, 0, 255, 0, 255]);
        // Una rebanada corta no da cuadro (falta una fila).
        assert!(to_rgba(&[0u8; 8], (2, 2), 8).is_none());
    }

    /// Un MP4 de verdad, a mano: `ATIC_TEST_VIDEO=<video>`. Sale un GIF de verdad: cabecera
    /// `GIF89a`, archivo con algo dentro y el avance hasta el final. Se pide lento y chico
    /// (5 fps, 160 px) porque lo que dura sale del video de prueba, no de lo que pida esto.
    #[test]
    #[ignore]
    fn exporta_un_video_de_verdad() {
        let Ok(source) = std::env::var("ATIC_TEST_VIDEO") else { return };
        let out = std::env::temp_dir().join(format!("atic-gif-prueba-{}.gif", std::process::id()));
        let done = std::cell::Cell::new(0.0);
        let cancel = AtomicBool::new(false);
        export_gif(Path::new(&source), &out, 160, 5, &|value| done.set(value), &cancel).expect("exporta");
        let bytes = std::fs::read(&out).expect("el GIF quedó");
        assert!(bytes.len() > 20, "el GIF no está vacío");
        assert!(bytes.starts_with(b"GIF89a"));
        assert_eq!(done.get(), 1.0);
        // Cancelado: no deja nada a medias.
        cancel.store(true, Ordering::Relaxed);
        let canceled = export_gif(Path::new(&source), &out, 160, 5, &(|_: f64| {}), &cancel);
        assert!(canceled.is_err());
        assert!(!out.exists());
        let _ = std::fs::remove_file(&out);
    }
}
