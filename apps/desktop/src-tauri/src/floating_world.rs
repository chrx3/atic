//! Registro persistente de superficies Pearl Flow.
//!
//! La pill es el origen, pero cada herramienta vive en su propia ventana. Este
//! registro garantiza una sola instancia por herramienta, conserva geometría y
//! visibilidad entre reinicios y deja los grupos listos para la fase magnética.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use atic_core::MutexExt;
use tauri::{Manager, State};

pub const TOOL_WINDOW_PREFIX: &str = "pearl-tool-";
const TOOL_ROUTE: &str = "/floating-tool";
const WORLD_VERSION: u32 = 1;

const TOOLS: [&str; 6] = [
    "meetings",
    "dictation",
    "clipboard",
    "snippets",
    "captures",
    "agents",
];

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PersistedObject {
    pub id: String,
    pub x: i32,
    pub y: i32,
    pub width: f64,
    pub height: f64,
    pub visible: bool,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PersistedGroup {
    pub id: String,
    pub members: Vec<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct PersistedWorld {
    pub version: u32,
    pub objects: Vec<PersistedObject>,
    pub groups: Vec<PersistedGroup>,
}

impl Default for PersistedWorld {
    fn default() -> Self {
        Self {
            version: WORLD_VERSION,
            objects: Vec::new(),
            groups: Vec::new(),
        }
    }
}

pub struct FloatingWorldState {
    path: PathBuf,
    world: Mutex<PersistedWorld>,
    save_generation: AtomicU64,
}

impl FloatingWorldState {
    pub fn load(path: PathBuf) -> Self {
        let world = std::fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<PersistedWorld>(&raw).ok())
            .filter(|world| world.version == WORLD_VERSION)
            .unwrap_or_default();
        Self {
            path,
            world: Mutex::new(world),
            save_generation: AtomicU64::new(0),
        }
    }

    fn save(&self) -> Result<(), String> {
        let world = self.world.lock_or_recover().clone();
        let raw = serde_json::to_string_pretty(&world).map_err(|error| error.to_string())?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(&self.path, raw).map_err(|error| error.to_string())
    }
}

pub fn manage(app: &tauri::AppHandle, path: PathBuf) {
    app.manage(FloatingWorldState::load(path));
}

fn is_valid_tool(tool: &str) -> bool {
    TOOLS.contains(&tool)
}

fn tool_label(tool: &str) -> String {
    format!("{TOOL_WINDOW_PREFIX}{tool}")
}

fn tool_from_label(label: &str) -> Option<&str> {
    let tool = label.strip_prefix(TOOL_WINDOW_PREFIX)?;
    is_valid_tool(tool).then_some(tool)
}

fn tool_title(tool: &str) -> &'static str {
    match tool {
        "meetings" => "Pearl Flow Reuniones",
        "dictation" => "Pearl Flow Dictado",
        "clipboard" => "Pearl Flow Clipboard",
        "snippets" => "Pearl Flow Textos",
        "captures" => "Pearl Flow Capturas",
        "agents" => "Pearl Flow Agentes",
        _ => "Pearl Flow",
    }
}

fn tool_size(tool: &str) -> (f64, f64) {
    match tool {
        "clipboard" | "snippets" => (440.0, 520.0),
        "agents" => (640.0, 520.0),
        "dictation" => (360.0, 240.0),
        _ => (360.0, 260.0),
    }
}

