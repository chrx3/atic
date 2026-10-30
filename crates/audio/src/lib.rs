//! Captura de audio para Atic.
//!
//! Graba micrófono y/o audio del sistema (loopback WASAPI en Windows,
//! ScreenCaptureKit en macOS) a WAV separados, y publica niveles RMS
//! periódicos para el medidor de la UI.
//!
//! El `cpal::Stream` no es `Send` en Windows, por lo que toda la captura vive
//! en un hilo dedicado ("audio-capture") controlado por canales. `start`
//! espera a que los streams estén activos antes de devolver el handle, de modo
//! que los errores de inicio se propagan de inmediato.
//!
//! Las dos pistas comparten un origen (el `Instant` de la sesión): cada bloque
//! llega con el instante de su primera muestra, sacado del reloj del hardware,
//! y el escritor lo ubica en esa línea de tiempo. Así una pista que arranca
//! tarde parte con silencio, los huecos (el sistema sin sonar, un micrófono
//! que se cambia) se rellenan y al detener las dos quedan del mismo largo.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use thiserror::Error;

mod noise;
#[cfg(target_os = "macos")]
mod screencapturekit;
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
    #[error("audio del sistema no soportado: {0}")]
    SystemAudioUnsupported(String),
    #[error("falta el permiso de Grabación de pantalla para el audio del sistema")]
    SystemAudioPermission,
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
            Self::SystemAudioUnsupported(detail) => {
                if en {
                    format!("System audio is not supported: {detail}")
                } else {
                    self.to_string()
                }
            }
            Self::SystemAudioPermission => {
                if en {
                    "Atic needs Screen Recording permission for system audio. Enable it in \
                     Settings → Privacy & Security → Screen Recording, then restart Atic."
                        .into()
                } else {
                    "Atic necesita permiso de Grabación de pantalla para el audio del sistema. \
                     Actívalo en Ajustes → Privacidad y seguridad → Grabación de pantalla y \
                     reinicia Atic."
                        .into()
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
    /// Inicio del bloque en ms desde el inicio de la sesión (común a las dos
    /// pistas, el mismo origen que los WAV).
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

/// Corrige el largo que dice el encabezado de un WAV que no se cerró (la app
/// se cayó grabando): el de los datos pasa a ser lo que de verdad hay en el
/// archivo, sin el último cuadro si quedó a medias. Devuelve `true` si cambió
/// algo. Un archivo que no es WAV o no tiene datos es un error.
pub fn repair_wav_header(path: &Path) -> std::io::Result<bool> {
    use std::io::{Read, Seek, SeekFrom, Write};
    let bad = |what: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, what.to_string());
    let mut file = std::fs::OpenOptions::new().read(true).write(true).open(path)?;
    let len = file.metadata()?.len();
    let mut head = [0u8; 12];
    file.read_exact(&mut head)?;
    if &head[0..4] != b"RIFF" || &head[8..12] != b"WAVE" {
        return Err(bad("no es un WAV"));
    }
    let mut offset = 12u64;
    let mut block_align = 1u64;
    loop {
        if offset + 8 > len {
            return Err(bad("el WAV no tiene datos"));
        }
        file.seek(SeekFrom::Start(offset))?;
        let mut chunk = [0u8; 8];
        file.read_exact(&mut chunk)?;
        let size = u64::from(u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]));
        if &chunk[0..4] == b"fmt " {
            // `nBlockAlign`: bytes por cuadro (todos los canales).
            let mut fmt = [0u8; 14];
            file.read_exact(&mut fmt)?;
            block_align = u64::from(u16::from_le_bytes([fmt[12], fmt[13]])).max(1);
        }
        if &chunk[0..4] == b"data" {
            let start = offset + 8;
            let actual = (len - start) / block_align * block_align;
            let end = start + actual;
            if size == actual && end == len {
                return Ok(false);
            }
            let data_size = u32::try_from(actual).map_err(|_| bad("el WAV pasa de 4 GiB"))?;
            file.seek(SeekFrom::Start(offset + 4))?;
            file.write_all(&data_size.to_le_bytes())?;
            file.seek(SeekFrom::Start(4))?;
            file.write_all(&((end - 8) as u32).to_le_bytes())?;
            file.set_len(end)?;
            file.sync_all()?;
            return Ok(true);
        }
        offset += 8 + size + (size & 1);
    }
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

/// Hilo que vuelca una pista a WAV hasta que se le pida cerrar.
///
/// El cierre va por canal propio y no por la caída del `Sender` de audio: en
/// CoreAudio el callback del stream puede sobrevivir al `drop` del stream, el
/// writer quedaría esperando en `recv()` para siempre y `CaptureHandle::stop`
/// no volvería (la pill se quedaba en «Transcribiendo…»).
struct TrackWriter {
    /// `Some(fin)`: rellenar con silencio hasta `fin` (desde el origen de la
    /// sesión) antes de cerrar; `None`: cerrar tal cual.
    done: Sender<Option<Duration>>,
    join: JoinHandle<Result<u64, AudioError>>,
}

impl TrackWriter {
    /// Pide el cierre, drena lo pendiente y espera al hilo. Con `end`, la
    /// pista se completa con silencio hasta ese instante: las dos pistas de
    /// una sesión terminan juntas. Devuelve `true` solo si el WAV quedó
    /// finalizado.
    fn finish(self, end: Option<Duration>) -> bool {
        let _ = self.done.send(end);
        matches!(self.join.join(), Ok(Ok(_)))
    }
}

