//! Captura de audio para Atic.
//!
//! Graba micrófono y/o audio del sistema (loopback WASAPI en Windows) a WAV
//! separados, y publica niveles RMS periódicos para el medidor de la UI.
//!
//! El `cpal::Stream` no es `Send` en Windows, por lo que toda la captura vive
//! en un hilo dedicado ("audio-capture") controlado por canales. `start`
//! espera a que los streams estén activos antes de devolver el handle, de modo
//! que los errores de inicio se propagan de inmediato.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use thiserror::Error;

mod noise;
#[cfg(windows)]
mod wasapi;
use noise::MicNoiseProcessor;

pub use noise::NoiseLevel;

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("no se encontró dispositivo de micrófono")]
    NoInputDevice,
    #[error("no se encontró el micrófono «{0}»")]
    InputDeviceNotFound(String),
    #[error("no se encontró dispositivo de salida (audio del sistema)")]
    NoOutputDevice,
    #[error("no se encontró la salida «{0}»")]
    OutputDeviceNotFound(String),
    #[error("formato de muestra no soportado: {0}")]
    UnsupportedFormat(String),
    #[error("error de configuración de audio: {0}")]
    Config(String),
    #[error("no se pudo construir el stream: {0}")]
    BuildStream(#[from] cpal::BuildStreamError),
    #[error("no se pudo iniciar el stream: {0}")]
    PlayStream(#[from] cpal::PlayStreamError),
    #[error("error de E/S: {0}")]
    Io(#[from] std::io::Error),
    #[error("error escribiendo WAV: {0}")]
    Wav(#[from] hound::Error),
    #[error("el hilo de captura terminó inesperadamente")]
    CaptureThreadGone,
}

impl AudioError {
    pub fn to_ui(&self, en: bool) -> String {
        match self {
            Self::NoInputDevice => {
                if en {
                    "No microphone device was found".into()
                } else {
                    self.to_string()
                }
            }
            Self::InputDeviceNotFound(name) => {
                if en {
                    format!("Microphone “{name}” was not found")
                } else {
                    self.to_string()
                }
            }
            Self::NoOutputDevice => {
                if en {
                    "No output device was found (PC audio)".into()
                } else {
                    self.to_string()
                }
            }
            Self::OutputDeviceNotFound(name) => {
                if en {
                    format!("Output “{name}” was not found")
                } else {
                    self.to_string()
                }
            }
            Self::UnsupportedFormat(fmt) => {
                if en {
                    format!("Unsupported sample format: {fmt}")
                } else {
                    self.to_string()
                }
            }
            Self::Config(msg) => {
                if en && msg == "debes habilitar al menos una pista (mic o sistema)" {
                    "Enable at least one track (mic or system)".into()
                } else if en && msg == "no se pudo abrir ninguna pista de audio" {
                    "Could not open any audio track".into()
                } else if en {
                    format!("Audio configuration error: {msg}")
                } else {
                    self.to_string()
                }
            }
            Self::BuildStream(err) => {
                if en {
                    format!("Could not build the audio stream: {err}")
                } else {
                    self.to_string()
                }
            }
            Self::PlayStream(err) => {
                if en {
                    format!("Could not start the audio stream: {err}")
                } else {
                    self.to_string()
                }
            }
            Self::Io(err) => format!("{err}"),
            Self::Wav(err) => {
                if en {
                    format!("Error writing WAV: {err}")
                } else {
                    self.to_string()
                }
            }
            Self::CaptureThreadGone => {
                if en {
                    "The capture thread ended unexpectedly".into()
                } else {
                    self.to_string()
                }
            }
        }
    }
}

/// Dispositivo de entrada (micrófono) visible para la UI.
#[derive(Debug, Clone, serde::Serialize)]
pub struct InputDeviceInfo {
    /// ID persistente del endpoint WASAPI en Windows; nombre como fallback.
    pub id: String,
    pub name: String,
    pub is_default: bool,
    /// `true` si `default_input_config` y `supported_input_configs` fallaron
    /// o vinieron vacíos. En Windows el mic Hands-Free a veces aparece así
    /// hasta abrirlo; se lista igual para no ocultar mics Bluetooth.
    #[serde(default)]
    pub may_not_open: bool,
    /// Heurística basada en el nombre del endpoint (cpal no expone el bus).
    #[serde(default)]
    pub is_bluetooth: bool,
    /// Endpoint de comunicaciones HFP/HSP, normalmente mono y de baja frecuencia.
    #[serde(default)]
    pub is_hands_free: bool,
    /// Formato preferido informado por el dispositivo, si se pudo consultar.
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
}

/// Destinos de escritura para una sesión de captura.
#[derive(Debug, Clone)]
pub struct CaptureConfig {
    pub mic_wav: PathBuf,
    pub system_wav: PathBuf,
    /// Grabar micrófono.
    pub capture_mic: bool,
    /// Grabar audio del sistema (loopback).
    pub capture_system: bool,
    /// Nivel de supresión de ruido en mic: `off` | `low` | `medium` | `high`.
    /// No se aplica al audio del sistema. Por defecto `off`.
    pub noise_suppression: String,
    /// Nombre del micrófono a usar. Vacío = dispositivo por defecto.
    pub mic_device_id: String,
    /// Nombre de la salida (altavoces) para loopback. Vacío = por defecto del SO.
    pub output_device_id: String,
    /// Tap opcional de PCM para STT en vivo. `try_send`: si el consumidor se
    /// atrasa, se dropean chunks (nunca bloquea la captura ni el WAV).
    pub stt_tap: Option<SyncSender<AudioTapChunk>>,
    /// Copy de avisos hacia la UI.
    pub english: bool,
}

/// Pista de origen de un bloque de audio del tap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureTrack {
    Mic,
    System,
}

/// Bloque PCM interleaved (f32) con marca temporal relativa al inicio de captura.
#[derive(Debug, Clone)]
pub struct AudioTapChunk {
    pub track: CaptureTrack,
    /// Inicio del bloque en ms desde el arranque de esta pista.
    pub start_ms: i64,
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

/// Host de audio fresco. En Windows se fuerza WASAPI (ya es el default de
/// cpal, pero deja explícito el backend y re-consulta endpoints).
fn audio_host() -> cpal::Host {
    #[cfg(windows)]
    {
        match cpal::host_from_id(cpal::HostId::Wasapi) {
            Ok(host) => return host,
            Err(err) => {
                tracing::warn!(%err, "WASAPI no disponible; usando default_host");
            }
        }
    }
    cpal::default_host()
}

/// Lista micrófonos disponibles (entrada).
///
/// Incluye **todas** las entradas con nombre. Si la consulta de configs falla
/// (típico Hands-Free BT en Windows), el dispositivo se lista con
/// `may_not_open = true` en lugar de ocultarse.
pub fn list_input_devices() -> Result<Vec<InputDeviceInfo>, AudioError> {
    let host = audio_host();
    let default_name = host
        .default_input_device()
        .and_then(|d| d.name().ok())
        .unwrap_or_default();

    let devices = host
        .input_devices()
        .map_err(|err| AudioError::Config(err.to_string()))?;

    #[cfg(windows)]
    let mut native_endpoints = wasapi::active_endpoints(true).unwrap_or_default();
    let mut out = Vec::new();
    for device in devices {
        let Ok(name) = device.name() else {
            tracing::debug!("omitido dispositivo de entrada sin nombre");
            continue;
        };
        let preferred = device.default_input_config().ok();
        let may_not_open = preferred.is_none() && input_may_not_open(&device, &name);
        let is_default = !default_name.is_empty() && name == default_name;
        #[cfg(windows)]
        let id =
            wasapi::take_matching_id(&mut native_endpoints, &name).unwrap_or_else(|| name.clone());
        #[cfg(not(windows))]
        let id = name.clone();
        out.push(InputDeviceInfo {
            id,
            is_bluetooth: looks_like_bluetooth(&name),
            is_hands_free: looks_like_hands_free(&name),
            sample_rate: preferred.as_ref().map(|cfg| cfg.sample_rate().0),
            channels: preferred.as_ref().map(|cfg| cfg.channels()),
            name,
            is_default,
            may_not_open,
        });
    }

    sort_device_infos(&mut out);
    Ok(out)
}

/// Lista salidas de audio disponibles (altavoces / auriculares).
pub fn list_output_devices() -> Result<Vec<InputDeviceInfo>, AudioError> {
    let host = audio_host();
    let default_name = host
        .default_output_device()
        .and_then(|d| d.name().ok())
        .unwrap_or_default();

    let devices = host
        .output_devices()
        .map_err(|err| AudioError::Config(err.to_string()))?;

    #[cfg(windows)]
    let mut native_endpoints = wasapi::active_endpoints(false).unwrap_or_default();
    let mut out = Vec::new();
    for device in devices {
        let Ok(name) = device.name() else {
            tracing::debug!("omitido dispositivo de salida sin nombre");
            continue;
        };
        let Ok(preferred) = device.default_output_config() else {
            tracing::debug!(%name, "omitida salida sin default_output_config");
            continue;
        };
        let is_default = !default_name.is_empty() && name == default_name;
        #[cfg(windows)]
        let id =
            wasapi::take_matching_id(&mut native_endpoints, &name).unwrap_or_else(|| name.clone());
        #[cfg(not(windows))]
        let id = name.clone();
        out.push(InputDeviceInfo {
            id,
            is_bluetooth: looks_like_bluetooth(&name),
            is_hands_free: looks_like_hands_free(&name),
            sample_rate: Some(preferred.sample_rate().0),
            channels: Some(preferred.channels()),
            name,
            is_default,
            may_not_open: false,
        });
    }

    sort_device_infos(&mut out);
    Ok(out)
}

/// Diagnóstico: enumera **todas** las entradas/salidas que ve cpal (con
/// resultado de probe), para la consola / logs del backend.
pub fn debug_list_audio_devices() -> Result<String, AudioError> {
    use std::fmt::Write;

    let host = audio_host();
    let mut report = String::new();
    let _ = writeln!(report, "host={}", host.id().name());

    let _ = writeln!(report, "--- input ---");
    match host.input_devices() {
        Ok(devices) => {
            let mut n = 0usize;
            for device in devices {
                n += 1;
                let name = device.name().unwrap_or_else(|_| "<sin nombre>".to_string());
                let default_ok = device.default_input_config().is_ok();
                let supported = match device.supported_input_configs() {
                    Ok(configs) => configs.count(),
                    Err(_) => 0,
                };
                let may_not_open = !default_ok && supported == 0;
                let _ = writeln!(
                    report,
                    "  [{n}] {name} | default_ok={default_ok} supported={supported} \
                     may_not_open={may_not_open} headset={}",
                    looks_like_headset(&name)
                );
            }
            if n == 0 {
                let _ = writeln!(report, "  (ninguno)");
            }
        }
        Err(err) => {
            let _ = writeln!(report, "  error: {err}");
        }
    }

    let _ = writeln!(report, "--- output ---");
    match host.output_devices() {
        Ok(devices) => {
            let mut n = 0usize;
            for device in devices {
                n += 1;
                let name = device.name().unwrap_or_else(|_| "<sin nombre>".to_string());
                let default_ok = device.default_output_config().is_ok();
                let _ = writeln!(
                    report,
                    "  [{n}] {name} | default_ok={default_ok} headset={}",
                    looks_like_headset(&name)
                );
            }
            if n == 0 {
                let _ = writeln!(report, "  (ninguno)");
            }
        }
        Err(err) => {
            let _ = writeln!(report, "  error: {err}");
        }
    }

    tracing::info!(%report, "debug_list_audio_devices");
    Ok(report)
}

/// Aviso cuando la grabación de mic puede forzar perfil Hands-Free en Bluetooth.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BluetoothRecordingAdvisory {
    pub message: String,
    pub suggestion: Option<String>,
}

/// Devuelve un aviso si abrir el mic configurado puede degradar el audio Bluetooth.
///
/// Windows cambia de A2DP (alta calidad, solo salida) a HFP Hands-Free (mono ~16 kHz,
/// mic + audio) al activar el micrófono del auricular. No aplica si solo se graba
/// audio del sistema (`capture_mic = false`).
pub fn bluetooth_recording_advisory(
    capture_mic: bool,
    capture_system: bool,
    mic_device_id: &str,
    output_device_id: &str,
    english: bool,
) -> Option<BluetoothRecordingAdvisory> {
    let inputs = list_input_devices().ok()?;
    let outputs = list_output_devices().ok()?;
    bluetooth_recording_advisory_for_devices(
        capture_mic,
        capture_system,
        mic_device_id,
        output_device_id,
        &inputs,
        &outputs,
        english,
    )
}

pub fn bluetooth_recording_advisory_for_devices(
    capture_mic: bool,
    capture_system: bool,
    mic_device_id: &str,
    output_device_id: &str,
    inputs: &[InputDeviceInfo],
    outputs: &[InputDeviceInfo],
    en: bool,
) -> Option<BluetoothRecordingAdvisory> {
    if !capture_mic {
        return None;
    }

    let mic_name = resolve_listed_device_name(mic_device_id, inputs)?;
    if !looks_like_bluetooth(&mic_name) {
        return None;
    }

    let out_bt = resolve_listed_device_name(output_device_id, outputs)
        .is_some_and(|name| looks_like_bluetooth(&name));

    let suggestion = if out_bt && capture_system {
        Some(if en {
            "Try the built-in mic (e.g. Realtek) + Bluetooth output, or turn on Speaker mode to record only “others”."
                .into()
        } else {
            "Prueba micrófono interno (p. ej. Realtek) + salida Bluetooth, o activa \
             Modo parlantes para grabar solo «otros»."
                .into()
        })
    } else {
        Some(if en {
            "When you stop recording, audio quality should return to normal.".into()
        } else {
            "Al detener la grabación, el audio debería volver a la calidad normal.".into()
        })
    };

    let message = if en {
        "On Bluetooth, Windows often drops audio quality while you use the headset microphone."
            .to_string()
    } else {
        "En Bluetooth, Windows suele bajar la calidad del audio mientras \
         usas el micrófono del auricular."
            .to_string()
    };

    Some(BluetoothRecordingAdvisory {
        message,
        suggestion,
    })
}

fn resolve_listed_device_name(wanted_id: &str, devices: &[InputDeviceInfo]) -> Option<String> {
    let wanted = wanted_id.trim();
    if wanted.is_empty() {
        devices
            .iter()
            .find(|d| d.is_default)
            .or_else(|| devices.first())
            .map(|d| d.name.clone())
    } else {
        devices
            .iter()
            .find(|d| d.id == wanted)
            .map(|d| d.name.clone())
    }
}

/// Heurística: nombres típicos de auriculares / headset en Windows.
pub fn looks_like_headset(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    // Stereo Mix / mezclador no es un mic de auriculares.
    if n.contains("stereo mix") || n.contains("mezcla estéreo") {
        return false;
    }
    const KEYS: &[&str] = &[
        "headset",
        "headphone",
        "auricular",
        "hands-free",
        "handsfree",
        "hands free",
        "hfp",
        "ag audio",
        "communications",
        "comunicación",
        "comunicacion",
        "earphone",
        "earbuds",
        "airpods",
        "wh-",
        "bluetooth",
        "bt ",
        "usb audio",
        "usb headset",
    ];
    KEYS.iter().any(|k| n.contains(k))
}

/// Heurística conservadora para distinguir Bluetooth de headsets USB/cableados.
pub fn looks_like_bluetooth(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    const KEYS: &[&str] = &[
        "bluetooth",
        "hands-free",
        "handsfree",
        "hands free",
        "hfp",
        "ag audio",
        "airpods",
        "galaxy buds",
        "pixel buds",
        "wh-",
        "wf-",
        "quietcomfort",
        "freebuds",
    ];
    KEYS.iter().any(|key| n.contains(key))
}

pub fn looks_like_hands_free(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    [
        "hands-free",
        "handsfree",
        "hands free",
        "hfp",
        "ag audio",
        "communications",
        "comunicación",
        "comunicacion",
    ]
    .iter()
    .any(|key| n.contains(key))
}

/// Diagnóstico previo, seguro de consultar antes de abrir cualquier stream.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AudioPreflight {
    pub risk: String,
    pub message: Option<String>,
    pub current_mic: Option<InputDeviceInfo>,
    pub current_output: Option<InputDeviceInfo>,
    pub recommended_mic_id: Option<String>,
    pub recommended_output_id: Option<String>,
}

pub fn audio_preflight(
    capture_mic: bool,
    capture_system: bool,
    mic_device_id: &str,
    output_device_id: &str,
    english: bool,
) -> Result<AudioPreflight, AudioError> {
    let inputs = list_input_devices()?;
    let outputs = list_output_devices()?;
    let current_mic = resolve_listed_device(mic_device_id, &inputs).cloned();
    let current_output = resolve_listed_device(output_device_id, &outputs).cloned();
    let advisory = bluetooth_recording_advisory_for_devices(
        capture_mic,
        capture_system,
        mic_device_id,
        output_device_id,
        &inputs,
        &outputs,
        english,
    );
    let recommended_mic_id = advisory.as_ref().and_then(|_| {
        inputs
            .iter()
            .filter(|device| !device.is_bluetooth && !device.may_not_open)
            .max_by_key(|device| device.is_default)
            .map(|device| device.id.clone())
    });
    let message = advisory.map(|advisory| match advisory.suggestion {
        Some(suggestion) => format!("{} {}", advisory.message, suggestion),
        None => advisory.message,
    });

    Ok(AudioPreflight {
        risk: if message.is_some() {
            "bluetooth_hands_free".into()
        } else {
            "none".into()
        },
        message,
        current_mic,
        recommended_mic_id,
        recommended_output_id: current_output.as_ref().map(|device| device.id.clone()),
        current_output,
    })
}

fn resolve_listed_device<'a>(
    wanted_id: &str,
    devices: &'a [InputDeviceInfo],
) -> Option<&'a InputDeviceInfo> {
    let wanted = wanted_id.trim();
    if wanted.is_empty() {
        devices
            .iter()
            .find(|device| device.is_default)
            .or_else(|| devices.first())
    } else {
        devices.iter().find(|device| device.id == wanted)
    }
}

