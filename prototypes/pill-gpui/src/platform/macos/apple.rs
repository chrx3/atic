//! Lo que la pill le pide a macOS en Swift: el reconocimiento de voz en el
//! equipo (SpeechAnalyzer) y Apple Intelligence (Foundation Models).
//!
//! Esas APIs no existen en Objective-C, así que van en un ayudante aparte,
//! `atic-apple` (`macos/apple/main.swift`), que viaja junto al ejecutable en la
//! `.app`. Se lanza por pedido y contesta un JSON por línea; los errores llegan
//! como `{"error": "..."}`. Todo lo de aquí bloquea: se llama desde un hilo.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

/// El reconocimiento de voz del idioma.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Speech {
    /// Listo para transcribir sin internet.
    Ready,
    /// El sistema está bajando el modelo del idioma.
    Downloading,
    /// Este Mac o este idioma no lo tienen.
    Unsupported,
}

/// Apple Intelligence, como lo informa `SystemLanguageModel.availability`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intelligence {
    Available,
    /// El Mac lo admite pero está apagado en Ajustes del Sistema.
    NotEnabled,
    /// Encendido, pero el modelo todavía se está bajando.
    NotReady,
    /// Este Mac no lo admite (chip o región).
    NotEligible,
    Unavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub speech: Speech,
    /// El idioma con que se transcribe (`es-CL`).
    pub locale: String,
    pub intelligence: Intelligence,
    /// Apple Intelligence entiende el idioma del sistema.
    pub intelligence_speaks: bool,
}

/// Una frase de una pista de Reuniones, con su tiempo desde el inicio.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Phrase {
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Transcribed {
    pub text: String,
    pub phrases: Vec<Phrase>,
}

/// Las palabras que el reconocimiento no conoce y conviene que espere.
pub const VOCABULARY: &[&str] = &[
    "Atic", "GPUI", "notch", "pill", "Claude", "Claude Code", "Codex", "Groq", "Tauri", "Rust", "commit", "push",
    "deploy", "prompt",
];

/// El ayudante: junto al ejecutable (en la `.app` o en el target de cargo), o
/// donde diga `ATIC_APPLE_PATH`.
pub fn helper() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("ATIC_APPLE_PATH").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(path);
    }
    let exe = std::env::current_exe().ok()?;
    let path = exe.parent()?.join("atic-apple");
    path.is_file().then_some(path)
}

/// ¿Está el ayudante? Sin él (una compilación vieja) no se ofrece nada de esto.
pub fn installed() -> bool {
    helper().is_some()
}

fn missing() -> String {
    "Falta el ayudante de Apple (atic-apple) junto a la pill: vuelve a compilarla con scripts/pill-dev-mac.sh.".into()
}

/// Corre un comando y devuelve las líneas JSON; la última con `error` es el fallo.
fn run(args: &[&str], input: Option<&str>, mut on_line: impl FnMut(&Value)) -> Result<Value, String> {
    let helper = helper().ok_or_else(missing)?;
    let mut child = Command::new(&helper)
        .args(args)
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("No se pudo abrir el ayudante de Apple: {e}"))?;
    if let (Some(text), Some(mut stdin)) = (input, child.stdin.take()) {
        stdin.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    }
    let mut last = Value::Null;
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let Ok(value) = serde_json::from_str::<Value>(&line) else { continue };
            on_line(&value);
            last = value;
        }
    }
    let _ = child.wait();
    reply(last)
}

fn reply(last: Value) -> Result<Value, String> {
    if let Some(error) = last.get("error").and_then(Value::as_str) {
        return Err(error.to_string());
    }
    if last.is_null() {
        return Err("El ayudante de Apple no respondió.".into());
    }
    Ok(last)
}

/// El idioma que se le pasa: el de la configuración («es»), o vacío para el del sistema.
fn lang_arg(language: Option<&str>) -> &str {
    language.filter(|l| !l.is_empty() && *l != "auto").unwrap_or("")
}

pub fn status(language: Option<&str>) -> Result<Status, String> {
    run(&["status", "--lang", lang_arg(language)], None, |_| {}).map(|v| parse_status(&v))
}