/// Un bloque de una pista con el instante de su primera muestra, contado
/// desde el origen de la sesión.
pub(crate) struct TimedBuffer {
    pub(crate) at: Duration,
    pub(crate) samples: Vec<f32>,
}

/// Lo que una pista puede desviarse del reloj de la sesión antes de
/// corregirla: menos que esto no se oye como desfase y no vale un corte.
const ALIGN_TOLERANCE: Duration = Duration::from_millis(40);
/// Un bloque que llega más de esto «antes» de lo ya escrito no se recorta:
/// sería un reloj roto y no deriva, y se perdería audio de verdad.
const MAX_SKIP: Duration = Duration::from_secs(2);
/// Si el reloj del hardware se aparta más que esto de la hora de llegada, se
/// vuelve a anclar (el dispositivo se reinició o el backend no da instantes).
const REANCHOR: Duration = Duration::from_millis(1500);

/// Qué hacer con un bloque para que su primera muestra caiga donde dice su
/// instante.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Align {
    /// Va donde corresponde (dentro de la tolerancia).
    Keep,
    /// Hubo un hueco: tantos cuadros de silencio antes del bloque.
    Pad(u64),
    /// La pista se adelantó (deriva del reloj): se saltan tantos cuadros.
    Skip(u64),
}

/// `written`: cuadros ya escritos; `at`: cuadro donde debería empezar el
/// bloque. Todo en cuadros de la pista.
pub(crate) fn align(written: u64, at: u64, tolerance: u64, max_skip: u64) -> Align {
    if at > written.saturating_add(tolerance) {
        Align::Pad(at - written)
    } else if written > at.saturating_add(tolerance) && written - at <= max_skip {
        Align::Skip(written - at)
    } else {
        Align::Keep
    }
}

/// Cuadros que caben en `at` a `rate` Hz (redondeo hacia abajo).
pub(crate) fn frames_at(at: Duration, rate: u32) -> u64 {
    (at.as_nanos() * u128::from(rate) / 1_000_000_000) as u64
}

/// La línea de tiempo de un WAV: cuántos cuadros lleva y dónde va cada
/// bloque que llega.
pub(crate) struct Timeline {
    rate: u32,
    channels: usize,
    frames: u64,
}

impl Timeline {
    pub(crate) fn new(rate: u32, channels: u16) -> Self {
        Self {
            rate: rate.max(1),
            channels: usize::from(channels.max(1)),
            frames: 0,
        }
    }

    /// Ubica un bloque de `samples` muestras intercaladas que empieza en
    /// `at`. Devuelve cuántas muestras de silencio escribir antes y cuántas
    /// del comienzo del bloque saltar.
    ///
    /// La primera vez no hay tolerancia: el silencio inicial (lo que la pista
    /// tardó en arrancar) se respeta exacto.
    pub(crate) fn place(&mut self, at: Duration, samples: usize) -> (usize, usize) {
        let len = (samples / self.channels) as u64;
        let tolerance = if self.frames == 0 {
            0
        } else {
            frames_at(ALIGN_TOLERANCE, self.rate)
        };
        let max_skip = frames_at(MAX_SKIP, self.rate);
        match align(self.frames, frames_at(at, self.rate), tolerance, max_skip) {
            Align::Keep => {
                self.frames += len;
                (0, 0)
            }
            Align::Pad(gap) => {
                self.frames += gap + len;
                (gap as usize * self.channels, 0)
            }
            Align::Skip(ahead) => {
                let skip = ahead.min(len);
                self.frames += len - skip;
                (0, skip as usize * self.channels)
            }
        }
    }

    /// Muestras de silencio que faltan para llegar a `end`.
    pub(crate) fn pad_to(&mut self, end: Duration) -> usize {
        let missing = frames_at(end, self.rate).saturating_sub(self.frames);
        self.frames += missing;
        missing as usize * self.channels
    }
}

/// Pasa los instantes del hardware (host time en Mac, QPC en Windows) a la
/// línea de tiempo de la sesión.
///
/// El primer bloque se ancla a la hora en que llegó (`estimate`); de ahí en
/// adelante manda el reloj del hardware, que no tiene el jitter de cuándo se
/// despierta el callback y marca los huecos de verdad. Si los dos se apartan
/// más de [`REANCHOR`], se vuelve a anclar.
pub(crate) struct Clock {
    /// `(hardware, sesión)` del ancla.
    anchor: Option<(Duration, Duration)>,
}

impl Clock {
    pub(crate) const fn new() -> Self {
        Self { anchor: None }
    }

    /// `hw`: instante del hardware de la primera muestra (cualquier origen,
    /// solo importan las diferencias); `estimate`: dónde cae según la hora
    /// de llegada.
    pub(crate) fn stamp(&mut self, hw: Option<Duration>, estimate: Duration) -> Duration {
        if let (Some(hw), Some((hw0, at0))) = (hw, self.anchor) {
            if let Some(at) = hw.checked_sub(hw0).map(|d| at0 + d) {
                if at.abs_diff(estimate) <= REANCHOR {
                    return at;
                }
            }
        }
        self.anchor = hw.map(|hw| (hw, estimate));
        estimate
    }
}

/// Lo que pasa entre la línea de tiempo y el WAV.
pub(crate) enum Stage {
    /// Tal cual llega.
    Plain,
    /// High-pass + gate + RNNoise (solo micrófono).
    Noise(Box<MicNoiseProcessor>),
}

