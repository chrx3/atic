//! El audio de una grabación de pantalla: micrófono y sonido del sistema con
//! `atic-audio` (la captura de Reuniones), alineados al reloj del video.
//!
//! `atic-audio` marca los bloques contando muestras, y el loopback de Windows
//! no entrega nada mientras no suena nada: con solo contar, cada silencio
//! adelantaría el audio. Aquí cada bloque se ubica por la hora en que llegó,
//! medida desde que empezó el video, y los huecos se rellenan con silencio.
//! Cada pista se pasa en el acto a 48 kHz estéreo y se escribe a un archivo
//! crudo (i16); al terminar se mezclan las dos en uno, que `screen_record`
//! codifica en AAC junto al video.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use atic_audio::{AudioTapChunk, CaptureConfig, CaptureHandle, CaptureSession, CaptureTrack};

pub const RATE: u32 = 48_000;
/// Bytes de un cuadro: dos canales i16.
pub const FRAME_BYTES: usize = 4;
/// Un hueco más largo que esto se rellena con silencio; lo menor es el
/// vaivén normal de los bloques.
const GAP_FRAMES: u64 = RATE as u64 / 10;

/// El reloj de una grabación: corre desde que empieza el video y se para en
/// las pausas. Lo comparten el video y el audio, así lo pausado no queda en
/// ninguno de los dos.
#[derive(Default)]
pub struct RecordClock {
    start: OnceLock<Instant>,
    pause: Mutex<Pause>,
}

#[derive(Default)]
struct Pause {
    /// Lo que duraron las pausas ya terminadas.
    total: Duration,
    /// La pausa en curso.
    since: Option<Instant>,
}

impl RecordClock {
    pub fn begin(&self, at: Instant) {
        let _ = self.start.set(at);
    }

    pub fn set_paused(&self, paused: bool) {
        let mut pause = self.pause.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        match (paused, pause.since) {
            (true, None) => pause.since = Some(Instant::now()),
            (false, Some(since)) => {
                pause.total += since.elapsed();
                pause.since = None;
            }
            _ => {}
        }
    }

    pub fn paused(&self) -> bool {
        self.pause.lock().map(|pause| pause.since.is_some()).unwrap_or(false)
    }

    /// Lo grabado hasta ahora, sin las pausas. `None` antes de empezar o en
    /// pausa: lo que llegue entonces no va.
    pub fn now(&self) -> Option<Duration> {
        let start = self.start.get()?;
        let pause = self.pause.lock().ok()?;
        if pause.since.is_some() {
            return None;
        }
        Some(start.elapsed().saturating_sub(pause.total))
    }

    /// Lo grabado, también en pausa (queda fijo): el reloj del tab y el largo
    /// final.
    pub fn recorded(&self) -> Duration {
        let (Some(start), Ok(pause)) = (self.start.get(), self.pause.lock()) else {
            return Duration::ZERO;
        };
        let end = pause.since.unwrap_or_else(Instant::now);
        end.saturating_duration_since(*start).saturating_sub(pause.total)
    }
}

/// Qué pistas grabar.
#[derive(Clone, Copy, Debug)]
pub struct Tracks {
    pub mic: bool,
    pub system: bool,
}

/// El audio mezclado: 48 kHz, estéreo, i16 intercalado, sin cabecera.
pub struct Mixed {
    pub path: PathBuf,
    pub frames: u64,
}

pub struct ScreenAudio {
    capture: CaptureHandle,
    worker: JoinHandle<std::io::Result<Vec<(PathBuf, u64)>>>,
    dir: PathBuf,
}

