//! Filtra bloques de razonamiento (`<think>`, etc.) que algunos modelos
//! (MiniMax M2/M3, DeepSeek-R1, …) meten en `content`.

const OPEN_TAGS: &[&str] = &["<think>", "<thinking>"];
const CLOSE_TAGS: &[&str] = &["</think>", "</thinking>"];

/// Elimina bloques de thinking completos (y restos huérfanos de cierre).
pub fn strip_thinking_blocks(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    loop {
        let Some((open_at, open_len)) = find_any(rest, OPEN_TAGS) else {
            // Sin apertura: quitar cierres sueltos y devolver el resto.
            out.push_str(&strip_orphan_closes(rest));
            break;
        };
        out.push_str(&rest[..open_at]);
        let after_open = &rest[open_at + open_len..];
        if let Some((close_at, close_len)) = find_any(after_open, CLOSE_TAGS) {
            rest = &after_open[close_at + close_len..];
        } else {
            // Tag abierto sin cierre: descartar el resto (es thinking truncado).
            break;
        }
    }
    out.trim().to_string()
}

fn strip_orphan_closes(s: &str) -> String {
    let mut out = s.to_string();
    for tag in CLOSE_TAGS {
        out = out.replace(tag, "");
    }
    out
}

fn find_any(hay: &str, needles: &[&str]) -> Option<(usize, usize)> {
    needles
        .iter()
        .filter_map(|n| hay.find(n).map(|i| (i, n.len())))
        .min_by_key(|(i, _)| *i)
}

/// Filtro incremental para streaming: no emite texto dentro de bloques think.
#[derive(Default)]
pub struct ThinkingFilter {
    buf: String,
    in_think: bool,
}

impl ThinkingFilter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Empuja un trozo SSE y devuelve solo el texto visible (fuera de think).
    pub fn push(&mut self, chunk: &str) -> String {
        self.buf.push_str(chunk);
        let mut visible = String::new();

        loop {
            if self.in_think {
                if let Some((close_at, close_len)) = find_any(&self.buf, CLOSE_TAGS) {
                    self.buf = self.buf[close_at + close_len..].to_string();
                    self.in_think = false;
                    continue;
                }
                // Puede haber un cierre parcial al final; conservar cola corta.
                keep_tail(&mut self.buf, max_close_len());
                break;
            }

            if let Some((open_at, open_len)) = find_any(&self.buf, OPEN_TAGS) {
                visible.push_str(&self.buf[..open_at]);
                self.buf = self.buf[open_at + open_len..].to_string();
                self.in_think = true;
                continue;
            }

            // Posible apertura parcial al final: no emitir la cola ambigua.
            let hold = longest_open_prefix(&self.buf);
            let emit_end = self.buf.len().saturating_sub(hold);
            visible.push_str(&self.buf[..emit_end]);
            self.buf = self.buf[emit_end..].to_string();
            break;
        }

        visible
    }

    /// Vacía el buffer al terminar el stream (si no estamos dentro de think).
    pub fn finish(&mut self) -> String {
        if self.in_think {
            self.buf.clear();
            return String::new();
        }
        let leftover = std::mem::take(&mut self.buf);
        strip_orphan_closes(&leftover)
    }
}

fn max_close_len() -> usize {
    CLOSE_TAGS.iter().map(|t| t.len()).max().unwrap_or(0)
}

fn keep_tail(buf: &mut String, max: usize) {
    if buf.len() > max {
        *buf = buf[buf.len() - max..].to_string();
    }
}

fn longest_open_prefix(s: &str) -> usize {
    let mut best = 0;
    for tag in OPEN_TAGS {
        for len in 1..tag.len() {
            if s.ends_with(&tag[..len]) {
                best = best.max(len);
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_full_block() {
        let raw = "antes <think>razonamiento largo</think>\n## Acuerdos\n- ok";
        assert_eq!(
            strip_thinking_blocks(raw),
            "antes \n## Acuerdos\n- ok".trim()
        );
    }

    #[test]
    fn strips_unclosed_block() {
        let raw = "<think>solo thinking sin respuesta";
        assert_eq!(strip_thinking_blocks(raw), "");
    }

    #[test]
    fn filter_streaming_chunks() {
        let mut f = ThinkingFilter::new();
        let mut out = String::new();
        out.push_str(&f.push("<thi"));
        out.push_str(&f.push("nk>secreto</thi"));
        out.push_str(&f.push("nk>## Acuerdos"));
        out.push_str(&f.finish());
        assert_eq!(out.trim(), "## Acuerdos");
    }

    #[test]
    fn filter_passes_clean_text() {
        let mut f = ThinkingFilter::new();
        let mut out = String::new();
        out.push_str(&f.push("## Acuerdos\n- uno"));
        out.push_str(&f.finish());
        assert_eq!(out, "## Acuerdos\n- uno");
    }
}