/// El WAV de una pista, con su línea de tiempo y lo que se le aplica.
struct Sink {
    writer: hound::WavWriter<std::io::BufWriter<std::fs::File>>,
    stage: Stage,
    timeline: Timeline,
    channels: usize,
    track: CaptureTrack,
    started: bool,
    written: u64,
}

impl Sink {
    fn push(&mut self, samples: &[f32]) -> Result<(), AudioError> {
        match &mut self.stage {
            Stage::Plain => {
                for &sample in samples {
                    self.writer.write_sample(sample)?;
                }
                self.written += samples.len() as u64;
            }
            Stage::Noise(proc) => {
                let out = proc.process(samples);
                for &sample in &out {
                    self.writer.write_sample(sample)?;
                }
                self.written += out.len() as u64;
            }
        }
        Ok(())
    }

    /// Silencio en trozos de cuadros enteros (la supresión de ruido baja a
    /// mono por cuadro).
    fn silence(&mut self, samples: usize) -> Result<(), AudioError> {
        if samples == 0 {
            return Ok(());
        }
        let zeros = vec![0.0f32; 4096 * self.channels];
        let mut left = samples;
        while left > 0 {
            let n = left.min(zeros.len());
            self.push(&zeros[..n])?;
            left -= n;
        }
        Ok(())
    }

    fn place(&mut self, buffer: &TimedBuffer) -> Result<(), AudioError> {
        let (silence, skip) = self.timeline.place(buffer.at, buffer.samples.len());
        if !self.started {
            self.started = true;
            tracing::info!(
                track = ?self.track,
                offset_ms = buffer.at.as_millis() as u64,
                "pista arranca en la línea de tiempo de la sesión"
            );
        } else if silence > 0 {
            tracing::debug!(
                track = ?self.track,
                at_ms = buffer.at.as_millis() as u64,
                silence_frames = silence / self.channels,
                "hueco en la pista: se rellena con silencio"
            );
        } else if skip > 0 {
            tracing::debug!(
                track = ?self.track,
                at_ms = buffer.at.as_millis() as u64,
                skipped_frames = skip / self.channels,
                "pista adelantada al reloj de la sesión: se recorta"
            );
        }
        self.silence(silence)?;
        self.push(&buffer.samples[skip.min(buffer.samples.len())..])
    }

