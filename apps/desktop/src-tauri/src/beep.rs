//! Sonidos de feedback: toques graves tipo «ANC» / vibración suave.
//!
//! Baja frecuencia, ataque redondo y poco brillo para que se sientan como
//! una presión satisfactoria, no como un beep de alerta.

use std::sync::atomic::{AtomicBool, Ordering};

use atic_core::MutexExt;
use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};

/// Evita apilar hilos de audio si el scroll de la rueda dispara ticks seguidos.
static WHEEL_TICK_BUSY: AtomicBool = AtomicBool::new(false);

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

/// Rueda de la pill: click seco tipo cápsula / cilindro de revólver.
const WHEEL_TICK_NOTES: [(f32, f32); 1] = [(110.0, 0.038)];

/* --- Voces -------------------------------------------------------------
 *
 * Cada voz es (ataque, release, decaimiento, pico, armonicos) mas dos
 * multiplicadores: de frecuencia y de duracion.
 *
 * Lo que separa una voz de otra, por orden de cuanto se nota:
 *   1. REGISTRO (multiplicador de frecuencia). Es lo primero que se oye.
 *   2. DECAIMIENTO: un tick seco y una campana que resuena son cosas
 *      distintas aunque compartan nota.
 *   3. ARMONICOS: el color. Enteros suenan a instrumento afinado; no enteros,
 *      a metal.
 *   4. ATAQUE: golpe seco contra entrada suave.
 *
 * Las dos primeras voces de esta app compartian registro y armonicos y solo
 * variaban el volumen: sonaban iguales. Si dos voces no se separan en al menos
 * dos de esos ejes, sobra una.
 */

/// Grave: presion baja tipo ANC. El original de grabacion.
const BASS_PEAK: f32 = 0.14;
const BASS_ATTACK_SECS: f32 = 0.045;
const BASS_RELEASE_SECS: f32 = 0.055;
const BASS_DECAY_RATE: f32 = 4.2;
const BASS_HARMONICS: [(f32, f32, f32); 2] = [(0.5, 0.55, 0.7), (2.0, 0.08, 2.8)];

/// Pulso: toque corto y seco, dos octavas mas arriba. Reemplaza al viejo
/// "suave", que era el grave con menos volumen y sonaba igual.
const TAP_PEAK: f32 = 0.11;
const TAP_ATTACK_SECS: f32 = 0.003;
const TAP_RELEASE_SECS: f32 = 0.05;
const TAP_DECAY_RATE: f32 = 14.0;
const TAP_HARMONICS: [(f32, f32, f32); 2] = [(1.0, 0.40, 1.4), (3.0, 0.10, 3.0)];
const TAP_PITCH: f32 = 2.6;
const TAP_DURATION: f32 = 0.55;

/// Cristal: agudo y corto, como un tick de vidrio.
const GLASS_PEAK: f32 = 0.07;
const GLASS_ATTACK_SECS: f32 = 0.004;
const GLASS_RELEASE_SECS: f32 = 0.09;
const GLASS_DECAY_RATE: f32 = 9.5;
const GLASS_HARMONICS: [(f32, f32, f32); 3] =
    [(1.0, 0.50, 1.0), (2.7, 0.22, 2.0), (5.4, 0.08, 3.5)];
const GLASS_PITCH: f32 = 20.0;

/// Madera: registro medio, ataque seco, cola corta.
const WOOD_PEAK: f32 = 0.11;
const WOOD_ATTACK_SECS: f32 = 0.006;
const WOOD_RELEASE_SECS: f32 = 0.11;
/// El 4.o armonico marcado es lo que hace que suene a marimba y no a pitido.
const WOOD_DECAY_RATE: f32 = 6.5;
const WOOD_HARMONICS: [(f32, f32, f32); 3] =
    [(1.0, 0.55, 1.0), (4.0, 0.18, 2.2), (10.0, 0.04, 4.0)];
const WOOD_PITCH: f32 = 7.0;

/// Campana: resuena. Parciales NO enteros --2.76 y 5.40, las razones que hacen
/// sonar a metal-- y decaimiento lento; con enteros sonaria a organo.
const BELL_PEAK: f32 = 0.075;
const BELL_ATTACK_SECS: f32 = 0.005;
const BELL_RELEASE_SECS: f32 = 0.18;
const BELL_DECAY_RATE: f32 = 2.4;
const BELL_HARMONICS: [(f32, f32, f32); 3] =
    [(2.76, 0.28, 1.3), (5.40, 0.12, 2.0), (8.93, 0.05, 3.0)];