fn initial_position(app: &tauri::AppHandle, width: f64, height: f64) -> (i32, i32) {
    let pill = app.get_webview_window("pill");
    let pill_rect = pill.as_ref().and_then(|window| {
        Some((
            window.outer_position().ok()?,
            window.outer_size().ok()?,
            window.scale_factor().ok()?,
        ))
    });
    let monitors = atic_capture::monitors::enumerate();
    let monitor = pill_rect
        .as_ref()
        .and_then(|(position, size, _)| {
            let cx = position.x + size.width as i32 / 2;
            let cy = position.y + size.height as i32 / 2;
            monitors
                .iter()
                .find(|monitor| monitor.work_area.contains(cx, cy))
        })
        .or_else(|| monitors.iter().find(|monitor| monitor.is_primary))
        .or_else(|| monitors.first());

    let Some(monitor) = monitor else {
        return (120, 120);
    };
    let work = monitor.work_area;
    let scale = pill_rect
        .as_ref()
        .map(|(_, _, scale)| *scale)
        .unwrap_or(1.0);
    let physical_width = (width * scale).round() as i32;
    let physical_height = (height * scale).round() as i32;
    let gap = (10.0 * scale).round() as i32;
    let visible_count = app
        .try_state::<FloatingWorldState>()
        .map(|state| {
            state
                .world
                .lock_or_recover()
                .objects
                .iter()
                .filter(|object| object.visible)
                .count() as i32
        })
        .unwrap_or(0);
    let (mut x, mut y) = pill_rect
        .as_ref()
        .map(|(position, size, _)| {
            let centered_x = position.x + (size.width as i32 - physical_width) / 2;
            let centered_y = position.y + (size.height as i32 - physical_height) / 2;
            match visible_count % 4 {
                0 => (position.x + size.width as i32 + gap, centered_y),
                1 => (position.x - physical_width - gap, centered_y),
                2 => (centered_x, position.y + size.height as i32 + gap),
                _ => (centered_x, position.y - physical_height - gap),
            }
        })
        .unwrap_or((work.x + gap, work.y + gap));
    x = x.clamp(work.x + gap, work.right() - physical_width - gap);
    y = y.clamp(work.y + gap, work.bottom() - physical_height - gap);
    (x, y)
}

fn schedule_save(app: &tauri::AppHandle) {
    let Some(state) = app.try_state::<FloatingWorldState>() else {
        return;
    };
    let generation = state.save_generation.fetch_add(1, Ordering::Relaxed) + 1;
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(240));
        let Some(state) = handle.try_state::<FloatingWorldState>() else {
            return;
        };
        if state.save_generation.load(Ordering::Relaxed) == generation {
            if let Err(error) = state.save() {
                tracing::warn!(%error, "no se pudo persistir FloatingWorld");
            }
        }
    });
}

fn set_visibility(app: &tauri::AppHandle, tool: &str, visible: bool) {
    let Some(state) = app.try_state::<FloatingWorldState>() else {
        return;
    };
    if let Some(object) = state
        .world
        .lock_or_recover()
        .objects
        .iter_mut()
        .find(|object| object.id == tool)
    {
        object.visible = visible;
    }
    schedule_save(app);
}

fn open_tool(app: &tauri::AppHandle, tool: &str, focus: bool) -> Result<(), String> {
    if !is_valid_tool(tool) {
        return Err(format!("herramienta flotante desconocida: {tool}"));
    }
    let label = tool_label(tool);
    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.set_always_on_top(true);
        window.show().map_err(|error| error.to_string())?;
        if focus {
            let _ = window.set_focus();
        }
        remember_window(app, &label);
        crate::liquid_world::emit_snapshot(app);
        return Ok(());
    }

    let state = app
        .try_state::<FloatingWorldState>()
        .ok_or("FloatingWorld no está inicializado")?;
    let saved = state
        .world
        .lock_or_recover()
        .objects
        .iter()
        .find(|object| object.id == tool)
        .cloned();
    let (default_width, default_height) = tool_size(tool);
    let width = saved
        .as_ref()
        .map(|object| object.width)
        .unwrap_or(default_width);
    let height = saved
        .as_ref()
        .map(|object| object.height)
        .unwrap_or(default_height);
    let (x, y) = saved
        .as_ref()
        .map(|object| (object.x, object.y))
        .unwrap_or_else(|| initial_position(app, width, height));

    let window =
        tauri::WebviewWindowBuilder::new(app, &label, tauri::WebviewUrl::App(TOOL_ROUTE.into()))
            .title(tool_title(tool))
            .inner_size(width, height)
            .min_inner_size(320.0, 220.0)
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .resizable(tool == "agents")
            .always_on_top(true)
            .skip_taskbar(true)
            .visible(false)
            .build()
            .map_err(|error| error.to_string())?;
    window
        .set_position(tauri::PhysicalPosition::new(x, y))
        .map_err(|error| error.to_string())?;
    crate::webview_tweaks::disable_browser_accelerator_keys(&window);
    window.show().map_err(|error| error.to_string())?;
    if focus {
        let _ = window.set_focus();
    }
    remember_window(app, &label);
    crate::liquid_world::emit_snapshot(app);
    Ok(())
}

