//! Lo que la ventana de Reuniones necesita para mostrar texto y fechas:
//! etiquetas de día, duraciones, la marca de tiempo de un tramo y el filtro de
//! lo que Whisper inventa sobre silencio. Portado de `core/dayGroups.ts`,
//! `core/format.ts` y `core/transcriptText.ts` de Atic.

use std::collections::HashMap;

use atic_core::Segment;
use chrono::{Datelike, NaiveDate, Weekday};

const MONTHS: [&str; 12] = [
    "enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre",
    "octubre", "noviembre", "diciembre",
];

fn weekday(day: Weekday) -> &'static str {
    match day {
        Weekday::Mon => "Lunes",
        Weekday::Tue => "Martes",
        Weekday::Wed => "Miércoles",
        Weekday::Thu => "Jueves",
        Weekday::Fri => "Viernes",
        Weekday::Sat => "Sábado",
        Weekday::Sun => "Domingo",
    }
}

/// El encabezado de un día en la lista: relativo mientras sirve (hoy, ayer, el
/// día de la semana) y absoluto después. El año solo si no es el corriente.
pub fn day_label(day: NaiveDate, today: NaiveDate) -> String {
    match (today - day).num_days() {
        0 => "Hoy".into(),
        1 => "Ayer".into(),
        2..=6 => weekday(day.weekday()).into(),
        _ => long_date(day, today),
    }
}

/// «19 de septiembre», o «19 de septiembre de 2025» si no es de este año.
pub fn long_date(day: NaiveDate, today: NaiveDate) -> String {
    let month = MONTHS[day.month0() as usize];
    if day.year() == today.year() {
        format!("{} de {month}", day.day())
    } else {
        format!("{} de {month} de {}", day.day(), day.year())
    }
}

/// La fecha completa del encabezado de una reunión: «Jueves 19 de septiembre».
pub fn full_date(day: NaiveDate, today: NaiveDate) -> String {
    format!("{} {}", weekday(day.weekday()), long_date(day, today))
}

/// «45 s», «12 min», «1 h 05 min».
pub fn duration(secs: i64) -> String {
    let secs = secs.max(0);
    if secs < 60 {
        return format!("{secs} s");
    }
    let minutes = (secs + 30) / 60;
    if minutes < 60 {
        format!("{minutes} min")
    } else {
        format!("{} h {:02} min", minutes / 60, minutes % 60)
    }
}

/// La marca de un tramo: «04:12» o, pasada la hora, «1:04:12».
pub fn clock(ms: i64) -> String {
    let secs = ms.max(0) / 1000;
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

/// Segmentos seguidos del mismo hablante, en un bloque. Whisper corta cada
/// pocas frases y treinta líneas con la misma etiqueta se leen peor que cuatro
/// bloques. La hora es la del primer segmento.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub me: bool,
    pub label: String,
    pub start_ms: i64,
    pub text: String,
}

pub fn blocks(segments: &[Segment]) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    for segment in segments.iter().filter(|s| !is_junk(&s.text)) {
        let label = segment.speaker_label().to_string();
        let text = segment.text.trim();
        match out.last_mut() {
            Some(last) if last.label == label => {
                last.text.push(' ');
                last.text.push_str(text);
            }
            _ => out.push(Block {
                me: segment.speaker == atic_core::Speaker::Me,
                label,
                start_ms: segment.start_ms,
                text: text.to_string(),
            }),
        }
    }
    out
}

/// Texto que Whisper inventa sobre silencio o estática, no habla real.
pub fn is_junk(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() || is_silence_marker(trimmed) {
        return true;
    }
    // Puntos sueltos («...», «.»): Whisper los deja sobre los silencios.
    if trimmed.chars().all(|c| !c.is_alphanumeric()) {
        return true;
    }
    is_repetition(trimmed)
}

