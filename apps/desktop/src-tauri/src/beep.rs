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

/// Cristal: agudo y corto, como un tick de vidrio.
const GLASS_PEAK: f32 = 0.07;
const GLASS_ATTACK_SECS: f32 = 0.004;
const GLASS_RELEASE_SECS: f32 = 0.09;
const GLASS_DECAY_RATE: f32 = 9.5;
const GLASS_HARMONICS: [(f32, f32, f32); 3] = [(1.0, 0.5, 1.0), (2.7, 0.22, 2.0), (5.4, 0.08, 3.5)];
/// Cuánto sube respecto de la nota base grave.
const GLASS_PITCH: f32 = 20.0;

/// Madera: registro medio, ataque seco, cola corta.
const WOOD_PEAK: f32 = 0.11;
const WOOD_ATTACK_SECS: f32 = 0.006;
const WOOD_RELEASE_SECS: f32 = 0.11;
const WOOD_DECAY_RATE: f32 = 6.5;
/// El 4.º armónico marcado es lo que hace que suene a marimba y no a pitido.
const WOOD_HARMONICS: [(f32, f32, f32); 3] =
    [(1.0, 0.55, 1.0), (4.0, 0.18, 2.2), (10.0, 0.04, 4.0)];
const WOOD_PITCH: f32 = 7.0;

type ToneShape = (f32, f32, f32, f32, &'static [(f32, f32, f32)]);

/// Timbre elegible por acción.
///
/// El GESTO melódico lo define la acción (sube al iniciar, baja al parar) y no
/// se toca: es lo que hace que el sonido signifique algo. La voz solo cambia el
/// color y el registro. Separarlos así evita tener que inventar una melodía
/// nueva por cada combinación de acción y timbre.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToneProfile {
    /// Presencia clara pero grave. El original de grabación y captura.
    Bass,
    /// Aún más contenido. El original del dictado.
    BassSoft,
    /// Agudo y corto.
    Glass,
    /// Registro medio, madera.
    Wood,
    /// Silencio: la acción no suena.
    Silent,
}

impl ToneProfile {
    /// Desde la config. Un valor desconocido cae al default de la acción, que
    /// se pasa aparte: no todas las acciones suenan igual de fuerte.
    pub fn parse(raw: &str, fallback: ToneProfile) -> Self {
        match raw {
            "grave" => ToneProfile::Bass,
            "suave" => ToneProfile::BassSoft,
            "cristal" => ToneProfile::Glass,
            "madera" => ToneProfile::Wood,
            "ninguno" => ToneProfile::Silent,
            _ => fallback,
        }
    }

    /// Multiplicador de frecuencia sobre la nota base.
    fn pitch(self) -> f32 {
        match self {
            ToneProfile::Glass => GLASS_PITCH,
            ToneProfile::Wood => WOOD_PITCH,
            _ => 1.0,
        }
    }
}

/// Las cinco acciones que suenan. Cada una tiene su gesto y su voz por defecto.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SoundAction {
    RecordingStart,
    RecordingStop,
    DictationStart,
    DictationDone,
    Capture,
}

impl SoundAction {
    fn notes(self) -> &'static [(f32, f32)] {
        match self {
            SoundAction::RecordingStart => &START_NOTES,
            SoundAction::RecordingStop => &STOP_NOTES,
            SoundAction::DictationStart => &DICTATION_START_NOTES,
            SoundAction::DictationDone => &DICTATION_DONE_NOTES,
            SoundAction::Capture => &CAPTURE_NOTES,
        }
    }

    fn default_voice(self) -> ToneProfile {
        match self {
            SoundAction::DictationStart | SoundAction::DictationDone => ToneProfile::BassSoft,
            _ => ToneProfile::Bass,
        }
    }

    fn label(self) -> &'static str {
        match self {
            SoundAction::RecordingStart => "grabación (inicio)",
            SoundAction::RecordingStop => "grabación (fin)",
            SoundAction::DictationStart => "dictado (inicio)",
            SoundAction::DictationDone => "dictado (listo)",
            SoundAction::Capture => "captura",
        }
    }
}

/// Reproduce el sonido de una acción con la voz elegida.
///
/// `voice` viene de la config como texto; un valor desconocido cae al default
/// de la acción en vez de silenciarla, para que una config vieja o corrupta no
/// deje la app muda sin explicación.
pub fn play(action: SoundAction, voice: &str, output_device_id: &str) {
    let profile = ToneProfile::parse(voice, action.default_voice());
    if profile == ToneProfile::Silent {
        return;
    }
    play_chime(output_device_id, action.notes(), profile, action.label());
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
            ToneProfile::Glass => (
                GLASS_ATTACK_SECS,
                GLASS_RELEASE_SECS,
                GLASS_DECAY_RATE,
                GLASS_PEAK,
                &GLASS_HARMONICS,
            ),
            ToneProfile::Wood => (
                WOOD_ATTACK_SECS,
                WOOD_RELEASE_SECS,
                WOOD_DECAY_RATE,
                WOOD_PEAK,
                &WOOD_HARMONICS,
            ),
            // No se llega acá: `play_chime` corta antes de abrir el stream.
            ToneProfile::Silent => (0.0, 0.0, 1.0, 0.0, &BASS_HARMONICS),
        }
    }

    fn next_sample(&mut self) -> f32 {
        if self.lead_remaining > 0.0 {
            self.lead_remaining -= 1.0 / self.sample_rate;
            return 0.0;
        }

        let Some(&(base_freq, dur)) = self.notes.get(self.note_idx) else {
            return 0.0;
        };
        // La voz transpone; el intervalo entre notas se conserva, así que el
        // gesto (sube al iniciar, baja al parar) sigue leyéndose igual.
        let freq = base_freq * self.profile.pitch();

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

/// Reproduce una acción con una voz arbitraria, para probar desde Ajustes.
///
/// Existe porque no se puede elegir un sonido sin escucharlo: sin esto, la
/// única forma de comparar timbres sería guardar y provocar la acción real.
#[tauri::command]
pub fn preview_sound(
    state: tauri::State<'_, crate::state::AppState>,
    action: String,
    voice: String,
) -> Result<(), String> {
    let which = match action.as_str() {
        "recording_start" => SoundAction::RecordingStart,
        "recording_stop" => SoundAction::RecordingStop,
        "dictation_start" => SoundAction::DictationStart,
        "dictation_done" => SoundAction::DictationDone,
        "capture" => SoundAction::Capture,
        other => return Err(format!("acción de sonido desconocida: {other}")),
    };
    let out = state.config.lock().unwrap().output_device_id.clone();
    play(which, &voice, &out);
    Ok(())
}
