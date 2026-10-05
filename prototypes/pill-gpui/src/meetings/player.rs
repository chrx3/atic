//! El motor del reproductor de Reuniones: las dos pistas WAV de una grabación
//! (micrófono y sistema), mezcladas o por separado, por la salida de audio
//! por defecto.
//!
//! Un hilo propio decodifica con `hound` en trozos (una hora de audio no se
//! carga entera) y deja ~150 ms listos en un búfer; el callback de `cpal` solo
//! copia de ahí, sin tocar disco. La posición que suena se publica en un
//! atómico: la UI la lee cuando quiere, sin mensajes de ida y vuelta.
//!
//! El dispositivo se abre al reproducir y se suelta al detener (cambio de
//! reunión) o al soltar el `Player` (cerrar la ventana).

use std::collections::VecDeque;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use super::stretch::{self, Stretch};

/// Qué se escucha. Como en la web: «Todos» suma las dos pistas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Track {
    Mix,
    Mic,
    System,
}

impl Track {
    pub fn label(self) -> &'static str {
        match self {
            Track::Mix => "Todos",
            Track::Mic => "Yo",
            Track::System => "Otros",
        }
    }
}

/// Las pistas que existen de verdad en disco.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Files {
    pub mic: Option<PathBuf>,
    pub system: Option<PathBuf>,
}

impl Files {
    /// Las rutas de `Recording` son relativas a su carpeta; las que no están
    /// en disco no cuentan (una importación suele traer solo micrófono).
    pub fn resolve(dir: &Path, mic: Option<&str>, system: Option<&str>) -> Self {
        let find = |name: Option<&str>| {
            name.filter(|n| !n.trim().is_empty())
                .map(|n| dir.join(n))
                .filter(|p| p.is_file())
        };
        Self { mic: find(mic), system: find(system) }
    }

    pub fn is_empty(&self) -> bool {
        self.mic.is_none() && self.system.is_none()
    }

    /// Las opciones del selector, en orden; con una sola pista no hay selector.
    pub fn options(&self) -> Vec<Track> {
        let mut out = Vec::new();
        if self.mic.is_some() && self.system.is_some() {
            out.push(Track::Mix);
        }
        if self.mic.is_some() {
            out.push(Track::Mic);
        }
        if self.system.is_some() {
            out.push(Track::System);
        }
        out
    }

    /// La pista por defecto, o la que sí existe si piden una que no.
    pub fn resolve_track(&self, wanted: Track) -> Track {
        let (mic, sys) = (self.mic.is_some(), self.system.is_some());
        match wanted {
            Track::Mix if mic && sys => Track::Mix,
            Track::Mic if mic => Track::Mic,
            Track::System if sys => Track::System,
            _ if mic && sys => Track::Mix,
            _ if sys => Track::System,
            _ => Track::Mic,
        }
    }

    pub fn paths(&self, track: Track) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if matches!(track, Track::Mix | Track::Mic) {
            out.extend(self.mic.clone());
        }
        if matches!(track, Track::Mix | Track::System) {
            out.extend(self.system.clone());
        }
        out
    }
}

// --- Lógica pura: canales, mezcla, remuestreo, picos -------------------------------

pub type Frame = [f32; 2];

/// Cualquier cantidad de canales a estéreo: mono se duplica; de más de dos
/// quedan los dos primeros (Atic graba mono o estéreo).
pub fn to_stereo(samples: &[f32]) -> Frame {
    match samples {
        [] => [0.0, 0.0],
        [mono] => [*mono, *mono],
        [l, r, ..] => [*l, *r],
    }
}

/// Suma de pistas recortada a ±1: dos `<audio>` sonando juntos en la web
/// también se suman.
pub fn mix(frames: &[Frame]) -> Frame {
    let mut out = [0.0f32; 2];
    for f in frames {
        out[0] += f[0];
        out[1] += f[1];
    }
    [out[0].clamp(-1.0, 1.0), out[1].clamp(-1.0, 1.0)]
}

/// Un cuadro estéreo a los canales del dispositivo.
pub fn write_frame(frame: Frame, out: &mut [f32]) {
    match out.len() {
        0 => {}
        1 => out[0] = (frame[0] + frame[1]) * 0.5,
        _ => {
            out[0] = frame[0];
            out[1] = frame[1];
            for x in &mut out[2..] {
                *x = 0.0;
            }
        }
    }
}

