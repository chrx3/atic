//! Ping de hooks de Claude Code: un archivo en temp, no un segundo Atic.exe.
//!
//! El CLI manda JSON por stdin. `single_instance` no reenvía stdin, así que el
//! hook anexa la línea y el watcher la consume. No se escribe `settings.json`
//! ajeno: el fragmento se ofrece para pegar.
//!
//! Los hooks, la clasificación y la lectura por offset viven en
//! `atic_agents::hooks` (los comparte la pill GPUI). Las consolas de esta app
//! usan los archivos compartidos ([`Sink::Shared`]); acá se cruzan con la
//! presencia y lo pendiente.

use std::path::PathBuf;
use std::sync::Mutex;

use atic_agents::hooks::{self, HookPing, HookStatus, Sink};
use atic_core::MutexExt;

use super::presence::{self, AgentPresence, PresenceSource, PresenceStatus};

use super::console_prompts::{self, Cli};

pub use atic_agents::hooks::{kimi_config, CONSOLE_TOKEN_VAR};

static DRAIN: Mutex<Option<hooks::Drain>> = Mutex::new(None);

/// Línea de `settings.json` (hooks) para pegar. No se escribe sola.
pub fn hook_snippet() -> String {
    hooks::hook_settings(Sink::Shared)
}

/// Perfil `atic` de Codex (`codex -p atic`): una capa con los hooks de Atic
/// sobre la configuración del usuario, que no se toca. Solo pesa en las
/// consolas que lanza Atic. Devuelve el nombre del perfil.
pub fn codex_profile() -> Option<&'static str> {
    hooks::codex_home().and_then(|home| hooks::codex_profile(Sink::Shared, &home))
}

/// Los mismos hooks en un archivo, para lanzar `claude --settings <archivo>`
/// en las consolas de Atic sin tocar el `settings.json` del usuario. Así
/// Atic se entera de sus preguntas y el celular las puede contestar.
pub fn console_settings_path() -> Option<PathBuf> {
    hooks::console_settings_path(Sink::Shared)
}

fn presence_status(status: HookStatus) -> PresenceStatus {
    match status {
        HookStatus::Waiting => PresenceStatus::Waiting,
        HookStatus::Ready => PresenceStatus::Ready,
        HookStatus::Working => PresenceStatus::Working,
    }
}

pub(crate) fn apply_ping(ping: HookPing, cli: Cli) {
    let (backend_id, backend_name) = match cli {
        Cli::Claude => ("claude-code", "Claude Code"),
        Cli::Codex => ("codex", "Codex"),
        Cli::OpenCode => ("opencode", "OpenCode"),
        Cli::Kimi => ("kimi", "Kimi"),
    };
    let status = presence_status(ping.status);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let mut presence = presence::get(&ping.session_id).unwrap_or(AgentPresence {
        id: ping.session_id.clone(),
        backend_id: backend_id.into(),
        backend_name: backend_name.into(),
        cwd: ping.cwd.clone(),
        status,
        preview: None,
        updated_at: now,
        window: None,
        source: PresenceSource::Hook,
        activity: None,
    });
    presence.status = status;
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

fn handle(v: &serde_json::Value, cli: Cli) {
    console_prompts::observe(v, cli);
    if let Some(ping) = hooks::classify_hook(v) {
        apply_ping(ping, cli);
    }
}

/// Consume lo que anotaron los hooks de Claude, de Codex y de Kimi desde la
/// última vez.
pub fn drain() {
    let mut guard = DRAIN.lock_or_recover();
    let drain = guard.get_or_insert_with(Default::default);
    drain.file(&hooks::ping_path(), |v| handle(v, Cli::Claude));
    drain.file(&hooks::codex_ping_path(), |v| handle(v, Cli::Codex));
    drain.prefixed(&std::env::temp_dir(), hooks::KIMI_PREFIX, |token, v| {
        if let Some(session) = v.get("session_id").and_then(serde_json::Value::as_str) {
            console_prompts::link_session(session, token);
        }
        handle(v, Cli::Kimi);
    });
}
