use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Preferencias del usuario, persistidas como JSON en el directorio de datos.
///
/// Los secretos (API keys, contraseña SMTP) NUNCA se guardan aquí; van al
/// llavero del sistema operativo. Este archivo solo contiene preferencias.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Idioma de transcripción: "auto" o un código ISO ("es", "en", ...).
    pub language: String,
    /// Modelo Whisper para reuniones / transcripción de grabaciones.
    pub whisper_model: String,
    /// Modelo Whisper para dictado (latencia; suele ser más pequeño).
    pub dictation_whisper_model: String,
    /// Motor de dictado: `local` | `groq`.
    pub dictation_backend: String,
    /// Modelo Groq Whisper para dictado (`whisper-large-v3-turbo`, …).
    pub dictation_groq_model: String,
    /// Transcribir automáticamente los WAV completos al terminar una grabación.
    pub auto_transcribe_after_recording: bool,
    /// Vista previa experimental durante la grabación. Nunca es el transcript final.
    pub live_transcription: bool,
    /// Motor de live: `local` | `groq` (BYOK).
    pub live_engine: String,
    /// Modelo Whisper dedicado a live (catálogo local).
    pub live_whisper_model: String,
    /// Modelo Groq Whisper para live.
    pub live_groq_model: String,
    /// Backend de resumen: id de proveedor (`claude`, `ollama`, `openai`, …).
    pub summary_backend: String,
    /// Modelo activo del backend de resumen.
    pub summary_model: String,
    /// URL base del backend (Ollama / OpenAI-compat / Custom). Claude la ignora.
    pub summary_base_url: String,
    /// Backend de correo: "mailto" | "smtp".
    pub mail_backend: String,
    /// Host SMTP (solo si `mail_backend` es smtp).
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_username: String,
    /// Remitente visible en el correo.
    pub smtp_from: String,
    /// Usar STARTTLS (puerto 587) o TLS implícito (465).
    pub smtp_use_tls: bool,
    /// Atajo global para iniciar/detener grabación.
    pub global_shortcut: String,
    /// Atajo global para dictado (mic → texto → pegar).
    pub dictation_shortcut: String,
    /// Atajo global para traer la pill al cursor (animación).
    pub summon_pill_shortcut: String,
    /// Atajo global: abrir la rueda de herramientas en la pill.
    pub pill_radial_shortcut: String,
    /// Atajo global: traer pill + abrir historial de clipboard.
    pub clipboard_shortcut: String,
    /// Atajo global: traer pill + abrir panel de fragmentos.
    pub snippets_shortcut: String,
    /// Modo de dictado: `toggle` | `push_to_talk`.
    pub dictation_mode: String,
    /// Micrófono preferido (ID WASAPI o nombre legacy). Vacío = default del SO.
    pub mic_device_id: String,
    /// Micrófono exclusivo para dictado. Vacío = reutiliza `mic_device_id`.
    pub dictation_mic_device_id: String,
    /// Salida preferida (ID WASAPI o nombre legacy). Vacío = default del SO.
    /// Se usa para loopback («Otros») y para el beep de consentimiento.
    pub output_device_id: String,
    /// Mostrar la pill flotante.
    pub show_pill: bool,
    /// Última posición conocida de la pill (x, y) en coordenadas de pantalla.
    pub pill_position: Option<(f64, f64)>,
    /// Tamaño al que dejaste la burbuja de agentes, en píxeles lógicos.
    ///
    /// Es la VENTANA, marco de sombra incluido, que es lo mismo que mide
    /// `BubbleShape`. `None` = nunca la redimensionaste y vale la de diseño.
    pub agents_bubble_size: Option<(i32, i32)>,
    /// Sonido grave al iniciar/detener grabación (aviso de consentimiento).
    pub beep_on_start: bool,
    /// Toques graves de interfaz (capturas, dictado). Interruptor maestro.
    pub ui_sounds: bool,
    /// Timbre por acción: `grave` | `suave` | `cristal` | `madera` | `ninguno`.
    /// Vacío = el timbre por defecto de esa acción.
    pub sound_recording_start: String,
    pub sound_recording_stop: String,
    pub sound_dictation_start: String,
    pub sound_dictation_done: String,
    pub sound_capture: String,
    /// Timbre al pasar de herramienta en la rueda de la pill.
    pub sound_wheel_tick: String,

    /// Servidores MCP que Atic le suma al agente, como JSON serializado.
    ///
    /// Se guarda crudo porque el formato lo define cada servidor y esta capa no
    /// tiene por qué conocerlo: solo lo transporta hasta `--mcp-config`.
    pub agent_mcp_servers: String,
    /// Pistas a grabar: `both` | `mic` | `system`.
    pub record_tracks: String,
    /// Pistas a transcribir: `both` | `mic` | `system`.
    pub transcribe_tracks: String,
    /// Modo parlantes: prioriza solo audio del sistema (evita eco del mic).
    pub speakers_mode: bool,
    /// Supresión de ruido en mic: `off` | `low` | `medium` | `high`.
    /// Desactivada por defecto (`off`) para no alterar el audio sin aviso.
    pub noise_suppression: String,
    /// Arrancar Atic con el sistema (bandeja).
    pub autostart: bool,
    /// El usuario ya completó el onboarding de primer uso.
    pub onboarding_done: bool,
    /// Días que se conservan las grabaciones. `0` = sin vencimiento.
    pub retention_days: u32,
    /// Ejecutar la limpieza configurada al iniciar la aplicación.
    pub retention_auto_cleanup: bool,
    /// Detectar ventanas de reuniones y ofrecer iniciar una grabación.
    pub detect_meetings: bool,
    /// Guardar en disco lo que pasa por el portapapeles.
    ///
    /// Apagado, el panel sigue existiendo pero queda vacío: no hay vigilante
    /// que mire el portapapeles ni `history.json` que crezca. Es opt-out y no
    /// opt-in porque el historial es la razón por la que existe el panel, pero
    /// tiene que poder apagarse: lo copiado incluye lo que el usuario nunca
    /// eligió archivar.
    pub clipboard_history: bool,
    /// Atajo global para abrir el overlay de selección de captura.
    pub screenshot_shortcut: String,
    /// Atajo global para el launcher tipo Spotlight.
    pub launcher_shortcut: String,
    /// Lado del shelf de capturas: `right` | `left`.
    pub capture_shelf_side: String,
    /// Segundos sin interacción antes de que el shelf se retraiga.
    pub capture_shelf_timeout_seconds: u32,
    /// Horas que se conservan las capturas antes de borrarse. `0` = sin límite.
    pub capture_retention_hours: u32,
    /// Incluir el cursor del sistema en las capturas.
    pub capture_include_cursor: bool,
    /// Acción al hacer clic en la miniatura: `preview` (abrir imagen) |
    /// `location` (abrir carpeta).
    pub capture_click_action: String,
    /// Tema de interfaz: `light` | `dark` | `system`.
    pub ui_theme: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            // Español por defecto: con "auto", Whisper suele confundir audio
            // ruidoso/corto y inventar inglés basura.
            language: "es".to_string(),
            // Base permite que la primera instalación sea local, rápida y
            // liviana: dictado y reuniones comparten una sola descarga.
            // Small/Medium siguen disponibles desde Ajustes si se privilegia
            // precisión por sobre velocidad y memoria.
            whisper_model: "base".to_string(),
            // Dictado: Base prioriza latencia en frases cortas.
            dictation_whisper_model: "base".to_string(),
            // Groq por defecto cuando hay clave BYOK (más rápido en notebook).
            dictation_backend: "groq".to_string(),
            dictation_groq_model: "whisper-large-v3-turbo".to_string(),
            auto_transcribe_after_recording: true,
            live_transcription: false,
            live_engine: "local".to_string(),
            live_whisper_model: "small".to_string(),
            live_groq_model: "whisper-large-v3-turbo".to_string(),
            summary_backend: "claude".to_string(),
            summary_model: "claude-opus-4-8".to_string(),
            summary_base_url: String::new(),
            mail_backend: "mailto".to_string(),
            smtp_host: String::new(),
            smtp_port: 587,
            smtp_username: String::new(),
            smtp_from: String::new(),
            smtp_use_tls: true,
            global_shortcut: "CmdOrCtrl+Shift+R".to_string(),
            dictation_shortcut: "CmdOrCtrl+Shift+D".to_string(),
            summon_pill_shortcut: "CmdOrCtrl+Shift+P".to_string(),
            // Alt+Space a secas es del SO (menú de ventana de cualquier app):
            // registrarlo global lo mataba en todo Windows. Alt+Z se sostiene
            // con la izquierda y deja el mouse libre para la rueda.
            pill_radial_shortcut: "Alt+Z".to_string(),
            clipboard_shortcut: "CmdOrCtrl+Shift+V".to_string(),
            snippets_shortcut: "CmdOrCtrl+Shift+S".to_string(),
            dictation_mode: "push_to_talk".to_string(),
            mic_device_id: String::new(),
            dictation_mic_device_id: String::new(),
            output_device_id: String::new(),
            show_pill: true,
            pill_position: None,
            agents_bubble_size: None,
            beep_on_start: false,
            ui_sounds: true,
            sound_recording_start: String::new(),
            sound_recording_stop: String::new(),
            sound_dictation_start: String::new(),
            sound_dictation_done: String::new(),
            sound_capture: String::new(),
            sound_wheel_tick: String::new(),
            agent_mcp_servers: String::new(),
            record_tracks: "both".to_string(),
            transcribe_tracks: "both".to_string(),
            speakers_mode: false,
            noise_suppression: "off".to_string(),
            autostart: false,
            onboarding_done: false,
            retention_days: 0,
            retention_auto_cleanup: false,
            detect_meetings: false,
            clipboard_history: true,
            screenshot_shortcut: "CmdOrCtrl+Shift+4".to_string(),
            launcher_shortcut: "CmdOrCtrl+Space".to_string(),
            capture_shelf_side: "right".to_string(),
            capture_shelf_timeout_seconds: 20,
            capture_retention_hours: 24,
            capture_include_cursor: false,
            capture_click_action: "preview".to_string(),
            ui_theme: "system".to_string(),
        }
    }
}