impl ScreenAudio {
    /// Abre las pistas pedidas en `dir` (una carpeta temporal propia). Con
    /// los dispositivos de Ajustes, los mismos de Reuniones. Lo que llegue
    /// antes de que `clock` empiece, o en pausa, se descarta.
    pub fn start(tracks: Tracks, dir: PathBuf, clock: Arc<RecordClock>) -> Result<Self, String> {
        std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        let config = atic_core::AppDirs::new()
            .map(|dirs| atic_core::Config::load(&dirs.config_path()))
            .unwrap_or_default();
        // Holgado: el consumidor solo escribe a disco, pero si se atrasa
        // `atic-audio` descarta bloques en vez de esperar.
        let (tap, chunks) = mpsc::sync_channel::<AudioTapChunk>(512);
        let (events, _) = mpsc::channel();
        let capture = CaptureSession::start(
            CaptureConfig {
                mic_wav: dir.join("mic.wav"),
                system_wav: dir.join("system.wav"),
                capture_mic: tracks.mic,
                capture_system: tracks.system,
                noise_suppression: config.noise_suppression.clone(),
                mic_device_id: config.mic_device_id.clone(),
                output_device_id: config.output_device_id.clone(),
                stt_tap: Some(tap),
                english: false,
            },
            events,
        )
        .map_err(|error| error.to_ui(false))?;
        let worker = {
            let dir = dir.clone();
            std::thread::Builder::new()
                .name("grabar audio".into())
                .spawn(move || align(chunks, &clock, &dir))
                .map_err(|error| error.to_string())?
        };
        Ok(Self { capture, worker, dir })
    }

    /// Cierra las pistas y las mezcla, sin pasar de `duration` (lo que duró
    /// el video). `None` si no quedó nada.
    pub fn finish(self, duration: Duration) -> Result<Option<Mixed>, String> {
        let summary = self.capture.stop();
        let tracks = self.worker.join().map_err(|_| "el audio se cortó".to_string())?;
        let tracks = tracks.map_err(|error| error.to_string())?;
        // Los WAV de `atic-audio` no se usan: el audio alineado es el crudo.
        let _ = std::fs::remove_file(self.dir.join("mic.wav"));
        let _ = std::fs::remove_file(self.dir.join("system.wav"));
        println!(
            "grabar: audio {:.1} s (mic {}, sistema {})",
            summary.duration_secs, summary.mic_written, summary.system_written
        );
        let cap = (duration.as_secs_f64() * RATE as f64) as u64;
        let tracks: Vec<_> = tracks.into_iter().filter(|(_, frames)| *frames > 0).collect();
        if tracks.is_empty() {
            return Ok(None);
        }
        let path = self.dir.join("mezcla.raw");
        let frames = mix(&tracks, cap, &path).map_err(|error| error.to_string())?;
        for (track, _) in &tracks {
            let _ = std::fs::remove_file(track);
        }
        Ok(Some(Mixed { path, frames }))
    }
}

/// Una pista en camino a disco: 48 kHz estéreo, alineada al video.
struct Aligned {
    out: BufWriter<File>,
    path: PathBuf,
    written: u64,
    resampler: Resampler,
    scratch: Vec<[f32; 2]>,
}

impl Aligned {
    fn create(path: PathBuf, rate: u32) -> std::io::Result<Self> {
        Ok(Self {
            out: BufWriter::new(File::create(&path)?),
            path,
            written: 0,
            resampler: Resampler::new(rate),
            scratch: Vec::new(),
        })
    }

    fn silence(&mut self, frames: u64) -> std::io::Result<()> {
        for _ in 0..frames {
            self.out.write_all(&[0; FRAME_BYTES])?;
        }
        self.written += frames;
        Ok(())
    }

    fn push(&mut self, frames: &[[f32; 2]]) -> std::io::Result<()> {
        self.scratch.clear();
        self.resampler.push(frames, &mut self.scratch);
        for [left, right] in &self.scratch {
            self.out.write_all(&to_i16(*left).to_le_bytes())?;
            self.out.write_all(&to_i16(*right).to_le_bytes())?;
        }
        self.written += self.scratch.len() as u64;
        Ok(())
    }
}

fn to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
}

