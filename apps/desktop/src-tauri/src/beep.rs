//! Sonidos de feedback: toques graves tipo «ANC» / vibración suave.
//!
//! Baja frecuencia, ataque redondo y poco brillo para que se sientan como
//! una presión satisfactoria, no como un beep de alerta.

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};

/// Grabación on: presión grave que sube un semitono (como activar ANC).
const START_NOTES: [(f32, f32); 2] = [(58.0, 0.13), (78.0, 0.11)];
/// Grabación off: espejo descendente, un poco más largo al final.
const STOP_NOTES: [(f32, f32); 2] = [(78.0, 0.09), (52.0, 0.15)];

/// Dictado: un solo golpe suave al abrir mic.
const DICTATION_START_NOTES: [(f32, f32); 1] = [(64.0, 0.12)];
/// Dictado listo: confirmación grave en dos pasos.
const DICTATION_DONE_NOTES: [(f32, f32); 2] = [(56.0, 0.09), (72.0, 0.12)];

/// Captura: click de obturador grave (una sola pulsación).
const CAPTURE_NOTES: [(f32, f32); 1] = [(70.0, 0.11)];

const BASS_PEAK: f32 = 0.14;
const BASS_SOFT_PEAK: f32 = 0.09;
const BASS_ATTACK_SECS: f32 = 0.045;
const BASS_RELEASE_SECS: f32 = 0.055;
const BASS_SOFT_ATTACK_SECS: f32 = 0.055;
const BASS_SOFT_RELEASE_SECS: f32 = 0.07;
const BASS_DECAY_RATE: f32 = 4.2;
const BASS_SOFT_DECAY_RATE: f32 = 3.4;
/// Poco brillo: sub + un armónico muy atenuado.
const BASS_HARMONICS: [(f32, f32, f32); 2] = [(0.5, 0.55, 0.7), (2.0, 0.08, 2.8)];
const BASS_SOFT_HARMONICS: [(f32, f32, f32); 2] = [(0.5, 0.45, 0.75), (2.0, 0.05, 3.2)];

type ToneShape = (f32, f32, f32, f32, &'static [(f32, f32, f32)]);

#[derive(Clone, Copy)]
enum ToneProfile {
    /// Grabación / captura: presencia clara pero grave.
    Bass,
    /// Dictado: aún más contenido.
    BassSoft,
}

/// Carillón grave al iniciar grabación.
pub fn play_start_beep(output_device_id: &str) {
    play_chime(
        output_device_id,
        &START_NOTES,
        ToneProfile::Bass,
        "grabación (inicio)",
    );
}

/// Carillón grave al detener grabación.
pub fn play_stop_beep(output_device_id: &str) {
    play_chime(
        output_device_id,
        &STOP_NOTES,
        ToneProfile::Bass,
        "grabación (fin)",
    );
}

/// Toque al iniciar dictado.
pub fn play_dictation_start(output_device_id: &str) {
    play_chime(
        output_device_id,
        &DICTATION_START_NOTES,
        ToneProfile::BassSoft,
        "dictado (inicio)",
    );
}

/// Confirmación al pegar el texto dictado.
pub fn play_dictation_done(output_device_id: &str) {
    play_chime(
        output_device_id,
        &DICTATION_DONE_NOTES,
        ToneProfile::BassSoft,
        "dictado (listo)",
    );
}

/// Pulsación grave al completar una captura.
pub fn play_capture_thump(output_device_id: &str) {
    play_chime(
        output_device_id,
        &CAPTURE_NOTES,
        ToneProfile::Bass,
        "captura",
    );
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
    let lead_in = 0.018;
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
        lead_in + total_secs + 0.08,
    ));
    Ok(())
}

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

    fn shape(&self) -> ToneShape {
        match self.profile {
            ToneProfile::Bass => (
                BASS_ATTACK_SECS,
                BASS_RELEASE_SECS,
                BASS_DECAY_RATE,
                BASS_PEAK,
                &BASS_HARMONICS,
            ),
            ToneProfile::BassSoft => (
                BASS_SOFT_ATTACK_SECS,
                BASS_SOFT_RELEASE_SECS,
                BASS_SOFT_DECAY_RATE,
                BASS_SOFT_PEAK,
                &BASS_SOFT_HARMONICS,
            ),
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

        let (attack_secs, release_secs, decay, peak, harmonics) = self.shape();

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
        // Sine puro + subarmónico: sensación de “vibración” más que de beep.
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