/// Remuestreo lineal: suficiente para voz y barato. `step` = tasa de origen /
/// tasa de salida.
#[derive(Clone, Debug)]
pub struct Linear {
    step: f64,
    pos: f64,
    prev: Frame,
    next: Frame,
    primed: bool,
    last: bool,
    finished: bool,
}

impl Linear {
    pub fn new(from_rate: u32, to_rate: u32) -> Self {
        Self {
            step: from_rate.max(1) as f64 / to_rate.max(1) as f64,
            pos: 0.0,
            prev: [0.0; 2],
            next: [0.0; 2],
            primed: false,
            last: false,
            finished: false,
        }
    }

    /// Tras saltar: lo de antes ya no sirve para interpolar.
    pub fn reset(&mut self) {
        *self = Self::new(1, 1).with_step(self.step);
    }

    fn with_step(mut self, step: f64) -> Self {
        self.step = step;
        self
    }

    /// El próximo cuadro de salida; `pull` entrega los de origen.
    pub fn next(&mut self, mut pull: impl FnMut() -> Option<Frame>) -> Option<Frame> {
        if !self.primed {
            self.prev = pull()?;
            self.next = pull().unwrap_or_else(|| {
                self.last = true;
                self.prev
            });
            self.primed = true;
        }
        if self.finished {
            return None;
        }
        let t = self.pos as f32;
        let out = [
            self.prev[0] + (self.next[0] - self.prev[0]) * t,
            self.prev[1] + (self.next[1] - self.prev[1]) * t,
        ];
        self.pos += self.step;
        while self.pos >= 1.0 {
            self.pos -= 1.0;
            self.prev = self.next;
            match pull() {
                Some(frame) => self.next = frame,
                None if self.last => {
                    self.finished = true;
                    break;
                }
                None => self.last = true,
            }
        }
        Some(out)
    }
}

/// Cuántas barras de onda se calculan por grabación: de sobra para el ancho
/// de la barra, que las agrupa al dibujar.
pub const PEAK_BARS: usize = 480;

/// Acumula el máximo de `value` en la barra que le toca al segundo `at`.
pub fn add_peak(peaks: &mut [f32], at_secs: f64, total_secs: f64, value: f32) {
    if peaks.is_empty() || total_secs <= 0.0 {
        return;
    }
    let ix = ((at_secs / total_secs) * peaks.len() as f64) as usize;
    let slot = &mut peaks[ix.min(peaks.len() - 1)];
    if value > *slot {
        *slot = value;
    }
}

/// Lleva los picos a 0..1 contra el más alto de todas las pistas, con un
/// piso: una grabación casi muda no se infla hasta parecer ruido fuerte.
pub fn normalize(tracks: &mut [&mut Vec<f32>]) {
    let top = tracks
        .iter()
        .flat_map(|t| t.iter().copied())
        .fold(0.0f32, f32::max)
        .max(0.05);
    for track in tracks.iter_mut() {
        for v in track.iter_mut() {
            // Raíz suave: la voz baja no desaparece al lado de un golpe.
            *v = (*v / top).clamp(0.0, 1.0).powf(0.7);
        }
    }
}

/// Agrupa `peaks` en `n` barras (el máximo de cada grupo) para dibujarlas.
pub fn bucket(peaks: &[f32], n: usize) -> Vec<f32> {
    if n == 0 || peaks.is_empty() {
        return Vec::new();
    }
    (0..n)
        .map(|i| {
            let a = i * peaks.len() / n;
            let b = ((i + 1) * peaks.len() / n).max(a + 1).min(peaks.len());
            peaks[a..b].iter().copied().fold(0.0, f32::max)
        })
        .collect()
}

// --- Lectura de WAV ----------------------------------------------------------------

type Reader = hound::WavReader<BufReader<File>>;

/// Una pista abierta: lee en trozos y entrega cuadros estéreo.
struct Source {
    reader: Reader,
    channels: usize,
    rate: u32,
    float: bool,
    scale: f32,
    pending: VecDeque<Frame>,
    eof: bool,
    resampler: Linear,
}

