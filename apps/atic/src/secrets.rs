//! Lo que parece una clave se muestra incompleto: `sk-ab••••••xyz`. Pegarlo
//! sigue pegando el valor real; esto es solo lo que se ve en pantalla (alguien
//! mirando, una captura o compartir pantalla).
//!
//! Atic ya descarta lo que los gestores de contraseñas marcan como sensible
//! (`clipboard_is_sensitive`); esto cubre lo que se copia a mano.

const DOTS: &str = "••••••••";

/// Prefijos de tokens conocidos.
const PREFIXES: &[&str] = &[
    "sk-", "sk_live_", "sk_test_", "rk_live_", "pk_live_", "ghp_", "gho_", "ghu_", "ghs_",
    "ghr_", "github_pat_", "glpat-", "xoxb-", "xoxp-", "xoxa-", "xoxr-", "npm_", "hf_",
    "AKIA", "ASIA", "AIza", "ya29.", "SG.", "shpat_", "dop_v1_",
];

/// Nombres que anuncian que lo que sigue es un secreto: `password=…`,
/// `Contraseña: …`, `API_KEY …`.
const KEY_NAMES: &[&str] = &[
    "password", "passwd", "pwd", "pass", "secret", "token", "apikey", "api_key", "api-key",
    "access_key", "private_key", "client_secret", "auth", "bearer", "clave", "contraseña",
    "contrasena",
];

/// El texto tal como se muestra y si se ocultó algo.
pub fn mask(text: &str) -> (String, bool) {
    if text.contains("PRIVATE KEY-----") {
        return ("Llave privada ".to_string() + DOTS, true);
    }
    let mut out = String::with_capacity(text.len());
    let mut masked = false;
    // La palabra anterior anunciaba un secreto («Contraseña: hunter2»).
    let mut after_key = false;
    for piece in split_keep_space(text) {
        if piece.trim().is_empty() {
            out.push_str(piece);
            continue;
        }
        let (shown, hit, announces) = mask_word(piece, after_key);
        masked |= hit;
        after_key = announces;
        out.push_str(&shown);
    }
    (out, masked)
}

fn mask_word(word: &str, after_key: bool) -> (String, bool, bool) {
    // `clave=valor` o `clave: valor` en una sola palabra.
    if let Some(at) = word.find(['=', ':']) {
        let (key, rest) = word.split_at(at);
        let value = &rest[1..];
        if is_key_name(key) {
            if value.is_empty() {
                return (word.to_string(), false, true);
            }
            if looks_like_value(value) {
                return (format!("{key}{}{}", &rest[..1], hide(value)), true, false);
            }
        }
    }
    let bare = word.trim_end_matches([',', ';', '.', ')', '"', '\'']);
    if after_key && looks_like_value(bare) {
        return (word.replacen(bare, &hide(bare), 1), true, false);
    }
    if looks_like_token(bare) {
        return (word.replacen(bare, &hide(bare), 1), true, false);
    }
    let announces = is_key_name(word.trim_end_matches([':', '=']));
    (word.to_string(), false, announces)
}

fn is_key_name(word: &str) -> bool {
    let word = word
        .trim_matches(|ch: char| !ch.is_alphanumeric() && ch != '_' && ch != '-')
        .to_lowercase();
    // `OPENAI_API_KEY` termina en `_api_key`; `author` no es `auth`.
    !word.is_empty()
        && KEY_NAMES.iter().any(|name| {
            word == *name
                || word.ends_with(&format!("_{name}"))
                || word.ends_with(&format!("-{name}"))
        })
}

/// Un valor después de «contraseña:»: corto vale, pero no una palabra común
/// («pídela en recepción»). Pide un dígito, un símbolo o mayúsculas mezcladas.
fn looks_like_value(value: &str) -> bool {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() < 6 {
        return false;
    }
    let digit = chars.iter().any(char::is_ascii_digit);
    let symbol = chars.iter().any(|ch| !ch.is_alphanumeric());
    let upper = chars.iter().any(|ch| ch.is_uppercase());
    let lower = chars.iter().any(|ch| ch.is_lowercase());
    digit || symbol || (upper && lower)
}