fn parse_status(value: &Value) -> Status {
    let speech = match value.pointer("/speech/status").and_then(Value::as_str) {
        Some("installed" | "supported") => Speech::Ready,
        Some("downloading") => Speech::Downloading,
        _ => Speech::Unsupported,
    };
    let intelligence = match value.pointer("/intelligence/state").and_then(Value::as_str) {
        Some("available") => Intelligence::Available,
        Some("appleIntelligenceNotEnabled") => Intelligence::NotEnabled,
        Some("modelNotReady") => Intelligence::NotReady,
        Some("deviceNotEligible") => Intelligence::NotEligible,
        _ => Intelligence::Unavailable,
    };
    Status {
        speech,
        locale: value.pointer("/speech/locale").and_then(Value::as_str).unwrap_or_default().to_string(),
        intelligence,
        intelligence_speaks: value.pointer("/intelligence/language").and_then(Value::as_bool).unwrap_or(false),
    }
}

/// Baja el modelo de voz del idioma; `progress` va de 0 a 1.
pub fn install(language: Option<&str>, mut progress: impl FnMut(f32)) -> Result<(), String> {
    run(&["install", "--lang", lang_arg(language)], None, |line| {
        if let Some(p) = line.get("progress").and_then(Value::as_f64) {
            progress(p.clamp(0.0, 1.0) as f32);
        }
    })
    .map(|_| ())
}

/// Transcribe un WAV en el equipo. Con `meeting`, además las frases con su tiempo.
pub fn transcribe(wav: &Path, language: Option<&str>, meeting: bool) -> Result<Transcribed, String> {
    let wav = wav.to_string_lossy();
    let context = VOCABULARY.join(",");
    let mode = if meeting { "meeting" } else { "dictation" };
    let args: [&str; 8] = ["transcribe", "--lang", lang_arg(language), "--mode", mode, "--context", context.as_str(), &*wav];
    run(&args, None, |_| {}).map(|v| parse_transcribed(&v))
}

fn parse_transcribed(value: &Value) -> Transcribed {
    let phrases = value
        .get("segments")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|s| {
                    let text = s.get("text")?.as_str()?.trim().to_string();
                    let start_ms = s.get("start_ms").and_then(Value::as_i64).unwrap_or(0);
                    let end_ms = s.get("end_ms").and_then(Value::as_i64).unwrap_or(start_ms).max(start_ms);
                    (!text.is_empty()).then_some(Phrase { start_ms, end_ms, text })
                })
                .collect()
        })
        .unwrap_or_default();
    Transcribed { text: value.get("text").and_then(Value::as_str).unwrap_or_default().trim().to_string(), phrases }
}

/// Una línea de `atic-apple stream`.
#[derive(Clone, Debug, PartialEq, Eq)]
enum StreamLine {
    /// El modelo está cargado.
    Ready,
    /// Lo confirmado hasta ahora y lo provisorio, que puede cambiar.
    Partial { confirmed: String, volatile: String },
    /// Todo el texto, al cerrarse la entrada.
    Done(String),
    Error(String),
    Other,
}

fn parse_stream_line(line: &str) -> StreamLine {
    let Ok(value) = serde_json::from_str::<Value>(line) else { return StreamLine::Other };
    let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_string);
    if let Some(error) = text("error") {
        return StreamLine::Error(error);
    }
    if let Some(done) = text("text") {
        return StreamLine::Done(done.trim().to_string());
    }
    if value.get("final").is_some() || value.get("volatile").is_some() {
        return StreamLine::Partial {
            confirmed: text("final").unwrap_or_default(),
            volatile: text("volatile").unwrap_or_default(),
        };
    }
    if value.get("ready").and_then(Value::as_bool) == Some(true) {
        return StreamLine::Ready;
    }
    StreamLine::Other
}

/// Mono a partir de muestras intercaladas (promedio de los canales).
pub fn downmix(samples: &[f32], channels: u16) -> Vec<f32> {
    let ch = channels.max(1) as usize;
    if ch == 1 {
        return samples.to_vec();
    }
    samples.chunks_exact(ch).map(|frame| frame.iter().sum::<f32>() / ch as f32).collect()
}

/// El dictado en vivo: `atic-apple stream` recibe el audio mientras se graba y
/// va contando lo que reconoce. El ayudante se abre antes que el micrófono para
/// que el modelo cargue mientras tanto; el primer `push` le dice la tasa.
///
/// Entrada del ayudante: u32 LE con la tasa y después f32 LE mono.
pub struct LiveStream {
    child: std::process::Child,
    stdin: Option<std::process::ChildStdin>,
    rate: Option<u32>,
    /// El ayudante dejó de recibir (murió o cerró el pipe): no se insiste.
    broken: bool,
    done: std::sync::mpsc::Receiver<Result<String, String>>,
}