/// `true` si no hay config consultable (aún así se lista el dispositivo).
fn input_may_not_open(device: &cpal::Device, name: &str) -> bool {
    match device.default_input_config() {
        Ok(_) => return false,
        Err(err) => {
            tracing::debug!(
                %name,
                %err,
                "default_input_config falló; probando supported_input_configs"
            );
        }
    }

    match device.supported_input_configs() {
        Ok(mut configs) => {
            if let Some(cfg) = configs.next() {
                tracing::info!(
                    %name,
                    min_rate = cfg.min_sample_rate().0,
                    max_rate = cfg.max_sample_rate().0,
                    channels = cfg.channels(),
                    headset = looks_like_headset(name),
                    "incluyendo entrada vía supported_input_configs (default falló)"
                );
                return false;
            }
            tracing::warn!(
                %name,
                headset = looks_like_headset(name),
                "incluyendo entrada sin configs consultables \
                 (¿perfil A2DP? prueba Hands-Free en Sonido de Windows)"
            );
            true
        }
        Err(err) => {
            tracing::warn!(
                %name,
                %err,
                headset = looks_like_headset(name),
                "incluyendo entrada aunque la consulta de configs falló"
            );
            true
        }
    }
}

fn sort_device_infos(out: &mut [InputDeviceInfo]) {
    out.sort_by(|a, b| {
        b.is_default
            .cmp(&a.is_default)
            .then(looks_like_headset(&b.name).cmp(&looks_like_headset(&a.name)))
            // Preferir dispositivos que sí abren sobre los dudosos.
            .then(a.may_not_open.cmp(&b.may_not_open))
            .then(a.name.cmp(&b.name))
    });
}

