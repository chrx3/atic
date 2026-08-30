//! Presencia de un agente que corre en SU terminal.
//!
//! Atic no le habla: solo mira el rastro (JSONL, más adelante un hook) y lo
//! traduce a un snapshot chico para la pill. No reusa [`super::AgentDelta`]:
//! fabricar items o permisos falsos encendería la tarjeta de auth sin canal
//! para contestar.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// Un agente corriendo en su TUI. Atic solo mira.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentPresence {
    /// Clave estable. Claude Code: el id de sesión del CLI (nombre del `.jsonl`).
    pub id: String,
    pub backend_id: String,
    pub backend_name: String,
    pub cwd: String,
    pub status: PresenceStatus,
    /// Primera línea del último mensaje del agente, cruda y con tope ~120.
    /// El recorte a 28 lo hace la vista.
    pub preview: Option<String>,
    /// Última señal, epoch secs.
    pub updated_at: i64,
    /// Cómo enfocar la TUI. `None` = no se pudo resolver (MVP 1a).
    pub window: Option<PresenceWindow>,
    /// De dónde salió el estado. `waiting` SOLO es legítimo con `Hook`.
    pub source: PresenceSource,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PresenceStatus {
    Working,
    Waiting,
    Ready,
    Idle,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PresenceSource {
    Jsonl,
    Hook,
    Process,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PresenceWindow {
    pub pid: u32,
    pub hwnd: isize,
}

/// Lo que la pill ve: status + preview. `updated_at` y `window` no cuentan.
#[derive(Debug, Clone, PartialEq, Eq)]
struct VisibleKey {
    id: String,
    status: PresenceStatus,
    preview: Option<String>,
    hwnd: Option<isize>,
}

impl VisibleKey {
    fn from_presence(p: &AgentPresence) -> Self {
        Self {
            id: p.id.clone(),
            status: p.status,
            preview: p.preview.clone(),
            hwnd: p.window.as_ref().map(|w| w.hwnd),
        }
    }
}

/// Sin HWND/PID ni actividad reciente en JSONL/SQLite, no demover de golpe (PTY Codex).
const ORPHAN_GRACE_SECS: i64 = 90;

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Máximo un emit cada 400 ms, y solo si cambió algo que se ve.
pub struct Coalescer {
    last_emit: Option<Instant>,
    last_emitted: Vec<VisibleKey>,
    pending: Option<Vec<VisibleKey>>,
    min_interval: Duration,
}

impl Coalescer {
    pub fn new(min_interval: Duration) -> Self {
        Self {
            last_emit: None,
            last_emitted: Vec::new(),
            pending: None,
            min_interval,
        }
    }

    pub fn with_default_interval() -> Self {
        Self::new(Duration::from_millis(400))
    }

    fn visible_of(snapshot: &[AgentPresence]) -> Vec<VisibleKey> {
        let mut keys: Vec<_> = snapshot.iter().map(VisibleKey::from_presence).collect();
        keys.sort_by(|a, b| a.id.cmp(&b.id));
        keys
    }

    /// Anota el snapshot actual. Si no cambió lo visible, cancela el pendiente.
    pub fn note(&mut self, snapshot: &[AgentPresence]) {
        let visible = Self::visible_of(snapshot);
        if visible == self.last_emitted {
            self.pending = None;
        } else {
            self.pending = Some(visible);
        }
    }

    /// ¿Toca emitir ahora? Reloj inyectado para tests.
    pub fn take_emit(&mut self, now: Instant) -> bool {
        let Some(pending) = self.pending.take() else {
            return false;
        };
        if let Some(last) = self.last_emit {
            if now.saturating_duration_since(last) < self.min_interval {
                self.pending = Some(pending);
                return false;
            }
        }
        self.last_emitted = pending;
        self.last_emit = Some(now);
        true
    }
}

/// `waiting` solo es honesto si vino de un hook. Cualquier otra fuente miente.
pub fn normalize(mut presence: AgentPresence) -> AgentPresence {
    if presence.status == PresenceStatus::Waiting && presence.source != PresenceSource::Hook {
        presence.status = PresenceStatus::Working;
    }
    presence
}

struct Registry {
    items: HashMap<String, AgentPresence>,
    coalescer: Coalescer,
}

impl Registry {
    fn new() -> Self {
        Self {
            items: HashMap::new(),
            coalescer: Coalescer::with_default_interval(),
        }
    }

    fn snapshot(&self) -> Vec<AgentPresence> {
        let mut list: Vec<_> = self.items.values().cloned().collect();
        list.sort_by(|a, b| a.id.cmp(&b.id));
        list
    }

    /// Sin proceso ni HWND del agente, el JSONL/SQLite no debe seguir avisando.
    /// Presencias recién actualizadas se conservan (Codex en PTY no expone codex.exe).
    fn demote_orphans(&mut self) {
        let now = now_secs();
        let backends: Vec<String> = self
            .items
            .values()
            .map(|p| p.backend_id.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let live: HashMap<String, Vec<u32>> = backends
            .into_iter()
            .map(|b| (b.clone(), super::focus::agent_tui_pids(&b)))
            .collect();
        for p in self.items.values_mut() {
            let pids = live.get(&p.backend_id).map(Vec::as_slice).unwrap_or(&[]);
            let hwnd_ok = p.window.as_ref().is_some_and(|w| {
                super::focus::hwnd_alive(w.hwnd) && pids.contains(&w.pid)
            });
            if hwnd_ok {
                continue;
            }
            if pids.is_empty()
                && matches!(
                    p.status,
                    PresenceStatus::Ready | PresenceStatus::Working | PresenceStatus::Waiting
                )
                && now.saturating_sub(p.updated_at) >= ORPHAN_GRACE_SECS
            {
                p.status = PresenceStatus::Idle;
                p.window = None;
            }
        }
    }
}

static REGISTRY: Mutex<Option<Registry>> = Mutex::new(None);

fn with_registry<T>(f: impl FnOnce(&mut Registry) -> T) -> Option<T> {
    let mut guard = REGISTRY.lock().ok()?;
    let registry = guard.get_or_insert_with(Registry::new);
    Some(f(registry))
}

pub fn upsert(presence: AgentPresence) {
    let mut presence = normalize(presence);
    with_registry(|reg| {
        if let Some(old) = reg.items.get(&presence.id) {
            if presence.window.is_none() {
                if let Some(w) = old.window.clone() {
                    let pids = super::focus::agent_tui_pids(&presence.backend_id);
                    if super::focus::hwnd_alive(w.hwnd) && pids.contains(&w.pid) {
                        presence.window = Some(w);
                    }
                }
            }
            if old.status == PresenceStatus::Waiting
                && old.source == PresenceSource::Hook
                && presence.source != PresenceSource::Hook
                && presence.status != PresenceStatus::Ready
            {
                presence.status = PresenceStatus::Waiting;
                presence.source = PresenceSource::Hook;
            }
        }
        reg.items.insert(presence.id.clone(), presence);
    });
}

pub fn get(id: &str) -> Option<AgentPresence> {
    with_registry(|reg| reg.items.get(id).cloned()).flatten()
}

pub fn set_window(id: &str, window: PresenceWindow) {
    with_registry(|reg| {
        if let Some(p) = reg.items.get_mut(id) {
            p.window = Some(window);
        }
    });
}

/// Deja solo las presencias de este backend cuyos ids siguen vivos.
pub fn retain_backend(backend_id: &str, ids: &HashSet<String>) {
    with_registry(|reg| {
        reg.items
            .retain(|id, p| p.backend_id != backend_id || ids.contains(id));
    });
}

pub fn snapshot() -> Vec<AgentPresence> {
    with_registry(|reg| {
        reg.demote_orphans();
        reg.snapshot()
    })
    .unwrap_or_default()
}

/// Emite `agent-presence` si el coalescer lo permite.
pub fn publish(app: &AppHandle) {
    let Some(should) = with_registry(|reg| {
        reg.demote_orphans();
        let snap = reg.snapshot();
        reg.coalescer.note(&snap);
        let due = reg.coalescer.take_emit(Instant::now());
        (due, snap)
    }) else {
        return;
    };
    if should.0 {
        let _ = app.emit("agent-presence", should.1);
    }
}

/// Lista actual, para el montaje de la pill.
#[tauri::command]
pub fn agent_presences() -> Vec<AgentPresence> {
    if !super::PAGER_ENABLED {
        return Vec::new();
    }
    snapshot()
}

#[tauri::command]
pub fn agent_presence_focus(id: String) -> super::focus::PresenceFocusResult {
    super::focus::focus_id(&id)
}

#[tauri::command]
pub fn agent_presence_bind(app: AppHandle, id: String) -> super::focus::PresenceFocusResult {
    let result = super::focus::bind_id(&id);
    publish(&app);
    result
}

#[tauri::command]
pub fn agent_presence_hook_snippet() -> String {
    super::ping::hook_snippet()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn presence(id: &str, status: PresenceStatus, preview: Option<&str>, at: i64) -> AgentPresence {
        AgentPresence {
            id: id.into(),
            backend_id: "claude-code".into(),
            backend_name: "Claude Code".into(),
            cwd: "/x".into(),
            status,
            preview: preview.map(|s| s.into()),
            updated_at: at,
            window: None,
            source: PresenceSource::Jsonl,
        }
    }

    #[test]
    fn waiting_desde_jsonl_pasa_a_working() {
        let p = normalize(AgentPresence {
            source: PresenceSource::Jsonl,
            status: PresenceStatus::Waiting,
            ..presence("s", PresenceStatus::Waiting, None, 1)
        });
        assert_eq!(p.status, PresenceStatus::Working);
        assert_eq!(p.source, PresenceSource::Jsonl);
    }

    #[test]
    fn jsonl_no_pisa_waiting_de_hook() {
        upsert(AgentPresence {
            source: PresenceSource::Hook,
            status: PresenceStatus::Waiting,
            ..presence("hook-wait-1", PresenceStatus::Waiting, None, 1)
        });
        upsert(presence("hook-wait-1", PresenceStatus::Working, None, 2));
        let got = get("hook-wait-1").unwrap();
        assert_eq!(got.status, PresenceStatus::Waiting);
        assert_eq!(got.source, PresenceSource::Hook);
    }

    #[test]
    fn waiting_desde_hook_se_conserva() {
        let p = normalize(AgentPresence {
            source: PresenceSource::Hook,
            status: PresenceStatus::Waiting,
            ..presence("s", PresenceStatus::Waiting, None, 1)
        });
        assert_eq!(p.status, PresenceStatus::Waiting);
    }

    #[test]
    fn coalescer_un_emit_en_400ms() {
        let t0 = Instant::now();
        let mut c = Coalescer::new(Duration::from_millis(400));
        c.note(&[presence("a", PresenceStatus::Working, None, 1)]);
        assert!(c.take_emit(t0));
        c.note(&[presence("a", PresenceStatus::Ready, Some("listo"), 2)]);
        assert!(!c.take_emit(t0 + Duration::from_millis(100)));
        assert!(c.take_emit(t0 + Duration::from_millis(400)));
    }

    #[test]
    fn coalescer_updated_at_solo_no_emite() {
        let t0 = Instant::now();
        let mut c = Coalescer::new(Duration::from_millis(400));
        c.note(&[presence("a", PresenceStatus::Ready, Some("hola"), 1)]);
        assert!(c.take_emit(t0));
        c.note(&[presence("a", PresenceStatus::Ready, Some("hola"), 99)]);
        assert!(!c.take_emit(t0 + Duration::from_millis(500)));
    }
}
