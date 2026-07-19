//! Supresión de ruido para la pista de micrófono.
//!
//! Pipeline (solo mic, nunca sistema):
//! 1. Downmix a mono
//! 2. High-pass (quita rumble; intensidad según nivel)
//! 3. Gate suave por muestra (solo `high`)
//! 4. Resample a 48 kHz si hace falta
//! 5. RNNoise (`nnnoiseless`) en frames de 480 samples
//! 6. Mezcla wet/dry **dentro del frame** (entrada vs salida RNNoise)
//! 7. Resample de vuelta al rate del WAV
//!
//! Mezclar wet/dry a 48 kHz (antes del downsample) evita comb filtering por
//! deriva de los resamplers lineales.
//!
//! El WAV de salida queda en mono (1 canal) cuando la supresión está activa.

use nnnoiseless::DenoiseState;

/// Intensidad de supresión de ruido en el micrófono.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoiseLevel {
    Low,
    Medium,
    High,
}

impl NoiseLevel {
    /// Interpreta el string de config (`low` | `medium` | `high`).
    /// Valores desconocidos → Medium.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "off" | "" => None,
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            _ => Some(Self::Medium),
        }
    }

    /// Fracción de señal procesada (0 = solo original, 1 = solo filtrada).
    fn wet(&self) -> f32 {
        match self {
            Self::Low => 0.35,
            Self::Medium => 0.65,
            Self::High => 1.0,
        }
    }

    fn hp_cutoff_hz(&self) -> f32 {
        match self {
            Self::Low => 60.0,
            Self::Medium => 80.0,
            Self::High => 90.0,
        }
    }

    /// Umbral RMS del gate; `None` desactiva el gate.
    /// Solo `high` usa gate: RNNoise ya atenúa ruido estacionario en medium.
    fn gate_threshold(&self) -> Option<f32> {
        match self {
            Self::Low | Self::Medium => None,
            Self::High => Some(0.008),
        }
    }

    fn gate_floor(&self) -> f32 {
        match self {
            Self::Low | Self::Medium => 1.0,
            // Floor más alto que antes (0.15) para evitar bombeo agresivo.
            Self::High => 0.35,
        }
    }
}

/// Tamaño de frame de RNNoise (10 ms a 48 kHz).
const RN_FRAME: usize = DenoiseState::FRAME_SIZE;
const RN_RATE: u32 = 48_000;

/// Resampler lineal streaming: convierte `in_rate` → `out_rate`.
struct LinearResampler {
    /// Cuántas muestras de entrada avanzar por cada muestra de salida.
    step: f64,
    /// Posición fraccional dentro de `buf`.
    pos: f64,
    buf: Vec<f32>,
}

impl LinearResampler {
    fn new(in_rate: u32, out_rate: u32) -> Self {
        Self {
            step: in_rate as f64 / out_rate as f64,
            pos: 0.0,
            buf: Vec::new(),
        }
    }

    fn push(&mut self, samples: &[f32]) -> Vec<f32> {
        if samples.is_empty() {
            return Vec::new();
        }
        // Identity path.
        if (self.step - 1.0).abs() < f64::EPSILON {
            return samples.to_vec();
        }
        self.buf.extend_from_slice(samples);
        let mut out = Vec::new();
        while self.pos + 1.0 < self.buf.len() as f64 {
            let i = self.pos.floor() as usize;
            let frac = (self.pos - i as f64) as f32;
            let a = self.buf[i];
            let b = self.buf[i + 1];
            out.push(a + (b - a) * frac);
            self.pos += self.step;
        }
        let discard = self.pos.floor() as usize;
        if discard > 0 {
            self.buf.drain(..discard.min(self.buf.len()));
            self.pos -= discard as f64;
            if self.pos < 0.0 {
                self.pos = 0.0;
            }
        }
        out
    }

    /// Drena con padding de silencio para vaciar el interpolador.
    fn flush(&mut self) -> Vec<f32> {
        if self.buf.is_empty() && self.pos < 1.0 {
            return Vec::new();
        }
        // Suficiente silencio para emitir lo pendiente.
        self.push(&[0.0, 0.0, 0.0, 0.0])
    }
}

