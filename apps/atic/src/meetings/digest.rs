//! Las reuniones como las lleva Flip: título, cuándo, el resumen de la IA
//! por secciones y las tareas aparte, para ponerlas como lista de casillas.
//!
//! Así Flip no depende de cómo se guardan (la base de Atic, `summary.json`)
//! ni del formato del resumen: eso queda aquí, con el mismo `summary::parse`
//! que usa la ventana de Reuniones.

use chrono::Local;

use super::data::Source;
use super::summary::{self, Block, Kind};
use super::text;
use crate::i18n::t;

/// Una reunión en la lista para importar.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MeetingRow {
    pub id: String,
    pub title: String,
    /// «Jueves 19 de septiembre · 14:30 · 45 min».
    pub when: String,
    /// Ya tiene resumen de la IA.
    pub summarized: bool,
}

/// Una sección del resumen: título, sus párrafos y sus puntos.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MeetingSection {
    pub title: String,
    pub text: Vec<String>,
    /// Cada punto y si venía marcado (`- [x]`).
    pub items: Vec<(String, bool)>,
}

/// Una reunión lista para ponerla en el tablero.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MeetingDigest {
    pub title: String,
    pub when: String,
    /// Las secciones del resumen que no son tareas, cada una con su texto y
    /// sus puntos (que en el tablero son casillas).
    pub sections: Vec<MeetingSection>,
    /// Las tareas y próximos pasos, con si ya estaban marcadas.
    pub tasks: Vec<(String, bool)>,
}

fn when(recording: &atic_core::Recording) -> String {
    let local = recording.started_at.with_timezone(&Local);
    let today = Local::now().date_naive();
    format!(
        "{} · {} · {}",
        text::full_date(local.date_naive(), today),
        local.format("%H:%M"),
        text::duration(recording.duration_secs)
    )
}

/// Las reuniones, de la más nueva a la más vieja.
pub(crate) fn list() -> Vec<MeetingRow> {
    let Ok(source) = Source::open() else {
        return Vec::new();
    };
    source
        .list()
        .unwrap_or_default()
        .iter()
        .map(|recording| MeetingRow {
            id: recording.id.clone(),
            title: recording.title.clone(),
            when: when(recording),
            summarized: recording.status == atic_core::RecordingStatus::Summarized,
        })
        .collect()
}

/// Una reunión con su resumen ya ordenado para el tablero.
pub(crate) fn load(id: &str) -> Option<MeetingDigest> {
    let source = Source::open().ok()?;
    let recording = source.list().ok()?.into_iter().find(|r| r.id == id)?;
    let body = source.summary(id).ok().flatten().map(|s| s.body).unwrap_or_default();
    let (sections, tasks) = digest_summary(&body);
    Some(MeetingDigest { title: recording.title.clone(), when: when(&recording), sections, tasks })
}

/// «Ninguna», «Ninguno», «N/A»: lo que el modelo escribe cuando no hay nada.
pub(crate) fn is_nothing(text: &str) -> bool {
    // Los prompts piden «Ninguna» entre comillas, y a veces llega así.
    let quotes: &[char] = &['«', '»', '"', '“', '”', '\'', '‘', '’', '*', '_', '(', ')'];
    let t = text.trim().trim_matches(quotes).trim().trim_end_matches(['.', '…']).trim_matches(quotes).trim().to_lowercase();
    matches!(t.as_str(), "ninguna" | "ninguno" | "none" | "n/a" | "no hay" | "sin tareas" | "-" | "" | "no tasks" | "nothing")
        || t.starts_with("no se definieron")
        || t.starts_with("no se acordaron")
        || t.starts_with("no hubo tareas")
        || t.starts_with("no tasks were")
        || t.starts_with("no action items")
}

/// El resumen separado en secciones de texto y tareas.
pub(crate) fn digest_summary(body: &str) -> (Vec<MeetingSection>, Vec<(String, bool)>) {
    let mut sections = Vec::new();
    let mut tasks = Vec::new();
    for section in summary::parse(body, t("page.summary.title")) {
        if section.kind == Kind::Tasks {
            for block in &section.blocks {
                match block {
                    Block::List { items, .. } => tasks.extend(
                        items
                            .iter()
                            .filter(|item| !is_nothing(&item.text))
                            .map(|item| (item.text.clone(), item.checked.unwrap_or(false))),
                    ),
                    // Un párrafo suelto bajo «Tareas» también es una tarea
                    // (salvo el «Ninguna» de cuando no hay).
                    Block::Paragraph(text) if !is_nothing(text) => tasks.push((text.clone(), false)),
                    Block::Paragraph(_) => {}
                }
            }
            continue;
        }
        // Los párrafos quedan como texto; los puntos (con viñeta o
        // numerados), como casillas.
        let mut text = Vec::new();
        let mut items = Vec::new();
        for block in &section.blocks {
            match block {
                Block::Paragraph(paragraph) if !is_nothing(paragraph) => text.push(paragraph.clone()),
                Block::Paragraph(_) => {}
                Block::List { items: list, .. } => items.extend(
                    list.iter()
                        .filter(|item| !is_nothing(&item.text))
                        .map(|item| (item.text.clone(), item.checked.unwrap_or(false))),
                ),
            }
        }
        if !text.is_empty() || !items.is_empty() {
            sections.push(MeetingSection { title: section.title.clone(), text, items });
        }
    }
    (sections, tasks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_resumen_queda_en_secciones_y_tareas() {
        let body = "## Resumen\nSe revisaron los tres frentes.\n## Decisiones\n- Congelar funciones nuevas.\n## Próximos pasos\n- [ ] Confirmar fecha con móvil (viernes)\n- [x] Compartir la lista\n";
        let (sections, tasks) = digest_summary(body);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].title, "Resumen");
        assert_eq!(sections[0].text, vec!["Se revisaron los tres frentes.".to_string()]);
        // Los puntos de cualquier sección son casillas, no texto con «•».
        assert_eq!(sections[1].items, vec![("Congelar funciones nuevas.".to_string(), false)]);
        assert!(sections[1].text.is_empty());
        assert_eq!(
            tasks,
            vec![("Confirmar fecha con móvil (viernes)".to_string(), false), ("Compartir la lista".to_string(), true)]
        );
    }

    #[test]
    fn sin_tareas_no_inventa_casillas() {
        let (_, tasks) = digest_summary("## Resumen\nNada.\n## Tareas\nNinguna.\n");
        assert!(tasks.is_empty());
        let (_, tasks) = digest_summary("## Tareas\n- Ninguna\n");
        assert!(tasks.is_empty());
        let (_, tasks) = digest_summary("## Próximos pasos\n«Ninguna».\n## Tasks\n- “None”\n- No se definieron tareas.\n");
        assert!(tasks.is_empty(), "{tasks:?}");
    }

    #[test]
    fn las_secciones_se_reconocen_por_palabra() {
        let (sections, tasks) = digest_summary("## Repaso\n- Se vio el avance\n## Transacciones\n- Pago de marzo\n## Acciones\n- Llamar a Ana\n");
        assert_eq!(tasks, vec![("Llamar a Ana".to_string(), false)]);
        assert_eq!(sections.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(), ["Repaso", "Transacciones"]);
    }
}
