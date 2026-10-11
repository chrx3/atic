//! «Ocultar datos» de la Pizarra: qué partes de lo que leyó el OCR son
//! datos que no conviene mostrar en una captura, y dónde están.
//!
//! Se mira cada línea entera (el OCR la parte en palabras: un teléfono con
//! espacios son varias) y lo encontrado vuelve a sus palabras para saber qué
//! recuadro tapar. Prefiere tapar de más que de menos: una fecha se salva,
//! pero cualquier número largo se tapa.

use std::sync::OnceLock;

use regex::Regex;

use crate::ocr::Word;

/// Un recuadro a tapar, en píxeles de la imagen leída.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

struct Patterns {
    /// Lo que se tapa entero.
    found: Vec<Regex>,
    /// «clave: …»: se tapa lo que viene después de la etiqueta.
    labeled: Regex,
    /// Lo que parece número largo pero es una fecha o una hora.
    dates: Regex,
}

fn patterns() -> &'static Patterns {
    static PATTERNS: OnceLock<Patterns> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        let re = |pattern: &str| Regex::new(pattern).expect("patrón válido");
        Patterns {
            found: vec![
                // Correo.
                re(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}"),
                // RUT chileno, con o sin puntos.
                re(r"\b\d{1,2}\.?\d{3}\.?\d{3}-[\dkK]\b"),
                // IP.
                re(r"\b\d{1,3}(?:\.\d{1,3}){3}\b"),
                // Números largos: teléfonos, tarjetas, cuentas (8 cifras o
                // más, con espacios, guiones o paréntesis entre medio).
                re(r"\+?\(?\d(?:[\d\s().-]*\d){7,}"),
                // Llaves y tokens con prefijo conocido, y JWT.
                re(r"\b(?:sk|pk|rk)[-_][A-Za-z0-9_-]{10,}|\bgh[pousr]_[A-Za-z0-9]{20,}|\bxox[abpr]-[A-Za-z0-9-]{10,}|\bAKIA[A-Z0-9]{12,}|\beyJ[A-Za-z0-9_-]{10,}(?:\.[A-Za-z0-9_-]+)*"),
                // Cadenas largas de letras y cifras mezcladas (llaves sin prefijo).
                re(r"\b[A-Za-z0-9_-]*(?:[A-Za-z][A-Za-z0-9_-]*\d|\d[A-Za-z0-9_-]*[A-Za-z])[A-Za-z0-9_-]*\b"),
            ],
            labeled: re(
                r"(?i)\b(?:clave|contraseña|contrasena|password|passwd|pass|pin|token|secret|secreto|api[ _-]?key)\s*[:=]\s*(\S.*)",
            ),
            dates: re(r"^\s*(?:\d{4}[-/.]\d{1,2}[-/.]\d{1,2}|\d{1,2}[-/.]\d{1,2}[-/.]\d{2,4})(?:\s+\d{1,2}:\d{2}(?::\d{2})?)?\s*$"),
        }
    })
}

/// Los tramos (en bytes) de `line` que hay que tapar.
pub fn sensitive_spans(line: &str) -> Vec<std::ops::Range<usize>> {
    let p = patterns();
    let mut spans = Vec::new();
    for (i, pattern) in p.found.iter().enumerate() {
        for m in pattern.find_iter(line) {
            let text = m.as_str();
            // Números largos: no las fechas ni lo que tiene menos de 8 cifras.
            if i == 3 && (p.dates.is_match(text) || text.chars().filter(char::is_ascii_digit).count() < 8) {
                continue;
            }
            // Mezcla de letras y cifras: solo si es larga, como una llave.
            if i == 5 && text.chars().count() < 20 {
                continue;
            }
            spans.push(m.range());
        }
    }
    if let Some(value) = p.labeled.captures(line).and_then(|c| c.get(1)) {
        spans.push(value.range());
    }
    // Un RUT o una IP también son «números largos»: queda el tramo más amplio.
    spans.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
    let mut kept: Vec<std::ops::Range<usize>> = Vec::new();
    for span in spans {
        if !kept.iter().any(|k| k.start <= span.start && span.end <= k.end) {
            kept.push(span);
        }
    }
    kept
}

