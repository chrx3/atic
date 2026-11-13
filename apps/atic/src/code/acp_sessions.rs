//! Las conversaciones de los agentes ACP que abrió Atic Code (`code-acp-sessions.json`).
//! Claude y Codex guardan las suyas donde el sidecar las lee; OpenCode, Cursor y Grok no
//! tienen cómo listarlas, y sin esto desaparecían de la barra al reiniciar. También guarda
//! el nombre que les puso el usuario (Codex incluido): `renameSession` es solo de Claude.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::SessionInfo;

const FILE: &str = "code-acp-sessions.json";

/// Como mucho estas: las más viejas se olvidan.
const MAX: usize = 300;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub session_id: String,
    pub cli: String,
    pub cwd: String,
    pub title: String,
    /// La última actividad, en milisegundos desde 1970 (como `lastModified` del sidecar).
    pub modified: f64,
    /// El usuario le puso el nombre: gana sobre el de la lista del agente.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub custom: bool,
}

fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.)
}

pub fn decode(text: &str) -> Vec<Entry> {
    serde_json::from_str(text).unwrap_or_default()
}

fn load() -> Vec<Entry> {
    crate::paths::file(FILE).and_then(|path| std::fs::read_to_string(path).ok()).map(|text| decode(&text)).unwrap_or_default()
}

fn save(entries: &[Entry]) {
    // Una ventana abierta para capturas no guarda nada (como `history_cache::save`).
    if std::env::var_os("CODE_OPEN").is_some() {
        return;
    }
    let Some(path) = crate::paths::file(FILE) else {
        return;
    };
    if let Err(error) = atic_core::write_atomic_str(&path, &serde_json::to_string(entries).unwrap_or_default()) {
        tracing::warn!(%error, "atic code: no se guardó la lista de conversaciones de los agentes");
    }
}

fn update(change: impl FnOnce(&mut Vec<Entry>)) {
    let mut entries = load();
    change(&mut entries);
    save(&entries);
}

/// Anota o pone al día una conversación (abierta o con un mensaje nuevo). El título solo
/// se anota si no había uno: el primero es el del primer mensaje y uno puesto a mano no
/// se pisa.
pub fn upsert(entries: &mut Vec<Entry>, session_id: &str, cli: &str, cwd: &Path, title: &str, at: f64) {
    match entries.iter_mut().find(|e| e.session_id == session_id) {
        Some(entry) => {
            entry.modified = at;
            if entry.title.trim().is_empty() {
                entry.title = title.to_string();
            }
        }
        None => entries.push(Entry { session_id: session_id.to_string(), cli: cli.to_string(), cwd: cwd.to_string_lossy().to_string(), title: title.to_string(), modified: at, custom: false }),
    }
    if entries.len() > MAX {
        entries.sort_by(|a, b| b.modified.total_cmp(&a.modified));
        entries.truncate(MAX);
    }
}

/// El nombre que le puso el usuario. Una de Codex puede no estar todavía: se anota.
pub fn rename_in(entries: &mut Vec<Entry>, session_id: &str, cli: &str, cwd: &Path, title: &str) {
    match entries.iter_mut().find(|e| e.session_id == session_id) {
        Some(entry) => entry.title = title.to_string(),
        None => entries.push(Entry { session_id: session_id.to_string(), cli: cli.to_string(), cwd: cwd.to_string_lossy().to_string(), title: title.to_string(), modified: now_ms(), custom: true }),
    }
    if let Some(entry) = entries.iter_mut().find(|e| e.session_id == session_id) {
        entry.custom = true;
    }
}

pub fn record(session_id: &str, cli: &str, cwd: &Path, title: &str) {
    let at = now_ms();
    update(|entries| upsert(entries, session_id, cli, cwd, title, at));
}

pub fn rename(session_id: &str, cli: &str, cwd: &Path, title: &str) {
    update(|entries| rename_in(entries, session_id, cli, cwd, title));
}

pub fn forget(session_id: &str) {
    update(|entries| entries.retain(|e| e.session_id != session_id));
}

