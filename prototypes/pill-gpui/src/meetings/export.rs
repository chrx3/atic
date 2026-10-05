//! Exportar una reunión a Markdown, Word o PDF: el resumen y la transcripción
//! en un solo documento. Portado de `src-tauri/src/export.rs` de Atic: el
//! DOCX y el PDF se arman a mano (un zip con `document.xml` y un PDF de texto
//! con Helvetica), sin crates de documentos que pesan más que todo esto.
//!
//! A diferencia de Atic, la transcripción no es obligatoria: un resumen
//! escrito a mano también se puede exportar solo.

use std::io::Write;
use std::path::Path;

use atic_core::{Recording, Segment, Speaker, Summary, Transcript};
use chrono::Local;
use zip::write::SimpleFileOptions;

use super::text;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Markdown,
    Word,
    Pdf,
}

impl Format {
    pub const ALL: [Format; 3] = [Format::Markdown, Format::Word, Format::Pdf];

    pub fn extension(self) -> &'static str {
        match self {
            Format::Markdown => "md",
            Format::Word => "docx",
            Format::Pdf => "pdf",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Format::Markdown => "Markdown",
            Format::Word => "Word",
            Format::Pdf => "PDF",
        }
    }
}

/// Lo que va en el documento, ya leído del disco.
pub struct Document<'a> {
    pub recording: &'a Recording,
    pub transcript: Option<&'a Transcript>,
    pub summary: Option<&'a Summary>,
}

/// Escribe el documento en `path`. Si `path` no trae la extensión del
/// formato se le agrega (el diálogo de Windows deja borrarla).
pub fn write(doc: &Document, format: Format, path: &Path) -> anyhow::Result<std::path::PathBuf> {
    if doc.transcript.is_none() && doc.summary.is_none() {
        anyhow::bail!("No hay nada que exportar: falta la transcripción y el resumen.");
    }
    let path = with_extension(path, format);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match format {
        Format::Markdown => std::fs::write(&path, markdown(doc))?,
        Format::Word => std::fs::write(&path, docx(doc)?)?,
        Format::Pdf => std::fs::write(&path, pdf(doc))?,
    }
    Ok(path)
}

pub fn with_extension(path: &Path, format: Format) -> std::path::PathBuf {
    let ext = format.extension();
    let has = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(ext));
    if has {
        path.to_path_buf()
    } else {
        let mut name = path.as_os_str().to_owned();
        name.push(".");
        name.push(ext);
        name.into()
    }
}

/// Un nombre de archivo a partir del título: sin los caracteres que Windows
/// no admite y sin espacios sueltos al final.
pub fn file_name(title: &str, format: Format) -> String {
    let clean: String = title
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => ' ',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    let clean: String = clean.chars().take(80).collect();
    let clean = clean.trim_end_matches(['.', ' ']);
    let stem = if clean.is_empty() { "Reunión" } else { clean };
    format!("{stem}.{}", format.extension())
}

fn speaker(segment: &Segment) -> String {
    segment
        .speaker_name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| match segment.speaker {
            Speaker::Me => "Yo".into(),
            Speaker::Others => "Los demás".into(),
        })
}

fn title(doc: &Document) -> String {
    doc.recording.title.replace(['\r', '\n'], " ")
}

fn when(doc: &Document) -> String {
    doc.recording
        .started_at
        .with_timezone(&Local)
        .format("%d-%m-%Y %H:%M")
        .to_string()
}

fn transcript_lines(transcript: &Transcript) -> impl Iterator<Item = String> + '_ {
    transcript
        .segments
        .iter()
        .filter(|s| !s.text.trim().is_empty())
        .map(|s| format!("[{}] {}: {}", text::clock(s.start_ms), speaker(s), s.text.trim()))
}