const CHUNK_FRAMES: usize = 2048;

impl Source {
    fn open(path: &Path, out_rate: u32) -> anyhow::Result<Self> {
        let reader = hound::WavReader::open(path)?;
        let spec = reader.spec();
        Ok(Self {
            channels: spec.channels.max(1) as usize,
            rate: spec.sample_rate,
            float: spec.sample_format == hound::SampleFormat::Float,
            scale: 1.0 / (1u64 << (spec.bits_per_sample.clamp(1, 32) - 1)) as f32,
            reader,
            pending: VecDeque::with_capacity(CHUNK_FRAMES),
            eof: false,
            resampler: Linear::new(spec.sample_rate, out_rate),
        })
    }

    fn duration_ms(&self) -> u64 {
        self.reader.duration() as u64 * 1000 / self.rate.max(1) as u64
    }

    fn seek(&mut self, ms: u64) {
        let frame = (ms * self.rate as u64 / 1000).min(self.reader.duration() as u64) as u32;
        self.pending.clear();
        self.resampler.reset();
        self.eof = self.reader.seek(frame).is_err();
    }

    /// El próximo cuadro ya a la tasa de salida.
    fn next(&mut self) -> Option<Frame> {
        let Self { reader, channels, float, scale, pending, eof, resampler, .. } = self;
        resampler.next(|| {
            if pending.is_empty() && !*eof {
                *eof = refill(reader, *channels, *float, *scale, pending);
            }
            pending.pop_front()
        })
    }
}

/// Lee un trozo de la pista a `pending`; `true` si se acabó.
fn refill(reader: &mut Reader, ch: usize, float: bool, scale: f32, pending: &mut VecDeque<Frame>) -> bool {
    let mut buf = Vec::with_capacity(ch);
    macro_rules! read {
        ($ty:ty, $conv:expr) => {{
            let mut samples = reader.samples::<$ty>();
            for _ in 0..CHUNK_FRAMES {
                buf.clear();
                for _ in 0..ch {
                    match samples.next() {
                        Some(Ok(s)) => buf.push($conv(s)),
                        _ => return true,
                    }
                }
                pending.push_back(to_stereo(&buf));
            }
            false
        }};
    }
    if float {
        read!(f32, |s: f32| s)
    } else {
        read!(i32, |s: i32| s as f32 * scale)
    }
}

/// Lee una pista entera para la onda, sin guardarla: solo los máximos.
pub fn track_peaks(path: &Path, total_secs: f64) -> anyhow::Result<(Vec<f32>, f64)> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let ch = spec.channels.max(1) as usize;
    let rate = spec.sample_rate.max(1) as f64;
    let secs = reader.duration() as f64 / rate;
    let total = if total_secs > 0.0 { total_secs } else { secs };
    let mut peaks = vec![0.0f32; PEAK_BARS];
    let scale = 1.0 / (1u64 << (spec.bits_per_sample.clamp(1, 32) - 1)) as f32;
    let mut frame = 0u64;
    let mut acc = 0.0f32;
    let mut n = 0usize;
    let mut push = |v: f32, peaks: &mut Vec<f32>| {
        acc += v;
        n += 1;
        if n == ch {
            add_peak(peaks, frame as f64 / rate, total, (acc / ch as f32).abs());
            frame += 1;
            acc = 0.0;
            n = 0;
        }
    };
    if spec.sample_format == hound::SampleFormat::Float {
        for s in reader.samples::<f32>() {
            push(s?, &mut peaks);
        }
    } else {
        for s in reader.samples::<i32>() {
            push(s? as f32 * scale, &mut peaks);
        }
    }
    Ok((peaks, secs))
}

/// Duración de la pista más larga, en ms (lee solo la cabecera).
pub fn duration_ms(paths: &[PathBuf]) -> u64 {
    paths
        .iter()
        .filter_map(|p| hound::WavReader::open(p).ok())
        .map(|r| r.duration() as u64 * 1000 / r.spec().sample_rate.max(1) as u64)
        .max()
        .unwrap_or(0)
}

// --- El hilo de audio --------------------------------------------------------------