/// Recibe los bloques de las dos pistas hasta que la captura se cierra.
fn align(
    chunks: mpsc::Receiver<AudioTapChunk>,
    clock: &RecordClock,
    dir: &Path,
) -> std::io::Result<Vec<(PathBuf, u64)>> {
    let mut mic: Option<Aligned> = None;
    let mut system: Option<Aligned> = None;
    let mut frames: Vec<[f32; 2]> = Vec::new();
    loop {
        let chunk = match chunks.recv_timeout(Duration::from_millis(200)) {
            Ok(chunk) => chunk,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        let Some(now) = clock.now() else {
            continue;
        };
        let channels = chunk.channels.max(1) as usize;
        if chunk.sample_rate == 0 {
            continue;
        }
        frames.clear();
        frames.extend(chunk.samples.chunks_exact(channels).map(|frame| match frame {
            [mono] => [*mono, *mono],
            [left, right, ..] => [*left, *right],
            [] => [0.0, 0.0],
        }));
        // El bloque terminó al llegar: empezó su duración antes.
        let rate = chunk.sample_rate as f64;
        let began = now.as_secs_f64() - frames.len() as f64 / rate;
        // Lo de antes de que empezara el video no va.
        let skip = ((-began).max(0.0) * rate) as usize;
        if skip >= frames.len() {
            continue;
        }
        let (slot, name) = match chunk.track {
            CaptureTrack::Mic => (&mut mic, "mic.raw"),
            CaptureTrack::System => (&mut system, "sistema.raw"),
        };
        if slot.is_none() {
            *slot = Some(Aligned::create(dir.join(name), chunk.sample_rate)?);
        }
        let Some(track) = slot.as_mut() else {
            continue;
        };
        let expected = (began.max(0.0) * RATE as f64) as u64;
        if expected > track.written + GAP_FRAMES {
            track.silence(expected - track.written)?;
        }
        track.push(&frames[skip..])?;
    }
    let mut done = Vec::new();
    for track in [mic, system].into_iter().flatten() {
        let Aligned { mut out, path, written, .. } = track;
        out.flush()?;
        done.push((path, written));
    }
    Ok(done)
}

/// Suma las pistas cuadro a cuadro (con tope), `cap` cuadros justos.
fn mix(tracks: &[(PathBuf, u64)], cap: u64, out: &Path) -> std::io::Result<u64> {
    // Hasta donde llegó el video: una pista que terminó antes (el sistema
    // en silencio no entrega nada) se completa con silencio.
    let frames = cap;
    let mut readers = tracks
        .iter()
        .map(|(path, _)| File::open(path).map(BufReader::new))
        .collect::<std::io::Result<Vec<_>>>()?;
    let mut writer = BufWriter::new(File::create(out)?);
    const BLOCK: usize = RATE as usize / 10;
    let mut sum = vec![0i32; BLOCK * 2];
    let mut bytes = vec![0u8; BLOCK * FRAME_BYTES];
    let mut left = frames;
    while left > 0 {
        let n = (left as usize).min(BLOCK);
        sum[..n * 2].fill(0);
        for reader in &mut readers {
            let buf = &mut bytes[..n * FRAME_BYTES];
            let got = read_up_to(reader, buf)?;
            buf[got..].fill(0);
            for (acc, sample) in sum.iter_mut().zip(buf.chunks_exact(2)) {
                *acc += i16::from_le_bytes([sample[0], sample[1]]) as i32;
            }
        }
        for sample in &sum[..n * 2] {
            let clamped = (*sample).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            writer.write_all(&clamped.to_le_bytes())?;
        }
        left -= n as u64;
    }
    writer.flush()?;
    Ok(frames)
}

/// Como `read_exact`, pero una pista más corta no es error: devuelve cuánto leyó.
pub(crate) fn read_up_to(reader: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut got = 0;
    while got < buf.len() {
        match reader.read(&mut buf[got..])? {
            0 => break,
            n => got += n,
        }
    }
    Ok(got)
}

/// Pasa cuadros estéreo de una frecuencia a `RATE`, interpolando entre
/// vecinos. Guarda el último cuadro para seguir entre bloques sin saltos.
struct Resampler {
    step: f64,
    /// Posición de la próxima salida, en cuadros de entrada; 0 es `last`.
    pos: f64,
    last: [f32; 2],
}

impl Resampler {
    fn new(rate: u32) -> Self {
        Self {
            step: rate as f64 / RATE as f64,
            pos: 0.0,
            last: [0.0; 2],
        }
    }

    fn push(&mut self, input: &[[f32; 2]], out: &mut Vec<[f32; 2]>) {
        let Some(&final_frame) = input.last() else {
            return;
        };
        let last = self.last;
        let at = |i: usize| if i == 0 { last } else { input[i - 1] };
        let n = input.len() as f64;
        // Hace falta el vecino `i + 1`, que existe mientras `pos < n`.
        while self.pos < n {
            let i = self.pos.floor() as usize;
            let t = (self.pos - i as f64) as f32;
            let (a, b) = (at(i), at(i + 1));
            out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
            self.pos += self.step;
        }
        self.pos -= n;
        self.last = final_frame;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(n: usize) -> Vec<[f32; 2]> {
        (0..n).map(|i| [i as f32, -(i as f32)]).collect()
    }

    #[test]
    fn a_48_khz_pasa_igual_con_un_cuadro_de_retraso() {
        let mut resampler = Resampler::new(RATE);
        let mut out = Vec::new();
        resampler.push(&ramp(4), &mut out);
        resampler.push(&ramp(4), &mut out);
        assert_eq!(out.len(), 8);
        assert_eq!(out[1], [0.0, -0.0]);
        assert_eq!(out[4], [3.0, -3.0]);
    }

    #[test]
    fn de_24_khz_salen_el_doble_de_cuadros_interpolados() {
        let mut resampler = Resampler::new(24_000);
        let mut out = Vec::new();
        for _ in 0..10 {
            resampler.push(&ramp(100), &mut out);
        }
        assert!((1998..=2002).contains(&out.len()), "{}", out.len());
        // Entre 0 y 1 de la rampa queda el medio.
        assert_eq!(out[3], [0.5, -0.5]);
    }

    #[test]
    fn de_44_1_khz_conserva_la_duracion() {
        let mut resampler = Resampler::new(44_100);
        let mut out = Vec::new();
        for _ in 0..100 {
            resampler.push(&vec![[0.25, 0.25]; 441], &mut out);
        }
        assert!((47_995..=48_005).contains(&out.len()), "{}", out.len());
    }

    #[test]
    fn el_reloj_no_cuenta_las_pausas() {
        let clock = RecordClock::default();
        assert_eq!(clock.now(), None);
        clock.begin(Instant::now() - Duration::from_secs(10));
        assert!(clock.now().unwrap() >= Duration::from_secs(10));
        clock.set_paused(true);
        assert!(clock.paused());
        assert_eq!(clock.now(), None);
        let frozen = clock.recorded();
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(clock.recorded(), frozen);
        clock.set_paused(false);
        let after = clock.now().unwrap();
        assert!(after >= frozen && after < frozen + Duration::from_millis(20), "{after:?} {frozen:?}");
    }

    #[test]
    fn la_mezcla_suma_con_tope_y_rellena_la_pista_corta() {
        let dir = std::env::temp_dir().join(format!("atic-mezcla-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let write = |name: &str, samples: &[i16]| {
            let path = dir.join(name);
            let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
            std::fs::write(&path, bytes).unwrap();
            (path, samples.len() as u64 / 2)
        };
        let a = write("a.raw", &[1000, 1000, 30000, 30000, 5, 5]);
        let b = write("b.raw", &[-500, -500, 30000, 30000]);
        let out = dir.join("m.raw");
        assert_eq!(mix(&[a, b], 3, &out).unwrap(), 3);
        let mixed: Vec<i16> = std::fs::read(&out)
            .unwrap()
            .chunks_exact(2)
            .map(|s| i16::from_le_bytes([s[0], s[1]]))
            .collect();
        assert_eq!(mixed, vec![500, 500, i16::MAX, i16::MAX, 5, 5]);
        // El tope corta en lo que duró el video.
        let (a, b) = (dir.join("a.raw"), dir.join("b.raw"));
        assert_eq!(mix(&[(a.clone(), 3), (b.clone(), 2)], 1, &out).unwrap(), 1);
        // Y una pista corta se alarga hasta el video.
        assert_eq!(mix(&[(a, 3), (b, 2)], 5, &out).unwrap(), 5);
        assert_eq!(std::fs::read(&out).unwrap()[12..], [0; 8]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