impl LiveStream {
    /// Abre el ayudante. `on_partial(confirmado, provisorio)` se llama desde un
    /// hilo propio con cada resultado.
    pub fn start(language: Option<&str>, on_partial: impl Fn(&str, &str) + Send + 'static) -> Result<Self, String> {
        let helper = helper().ok_or_else(missing)?;
        let context = VOCABULARY.join(",");
        let mut child = Command::new(&helper)
            .args(["stream", "--lang", lang_arg(language), "--context", context.as_str()])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("No se pudo abrir el ayudante de Apple: {e}"))?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().ok_or("El ayudante de Apple no tiene salida.")?;
        let (tx, done) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("apple-voz".into())
            .spawn(move || {
                let mut result = Err("El ayudante de Apple no respondió.".to_string());
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    match parse_stream_line(&line) {
                        StreamLine::Partial { confirmed, volatile } => on_partial(&confirmed, &volatile),
                        StreamLine::Done(text) => result = Ok(text),
                        StreamLine::Error(error) => result = Err(error),
                        StreamLine::Ready | StreamLine::Other => {}
                    }
                }
                let _ = tx.send(result);
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { child, stdin, rate: None, broken: false, done })
    }

    /// Manda muestras mono. La tasa queda fija con el primer bloque; si cambia,
    /// el resto se descarta (el WAV sigue completo para el respaldo).
    pub fn push(&mut self, rate: u32, mono: &[f32]) {
        if self.broken || mono.is_empty() {
            return;
        }
        let Some(stdin) = self.stdin.as_mut() else { return };
        let mut bytes = Vec::with_capacity(4 + mono.len() * 4);
        match self.rate {
            None => {
                self.rate = Some(rate);
                bytes.extend_from_slice(&rate.to_le_bytes());
            }
            Some(first) if first != rate => {
                tracing::warn!(first, rate, "dictado en vivo: cambió la tasa del micrófono; dejo de mandar audio");
                self.broken = true;
                return;
            }
            Some(_) => {}
        }
        for sample in mono {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        if stdin.write_all(&bytes).is_err() {
            self.broken = true;
        }
    }

    /// ¿Llegó a recibir audio sin cortarse?
    pub fn healthy(&self) -> bool {
        self.rate.is_some() && !self.broken
    }

    /// Cierra la entrada y espera el texto final, a lo más `wait`.
    pub fn finish(mut self, wait: std::time::Duration) -> Result<String, String> {
        drop(self.stdin.take());
        let result = match self.done.recv_timeout(wait) {
            Ok(result) => result,
            Err(_) => {
                let _ = self.child.kill();
                Err("La voz de macOS no terminó a tiempo.".into())
            }
        };
        let _ = self.child.wait();
        result
    }
}