/// Un token suelto: prefijo conocido, JWT o una tira larga y aleatoria.
fn looks_like_token(word: &str) -> bool {
    if word.len() < 16 {
        return false;
    }
    if PREFIXES.iter().any(|prefix| word.starts_with(prefix)) && word.len() >= 20 {
        return true;
    }
    if word.starts_with("eyJ") && word.matches('.').count() == 2 {
        return true;
    }
    // Rutas, URLs y correos no: tienen estructura, no azar.
    if word.contains(['/', '\\', '@']) || word.starts_with("http") {
        return false;
    }
    if word.len() < 24 {
        return false;
    }
    let token_chars = word
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '+' | '='));
    if !token_chars {
        return false;
    }
    let digit = word.chars().any(|ch| ch.is_ascii_digit());
    let letter = word.chars().any(|ch| ch.is_ascii_alphabetic());
    digit && letter && entropy(word) >= 3.5
}

/// Bits por carácter (Shannon).
fn entropy(word: &str) -> f32 {
    let mut counts = std::collections::HashMap::new();
    for ch in word.chars() {
        *counts.entry(ch).or_insert(0u32) += 1;
    }
    let len = word.chars().count() as f32;
    counts
        .values()
        .map(|&count| {
            let p = count as f32 / len;
            -p * p.log2()
        })
        .sum()
}

/// Los primeros 4 y los últimos 3, para reconocerlo sin poder leerlo.
fn hide(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= 10 {
        return DOTS.to_string();
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 3..].iter().collect();
    format!("{head}{DOTS}{tail}")
}

/// Parte el texto en palabras y espacios, sin perder los espacios.
fn split_keep_space(text: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut in_space = None;
    for (at, ch) in text.char_indices() {
        let space = ch.is_whitespace();
        if in_space.is_some_and(|was| was != space) {
            pieces.push(&text[start..at]);
            start = at;
        }
        in_space = Some(space);
    }
    if start < text.len() {
        pieces.push(&text[start..]);
    }
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hidden(text: &str) -> bool {
        mask(text).1
    }

    #[test]
    fn tokens_conocidos() {
        assert_eq!(
            mask("sk-ant-api03-abcdefghijklmnop1234").0,
            "sk-a••••••••234"
        );
        assert!(hidden("ghp_16C7e42F292c6912E7710c838347Ae178B4a"));
        assert!(hidden("AKIAIOSFODNN7EXAMPLE"));
        assert!(hidden("eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.dozjgNryP4J3jVmNHl0w5N"));
        assert!(hidden("-----BEGIN OPENSSH PRIVATE KEY-----\nabc"));
    }

    #[test]
    fn clave_valor() {
        assert_eq!(mask("API_KEY=Xk29fj2Lq0").0, "API_KEY=••••••••");
        assert_eq!(mask("Contraseña: Hunter2!").0, "Contraseña: ••••••••");
        assert!(hidden("export OPENAI_API_KEY=sk-proj-abcdef1234567890xyz"));
    }

    #[test]
    fn texto_normal_no() {
        assert!(!hidden("Contraseña del wifi de invitados: pídela en recepción"));
        assert!(!hidden("https://github.com/zed-industries/zed/tree/main/crates/gpui"));
        assert!(!hidden("C:\\Users\\Lenovo\\AppData\\Roaming\\ciat\\atic\\data"));
        assert!(!hidden("cargo test --locked -p atic-core historial"));
        assert!(!hidden("Reunión con el equipo de plataforma el jueves a las 10:30"));
        assert!(!hidden("calcantara@ejemplo.cl"));
        assert!(!hidden("7AB3-55Q2-910Z"));
    }
}
