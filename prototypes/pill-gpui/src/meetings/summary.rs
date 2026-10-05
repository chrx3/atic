//! El resumen leído como documento: secciones con párrafos y listas.
//!
//! El texto lo escribe un modelo con un Markdown simple (y a veces con los
//! encabezados viejos en texto plano). Se convierte a un modelo propio y se
//! dibuja con elementos; nada se interpreta como formato fuera de esto.
//! Portado de `core/summary-format.ts` de Atic.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Summary,
    Topics,
    Decisions,
    Tasks,
    General,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub text: String,
    /// `Some` si el modelo lo escribió como casilla (`- [ ]` / `- [x]`).
    pub checked: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Paragraph(String),
    List { ordered: bool, items: Vec<Item> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    pub title: String,
    pub kind: Kind,
    pub blocks: Vec<Block>,
}

const KNOWN_SECTIONS: [&str; 24] = [
    "resumen", "summary", "temas tratados", "topics covered", "temas", "topics", "key points",
    "puntos clave", "decisiones", "decisions", "acuerdos", "agreements", "próximos pasos",
    "proximos pasos", "next steps", "tareas", "tasks", "acciones", "actions", "conclusiones",
    "cierre", "contexto", "pendientes", "compromisos",
];

fn classify(title: &str) -> Kind {
    let t = title.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| t.contains(w));
    if has(&["resumen", "summary", "contexto", "key point", "punto clave"]) {
        Kind::Summary
    } else if has(&["tema", "topic"]) {
        Kind::Topics
    } else if has(&["decisi", "acuerdo", "agreement"]) {
        Kind::Decisions
    } else if has(&["tarea", "task", "paso", "next step", "acci", "action", "pendiente", "compromiso"]) {
        Kind::Tasks
    } else {
        Kind::General
    }
}

