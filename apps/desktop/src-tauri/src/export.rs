//! Exportación local de transcripciones y resúmenes.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::State;
use zip::write::SimpleFileOptions;

use atic_core::{Recording, Segment, Speaker, Summary, Transcript};

use crate::state::AppState;
use atic_core::MutexExt;

struct ExportCopy {
    date: &'static str,
    duration: &'static str,
    summary: &'static str,
    transcript: &'static str,
    me: &'static str,
    others: &'static str,
}

fn export_copy(en: bool) -> ExportCopy {
    if en {
        ExportCopy {
            date: "Date",
            duration: "Duration",
            summary: "Summary",
            transcript: "Transcript",
            me: "Me",
            others: "Others",
        }
    } else {
        ExportCopy {
            date: "Fecha",
            duration: "Duración",
            summary: "Resumen",
            transcript: "Transcripción",
            me: "Yo",
            others: "Los demás",
        }
    }
}

fn speaker_line(segment: &Segment, copy: &ExportCopy) -> String {
    segment
        .speaker_name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| match segment.speaker {
            Speaker::Me => copy.me.to_string(),
            Speaker::Others => copy.others.to_string(),
        })
}

#[derive(Serialize)]
pub struct ExportResult {
    pub path: String,
    pub format: String,
}

#[tauri::command]
pub fn export_recording(
    state: State<AppState>,
    id: String,
    format: String,
    path: String,
) -> Result<ExportResult, String> {
    let copy = export_copy(state.config.lock_or_recover().resolved_ui_language() == "en");
    let recording = state
        .db
        .lock_or_recover()
        .get_recording(&id)
        .map_err(|error| error.to_string())?
        .ok_or_else(crate::ui_lang::rec_missing)?;
    let transcript = Transcript::load(&state.dirs.transcript_path(&id))
        .map_err(|error| error.to_string())?
        .ok_or_else(|| {
            crate::ui_lang::msg(
                "La grabación todavía no tiene transcripción.",
                "This recording does not have a transcript yet.",
            )
        })?;
    let summary =
        Summary::load(&state.dirs.summary_path(&id)).map_err(|error| error.to_string())?;
    let destination = PathBuf::from(path.trim());
    if destination.as_os_str().is_empty() {
        return Err(crate::ui_lang::msg(
            "Elige un archivo de destino.",
            "Choose a destination file.",
        ));
    }
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    match format.as_str() {
        "md" => std::fs::write(
            &destination,
            render_markdown(&recording, &transcript, summary.as_ref(), &copy),
        )
        .map_err(|error| error.to_string())?,
        "docx" => write_docx(
            &destination,
            &recording,
            &transcript,
            summary.as_ref(),
            &copy,
        )?,
        "pdf" => write_pdf(
            &destination,
            &recording,
            &transcript,
            summary.as_ref(),
            &copy,
        )?,
        _ => {
            return Err(crate::ui_lang::msg(
                "Formato de exportación inválido.",
                "Invalid export format.",
            ))
        }
    }

    Ok(ExportResult {
        path: destination.to_string_lossy().into_owned(),
        format,
    })
}

fn document_lines(
    recording: &Recording,
    transcript: &Transcript,
    summary: Option<&Summary>,
    copy: &ExportCopy,
) -> Vec<String> {
    let mut lines = vec![
        recording.title.replace(['\r', '\n'], " "),
        format!(
            "{}: {}",
            copy.date,
            recording.started_at.format("%Y-%m-%d %H:%M")
        ),
        format!("{}: {} s", copy.duration, recording.duration_secs),
        String::new(),
    ];
    if let Some(summary) = summary {
        lines.push(copy.summary.into());
        lines.extend(summary.body.lines().map(str::to_string));
        lines.push(String::new());
    }
    lines.push(copy.transcript.into());
    for segment in &transcript.segments {
        let minutes = segment.start_ms.max(0) / 60_000;
        let seconds = segment.start_ms.max(0) / 1_000 % 60;
        lines.push(format!(
            "[{minutes}:{seconds:02}] {}: {}",
            speaker_line(segment, copy),
            segment.text.trim()
        ));
    }
    lines
}

fn render_markdown(
    recording: &Recording,
    transcript: &Transcript,
    summary: Option<&Summary>,
    copy: &ExportCopy,
) -> String {
    let mut out = format!(
        "# {}\n\n- {}: {}\n- {}: {} s\n\n",
        recording.title.replace(['\r', '\n'], " "),
        copy.date,
        recording.started_at.format("%Y-%m-%d %H:%M"),
        copy.duration,
        recording.duration_secs
    );
    if let Some(summary) = summary {
        out.push_str(&format!("## {}\n\n", copy.summary));
        out.push_str(summary.body.trim());
        out.push_str("\n\n");
    }
    out.push_str(&format!("## {}\n\n", copy.transcript));
    for segment in &transcript.segments {
        let minutes = segment.start_ms.max(0) / 60_000;
        let seconds = segment.start_ms.max(0) / 1_000 % 60;
        out.push_str(&format!(
            "**[{minutes}:{seconds:02}] {}:** {}\n\n",
            speaker_line(segment, copy),
            segment.text.trim()
        ));
    }
    out
}