/// Suma a la lista del sidecar de la carpeta `dir` las conversaciones anotadas aquí que no
/// trae, con su agente, y les pone el nombre que eligió el usuario. Queda ordenada por la
/// última actividad y con `limit` como mucho.
pub fn merge_into(list: &mut Vec<SessionInfo>, entries: &[Entry], dir: &Path, limit: usize) {
    for entry in entries.iter().filter(|e| crate::kit::workspaces::same(Path::new(&e.cwd), dir)) {
        match list.iter_mut().find(|info| info.session_id == entry.session_id) {
            Some(info) if entry.custom => info.title = entry.title.clone(),
            Some(_) => {}
            None => {
                // Una de Codex que su lista ya no trae (borrada o de otra carpeta) no vuelve.
                let Some(agent) = super::acp::agent(&entry.cli).filter(|a| a.cli != "codex") else {
                    continue;
                };
                let title = entry.title.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or(crate::i18n::t("pill.code.chat.fallbackTitle")).to_string();
                list.push(SessionInfo { session_id: entry.session_id.clone(), title, modified: Some(entry.modified), branch: None, agent: Some(agent.cli) });
            }
        }
    }
    list.sort_by(|a, b| b.modified.unwrap_or(0.).total_cmp(&a.modified.unwrap_or(0.)));
    list.truncate(limit);
}

/// Lo anotado de la carpeta `dir`, para sumarlo a la lista.
pub fn merge(list: &mut Vec<SessionInfo>, dir: &Path, limit: usize) {
    merge_into(list, &load(), dir, limit);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(id: &str, modified: f64, agent: Option<&'static str>) -> SessionInfo {
        SessionInfo { session_id: id.into(), title: format!("t-{id}"), modified: Some(modified), branch: None, agent }
    }

    #[test]
    fn se_anota_una_vez_y_el_titulo_no_se_pisa() {
        let mut entries = Vec::new();
        upsert(&mut entries, "s1", "opencode", Path::new("/p"), "Arreglar el parser", 10.);
        upsert(&mut entries, "s1", "opencode", Path::new("/p"), "Nueva conversación", 20.);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Arreglar el parser");
        assert_eq!(entries[0].modified, 20.);
        let text = serde_json::to_string(&entries).unwrap();
        assert_eq!(decode(&text), entries);
        assert!(decode("roto").is_empty());
    }

    #[test]
    fn renombrar_gana_sobre_el_primer_mensaje() {
        let mut entries = Vec::new();
        upsert(&mut entries, "s1", "grok", Path::new("/p"), "hola", 10.);
        rename_in(&mut entries, "s1", "grok", Path::new("/p"), "Mi nombre");
        upsert(&mut entries, "s1", "grok", Path::new("/p"), "otro", 30.);
        assert_eq!(entries[0].title, "Mi nombre");
        assert!(entries[0].custom);
        // Una de Codex que no estaba queda anotada con su nombre.
        rename_in(&mut entries, "c1", "codex", Path::new("/p"), "Codex con nombre");
        assert!(entries.iter().any(|e| e.session_id == "c1" && e.custom && e.title == "Codex con nombre"));
    }

    #[test]
    fn la_lista_suma_las_de_la_carpeta_con_su_agente() {
        let mut entries = Vec::new();
        upsert(&mut entries, "oc", "opencode", Path::new("/p/"), "De OpenCode", 50.);
        upsert(&mut entries, "cu", "cursor-agent", Path::new("/otra"), "De otra carpeta", 60.);
        upsert(&mut entries, "x", "desconocido", Path::new("/p"), "Agente que ya no existe", 70.);
        rename_in(&mut entries, "cx", "codex", Path::new("/p"), "Codex renombrada");
        rename_in(&mut entries, "gone", "codex", Path::new("/p"), "Codex borrada");
        let mut list = vec![info("cl", 40., None), info("cx", 30., Some("codex"))];
        merge_into(&mut list, &entries, Path::new("/p"), 40);
        let ids: Vec<&str> = list.iter().map(|i| i.session_id.as_str()).collect();
        assert_eq!(ids, ["oc", "cl", "cx"]);
        assert_eq!(list[0].agent, Some("opencode"));
        assert_eq!(list[0].title, "De OpenCode");
        assert_eq!(list[2].title, "Codex renombrada");
        // El límite vale también con las sumadas.
        merge_into(&mut list, &entries, Path::new("/p"), 2);
        assert_eq!(list.len(), 2);
    }
}
