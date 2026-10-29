//! La pantalla en vivo a H.264, capturada y codificada por el sistema (sin pasar los
//! píxeles por la CPU): la pantalla del PC en el celular.
//!
//! - macOS: ScreenCaptureKit entrega cada cuadro ya escalado y en NV12, solo cuando
//!   algo cambió, y VideoToolbox lo codifica en hardware.
//!
//! Donde no hay (u otro sistema), [`ScreenStream::start`] falla y quien comparte usa
//! la captura y el codificador de software ([`crate::h264::ScreenEncoder`]).
//!
//! Lo que sale es igual en los dos caminos: FORMAT al empezar y cuadros H.264 en
//! Annex-B, con SPS/PPS antes de cada keyframe ([`crate::h264::out`]).

#[cfg(target_os = "macos")]
mod macos;

/// Recibe cada cuadro: los flags de [`crate::h264::out`] y los datos. Se llama desde un
/// hilo del sistema: tiene que volver rápido.
pub type Output = Box<dyn Fn(u8, Vec<u8>) + Send + Sync>;
/// `false` si quien recibe va atrasado: ese cuadro no se codifica (así no se rompe la
/// cadena de cuadros, como pasaría si se descartara ya codificado).
pub type Ready = Box<dyn Fn() -> bool + Send + Sync>;

/// La pantalla en vivo. Soltarla la detiene.
pub struct ScreenStream {
    #[cfg(target_os = "macos")]
    inner: macos::Stream,
}

impl ScreenStream {
    /// Empieza a capturar `monitor` a `max_width` de ancho como mucho (con su
    /// proporción), a `fps` como mucho y a `bitrate` (también el tope). `Err` si el
    /// sistema no lo permite (sin permiso, sin codificador de hardware, otro sistema).
    #[allow(unused_variables)]
    pub fn start(monitor: &crate::MonitorInfo, max_width: u32, bitrate: u32, fps: u32, output: Output, ready: Ready) -> Result<Self, String> {
        #[cfg(target_os = "macos")]
        {
            macos::Stream::start(monitor, max_width, bitrate, fps, output, ready).map(|inner| Self { inner })
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err("sin captura de hardware en este sistema".into())
        }
    }

    /// El próximo cuadro sale completo (aunque la pantalla esté quieta).
    pub fn request_keyframe(&self) {
        #[cfg(target_os = "macos")]
        self.inner.request_keyframe();
    }

    /// Bitrate nuevo, sin keyframe; no pasa del de partida.
    #[allow(unused_variables)]
    pub fn set_bitrate(&self, bps: u32) {
        #[cfg(target_os = "macos")]
        self.inner.set_bitrate(bps);
    }

    /// Las medidas a las que sale el video.
    pub fn size(&self) -> (u32, u32) {
        #[cfg(target_os = "macos")]
        {
            self.inner.size()
        }
        #[cfg(not(target_os = "macos"))]
        {
            (0, 0)
        }
    }
}

/// De AVCC (cada NAL con su largo en 4 bytes, lo que entregan VideoToolbox y Media
/// Foundation) a Annex-B (cada NAL tras `00 00 00 01`, lo que lleva el canal), detrás
/// de `out`. `false` si los largos no cuadran.
pub fn avcc_to_annex_b(avcc: &[u8], out: &mut Vec<u8>) -> bool {
    let mut rest = avcc;
    while !rest.is_empty() {
        if rest.len() < 4 {
            return false;
        }
        let len = u32::from_be_bytes(rest[..4].try_into().unwrap()) as usize;
        let Some(nal) = rest.get(4..4 + len) else { return false };
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(nal);
        rest = &rest[4 + len..];
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// La pantalla principal de verdad, en vivo por el camino del sistema: a mano (pide
    /// el permiso de grabar pantalla). Lo que sale se tiene que poder decodificar.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn la_pantalla_en_vivo_se_decodifica() {
        use std::sync::{Arc, Mutex};
        use std::time::{Duration, Instant};
        let monitor = crate::monitors::enumerate().into_iter().find(|m| m.is_primary).unwrap();
        let frames: Arc<Mutex<Vec<(u8, Vec<u8>, Instant)>>> = Arc::default();
        let sink = frames.clone();
        let started = Instant::now();
        let stream = ScreenStream::start(
            &monitor,
            1600,
            4_000_000,
            30,
            Box::new(move |flags, data| sink.lock().unwrap().push((flags, data, Instant::now()))),
            Box::new(|| true),
        )
        .expect("empieza");
        println!("arrancó en {:?}, {:?}", started.elapsed(), stream.size());
        std::thread::sleep(Duration::from_secs(2));
        // Con la pantalla quieta (o no), un keyframe pedido sale igual.
        let before = frames.lock().unwrap().len();
        stream.request_keyframe();
        std::thread::sleep(Duration::from_millis(400));
        drop(stream);
        let frames = std::mem::take(&mut *frames.lock().unwrap());
        assert_eq!(frames[0].0, crate::h264::out::FORMAT);
        assert!(frames[before..].iter().any(|(flags, ..)| flags & crate::h264::out::KEY != 0), "salió el keyframe pedido");
        let mut decoder = crate::h264::Decoder::new().unwrap();
        let (mut shown, mut bytes) = (0, 0);
        for (flags, data, _) in &frames[1..] {
            bytes += data.len();
            if decoder.decode(data).unwrap_or_else(|e| panic!("cuadro {flags}: {e}")).is_some() {
                shown += 1;
            }
        }
        let span = frames.last().unwrap().2 - frames[1].2;
        println!("{} cuadros en {:?} ({} decodificados), {} KB; primer keyframe {} KB", frames.len() - 1, span, shown, bytes / 1024, frames[1].1.len() / 1024);
        assert!(shown > 0);
    }

    #[test]
    fn avcc_pasa_a_annex_b() {
        let avcc = [0, 0, 0, 2, 0x65, 1, 0, 0, 0, 1, 0x41];
        let mut out = vec![9];
        assert!(avcc_to_annex_b(&avcc, &mut out));
        assert_eq!(out, vec![9, 0, 0, 0, 1, 0x65, 1, 0, 0, 0, 1, 0x41]);
        // Un largo que se pasa del final no se acepta.
        assert!(!avcc_to_annex_b(&[0, 0, 0, 5, 1], &mut Vec::new()));
    }
}
