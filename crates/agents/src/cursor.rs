//! Cursor TUI (`cursor-agent`).
//!
//! `~/.cursor/chats` es el IDE: no se mira. `acp-sessions` es un store de
//! blobs (a veces cifrado) sin marcador de fin de turno, así que el estado
//! honesto es «proceso vivo»: una sesión trabajando por cada `cursor-agent`
//! que no sea hijo de `Cursor.exe`. Encontrar los procesos es de cada app
//! (es del sistema operativo); aquí van las reglas.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde_json::Value;

use crate::seen::{Seen, SeenStatus};

/// El ejecutable del TUI.
pub const EXE: &str = "cursor-agent.exe";

pub fn acp_root() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|h| PathBuf::from(h).join(".cursor").join("acp-sessions"))
}

pub fn cwd_from_meta(v: &Value) -> Option<String> {
    v.get("cwd")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn mtime_secs(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// La carpeta de la sesión ACP más reciente con `store.db` (las carpetas
/// vacías son inicios fallidos).
pub fn recent_cwd(root: &Path) -> Option<String> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return None;
    };
    let mut best: Option<(u64, String)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let store = path.join("store.db");
        if !store.exists() {
            continue;
        }
        let mtime = mtime_secs(&store);
        let cwd = std::fs::read_to_string(path.join("meta.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| cwd_from_meta(&v))
            .unwrap_or_default();
        if best.as_ref().is_none_or(|(t, _)| mtime >= *t) {
            best = Some((mtime, cwd));
        }
    }
    best.map(|(_, cwd)| cwd).filter(|s| !s.is_empty())
}

/// Una sesión trabajando por proceso vivo.
pub fn sessions(pids: &[u32], cwd: Option<&str>, now: i64) -> Vec<Seen> {
    pids.iter()
        .map(|pid| Seen {
            id: format!("cursor-{pid}"),
            cwd: cwd.unwrap_or("").to_string(),
            status: SeenStatus::Working,
            preview: None,
            updated: now,
        })
        .collect()
}

/// Los pids de `exe` que no descienden de ninguno de `skip_exe` (el IDE, la
/// app que los lanzó por su cuenta). `processes`: pid, padre y nombre del
/// ejecutable en minúsculas.
pub fn pids_outside(processes: &[(u32, u32, String)], exe: &str, skip_exe: &[&str]) -> Vec<u32> {
    let pids: Vec<u32> = processes.iter().filter(|(_, _, name)| name == exe).map(|(pid, _, _)| *pid).collect();
    exclude_ide_children(&pids, processes, skip_exe)
}

/// Quita procesos cuyo ancestro es el IDE / Atic: no son TUI.
pub fn exclude_ide_children(agent_pids: &[u32], processes: &[(u32, u32, String)], skip_exe: &[&str]) -> Vec<u32> {
    let tree: HashMap<u32, (u32, String)> =
        processes.iter().map(|(pid, ppid, name)| (*pid, (*ppid, name.clone()))).collect();
    agent_pids
        .iter()
        .copied()
        .filter(|pid| {
            let mut current = *pid;
            let mut seen = HashSet::new();
            while seen.insert(current) {
                let Some((ppid, name)) = tree.get(&current) else {
                    break;
                };
                if current != *pid && skip_exe.contains(&name.as_str()) {
                    return false;
                }
                if *ppid == 0 || *ppid == current {
                    break;
                }
                current = *ppid;
            }
            true
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_saca_cwd() {
        let v = serde_json::json!({ "schemaVersion": 1, "cwd": "C:\\\\repo" });
        assert_eq!(cwd_from_meta(&v).as_deref(), Some("C:\\\\repo"));
    }

    #[test]
    fn sin_proceso_no_hay_sesion() {
        assert!(sessions(&[], Some("/x"), 10).is_empty());
    }

    #[test]
    fn un_pid_es_una_sesion_trabajando() {
        let list = sessions(&[42], Some("/repo"), 10);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "cursor-42");
        assert_eq!(list[0].status, SeenStatus::Working);
        assert_eq!(list[0].cwd, "/repo");
    }

    #[test]
    fn recent_cwd_ignora_sin_store() {
        let nonce = std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("atic-watch-cursor-{nonce}"));
        let empty = root.join("empty-uuid");
        std::fs::create_dir_all(&empty).unwrap();
        std::fs::write(empty.join("meta.json"), r#"{"cwd":"/skip"}"#).unwrap();
        let live = root.join("live-uuid");
        std::fs::create_dir_all(&live).unwrap();
        std::fs::write(live.join("meta.json"), r#"{"cwd":"/tui"}"#).unwrap();
        std::fs::write(live.join("store.db"), b"x").unwrap();
        assert_eq!(recent_cwd(&root).as_deref(), Some("/tui"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn los_hijos_del_ide_no_cuentan() {
        let procs = vec![
            (1, 0, "cursor.exe".to_string()),
            (2, 1, "cursor-agent.exe".to_string()),
            (3, 0, "cmd.exe".to_string()),
            (4, 3, "cursor-agent.exe".to_string()),
        ];
        assert_eq!(pids_outside(&procs, EXE, &["cursor.exe"]), vec![4]);
    }
}
