//! Ping de hooks de Claude Code: un archivo en temp, no un segundo Atic.exe.
//!
//! El CLI manda JSON por stdin. `single_instance` no reenvía stdin, así que el
//! hook anexa la línea y el watcher la consume. No se escribe `settings.json`
//! ajeno: el fragmento se ofrece para pegar.

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::Value;

use super::presence::{self, AgentPresence, PresenceSource, PresenceStatus};

const BACKEND_ID: &str = "claude-code";
const BACKEND_NAME: &str = "Claude Code";

static OFFSET: Mutex<u64> = Mutex::new(0);

pub fn ping_path() -> PathBuf {
    std::env::temp_dir().join("atic-agent-ping.jsonl")
}

/// Línea de `settings.json` (hooks) para pegar. No se escribe sola.
pub fn hook_snippet() -> String {
    let command = hook_command(&ping_path());
    serde_json::to_string_pretty(&serde_json::json!({
        "hooks": {
            "PermissionRequest": [{
                "hooks": [{ "type": "command", "command": command.clone() }]
            }],
            "Notification": [{
                "matcher": "permission_prompt|idle_prompt|agent_needs_input",
                "hooks": [{ "type": "command", "command": command.clone() }]
            }],
            "Stop": [{
                "hooks": [{ "type": "command", "command": command }]
            }]
        }
    }))
    .unwrap_or_else(|_| "{}".into())
}

/// El comando que el CLI corre por cada hook, con el shell de su SO.
///
/// Claude Code ejecuta los hooks con el shell del sistema: un `powershell`
/// en macOS/Linux no existe y el ping nunca llegaba —por eso el chip no
/// podía decir «permiso»—. Los dos caminos anexan la línea JSON que el CLI
/// manda por stdin, tal cual; Atic la drena por offset.
#[cfg(windows)]
fn hook_command(path: &std::path::Path) -> String {
    let path_ps = path.to_string_lossy().replace('\\', "\\\\");
    format!(
        "powershell -NoProfile -NonInteractive -WindowStyle Hidden -Command \"$t=[Console]::In.ReadToEnd().Trim(); if($t){{ Add-Content -LiteralPath '{path_ps}' -Value $t -Encoding utf8 }}\""
    )
}

#[cfg(not(windows))]
fn hook_command(path: &std::path::Path) -> String {
    // La ruta va entre comillas dobles dentro del `sh -c`; una comilla simple
    // en el camino se escapa cerrando y reabriendo el literal.
    let path_sh = path.to_string_lossy().replace('\'', r"'\''");
    format!("sh -c 'printf \"%s\\n\" \"$(cat)\" >> \"{path_sh}\"'")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookPing {
    pub session_id: String,
    pub cwd: String,
    pub status: PresenceStatus,
    pub preview: Option<String>,
}

pub fn classify_hook(v: &Value) -> Option<HookPing> {
    let session_id = v
        .get("session_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())?
        .to_string();
    let cwd = v
        .get("cwd")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let event = v
        .get("hook_event_name")
        .and_then(Value::as_str)
        .unwrap_or("");
    let ntype = v
        .get("notification_type")
        .and_then(Value::as_str)
        .unwrap_or("");
    let status = match event {
        "PermissionRequest" => PresenceStatus::Waiting,
        "Notification"
            if matches!(
                ntype,
                "permission_prompt" | "idle_prompt" | "agent_needs_input"
            ) =>
        {
            PresenceStatus::Waiting
        }
        "Stop" => PresenceStatus::Ready,
        _ => return None,
    };
    let preview = v
        .get("last_assistant_message")
        .and_then(Value::as_str)
        .or_else(|| v.get("message").and_then(Value::as_str))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().take(120).collect());
    Some(HookPing {
        session_id,
        cwd,
        status,
        preview,
    })
}

pub fn apply_ping(ping: HookPing) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let mut presence = presence::get(&ping.session_id).unwrap_or(AgentPresence {
        id: ping.session_id.clone(),
        backend_id: BACKEND_ID.into(),
        backend_name: BACKEND_NAME.into(),
        cwd: ping.cwd.clone(),
        status: ping.status,
        preview: None,
        updated_at: now,
        window: None,
        source: PresenceSource::Hook,
    });
    presence.status = ping.status;
    presence.source = PresenceSource::Hook;
    presence.updated_at = now;
    if !ping.cwd.is_empty() {
        presence.cwd = ping.cwd;
    }
    if ping.preview.is_some() {
        presence.preview = ping.preview;
    }
    presence::upsert(presence);
}

