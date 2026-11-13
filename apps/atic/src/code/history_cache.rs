//! La última lista de conversaciones de cada espacio (`code-history.json`). Al abrir Atic
//! Code la barra la dibuja de inmediato, mientras el agente arranca y relee los archivos de
//! las sesiones (segundos, con muchas); cuando llega la lista nueva la reemplaza y se guarda.
//! Es solo un atajo: si el archivo falta o está roto, se ignora y la barra espera como antes.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::chat::RowState;
use super::SessionInfo;

const FILE: &str = "code-history.json";

/// Lo mismo que se le pide al agente (`limit` de `listSessions`).
pub const PER_SPACE: usize = 40;

#[derive(Serialize, Deserialize)]
struct Row {
    id: String,
    title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    modified: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    branch: Option<String>,
    /// El `endState` del agente («done», «asking», «stopped»), para la etiqueta de la fila.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    end: Option<String>,
    /// El agente ACP de la conversación (`codex`); sin él, es de Claude.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    agent: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct Saved {
    #[serde(default)]
    spaces: HashMap<u64, Vec<Row>>,
}

/// Las listas guardadas y el estado final de sus sesiones (lo que llena `session_ends`).
pub type Cached = (HashMap<u64, Vec<SessionInfo>>, HashMap<String, RowState>);

/// El inverso de `RowState::from_end`: solo esos tres estados vienen del agente.
fn end_name(state: RowState) -> Option<&'static str> {
    match state {
        RowState::Done => Some("done"),
        RowState::Asking => Some("asking"),
        RowState::Stopped => Some("stopped"),
        _ => None,
    }
}

/// Lee el archivo; si está roto, como si no hubiera nada.
pub fn decode(text: &str) -> Cached {
    let saved: Saved = serde_json::from_str(text).unwrap_or_default();
    let mut ends = HashMap::new();
    let history = saved
        .spaces
        .into_iter()
        .map(|(space, rows)| {
            let list = rows
                .into_iter()
                .take(PER_SPACE)
                .map(|row| {
                    if let Some(end) = row.end.as_deref().and_then(RowState::from_end) {
                        ends.insert(row.id.clone(), end);
                    }
                    SessionInfo { session_id: row.id, title: row.title, modified: row.modified, branch: row.branch, agent: row.agent.as_deref().and_then(super::acp::agent).map(|a| a.cli) }
                })
                .collect();
            (space, list)
        })
        .collect();
    (history, ends)
}

pub fn encode(history: &HashMap<u64, Vec<SessionInfo>>, ends: &HashMap<String, RowState>) -> String {
    let spaces = history
        .iter()
        .map(|(space, list)| {
            let rows = list
                .iter()
                .take(PER_SPACE)
                .map(|info| Row {
                    id: info.session_id.clone(),
                    title: info.title.clone(),
                    modified: info.modified,
                    branch: info.branch.clone(),
                    end: ends.get(&info.session_id).copied().and_then(end_name).map(str::to_string),
                    agent: info.agent.map(str::to_string),
                })
                .collect();
            (*space, rows)
        })
        .collect();
    serde_json::to_string(&Saved { spaces }).unwrap_or_default()
}

pub fn load() -> Cached {
    crate::paths::file(FILE).and_then(|path| std::fs::read_to_string(path).ok()).map(|text| decode(&text)).unwrap_or_default()
}

pub fn save(history: &HashMap<u64, Vec<SessionInfo>>, ends: &HashMap<String, RowState>) {
    // Una ventana abierta para capturas no guarda nada (como `Configs::save`).
    if std::env::var_os("CODE_OPEN").is_some() {
        return;
    }
    let Some(path) = crate::paths::file(FILE) else {
        return;
    };
    if let Err(error) = atic_core::write_atomic_str(&path, &encode(history, ends)) {
        tracing::warn!(%error, "atic code: no se guardó la lista de conversaciones");
    }
}

/// Ya hubo una lista guardada: Atic Code se usó en esta máquina.
pub fn exists() -> bool {
    crate::paths::file(FILE).is_some_and(|path| path.is_file())
}

/// Qué dibuja la barra en el lugar de las conversaciones de un espacio.
#[derive(Debug, PartialEq)]
pub enum ListShow {
    Rows,
    Loading,
    Empty,
}

