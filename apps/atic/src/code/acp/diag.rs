//! Lo que ayuda a entender por qué falló un agente: las últimas líneas que su CLI
//! escribió fuera del protocolo (su stderr, o un error suelto en stdout), la versión de
//! ACP que dice hablar y los mensajes que Atic no conoce.
//!
//! Esas líneas llegan al log y al aviso del chat, así que pasan antes por `redact`: el
//! stderr de una CLI puede traer un token o una clave de API.

use std::collections::{HashSet, VecDeque};

use serde_json::Value;

/// La versión de ACP que habla Atic (la que manda en `initialize`).
pub(super) const PROTOCOL_VERSION: u64 = 1;

/// Cuántas líneas se le muestran al usuario junto al error.
pub(super) const SHOWN_LINES: usize = 6;

const MAX_LINES: usize = 40;
const MAX_BYTES: usize = 8 * 1024;
const MAX_LINE_BYTES: usize = 400;
/// Tope de tipos desconocidos que se recuerdan: un agente que inventa nombres sin parar
/// no hace crecer la lista (ni el log).
const MAX_SEEN: usize = 200;
const MAX_NAME_BYTES: usize = 80;

const MASK: &str = "***";

/// El texto sin secuencias de escape ANSI (colores, títulos de ventana) ni otros
/// caracteres de control.
pub(super) fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => match chars.next() {
                // CSI: sus parámetros llegan hasta el byte final (0x40–0x7E).
                Some('[') => {
                    for c in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&c) {
                            break;
                        }
                    }
                }
                // OSC: hasta BEL o `ESC \`.
                Some(']') => {
                    while let Some(c) = chars.next() {
                        if c == '\u{7}' {
                            break;
                        }
                        if c == '\u{1b}' {
                            if chars.peek() == Some(&'\\') {
                                chars.next();
                            }
                            break;
                        }
                    }
                }
                _ => {}
            },
            '\t' => out.push(' '),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// Las palabras que anuncian un secreto. Tras ellas va `=` o `:` y el valor, salvo
/// `bearer`, que lo lleva después de un espacio.
const SECRET_WORDS: [&str; 7] = ["authorization", "password", "passwd", "secret", "bearer", "token", "key"];

/// Los esquemas de `Authorization`: el secreto es la palabra que les sigue.
const AUTH_SCHEMES: [&str; 3] = ["bearer", "basic", "token"];

/// Los prefijos de credenciales conocidas (OpenAI, GitHub, Slack, AWS, un JWT).
const SECRET_PREFIXES: [&str; 9] = ["sk-", "ghp_", "gho_", "ghs_", "github_pat_", "xoxb-", "xoxp-", "AKIA", "eyJ"];

fn ends_value(byte: u8) -> bool {
    byte.is_ascii_whitespace() || matches!(byte, b'"' | b'\'' | b',' | b';' | b'&' | b')' | b'}' | b']' | b'<' | b'>')
}

fn skip_spaces(bytes: &[u8], mut at: usize) -> usize {
    while bytes.get(at).is_some_and(|b| *b == b' ') {
        at += 1;
    }
    at
}

fn value_end(bytes: &[u8], mut at: usize) -> usize {
    while bytes.get(at).is_some_and(|b| !ends_value(*b)) {
        at += 1;
    }
    at
}