/// Config de captura: preferir default; si falla (típico HFP BT), primera
/// `supported_input_configs` con su sample rate máximo.
fn resolve_input_config(device: &cpal::Device) -> Result<cpal::SupportedStreamConfig, AudioError> {
    match device.default_input_config() {
        Ok(cfg) => Ok(cfg),
        Err(default_err) => {
            let name = device
                .name()
                .unwrap_or_else(|_| "<desconocido>".to_string());
            tracing::warn!(
                %name,
                %default_err,
                "default_input_config falló; usando primera supported_input_config"
            );
            let range = device
                .supported_input_configs()
                .map_err(|err| AudioError::Config(err.to_string()))?
                .next()
                .ok_or_else(|| {
                    AudioError::Config(format!(
                        "dispositivo «{name}» sin configs de entrada soportadas"
                    ))
                })?;
            Ok(range.with_max_sample_rate())
        }
    }
}

fn resolve_input_device(
    host: &cpal::Host,
    mic_device_id: &str,
) -> Result<cpal::Device, AudioError> {
    let wanted = mic_device_id.trim();
    if wanted.is_empty() {
        return host.default_input_device().ok_or(AudioError::NoInputDevice);
    }

    #[cfg(windows)]
    let wanted_name = wasapi::friendly_name(wanted, true).unwrap_or_else(|| wanted.to_string());
    #[cfg(not(windows))]
    let wanted_name = wanted.to_string();
    let devices = host
        .input_devices()
        .map_err(|err| AudioError::Config(err.to_string()))?;
    for device in devices {
        if device.name().ok().as_deref() == Some(wanted_name.as_str()) {
            return Ok(device);
        }
    }
    Err(AudioError::InputDeviceNotFound(wanted.to_string()))
}