/// Forma intermedia para migrar configs antiguas (`claude_model` / `ollama_*`).
#[derive(Debug, Deserialize)]
#[serde(default)]
struct ConfigFile {
    language: String,
    whisper_model: String,
    dictation_whisper_model: Option<String>,
    dictation_backend: Option<String>,
    dictation_groq_model: Option<String>,
    auto_transcribe_after_recording: Option<bool>,
    live_transcription: Option<bool>,
    live_engine: Option<String>,
    live_whisper_model: Option<String>,
    live_groq_model: Option<String>,
    summary_backend: String,
    summary_model: Option<String>,
    summary_base_url: Option<String>,
    /// Legacy.
    claude_model: Option<String>,
    ollama_base_url: Option<String>,
    ollama_model: Option<String>,
    mail_backend: String,
    smtp_host: String,
    smtp_port: u16,
    smtp_username: String,
    smtp_from: String,
    smtp_use_tls: bool,
    global_shortcut: String,
    dictation_shortcut: Option<String>,
    summon_pill_shortcut: Option<String>,
    pill_radial_shortcut: Option<String>,
    clipboard_shortcut: Option<String>,
    snippets_shortcut: Option<String>,
    dictation_mode: Option<String>,
    mic_device_id: Option<String>,
    dictation_mic_device_id: Option<String>,
    output_device_id: Option<String>,
    show_pill: bool,
    pill_position: Option<(f64, f64)>,
    agents_bubble_size: Option<(i32, i32)>,
    beep_on_start: bool,
    ui_sounds: Option<bool>,
    sound_recording_start: Option<String>,
    sound_recording_stop: Option<String>,
    sound_dictation_start: Option<String>,
    sound_dictation_done: Option<String>,
    sound_capture: Option<String>,
    sound_wheel_tick: Option<String>,
    agent_mcp_servers: Option<String>,
    record_tracks: Option<String>,
    transcribe_tracks: Option<String>,
    speakers_mode: Option<bool>,
    /// Acepta bool legacy (`true`/`false`) o string (`off`|`low`|`medium`|`high`).
    #[serde(default, deserialize_with = "deserialize_noise_suppression")]
    noise_suppression: Option<String>,
    autostart: Option<bool>,
    onboarding_done: Option<bool>,
    retention_days: Option<u32>,
    retention_auto_cleanup: Option<bool>,
    detect_meetings: Option<bool>,
    clipboard_history: Option<bool>,
    screenshot_shortcut: Option<String>,
    launcher_shortcut: Option<String>,
    capture_shelf_side: Option<String>,
    capture_shelf_timeout_seconds: Option<u32>,
    capture_retention_hours: Option<u32>,
    capture_include_cursor: Option<bool>,
    capture_click_action: Option<String>,
    ui_theme: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum NoiseSuppressionRaw {
    Bool(bool),
    Str(String),
}

/// Migra `noise_suppression` bool → string y valida niveles conocidos.
fn deserialize_noise_suppression<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<NoiseSuppressionRaw>::deserialize(deserializer)?;
    Ok(opt.map(|raw| match raw {
        // Legacy: true → medium (filtro agresivo anterior), false → off.
        NoiseSuppressionRaw::Bool(true) => "medium".into(),
        NoiseSuppressionRaw::Bool(false) => "off".into(),
        NoiseSuppressionRaw::Str(s) => normalize_noise_level(&s),
    }))
}