/// Tapa los valores que siguen a `key=`, `token:`, `Authorization: Bearer …` y parecidos.
fn mask_labelled(text: &str) -> String {
    // En minúsculas ASCII los bytes quedan en el mismo lugar: los índices valen para `text`.
    // Todos los cortes caen junto a un byte ASCII, así que nunca parten un carácter.
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut out = String::with_capacity(text.len());
    let (mut copied, mut i) = (0, 0);
    while i < bytes.len() {
        let Some(word) = SECRET_WORDS.iter().find(|word| bytes[i..].starts_with(word.as_bytes())) else {
            i += 1;
            continue;
        };
        let after = i + word.len();
        let mut start = if *word == "bearer" {
            let start = skip_spaces(bytes, after);
            if start == after {
                i = after;
                continue;
            }
            start
        } else {
            // `"api_key": "…"`: la comilla que cierra el nombre, el separador y la que abre el valor.
            let mut at = after;
            if matches!(bytes.get(at), Some(b'"' | b'\'')) {
                at += 1;
            }
            at = skip_spaces(bytes, at);
            if !matches!(bytes.get(at), Some(b'=' | b':')) {
                i = after;
                continue;
            }
            at = skip_spaces(bytes, at + 1);
            if matches!(bytes.get(at), Some(b'"' | b'\'')) {
                at += 1;
            }
            at
        };
        let mut end = value_end(bytes, start);
        if AUTH_SCHEMES.contains(&&lower[start..end]) && bytes.get(end) == Some(&b' ') {
            start = skip_spaces(bytes, end);
            end = value_end(bytes, start);
        }
        if end == start {
            i = after;
            continue;
        }
        out.push_str(&text[copied..start]);
        out.push_str(MASK);
        copied = end;
        i = end;
    }
    out.push_str(&text[copied..]);
    out
}

fn token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=' | b'_' | b'-')
}

/// Una tira larga de letras y números (hex, base64) o con el prefijo de una credencial
/// conocida. Una ruta (`/home/ana/proyecto1/src`) también es una tira así, pero con
/// muchas barras: esa se deja.
fn looks_secret(run: &str) -> bool {
    if run.len() >= 20 && SECRET_PREFIXES.iter().any(|prefix| run.starts_with(prefix)) {
        return true;
    }
    let digit = run.bytes().any(|b| b.is_ascii_digit());
    let letter = run.bytes().any(|b| b.is_ascii_alphabetic());
    let slashes = run.bytes().filter(|b| *b == b'/').count();
    run.len() >= 32 && digit && letter && slashes * 16 <= run.len()
}

