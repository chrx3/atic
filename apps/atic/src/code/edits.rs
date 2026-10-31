//! Lo que cambia una herramienta de edición, listo para mostrarlo como diff: un
//! par «antes / después» por cada cambio. Edit es uno; MultiEdit, uno por cada
//! edición (en el orden en que se aplican); Write es todo agregado (antes vacío).
//! Las estadísticas +/− salen del mismo diff de líneas que dibuja el `DiffView`
//! de gpui-m3, no de una aproximación.

use gpui_m3::DiffStats;
use serde_json::Value;

/// Un cambio: el texto de antes y el de después.
#[derive(Clone, Debug, PartialEq)]
pub struct EditDiff {
    pub old: String,
    pub new: String,
}

fn text(value: &Value, name: &str) -> String {
    value.get(name).and_then(Value::as_str).unwrap_or_default().to_string()
}

/// Los cambios de una herramienta de edición (vacío si no lo es o le falta el input).
pub fn edit_diffs(tool: &str, input: &Value) -> Vec<EditDiff> {
    match tool {
        "Edit" if input.get("old_string").is_some() || input.get("new_string").is_some() => {
            vec![EditDiff { old: text(input, "old_string"), new: text(input, "new_string") }]
        }
        "MultiEdit" => input
            .get("edits")
            .and_then(Value::as_array)
            .map(|edits| edits.iter().map(|e| EditDiff { old: text(e, "old_string"), new: text(e, "new_string") }).collect())
            .unwrap_or_default(),
        "Write" if input.get("content").is_some() => vec![EditDiff { old: String::new(), new: text(input, "content") }],
        _ => Vec::new(),
    }
}

/// Las líneas agregadas y quitadas en total, con un diff de líneas real (por cambio,
/// y el diff queda en el caché de gpui-m3: dibujar la tarjeta no lo recalcula).
pub fn edit_stats(tool: &str, input: &Value) -> Option<DiffStats> {
    let diffs = edit_diffs(tool, input);
    if diffs.is_empty() {
        return None;
    }
    Some(diffs.iter().fold(DiffStats::default(), |sum, d| {
        let stats = gpui_m3::diff_cached(&d.old, &d.new).stats();
        DiffStats { added: sum.added + stats.added, removed: sum.removed + stats.removed }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn multiedit_da_un_diff_por_edicion_en_orden() {
        let input = json!({
            "file_path": "a.rs",
            "edits": [
                { "old_string": "uno\ndos", "new_string": "uno\nDOS" },
                { "old_string": "tres", "new_string": "tres\ncuatro" },
                { "old_string": "x" },
            ]
        });
        let diffs = edit_diffs("MultiEdit", &input);
        assert_eq!(diffs.len(), 3);
        assert_eq!(diffs[0], EditDiff { old: "uno\ndos".into(), new: "uno\nDOS".into() });
        assert_eq!(diffs[1].new, "tres\ncuatro");
        // Lo que falta cuenta como vacío.
        assert_eq!(diffs[2], EditDiff { old: "x".into(), new: String::new() });
        assert!(edit_diffs("MultiEdit", &json!({ "file_path": "a.rs" })).is_empty());
    }

    #[test]
    fn edit_es_un_cambio_y_write_es_todo_agregado() {
        let edit = edit_diffs("Edit", &json!({ "old_string": "a", "new_string": "b", "replace_all": true }));
        assert_eq!(edit, vec![EditDiff { old: "a".into(), new: "b".into() }]);
        let write = edit_diffs("Write", &json!({ "file_path": "n.txt", "content": "uno\ndos\n" }));
        assert_eq!(write, vec![EditDiff { old: String::new(), new: "uno\ndos\n".into() }]);
        // Una herramienta que no edita, o un Edit sin texto, no da diffs.
        assert!(edit_diffs("Read", &json!({ "old_string": "a", "new_string": "b" })).is_empty());
        assert!(edit_diffs("Edit", &json!({ "file_path": "a.rs" })).is_empty());
    }

    #[test]
    fn las_estadisticas_son_de_un_diff_real() {
        // Cambiar una línea del medio: +1 −1, no las 3 líneas de antes ni de después.
        let input = json!({ "old_string": "a\nb\nc", "new_string": "a\nB\nc" });
        assert_eq!(edit_stats("Edit", &input), Some(DiffStats { added: 1, removed: 1 }));
        // Agregar una línea en medio no quita nada.
        let input = json!({ "old_string": "a\nc", "new_string": "a\nb\nc" });
        assert_eq!(edit_stats("Edit", &input), Some(DiffStats { added: 1, removed: 0 }));
        // MultiEdit suma los cambios.
        let input = json!({ "edits": [
            { "old_string": "a\nb", "new_string": "a\nB" },
            { "old_string": "x", "new_string": "x\ny\nz" },
        ] });
        assert_eq!(edit_stats("MultiEdit", &input), Some(DiffStats { added: 3, removed: 1 }));
        // Write: todo agregado.
        assert_eq!(edit_stats("Write", &json!({ "content": "a\nb\nc\n" })), Some(DiffStats { added: 3, removed: 0 }));
        assert_eq!(edit_stats("Write", &json!({ "content": "" })), Some(DiffStats::default()));
        assert_eq!(edit_stats("Bash", &json!({ "command": "ls" })), None);
    }
}
