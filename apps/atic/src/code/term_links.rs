//! Enlaces clicables de la terminal: la detección pura (qué parte de una línea es
//! un enlace), sin GPUI ni disco. La vista (`terminal.rs`) los resuelve contra
//! las carpetas del espacio (`files::resolve_ref`) y los abre: las URLs en el
//! navegador y las rutas con línea con `open_file_at`.

/// A dónde lleva lo marcado: una web o un archivo con su línea.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TermLink {
    Url(String),
    File { path: String, line: Option<u32> },
}

/// Lo marcado en una línea: de `from` a `to` (en caracteres, fin exclusivo),
/// con el texto tal cual y a dónde lleva.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TermHit {
    pub from: usize,
    pub to: usize,
    pub text: String,
    pub link: TermLink,
}

/// Si el carácter corta una URL (un espacio o un cierre).
fn url_end(c: char) -> bool {
    c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '^' | '`')
}

/// Una URL que empieza en `start` (tras `http://` o `https://`): hasta el primer
/// corte, sin la puntuación del final (`mira https://x.io/y.`).
fn url_hit(chars: &[char], start: usize) -> Option<TermHit> {
    let mut end = start;
    while end < chars.len() && !url_end(chars[end]) {
        end += 1;
    }
    while end > start && matches!(chars[end - 1], '.' | ',' | ';' | ':' | '!' | '?' | ')') {
        end -= 1;
    }
    (end > start).then(|| {
        let text: String = chars[start..end].iter().collect();
        TermHit { from: start, to: end, link: TermLink::Url(text.clone()), text }
    })
}

/// `ruta:línea[:col]` en un `token` ya limpio: con `/`, `\` o extensión, y la
/// línea en dígitos. Sin línea no es enlace (un `Cargo.toml` suelto se marca
/// igual con el mouse). El texto queda normalizado como `ruta:línea`, que es lo
/// que resuelve `files::resolve_ref`.
fn file_hit(token: &str, from: usize) -> Option<TermHit> {
    let (path, line) = split_file_line(token)?;
    let looks_like_path = path.contains('/') || path.contains('\\') || path.rsplit_once('.').is_some_and(|(_, ext)| !ext.is_empty() && ext.len() <= 5 && ext.chars().all(|c| c.is_alphanumeric()));
    if !looks_like_path || path.contains(' ') {
        return None;
    }
    let to = from + token.chars().count();
    let text = format!("{path}:{line}");
    Some(TermHit { from, to, text, link: TermLink::File { path: path.to_string(), line: Some(line) } })
}

/// La ruta y la línea de `ruta:línea[:col]`: `split_path_line` solo saca el
/// último tramo (`a:42:7` daría línea 7), así que si a la ruta le queda
/// `:dígitos` esos son la línea y lo de fuera la columna.
fn split_file_line(token: &str) -> Option<(&str, u32)> {
    let (path, line) = gpui_m3::split_path_line(token);
    let line: u32 = match line {
        Some((start, _)) => start,
        None => return None,
    };
    match path.rsplit_once(':') {
        Some((base, digits)) if digits.parse::<u32>().is_ok() && (base.contains('/') || base.contains('\\') || base.contains('.')) => {
            Some((base, digits.parse().ok()?))
        }
        _ => Some((path, line)),
    }
}

/// Los enlaces de una línea de la terminal, de izquierda a derecha y sin
/// traslaparse: primero las URLs, y en el resto `ruta:línea[:col]`.
pub fn links_in_line(text: &str) -> Vec<TermHit> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<TermHit> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let tail = &chars[i..];
        let is_url = tail.starts_with(&['h', 't', 't', 'p', 's', ':', '/', '/']) || tail.starts_with(&['h', 't', 't', 'p', ':', '/', '/']);
        if is_url {
            if let Some(hit) = url_hit(&chars, i) {
                i = hit.to;
                out.push(hit);
                continue;
            }
            // El esquema solo, sin nada tras él: no es enlace.
            i += 1;
            continue;
        }
        i += 1;
    }
    // Las rutas van por palabras (sin espacios), fuera de las URLs.
    let mut start: Option<usize> = None;
    for i in 0..=chars.len() {
        let boundary = i == chars.len() || chars[i].is_whitespace();
        match (start, boundary) {
            (None, false) => start = Some(i),
            (Some(from), true) => {
                start = None;
                let word: String = chars[from..i].iter().collect();
                if out.iter().any(|hit: &TermHit| from >= hit.from && from < hit.to) {
                    continue;
                }
                let clean = word.trim_matches(|c: char| matches!(c, '"' | '\'' | '(' | ')' | '[' | ']' | '<' | '>' | ',' | ';' | ':' | '!' | '?' | '.'));
                if clean.is_empty() || clean.contains("://") {
                    continue;
                }
                let skipped = word.find(clean).unwrap_or(0);
                let at = from + word[..skipped].chars().count();
                if let Some(hit) = file_hit(clean, at) {
                    // Sin traslape con una URL.
                    if !out.iter().any(|taken: &TermHit| hit.from < taken.to && taken.from < hit.to) {
                        out.push(hit);
                    }
                }
            }
            _ => {}
        }
    }
    out.sort_by_key(|hit| hit.from);
    out
}

/// El enlace bajo el carácter `at` (índice de carácter), si hay uno.
pub fn link_at(text: &str, at: usize) -> Option<TermHit> {
    links_in_line(text).into_iter().find(|hit| at >= hit.from && at < hit.to)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detecta_urls_y_les_saca_la_puntuacion_final() {
        let hits = links_in_line("mira https://atic.app/x?y=1, y http://a.io.");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].link, TermLink::Url("https://atic.app/x?y=1".into()));
        assert_eq!(hits[0].text, "https://atic.app/x?y=1");
        assert_eq!(hits[1].link, TermLink::Url("http://a.io".into()));
        // Sin esquema no es URL.
        assert!(links_in_line("mira atic.app/x").is_empty());
    }

    #[test]
    fn detecta_rutas_con_linea_y_columna() {
        let hits = links_in_line("falla en src/main.rs:42:7 ya");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].link, TermLink::File { path: "src/main.rs".into(), line: Some(42) });
        // El texto se normaliza a `ruta:línea`, que es lo que resuelve el espacio.
        assert_eq!(hits[0].text, "src/main.rs:42");
        let hits = links_in_line("(apps/atic/src/code/term.rs:10)");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].text, "apps/atic/src/code/term.rs:10");
        // Sin línea no es enlace.
        assert!(links_in_line("edita Cargo.toml ya").is_empty());
        // Sin pinta de ruta tampoco.
        assert!(links_in_line("avísame a las 12:30 ya").is_empty());
    }

    #[test]
    fn el_enlace_bajo_el_cursor() {
        let text = "ver src/a.rs:3 y https://x.io";
        let hit = link_at(text, text.chars().position(|c| c == 'a').unwrap()).unwrap();
        assert_eq!(hit.link, TermLink::File { path: "src/a.rs".into(), line: Some(3) });
        assert!(link_at(text, 0).is_none());
        assert!(link_at(text, text.chars().count()).is_none());
    }
}