/// Línea a línea, con cuáles son títulos: lo comparten Word y PDF.
fn lines(doc: &Document) -> Vec<(String, Style)> {
    let mut out = vec![
        (title(doc), Style::Title),
        (
            format!("Fecha: {} · Duración: {}", when(doc), text::duration(doc.recording.duration_secs)),
            Style::Body,
        ),
        (String::new(), Style::Body),
    ];
    if let Some(summary) = doc.summary {
        out.push(("Resumen".into(), Style::Heading));
        if let Some(subject) = summary.subject.as_deref().filter(|s| !s.trim().is_empty()) {
            out.push((format!("Asunto: {}", subject.trim()), Style::Body));
        }
        out.extend(summary.body.trim().lines().map(|l| (l.to_string(), Style::Body)));
        out.push((String::new(), Style::Body));
    }
    if let Some(transcript) = doc.transcript {
        out.push(("Transcripción".into(), Style::Heading));
        out.extend(transcript_lines(transcript).map(|l| (l, Style::Body)));
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Style {
    Title,
    Heading,
    Body,
}

pub fn markdown(doc: &Document) -> String {
    let mut out = format!(
        "# {}\n\n- Fecha: {}\n- Duración: {}\n\n",
        title(doc),
        when(doc),
        text::duration(doc.recording.duration_secs)
    );
    if let Some(summary) = doc.summary {
        out.push_str("## Resumen\n\n");
        if let Some(subject) = summary.subject.as_deref().filter(|s| !s.trim().is_empty()) {
            out.push_str(&format!("**Asunto:** {}\n\n", subject.trim()));
        }
        // Los títulos del resumen bajan un nivel para quedar dentro de «Resumen».
        for line in summary.body.trim().lines() {
            if line.starts_with('#') {
                out.push('#');
            }
            out.push_str(line);
            out.push('\n');
        }
        out.push('\n');
    }
    if let Some(transcript) = doc.transcript {
        out.push_str("## Transcripción\n\n");
        for s in transcript.segments.iter().filter(|s| !s.text.trim().is_empty()) {
            out.push_str(&format!(
                "**[{}] {}:** {}\n\n",
                text::clock(s.start_ms),
                speaker(s),
                s.text.trim()
            ));
        }
    }
    out
}

pub fn docx(doc: &Document) -> anyhow::Result<Vec<u8>> {
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    archive.start_file("[Content_Types].xml", options)?;
    archive.write_all(br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#)?;
    archive.start_file("_rels/.rels", options)?;
    archive.write_all(br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#)?;
    archive.start_file("word/document.xml", options)?;
    let mut xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>"#,
    );
    for (line, style) in lines(doc) {
        // Sin styles.xml: el formato va en el run (Word ignora estilos que
        // no existen y todo saldría igual).
        let run = match style {
            Style::Title => "<w:rPr><w:b/><w:sz w:val=\"36\"/></w:rPr>",
            Style::Heading => "<w:rPr><w:b/><w:sz w:val=\"28\"/></w:rPr>",
            Style::Body => "",
        };
        xml.push_str(&format!(
            "<w:p><w:r>{run}<w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
            escape_xml(&line)
        ));
    }
    xml.push_str("<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/><w:pgMar w:top=\"1080\" w:right=\"1080\" w:bottom=\"1080\" w:left=\"1080\"/></w:sectPr></w:body></w:document>");
    archive.write_all(xml.as_bytes())?;
    Ok(archive.finish()?.into_inner())
}

fn escape_xml(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // XML 1.0 no admite controles (salvo tab); Word no abriría el archivo.
            c if c.is_control() && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}

const PDF_LINES_PER_PAGE: usize = 49;
const PDF_CHARS_PER_LINE: usize = 92;

pub fn pdf(doc: &Document) -> Vec<u8> {
    let wrapped: Vec<(String, Style)> = lines(doc)
        .into_iter()
        .flat_map(|(line, style)| {
            let width = if style == Style::Body { PDF_CHARS_PER_LINE } else { 60 };
            wrap_line(&line, width).into_iter().map(move |l| (l, style))
        })
        .collect();
    let pages: Vec<&[(String, Style)]> = if wrapped.is_empty() {
        vec![&[]]
    } else {
        wrapped.chunks(PDF_LINES_PER_PAGE).collect()
    };
    // 1 catálogo, 2 páginas, 3 Helvetica, 4 Helvetica-Bold, luego página y
    // contenido por cada una.
    let first_page = 5;
    let mut objects: Vec<Vec<u8>> = vec![Vec::new(); 4 + pages.len() * 2];
    objects[0] = b"<< /Type /Catalog /Pages 2 0 R >>".to_vec();
    let kids = (0..pages.len())
        .map(|i| format!("{} 0 R", first_page + i * 2))
        .collect::<Vec<_>>()
        .join(" ");
    objects[1] = format!("<< /Type /Pages /Kids [{kids}] /Count {} >>", pages.len()).into_bytes();
    objects[2] = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_vec();
    objects[3] = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>".to_vec();

    for (i, lines) in pages.iter().enumerate() {
        let page_id = first_page + i * 2;
        let content_id = page_id + 1;
        objects[page_id - 1] = format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents {content_id} 0 R >>"
        )
        .into_bytes();
        let mut stream = b"BT 46 748 Td 14 TL\n".to_vec();
        for (line, style) in *lines {
            let font: &[u8] = match style {
                Style::Title => b"/F2 15 Tf ",
                Style::Heading => b"/F2 12 Tf ",
                Style::Body => b"/F1 10 Tf ",
            };
            stream.extend_from_slice(font);
            stream.push(b'(');
            stream.extend(pdf_escape(line));
            stream.extend_from_slice(b") Tj T*\n");
        }
        stream.extend_from_slice(b"ET");
        let mut object = format!("<< /Length {} >>\nstream\n", stream.len()).into_bytes();
        object.extend(stream);
        object.extend_from_slice(b"\nendstream");
        objects[content_id - 1] = object;
    }

    let mut pdf = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (i, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        pdf.extend_from_slice(object);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer << /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1)
            .as_bytes(),
    );
    pdf
}

fn wrap_line(value: &str, max_chars: usize) -> Vec<String> {
    if value.trim().is_empty() {
        return vec![String::new()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in value.split_whitespace() {
        if !current.is_empty() && current.chars().count() + word.chars().count() + 1 > max_chars {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// Texto a WinAnsi (lo que entiende la Helvetica de base del PDF): los
/// acentos del español entran; lo que no tiene lugar sale como `?`.
fn pdf_escape(value: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for c in value.chars() {
        let byte = match c {
            '\\' | '(' | ')' => {
                out.push(b'\\');
                c as u8
            }
            '\u{20AC}' => 0x80,
            '\u{2026}' => 0x85,
            '\u{2022}' => 0x95,
            '\u{2018}' => 0x91,
            '\u{2019}' => 0x92,
            '\u{201C}' => 0x93,
            '\u{201D}' => 0x94,
            '\u{2013}' => 0x96,
            '\u{2014}' => 0x97,
            '\t' => b' ',
            c if (c as u32) < 0x20 => continue,
            c if (c as u32) <= 0xFF => c as u8,
            _ => b'?',
        };
        out.push(byte);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use atic_core::RecordingStatus;
    use chrono::Utc;

    fn fixture() -> (Recording, Transcript, Summary) {
        let recording = Recording {
            id: "r1".into(),
            title: "Plan <Q4> & cierre".into(),
            started_at: Utc::now(),
            duration_secs: 125,
            mic_path: None,
            system_path: None,
            status: RecordingStatus::Summarized,
        };
        let transcript = Transcript {
            language: Some("es".into()),
            segments: vec![
                Segment {
                    start_ms: 0,
                    end_ms: 1_000,
                    speaker: Speaker::Me,
                    speaker_name: None,
                    text: "Partamos (rápido).".into(),
                },
                Segment {
                    start_ms: 65_000,
                    end_ms: 66_000,
                    speaker: Speaker::Others,
                    speaker_name: Some("José".into()),
                    text: "Revisión de acuerdos".into(),
                },
                Segment {
                    start_ms: 70_000,
                    end_ms: 71_000,
                    speaker: Speaker::Others,
                    speaker_name: None,
                    text: "   ".into(),
                },
            ],
        };
        let summary = Summary {
            template: "executive_minutes".into(),
            title: "Acta".into(),
            body: "## Decisiones\n- Congelar la app vieja".into(),
            subject: Some("Seguimiento".into()),
            backend: "claude".into(),
            created_at: Utc::now(),
        };
        (recording, transcript, summary)
    }

    #[test]
    fn el_markdown_lleva_resumen_y_transcripcion_en_orden() {
        let (rec, tr, su) = fixture();
        let md = markdown(&Document { recording: &rec, transcript: Some(&tr), summary: Some(&su) });
        assert!(md.starts_with("# Plan <Q4> & cierre\n"));
        let resumen = md.find("## Resumen").unwrap();
        let transcripcion = md.find("## Transcripción").unwrap();
        assert!(resumen < transcripcion);
        // El título del resumen baja un nivel.
        assert!(md.contains("\n### Decisiones\n"));
        assert!(md.contains("**Asunto:** Seguimiento"));
        assert!(md.contains("**[00:00] Yo:** Partamos (rápido)."));
        assert!(md.contains("**[01:05] José:** Revisión de acuerdos"));
        // Los segmentos vacíos no salen.
        assert_eq!(md.matches("Los demás").count(), 0);
    }

    #[test]
    fn sin_transcripcion_exporta_solo_el_resumen() {
        let (rec, _, su) = fixture();
        let md = markdown(&Document { recording: &rec, transcript: None, summary: Some(&su) });
        assert!(md.contains("## Resumen"));
        assert!(!md.contains("## Transcripción"));
    }

    #[test]
    fn sin_nada_no_escribe() {
        let (rec, _, _) = fixture();
        let dir = std::env::temp_dir().join("meetings-export-vacio");
        let out = write(&Document { recording: &rec, transcript: None, summary: None }, Format::Markdown, &dir.join("x"));
        assert!(out.is_err());
    }

    #[test]
    fn el_docx_es_un_zip_con_document_xml() {
        let (rec, tr, su) = fixture();
        let bytes = docx(&Document { recording: &rec, transcript: Some(&tr), summary: Some(&su) }).unwrap();
        assert!(bytes.starts_with(b"PK"));
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        for name in ["[Content_Types].xml", "_rels/.rels"] {
            assert!(zip.by_name(name).is_ok(), "falta {name}");
        }
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("word/document.xml").unwrap(), &mut xml).unwrap();
        assert!(xml.contains("Plan &lt;Q4&gt; &amp; cierre"));
        assert!(xml.contains("Revisión de acuerdos"));
        assert!(xml.ends_with("</w:document>"));
    }

    #[test]
    fn el_pdf_tiene_cabecera_xref_y_final() {
        let (rec, tr, su) = fixture();
        let bytes = pdf(&Document { recording: &rec, transcript: Some(&tr), summary: Some(&su) });
        assert!(bytes.starts_with(b"%PDF-1.4"));
        assert!(bytes.ends_with(b"%%EOF\n"));
        let text = String::from_utf8_lossy(&bytes);
        // `startxref` apunta justo a la tabla `xref`.
        let at: usize = text
            .rsplit("startxref\n")
            .next()
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(&bytes[at..at + 4], b"xref");
        // Los paréntesis van escapados y los acentos en WinAnsi.
        assert!(bytes.windows(8).any(|w| w == b"\\(r\xE1pido"));
    }

    #[test]
    fn el_pdf_pagina_los_textos_largos() {
        let (rec, mut tr, _) = fixture();
        let seg = tr.segments[0].clone();
        tr.segments = (0..200).map(|_| seg.clone()).collect();
        let bytes = pdf(&Document { recording: &rec, transcript: Some(&tr), summary: None });
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/Count 5"));
    }

    #[test]
    fn la_extension_se_agrega_si_falta() {
        assert_eq!(with_extension(Path::new("a/acta"), Format::Pdf), Path::new("a/acta.pdf"));
        assert_eq!(with_extension(Path::new("a/acta.PDF"), Format::Pdf), Path::new("a/acta.PDF"));
        assert_eq!(with_extension(Path::new("a/v1.2"), Format::Word), Path::new("a/v1.2.docx"));
    }

    #[test]
    fn el_nombre_de_archivo_es_valido_en_windows() {
        assert_eq!(file_name("Plan: Q4 / 2026?", Format::Markdown), "Plan Q4 2026.md");
        assert_eq!(file_name("  ...  ", Format::Pdf), "Reunión.pdf");
    }

    #[test]
    fn envuelve_sin_perder_palabras() {
        assert_eq!(wrap_line("uno dos tres cuatro", 8), ["uno dos", "tres", "cuatro"]);
        assert_eq!(escape_xml("A&B\u{1}"), "A&amp;B");
    }
}