/// Los recuadros a tapar en lo que leyó el OCR: uno por dato encontrado, que
/// cubre sus palabras (con un poco de margen).
pub fn sensitive_areas(lines: &[Vec<Word>]) -> Vec<Area> {
    const PAD: f32 = 2.0;
    let mut areas = Vec::new();
    for words in lines {
        // La línea como texto, con el tramo de cada palabra.
        let mut text = String::new();
        let mut ranges = Vec::with_capacity(words.len());
        for word in words {
            if !text.is_empty() {
                text.push(' ');
            }
            let start = text.len();
            text.push_str(&word.text);
            ranges.push(start..text.len());
        }
        for span in sensitive_spans(&text) {
            let hit: Vec<&Word> = words
                .iter()
                .zip(&ranges)
                .filter(|(_, range)| range.start < span.end && span.start < range.end)
                .map(|(word, _)| word)
                .collect();
            let Some(first) = hit.first() else { continue };
            let (mut x0, mut y0, mut x1, mut y1) = (first.x, first.y, first.x + first.w, first.y + first.h);
            for word in &hit[1..] {
                x0 = x0.min(word.x);
                y0 = y0.min(word.y);
                x1 = x1.max(word.x + word.w);
                y1 = y1.max(word.y + word.h);
            }
            let area = Area { x: x0 - PAD, y: y0 - PAD, w: x1 - x0 + PAD * 2.0, h: y1 - y0 + PAD * 2.0 };
            if !areas.contains(&area) {
                areas.push(area);
            }
        }
    }
    areas
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hidden(line: &str) -> Vec<&str> {
        let mut spans = sensitive_spans(line);
        spans.sort_by_key(|s| s.start);
        spans.into_iter().map(|s| &line[s]).collect()
    }

    #[test]
    fn encuentra_los_datos_tipicos() {
        assert_eq!(hidden("Escríbeme a carla.rojas@empresa.cl mañana"), vec!["carla.rojas@empresa.cl"]);
        assert_eq!(hidden("RUT 12.345.678-K"), vec!["12.345.678-K"]);
        assert_eq!(hidden("Llama al +56 9 8765 4321"), vec!["+56 9 8765 4321"]);
        assert_eq!(hidden("Tarjeta 4111 1111 1111 1111"), vec!["4111 1111 1111 1111"]);
        assert_eq!(hidden("Servidor 192.168.1.20 caído"), vec!["192.168.1.20"]);
        assert!(hidden("export KEY=sk-proj-AbCdEf1234567890xyz").iter().any(|s| s.starts_with("sk-proj")));
    }

    #[test]
    fn tapa_lo_que_sigue_a_una_etiqueta() {
        assert!(hidden("Contraseña: hunter2 ").contains(&"hunter2 "));
        assert!(hidden("api_key = abc").contains(&"abc"));
    }

    #[test]
    fn no_tapa_texto_comun_ni_fechas() {
        assert!(hidden("Botón Guardar cambios").is_empty());
        assert!(hidden("Reunión el 2026-10-11 a las 14:30").is_empty());
        assert!(hidden("Versión 0.4.46 lista").is_empty());
        assert!(hidden("Pedido 1234").is_empty());
    }

    #[test]
    fn el_dato_vuelve_a_sus_palabras() {
        let word = |text: &str, x: f32| Word { text: text.into(), x, y: 10.0, w: 30.0, h: 12.0 };
        let lines = vec![vec![word("Fono", 0.0), word("+56", 40.0), word("9", 80.0), word("8765", 120.0), word("4321", 160.0)]];
        let areas = sensitive_areas(&lines);
        assert_eq!(areas.len(), 1);
        // Del «+56» al «4321», sin «Fono».
        assert_eq!(areas[0].x, 40.0 - 2.0);
        assert_eq!(areas[0].w, 150.0 + 4.0);
    }
}