fn normalize_noise_level(v: &str) -> String {
    match v {
        "off" | "low" | "medium" | "high" => v.to_string(),
        _ => "off".to_string(),
    }
}

impl Default for ConfigFile {
    fn default() -> Self {
        let d = Config::default();
        Self {
            language: d.language,
            whisper_model: d.whisper_model,
            dictation_whisper_model: None,
            dictation_backend: None,
            dictation_groq_model: None,
            auto_transcribe_after_recording: None,
            live_transcription: None,
            live_engine: None,
            live_whisper_model: None,
            live_groq_model: None,
            summary_backend: d.summary_backend,
            summary_model: None,
            summary_base_url: None,
            claude_model: None,
            ollama_base_url: None,
            ollama_model: None,
            mail_backend: d.mail_backend,
            smtp_host: d.smtp_host,
            smtp_port: d.smtp_port,
            smtp_username: d.smtp_username,
            smtp_from: d.smtp_from,
            smtp_use_tls: d.smtp_use_tls,
            global_shortcut: d.global_shortcut,
            dictation_shortcut: None,
            summon_pill_shortcut: None,
            pill_radial_shortcut: None,
            clipboard_shortcut: None,
            snippets_shortcut: None,
            dictation_mode: None,
            mic_device_id: None,
            dictation_mic_device_id: None,
            output_device_id: None,
            show_pill: d.show_pill,
            pill_position: d.pill_position,
            agents_bubble_size: d.agents_bubble_size,
            beep_on_start: d.beep_on_start,
            ui_sounds: None,
            sound_recording_start: None,
            sound_recording_stop: None,
            sound_dictation_start: None,
            sound_dictation_done: None,
            sound_capture: None,
            sound_wheel_tick: None,
            agent_mcp_servers: None,
            record_tracks: None,
            transcribe_tracks: None,
            speakers_mode: None,
            noise_suppression: None,
            autostart: None,
            onboarding_done: None,
            retention_days: None,
            retention_auto_cleanup: None,
            detect_meetings: None,
            clipboard_history: None,
            screenshot_shortcut: None,
            launcher_shortcut: None,
            capture_shelf_side: None,
            capture_shelf_timeout_seconds: None,
            capture_retention_hours: None,
            capture_include_cursor: None,
            capture_click_action: None,
            ui_theme: None,
        }
    }
}