fn resolve_output_device(
    host: &cpal::Host,
    output_device_id: &str,
) -> Result<cpal::Device, AudioError> {
    let wanted = output_device_id.trim();
    if wanted.is_empty() {
        return host
            .default_output_device()
            .ok_or(AudioError::NoOutputDevice);
    }

    #[cfg(windows)]
    let wanted_name = wasapi::friendly_name(wanted, false).unwrap_or_else(|| wanted.to_string());
    #[cfg(not(windows))]
    let wanted_name = wanted.to_string();
    let devices = host
        .output_devices()
        .map_err(|err| AudioError::Config(err.to_string()))?;
    for device in devices {
        if device.name().ok().as_deref() == Some(wanted_name.as_str()) {
            return Ok(device);
        }
    }
    Err(AudioError::OutputDeviceNotFound(wanted.to_string()))
}

/// Resuelve un dispositivo de salida por nombre (vacío = default del SO).
pub fn resolve_output_device_by_id(output_device_id: &str) -> Result<cpal::Device, AudioError> {
    resolve_output_device(&audio_host(), output_device_id)
}

/// Eventos publicados durante la captura (para reflejar en la UI).
#[derive(Debug, Clone)]
pub enum CaptureEvent {
    /// Niveles RMS actuales por pista, en rango \[0.0, 1.0\].
    Levels { mic: f32, system: f32 },
    /// Error no fatal (p. ej. audio del sistema no disponible).
    Error(String),
}

