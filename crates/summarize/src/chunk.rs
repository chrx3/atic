//! Trocear transcripciones para cupos chicos (Groq TPM ≈ techo por request).

/// Cupo on_demand de Groq cuando el 413 no trae `Limit`.
pub const GROQ_TPM_FALLBACK: u32 = 8000;

/// Tokens de sistema + instrucciones, aparte del texto fuente.
const OVERHEAD_TOKENS: u32 = 900;

/// Cuántos caracteres de fuente caben en un request con este cupo y `max_tokens`.
///
/// 2.5 chars/token: el español tokeniza más denso que el 4 inglés, y pasarse
/// por 200 tokens es un 413 que tira el minuto entero.
pub fn chunk_char_budget(tpm_limit: u32, max_out: u32) -> usize {
    let input = tpm_limit
        .saturating_sub(max_out)
        .saturating_sub(OVERHEAD_TOKENS)
        .max(400);
    (input as usize).saturating_mul(5) / 2
}

pub fn source_fits(source_chars: usize, tpm_limit: u32, max_out: u32) -> bool {
    source_chars <= chunk_char_budget(tpm_limit, max_out)
}

/// Parte el texto por líneas; si una línea no cabe, la corta por caracteres.
pub fn split_plain(text: &str, max_chars: usize) -> Vec<String> {
    let max_chars = max_chars.max(1);
    let mut out = Vec::new();
    let mut current = String::new();

    for line in text.split('\n') {
        if line.chars().count() > max_chars {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            push_oversized_line(line, max_chars, &mut out);
            continue;
        }
        let extra = if current.is_empty() {
            line.chars().count()
        } else {
            current.chars().count() + 1 + line.chars().count()
        };
        if extra > max_chars && !current.is_empty() {
            out.push(std::mem::take(&mut current));
            current.push_str(line);
        } else {
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

fn push_oversized_line(line: &str, max_chars: usize, out: &mut Vec<String>) {
    let mut rest = line;
    while rest.chars().count() > max_chars {
        let idx = rest
            .char_indices()
            .nth(max_chars)
            .map(|(i, _)| i)
            .unwrap_or(rest.len());
        out.push(rest[..idx].to_string());
        rest = &rest[idx..];
    }
    if !rest.is_empty() {
        out.push(rest.to_string());
    }
}

/// `Limit 8000, Requested 8272` en el JSON de Groq.
pub fn parse_tpm_limit(body: &str) -> Option<u32> {
    parse_labeled_u32(body, "Limit ")
}

pub fn parse_tpm_requested(body: &str) -> Option<u32> {
    parse_labeled_u32(body, "Requested ")
}

fn parse_labeled_u32(body: &str, marker: &str) -> Option<u32> {
    let idx = body.find(marker)?;
    let rest = &body[idx + marker.len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    let n: u32 = digits.parse().ok()?;
    (n > 0).then_some(n)
}

/// 413 / "reduce your message" = el prompt no cabe.
/// Un 429 con `Requested < Limit` es ritmo, no tamaño: no trocear.
pub fn is_payload_too_large(status: u16, body: &str) -> bool {
    if status == 413
        || body.contains("Request too large")
        || body.contains("please reduce your message size")
    {
        return true;
    }
    match (parse_tpm_limit(body), parse_tpm_requested(body)) {
        (Some(limit), Some(requested)) => requested > limit,
        _ => false,
    }
}

pub fn parse_retry_secs(body: &str) -> Option<u64> {
    let marker = "please wait ";
    let idx = body.to_ascii_lowercase().find(marker)?;
    let rest = &body[idx + marker.len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok().filter(|n| *n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_lines_without_exceeding() {
        let parts = split_plain("aaa\nbbb\nccc", 8);
        assert_eq!(parts, vec!["aaa\nbbb", "ccc"]);
    }

    #[test]
    fn splits_an_oversized_line() {
        let parts = split_plain("abcdefghij", 4);
        assert_eq!(parts, vec!["abcd", "efgh", "ij"]);
    }

    #[test]
    fn empty_text_yields_one_empty_chunk() {
        assert_eq!(split_plain("", 10), vec![""]);
    }

    #[test]
    fn parses_groq_tpm_limit() {
        let body = r#"{"error":{"message":"Request too large for model `openai/gpt-oss-120b` on tokens per minute (TPM): Limit 8000, Requested 8272, please reduce your message size"}}"#;
        assert_eq!(parse_tpm_limit(body), Some(8000));
        assert_eq!(parse_tpm_requested(body), Some(8272));
        assert!(is_payload_too_large(413, body));
        assert!(!source_fits(30_000, 8000, 1024));
        assert!(source_fits(8_000, 8000, 1024));
    }

    #[test]
    fn tpm_wait_is_rate_not_size() {
        let body =
            "Rate limit reached for model: Limit 8000, Requested 2100, please wait 7 seconds";
        assert!(!is_payload_too_large(429, body));
        assert_eq!(parse_retry_secs(body), Some(7));
    }

    #[test]
    fn chunk_budget_leaves_room_for_output() {
        let chars = chunk_char_budget(8000, 1024);
        assert!(chars >= 12_000);
        assert!(chars < 25_000);
    }

    #[test]
    fn parse_wait_from_groq_copy() {
        assert_eq!(
            parse_retry_secs("please wait 7 seconds and try again"),
            Some(7)
        );
    }
}