enum Cmd {
    /// Abre estas pistas desde `at_ms`; `play` arranca de una.
    Load { paths: Vec<PathBuf>, at_ms: u64, play: bool },
    Play,
    Pause,
    Seek(u64),
    /// Otra velocidad (ver `stretch`).
    Speed,
    /// Suelta el dispositivo y las pistas.
    Stop,
}

/// Lo que el hilo publica para la UI.
#[derive(Default)]
struct Shared {
    pos_ms: AtomicU64,
    duration_ms: AtomicU64,
    playing: AtomicBool,
    /// Lo pone el callback al vaciar el búfer con la pista ya terminada.
    ended: AtomicBool,
    error: Mutex<Option<String>>,
    /// La velocidad (bits de un `f32`; 0 es 1×).
    speed: AtomicU32,
}

impl Shared {
    fn speed(&self) -> f32 {
        match self.speed.load(Ordering::Relaxed) {
            0 => 1.0,
            bits => f32::from_bits(bits),
        }
    }
}

/// Lo decodificado que espera al callback, intercalado con los canales del
/// dispositivo. `tail_ms` es el tiempo del último cuadro agregado: lo que suena
/// es eso menos lo que queda en la cola. Los tiempos son de la grabación: a
/// 1,5× cada cuadro de salida vale 1,5 cuadros de ella (`ms_per_frame`).
struct Buf {
    samples: VecDeque<f32>,
    tail_ms: f64,
    ms_per_frame: f64,
    channels: usize,
    /// Las pistas se acabaron: lo que queda en la cola es el final.
    done: bool,
    playing: bool,
}

impl Buf {
    fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1)
    }
}

/// Lo que se tiene listo por delante: chico para que saltar responda,
/// suficiente para que un tirón del hilo no se oiga.
const AHEAD_MS: f64 = 150.0;

/// El reproductor. Se crea sin costo; el hilo nace al primer uso y el
/// dispositivo se abre al reproducir.
pub struct Player {
    tx: Option<Sender<Cmd>>,
    thread: Option<JoinHandle<()>>,
    shared: Arc<Shared>,
}

impl Default for Player {
    fn default() -> Self {
        Self { tx: None, thread: None, shared: Arc::new(Shared::default()) }
    }
}

impl Player {
    fn send(&mut self, cmd: Cmd) {
        if self.tx.is_none() {
            let (tx, rx) = mpsc::channel();
            let shared = self.shared.clone();
            match std::thread::Builder::new()
                .name("reuniones-audio".into())
                .spawn(move || worker(rx, shared))
            {
                Ok(handle) => {
                    self.tx = Some(tx);
                    self.thread = Some(handle);
                }
                Err(error) => {
                    self.set_error(format!("No se pudo iniciar el audio: {error}"));
                    return;
                }
            }
        }
        if let Some(tx) = &self.tx {
            let _ = tx.send(cmd);
        }
    }

    fn set_error(&self, error: String) {
        if let Ok(mut slot) = self.shared.error.lock() {
            *slot = Some(error);
        }
    }

    pub fn load(&mut self, paths: Vec<PathBuf>, at_ms: u64, play: bool) {
        self.shared.pos_ms.store(at_ms, Ordering::Relaxed);
        self.shared.ended.store(false, Ordering::Relaxed);
        if play {
            self.shared.playing.store(true, Ordering::Relaxed);
        }
        if let Ok(mut slot) = self.shared.error.lock() {
            *slot = None;
        }
        self.send(Cmd::Load { paths, at_ms, play });
    }

    pub fn play(&mut self) {
        self.shared.playing.store(true, Ordering::Relaxed);
        self.send(Cmd::Play);
    }

    pub fn pause(&mut self) {
        self.shared.playing.store(false, Ordering::Relaxed);
        self.send(Cmd::Pause);
    }

    /// La posición se publica ya: la barra salta sin esperar al hilo.
    pub fn seek(&mut self, ms: u64) {
        let ms = match self.duration_ms() {
            0 => ms,
            d => ms.min(d),
        };
        self.shared.pos_ms.store(ms, Ordering::Relaxed);
        self.send(Cmd::Seek(ms));
    }