/// Resultado de una captura finalizada.
#[derive(Debug, Clone, Default)]
pub struct CaptureSummary {
    pub duration_secs: f64,
    pub mic_written: bool,
    pub system_written: bool,
    pub mic_peak_rms: f32,
    pub system_peak_rms: f32,
}

/// Métricas simples para validar una pista antes de una reunión.
#[derive(Debug, Clone, serde::Serialize)]
pub struct WavAnalysis {
    pub duration_secs: f64,
    pub sample_rate: u32,
    pub channels: u16,
    pub rms: f32,
    pub peak: f32,
    pub silent: bool,
    pub clipped: bool,
}

pub fn analyze_wav(path: &Path) -> Result<WavAnalysis, AudioError> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let mut count = 0u64;
    let mut sum_sq = 0.0f64;
    let mut peak = 0.0f32;

    match spec.sample_format {
        hound::SampleFormat::Float => {
            for sample in reader.samples::<f32>() {
                let value = sample?.clamp(-1.0, 1.0);
                sum_sq += f64::from(value) * f64::from(value);
                peak = peak.max(value.abs());
                count += 1;
            }
        }
        hound::SampleFormat::Int => {
            let divisor = 2f32.powi(i32::from(spec.bits_per_sample.saturating_sub(1)));
            for sample in reader.samples::<i32>() {
                let value = (sample? as f32 / divisor).clamp(-1.0, 1.0);
                sum_sq += f64::from(value) * f64::from(value);
                peak = peak.max(value.abs());
                count += 1;
            }
        }
    }

    let rms = if count == 0 {
        0.0
    } else {
        (sum_sq / count as f64).sqrt() as f32
    };
    let frames = count as f64 / f64::from(spec.channels.max(1));
    let duration_secs = if spec.sample_rate == 0 {
        0.0
    } else {
        frames / f64::from(spec.sample_rate)
    };

    Ok(WavAnalysis {
        duration_secs,
        sample_rate: spec.sample_rate,
        channels: spec.channels,
        rms,
        peak,
        silent: duration_secs < 0.25 || rms < 0.0015 || peak < 0.008,
        clipped: peak >= 0.995,
    })
}

/// Handle para detener una captura en curso.
pub struct CaptureHandle {
    stop_tx: Sender<()>,
    ctrl: Option<JoinHandle<CaptureSummary>>,
}

impl CaptureHandle {
    /// Detiene la captura, finaliza los WAV y devuelve el resumen.
    pub fn stop(mut self) -> CaptureSummary {
        let _ = self.stop_tx.send(());
        self.ctrl
            .take()
            .and_then(|h| h.join().ok())
            .unwrap_or_default()
    }
}

/// Punto de entrada para iniciar una sesión de captura.
pub struct CaptureSession;

impl CaptureSession {
    /// Inicia la captura. Según `CaptureConfig` puede ser solo mic, solo
    /// sistema, o ambas. Al menos una pista debe estar habilitada.
    pub fn start(
        config: CaptureConfig,
        events: Sender<CaptureEvent>,
    ) -> Result<CaptureHandle, AudioError> {
        if !config.capture_mic && !config.capture_system {
            return Err(AudioError::Config(
                "debes habilitar al menos una pista (mic o sistema)".into(),
            ));
        }

        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), AudioError>>();
        let (stop_tx, stop_rx) = mpsc::channel::<()>();

        let ctrl = thread::Builder::new()
            .name("audio-capture".into())
            .spawn(move || control_loop(config, events, ready_tx, stop_rx))
            .map_err(AudioError::Io)?;

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(CaptureHandle {
                stop_tx,
                ctrl: Some(ctrl),
            }),
            Ok(Err(err)) => {
                let _ = ctrl.join();
                Err(err)
            }
            Err(_) => Err(AudioError::CaptureThreadGone),
        }
    }
}