/// Procesador stateful de la pista de micrófono.
pub struct MicNoiseProcessor {
    channels: u16,
    in_rate: u32,
    wet: f32,
    denoise: Box<DenoiseState<'static>>,
    /// Buffer de entrada a 48 kHz esperando completar un frame RNNoise.
    rn_in: Vec<f32>,
    /// Primer frame de salida de RNNoise se descarta (fade-in).
    first_rn_frame: bool,
    /// Estado del high-pass (one-pole) a rate de entrada.
    hp_x1: f32,
    hp_y1: f32,
    hp_alpha: f32,
    /// Gate envelope (ataque/release suaves por muestra).
    gate_env: f32,
    gate_threshold: Option<f32>,
    gate_floor: f32,
    /// Acumulador RMS del gate (ventana corta ~10 ms a rate de entrada).
    gate_sum_sq: f64,
    gate_count: usize,
    gate_window: usize,
    up: LinearResampler,
    down: LinearResampler,
}

impl MicNoiseProcessor {
    pub fn new(channels: u16, sample_rate: u32, level: NoiseLevel) -> Self {
        let channels = channels.max(1);
        let in_rate = sample_rate.max(1);
        let hp_hz = level.hp_cutoff_hz();
        // One-pole HP: y[n] = α (y[n-1] + x[n] - x[n-1])
        let rc = 1.0 / (2.0 * std::f32::consts::PI * hp_hz);
        let dt = 1.0 / in_rate as f32;
        let hp_alpha = rc / (rc + dt);
        // Ventana ~10 ms para estimar RMS del gate sin bombeo por bloque del dispositivo.
        let gate_window = ((in_rate as usize) / 100).max(1);

        Self {
            channels,
            in_rate,
            wet: level.wet(),
            denoise: DenoiseState::new(),
            rn_in: Vec::with_capacity(RN_FRAME),
            first_rn_frame: true,
            hp_x1: 0.0,
            hp_y1: 0.0,
            hp_alpha,
            gate_env: 1.0,
            gate_threshold: level.gate_threshold(),
            gate_floor: level.gate_floor(),
            gate_sum_sq: 0.0,
            gate_count: 0,
            gate_window,
            up: LinearResampler::new(in_rate, RN_RATE),
            down: LinearResampler::new(RN_RATE, in_rate),
        }
    }

    /// Especificación WAV de salida (siempre mono float32 al rate del dispositivo).
    pub fn out_spec(&self) -> hound::WavSpec {
        hound::WavSpec {
            channels: 1,
            sample_rate: self.in_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        }
    }

    /// Procesa un buffer interleaved del dispositivo y devuelve mono filtrado
    /// al sample rate original (mezclado wet/dry según nivel).
    pub fn process(&mut self, interleaved: &[f32]) -> Vec<f32> {
        let mono = self.downmix(interleaved);
        let filtered = self.highpass_gate(&mono);
        let at_48k = self.up.push(&filtered);
        let mut mixed_48k = Vec::new();
        for s in at_48k {
            self.rn_in.push(s);
            if self.rn_in.len() == RN_FRAME {
                if let Some(frame) = self.run_rn_frame() {
                    mixed_48k.extend_from_slice(&frame);
                }
            }
        }
        self.down.push(&mixed_48k)
    }

    /// Vacía buffers pendientes al detener la captura.
    pub fn flush(&mut self) -> Vec<f32> {
        let mut mixed_48k = Vec::new();
        // Drenar resampler de subida.
        let tail = self.up.flush();
        for s in tail {
            self.rn_in.push(s);
            if self.rn_in.len() == RN_FRAME {
                if let Some(frame) = self.run_rn_frame() {
                    mixed_48k.extend_from_slice(&frame);
                }
            }
        }
        if !self.rn_in.is_empty() {
            self.rn_in.resize(RN_FRAME, 0.0);
            if let Some(frame) = self.run_rn_frame() {
                mixed_48k.extend_from_slice(&frame);
            }
        }
        let mut out = self.down.push(&mixed_48k);
        out.extend(self.down.flush());
        out
    }

    fn downmix(&self, interleaved: &[f32]) -> Vec<f32> {
        let ch = self.channels as usize;
        if ch == 0 || interleaved.is_empty() {
            return Vec::new();
        }
        let frames = interleaved.len() / ch;
        let mut mono = Vec::with_capacity(frames);
        for i in 0..frames {
            let mut sum = 0.0f32;
            for c in 0..ch {
                sum += interleaved[i * ch + c];
            }
            mono.push(sum / ch as f32);
        }
        mono
    }

