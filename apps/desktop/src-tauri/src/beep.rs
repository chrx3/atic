//! Sonidos de feedback: grabación (carillón) y dictado (toques suaves).
//!
//! Cada nota se sintetiza con envolvente suave (ataque/release) para evitar
//! clics al inicio o al final.

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};

/// Arpegio ascendente suave al iniciar grabación.
const START_NOTES: [(f32, f32); 3] = [(392.00, 0.10), (523.25, 0.10), (659.25, 0.16)];
/// Arpegio descendente que resuelve al detener: misma identidad, en espejo.
const STOP_NOTES: [(f32, f32); 3] = [(659.25, 0.09), (523.25, 0.09), (392.00, 0.18)];

/// Dictado: toque suave al abrir el mic (F3, breve y redondo).
const DICTATION_START_NOTES: [(f32, f32); 1] = [(174.61, 0.18)];
/// Dictado: confirmación elegante al pegar (A3 → C4, piano).
const DICTATION_DONE_NOTES: [(f32, f32); 2] = [(220.00, 0.11), (261.63, 0.16)];

const PEAK_AMPLITUDE: f32 = 0.16;
const DICTATION_PEAK: f32 = 0.085;
const ATTACK_SECS: f32 = 0.008;
const RELEASE_SECS: f32 = 0.012;
const DICTATION_ATTACK_SECS: f32 = 0.028;
const DICTATION_RELEASE_SECS: f32 = 0.055;
const DECAY_RATE: f32 = 9.0;
const DICTATION_DECAY_RATE: f32 = 5.2;
/// (múltiplo de la fundamental, amplitud relativa, multiplicador de decaimiento).
const HARMONICS: [(f32, f32, f32); 2] = [(2.0, 0.35, 1.6), (3.0, 0.14, 2.4)];
/// Timbre suave: poco 2º armónico, casi sin 3º.
const DICTATION_HARMONICS: [(f32, f32, f32); 2] = [(2.0, 0.12, 2.2), (3.0, 0.04, 3.0)];
type ToneShape = (f32, f32, f32, f32, &'static [(f32, f32, f32)]);

/// Reproduce el carillón de inicio de grabación en un hilo aparte.
pub fn play_start_beep(output_device_id: &str) {
    play_chime(
        output_device_id,
        &START_NOTES,
        ToneProfile::Recording,
        "grabación (inicio)",
    );
}

/// Reproduce el carillón de fin de grabación en un hilo aparte.
pub fn play_stop_beep(output_device_id: &str) {
    play_chime(
        output_device_id,
        &STOP_NOTES,
        ToneProfile::Recording,
        "grabación (fin)",
    );
}

/// Toque suave al iniciar dictado (mic abierto).
pub fn play_dictation_start(output_device_id: &str) {
    play_chime(
        output_device_id,
        &DICTATION_START_NOTES,
        ToneProfile::Dictation,
        "dictado (inicio)",
    );
}

/// Confirmación suave al pegar el texto dictado.
pub fn play_dictation_done(output_device_id: &str) {
    play_chime(
        output_device_id,
        &DICTATION_DONE_NOTES,
        ToneProfile::Dictation,
        "dictado (listo)",
    );
}

#[derive(Clone, Copy)]
enum ToneProfile {
    Recording,
    Dictation,
}

fn play_chime(
    output_device_id: &str,
    notes: &'static [(f32, f32)],
    profile: ToneProfile,
    label: &'static str,
) {
    let device_id = output_device_id.to_string();
    std::thread::spawn(move || {
        if let Err(err) = play_chime_blocking(&device_id, notes, profile) {
            tracing::debug!(%err, %label, "no se pudo reproducir el sonido");
        }
    });
}

fn play_chime_blocking(
    output_device_id: &str,
    notes: &'static [(f32, f32)],
    profile: ToneProfile,
) -> Result<(), Box<dyn std::error::Error>> {
    let device = atic_audio::resolve_output_device_by_id(output_device_id)?;
    let supported = device.default_output_config()?;
    let sample_rate = supported.sample_rate().0 as f32;
    let channels = supported.channels() as usize;
    let sample_format = supported.sample_format();
    let config: StreamConfig = supported.into();

    let total_secs: f32 = notes.iter().map(|(_, dur)| *dur).sum();
    // Silencio breve al inicio para que el dispositivo abra el stream en cero.
    let lead_in = 0.012;
    let mut gen = ChimeGenerator::new(sample_rate, notes, profile, lead_in);

    let stream = match sample_format {
        SampleFormat::F32 => device.build_output_stream(
            &config,
            move |data: &mut [f32], _| gen.fill(data, channels, |s| s),
            |err| tracing::debug!(%err, "error de stream de audio"),
            None,
        )?,
        SampleFormat::I16 => device.build_output_stream(
            &config,
            move |data: &mut [i16], _| gen.fill(data, channels, |s| (s * i16::MAX as f32) as i16),
            |err| tracing::debug!(%err, "error de stream de audio"),
            None,
        )?,
        other => return Err(format!("formato de salida no soportado: {other:?}").into()),
    };

    stream.play()?;
    std::thread::sleep(std::time::Duration::from_secs_f32(
        lead_in + total_secs + 0.06,
    ));
    Ok(())
}

/// Genera, muestra a muestra, la envolvente de las notas.
struct ChimeGenerator {
    sample_rate: f32,
    notes: &'static [(f32, f32)],
    note_idx: usize,
    t_in_note: f32,
    lead_remaining: f32,
    profile: ToneProfile,
}

impl ChimeGenerator {
    fn new(
        sample_rate: f32,
        notes: &'static [(f32, f32)],
        profile: ToneProfile,
        lead_in: f32,
    ) -> Self {
        Self {
            sample_rate,
            notes,
            note_idx: 0,
            t_in_note: 0.0,
            lead_remaining: lead_in,
            profile,
        }
    }

    fn next_sample(&mut self) -> f32 {
        if self.lead_remaining > 0.0 {
            self.lead_remaining -= 1.0 / self.sample_rate;
            return 0.0;
        }

        let Some(&(freq, dur)) = self.notes.get(self.note_idx) else {
            return 0.0;
        };

        let (attack_secs, release_secs, decay, peak, harmonics): ToneShape = match self.profile {
            ToneProfile::Recording => (
                ATTACK_SECS,
                RELEASE_SECS,
                DECAY_RATE,
                PEAK_AMPLITUDE,
                &HARMONICS,
            ),
            ToneProfile::Dictation => (
                DICTATION_ATTACK_SECS,
                DICTATION_RELEASE_SECS,
                DICTATION_DECAY_RATE,
                DICTATION_PEAK,
                &DICTATION_HARMONICS,
            ),
        };

        // Ataque/release en coseno (0→1→0) para evitar discontinuidades.
        let attack = if attack_secs <= 0.0 {
            1.0
        } else {
            let x = (self.t_in_note / attack_secs).clamp(0.0, 1.0);
            0.5 - 0.5 * (std::f32::consts::PI * x).cos()
        };
        let release = if release_secs <= 0.0 || self.t_in_note + release_secs < dur {
            1.0
        } else {
            let x = ((dur - self.t_in_note) / release_secs).clamp(0.0, 1.0);
            0.5 - 0.5 * (std::f32::consts::PI * x).cos()
        };
        let env = attack * release;

        let two_pi = 2.0 * std::f32::consts::PI;
        let mut s = (two_pi * freq * self.t_in_note).sin() * (-self.t_in_note * decay).exp();
        for &(mult, amp, decay_mult) in harmonics {
            s += (two_pi * freq * mult * self.t_in_note).sin()
                * (-self.t_in_note * decay * decay_mult).exp()
                * amp;
        }
        s *= env * peak;

        self.t_in_note += 1.0 / self.sample_rate;
        if self.t_in_note >= dur {
            self.t_in_note = 0.0;
            self.note_idx += 1;
        }
        s
    }

    fn fill<T: Copy>(&mut self, data: &mut [T], channels: usize, conv: impl Fn(f32) -> T) {
        for frame in data.chunks_mut(channels) {
            let s = conv(self.next_sample());
            for out in frame.iter_mut() {
                *out = s;
            }
        }
    }
}