/// Cuerpo del hilo de captura: construye streams, mide niveles y espera stop.
fn control_loop(
    config: CaptureConfig,
    events: Sender<CaptureEvent>,
    ready_tx: Sender<Result<(), AudioError>>,
    stop_rx: Receiver<()>,
) -> CaptureSummary {
    let host = audio_host();
    let want_mic = config.capture_mic;
    let want_system = config.capture_system;

    let mic_meter = LevelMeter::new();
    let sys_meter = LevelMeter::new();

    let mut mic_stream = None;
    let mut mic_writer = None;
    let mut sys_stream = None;
    let mut sys_writer = None;

    // --- Micrófono ---
    if want_mic {
        match start_mic_stream(
            &host,
            &config.mic_wav,
            mic_meter.clone(),
            events.clone(),
            &config.noise_suppression,
            &config.mic_device_id,
            config.stt_tap.clone(),
        ) {
            Ok((stream, writer)) => {
                mic_stream = Some(stream);
                mic_writer = Some(writer);
            }
            Err(err) => {
                if !want_system {
                    let _ = ready_tx.send(Err(err));
                    return CaptureSummary::default();
                }
                tracing::warn!(%err, "micrófono no disponible; continuando solo con sistema");
                let _ = events.send(CaptureEvent::Error(if config.english {
                    format!("Microphone unavailable: {}", err.to_ui(true))
                } else {
                    format!("Micrófono no disponible: {err}")
                }));
            }
        }
    }

    // --- Audio del sistema / loopback ---
    if want_system {
        match try_start_system_stream(
            &host,
            &config.system_wav,
            sys_meter.clone(),
            events.clone(),
            &config.output_device_id,
            config.stt_tap.clone(),
        ) {
            Ok((stream, writer)) => {
                sys_stream = Some(stream);
                sys_writer = Some(writer);
            }
            Err(err) => {
                if mic_stream.is_none() {
                    let _ = ready_tx.send(Err(err));
                    return CaptureSummary::default();
                }
                tracing::warn!(%err, "audio del sistema no disponible; grabando solo micrófono");
                let _ = events.send(CaptureEvent::Error(if config.english {
                    format!("System audio unavailable: {}", err.to_ui(true))
                } else {
                    format!("Audio del sistema no disponible: {err}")
                }));
            }
        }
    }

    if mic_stream.is_none() && sys_stream.is_none() {
        let _ = ready_tx.send(Err(AudioError::Config(
            "no se pudo abrir ninguna pista de audio".into(),
        )));
        return CaptureSummary::default();
    }

    let _ = ready_tx.send(Ok(()));
    let started = Instant::now();

    loop {
        match stop_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {
                let mic = mic_meter.current();
                let system = sys_meter.current();
                let _ = events.send(CaptureEvent::Levels { mic, system });
            }
        }
    }

    let duration_secs = started.elapsed().as_secs_f64();

    drop(mic_stream);
    drop(sys_stream);

    let mic_written = match mic_writer {
        Some(writer) => matches!(writer.join(), Ok(Ok(_))),
        None => false,
    };
    let system_written = match sys_writer {
        Some(writer) => matches!(writer.join(), Ok(Ok(_))),
        None => false,
    };

    CaptureSummary {
        duration_secs,
        mic_written,
        system_written,
        mic_peak_rms: mic_meter.peak(),
        system_peak_rms: sys_meter.peak(),
    }
}

#[derive(Clone)]
struct LevelMeter {
    current: Arc<AtomicU32>,
    peak: Arc<AtomicU32>,
}

impl LevelMeter {
    fn new() -> Self {
        Self {
            current: Arc::new(AtomicU32::new(0)),
            peak: Arc::new(AtomicU32::new(0)),
        }
    }

    fn observe(&self, rms: f32) {
        self.current.store(rms.to_bits(), Ordering::Relaxed);
        self.peak.fetch_max(rms.to_bits(), Ordering::Relaxed);
    }

    fn current(&self) -> f32 {
        f32::from_bits(self.current.load(Ordering::Relaxed))
    }

    fn peak(&self) -> f32 {
        f32::from_bits(self.peak.load(Ordering::Relaxed))
    }
}

fn start_mic_stream(
    host: &cpal::Host,
    path: &Path,
    meter: LevelMeter,
    events: Sender<CaptureEvent>,
    noise_suppression: &str,
    mic_device_id: &str,
    stt_tap: Option<SyncSender<AudioTapChunk>>,
) -> Result<(cpal::Stream, JoinHandle<Result<u64, AudioError>>), AudioError> {
    let device = resolve_input_device(host, mic_device_id)?;
    let supported = resolve_input_config(&device)?;
    if let Ok(name) = device.name() {
        tracing::info!(%name, "usando micrófono");
    }
    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    let writer = if let Some(noise_level) = NoiseLevel::parse(noise_suppression) {
        let proc =
            MicNoiseProcessor::new(supported.channels(), supported.sample_rate().0, noise_level);
        tracing::info!(
            rate = supported.sample_rate().0,
            channels = supported.channels(),
            level = ?noise_level,
            "supresión de ruido activa en micrófono"
        );
        spawn_mic_noise_writer(path.to_path_buf(), proc, rx)
    } else {
        spawn_writer(path.to_path_buf(), wav_spec(&supported), rx)
    };
    let stream = build_capture_stream(
        &device,
        &supported,
        tx,
        meter,
        events,
        stt_tap,
        CaptureTrack::Mic,
    )?;
    stream.play()?;
    Ok((stream, writer))
}