    fn finish(mut self) -> Result<u64, AudioError> {
        if let Stage::Noise(proc) = &mut self.stage {
            for sample in proc.flush() {
                self.writer.write_sample(sample)?;
                self.written += 1;
            }
        }
        self.writer.finalize()?;
        Ok(self.written)
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

/// Pista de sistema activa: loopback WASAPI (cpal) o ScreenCaptureKit.
///
/// El valor no se lee nunca: tenerlo es lo que mantiene viva la captura, y
/// soltarlo la detiene (RAII). De ahí el `allow`, en todas las plataformas.
#[allow(dead_code)]
enum SystemStream {
    #[cfg(not(target_os = "macos"))]
    Cpal(cpal::Stream),
    #[cfg(target_os = "macos")]
    Sck(crate::screencapturekit::SystemAudioGuard),
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
    let mut mic_name = String::new();
    let mut sys_stream: Option<SystemStream> = None;
    let mut sys_writer = None;

    // El cero común de las dos pistas, antes de abrir nada: lo que cada una
    // tarde en arrancar (ScreenCaptureKit puede demorar más de medio
    // segundo) queda como silencio al comienzo de su WAV.
    let origin = Instant::now();

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
            origin,
        ) {
            Ok((stream, writer, name)) => {
                mic_stream = Some(stream);
                mic_writer = Some(writer);
                mic_name = name;
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
            config.english,
            origin,
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
    let mut mic_checked = mic_stream.is_none();

    loop {
        match stop_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {
                if !mic_checked && started.elapsed() >= MIC_GRACE {
                    mic_checked = true;
                    if mic_meter.buffers() == 0 {
                        tracing::warn!(name = %mic_name, "el micrófono no entrega audio; buscando otro");
                        drop(mic_stream.take());
                        if let Some(writer) = mic_writer.take() {
                            writer.finish(None);
                        }
                        let replaced = replace_dead_mic(
                            &host, &config, &mic_meter, &events, &mic_name, origin,
                        );
                        let message = match replaced {
                            Some((stream, writer, name)) => {
                                mic_stream = Some(stream);
                                mic_writer = Some(writer);
                                let message = if config.english {
                                    format!("«{mic_name}» isn't sending audio: recording with «{name}».")
                                } else {
                                    format!("«{mic_name}» no entrega audio: se graba con «{name}».")
                                };
                                mic_name = name;
                                message
                            }
                            None if config.english => format!(
                                "«{mic_name}» isn't sending audio and no other microphone works."
                            ),
                            None => format!(
                                "«{mic_name}» no entrega audio y ningún otro micrófono responde."
                            ),
                        };
                        let _ = events.send(CaptureEvent::Error(message));
                    }
                }
                let mic = mic_meter.current();
                let system = sys_meter.current();
                let _ = events.send(CaptureEvent::Levels { mic, system });
            }
        }
    }

    // Las dos pistas se completan hasta el mismo instante: quedan del mismo
    // largo aunque una haya dejado de recibir audio antes.
    let end = origin.elapsed();
    let duration_secs = end.as_secs_f64();

    drop(mic_stream);
    drop(sys_stream);

    let mic_written = mic_writer.is_some_and(|w| w.finish(Some(end)));
    let system_written = sys_writer.is_some_and(|w| w.finish(Some(end)));

    CaptureSummary {
        duration_secs,
        mic_written,
        system_written,
        mic_peak_rms: mic_meter.peak(),
        system_peak_rms: sys_meter.peak(),
    }
}

#[derive(Clone)]
pub(crate) struct LevelMeter {
    current: Arc<AtomicU32>,
    peak: Arc<AtomicU32>,
    buffers: Arc<AtomicU64>,
}

impl LevelMeter {
    pub(crate) fn new() -> Self {
        Self {
            current: Arc::new(AtomicU32::new(0)),
            peak: Arc::new(AtomicU32::new(0)),
            buffers: Arc::new(AtomicU64::new(0)),
        }
    }

    pub(crate) fn observe(&self, rms: f32) {
        self.current.store(rms.to_bits(), Ordering::Relaxed);
        self.peak.fetch_max(rms.to_bits(), Ordering::Relaxed);
        self.buffers.fetch_add(1, Ordering::Relaxed);
    }

    /// Buffers recibidos. Distinto de silencio: un endpoint que Windows da
    /// por activo (un adaptador USB sin nada enchufado) puede abrir sin error
    /// y no llamar nunca al callback.
    fn buffers(&self) -> u64 {
        self.buffers.load(Ordering::Relaxed)
    }

    fn current(&self) -> f32 {
        f32::from_bits(self.current.load(Ordering::Relaxed))
    }

    fn peak(&self) -> f32 {
        f32::from_bits(self.peak.load(Ordering::Relaxed))
    }
}

#[allow(clippy::too_many_arguments)]
fn start_mic_stream(
    host: &cpal::Host,
    path: &Path,
    meter: LevelMeter,
    events: Sender<CaptureEvent>,
    noise_suppression: &str,
    mic_device_id: &str,
    stt_tap: Option<SyncSender<AudioTapChunk>>,
    origin: Instant,
) -> Result<(cpal::Stream, TrackWriter, String), AudioError> {
    let device = resolve_input_device(host, mic_device_id)?;
    let supported = resolve_input_config(&device)?;
    let name = device.name().unwrap_or_default();
    tracing::info!(%name, "usando micrófono");
    let (tx, rx) = mpsc::channel::<TimedBuffer>();
    // Un micrófono que entra tarde (el de reemplazo) parte con silencio: el
    // escritor lo ubica en la línea de tiempo de la sesión.
    let rate = supported.sample_rate().0;
    let channels = supported.channels();
    let writer = if let Some(noise_level) = NoiseLevel::parse(noise_suppression) {
        let proc = MicNoiseProcessor::new(channels, rate, noise_level);
        tracing::info!(
            rate,
            channels,
            level = ?noise_level,
            "supresión de ruido activa en micrófono"
        );
        let spec = proc.out_spec();
        spawn_writer(
            path.to_path_buf(),
            spec,
            rate,
            channels,
            Stage::Noise(Box::new(proc)),
            rx,
            CaptureTrack::Mic,
        )
    } else {
        spawn_writer(
            path.to_path_buf(),
            wav_spec(&supported),
            rate,
            channels,
            Stage::Plain,
            rx,
            CaptureTrack::Mic,
        )
    };
    let stream = build_capture_stream(
        &device,
        &supported,
        tx,
        meter,
        events,
        stt_tap,
        CaptureTrack::Mic,
        origin,
    )?;
    stream.play()?;
    Ok((stream, writer, name))
}

/// Micrófonos a probar cuando el elegido no entrega audio: los demás que
/// abren, sin Bluetooth (pasaría los audífonos a manos libres sin avisar).
fn fallback_mics(devices: Vec<InputDeviceInfo>, dead_name: &str) -> Vec<InputDeviceInfo> {
    devices
        .into_iter()
        .filter(|d| d.name != dead_name && !d.is_bluetooth && !d.may_not_open)
        .collect()
}

/// El micrófono abrió pero no entregó nada: se cierra su WAV vacío y se
/// prueba con los demás hasta dar con uno que sí entregue buffers. Devuelve
/// el stream nuevo y su nombre, o `None` si ninguno sirvió.
fn replace_dead_mic(
    host: &cpal::Host,
    config: &CaptureConfig,
    meter: &LevelMeter,
    events: &Sender<CaptureEvent>,
    dead_name: &str,
    origin: Instant,
) -> Option<(cpal::Stream, TrackWriter, String)> {
    let candidates = fallback_mics(list_input_devices().ok()?, dead_name);
    for candidate in candidates {
        let before = meter.buffers();
        let opened = start_mic_stream(
            host,
            &config.mic_wav,
            meter.clone(),
            events.clone(),
            &config.noise_suppression,
            &candidate.id,
            config.stt_tap.clone(),
            origin,
        );
        let (stream, writer, name) = match opened {
            Ok(opened) => opened,
            Err(err) => {
                tracing::warn!(name = %candidate.name, %err, "micrófono de reemplazo no abrió");
                continue;
            }
        };
        let deadline = Instant::now() + MIC_PROBE;
        while Instant::now() < deadline && meter.buffers() == before {
            thread::sleep(Duration::from_millis(50));
        }
        if meter.buffers() > before {
            return Some((stream, writer, name));
        }
        tracing::warn!(%name, "micrófono de reemplazo tampoco entrega audio");
        drop(stream);
        writer.finish(None);
    }
    None
}

/// Una entrada escuchada por [`InputMonitor`].
#[derive(Clone)]
pub struct MonitoredInput {
    pub info: InputDeviceInfo,
    /// `false` si no se abrió: Bluetooth (pasaría los audífonos a manos
    /// libres), sin configs consultables o falló al abrir.
    pub listening: bool,
    meter: LevelMeter,
}

impl MonitoredInput {
    /// RMS del último buffer.
    pub fn level(&self) -> f32 {
        self.meter.current()
    }

    /// Si ya entregó algún buffer.
    pub fn alive(&self) -> bool {
        self.meter.buffers() > 0
    }
}

/// Escucha todas las entradas a la vez, sin grabar, para ver cuál responde.
/// Soltarlo cierra los streams.
pub struct InputMonitor {
    pub inputs: Vec<MonitoredInput>,
    _stop: Sender<()>,
}

/// Abre las entradas que se pueden abrir sin efectos y mide su nivel. Los
/// streams viven en un hilo propio (`cpal::Stream` no es `Send`).
pub fn monitor_inputs() -> Result<InputMonitor, AudioError> {
    let devices = list_input_devices()?;
    let (ready_tx, ready_rx) = mpsc::channel::<Vec<MonitoredInput>>();
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    thread::Builder::new()
        .name("audio-monitor".into())
        .spawn(move || {
            let host = audio_host();
            // Sin receptores: el audio no se guarda ni se acumula, solo se mide.
            let (tx, _) = mpsc::channel::<TimedBuffer>();
            let origin = Instant::now();
            let (events, _) = mpsc::channel::<CaptureEvent>();
            let mut streams = Vec::new();
            let mut inputs = Vec::new();
            for info in devices {
                let meter = LevelMeter::new();
                let stream = (!info.is_bluetooth && !info.may_not_open)
                    .then(|| {
                        let device = resolve_input_device(&host, &info.id)?;
                        let supported = resolve_input_config(&device)?;
                        let stream = build_capture_stream(
                            &device,
                            &supported,
                            tx.clone(),
                            meter.clone(),
                            events.clone(),
                            None,
                            CaptureTrack::Mic,
                            origin,
                        )?;
                        stream.play()?;
                        Ok::<_, AudioError>(stream)
                    })
                    .and_then(|opened| {
                        opened
                            .map_err(|err| tracing::warn!(name = %info.name, %err, "no se pudo escuchar"))
                            .ok()
                    });
                inputs.push(MonitoredInput { info, listening: stream.is_some(), meter });
                streams.extend(stream);
            }
            let _ = ready_tx.send(inputs);
            // Hasta que suelten el monitor.
            let _ = stop_rx.recv();
            drop(streams);
        })
        .map_err(AudioError::Io)?;
    let inputs = ready_rx.recv().map_err(|_| AudioError::CaptureThreadGone)?;
    Ok(InputMonitor { inputs, _stop: stop_tx })
}

/// Plazo para que el micrófono entregue su primer buffer.
const MIC_GRACE: Duration = Duration::from_secs(3);
/// Plazo para cada micrófono de reemplazo.
const MIC_PROBE: Duration = Duration::from_secs(2);

/// Intenta abrir la pista de sistema: loopback WASAPI en Windows,
/// ScreenCaptureKit en macOS. En Mac `output_device_id` se ignora (SCK captura
/// la mezcla del display elegido).
fn try_start_system_stream(
    host: &cpal::Host,
    path: &Path,
    meter: LevelMeter,
    events: Sender<CaptureEvent>,
    output_device_id: &str,
    stt_tap: Option<SyncSender<AudioTapChunk>>,
    english: bool,
    origin: Instant,
) -> Result<(SystemStream, TrackWriter), AudioError> {
    #[cfg(target_os = "macos")]
    {
        let _ = (host, output_device_id);
        let (guard, writer) =
            crate::screencapturekit::start(path, meter, events, stt_tap, english, origin)?;
        return Ok((SystemStream::Sck(guard), writer));
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = english;
        let device = resolve_output_device(host, output_device_id)?;
        // En WASAPI, construir un stream de entrada sobre un dispositivo de salida
        // usando su formato de reproducción activa el modo loopback.
        let supported = device
            .default_output_config()
            .map_err(|err| AudioError::Config(err.to_string()))?;

        // El loopback de WASAPI tampoco entrega nada mientras no suena
        // audio: la línea de tiempo rellena esos huecos.
        let (tx, rx) = mpsc::channel::<TimedBuffer>();
        let writer = spawn_writer(
            path.to_path_buf(),
            wav_spec(&supported),
            supported.sample_rate().0,
            supported.channels(),
            Stage::Plain,
            rx,
            CaptureTrack::System,
        );
        let stream = build_capture_stream(
            &device,
            &supported,
            tx,
            meter,
            events,
            stt_tap,
            CaptureTrack::System,
            origin,
        )?;
        stream.play()?;
        Ok((SystemStream::Cpal(stream), writer))
    }
}

/// Cada cuánto el hilo escritor deja el WAV válido en disco (`flush` de
/// hound: vuelca el búfer y corrige el largo del encabezado).
const WAV_CHECKPOINT: Duration = Duration::from_secs(5);

/// Hilo escritor: recibe bloques con su instante, los ubica en la línea de
/// tiempo de la sesión (silencio en los huecos) y los vuelca a un WAV,
/// finalizándolo cuando se le pide cerrar.
///
/// `rate` y `channels` son los de los bloques que llegan (antes de `stage`).
/// Si el canal de audio se corta antes de la señal de cierre, se espera a esa
/// señal igual: trae el instante final con que se completa la pista.
pub(crate) fn spawn_writer(
    path: PathBuf,
    spec: hound::WavSpec,
    rate: u32,
    channels: u16,
    stage: Stage,
    rx: Receiver<TimedBuffer>,
    track: CaptureTrack,
) -> TrackWriter {
    let (done_tx, done) = mpsc::channel::<Option<Duration>>();
    let join = thread::spawn(move || {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let writer = hound::WavWriter::create(&path, spec)?;
        let mut sink = Sink {
            writer,
            stage,
            timeline: Timeline::new(rate, channels),
            channels: usize::from(channels.max(1)),
            track,
            started: false,
            written: 0,
        };
        let mut checkpoint = Instant::now();
        let mut failed = false;
        let end = loop {
            match done.try_recv() {
                Ok(end) => break end,
                Err(TryRecvError::Disconnected) => break None,
                Err(TryRecvError::Empty) => {}
            }
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(buffer) => {
                    // Disco lleno o similar: se deja de escribir, pero lo ya
                    // grabado se cierra bien y la pista no se pierde.
                    if let Err(error) = sink.place(&buffer) {
                        tracing::error!(track = ?sink.track, %error, "no se pudo escribir la pista: se cierra con lo grabado");
                        failed = true;
                        break None;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break done.recv().ok().flatten(),
            }
            // Cada tanto el encabezado dice el largo real: si la app se cae
            // grabando, el WAV se puede leer hasta aquí (y no dice largo 0).
            if checkpoint.elapsed() >= WAV_CHECKPOINT {
                checkpoint = Instant::now();
                if let Err(error) = sink.writer.flush() {
                    tracing::warn!(track = ?sink.track, %error, "no se pudo actualizar el encabezado del WAV");
                }
            }
        };
        if failed {
            // Sin el resto de la supresión de ruido: con el disco lleno no
            // cabe, y bastaría para no cerrar el archivo.
            sink.writer.finalize()?;
            return Ok(sink.written);
        }
        // Lo que quedó encolado antes del cierre no se pierde.
        while let Ok(buffer) = rx.try_recv() {
            sink.place(&buffer)?;
        }
        if let Some(end) = end {
            let tail = sink.timeline.pad_to(end);
            sink.silence(tail)?;
        }
        sink.finish()
    });
    TrackWriter {
        done: done_tx,
        join,
    }
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
    tx: Sender<TimedBuffer>,
    meter: LevelMeter,
    events: Sender<CaptureEvent>,
    stt_tap: Option<SyncSender<AudioTapChunk>>,
    track: CaptureTrack,
    origin: Instant,
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
            origin,
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
            origin,
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
            origin,
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
            origin,
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
            origin,
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
    tx: Sender<TimedBuffer>,
    meter: LevelMeter,
    events: Sender<CaptureEvent>,
    stt_tap: Option<SyncSender<AudioTapChunk>>,
    track: CaptureTrack,
    origin: Instant,
    sample_rate: u32,
    channels: u16,
    conv: fn(T) -> f32,
) -> Result<cpal::Stream, AudioError>
where
    T: cpal::SizedSample + Send + 'static,
{
    let ch = channels.max(1) as usize;
    let mut clock = Clock::new();
    let mut first_capture: Option<cpal::StreamInstant> = None;
    let stream = device.build_input_stream(
        config,
        move |data: &[T], info: &cpal::InputCallbackInfo| {
            // Dónde cae este bloque en la sesión: por la hora de llegada
            // menos lo que dura (el callback llega cuando el bloque ya se
            // llenó) y, desde el segundo bloque, por el reloj del hardware
            // (`mHostTime` en Core Audio), que marca huecos y deriva.
            let arrival = origin.elapsed();
            let span = if sample_rate == 0 {
                Duration::ZERO
            } else {
                Duration::from_secs_f64((data.len() / ch) as f64 / f64::from(sample_rate))
            };
            let captured = info.timestamp().capture;
            let hw = captured.duration_since(first_capture.get_or_insert(captured));
            let at = clock.stamp(hw, arrival.saturating_sub(span));

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
                let _ = tap.try_send(AudioTapChunk {
                    track,
                    start_ms: at.as_millis() as i64,
                    sample_rate,
                    channels,
                    samples: buf.clone(),
                });
            }

            let _ = tx.send(TimedBuffer { at, samples: buf });
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

    #[test]
    fn repara_el_largo_de_un_wav_que_no_se_cerro() {
        let dir = std::env::temp_dir().join(format!("atic-audio-repair-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cortado.wav");
        let spec = hound::WavSpec { channels: 2, sample_rate: 48_000, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for i in 0..960 {
            writer.write_sample(i as f32 / 960.0).unwrap();
        }
        writer.finalize().unwrap();
        // Como si la app se cayera: el encabezado dice 0 y queda medio cuadro.
        let mut bytes = std::fs::read(&path).unwrap();
        let data = bytes.windows(4).position(|w| w == b"data").unwrap();
        bytes[data + 4..data + 8].copy_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&[0, 0, 0]);
        std::fs::write(&path, &bytes).unwrap();

        assert!(repair_wav_header(&path).unwrap());
        let reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.len(), 960);
        // Ya bien: no se toca.
        assert!(!repair_wav_header(&path).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

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
    fn fallback_mics_skip_dead_bluetooth_and_unopenable() {
        let mut unopenable = device("FrontMic (Realtek(R) Audio)", false);
        unopenable.may_not_open = true;
        let devices = vec![
            device("Micrófono (Realtek USB MIC)", true),
            device("Auriculares con micrófono (REDMI Buds 8 Pro Hands-Free)", false),
            unopenable,
            device("Micrófono (Realtek(R) Audio)", false),
        ];
        let names: Vec<_> = fallback_mics(devices, "Micrófono (Realtek USB MIC)")
            .into_iter()
            .map(|d| d.name)
            .collect();
        assert_eq!(names, ["Micrófono (Realtek(R) Audio)"]);
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

    /// Regresión: `CaptureHandle::stop` se quedaba colgado en `writer.join()`
    /// cuando el stream (CoreAudio) no soltaba su `Sender` al dropearse: la
    /// pill quedaba en «Transcribiendo…» para siempre.
    #[test]
    fn writer_cierra_con_el_canal_de_audio_abierto() {
        let dir = std::env::temp_dir().join(format!("atic-audio-writer-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("mic.wav");
        let (tx, rx) = mpsc::channel::<TimedBuffer>();
        let writer = spawn_writer(
            path.clone(),
            float_spec(1, 48_000),
            48_000,
            1,
            Stage::Plain,
            rx,
            CaptureTrack::Mic,
        );

        tx.send(TimedBuffer { at: Duration::ZERO, samples: vec![0.5; 480] }).unwrap();
        assert!(writer.finish(None), "el WAV debe finalizar sin cerrar el canal");

        // El Sender sigue vivo a propósito: es el caso que colgaba.
        drop(tx);
        let reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.duration(), 480);
        std::fs::remove_dir_all(&dir).ok();
    }

    fn float_spec(channels: u16, sample_rate: u32) -> hound::WavSpec {
        hound::WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        }
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn alinear_deja_pasar_lo_que_esta_dentro_de_la_tolerancia() {
        assert_eq!(align(1000, 1000, 10, 100), Align::Keep);
        assert_eq!(align(1000, 1010, 10, 100), Align::Keep);
        assert_eq!(align(1010, 1000, 10, 100), Align::Keep);
        assert_eq!(align(1000, 1011, 10, 100), Align::Pad(11));
        assert_eq!(align(1011, 1000, 10, 100), Align::Skip(11));
        // Adelantada de más: reloj roto, no se tira audio.
        assert_eq!(align(1200, 1000, 10, 100), Align::Keep);
    }

    #[test]
    fn cuadros_por_instante() {
        assert_eq!(frames_at(ms(1000), 48_000), 48_000);
        assert_eq!(frames_at(ms(600), 48_000), 28_800);
        assert_eq!(frames_at(ms(10), 44_100), 441);
        assert_eq!(frames_at(Duration::from_secs(3 * 3600), 48_000), 518_400_000);
    }

    /// Regresión: el audio del sistema (ScreenCaptureKit) arrancaba ~0,6 s
    /// después del micrófono y su WAV empezaba en su propio cero: los
    /// «otros» quedaban adelantados respecto de «yo» toda la reunión.
    #[test]
    fn la_pista_que_arranca_tarde_parte_con_silencio_exacto() {
        let mut t = Timeline::new(48_000, 2);
        // Primer bloque 3 ms tarde: aun dentro de la tolerancia, se respeta.
        assert_eq!(t.place(ms(3), 960), (144 * 2, 0));
        let mut t = Timeline::new(48_000, 2);
        assert_eq!(t.place(ms(600), 960), (28_800 * 2, 0));
        assert_eq!(t.frames, 28_800 + 480);
        // El siguiente, contiguo: va tal cual.
        assert_eq!(t.place(ms(610), 960), (0, 0));
    }

    /// Regresión: cuando no suena nada ScreenCaptureKit (y el loopback de
    /// WASAPI) no entrega bloques; sin rellenar, la pista se encogía.
    #[test]
    fn los_huecos_se_rellenan_y_el_jitter_no() {
        let mut t = Timeline::new(48_000, 1);
        assert_eq!(t.place(Duration::ZERO, 480), (0, 0));
        // 15 ms de jitter: no se toca.
        assert_eq!(t.place(ms(25), 480), (0, 0));
        assert_eq!(t.frames, 960);
        // Cinco segundos sin bloques.
        let (silence, skip) = t.place(ms(5_020), 480);
        assert_eq!((silence, skip), (5_020 * 48 - 960, 0));
        assert_eq!(t.frames, 5_020 * 48 + 480);
    }

    #[test]
    fn la_deriva_hacia_adelante_se_recorta() {
        let mut t = Timeline::new(48_000, 2);
        assert_eq!(t.place(Duration::ZERO, 48_000 * 2), (0, 0));
        // El dispositivo entregó un segundo pero el reloj dice que pasaron
        // 950 ms: 50 ms adelantada, sobre la tolerancia.
        assert_eq!(t.place(ms(950), 4_800 * 2), (0, 2_400 * 2));
        assert_eq!(t.frames, 48_000 + 2_400);
        // Un bloque entero dentro de lo adelantado se salta completo.
        let mut t = Timeline::new(48_000, 1);
        t.place(Duration::ZERO, 48_000);
        assert_eq!(t.place(ms(900), 480), (0, 480));
        assert_eq!(t.frames, 48_000);
    }

    #[test]
    fn al_cerrar_las_pistas_quedan_del_mismo_largo() {
        let end = ms(10_000);
        let mut mic = Timeline::new(48_000, 1);
        mic.place(Duration::ZERO, 48_000 * 9);
        let mut sys = Timeline::new(48_000, 2);
        sys.place(ms(700), 48_000 * 2);
        assert_eq!(mic.pad_to(end), 48_000);
        assert_eq!(sys.pad_to(end), (480_000 - 33_600 - 48_000) * 2);
        assert_eq!(mic.frames, sys.frames);
        // Ya completa: no agrega nada.
        assert_eq!(mic.pad_to(end), 0);
    }

    #[test]
    fn el_reloj_sigue_al_hardware_y_se_reancla_si_se_aparta() {
        let mut clock = Clock::new();
        let hw = |n: u64| Some(Duration::from_millis(1_000_000 + n));
        // Primer bloque: manda la llegada.
        assert_eq!(clock.stamp(hw(0), ms(120)), ms(120));
        // Después, el hardware (la llegada trae jitter).
        assert_eq!(clock.stamp(hw(10), ms(137)), ms(130));
        // Un hueco de 3 s se ve en el hardware y en la llegada.
        assert_eq!(clock.stamp(hw(3_010), ms(3_134)), ms(3_130));
        // El hardware salta lejos de la hora real: se reancla.
        assert_eq!(clock.stamp(hw(90_000), ms(3_150)), ms(3_150));
        assert_eq!(clock.stamp(hw(90_010), ms(3_161)), ms(3_160));
        // Sin instante del hardware, la llegada.
        let mut clock = Clock::new();
        assert_eq!(clock.stamp(None, ms(40)), ms(40));
        assert_eq!(clock.stamp(None, ms(52)), ms(52));
    }

    /// Las dos pistas de una sesión, de punta a punta: el sistema entra tarde
    /// y con un hueco, el micrófono deja de llegar antes de detener, y el
    /// canal de audio se corta antes de pedir el cierre (como al soltar el
    /// stream). Ambos WAV deben durar lo mismo y el audio caer en su lugar.
    #[test]
    fn los_wav_de_una_sesion_quedan_alineados() {
        let dir = std::env::temp_dir().join(format!("atic-audio-sync-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mic_path = dir.join("mic.wav");
        let sys_path = dir.join("system.wav");

        let (mic_tx, mic_rx) = mpsc::channel::<TimedBuffer>();
        let mic = spawn_writer(
            mic_path.clone(),
            float_spec(1, 48_000),
            48_000,
            1,
            Stage::Plain,
            mic_rx,
            CaptureTrack::Mic,
        );
        let (sys_tx, sys_rx) = mpsc::channel::<TimedBuffer>();
        let sys = spawn_writer(
            sys_path.clone(),
            float_spec(2, 48_000),
            48_000,
            2,
            Stage::Plain,
            sys_rx,
            CaptureTrack::System,
        );

        for k in 0..100u64 {
            mic_tx
                .send(TimedBuffer { at: ms(20 + k * 10), samples: vec![0.25; 480] })
                .unwrap();
        }
        // Sistema: arranca a los 600 ms, 200 ms de audio, 1 s callado, 200 ms más.
        for k in 0..20u64 {
            sys_tx
                .send(TimedBuffer { at: ms(600 + k * 10), samples: vec![0.5; 960] })
                .unwrap();
        }
        for k in 0..20u64 {
            sys_tx
                .send(TimedBuffer { at: ms(1_800 + k * 10), samples: vec![0.5; 960] })
                .unwrap();
        }
        drop(mic_tx);
        drop(sys_tx);
        let end = ms(2_500);
        assert!(mic.finish(Some(end)));
        assert!(sys.finish(Some(end)));

        let mic_reader = hound::WavReader::open(&mic_path).unwrap();
        let sys_reader = hound::WavReader::open(&sys_path).unwrap();
        assert_eq!(mic_reader.duration(), 120_000);
        assert_eq!(sys_reader.duration(), 120_000);

        let sys_samples: Vec<f32> =
            sys_reader.into_samples::<f32>().map(Result::unwrap).collect();
        let at = |ms: usize| sys_samples[ms * 48 * 2];
        assert_eq!(at(599), 0.0);
        assert_eq!(at(600), 0.5);
        assert_eq!(at(799), 0.5);
        assert_eq!(at(800), 0.0);
        assert_eq!(at(1_799), 0.0);
        assert_eq!(at(1_800), 0.5);
        assert_eq!(at(2_000), 0.0);
        let mic_samples: Vec<f32> =
            mic_reader.into_samples::<f32>().map(Result::unwrap).collect();
        assert_eq!(mic_samples[19 * 48], 0.0);
        assert_eq!(mic_samples[20 * 48], 0.25);
        assert_eq!(mic_samples[1_020 * 48 - 1], 0.25);
        assert_eq!(mic_samples[1_020 * 48], 0.0);
        std::fs::remove_dir_all(&dir).ok();
    }
}