    /// La posición, la onda y los saltos siguen en tiempo de la grabación;
    /// solo cambia lo rápido que avanza.
    pub fn set_speed(&mut self, speed: f32) {
        if (self.shared.speed() - speed).abs() < 0.001 {
            return;
        }
        self.shared.speed.store(speed.to_bits(), Ordering::Relaxed);
        if self.tx.is_some() {
            self.send(Cmd::Speed);
        }
    }

    pub fn stop(&mut self) {
        self.shared.playing.store(false, Ordering::Relaxed);
        self.shared.pos_ms.store(0, Ordering::Relaxed);
        self.shared.duration_ms.store(0, Ordering::Relaxed);
        if self.tx.is_some() {
            self.send(Cmd::Stop);
        }
    }

    pub fn playing(&self) -> bool {
        self.shared.playing.load(Ordering::Relaxed)
    }

    pub fn position_ms(&self) -> u64 {
        self.shared.pos_ms.load(Ordering::Relaxed)
    }

    pub fn duration_ms(&self) -> u64 {
        self.shared.duration_ms.load(Ordering::Relaxed)
    }

    pub fn error(&self) -> Option<String> {
        self.shared.error.lock().ok().and_then(|e| e.clone())
    }
}

impl Drop for Player {
    /// Cerrar la ventana suelta el dispositivo: sin el canal el hilo termina
    /// y se lleva el stream.
    fn drop(&mut self) {
        self.tx = None;
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

struct Output {
    stream: cpal::Stream,
    rate: u32,
    buf: Arc<Mutex<Buf>>,
}

fn open_output(shared: &Arc<Shared>) -> anyhow::Result<Output> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| anyhow::anyhow!("no hay salida de audio"))?;
    let supported = device.default_output_config()?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let channels = config.channels.max(1) as usize;
    let rate = config.sample_rate.0;
    let buf = Arc::new(Mutex::new(Buf {
        samples: VecDeque::with_capacity(rate as usize * channels / 2),
        tail_ms: 0.0,
        ms_per_frame: 1000.0 / rate.max(1) as f64 * shared.speed() as f64,
        channels,
        done: false,
        playing: false,
    }));
    let stream = match format {
        cpal::SampleFormat::F32 => build::<f32>(&device, &config, buf.clone(), shared.clone())?,
        cpal::SampleFormat::I16 => build::<i16>(&device, &config, buf.clone(), shared.clone())?,
        cpal::SampleFormat::U16 => build::<u16>(&device, &config, buf.clone(), shared.clone())?,
        cpal::SampleFormat::I32 => build::<i32>(&device, &config, buf.clone(), shared.clone())?,
        other => anyhow::bail!("formato de salida no soportado: {other:?}"),
    };
    Ok(Output { stream, rate, buf })
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    buf: Arc<Mutex<Buf>>,
    shared: Arc<Shared>,
) -> anyhow::Result<cpal::Stream>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let errors = shared.clone();
    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            // Si el hilo tiene el candado justo ahora, un trozo de silencio:
            // nunca se espera dentro del callback.
            let Ok(mut b) = buf.try_lock() else {
                data.fill(T::from_sample(0.0));
                return;
            };
            if !b.playing {
                data.fill(T::from_sample(0.0));
                return;
            }
            for out in data.iter_mut() {
                *out = T::from_sample(b.samples.pop_front().unwrap_or(0.0));
            }
            let pos = (b.tail_ms - b.frames() as f64 * b.ms_per_frame).max(0.0);
            shared.pos_ms.store(pos as u64, Ordering::Relaxed);
            if b.done && b.samples.is_empty() {
                b.playing = false;
                shared.ended.store(true, Ordering::Relaxed);
            }
        },
        move |error| {
            if let Ok(mut slot) = errors.error.lock() {
                *slot = Some(format!("Falló la salida de audio: {error}"));
            }
        },
        None,
    )?;
    Ok(stream)
}

/// El estado del hilo: las pistas abiertas y, si se está oyendo, la salida.
struct Engine {
    shared: Arc<Shared>,
    paths: Vec<PathBuf>,
    sources: Vec<Source>,
    output: Option<Output>,
    scratch: Vec<Frame>,
    frame_out: Vec<f32>,
    /// A la tasa de la salida; nace con ella.
    stretch: Option<Stretch>,
}