impl Drop for LiveStream {
    fn drop(&mut self) {
        // Si nadie llamó a `finish` (se descartó el dictado), no queda colgado.
        if self.stdin.take().is_some() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// Pule un texto dictado con Apple Intelligence. Si el modelo devuelve algo que
/// ya no es el mismo texto (lo respondió o lo resumió), queda el original.
pub fn polish(text: &str) -> Result<String, String> {
    let value = run(&["polish"], Some(text), |_| {})?;
    let polished = value.get("text").and_then(Value::as_str).unwrap_or_default().trim().to_string();
    Ok(if same_text(text, &polished) { polished } else { text.to_string() })
}

/// ¿Sigue siendo el mismo texto? Casi todas las palabras del original tienen que
/// seguir ahí (quitar muletillas está bien) y no puede crecer de más.
fn same_text(original: &str, polished: &str) -> bool {
    fn words(text: &str) -> Vec<String> {
        text.split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.chars().count() > 3)
            .map(|w| fold(&w.to_lowercase()))
            .collect()
    }
    fn fold(word: &str) -> String {
        word.chars()
            .map(|c| match c {
                'á' => 'a',
                'é' => 'e',
                'í' => 'i',
                'ó' => 'o',
                'ú' | 'ü' => 'u',
                other => other,
            })
            .collect()
    }
    if polished.is_empty() {
        return false;
    }
    let before = words(original);
    let after = words(polished);
    if before.is_empty() {
        return polished.chars().count() <= original.chars().count() * 2;
    }
    let kept = before.iter().filter(|w| after.contains(w)).count();
    kept * 100 >= before.len() * 85 && after.len() <= before.len() + before.len() / 4 + 2
}

/// Ajustes del Sistema en «Apple Intelligence y Siri».
pub fn open_intelligence_settings() {
    let _ = Command::new("open").arg("x-apple.systempreferences:com.apple.Siri-Settings.extension").spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_el_estado() {
        let value: Value = serde_json::from_str(
            r#"{"intelligence":{"language":true,"state":"appleIntelligenceNotEnabled"},"speech":{"locale":"es-CL","status":"supported"}}"#,
        )
        .unwrap();
        let status = parse_status(&value);
        assert_eq!(status.speech, Speech::Ready);
        assert_eq!(status.locale, "es-CL");
        assert_eq!(status.intelligence, Intelligence::NotEnabled);
        assert!(status.intelligence_speaks);
        let bare = parse_status(&serde_json::json!({}));
        assert_eq!((bare.speech, bare.intelligence), (Speech::Unsupported, Intelligence::Unavailable));
    }

    #[test]
    fn lee_las_frases_y_descarta_las_vacias() {
        let value = serde_json::json!({
            "text": " Hola. ",
            "segments": [
                {"start_ms": 1920, "end_ms": 7157, "text": "Hola."},
                {"start_ms": 8000, "end_ms": 7000, "text": "  "},
                {"start_ms": 9000, "end_ms": 8000, "text": "Chao."}
            ]
        });
        let t = parse_transcribed(&value);
        assert_eq!(t.text, "Hola.");
        assert_eq!(t.phrases.len(), 2);
        // Un fin anterior al inicio se lleva al inicio.
        assert_eq!((t.phrases[1].start_ms, t.phrases[1].end_ms), (9000, 9000));
    }

    #[test]
    fn el_error_del_ayudante_llega_como_error() {
        assert_eq!(reply(serde_json::json!({"error": "sin voz"})), Err("sin voz".into()));
        assert!(reply(Value::Null).is_err());
    }

    #[test]
    fn pulir_no_puede_cambiar_el_texto() {
        let dicho = "eh este mañana tenemos reunion a las diez con el equipo de diseño";
        assert!(same_text(dicho, "Mañana tenemos reunión a las diez con el equipo de diseño."));
        // Respondió la pregunta en vez de corregirla.
        assert!(!same_text("que hora es en santiago ahora", "Son las tres de la tarde en Santiago de Chile."));
        assert!(!same_text(dicho, ""));
        // Lo resumió.
        assert!(!same_text(
            "mañana tenemos reunion a las diez con el equipo de diseño y despues revisamos el notch y la pill",
            "Reunión mañana."
        ));
    }

    #[test]
    fn lee_las_lineas_del_dictado_en_vivo() {
        assert_eq!(parse_stream_line(r#"{"locale":"es-CL","rate":16000,"ready":true}"#), StreamLine::Ready);
        assert_eq!(
            parse_stream_line(r#"{"final":"Hola.","volatile":" esto es"}"#),
            StreamLine::Partial { confirmed: "Hola.".into(), volatile: " esto es".into() }
        );
        // Sin una de las dos llaves, la otra va vacía.
        assert_eq!(
            parse_stream_line(r#"{"volatile":"Hola"}"#),
            StreamLine::Partial { confirmed: String::new(), volatile: "Hola".into() }
        );
        assert_eq!(parse_stream_line(r#"{"locale":"es-CL","text":" Hola, esto es. "}"#), StreamLine::Done("Hola, esto es.".into()));
        assert_eq!(parse_stream_line(r#"{"locale":"es-CL","text":""}"#), StreamLine::Done(String::new()));
        assert_eq!(parse_stream_line(r#"{"error":"sin voz"}"#), StreamLine::Error("sin voz".into()));
        assert_eq!(parse_stream_line(r#"{"audio_ms":2}"#), StreamLine::Other);
        assert_eq!(parse_stream_line("basura"), StreamLine::Other);
    }

    #[test]
    fn baja_a_mono_promediando() {
        assert_eq!(downmix(&[0.5, -0.5, 1.0, 0.0], 2), vec![0.0, 0.5]);
        assert_eq!(downmix(&[0.25, 0.5], 1), vec![0.25, 0.5]);
        // Un bloque cortado a medio cuadro descarta lo que sobra.
        assert_eq!(downmix(&[1.0, 1.0, 1.0], 2), vec![1.0]);
    }

    #[test]
    fn idioma_automatico_es_el_del_sistema() {
        assert_eq!(lang_arg(None), "");
        assert_eq!(lang_arg(Some("auto")), "");
        assert_eq!(lang_arg(Some("es")), "es");
    }
}
