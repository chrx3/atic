//! Tareas: la pestaña principal de Textos y lo primero de su vistazo.
//!
//! Se escriben a mano, o llegan solas desde los resúmenes de Reuniones: de
//! cada resumen se toman las viñetas de «Próximos pasos» o «Tareas» (las que
//! arman las plantillas de `crates/summarize`), sin «Ninguna», y quedan con la
//! reunión de la que salieron.
//!
//! Se guardan en `<datos de Atic>/pill/tasks.json`. De cada reunión se anota
//! el resumen ya leído (su fecha): un resumen nuevo de la misma reunión trae
//! sus tareas otra vez, pero una tarea borrada no vuelve con el mismo resumen.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: u64,
    pub text: String,
    pub done: bool,
    pub created_ms: i64,
    /// La reunión de la que salió (id y título), si salió de una.
    #[serde(default)]
    pub meeting: Option<(String, String)>,
    /// Cuándo se marcó hecha: cuenta para «hechas hoy».
    #[serde(default)]
    pub done_ms: Option<i64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Tasks {
    pub items: Vec<Task>,
    next: u64,
    /// Por reunión, la fecha del resumen del que ya se sacaron tareas.
    imported: HashMap<String, String>,
}

fn file() -> Option<PathBuf> {
    crate::paths::file("tasks.json")
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

impl Tasks {
    pub fn load() -> Self {
        let Some(path) = file() else {
            return Self::default();
        };
        let Ok(bytes) = std::fs::read(&path) else {
            return Self::default();
        };
        match serde_json::from_slice(&bytes) {
            Ok(tasks) => tasks,
            // El próximo guardado lo pisa: antes queda una copia (una vez por
            // sesión, para no pisarla con cada lectura).
            Err(error) => {
                static BAD_SAVED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
                if !BAD_SAVED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    tracing::error!(%error, "tareas: tasks.json no se pudo leer; queda una copia en tasks.json.bad");
                    let _ = std::fs::copy(&path, path.with_extension("json.bad"));
                }
                Self::default()
            }
        }
    }

    fn save(&self) {
        // Las pruebas no tocan los datos de verdad.
        if cfg!(test) {
            return;
        }
        let Some(path) = file() else {
            return;
        };
        if let Ok(bytes) = serde_json::to_vec_pretty(self) {
            // Un corte a mitad de `fs::write` dejaba el archivo truncado, y
            // `load` lo leía como una lista vacía.
            if let Err(error) = atic_core::write_atomic(&path, &bytes) {
                tracing::warn!(%error, "tareas: no se pudo guardar tasks.json");
            }
        }
    }

    /// Un cambio sobre lo que está en disco: un resumen recién hecho pudo
    /// traer tareas mientras Textos estaba abierto con su copia, y guardarla
    /// tal cual las borraba.
    fn update<R>(&mut self, change: impl FnOnce(&mut Tasks) -> R) -> R {
        if !cfg!(test) {
            *self = Tasks::load();
        }
        let out = change(self);
        self.save();
        out
    }

    fn push(&mut self, text: String, meeting: Option<(String, String)>) {
        self.next += 1;
        self.items.push(Task { id: self.next, text, done: false, created_ms: now_ms(), meeting, done_ms: None });
    }

    /// Una tarea escrita a mano.
    pub fn add(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        self.update(|tasks| tasks.push(text.to_string(), None));
    }

    pub fn toggle(&mut self, id: u64) {
        self.update(|tasks| {
            if let Some(task) = tasks.items.iter_mut().find(|t| t.id == id) {
                task.done = !task.done;
                task.done_ms = task.done.then(now_ms);
            }
        });
    }

    pub fn remove(&mut self, id: u64) {
        self.update(|tasks| tasks.items.retain(|t| t.id != id));
    }

    /// Las pendientes primero (las más nuevas arriba) y después las hechas.
    pub fn ordered(&self) -> Vec<&Task> {
        let mut out: Vec<&Task> = self.items.iter().collect();
        out.sort_by_key(|t| (t.done, std::cmp::Reverse(t.created_ms), std::cmp::Reverse(t.id)));
        out
    }

    /// «Borrar hechas».
    pub fn clear_done(&mut self) {
        self.update(|tasks| tasks.items.retain(|t| !t.done));
    }

    pub fn pending(&self) -> usize {
        self.items.iter().filter(|t| !t.done).count()
    }

    /// Hechas hoy, y el total del día: esas más las pendientes.
    pub fn today(&self) -> (usize, usize) {
        let midnight = chrono::Local::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .and_then(|start| start.and_local_timezone(chrono::Local).single())
            .map_or(0, |start| start.timestamp_millis());
        let done = self.items.iter().filter(|t| t.done && t.done_ms.is_some_and(|at| at >= midnight)).count();
        (done, done + self.pending())
    }

    /// Las tareas de un resumen, si no se leyó ya. `stamp` lo distingue de
    /// uno anterior de la misma reunión. Devuelve cuántas llegaron.
    pub fn import(&mut self, meeting: &str, title: &str, stamp: &str, body: &str) -> usize {
        if self.imported.get(meeting).is_some_and(|seen| seen == stamp) {
            return 0;
        }
        self.imported.insert(meeting.to_string(), stamp.to_string());
        let mut added = 0;
        for text in extract(body) {
            let repeated = self
                .items
                .iter()
                .any(|t| t.text == text && t.meeting.as_ref().is_some_and(|(id, _)| id == meeting));
            if !repeated {
                self.push(text, Some((meeting.to_string(), title.to_string())));
                added += 1;
            }
        }
        self.save();
        added
    }
}