const BELL_PITCH: f32 = 11.0;
/// Se le da el doble de tiempo: cortarla en seco anularia lo que la define.
const BELL_DURATION: f32 = 2.0;

/// Cuerda: pulsada. Ataque instantaneo y armonicos impares, como al soltar.
const PLUCK_PEAK: f32 = 0.10;
const PLUCK_ATTACK_SECS: f32 = 0.002;
const PLUCK_RELEASE_SECS: f32 = 0.12;
const PLUCK_DECAY_RATE: f32 = 5.0;
const PLUCK_HARMONICS: [(f32, f32, f32); 3] =
    [(2.0, 0.30, 1.6), (3.0, 0.16, 2.2), (5.0, 0.07, 3.2)];
const PLUCK_PITCH: f32 = 9.0;
const PLUCK_DURATION: f32 = 1.4;

/// Aire: entra sin golpe. El unico sin transitorio, para cuando el sonido no
/// deberia interrumpir lo que estas haciendo.
const AIR_PEAK: f32 = 0.085;
const AIR_ATTACK_SECS: f32 = 0.10;
const AIR_RELEASE_SECS: f32 = 0.16;
const AIR_DECAY_RATE: f32 = 1.6;
const AIR_HARMONICS: [(f32, f32, f32); 2] = [(0.5, 0.40, 0.9), (2.0, 0.06, 1.8)];
const AIR_PITCH: f32 = 4.0;
const AIR_DURATION: f32 = 1.8;

/// Digital: armonicos impares fuertes = onda cuadrada. Suena a consola vieja.
const CHIP_PEAK: f32 = 0.065;
const CHIP_ATTACK_SECS: f32 = 0.002;
const CHIP_RELEASE_SECS: f32 = 0.04;
const CHIP_DECAY_RATE: f32 = 11.0;
const CHIP_HARMONICS: [(f32, f32, f32); 3] = [(3.0, 0.33, 1.0), (5.0, 0.20, 1.0), (7.0, 0.14, 1.0)];
const CHIP_PITCH: f32 = 14.0;
const CHIP_DURATION: f32 = 0.5;

/// Click: golpe metalico casi sin cola. Parciales no enteros = casquillo, no pitido.
const CLICK_PEAK: f32 = 0.10;
const CLICK_ATTACK_SECS: f32 = 0.001;
const CLICK_RELEASE_SECS: f32 = 0.022;
const CLICK_DECAY_RATE: f32 = 28.0;
const CLICK_HARMONICS: [(f32, f32, f32); 3] =
    [(1.0, 0.50, 1.0), (2.35, 0.32, 2.8), (5.15, 0.14, 4.5)];
const CLICK_PITCH: f32 = 11.5;
const CLICK_DURATION: f32 = 0.42;

type ToneShape = (f32, f32, f32, f32, &'static [(f32, f32, f32)]);

/// Timbre elegible por accion.
///
/// El GESTO melodico lo define la accion (sube al iniciar, baja al parar) y no
/// se toca: es lo que hace que el sonido signifique algo sin mirar la pantalla.
/// La voz solo cambia color, registro y duracion. Separarlos asi evita tener
/// que inventar una melodia nueva por cada combinacion de accion y timbre.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToneProfile {
    /// Presion grave tipo ANC.
    Bass,
    /// Toque corto y seco.
    Tap,
    /// Agudo y corto, como vidrio.
    Glass,
    /// Registro medio, madera.
    Wood,
    /// Metalico, resuena.
    Bell,
    /// Cuerda pulsada.
    Pluck,
    /// Entra sin golpe.
    Air,
    /// Onda cuadrada, tipo consola vieja.
    Chip,
    /// Click seco metalico (rueda / capsula).
    Click,
    /// Silencio: la accion no suena.
    Silent,
}