fn write_docx(
    path: &Path,
    recording: &Recording,
    transcript: &Transcript,
    summary: Option<&Summary>,
    copy: &ExportCopy,
) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|error| error.to_string())?;
    let mut archive = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    archive
        .start_file("[Content_Types].xml", options)
        .map_err(|error| error.to_string())?;
    archive
        .write_all(br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#)
        .map_err(|error| error.to_string())?;
    archive
        .start_file("_rels/.rels", options)
        .map_err(|error| error.to_string())?;
    archive
        .write_all(br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#)
        .map_err(|error| error.to_string())?;
    archive
        .start_file("word/document.xml", options)
        .map_err(|error| error.to_string())?;
    let mut document = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>"#,
    );
    for (index, line) in document_lines(recording, transcript, summary, copy)
        .iter()
        .enumerate()
    {
        let style = if index == 0 {
            "<w:pPr><w:pStyle w:val=\"Title\"/></w:pPr>"
        } else {
            ""
        };
        document.push_str(&format!(
            "<w:p>{style}<w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
            escape_xml(line)
        ));
    }
    document.push_str("<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/><w:pgMar w:top=\"1080\" w:right=\"1080\" w:bottom=\"1080\" w:left=\"1080\"/></w:sectPr></w:body></w:document>");
    archive
        .write_all(document.as_bytes())
        .map_err(|error| error.to_string())?;
    archive.finish().map_err(|error| error.to_string())?;
    Ok(())
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn write_pdf(
    path: &Path,
    recording: &Recording,
    transcript: &Transcript,
    summary: Option<&Summary>,
    copy: &ExportCopy,
) -> Result<(), String> {
    let wrapped: Vec<String> = document_lines(recording, transcript, summary, copy)
        .into_iter()
        .flat_map(|line| wrap_line(&line, 92))
        .collect();
    let pages: Vec<&[String]> = if wrapped.is_empty() {
        vec![&[]]
    } else {
        wrapped.chunks(49).collect()
    };
    let mut objects: Vec<Vec<u8>> = vec![Vec::new(); 3 + pages.len() * 2];
    objects[0] = b"<< /Type /Catalog /Pages 2 0 R >>".to_vec();
    let kids = (0..pages.len())
        .map(|index| format!("{} 0 R", 4 + index * 2))
        .collect::<Vec<_>>()
        .join(" ");
    objects[1] = format!("<< /Type /Pages /Kids [{kids}] /Count {} >>", pages.len()).into_bytes();
    objects[2] =
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_vec();

    for (index, lines) in pages.iter().enumerate() {
        let page_id = 4 + index * 2;
        let content_id = page_id + 1;
        objects[page_id - 1] = format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents {content_id} 0 R >>").into_bytes();
        let mut stream = b"BT /F1 10 Tf 46 748 Td 14 TL\n".to_vec();
        for line in *lines {
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
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        pdf.extend_from_slice(object);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    pdf.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer << /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    std::fs::write(path, pdf).map_err(|error| error.to_string())
}

fn wrap_line(value: &str, max_chars: usize) -> Vec<String> {
    if value.is_empty() {
        return vec![String::new()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in value.split_whitespace() {
        if !current.is_empty() && current.chars().count() + word.chars().count() + 1 > max_chars {
            lines.push(current);
            current = String::new();
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

fn pdf_escape(value: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for character in value.chars() {
        let byte = match character {
            '\\' | '(' | ')' => {
                out.push(b'\\');
                character as u8
            }
            '\u{20AC}' => 0x80,
            '\u{2018}' => 0x91,
            '\u{2019}' => 0x92,
            '\u{201C}' => 0x93,
            '\u{201D}' => 0x94,
            '\u{2013}' => 0x96,
            '\u{2014}' => 0x97,
            character if (character as u32) <= 0xFF => character as u8,
            _ => b'?',
        };
        out.push(byte);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use atic_core::{Segment, Speaker};
    use chrono::Utc;

    fn fixture() -> (Recording, Transcript) {
        let recording = Recording::new(Utc::now());
        let transcript = Transcript {
            language: Some("es".into()),
            segments: vec![Segment {
                start_ms: 0,
                end_ms: 1_000,
                speaker: Speaker::Others,
                speaker_name: Some("José".into()),
                text: "Revisión de acuerdos".into(),
            }],
        };
        (recording, transcript)
    }

    #[test]
    fn wraps_long_lines_without_losing_words() {
        assert_eq!(
            wrap_line("uno dos tres cuatro", 8),
            ["uno dos", "tres", "cuatro"]
        );
    }

    #[test]
    fn escapes_xml_and_pdf_delimiters() {
        assert_eq!(escape_xml("A&B"), "A&amp;B");
        assert_eq!(pdf_escape("(hola)"), b"\\(hola\\)");
    }

    #[test]
    fn writes_valid_container_signatures() {
        let (recording, transcript) = fixture();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("resume-export-test-{nonce}"));
        std::fs::create_dir_all(&dir).unwrap();
        let docx = dir.join("test.docx");
        let pdf = dir.join("test.pdf");
        let copy = export_copy(false);
        write_docx(&docx, &recording, &transcript, None, &copy).unwrap();
        write_pdf(&pdf, &recording, &transcript, None, &copy).unwrap();
        assert!(std::fs::read(&docx).unwrap().starts_with(b"PK"));
        assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF-1.4"));
        std::fs::remove_dir_all(dir).ok();
    }
}