fn mask_runs(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let (mut copied, mut i) = (0, 0);
    while i < bytes.len() {
        if !token_byte(bytes[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && token_byte(bytes[i]) {
            i += 1;
        }
        if looks_secret(&text[start..i]) {
            out.push_str(&text[copied..start]);
            out.push_str(MASK);
            copied = i;
        }
    }
    out.push_str(&text[copied..]);
    out
}

/// El texto sin lo que parece una credencial. Prefiere tapar de más: un hash de commit o
/// un UUID también se van.
pub(super) fn redact(text: &str) -> String {
    mask_runs(&mask_labelled(text))
}

/// Corta `text` a `max` bytes sin partir un carácter.
fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

/// Las últimas líneas que escribió el proceso de un agente, ya limpias y sin secretos,
/// con tope de líneas y de bytes.
#[derive(Default)]
pub(super) struct Tail {
    lines: VecDeque<String>,
    bytes: usize,
    /// Cuántas líneas se han guardado en total (las que ya salieron también).
    total: u64,
    /// El proceso cerró su stderr: no van a llegar más.
    closed: bool,
}

impl Tail {
    /// Guarda una línea. Devuelve lo que quedó guardado (para el log), o `None` si estaba vacía.
    pub(super) fn push(&mut self, raw: &str) -> Option<String> {
        let clean = redact(&strip_ansi(raw));
        let clean = clean.trim();
        if clean.is_empty() {
            return None;
        }
        let line = truncate(clean, MAX_LINE_BYTES);
        self.bytes += line.len();
        self.total += 1;
        self.lines.push_back(line.clone());
        while self.lines.len() > MAX_LINES || (self.bytes > MAX_BYTES && self.lines.len() > 1) {
            if let Some(old) = self.lines.pop_front() {
                self.bytes -= old.len();
            }
        }
        Some(line)
    }

    /// Una marca para pedir después solo lo que llegue desde ahora (`since`).
    pub(super) fn mark(&self) -> u64 {
        self.total
    }

    /// Las últimas `max` líneas de las que llegaron después de `mark`.
    pub(super) fn since(&self, mark: u64, max: usize) -> Vec<String> {
        let first = self.total - self.lines.len() as u64;
        let fresh: Vec<&String> = self.lines.iter().enumerate().filter(|(index, _)| first + *index as u64 >= mark).map(|(_, line)| line).collect();
        fresh[fresh.len().saturating_sub(max)..].iter().map(|line| line.to_string()).collect()
    }

    pub(super) fn close(&mut self) {
        self.closed = true;
    }

    pub(super) fn closed(&self) -> bool {
        self.closed
    }
}

/// El error con lo que se sabe del proceso debajo: con qué código salió, si habla un ACP
/// más nuevo que el de Atic y sus últimas líneas (las que el error ya dice no se repiten).
pub(super) fn with_details(error: String, exit: Option<i64>, newer_protocol: Option<u64>, lines: &[String]) -> String {
    let mut details = Vec::new();
    if let Some(code) = exit {
        details.push(format!("(exit {code})"));
    }
    if let Some(version) = newer_protocol {
        details.push(format!("(ACP v{version} > v{PROTOCOL_VERSION})"));
    }
    details.extend(lines.iter().filter(|line| !error.contains(line.as_str())).map(|line| format!("> {line}")));
    if details.is_empty() {
        return error;
    }
    format!("{error}\n\n{}", details.join("\n"))
}

/// La versión de ACP que el agente dice hablar en su respuesta a `initialize`. `Ok(None)`
/// si no la dice (se sigue igual); `Err` con qué está mal si la respuesta no sirve.
pub(super) fn protocol_version(init: &Value) -> Result<Option<u64>, &'static str> {
    let Some(fields) = init.as_object() else {
        return Err("initialize: invalid response");
    };
    let invalid = "initialize: invalid protocolVersion";
    match fields.get("protocolVersion") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => match (number.as_u64(), number.as_f64()) {
            (Some(version), _) => Ok(Some(version)),
            (None, Some(version)) if version >= 0.0 && version.fract() == 0.0 => Ok(Some(version as u64)),
            _ => Err(invalid),
        },
        Some(Value::String(text)) => text.trim().parse().map(Some).map_err(|_| invalid),
        Some(_) => Err(invalid),
    }
}

/// La versión del agente, si es más nueva que la de Atic.
pub(super) fn newer_protocol(version: Option<u64>) -> Option<u64> {
    version.filter(|version| *version > PROTOCOL_VERSION)
}

/// El texto habla de iniciar sesión (lo que una CLI escribe cuando no tiene cuenta).
pub(super) fn mentions_login(text: &str) -> bool {
    let text = text.to_lowercase();
    ["not logged in", "not authenticated", "unauthenticated", "unauthorized", "authentication required", "login required", "please log in", "please login", "sign in"]
        .iter()
        .any(|hint| text.contains(hint))
}

/// Los tipos de mensaje desconocidos de los que ya se avisó.
#[derive(Default)]
pub(super) struct Seen(HashSet<String>);