    fn highpass_gate(&mut self, mono_in: &[f32]) -> Vec<f32> {
        let a = self.hp_alpha;
        let mut mono = Vec::with_capacity(mono_in.len());
        for &x in mono_in {
            let y = a * (self.hp_y1 + x - self.hp_x1);
            self.hp_x1 = x;
            self.hp_y1 = y;
            mono.push(y);
        }

        if let Some(threshold) = self.gate_threshold {
            for s in &mut mono {
                self.gate_sum_sq += (*s as f64) * (*s as f64);
                self.gate_count += 1;
                if self.gate_count >= self.gate_window {
                    let rms = (self.gate_sum_sq / self.gate_count as f64).sqrt() as f32;
                    let target = if rms < threshold {
                        self.gate_floor
                    } else {
                        1.0
                    };
                    // Suavizado por muestra: release más lento que attack.
                    let coeff = if target < self.gate_env { 0.08 } else { 0.02 };
                    self.gate_env += coeff * (target - self.gate_env);
                    self.gate_sum_sq = 0.0;
                    self.gate_count = 0;
                }
                *s *= self.gate_env;
            }
        }
        mono
    }

    /// Procesa un frame RNNoise y mezcla wet/dry alineado muestra a muestra.
    fn run_rn_frame(&mut self) -> Option<[f32; RN_FRAME]> {
        debug_assert_eq!(self.rn_in.len(), RN_FRAME);
        let mut input = [0.0f32; RN_FRAME];
        // Copia dry en escala float [-1,1] para la mezcla (antes del ×32768).
        let mut dry = [0.0f32; RN_FRAME];
        for (i, &s) in self.rn_in.iter().enumerate() {
            dry[i] = s;
            input[i] = (s * 32768.0).clamp(-32768.0, 32767.0);
        }
        self.rn_in.clear();

        let mut output = [0.0f32; RN_FRAME];
        self.denoise.process_frame(&mut output, &input);

        if self.first_rn_frame {
            self.first_rn_frame = false;
            return None;
        }

        let wet = self.wet;
        let dry_w = 1.0 - wet;
        for i in 0..RN_FRAME {
            let w = output[i] / 32768.0;
            output[i] = dry[i] * dry_w + w * wet;
        }
        Some(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processor_runs_on_silence_48k() {
        let mut p = MicNoiseProcessor::new(1, 48_000, NoiseLevel::Medium);
        let silence = vec![0.0f32; 960];
        let out = p.process(&silence);
        let flushed = p.flush();
        // Tras descartar el primer frame RNNoise, debe haber salida útil.
        assert!(out.len() + flushed.len() >= 480);
    }

    #[test]
    fn processor_handles_stereo_44k1() {
        let mut p = MicNoiseProcessor::new(2, 44_100, NoiseLevel::Low);
        let mut buf = Vec::with_capacity(882 * 2);
        for i in 0..882 {
            let s = ((i as f32) * 0.01).sin() * 0.2;
            buf.push(s);
            buf.push(s);
        }
        let out = p.process(&buf);
        let flushed = p.flush();
        assert!(out.len() + flushed.len() > 100);
        assert_eq!(p.out_spec().channels, 1);
        assert_eq!(p.out_spec().sample_rate, 44_100);
    }

    #[test]
    fn resampler_identity() {
        let mut r = LinearResampler::new(48_000, 48_000);
        let v = vec![0.1, 0.2, 0.3];
        assert_eq!(r.push(&v), v);
    }

    #[test]
    fn parse_levels() {
        assert_eq!(NoiseLevel::parse("off"), None);
        assert_eq!(NoiseLevel::parse("low"), Some(NoiseLevel::Low));
        assert_eq!(NoiseLevel::parse("high"), Some(NoiseLevel::High));
    }

    #[test]
    fn low_keeps_more_dry_than_high() {
        assert!(NoiseLevel::Low.wet() < NoiseLevel::Medium.wet());
        assert!(NoiseLevel::Medium.wet() < NoiseLevel::High.wet());
    }

    #[test]
    fn medium_has_no_gate() {
        assert!(NoiseLevel::Medium.gate_threshold().is_none());
        assert!(NoiseLevel::High.gate_threshold().is_some());
    }
}