/// `rows`: las filas que hay para dibujar; `known`: hay una lista, guardada o recién llegada.
/// No mira si se está pidiendo otra: con la guardada a la vista, «Cargando…» sobra, y un
/// espacio vacío diría «Cargando…» cada vez que se elige (`select_workspace` la vuelve a pedir).
pub fn list_show(rows: usize, known: bool) -> ListShow {
    if rows > 0 {
        ListShow::Rows
    } else if known {
        ListShow::Empty
    } else {
        ListShow::Loading
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(id: &str, title: &str) -> SessionInfo {
        SessionInfo { session_id: id.into(), title: title.into(), modified: Some(1_700_000_000_000.), branch: Some("dev".into()), agent: None }
    }

    #[test]
    fn la_lista_guardada_vuelve_igual() {
        let mut history = HashMap::new();
        history.insert(7, vec![info("a", "Arreglar la barra"), SessionInfo { session_id: "b".into(), title: "Otra".into(), modified: None, branch: None, agent: None }]);
        history.insert(super::super::LOOSE, vec![info("s", "Suelta")]);
        let mut ends = HashMap::new();
        ends.insert("a".to_string(), RowState::Asking);
        ends.insert("s".to_string(), RowState::Stopped);
        ends.insert("ajena".to_string(), RowState::Done);

        let (back, back_ends) = decode(&encode(&history, &ends));
        let list = &back[&7];
        assert_eq!(list.len(), 2);
        assert_eq!((list[0].session_id.as_str(), list[0].title.as_str(), list[0].modified, list[0].branch.as_deref()), ("a", "Arreglar la barra", Some(1_700_000_000_000.), Some("dev")));
        assert_eq!((list[1].session_id.as_str(), list[1].modified, list[1].branch.as_deref()), ("b", None, None));
        assert_eq!(back[&super::super::LOOSE][0].session_id, "s");
        assert_eq!(back_ends.get("a"), Some(&RowState::Asking));
        assert_eq!(back_ends.get("s"), Some(&RowState::Stopped));
        assert_eq!(back_ends.get("b"), None);
        assert_eq!(back_ends.get("ajena"), None, "solo los estados de sesiones de la lista");
    }

    #[test]
    fn la_de_codex_vuelve_con_su_agente() {
        let mut history = HashMap::new();
        history.insert(1, vec![SessionInfo { agent: Some("codex"), ..info("c", "Workflow de release") }, info("d", "De Claude")]);
        let back = decode(&encode(&history, &HashMap::new())).0;
        assert_eq!(back[&1][0].agent, Some("codex"));
        assert_eq!(back[&1][1].agent, None);
        // Un agente que esta versión no conoce se lee como de Claude, no rompe la lista.
        let (unknown, _) = decode(r#"{"spaces":{"1":[{"id":"x","title":"t","agent":"otro"}]}}"#);
        assert_eq!(unknown[&1][0].agent, None);
    }

    #[test]
    fn guarda_a_lo_mas_las_que_se_piden() {
        let mut history = HashMap::new();
        history.insert(1, (0..PER_SPACE + 5).map(|i| info(&i.to_string(), "x")).collect());
        assert_eq!(decode(&encode(&history, &HashMap::new())).0[&1].len(), PER_SPACE);
    }

    #[test]
    fn un_archivo_roto_no_trae_nada() {
        for text in ["", "roto", "[]", r#"{"spaces":{"no-es-numero":[]}}"#, r#"{"spaces":{"1":[{"title":"sin id"}]}}"#] {
            let (history, ends) = decode(text);
            assert!(history.is_empty() && ends.is_empty(), "{text:?}");
        }
        assert!(decode("{}").0.is_empty());
        let (history, _) = decode(r#"{"spaces":{"3":[{"id":"x","title":"t","end":"raro","otro":1}]},"version":9}"#);
        assert_eq!(history[&3][0].session_id, "x", "campos desconocidos no estorban");
    }

    #[test]
    fn con_filas_guardadas_no_dice_cargando() {
        // Recién abierta: la lista del archivo ya está en `history` mientras el agente responde.
        let (history, _) = decode(r#"{"spaces":{"7":[{"id":"a","title":"Guardada"}]}}"#);
        let rows = history.get(&7).map_or(0, Vec::len);
        assert_eq!(list_show(rows, history.contains_key(&7)), ListShow::Rows);
    }

    #[test]
    fn sin_lista_guardada_dice_cargando() {
        let (history, _) = decode("roto");
        assert_eq!(list_show(0, history.contains_key(&7)), ListShow::Loading);
        assert_eq!(list_show(0, true), ListShow::Empty, "una lista que llegó vacía");
    }
}