impl ToneProfile {
    /// Desde la config. Un valor desconocido cae al default de la accion, que
    /// se pasa aparte: no todas las acciones suenan igual de fuerte.
    pub fn parse(raw: &str, fallback: ToneProfile) -> Self {
        match raw {
            "grave" => ToneProfile::Bass,
            "pulso" => ToneProfile::Tap,
            "cristal" => ToneProfile::Glass,
            "madera" => ToneProfile::Wood,
            "campana" => ToneProfile::Bell,
            "cuerda" => ToneProfile::Pluck,
            "aire" => ToneProfile::Air,
            "digital" => ToneProfile::Chip,
            "click" | "capsula" => ToneProfile::Click,
            "ninguno" => ToneProfile::Silent,
            _ => fallback,
        }
    }

    /// Multiplicador de frecuencia sobre la nota base.
    fn pitch(self) -> f32 {
        match self {
            ToneProfile::Tap => TAP_PITCH,
            ToneProfile::Glass => GLASS_PITCH,
            ToneProfile::Wood => WOOD_PITCH,
            ToneProfile::Bell => BELL_PITCH,
            ToneProfile::Pluck => PLUCK_PITCH,
            ToneProfile::Air => AIR_PITCH,
            ToneProfile::Chip => CHIP_PITCH,
            ToneProfile::Click => CLICK_PITCH,
            _ => 1.0,
        }
    }

    /// Cuanto se estira o acorta la nota.
    ///
    /// Las duraciones base estan pensadas para el registro grave. Una campana
    /// con esa duracion se cortaria antes de resonar, y un tick digital
    /// arrastraria cola muerta.
    fn duration(self) -> f32 {
        match self {
            ToneProfile::Tap => TAP_DURATION,
            ToneProfile::Bell => BELL_DURATION,
            ToneProfile::Pluck => PLUCK_DURATION,
            ToneProfile::Air => AIR_DURATION,
            ToneProfile::Chip => CHIP_DURATION,
            ToneProfile::Click => CLICK_DURATION,
            _ => 1.0,
        }
    }
}

/// Acciones que suenan. Cada una tiene su gesto y su voz por defecto.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SoundAction {
    RecordingStart,
    RecordingStop,
    DictationStart,
    DictationDone,
    Capture,
    /// Paso de la rueda de herramientas (pill).
    WheelTick,
}