impl Engine {
    fn fail(&mut self, error: String) {
        eprintln!("reuniones: {error}");
        if let Ok(mut slot) = self.shared.error.lock() {
            *slot = Some(error);
        }
        self.shared.playing.store(false, Ordering::Relaxed);
    }

    fn ensure_output(&mut self) -> bool {
        if self.output.is_some() {
            return true;
        }
        match open_output(&self.shared) {
            Ok(output) => {
                self.stretch = Some(Stretch::new(output.rate, self.shared.speed()));
                self.output = Some(output);
                // Las pistas se abren a la tasa del dispositivo.
                let at = self.shared.pos_ms.load(Ordering::Relaxed);
                self.open_sources(at);
                true
            }
            Err(error) => {
                self.fail(format!("No se pudo abrir la salida de audio: {error}"));
                false
            }
        }
    }

    fn open_sources(&mut self, at_ms: u64) {
        let rate = self.output.as_ref().map_or(48_000, |o| o.rate);
        self.sources.clear();
        for path in self.paths.clone() {
            match Source::open(&path, rate) {
                Ok(source) => self.sources.push(source),
                Err(error) => self.fail(format!("No se pudo leer {}: {error}", path.display())),
            }
        }
        let duration = self.sources.iter().map(Source::duration_ms).max().unwrap_or(0);
        self.shared.duration_ms.store(duration, Ordering::Relaxed);
        self.seek(at_ms);
    }

    fn seek(&mut self, ms: u64) {
        for source in &mut self.sources {
            source.seek(ms);
        }
        self.shared.pos_ms.store(ms, Ordering::Relaxed);
        self.shared.ended.store(false, Ordering::Relaxed);
        let speed = self.shared.speed();
        if let Some(stretch) = &mut self.stretch {
            stretch.reset(speed);
        }
        if let Some(output) = &self.output {
            if let Ok(mut b) = output.buf.lock() {
                b.samples.clear();
                b.tail_ms = ms as f64;
                b.ms_per_frame = 1000.0 / output.rate.max(1) as f64 * speed as f64;
                b.done = false;
            }
        }
    }

    fn set_playing(&mut self, on: bool) {
        if on && !self.ensure_output() {
            return;
        }
        self.shared.playing.store(on, Ordering::Relaxed);
        let Some(output) = &self.output else {
            return;
        };
        if let Ok(mut b) = output.buf.lock() {
            b.playing = on;
        }
        let result = if on {
            output.stream.play().map_err(|e| e.to_string())
        } else {
            output.stream.pause().map_err(|e| e.to_string())
        };
        if let Err(error) = result {
            self.fail(format!("No se pudo controlar la salida de audio: {error}"));
        }
    }

    fn release(&mut self) {
        self.output = None;
        self.stretch = None;
        self.sources.clear();
        self.paths.clear();
        self.shared.playing.store(false, Ordering::Relaxed);
    }