/// Las viñetas bajo «Próximos pasos» o «Tareas» (en inglés, «Next steps»,
/// «Tasks» o «Action items»), sin la casilla de Markdown ni «Ninguna».
pub fn extract(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(heading) = trimmed.strip_prefix('#') {
            let heading = heading.trim_start_matches('#').trim().to_lowercase();
            inside = ["próximos pasos", "proximos pasos", "tareas", "next steps", "tasks", "action items"]
                .iter()
                .any(|name| heading.starts_with(name));
            continue;
        }
        if !inside {
            continue;
        }
        let Some(item) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) else {
            continue;
        };
        let item = item
            .trim_start_matches("[ ]")
            .trim_start_matches("[x]")
            .trim_start_matches("[X]")
            .trim()
            .trim_matches('*')
            .trim();
        let none = ["ninguna", "ninguno", "none", "…", "..."].iter().any(|n| item.trim_end_matches('.').eq_ignore_ascii_case(n));
        if !item.is_empty() && !none {
            out.push(item.to_string());
        }
    }
    out
}

/// Un resumen recién generado: sus tareas llegan al tiro (lo llama
/// `meetings::pipeline` apenas lo guarda, en su hilo).
pub fn import_summary(meeting: &str, title: &str, summary: &atic_core::Summary) -> usize {
    let mut tasks = Tasks::default();
    let stamp = summary.created_at.to_rfc3339();
    tasks.update(|tasks| tasks.import(meeting, title, &stamp, &summary.body))
}

/// Lee los resúmenes de Reuniones y trae sus tareas (en un hilo aparte: abre
/// la base de datos de las grabaciones).
pub fn import_meetings(tasks: &mut Tasks) -> usize {
    let Ok(source) = crate::meetings::data::Source::open() else {
        return 0;
    };
    let Ok(recordings) = source.list() else {
        return 0;
    };
    let mut added = 0;
    for recording in recordings {
        let Ok(Some(summary)) = source.summary(&recording.id) else {
            continue;
        };
        let stamp = summary.created_at.to_rfc3339();
        added += tasks.import(&recording.id, &recording.title, &stamp, &summary.body);
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saca_las_tareas_de_los_resumenes() {
        let minutes = "## Resumen\nHablamos.\n\n## Decisiones\n- Usar Rust\n\n## Próximos pasos\n- Carlos — revisar el build — viernes\n- **Probar en Windows**\n";
        assert_eq!(extract(minutes), ["Carlos — revisar el build — viernes", "Probar en Windows"]);
        let items = "## Acuerdos\n- Nada\n## Tareas\n- [ ] Ana — enviar el informe\n- [ ] Por definir — llamar al proveedor\n";
        assert_eq!(extract(items), ["Ana — enviar el informe", "Por definir — llamar al proveedor"]);
        assert!(extract("## Tareas\n- Ninguna\n").is_empty());
        assert!(extract("## Next steps\n- None.\n").is_empty());
    }

    #[test]
    fn el_mismo_resumen_no_las_trae_dos_veces() {
        let mut tasks = Tasks::default();
        let body = "## Tareas\n- [ ] Ana — enviar el informe\n";
        assert_eq!(tasks.import("r1", "Semanal", "2026-10-25", body), 1);
        assert_eq!(tasks.import("r1", "Semanal", "2026-10-25", body), 0);
        // Borrada, no vuelve con el mismo resumen; un resumen nuevo sí la trae.
        let id = tasks.items[0].id;
        tasks.remove(id);
        assert_eq!(tasks.import("r1", "Semanal", "2026-10-25", body), 0);
        assert_eq!(tasks.import("r1", "Semanal", "2026-10-26", body), 1);
        assert_eq!(tasks.items[0].meeting.as_ref().map(|(_, t)| t.as_str()), Some("Semanal"));
    }

    #[test]
    fn pendientes_primero() {
        let mut tasks = Tasks::default();
        tasks.add("una");
        tasks.add("dos");
        let first = tasks.items[0].id;
        tasks.toggle(first);
        let order: Vec<&str> = tasks.ordered().iter().map(|t| t.text.as_str()).collect();
        assert_eq!(order, ["dos", "una"]);
        assert_eq!(tasks.pending(), 1);
    }
}
