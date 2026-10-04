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

use super::console_prompts::Cli;

static OFFSET: Mutex<u64> = Mutex::new(0);
static CODEX_OFFSET: Mutex<u64> = Mutex::new(0);

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
            "PostToolUse": [{
                // Las que pueden pedir permiso: así se sabe que ya se contestó.
                // Leer y buscar quedan fuera; son las más seguidas y no preguntan.
                "matcher": "AskUserQuestion|Bash|PowerShell|Edit|MultiEdit|Write|NotebookEdit|WebFetch|WebSearch|mcp__.*",
                "hooks": [{ "type": "command", "command": command.clone() }]
            }],
            "UserPromptSubmit": [{
                "hooks": [{ "type": "command", "command": command.clone() }]
            }],
            "Stop": [{
                "hooks": [{ "type": "command", "command": command }]
            }]
        }
    }))
    .unwrap_or_else(|_| "{}".into())
}

/// Dónde anotan los hooks de Codex. Archivo aparte: sus ids de sesión son de
/// Codex y la presencia tiene que saber de qué CLI es cada uno.
pub fn codex_ping_path() -> PathBuf {
    std::env::temp_dir().join("atic-codex-ping.jsonl")
}

/// El comando de los hooks de Codex. En Windows Codex los corre con
/// PowerShell (no con `sh` como Claude), y su `[Console]::In` lee en la página
/// de códigos de la consola: sin fijar UTF-8 los acentos llegaban rotos.
fn codex_hook_command(path: &std::path::Path) -> String {
    #[cfg(windows)]
    {
        let path_ps = path.to_string_lossy().replace('\'', "''");
        format!(
            "[Console]::InputEncoding=[Text.UTF8Encoding]::new($false); [Console]::In.ReadToEnd() | Add-Content -LiteralPath '{path_ps}' -Encoding utf8"
        )
    }
    #[cfg(not(windows))]
    {
        hook_command(path)
    }
}

/// Perfil `atic` de Codex (`codex -p atic`): una capa con los hooks de Atic
/// sobre la configuración del usuario, que no se toca. Solo pesa en las
/// consolas que lanza Atic. Devuelve el nombre del perfil.
pub fn codex_profile() -> Option<&'static str> {
    let home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| super::watch_codex::sessions_root().and_then(|s| s.parent().map(PathBuf::from)))?;
    if !home.is_dir() {
        return None;
    }
    // Una cadena JSON es una cadena básica de TOML válida.
    let command = serde_json::to_string(&codex_hook_command(&codex_ping_path())).ok()?;
    let hook = |matcher: Option<&str>| {
        let matcher = matcher.map(|m| format!("matcher = \"{m}\", ")).unwrap_or_default();
        format!("[{{ {matcher}hooks = [{{ type = \"command\", command = {command} }}] }}]")
    };
    // Sin `PostToolUse` para todas las herramientas: cada hook es un PowerShell
    // que arranca, y Codex corre muchos comandos. Un permiso contestado en el
    // PC se borra con el `Stop` del turno.
    let body = format!(
        "# Lo escribe Atic para sus consolas (`codex -p atic`). Se regenera solo.\n\
         [hooks]\n\
         PermissionRequest = {}\n\
         PreToolUse = {}\n\
         PostToolUse = {}\n\
         UserPromptSubmit = {}\n\
         Stop = {}\n",
        hook(None),
        hook(Some("request_user_input")),
        hook(Some("request_user_input")),
        hook(None),
        hook(None),
    );
    std::fs::write(home.join("atic.config.toml"), body).ok()?;
    Some("atic")
}

/// Los mismos hooks en un archivo, para lanzar `claude --settings <archivo>`
/// en las consolas de Atic sin tocar el `settings.json` del usuario. Así
/// Atic se entera de sus preguntas y el celular las puede contestar.
pub fn console_settings_path() -> Option<PathBuf> {
    let path = std::env::temp_dir().join("atic-claude-hooks.json");
    std::fs::write(&path, hook_snippet()).ok()?;
    Some(path)
}

/// El comando que el CLI corre por cada hook.
///
/// Claude Code corre los hooks con `sh`, también en Windows (Git Bash). Un
/// comando de PowerShell no sobrevivía: bash expandía su `$t` a nada antes de
/// pasárselo y el ping nunca llegaba. Anexa la línea JSON que el CLI manda por
/// stdin, tal cual; Atic la drena por offset.
fn hook_command(path: &std::path::Path) -> String {
    // Git Bash entiende `C:/…`; las barras invertidas se las comería.
    let path = path.to_string_lossy().replace('\\', "/");
    // La ruta va entre comillas dobles dentro del `sh -c`; una comilla simple
    // en el camino se escapa cerrando y reabriendo el literal.
    let path_sh = path.replace('\'', r"'\''");
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
        // Contestó la pregunta o mandó otro mensaje: vuelve a trabajar.
        "PostToolUse" | "UserPromptSubmit" => PresenceStatus::Working,
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

pub(crate) fn apply_ping(ping: HookPing, cli: Cli) {
    let (backend_id, backend_name) = match cli {
        Cli::Claude => ("claude-code", "Claude Code"),
        Cli::Codex => ("codex", "Codex"),
        Cli::OpenCode => ("opencode", "OpenCode"),
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let mut presence = presence::get(&ping.session_id).unwrap_or(AgentPresence {
        id: ping.session_id.clone(),
        backend_id: backend_id.into(),
        backend_name: backend_name.into(),
        cwd: ping.cwd.clone(),
        status: ping.status,
        preview: None,
        updated_at: now,
        window: None,
        source: PresenceSource::Hook,
        activity: None,
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

/// Consume lo que anotaron los hooks de Claude y de Codex desde la última vez.
pub fn drain() {
    drain_file(&ping_path(), &OFFSET, Cli::Claude);
    drain_file(&codex_ping_path(), &CODEX_OFFSET, Cli::Codex);
}

fn drain_file(path: &std::path::Path, offset_lock: &Mutex<u64>, cli: Cli) {
    let Ok(mut file) = OpenOptions::new().read(true).open(path) else {
        return;
    };
    let Ok(len) = file.metadata().map(|m| m.len()) else {
        return;
    };
    let mut offset = offset_lock.lock().ok();
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
        // PowerShell abre el archivo con BOM.
        if let Ok(v) = serde_json::from_str::<Value>(line.trim_start_matches('\u{feff}')) {
            super::console_prompts::observe(&v, cli);
            if let Some(ping) = classify_hook(&v) {
                apply_ping(ping, cli);
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
    fn el_hook_de_codex_es_powershell_en_utf8() {
        let command = codex_hook_command(&codex_ping_path());
        #[cfg(windows)]
        {
            assert!(command.contains("UTF8Encoding"), "{command}");
            assert!(command.contains("Add-Content"), "{command}");
        }
        assert!(command.contains("atic-codex-ping.jsonl"), "{command}");
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
        assert!(command.starts_with("sh -c "), "{command}");
        let ruta = path.to_string_lossy().replace('\\', "/");
        assert!(command.contains(&ruta), "{command}");
        assert!(!command.contains("powershell"), "{command}");
    }
}