/// Intenta abrir el loopback del dispositivo de salida elegido (o el default).
fn try_start_system_stream(
    host: &cpal::Host,
    path: &Path,
    meter: LevelMeter,
    events: Sender<CaptureEvent>,
    output_device_id: &str,
    stt_tap: Option<SyncSender<AudioTapChunk>>,
) -> Result<(cpal::Stream, JoinHandle<Result<u64, AudioError>>), AudioError> {
    #[cfg(target_os = "macos")]
    {
        let _ = (host, path, meter, events, output_device_id, stt_tap);
        // Fase 4: ScreenCaptureKit / Core Audio taps. Mientras tanto solo mic.
        return Err(AudioError::Config(
            "En macOS el audio del sistema requiere ScreenCaptureKit (fase 4). \
             Grabando solo micrófono."
                .into(),
        ));
    }

    #[cfg(not(target_os = "macos"))]
    {
        let device = resolve_output_device(host, output_device_id)?;
        // En WASAPI, construir un stream de entrada sobre un dispositivo de salida
        // usando su formato de reproducción activa el modo loopback.
        let supported = device
            .default_output_config()
            .map_err(|err| AudioError::Config(err.to_string()))?;

        let (tx, rx) = mpsc::channel::<Vec<f32>>();
        let writer = spawn_writer(path.to_path_buf(), wav_spec(&supported), rx);
        let stream = build_capture_stream(
            &device,
            &supported,
            tx,
            meter,
            events,
            stt_tap,
            CaptureTrack::System,
        )?;
        stream.play()?;
        Ok((stream, writer))
    }
}

/// Hilo escritor: recibe buffers de f32 y los vuelca a un WAV, finalizándolo
/// cuando el canal se cierra.
fn spawn_writer(
    path: PathBuf,
    spec: hound::WavSpec,
    rx: Receiver<Vec<f32>>,
) -> JoinHandle<Result<u64, AudioError>> {
    thread::spawn(move || {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut writer = hound::WavWriter::create(&path, spec)?;
        let mut written: u64 = 0;
        while let Ok(buf) = rx.recv() {
            for sample in buf {
                writer.write_sample(sample)?;
                written += 1;
            }
        }
        writer.finalize()?;
        Ok(written)
    })
}

/// Escritor de micrófono con high-pass + gate + RNNoise (solo esta pista).
fn spawn_mic_noise_writer(
    path: PathBuf,
    mut proc: MicNoiseProcessor,
    rx: Receiver<Vec<f32>>,
) -> JoinHandle<Result<u64, AudioError>> {
    thread::spawn(move || {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut writer = hound::WavWriter::create(&path, proc.out_spec())?;
        let mut written: u64 = 0;
        while let Ok(buf) = rx.recv() {
            for sample in proc.process(&buf) {
                writer.write_sample(sample)?;
                written += 1;
            }
        }
        for sample in proc.flush() {
            writer.write_sample(sample)?;
            written += 1;
        }
        writer.finalize()?;
        Ok(written)
    })
}

fn wav_spec(cfg: &cpal::SupportedStreamConfig) -> hound::WavSpec {
    hound::WavSpec {
        channels: cfg.channels(),
        sample_rate: cfg.sample_rate().0,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    }
}

/// Construye el stream de entrada, eligiendo la conversión a f32 según el
/// formato de muestra nativo del dispositivo.
fn build_capture_stream(
    device: &cpal::Device,
    supported: &cpal::SupportedStreamConfig,
    tx: Sender<Vec<f32>>,
    meter: LevelMeter,
    events: Sender<CaptureEvent>,
    stt_tap: Option<SyncSender<AudioTapChunk>>,
    track: CaptureTrack,
) -> Result<cpal::Stream, AudioError> {
    let config: cpal::StreamConfig = supported.config();
    let sample_rate = config.sample_rate.0;
    let channels = config.channels;
    match supported.sample_format() {
        cpal::SampleFormat::F32 => build_typed::<f32>(
            device,
            &config,
            tx,
            meter,
            events,
            stt_tap,
            track,
            sample_rate,
            channels,
            |s| s,
        ),
        cpal::SampleFormat::I16 => build_typed::<i16>(
            device,
            &config,
            tx,
            meter,
            events,
            stt_tap,
            track,
            sample_rate,
            channels,
            |s| s as f32 / 32768.0,
        ),
        cpal::SampleFormat::U16 => build_typed::<u16>(
            device,
            &config,
            tx,
            meter,
            events,
            stt_tap,
            track,
            sample_rate,
            channels,
            |s| (s as f32 - 32768.0) / 32768.0,
        ),
        cpal::SampleFormat::I32 => build_typed::<i32>(
            device,
            &config,
            tx,
            meter,
            events,
            stt_tap,
            track,
            sample_rate,
            channels,
            |s| s as f32 / 2_147_483_648.0,
        ),
        cpal::SampleFormat::U8 => build_typed::<u8>(
            device,
            &config,
            tx,
            meter,
            events,
            stt_tap,
            track,
            sample_rate,
            channels,
            |s| (s as f32 - 128.0) / 128.0,
        ),
        other => Err(AudioError::UnsupportedFormat(format!("{other:?}"))),
    }
}