/// Quita los adornos en línea: `**negrita**`, `__x__`, `` `código` `` y los
/// enlaces `[texto](url)`, que quedan como su texto.
fn clean_inline(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let chars: Vec<char> = value.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '[' {
            if let Some(close) = chars[i + 1..].iter().position(|&c| c == ']') {
                let close = i + 1 + close;
                if chars.get(close + 1) == Some(&'(') {
                    if let Some(end) = chars[close + 2..].iter().position(|&c| c == ')') {
                        out.extend(&chars[i + 1..close]);
                        i = close + 2 + end + 1;
                        continue;
                    }
                }
            }
        }
        // Solo los dobles: un `_` suelto es parte de un nombre (`snake_case`).
        let doubled = |m: char| c == m && chars.get(i + 1) == Some(&m);
        if doubled('*') || doubled('_') {
            i += 2;
            continue;
        }
        if c == '`' {
            i += 1;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn heading(line: &str) -> Option<String> {
    if let Some(rest) = line.strip_prefix('#') {
        let rest = rest.trim_start_matches('#');
        let hashes = line.len() - rest.len();
        if hashes <= 4 && rest.starts_with(char::is_whitespace) {
            let title = rest.trim().trim_end_matches('#').trim();
            if !title.is_empty() {
                return Some(title.to_string());
            }
        }
    }
    // `**Decisiones**` o `**Decisiones:**` solo en la línea.
    if let Some(inner) = line.strip_prefix("**") {
        let inner = inner.trim_end().trim_end_matches(':').trim_end();
        if let Some(inner) = inner.strip_suffix("**") {
            let inner = inner.trim_end_matches(':');
            if !inner.is_empty() && !inner.contains('*') {
                return Some(inner.to_string());
            }
        }
    }
    let bare = line.trim_end_matches(':').trim().to_lowercase();
    KNOWN_SECTIONS.contains(&bare.as_str()).then(|| line.to_string())
}

/// `- texto`, `* texto`, `• texto`, `1. texto`, `2) texto`, con casilla opcional.
fn list_item(line: &str) -> Option<(bool, Item)> {
    let (ordered, rest) = if let Some(rest) = line.strip_prefix(['-', '*', '•']) {
        (false, rest)
    } else {
        let digits = line.chars().take_while(char::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        let rest = line[digits..].strip_prefix(['.', ')'])?;
        (true, rest)
    };
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let mut rest = rest.trim();
    let mut checked = None;
    for (mark, value) in [("[ ]", false), ("[x]", true), ("[X]", true)] {
        if let Some(after) = rest.strip_prefix(mark) {
            if after.starts_with(char::is_whitespace) {
                checked = Some(value);
                rest = after.trim_start();
            }
        }
    }
    if rest.is_empty() {
        return None;
    }
    Some((ordered, Item { text: clean_inline(rest), checked }))
}

pub fn parse(source: &str, default_title: &str) -> Vec<Section> {
    let normalized = source.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines: Vec<&str> = normalized.split('\n').collect();
    if lines.first().is_some_and(|l| l.trim().starts_with("```")) {
        lines.remove(0);
    }
    if lines.last().is_some_and(|l| l.trim() == "```") {
        lines.pop();
    }

    let mut sections: Vec<Section> = Vec::new();
    let mut paragraph: Vec<&str> = Vec::new();
    let mut list: Option<(bool, Vec<Item>)> = None;

    fn current<'a>(sections: &'a mut Vec<Section>, default_title: &str) -> &'a mut Section {
        if sections.is_empty() {
            sections.push(Section {
                title: default_title.to_string(),
                kind: classify(default_title),
                blocks: Vec::new(),
            });
        }
        sections.last_mut().unwrap()
    }
    let flush_paragraph = |sections: &mut Vec<Section>, paragraph: &mut Vec<&str>| {
        let text = clean_inline(&paragraph.join(" "));
        if !text.is_empty() {
            current(sections, default_title).blocks.push(Block::Paragraph(text));
        }
        paragraph.clear();
    };
    let flush_list = |sections: &mut Vec<Section>, list: &mut Option<(bool, Vec<Item>)>| {
        if let Some((ordered, items)) = list.take() {
            if !items.is_empty() {
                current(sections, default_title).blocks.push(Block::List { ordered, items });
            }
        }
    };

    for raw in lines {
        let line = raw.trim();
        if let Some(title) = heading(line) {
            flush_paragraph(&mut sections, &mut paragraph);
            flush_list(&mut sections, &mut list);
            let title = clean_inline(title.trim_end_matches(':'));
            let title = if title.is_empty() { default_title.to_string() } else { title };
            sections.push(Section { kind: classify(&title), title, blocks: Vec::new() });
            continue;
        }
        if let Some((ordered, item)) = list_item(line) {
            flush_paragraph(&mut sections, &mut paragraph);
            if list.as_ref().is_some_and(|(o, _)| *o != ordered) {
                flush_list(&mut sections, &mut list);
            }
            list.get_or_insert_with(|| (ordered, Vec::new())).1.push(item);
            continue;
        }
        if line.is_empty() {
            flush_paragraph(&mut sections, &mut paragraph);
            flush_list(&mut sections, &mut list);
            continue;
        }
        flush_list(&mut sections, &mut list);
        paragraph.push(line);
    }
    flush_paragraph(&mut sections, &mut paragraph);
    flush_list(&mut sections, &mut list);
    sections
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_sections_lists_and_checkboxes() {
        let source = "## Resumen\nSe revisó el **presupuesto** del\ntrimestre.\n\n### Decisiones\n- Congelar contrataciones\n- Mover el lanzamiento\n\n## Próximos pasos\n1. [x] Enviar acta\n2) [ ] Llamar a [Ana](mailto:ana@x.cl)";
        let sections = parse(source, "Resumen");
        assert_eq!(sections.len(), 3);
        assert_eq!(sections[0].kind, Kind::Summary);
        assert_eq!(
            sections[0].blocks,
            vec![Block::Paragraph("Se revisó el presupuesto del trimestre.".into())]
        );
        assert_eq!(sections[1].kind, Kind::Decisions);
        let Block::List { ordered, items } = &sections[2].blocks[0] else { panic!() };
        assert!(*ordered);
        assert_eq!(items[0].checked, Some(true));
        assert_eq!(items[1].text, "Llamar a Ana");
        assert_eq!(items[1].checked, Some(false));
        assert_eq!(sections[2].kind, Kind::Tasks);
    }

    #[test]
    fn legacy_headings_fences_and_plain_text() {
        let source = "```\nTemas tratados:\n* Roadmap\nDecisiones\nNinguna.\n```";
        let sections = parse(source, "Resumen");
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].title, "Temas tratados");
        assert_eq!(sections[0].kind, Kind::Topics);
        assert_eq!(sections[1].blocks, vec![Block::Paragraph("Ninguna.".into())]);

        let plain = parse("Solo un párrafo sin títulos.", "Resumen");
        assert_eq!(plain.len(), 1);
        assert_eq!(plain[0].title, "Resumen");
        assert!(parse("   \n\n", "Resumen").is_empty());
    }

    #[test]
    fn bold_heading_and_list_kind_switch() {
        let sections = parse("**Acuerdos:**\n- uno\n1. dos", "Resumen");
        assert_eq!(sections[0].title, "Acuerdos");
        assert_eq!(sections[0].blocks.len(), 2);
        // Un guion pegado al texto no es lista.
        assert_eq!(parse("-nota", "R")[0].blocks, vec![Block::Paragraph("-nota".into())]);
    }
}