#[tauri::command]
pub async fn open_floating_tool(app: tauri::AppHandle, tool: String) -> Result<(), String> {
    tracing::info!(%tool, "abriendo superficie Pearl Flow");
    if !is_valid_tool(&tool) {
        return Err(format!("herramienta flotante desconocida: {tool}"));
    }
    open_tool(&app, &tool, true).map_err(|error| {
        tracing::error!(%tool, %error, "no se pudo abrir superficie Pearl Flow");
        error
    })
}

#[tauri::command]
pub fn close_floating_tool(app: tauri::AppHandle, tool: String) -> Result<(), String> {
    if !is_valid_tool(&tool) {
        return Err(format!("herramienta flotante desconocida: {tool}"));
    }
    if let Some(window) = app.get_webview_window(&tool_label(&tool)) {
        window.hide().map_err(|error| error.to_string())?;
    }
    set_visibility(&app, &tool, false);
    crate::liquid_world::emit_snapshot(&app);
    Ok(())
}

#[tauri::command]
pub fn get_floating_world(state: State<'_, FloatingWorldState>) -> PersistedWorld {
    state.world.lock_or_recover().clone()
}

pub fn restore_open_tools(app: &tauri::AppHandle) {
    let Some(state) = app.try_state::<FloatingWorldState>() else {
        return;
    };
    let tools: Vec<String> = state
        .world
        .lock_or_recover()
        .objects
        .iter()
        .filter(|object| object.visible && is_valid_tool(&object.id))
        .map(|object| object.id.clone())
        .collect();
    for tool in tools {
        if let Err(error) = open_tool(app, &tool, false) {
            tracing::warn!(%tool, %error, "no se pudo restaurar superficie Pearl Flow");
        }
    }
}

pub fn is_tool_window(label: &str) -> bool {
    tool_from_label(label).is_some()
}

pub fn remember_window(app: &tauri::AppHandle, label: &str) {
    let Some(tool) = tool_from_label(label) else {
        return;
    };
    let Some(window) = app.get_webview_window(label) else {
        return;
    };
    let (Ok(position), Ok(size), Ok(scale)) = (
        window.outer_position(),
        window.outer_size(),
        window.scale_factor(),
    ) else {
        return;
    };
    let visible = window.is_visible().unwrap_or(true);
    let object = PersistedObject {
        id: tool.to_string(),
        x: position.x,
        y: position.y,
        width: f64::from(size.width) / scale,
        height: f64::from(size.height) / scale,
        visible,
    };
    let Some(state) = app.try_state::<FloatingWorldState>() else {
        return;
    };
    let mut world = state.world.lock_or_recover();
    if let Some(current) = world.objects.iter_mut().find(|current| current.id == tool) {
        *current = object;
    } else {
        world.objects.push(object);
    }
    drop(world);
    schedule_save(app);
}

pub fn hide_from_close_request(app: &tauri::AppHandle, label: &str) {
    let Some(tool) = tool_from_label(label) else {
        return;
    };
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.hide();
    }
    set_visibility(app, tool, false);
    crate::liquid_world::emit_snapshot(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_world_round_trips_and_rejects_unknown_tools() {
        let world = PersistedWorld {
            objects: vec![PersistedObject {
                id: "agents".into(),
                x: -720,
                y: 120,
                width: 640.0,
                height: 520.0,
                visible: true,
            }],
            groups: vec![PersistedGroup {
                id: "group:agents+pill".into(),
                members: vec!["pill".into(), "agents".into()],
            }],
            ..PersistedWorld::default()
        };
        let raw = serde_json::to_string(&world).unwrap();
        assert_eq!(serde_json::from_str::<PersistedWorld>(&raw).unwrap(), world);
        assert!(is_valid_tool("agents"));
        assert!(!is_valid_tool("unknown"));
    }
}