impl Seen {
    /// El nombre listo para el log si es la primera vez que se ve; `None` si ya se avisó
    /// (o si la lista se llenó).
    pub(super) fn first(&mut self, name: &str) -> Option<String> {
        let name = truncate(&redact(&strip_ansi(name)), MAX_NAME_BYTES);
        if self.0.len() >= MAX_SEEN || !self.0.insert(name.clone()) {
            return None;
        }
        Some(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn se_quitan_los_escapes_ansi() {
        assert_eq!(strip_ansi("\u{1b}[31;1merror\u{1b}[0m: sin sesión"), "error: sin sesión");
        assert_eq!(strip_ansi("\u{1b}]0;título\u{7}hola\u{1b}]8;;x\u{1b}\\!"), "hola!");
        assert_eq!(strip_ansi("a\tb\r\u{8}"), "a b");
        assert_eq!(strip_ansi("corta\u{1b}"), "corta");
        assert_eq!(strip_ansi("ñandú ✓"), "ñandú ✓");
    }

    #[test]
    fn no_sale_nada_que_parezca_una_credencial() {
        assert_eq!(redact("OPENAI_API_KEY=sk-abc123 listo"), "OPENAI_API_KEY=*** listo");
        assert_eq!(redact("url ?token=abc&x=1"), "url ?token=***&x=1");
        assert_eq!(redact(r#"{"api_key": "abc", "n": 1}"#), r#"{"api_key": "***", "n": 1}"#);
        assert_eq!(redact("Authorization: Bearer abc.def"), "Authorization: Bearer ***");
        assert_eq!(redact("authorization=Basic dXNlcjpwYXNz"), "authorization=Basic ***");
        assert_eq!(redact("header bearer abcdef fin"), "header bearer *** fin");
        assert_eq!(redact("password: hunter2, user: ana"), "password: ***, user: ana");
        // Tiras largas: hex, base64, un JWT y los prefijos conocidos.
        assert_eq!(redact("sha 0123456789abcdef0123456789abcdef listo"), "sha *** listo");
        assert_eq!(redact("id QWxhZGRpbjpvcGVuIHNlc2FtZQ1234567890+/== fin"), "id *** fin");
        assert_eq!(redact("usa sk-proj-AbCdEfGhIjKlMnOp ya"), "usa *** ya");
        assert_eq!(redact("ghp_abcdefghijklmnopqrstuvwxyz"), "***");
        // Lo demás queda como está.
        for plain in [
            "Error: not logged in. Run `codex login`",
            "max_tokens: 4096 (tokens usados)",
            "no se pudo abrir /home/ana/proyecto1/src/main/java/com/x1/App",
            "C:\\Users\\ana\\AppData\\Local\\opencode\\log2025.txt",
            "keyboard interrupt · ñandú",
        ] {
            assert_eq!(redact(plain), plain);
        }
    }

    #[test]
    fn la_cola_tiene_tope_de_lineas_y_de_bytes() {
        let mut tail = Tail::default();
        assert_eq!(tail.push("  \u{1b}[2m\u{1b}[0m "), None, "una línea vacía no se guarda");
        for n in 0..100 {
            tail.push(&format!("línea {n}"));
        }
        assert_eq!(tail.lines.len(), MAX_LINES);
        assert_eq!(tail.since(0, 2), vec!["línea 98", "línea 99"]);
        assert_eq!(tail.bytes, tail.lines.iter().map(String::len).sum::<usize>());

        // Líneas largas: cada una se corta y el total no pasa del tope de bytes.
        let mut tail = Tail::default();
        for _ in 0..100 {
            let stored = tail.push(&"ñ ".repeat(1000)).unwrap();
            assert!(stored.len() <= MAX_LINE_BYTES + '…'.len_utf8());
        }
        assert!(tail.bytes <= MAX_BYTES);
        assert!(tail.lines.len() < MAX_LINES);
        assert_eq!(tail.bytes, tail.lines.iter().map(String::len).sum::<usize>());
    }

    #[test]
    fn la_cola_guarda_limpio_y_sin_secretos() {
        let mut tail = Tail::default();
        let stored = tail.push("\u{1b}[31mERROR\u{1b}[0m token=abc123\r\n");
        assert_eq!(stored.as_deref(), Some("ERROR token=***"));
        assert_eq!(tail.since(0, SHOWN_LINES), vec!["ERROR token=***"]);
    }

    #[test]
    fn la_marca_separa_lo_de_este_turno() {
        let mut tail = Tail::default();
        tail.push("aviso viejo");
        let mark = tail.mark();
        assert!(tail.since(mark, SHOWN_LINES).is_empty());
        tail.push("error nuevo");
        assert_eq!(tail.since(mark, SHOWN_LINES), vec!["error nuevo"]);
        assert_eq!(tail.since(0, SHOWN_LINES).len(), 2);
        // La marca sigue valiendo cuando lo viejo ya salió de la cola.
        for n in 0..MAX_LINES * 2 {
            tail.push(&format!("más {n}"));
        }
        assert_eq!(tail.since(mark, usize::MAX).len(), MAX_LINES);
        assert!(!tail.closed());
        tail.close();
        assert!(tail.closed());
    }

    #[test]
    fn el_error_lleva_los_detalles_debajo() {
        assert_eq!(with_details("Se detuvo".into(), None, None, &[]), "Se detuvo");
        let lines = vec!["Error: not logged in".to_string(), "Run codex login".to_string()];
        assert_eq!(with_details("Se detuvo".into(), Some(1), None, &lines), "Se detuvo\n\n(exit 1)\n> Error: not logged in\n> Run codex login");
        assert_eq!(with_details("Falló".into(), None, Some(2), &[]), "Falló\n\n(ACP v2 > v1)");
        // Lo que el error ya dice no se repite.
        assert_eq!(with_details("OpenCode: Rate limit exceeded".into(), None, None, &["Rate limit exceeded".to_string()]), "OpenCode: Rate limit exceeded");
    }

    #[test]
    fn la_version_del_protocolo_se_valida_sin_exigirla() {
        assert_eq!(protocol_version(&json!({ "protocolVersion": 1, "agentCapabilities": {} })), Ok(Some(1)));
        assert_eq!(protocol_version(&json!({ "agentCapabilities": {} })), Ok(None), "un agente que no la dice sigue");
        assert_eq!(protocol_version(&json!({ "protocolVersion": null })), Ok(None));
        assert_eq!(protocol_version(&json!({ "protocolVersion": "2" })), Ok(Some(2)));
        assert_eq!(protocol_version(&json!({ "protocolVersion": 2.0 })), Ok(Some(2)));
        assert!(protocol_version(&json!({ "protocolVersion": "uno" })).is_err());
        assert!(protocol_version(&json!({ "protocolVersion": -1 })).is_err());
        assert!(protocol_version(&json!({ "protocolVersion": 1.5 })).is_err());
        assert!(protocol_version(&json!({ "protocolVersion": [1] })).is_err());
        assert!(protocol_version(&Value::Null).is_err());
        assert!(protocol_version(&json!("ok")).is_err());
        assert_eq!(newer_protocol(Some(2)), Some(2));
        assert_eq!(newer_protocol(Some(1)), None);
        assert_eq!(newer_protocol(Some(0)), None);
        assert_eq!(newer_protocol(None), None);
    }

    #[test]
    fn se_reconoce_cuando_falta_iniciar_sesion() {
        assert!(mentions_login("Error: Not logged in. Run `codex login`"));
        assert!(mentions_login("401 Unauthorized"));
        assert!(mentions_login("Please sign in to continue"));
        assert!(!mentions_login("Rate limit exceeded"));
        assert!(!mentions_login("loading author list"));
    }

    #[test]
    fn un_tipo_desconocido_se_avisa_una_vez() {
        let mut seen = Seen::default();
        assert_eq!(seen.first("usage_update").as_deref(), Some("usage_update"));
        assert_eq!(seen.first("usage_update"), None);
        assert_eq!(seen.first("otro").as_deref(), Some("otro"));
        assert!(seen.first(&"x".repeat(500)).unwrap().len() <= MAX_NAME_BYTES + '…'.len_utf8());
        for n in 0..MAX_SEEN * 2 {
            seen.first(&format!("tipo-{n}"));
        }
        assert_eq!(seen.0.len(), MAX_SEEN);
        assert_eq!(seen.first("uno más"), None, "con la lista llena ya no se avisa");
    }
}