impl From<ConfigFile> for Config {
    fn from(f: ConfigFile) -> Self {
        // La incorporación de este campo marca la nueva separación entre preview
        // y transcript final. Las configs anteriores podían tener live=true por
        // defecto, así que se apaga una vez hasta que el usuario lo vuelva a elegir.
        let migrating_to_batch_default = f.auto_transcribe_after_recording.is_none();
        let backend = f.summary_backend;
        let summary_model = f
            .summary_model
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| match backend.as_str() {
                "ollama" => f
                    .ollama_model
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "llama3.2".into()),
                "openai" => "gpt-4.1-mini".into(),
                "openrouter" => "openai/gpt-4.1-mini".into(),
                "groq" => "llama-3.3-70b-versatile".into(),
                "minimax" => "MiniMax-M3".into(),
                "custom" => String::new(),
                _ => f
                    .claude_model
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "claude-opus-4-8".into()),
            });
        let summary_base_url = f
            .summary_base_url
            .filter(|s| !s.is_empty())
            .or_else(|| {
                if backend == "ollama" {
                    f.ollama_base_url.filter(|s| !s.is_empty())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| match backend.as_str() {
                "ollama" => "http://127.0.0.1:11434".into(),
                "openai" => "https://api.openai.com/v1".into(),
                "openrouter" => "https://openrouter.ai/api/v1".into(),
                "groq" => "https://api.groq.com/openai/v1".into(),
                "minimax" => "https://api.minimax.io/v1".into(),
                _ => String::new(),
            });

        Config {
            language: f.language,
            whisper_model: f.whisper_model,
            // Configs antiguas sin el campo: dictado usa Base (rápido).
            dictation_whisper_model: f
                .dictation_whisper_model
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "base".into()),
            dictation_backend: match f.dictation_backend.as_deref() {
                Some("local") => "local".into(),
                // Default interno: Groq (también configs antiguas sin el campo).
                _ => "groq".into(),
            },
            dictation_groq_model: f
                .dictation_groq_model
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "whisper-large-v3-turbo".into()),
            auto_transcribe_after_recording: f.auto_transcribe_after_recording.unwrap_or(true),
            live_transcription: if migrating_to_batch_default {
                false
            } else {
                f.live_transcription.unwrap_or(false)
            },
            live_engine: match f.live_engine.as_deref() {
                Some("groq") => "groq".into(),
                _ => "local".into(),
            },
            live_whisper_model: f
                .live_whisper_model
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "small".into()),
            live_groq_model: f
                .live_groq_model
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "whisper-large-v3-turbo".into()),
            summary_backend: backend,
            summary_model,
            summary_base_url,
            mail_backend: f.mail_backend,
            smtp_host: f.smtp_host,
            smtp_port: f.smtp_port,
            smtp_username: f.smtp_username,
            smtp_from: f.smtp_from,
            smtp_use_tls: f.smtp_use_tls,
            global_shortcut: f.global_shortcut,
            dictation_shortcut: f
                .dictation_shortcut
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "CmdOrCtrl+Shift+D".into()),
            summon_pill_shortcut: f
                .summon_pill_shortcut
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "CmdOrCtrl+Shift+P".into()),
            // "Alt+Space" fue el default hasta 0.2.0 y secuestraba el menú de
            // ventana del SO; se reescribe al nuevo para que el arreglo alcance
            // también a las instalaciones existentes.
            pill_radial_shortcut: f
                .pill_radial_shortcut
                .filter(|s| !s.is_empty() && s != "Alt+Space")
                .unwrap_or_else(|| "Alt+Z".into()),
            clipboard_shortcut: f
                .clipboard_shortcut
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "CmdOrCtrl+Shift+V".into()),
            snippets_shortcut: f
                .snippets_shortcut
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "CmdOrCtrl+Shift+S".into()),
            dictation_mode: match f.dictation_mode.as_deref() {
                Some("toggle") => "toggle".into(),
                _ => "push_to_talk".into(),
            },
            mic_device_id: f.mic_device_id.unwrap_or_default(),
            dictation_mic_device_id: f.dictation_mic_device_id.unwrap_or_default(),
            output_device_id: f.output_device_id.unwrap_or_default(),
            show_pill: f.show_pill,
            pill_position: f.pill_position,
            agents_bubble_size: f.agents_bubble_size,
            beep_on_start: f.beep_on_start,
            // Configs antiguas: activar toques de UI (captura/dictado).
            ui_sounds: f.ui_sounds.unwrap_or(true),
            sound_recording_start: f.sound_recording_start.unwrap_or_default(),
            sound_recording_stop: f.sound_recording_stop.unwrap_or_default(),
            sound_dictation_start: f.sound_dictation_start.unwrap_or_default(),
            sound_dictation_done: f.sound_dictation_done.unwrap_or_default(),
            sound_capture: f.sound_capture.unwrap_or_default(),
            sound_wheel_tick: f.sound_wheel_tick.unwrap_or_default(),
            agent_mcp_servers: f.agent_mcp_servers.unwrap_or_default(),
            record_tracks: f.record_tracks.unwrap_or_else(|| "both".into()),
            transcribe_tracks: f.transcribe_tracks.unwrap_or_else(|| "both".into()),
            speakers_mode: f.speakers_mode.unwrap_or(false),
            noise_suppression: f.noise_suppression.unwrap_or_else(|| "off".into()),
            autostart: f.autostart.unwrap_or(false),
            // Configs antiguas: no volver a mostrar el wizard.
            onboarding_done: f.onboarding_done.unwrap_or(true),
            retention_days: f.retention_days.unwrap_or(0).min(3_650),
            retention_auto_cleanup: f.retention_auto_cleanup.unwrap_or(false),
            detect_meetings: f.detect_meetings.unwrap_or(false),
            // Ausente en configs viejas: ahí el historial ya venía andando, y
            // apagarlo de golpe en una actualización perdería lo guardado.
            clipboard_history: f.clipboard_history.unwrap_or(true),
            screenshot_shortcut: f
                .screenshot_shortcut
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "CmdOrCtrl+Shift+4".into()),
            launcher_shortcut: f
                .launcher_shortcut
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "CmdOrCtrl+Space".into()),
            capture_shelf_side: match f.capture_shelf_side.as_deref() {
                Some("left") => "left".into(),
                _ => "right".into(),
            },
            capture_shelf_timeout_seconds: f.capture_shelf_timeout_seconds.unwrap_or(20),
            capture_retention_hours: f.capture_retention_hours.unwrap_or(24),
            capture_include_cursor: f.capture_include_cursor.unwrap_or(false),
            capture_click_action: match f.capture_click_action.as_deref() {
                Some("location") => "location".into(),
                _ => "preview".into(),
            },
            ui_theme: match f.ui_theme.as_deref() {
                Some("light") => "light".into(),
                Some("dark") => "dark".into(),
                _ => "system".into(),
            },
        }
    }
}