impl SoundAction {
    fn notes(self) -> &'static [(f32, f32)] {
        match self {
            SoundAction::RecordingStart => &START_NOTES,
            SoundAction::RecordingStop => &STOP_NOTES,
            SoundAction::DictationStart => &DICTATION_START_NOTES,
            SoundAction::DictationDone => &DICTATION_DONE_NOTES,
            SoundAction::Capture => &CAPTURE_NOTES,
            SoundAction::WheelTick => &WHEEL_TICK_NOTES,
        }
    }

    fn default_voice(self) -> ToneProfile {
        match self {
            // El dictado interrumpe menos que una grabación: toque corto en vez
            // de presión grave.
            SoundAction::DictationStart | SoundAction::DictationDone => ToneProfile::Tap,
            SoundAction::WheelTick => ToneProfile::Click,
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
            SoundAction::WheelTick => "rueda (paso)",
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "recording_start" => Some(SoundAction::RecordingStart),
            "recording_stop" => Some(SoundAction::RecordingStop),
            "dictation_start" => Some(SoundAction::DictationStart),
            "dictation_done" => Some(SoundAction::DictationDone),
            "capture" => Some(SoundAction::Capture),
            "wheel_tick" => Some(SoundAction::WheelTick),
            _ => None,
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
    // Un solo tick a la vez: el scroll puede disparar varios pasos por frame.
    if action == SoundAction::WheelTick
        && WHEEL_TICK_BUSY
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
    {
        return;
    }
    play_chime(
        output_device_id,
        action.notes(),
        profile,
        action.label(),
        action == SoundAction::WheelTick,
    );
}

fn play_chime(
    output_device_id: &str,
    notes: &'static [(f32, f32)],
    profile: ToneProfile,
    label: &'static str,
    release_wheel_tick: bool,
) {
    let device_id = output_device_id.to_string();
    std::thread::spawn(move || {
        let result = play_chime_blocking(&device_id, notes, profile, release_wheel_tick);
        if release_wheel_tick {
            WHEEL_TICK_BUSY.store(false, Ordering::Release);
        }
        if let Err(err) = result {
            tracing::debug!(%err, %label, "no se pudo reproducir el sonido");
        }
    });
}

fn play_chime_blocking(
    output_device_id: &str,
    notes: &'static [(f32, f32)],
    profile: ToneProfile,
    snappy: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let device = atic_audio::resolve_output_device_by_id(output_device_id)?;
    let supported = device.default_output_config()?;
    let sample_rate = supported.sample_rate().0 as f32;
    let channels = supported.channels() as usize;
    let sample_format = supported.sample_format();
    let config: StreamConfig = supported.into();

    // El multiplicador de la voz cambia cuanto dura el sonido, asi que la
    // espera tiene que contarlo o el stream se cerraria a mitad de la campana.
    let total_secs: f32 = notes.iter().map(|(_, dur)| *dur).sum::<f32>() * profile.duration();
    // Silencio breve al inicio para que el dispositivo abra el stream en cero.
    // La rueda usa menos lead: el click tiene que llegar al paso, no despues.
    let lead_in = if snappy { 0.006 } else { 0.018 };
    let tail = if snappy { 0.02 } else { 0.08 };
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
    std::thread::sleep(std::time::Duration::from_secs_f32(lead_in + total_secs + tail));
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
            ToneProfile::Tap => (
                TAP_ATTACK_SECS,
                TAP_RELEASE_SECS,
                TAP_DECAY_RATE,
                TAP_PEAK,
                &TAP_HARMONICS,
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
            ToneProfile::Bell => (
                BELL_ATTACK_SECS,
                BELL_RELEASE_SECS,
                BELL_DECAY_RATE,
                BELL_PEAK,
                &BELL_HARMONICS,
            ),
            ToneProfile::Pluck => (
                PLUCK_ATTACK_SECS,
                PLUCK_RELEASE_SECS,
                PLUCK_DECAY_RATE,
                PLUCK_PEAK,
                &PLUCK_HARMONICS,
            ),
            ToneProfile::Air => (
                AIR_ATTACK_SECS,
                AIR_RELEASE_SECS,
                AIR_DECAY_RATE,
                AIR_PEAK,
                &AIR_HARMONICS,
            ),
            ToneProfile::Chip => (
                CHIP_ATTACK_SECS,
                CHIP_RELEASE_SECS,
                CHIP_DECAY_RATE,
                CHIP_PEAK,
                &CHIP_HARMONICS,
            ),
            ToneProfile::Click => (
                CLICK_ATTACK_SECS,
                CLICK_RELEASE_SECS,
                CLICK_DECAY_RATE,
                CLICK_PEAK,
                &CLICK_HARMONICS,
            ),
            // No se llega aca: `play` corta antes de abrir el stream.
            ToneProfile::Silent => (0.0, 0.0, 1.0, 0.0, &BASS_HARMONICS),
        }
    }

    fn next_sample(&mut self) -> f32 {
        if self.lead_remaining > 0.0 {
            self.lead_remaining -= 1.0 / self.sample_rate;
            return 0.0;
        }

        let Some(&(base_freq, base_dur)) = self.notes.get(self.note_idx) else {
            return 0.0;
        };
        let dur = base_dur * self.profile.duration();
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
    let which = SoundAction::parse(&action)
        .ok_or_else(|| format!("acción de sonido desconocida: {action}"))?;
    let out = state.config.lock_or_recover().output_device_id.clone();
    play(which, &voice, &out);
    Ok(())
}

/// Reproduce una acción respetando `ui_sounds` y el timbre guardado en config.
///
/// Lo usa la UI (rueda de la pill) sin tener que leer la config en el front.
#[tauri::command]
pub fn play_ui_sound(
    state: tauri::State<'_, crate::state::AppState>,
    action: String,
) -> Result<(), String> {
    let which = SoundAction::parse(&action)
        .ok_or_else(|| format!("acción de sonido desconocida: {action}"))?;
    let cfg = state.config.lock_or_recover();
    if !cfg.ui_sounds {
        return Ok(());
    }
    let voice = match which {
        SoundAction::RecordingStart => cfg.sound_recording_start.clone(),
        SoundAction::RecordingStop => cfg.sound_recording_stop.clone(),
        SoundAction::DictationStart => cfg.sound_dictation_start.clone(),
        SoundAction::DictationDone => cfg.sound_dictation_done.clone(),
        SoundAction::Capture => cfg.sound_capture.clone(),
        SoundAction::WheelTick => cfg.sound_wheel_tick.clone(),
    };
    let out = cfg.output_device_id.clone();
    drop(cfg);
    play(which, &voice, &out);
    Ok(())
}