    /// Decodifica hasta tener `AHEAD_MS` en la cola.
    fn fill(&mut self) {
        let Some(output) = &self.output else {
            return;
        };
        let (want, channels) = {
            let Ok(b) = output.buf.lock() else {
                return;
            };
            if !b.playing || b.done {
                return;
            }
            let ahead = b.frames() as f64 * b.ms_per_frame;
            if ahead >= AHEAD_MS {
                return;
            }
            (((AHEAD_MS - ahead) / b.ms_per_frame) as usize + 1, b.channels)
        };
        // Se decodifica fuera del candado: el callback no espera al disco.
        let mut chunk = Vec::with_capacity(want * channels);
        let mut done = false;
        self.frame_out.resize(channels, 0.0);
        let Self { sources, scratch, stretch, frame_out, .. } = self;
        let mut next_mixed = || {
            scratch.clear();
            for source in sources.iter_mut() {
                if let Some(frame) = source.next() {
                    scratch.push(frame);
                }
            }
            (!scratch.is_empty()).then(|| mix(scratch))
        };
        // A otra velocidad la mezcla pasa por el estirador; a 1× ni lo toca.
        let mut stretch = stretch.as_mut().filter(|s| !stretch::is_normal(s.speed()));
        for _ in 0..want {
            let frame = match stretch.as_deref_mut() {
                None => next_mixed(),
                Some(stretch) => loop {
                    if let Some(frame) = stretch.pop() {
                        break Some(frame);
                    }
                    // Los pocos ms que quedan dentro al final se sueltan.
                    match next_mixed() {
                        Some(frame) => stretch.push(frame),
                        None => break None,
                    }
                },
            };
            let Some(frame) = frame else {
                done = true;
                break;
            };
            write_frame(frame, frame_out);
            chunk.extend_from_slice(frame_out);
        }
        if let Ok(mut b) = output.buf.lock() {
            let frames = chunk.len() / channels.max(1);
            b.tail_ms += frames as f64 * b.ms_per_frame;
            b.samples.extend(chunk);
            b.done = done;
        }
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Load { paths, at_ms, play } => {
                self.paths = paths;
                self.open_sources(at_ms);
                self.set_playing(play);
            }
            Cmd::Play => {
                // Terminada, «reproducir» vuelve a empezar.
                if self.shared.ended.swap(false, Ordering::Relaxed) {
                    self.seek(0);
                }
                self.set_playing(true);
            }
            Cmd::Pause => self.set_playing(false),
            Cmd::Seek(ms) => self.seek(ms),
            // Desde lo que suena ahora: lo ya preparado iba a la otra velocidad.
            Cmd::Speed => {
                if !self.sources.is_empty() {
                    self.seek(self.shared.pos_ms.load(Ordering::Relaxed));
                }
            }
            Cmd::Stop => self.release(),
        }
    }
}