impl Config {
    /// Qué pistas grabar tras aplicar `speakers_mode`.
    pub fn effective_record_tracks(&self) -> &str {
        if self.speakers_mode {
            "system"
        } else {
            match self.record_tracks.as_str() {
                "mic" | "system" => self.record_tracks.as_str(),
                _ => "both",
            }
        }
    }

    /// Qué pistas enviar a Whisper.
    pub fn effective_transcribe_tracks(&self) -> &str {
        if self.speakers_mode {
            return "system";
        }
        match self.transcribe_tracks.as_str() {
            "mic" | "system" => self.transcribe_tracks.as_str(),
            _ => "both",
        }
    }

    /// Carga la configuración desde disco; si no existe o está corrupta,
    /// devuelve los valores por defecto (sin fallar).
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str::<ConfigFile>(&text)
                .map(Config::from)
                .unwrap_or_else(|err| {
                    tracing::warn!(%err, "config.json inválido, usando valores por defecto");
                    Config::default()
                }),
            Err(_) => Config::default(),
        }
    }

    /// Persiste la configuración a disco (JSON con formato legible).
    ///
    /// Atómica a propósito: [`Config::load`] está escrito para no fallar ante
    /// un archivo roto —devuelve los valores por defecto—, así que una escritura
    /// truncada a mitad no se vería como un error sino como la configuración del
    /// usuario borrada, sin aviso.
    pub fn save(&self, path: &Path) -> Result<()> {
        let text = serde_json::to_string_pretty(self)?;
        crate::fs_atomic::write_atomic_str(path, &text)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_legacy_claude_fields() {
        let json = r#"{
            "language": "es",
            "whisper_model": "base",
            "summary_backend": "claude",
            "claude_model": "claude-sonnet-4-5",
            "ollama_base_url": "http://127.0.0.1:11434",
            "ollama_model": "llama3.2",
            "mail_backend": "mailto",
            "smtp_host": "",
            "smtp_port": 587,
            "smtp_username": "",
            "smtp_from": "",
            "smtp_use_tls": true,
            "global_shortcut": "CmdOrCtrl+Shift+R",
            "show_pill": true,
            "beep_on_start": false
        }"#;
        let cfg: Config = serde_json::from_str::<ConfigFile>(json).unwrap().into();
        assert_eq!(cfg.summary_model, "claude-sonnet-4-5");
        assert!(cfg.summary_base_url.is_empty());
        assert_eq!(cfg.dictation_mode, "push_to_talk");
        assert!(cfg.mic_device_id.is_empty());
        assert!(cfg.output_device_id.is_empty());
        assert_eq!(cfg.record_tracks, "both");
        assert!(cfg.ui_sounds);
    }

    #[test]
    fn speakers_mode_forces_system() {
        let cfg = Config {
            speakers_mode: true,
            ..Config::default()
        };
        assert_eq!(cfg.effective_record_tracks(), "system");
        assert_eq!(cfg.effective_transcribe_tracks(), "system");
    }

    #[test]
    fn default_language_is_spanish() {
        assert_eq!(Config::default().language, "es");
    }

    #[test]
    fn migrates_the_radial_shortcut_away_from_the_system_alt_space() {
        // Alt+Space es el menú de ventana del SO: registrarlo global lo mataba
        // en todas las apps, así que las configs viejas se reescriben.
        let json = r#"{ "pill_radial_shortcut": "Alt+Space" }"#;
        let cfg: Config = serde_json::from_str::<ConfigFile>(json).unwrap().into();
        assert_eq!(cfg.pill_radial_shortcut, "Alt+Z");
    }

    #[test]
    fn keeps_a_radial_shortcut_the_user_chose() {
        let json = r#"{ "pill_radial_shortcut": "CmdOrCtrl+Shift+Space" }"#;
        let cfg: Config = serde_json::from_str::<ConfigFile>(json).unwrap().into();
        assert_eq!(cfg.pill_radial_shortcut, "CmdOrCtrl+Shift+Space");
    }

    #[test]
    fn default_models_share_the_fast_local_model() {
        let cfg = Config::default();
        assert_eq!(cfg.whisper_model, "base");
        assert_eq!(cfg.dictation_whisper_model, "base");
        assert_eq!(cfg.dictation_backend, "groq");
    }

    #[test]
    fn migrates_missing_dictation_model_to_base() {
        let json = r#"{ "whisper_model": "small" }"#;
        let cfg: Config = serde_json::from_str::<ConfigFile>(json).unwrap().into();
        assert_eq!(cfg.whisper_model, "small");
        assert_eq!(cfg.dictation_whisper_model, "base");
    }

    #[test]
    fn keeps_the_clipboard_history_on_for_configs_without_the_field() {
        let json = r#"{ "language": "es" }"#;
        let cfg: Config = serde_json::from_str::<ConfigFile>(json).unwrap().into();
        assert!(cfg.clipboard_history);
    }

    #[test]
    fn respects_an_explicit_clipboard_history_opt_out() {
        let json = r#"{ "clipboard_history": false }"#;
        let cfg: Config = serde_json::from_str::<ConfigFile>(json).unwrap().into();
        assert!(!cfg.clipboard_history);
    }

    #[test]
    fn migrates_noise_suppression_bool_true() {
        let json = r#"{ "noise_suppression": true }"#;
        let cfg: Config = serde_json::from_str::<ConfigFile>(json).unwrap().into();
        assert_eq!(cfg.noise_suppression, "medium");
    }

    #[test]
    fn migrates_noise_suppression_bool_false() {
        let json = r#"{ "noise_suppression": false }"#;
        let cfg: Config = serde_json::from_str::<ConfigFile>(json).unwrap().into();
        assert_eq!(cfg.noise_suppression, "off");
    }

    #[test]
    fn accepts_noise_suppression_string() {
        let json = r#"{ "noise_suppression": "low" }"#;
        let cfg: Config = serde_json::from_str::<ConfigFile>(json).unwrap().into();
        assert_eq!(cfg.noise_suppression, "low");
    }

    #[test]
    fn defaults_to_batch_transcription_without_live_preview() {
        let cfg = Config::default();
        assert!(cfg.auto_transcribe_after_recording);
        assert!(!cfg.live_transcription);
        assert_eq!(cfg.live_engine, "local");
        assert_eq!(cfg.live_whisper_model, "small");
    }

    #[test]
    fn migrates_missing_live_fields() {
        let json = r#"{ "whisper_model": "base" }"#;
        let cfg: Config = serde_json::from_str::<ConfigFile>(json).unwrap().into();
        assert!(cfg.auto_transcribe_after_recording);
        assert!(!cfg.live_transcription);
        assert_eq!(cfg.live_engine, "local");
        assert_eq!(cfg.live_whisper_model, "small");
    }

    #[test]
    fn disables_the_old_live_default_during_batch_migration() {
        let json = r#"{ "live_transcription": true }"#;
        let cfg: Config = serde_json::from_str::<ConfigFile>(json).unwrap().into();
        assert!(cfg.auto_transcribe_after_recording);
        assert!(!cfg.live_transcription);
    }

    #[test]
    fn preserves_an_explicit_live_preview_choice_after_migration() {
        let json = r#"{
            "auto_transcribe_after_recording": true,
            "live_transcription": true
        }"#;
        let cfg: Config = serde_json::from_str::<ConfigFile>(json).unwrap().into();
        assert!(cfg.live_transcription);
    }

    #[test]
    fn migrates_legacy_ollama_fields() {
        let json = r#"{
            "summary_backend": "ollama",
            "ollama_base_url": "http://localhost:11434",
            "ollama_model": "mistral"
        }"#;
        let cfg: Config = serde_json::from_str::<ConfigFile>(json).unwrap().into();
        assert_eq!(cfg.summary_model, "mistral");
        assert_eq!(cfg.summary_base_url, "http://localhost:11434");
    }
}