#[allow(clippy::too_many_arguments)]
fn build_typed<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    tx: Sender<Vec<f32>>,
    meter: LevelMeter,
    events: Sender<CaptureEvent>,
    stt_tap: Option<SyncSender<AudioTapChunk>>,
    track: CaptureTrack,
    sample_rate: u32,
    channels: u16,
    conv: fn(T) -> f32,
) -> Result<cpal::Stream, AudioError>
where
    T: cpal::SizedSample + Send + 'static,
{
    let frames_seen = Arc::new(AtomicU64::new(0));
    let ch = channels.max(1) as usize;
    let stream = device.build_input_stream(
        config,
        move |data: &[T], _: &cpal::InputCallbackInfo| {
            let mut buf = Vec::with_capacity(data.len());
            let mut sum_sq = 0.0f64;
            for &sample in data {
                let value = conv(sample);
                sum_sq += (value as f64) * (value as f64);
                buf.push(value);
            }
            let rms = if data.is_empty() {
                0.0
            } else {
                (sum_sq / data.len() as f64).sqrt() as f32
            };
            meter.observe(rms);

            if let Some(tap) = &stt_tap {
                let n_frames = (data.len() / ch) as u64;
                let start_frames = frames_seen.fetch_add(n_frames, Ordering::Relaxed);
                let start_ms = if sample_rate == 0 {
                    0
                } else {
                    ((start_frames * 1000) / u64::from(sample_rate)) as i64
                };
                let _ = tap.try_send(AudioTapChunk {
                    track,
                    start_ms,
                    sample_rate,
                    channels,
                    samples: buf.clone(),
                });
            }

            let _ = tx.send(buf);
        },
        move |err| {
            tracing::error!(%err, "error en stream de captura");
            let _ = events.send(CaptureEvent::Error(err.to_string()));
        },
        None,
    )?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(name: &str, is_default: bool) -> InputDeviceInfo {
        InputDeviceInfo {
            id: name.into(),
            name: name.into(),
            is_default,
            may_not_open: false,
            is_bluetooth: looks_like_bluetooth(name),
            is_hands_free: looks_like_hands_free(name),
            sample_rate: Some(48_000),
            channels: Some(2),
        }
    }

    #[test]
    fn summary_default_is_empty() {
        let s = CaptureSummary::default();
        assert_eq!(s.duration_secs, 0.0);
        assert!(!s.mic_written);
        assert!(!s.system_written);
    }

    #[test]
    fn event_is_clone() {
        let e = CaptureEvent::Levels {
            mic: 0.5,
            system: 0.25,
        };
        let _ = e.clone();
    }

    #[test]
    fn rejects_no_tracks() {
        let result = CaptureSession::start(
            CaptureConfig {
                mic_wav: PathBuf::from("mic.wav"),
                system_wav: PathBuf::from("system.wav"),
                capture_mic: false,
                capture_system: false,
                noise_suppression: "off".into(),
                mic_device_id: String::new(),
                output_device_id: String::new(),
                stt_tap: None,
                english: false,
            },
            mpsc::channel().0,
        );
        match result {
            Err(AudioError::Config(_)) => {}
            Ok(_) => panic!("expected Config error"),
            Err(other) => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn bluetooth_advisory_when_mic_is_headset() {
        let inputs = vec![device(
            "Headset Microphone (WH-1000XM4 Hands-Free AG Audio)",
            true,
        )];
        let outputs = vec![device("Headphones (WH-1000XM4 Stereo)", true)];
        let advisory = bluetooth_recording_advisory_for_devices(
            true,
            true,
            "Headset Microphone (WH-1000XM4 Hands-Free AG Audio)",
            "Headphones (WH-1000XM4 Stereo)",
            &inputs,
            &outputs,
            false,
        );
        assert!(advisory.is_some());
        let a = advisory.unwrap();
        assert!(a.message.contains("Bluetooth"));
        assert!(a.suggestion.as_ref().is_some_and(|s| s.contains("Realtek")));
    }

    #[test]
    fn bluetooth_advisory_skips_system_only() {
        let inputs = vec![device("Headset Microphone (Hands-Free AG Audio)", true)];
        let outputs = vec![device("Headphones (Stereo)", true)];
        assert!(bluetooth_recording_advisory_for_devices(
            false,
            true,
            "Headset Microphone (Hands-Free AG Audio)",
            "Headphones (Stereo)",
            &inputs,
            &outputs,
            false,
        )
        .is_none());
    }

    #[test]
    fn bluetooth_advisory_skips_non_headset_mic() {
        let inputs = vec![device("Microphone Array (Realtek Audio)", true)];
        let outputs = vec![device("Speakers (Realtek Audio)", true)];
        assert!(bluetooth_recording_advisory_for_devices(
            true,
            true,
            "Microphone Array (Realtek Audio)",
            "Speakers (Realtek Audio)",
            &inputs,
            &outputs,
            false,
        )
        .is_none());
    }

    #[test]
    fn headset_heuristic_matches_windows_bt_names() {
        assert!(looks_like_headset(
            "Headset Microphone (WH-1000XM4 Hands-Free AG Audio)"
        ));
        assert!(looks_like_headset("Hands-Free AG Audio"));
        assert!(looks_like_headset("Auriculares Bluetooth"));
        assert!(looks_like_headset("HFP Hands Free"));
        assert!(looks_like_headset("Communications Microphone"));
        assert!(!looks_like_headset("Speakers (Realtek Audio)"));
        assert!(!looks_like_headset("Stereo Mix (Realtek Audio)"));
    }

    #[test]
    fn bluetooth_heuristic_does_not_flag_usb_headsets() {
        assert!(!looks_like_bluetooth("USB Headset (Jabra Evolve2)"));
        assert!(looks_like_headset("USB Headset (Jabra Evolve2)"));
        assert!(looks_like_bluetooth("AirPods Hands-Free AG Audio"));
    }
}