fn is_silence_marker(text: &str) -> bool {
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_lowercase();
    matches!(
        compact.as_str(),
        "[silence]" | "(silence)" | "[silence]." | "(silence)." | "[blank_audio]" | "[blankaudio]"
            | "[inaudible]" | "(inaudible)" | "[music]" | "(music)" | "silence"
    ) || ["[silence", "(silence", "[music", "(music", "[blank"]
        .iter()
        .any(|prefix| compact.starts_with(prefix))
}

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

fn is_repetition(text: &str) -> bool {
    let words = words(text);
    if words.len() < 6 {
        return false;
    }
    let mut run = 1;
    for pair in words.windows(2) {
        if pair[0] == pair[1] {
            run += 1;
            if run >= 6 {
                return true;
            }
        } else {
            run = 1;
        }
    }
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for word in &words {
        *counts.entry(word).or_default() += 1;
    }
    let total = words.len();
    if total >= 12 && counts.len() as f32 / total as f32 <= 0.28 {
        return true;
    }
    if counts.values().any(|&n| n >= 8 && n as f32 / total as f32 >= 0.35) {
        return true;
    }
    (2..=4).any(|n| max_ngram_repeats(&words, n) >= 4)
}

fn max_ngram_repeats(words: &[String], n: usize) -> usize {
    if words.len() < n * 2 {
        return 1;
    }
    let mut best = 1;
    for i in 0..=words.len() - n * 2 {
        let first = &words[i..i + n];
        let mut repeats = 1;
        let mut j = i + n;
        while j + n <= words.len() && &words[j..j + n] == first {
            repeats += 1;
            j += n;
        }
        best = best.max(repeats);
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use atic_core::Speaker;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn day_labels_are_relative_then_absolute() {
        let today = day(2026, 10, 4); // domingo
        assert_eq!(day_label(day(2026, 10, 4), today), "Hoy");
        assert_eq!(day_label(day(2026, 10, 3), today), "Ayer");
        assert_eq!(day_label(day(2026, 10, 1), today), "Jueves");
        assert_eq!(day_label(day(2026, 9, 19), today), "19 de septiembre");
        assert_eq!(day_label(day(2025, 12, 2), today), "2 de diciembre de 2025");
        assert_eq!(full_date(day(2026, 9, 17), today), "Jueves 17 de septiembre");
    }

    #[test]
    fn durations_and_clocks() {
        assert_eq!(duration(45), "45 s");
        assert_eq!(duration(12 * 60 + 10), "12 min");
        assert_eq!(duration(65 * 60), "1 h 05 min");
        assert_eq!(clock(252_000), "04:12");
        assert_eq!(clock(3_852_000), "1:04:12");
    }

    #[test]
    fn junk_text_is_filtered() {
        assert!(is_junk("..."));
        assert!(is_junk(" [BLANK_AUDIO] "));
        assert!(is_junk("(música de fondo)") == false);
        assert!(is_junk("gracias gracias gracias gracias gracias gracias"));
        assert!(is_junk("no sé no sé no sé no sé"));
        assert!(!is_junk("Revisemos el presupuesto del trimestre."));
        assert!(!is_junk("y"));
    }

    fn segment(start_ms: i64, speaker: Speaker, text: &str) -> Segment {
        Segment { start_ms, end_ms: start_ms + 1000, speaker, speaker_name: None, text: text.into() }
    }

    #[test]
    fn consecutive_segments_merge_by_speaker() {
        let segments = [
            segment(0, Speaker::Me, "Hola."),
            segment(1000, Speaker::Me, " ¿Me escuchan?"),
            segment(2000, Speaker::Others, "..."),
            segment(3000, Speaker::Others, "Sí, fuerte y claro."),
            segment(4000, Speaker::Me, "Perfecto."),
        ];
        let out = blocks(&segments);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].text, "Hola. ¿Me escuchan?");
        assert!(out[0].me);
        assert_eq!(out[1].start_ms, 3000);
        assert_eq!(out[1].label, "Los demás");
    }
}
