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

/// Una reunión lista para ponerla en el tablero.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MeetingDigest {
    pub title: String,
    pub when: String,
    /// Las secciones del resumen que no son tareas: título y sus líneas
    /// (párrafos tal cual; los puntos, con «•»).
    pub sections: Vec<(String, Vec<String>)>,
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
fn is_nothing(text: &str) -> bool {
    let t = text.trim().trim_end_matches('.').to_lowercase();
    matches!(t.as_str(), "ninguna" | "ninguno" | "none" | "n/a" | "no hay" | "sin tareas" | "-")
}

/// El resumen separado en secciones de texto y tareas.
pub(crate) fn digest_summary(body: &str) -> (Vec<(String, Vec<String>)>, Vec<(String, bool)>) {
    let mut sections = Vec::new();
    let mut tasks = Vec::new();
    for section in summary::parse(body, "Resumen") {
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
        let lines: Vec<String> = section
            .blocks
            .iter()
            .flat_map(|block| match block {
                Block::Paragraph(text) => vec![text.clone()],
                Block::List { ordered, items } => items
                    .iter()
                    .enumerate()
                    .map(|(i, item)| if *ordered { format!("{}. {}", i + 1, item.text) } else { format!("• {}", item.text) })
                    .collect(),
            })
            .filter(|line| !is_nothing(line.trim_start_matches("• ")))
            .collect();
        if !lines.is_empty() {
            sections.push((section.title.clone(), lines));
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
        assert_eq!(sections[0].0, "Resumen");
        assert_eq!(sections[1].1, vec!["• Congelar funciones nuevas.".to_string()]);
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
    }
}