fn worker(rx: Receiver<Cmd>, shared: Arc<Shared>) {
    let mut engine = Engine {
        shared,
        paths: Vec::new(),
        sources: Vec::new(),
        output: None,
        scratch: Vec::with_capacity(2),
        frame_out: Vec::new(),
        stretch: None,
    };
    loop {
        // Sonando se despierta seguido para rellenar; quieto, solo con órdenes.
        let wait = if engine.shared.playing.load(Ordering::Relaxed) {
            Duration::from_millis(20)
        } else {
            Duration::from_secs(3600)
        };
        match rx.recv_timeout(wait) {
            Ok(cmd) => {
                engine.handle(cmd);
                while let Ok(cmd) = rx.try_recv() {
                    engine.handle(cmd);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if engine.shared.ended.load(Ordering::Relaxed) && engine.shared.playing.load(Ordering::Relaxed) {
            // Llegó al final: queda en pausa al principio, como la web.
            engine.set_playing(false);
            engine.seek(0);
            engine.shared.ended.store(true, Ordering::Relaxed);
        }
        engine.fill();
    }
    engine.release();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(n: usize) -> Vec<Frame> {
        (0..n).map(|i| [i as f32, -(i as f32)]).collect()
    }

    fn run(mut r: Linear, input: &[Frame]) -> Vec<Frame> {
        let mut it = input.iter().copied();
        std::iter::from_fn(|| r.next(|| it.next())).collect()
    }

    #[test]
    fn misma_tasa_deja_pasar_todo() {
        let input = frames(10);
        assert_eq!(run(Linear::new(48_000, 48_000), &input), input);
    }

    #[test]
    fn subir_la_tasa_interpola_y_duplica() {
        let out = run(Linear::new(24_000, 48_000), &frames(4));
        assert_eq!(out.len(), 8);
        assert_eq!(out[0], [0.0, 0.0]);
        assert_eq!(out[1], [0.5, -0.5]);
        assert_eq!(out[2], [1.0, -1.0]);
    }

    #[test]
    fn bajar_la_tasa_salta_cuadros() {
        let out = run(Linear::new(48_000, 16_000), &frames(30));
        assert_eq!(out.len(), 10);
        assert_eq!(out[1], [3.0, -3.0]);
    }

    #[test]
    fn una_pista_vacia_no_entrega_nada() {
        assert!(run(Linear::new(48_000, 44_100), &[]).is_empty());
        assert_eq!(run(Linear::new(48_000, 48_000), &[[0.3, 0.3]]).len(), 1);
    }

    #[test]
    fn canales_a_estereo_y_al_dispositivo() {
        assert_eq!(to_stereo(&[0.5]), [0.5, 0.5]);
        assert_eq!(to_stereo(&[0.1, 0.2, 0.9]), [0.1, 0.2]);
        let mut mono = [0.0];
        write_frame([0.2, 0.4], &mut mono);
        assert!((mono[0] - 0.3).abs() < 1e-6);
        let mut surround = [9.0; 6];
        write_frame([0.2, 0.4], &mut surround);
        assert_eq!(surround, [0.2, 0.4, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn la_mezcla_suma_y_recorta() {
        assert_eq!(mix(&[[0.25, -0.5], [0.25, -0.75]]), [0.5, -1.0]);
        assert_eq!(mix(&[]), [0.0, 0.0]);
    }

    #[test]
    fn picos_por_tiempo_y_normalizados() {
        let mut peaks = vec![0.0; 4];
        add_peak(&mut peaks, 0.0, 4.0, 0.2);
        add_peak(&mut peaks, 0.5, 4.0, 0.4);
        add_peak(&mut peaks, 3.9, 4.0, 0.8);
        add_peak(&mut peaks, 4.0, 4.0, 0.1); // el borde cae en la última
        assert_eq!(peaks, vec![0.4, 0.0, 0.0, 0.8]);
        let mut other = vec![0.0, 0.0, 0.8, 0.0];
        normalize(&mut [&mut peaks, &mut other]);
        assert_eq!(peaks[3], 1.0);
        assert_eq!(other[2], 1.0);
        assert!(peaks[0] > 0.5 && peaks[0] < 1.0);
    }

    #[test]
    fn el_silencio_no_se_infla() {
        let mut quiet = vec![0.001; 3];
        normalize(&mut [&mut quiet]);
        assert!(quiet[0] < 0.1);
    }

    #[test]
    fn agrupar_toma_el_maximo() {
        assert_eq!(bucket(&[0.1, 0.5, 0.2, 0.3], 2), vec![0.5, 0.3]);
        assert_eq!(bucket(&[0.4, 0.2], 4).len(), 4);
        assert!(bucket(&[], 3).is_empty());
    }

    #[test]
    fn la_pista_cae_a_la_que_existe() {
        let both = Files { mic: Some("m".into()), system: Some("s".into()) };
        let mic = Files { mic: Some("m".into()), system: None };
        let sys = Files { mic: None, system: Some("s".into()) };
        assert_eq!(both.options(), vec![Track::Mix, Track::Mic, Track::System]);
        assert_eq!(mic.options(), vec![Track::Mic]);
        assert_eq!(both.resolve_track(Track::Mix), Track::Mix);
        assert_eq!(mic.resolve_track(Track::Mix), Track::Mic);
        assert_eq!(sys.resolve_track(Track::Mic), Track::System);
        assert_eq!(both.paths(Track::System), vec![PathBuf::from("s")]);
        assert_eq!(both.paths(Track::Mix).len(), 2);
    }

    #[test]
    fn lee_un_wav_y_salta() {
        let dir = std::env::temp_dir().join(format!("atic-player-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tono.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 8_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        // Un segundo mudo y uno a media escala.
        for i in 0..16_000 {
            w.write_sample(if i < 8_000 { 0i16 } else { 16_384 }).unwrap();
        }
        w.finalize().unwrap();

        let mut source = Source::open(&path, 16_000).unwrap();
        assert_eq!(source.duration_ms(), 2_000);
        source.seek(1_500);
        let frame = source.next().unwrap();
        assert!((frame[0] - 0.5).abs() < 1e-3 && frame[0] == frame[1]);
        // A doble tasa, el medio segundo que queda son 8000 cuadros.
        assert_eq!(std::iter::from_fn(|| source.next()).count() + 1, 8_000);

        let (peaks, secs) = track_peaks(&path, 0.0).unwrap();
        assert_eq!(secs, 2.0);
        assert_eq!(peaks[0], 0.0);
        assert!((peaks[PEAK_BARS - 1] - 0.5).abs() < 1e-3);
        assert_eq!(duration_ms(&[path.clone()]), 2_000);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