pub fn drain() {
    let path = ping_path();
    let Ok(mut file) = OpenOptions::new().read(true).open(&path) else {
        return;
    };
    let Ok(len) = file.metadata().map(|m| m.len()) else {
        return;
    };
    let mut offset = OFFSET.lock().ok();
    let start = offset.as_deref().copied().unwrap_or(0);
    if len < start {
        if let Some(o) = offset.as_mut() {
            **o = 0;
        }
    }
    let start = offset.as_deref().copied().unwrap_or(0).min(len);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return;
    }
    let reader = BufReader::new(file);
    let mut consumed = start;
    for line in reader.lines() {
        let Ok(line) = line else {
            break;
        };
        consumed += line.len() as u64 + 1;
        if let Ok(v) = serde_json::from_str::<Value>(&line) {
            if let Some(ping) = classify_hook(&v) {
                apply_ping(ping);
            }
        }
    }
    if let Some(mut o) = offset {
        *o = consumed.min(len);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn permission_request_es_waiting() {
        let ping = classify_hook(&json!({
            "session_id": "s1",
            "cwd": "/x",
            "hook_event_name": "PermissionRequest"
        }))
        .unwrap();
        assert_eq!(ping.status, PresenceStatus::Waiting);
        assert_eq!(ping.session_id, "s1");
    }

    #[test]
    fn notification_permiso_es_waiting() {
        let ping = classify_hook(&json!({
            "session_id": "s1",
            "hook_event_name": "Notification",
            "notification_type": "permission_prompt",
            "message": "Claude needs your permission to use Bash"
        }))
        .unwrap();
        assert_eq!(ping.status, PresenceStatus::Waiting);
        assert_eq!(
            ping.preview.as_deref(),
            Some("Claude needs your permission to use Bash")
        );
    }

    #[test]
    fn stop_es_ready() {
        let ping = classify_hook(&json!({
            "session_id": "s1",
            "hook_event_name": "Stop",
            "last_assistant_message": "listo"
        }))
        .unwrap();
        assert_eq!(ping.status, PresenceStatus::Ready);
        assert_eq!(ping.preview.as_deref(), Some("listo"));
    }

    #[test]
    fn notification_irrelevante_se_ignora() {
        assert!(classify_hook(&json!({
            "session_id": "s1",
            "hook_event_name": "Notification",
            "notification_type": "auth_success"
        }))
        .is_none());
    }

    #[test]
    fn snippet_es_json_con_hooks() {
        let v: Value = serde_json::from_str(&hook_snippet()).unwrap();
        assert!(v.get("hooks").and_then(|h| h.get("Stop")).is_some());
        assert!(v
            .pointer("/hooks/PermissionRequest")
            .and_then(Value::as_array)
            .is_some());
    }

    #[test]
    fn el_comando_del_hook_usa_el_shell_de_su_so() {
        let path = ping_path();
        let command = hook_command(&path);
        #[cfg(windows)]
        {
            assert!(command.contains("powershell"), "{command}");
            let escapada = path.to_string_lossy().replace('\\', "\\\\");
            assert!(command.contains(&escapada), "{command}");
        }
        #[cfg(not(windows))]
        {
            assert!(command.starts_with("sh -c "), "{command}");
            assert!(
                command.contains(&path.to_string_lossy().to_string()),
                "{command}"
            );
        }
    }
}
