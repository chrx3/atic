//! Registro persistente de superficies Pearl Flow.
//!
//! La pill es el origen, pero cada herramienta vive en su propia ventana. Este
//! registro garantiza una sola instancia por herramienta, conserva geometría y
//! visibilidad entre reinicios y deja los grupos listos para la fase magnética.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use atic_core::MutexExt;
use tauri::{Emitter, Manager, PhysicalPosition, State};

pub const TOOL_WINDOW_PREFIX: &str = "pearl-tool-";
const TOOL_ROUTE: &str = "/floating-tool";
const WORLD_VERSION: u32 = 1;
const MAGNETIC_GAP: f64 = 26.0;
const FUSED_GAP: f64 = 7.0;

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
    #[serde(default)]
    pub detached: bool,
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
    drag: Mutex<Option<DragSession>>,
    drag_generation: AtomicU64,
    save_generation: AtomicU64,
}

#[derive(Clone, Debug)]
struct DragMember {
    id: String,
    x: i32,
    y: i32,
}

#[derive(Clone, Debug)]
struct DragSession {
    anchor: String,
    anchor_x: i32,
    anchor_y: i32,
    members: Vec<DragMember>,
}

#[derive(Clone, Copy, Debug)]
struct PhysicalRect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
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
            drag: Mutex::new(None),
            drag_generation: AtomicU64::new(0),
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

fn object_from_label(label: &str) -> Option<&str> {
    if label == "pill" {
        Some("pill")
    } else {
        tool_from_label(label)
    }
}

fn object_label(id: &str) -> Option<String> {
    if id == "pill" {
        Some("pill".into())
    } else if is_valid_tool(id) {
        Some(tool_label(id))
    } else {
        None
    }
}

fn normalized_members(mut members: Vec<String>) -> Vec<String> {
    members.sort();
    members.dedup();
    members
}

fn group_members(world: &PersistedWorld, id: &str) -> Vec<String> {
    world
        .groups
        .iter()
        .find(|group| group.members.iter().any(|member| member == id))
        .map(|group| group.members.clone())
        .unwrap_or_else(|| vec![id.to_string()])
}

fn merge_groups(world: &mut PersistedWorld, first: &str, second: &str) -> bool {
    let first_members = group_members(world, first);
    let second_members = group_members(world, second);
    let members = normalized_members(
        first_members
            .iter()
            .chain(second_members.iter())
            .cloned()
            .collect(),
    );
    if members.len() < 2 || first_members == members && second_members == members {
        return false;
    }
    let member_set = members.iter().cloned().collect::<HashSet<_>>();
    world.groups.retain(|group| {
        !group
            .members
            .iter()
            .any(|member| member_set.contains(member))
    });
    world.groups.push(PersistedGroup {
        id: format!("group:{}", members.join("+")),
        members: members.clone(),
    });
    for object in &mut world.objects {
        if member_set.contains(&object.id) {
            object.detached = false;
        }
    }
    true
}

fn detach_member(world: &mut PersistedWorld, id: &str) -> bool {
    let mut changed = false;
    for group in &mut world.groups {
        let before = group.members.len();
        group.members.retain(|member| member != id);
        changed |= group.members.len() != before;
    }
    world.groups.retain(|group| group.members.len() > 1);
    if let Some(object) = world.objects.iter_mut().find(|object| object.id == id) {
        object.detached = true;
    }
    changed
}

fn rect_gap(first: PhysicalRect, second: PhysicalRect) -> f64 {
    let first_right = first.x + first.width;
    let first_bottom = first.y + first.height;
    let second_right = second.x + second.width;
    let second_bottom = second.y + second.height;
    let dx = (first.x - second_right).max(second.x - first_right).max(0) as f64;
    let dy = (first.y - second_bottom)
        .max(second.y - first_bottom)
        .max(0) as f64;
    dx.hypot(dy)
}

fn attraction_delta(moving: PhysicalRect, target: PhysicalRect, desired_gap: i32) -> (i32, i32) {
    if let Some(delta) = overlap_separation_delta(moving, target, desired_gap) {
        return delta;
    }
    let moving_right = moving.x + moving.width;
    let moving_bottom = moving.y + moving.height;
    let target_right = target.x + target.width;
    let target_bottom = target.y + target.height;
    let dx = if moving_right <= target.x {
        target.x - moving_right - desired_gap
    } else if target_right <= moving.x {
        target_right - moving.x + desired_gap
    } else {
        0
    };
    let dy = if moving_bottom <= target.y {
        target.y - moving_bottom - desired_gap
    } else if target_bottom <= moving.y {
        target_bottom - moving.y + desired_gap
    } else {
        0
    };
    (dx, dy)
}

fn overlap_separation_delta(
    moving: PhysicalRect,
    target: PhysicalRect,
    desired_gap: i32,
) -> Option<(i32, i32)> {
    let moving_right = moving.x + moving.width;
    let moving_bottom = moving.y + moving.height;
    let target_right = target.x + target.width;
    let target_bottom = target.y + target.height;
    let overlaps_x = moving.x < target_right && moving_right > target.x;
    let overlaps_y = moving.y < target_bottom && moving_bottom > target.y;
    if !overlaps_x || !overlaps_y {
        return None;
    }
    [
        (target.x - desired_gap - moving_right, 0),
        (target_right + desired_gap - moving.x, 0),
        (0, target.y - desired_gap - moving_bottom),
        (0, target_bottom + desired_gap - moving.y),
    ]
    .into_iter()
    .min_by_key(|(dx, dy)| dx.abs() + dy.abs())
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

fn emit_world_changed(app: &tauri::AppHandle) {
    let Some(state) = app.try_state::<FloatingWorldState>() else {
        return;
    };
    let snapshot = state.world.lock_or_recover().clone();
    let _ = app.emit("floating-world-changed", snapshot);
}

fn live_rect(app: &tauri::AppHandle, id: &str) -> Option<PhysicalRect> {
    let label = object_label(id)?;
    let window = app.get_webview_window(&label)?;
    if !window.is_visible().unwrap_or(false) {
        return None;
    }
    let position = window.outer_position().ok()?;
    let size = window.outer_size().ok()?;
    let scale = window.scale_factor().ok()?;
    let logical_width = f64::from(size.width) / scale;
    let logical_height = f64::from(size.height) / scale;
    let (inset_x, inset_y, width, height) = if id == "pill" && logical_width <= 80.0 {
        (
            ((logical_width - 48.0) * scale / 2.0).round() as i32,
            (8.0 * scale).round() as i32,
            (48.0 * scale).round() as i32,
            (48.0 * scale).round() as i32,
        )
    } else if id == "pill" && logical_height < 400.0 {
        (
            ((logical_width - 154.0) * scale / 2.0).round() as i32,
            (32.0 * scale).round() as i32,
            (154.0 * scale).round() as i32,
            (50.0 * scale).round() as i32,
        )
    } else {
        (0, 0, size.width as i32, size.height as i32)
    };
    Some(PhysicalRect {
        x: position.x + inset_x,
        y: position.y + inset_y,
        width,
        height,
    })
}

fn live_position(app: &tauri::AppHandle, id: &str) -> Option<(i32, i32)> {
    let label = object_label(id)?;
    let window = app.get_webview_window(&label)?;
    let position = window.outer_position().ok()?;
    Some((position.x, position.y))
}

fn set_object_position(app: &tauri::AppHandle, id: &str, x: i32, y: i32) {
    if let Some(label) = object_label(id) {
        if let Some(window) = app.get_webview_window(&label) {
            let _ = window.set_position(PhysicalPosition::new(x, y));
        }
    }
    if id != "pill" {
        if let Some(state) = app.try_state::<FloatingWorldState>() {
            if let Some(object) = state
                .world
                .lock_or_recover()
                .objects
                .iter_mut()
                .find(|object| object.id == id)
            {
                object.x = x;
                object.y = y;
            }
        }
    }
}

fn clamp_window_to_work_area(window: &tauri::WebviewWindow) {
    let (Ok(position), Ok(size)) = (window.outer_position(), window.outer_size()) else {
        return;
    };
    let monitors = atic_capture::monitors::enumerate();
    let center_x = position.x + size.width as i32 / 2;
    let center_y = position.y + size.height as i32 / 2;
    let monitor = monitors
        .iter()
        .find(|monitor| monitor.work_area.contains(center_x, center_y))
        .or_else(|| monitors.iter().find(|monitor| monitor.is_primary))
        .or_else(|| monitors.first());
    let Some(monitor) = monitor else {
        return;
    };
    let work = monitor.work_area;
    let margin = 10;
    let max_x = (work.right() - size.width as i32 - margin).max(work.x + margin);
    let max_y = (work.bottom() - size.height as i32 - margin).max(work.y + margin);
    let x = position.x.clamp(work.x + margin, max_x);
    let y = position.y.clamp(work.y + margin, max_y);
    if x != position.x || y != position.y {
        let _ = window.set_position(PhysicalPosition::new(x, y));
    }
}

fn repair_group_overlap(app: &tauri::AppHandle, id: &str) {
    let Some(moving) = live_rect(app, id) else {
        return;
    };
    let members = app
        .try_state::<FloatingWorldState>()
        .map(|state| group_members(&state.world.lock_or_recover(), id))
        .unwrap_or_default();
    let desired_gap = object_label(id)
        .and_then(|label| app.get_webview_window(&label))
        .and_then(|window| window.scale_factor().ok())
        .map(|scale| (FUSED_GAP * scale).round() as i32)
        .unwrap_or(FUSED_GAP as i32);
    let separation = members
        .iter()
        .filter(|member| member.as_str() != id)
        .filter_map(|member| overlap_separation_delta(moving, live_rect(app, member)?, desired_gap))
        .min_by_key(|(dx, dy)| dx.abs() + dy.abs());
    let (Some((dx, dy)), Some((x, y))) = (separation, live_position(app, id)) else {
        return;
    };
    set_object_position(app, id, x + dx, y + dy);
}

fn translate_members(app: &tauri::AppHandle, members: &[String], dx: i32, dy: i32) {
    if dx == 0 && dy == 0 {
        return;
    }
    let positions = members
        .iter()
        .filter_map(|id| live_position(app, id).map(|(x, y)| (id.clone(), x + dx, y + dy)))
        .collect::<Vec<_>>();
    for (id, x, y) in positions {
        set_object_position(app, &id, x, y);
    }
}

fn attach_objects(app: &tauri::AppHandle, first: &str, second: &str) -> bool {
    let Some(state) = app.try_state::<FloatingWorldState>() else {
        return false;
    };
    let changed = merge_groups(&mut state.world.lock_or_recover(), first, second);
    if changed {
        schedule_save(app);
        emit_world_changed(app);
    }
    changed
}

fn attach_to_pill_by_default(app: &tauri::AppHandle, tool: &str) {
    let Some(state) = app.try_state::<FloatingWorldState>() else {
        return;
    };
    let should_attach = {
        let world = state.world.lock_or_recover();
        let detached = world
            .objects
            .iter()
            .find(|object| object.id == tool)
            .is_some_and(|object| object.detached);
        let grouped = world
            .groups
            .iter()
            .any(|group| group.members.iter().any(|member| member == tool));
        !detached && !grouped
    };
    if should_attach && live_rect(app, "pill").is_some() {
        attach_objects(app, "pill", tool);
    }
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

pub(crate) fn open_tool(app: &tauri::AppHandle, tool: &str, focus: bool) -> Result<(), String> {
    if !is_valid_tool(tool) {
        return Err(format!("herramienta flotante desconocida: {tool}"));
    }
    let label = tool_label(tool);
    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.set_always_on_top(true);
        window.show().map_err(|error| error.to_string())?;
        clamp_window_to_work_area(&window);
        if focus {
            let _ = window.set_focus();
        }
        attach_to_pill_by_default(app, tool);
        repair_group_overlap(app, tool);
        clamp_window_to_work_area(&window);
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
    clamp_window_to_work_area(&window);
    if focus {
        let _ = window.set_focus();
    }
    attach_to_pill_by_default(app, tool);
    repair_group_overlap(app, tool);
    clamp_window_to_work_area(&window);
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

fn valid_object(id: &str) -> bool {
    id == "pill" || is_valid_tool(id)
}

fn nearest_attachment(app: &tauri::AppHandle, moving_id: &str) -> Option<(String, i32, i32)> {
    let moving = live_rect(app, moving_id)?;
    let scale = object_label(moving_id)
        .and_then(|label| app.get_webview_window(&label))
        .and_then(|window| window.scale_factor().ok())
        .unwrap_or(1.0);
    let max_gap = MAGNETIC_GAP * scale;
    let desired_gap = (FUSED_GAP * scale).round() as i32;
    let own_members = app
        .try_state::<FloatingWorldState>()
        .map(|state| group_members(&state.world.lock_or_recover(), moving_id))
        .unwrap_or_else(|| vec![moving_id.to_string()])
        .into_iter()
        .collect::<HashSet<_>>();
    let mut ids = vec!["pill".to_string()];
    if let Some(state) = app.try_state::<FloatingWorldState>() {
        ids.extend(
            state
                .world
                .lock_or_recover()
                .objects
                .iter()
                .filter(|object| object.visible)
                .map(|object| object.id.clone()),
        );
    }
    ids.into_iter()
        .filter(|id| !own_members.contains(id))
        .filter_map(|id| {
            let target = live_rect(app, &id)?;
            let gap = rect_gap(moving, target);
            (gap <= max_gap).then(|| {
                let (dx, dy) = attraction_delta(moving, target, desired_gap);
                (id, dx, dy, gap)
            })
        })
        .min_by(|first, second| first.3.total_cmp(&second.3))
        .map(|(id, dx, dy, _)| (id, dx, dy))
}

#[cfg(windows)]
fn primary_pointer_down() -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
    unsafe { GetAsyncKeyState(VK_LBUTTON as i32) < 0 }
}

#[cfg(not(windows))]
fn primary_pointer_down() -> bool {
    false
}

fn finish_drag(app: &tauri::AppHandle, object_id: &str) -> Result<(), String> {
    let state = app
        .try_state::<FloatingWorldState>()
        .ok_or("FloatingWorld no está inicializado")?;
    let was_active = state
        .drag
        .lock_or_recover()
        .take()
        .is_some_and(|drag| drag.anchor == object_id);
    if !was_active {
        return Ok(());
    }
    if let Some((target, dx, dy)) = nearest_attachment(app, object_id) {
        let members = group_members(&state.world.lock_or_recover(), object_id);
        translate_members(app, &members, dx, dy);
        attach_objects(app, object_id, &target);
    }
    schedule_save(app);
    crate::liquid_world::emit_snapshot(app);
    Ok(())
}

fn finish_drag_after_pointer_release(app: &tauri::AppHandle, object_id: String) {
    let Some(state) = app.try_state::<FloatingWorldState>() else {
        return;
    };
    let generation = state.drag_generation.fetch_add(1, Ordering::Relaxed) + 1;
    let handle = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(55));
        let Some(state) = handle.try_state::<FloatingWorldState>() else {
            return;
        };
        if state.drag_generation.load(Ordering::Relaxed) != generation {
            return;
        }
        if primary_pointer_down() {
            continue;
        }
        if let Err(error) = finish_drag(&handle, &object_id) {
            tracing::warn!(%object_id, %error, "no se pudo cerrar el arrastre Pearl Flow");
        }
        return;
    });
}

#[tauri::command]
pub fn begin_floating_drag(
    app: tauri::AppHandle,
    object_id: String,
    detach: bool,
) -> Result<(), String> {
    if !valid_object(&object_id) {
        return Err(format!("objeto Pearl Flow desconocido: {object_id}"));
    }
    let state = app
        .try_state::<FloatingWorldState>()
        .ok_or("FloatingWorld no está inicializado")?;
    let members = {
        let mut world = state.world.lock_or_recover();
        if detach {
            detach_member(&mut world, &object_id);
        }
        group_members(&world, &object_id)
    };
    let (anchor_x, anchor_y) = live_position(&app, &object_id)
        .ok_or_else(|| format!("{object_id} no tiene una ventana visible"))?;
    let members = members
        .into_iter()
        .filter_map(|id| {
            let (x, y) = live_position(&app, &id)?;
            Some(DragMember { id, x, y })
        })
        .collect();
    *state.drag.lock_or_recover() = Some(DragSession {
        anchor: object_id.clone(),
        anchor_x,
        anchor_y,
        members,
    });
    // Oculta la piel global antes de entregar el drag a Windows. Esperar al
    // primer WindowEvent::Moved deja un frame con el cuerpo anterior visible.
    crate::liquid_world::emit_snapshot(&app);
    if detach {
        schedule_save(&app);
        emit_world_changed(&app);
    }
    finish_drag_after_pointer_release(&app, object_id);
    Ok(())
}

#[tauri::command]
pub fn end_floating_drag(app: tauri::AppHandle, object_id: String) -> Result<(), String> {
    if !valid_object(&object_id) {
        return Err(format!("objeto Pearl Flow desconocido: {object_id}"));
    }
    if let Some(state) = app.try_state::<FloatingWorldState>() {
        state.drag_generation.fetch_add(1, Ordering::Relaxed);
    }
    finish_drag(&app, &object_id)
}

#[tauri::command]
pub fn detach_floating_object(app: tauri::AppHandle, object_id: String) -> Result<(), String> {
    if !valid_object(&object_id) {
        return Err(format!("objeto Pearl Flow desconocido: {object_id}"));
    }
    let state = app
        .try_state::<FloatingWorldState>()
        .ok_or("FloatingWorld no está inicializado")?;
    let changed = detach_member(&mut state.world.lock_or_recover(), &object_id);
    if changed {
        schedule_save(&app);
        emit_world_changed(&app);
        crate::liquid_world::emit_snapshot(&app);
    }
    Ok(())
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
        detached: false,
    };
    let Some(state) = app.try_state::<FloatingWorldState>() else {
        return;
    };
    let mut world = state.world.lock_or_recover();
    if let Some(current) = world.objects.iter_mut().find(|current| current.id == tool) {
        let detached = current.detached;
        *current = object;
        current.detached = detached;
    } else {
        world.objects.push(object);
    }
    drop(world);
    schedule_save(app);
}

/// Grupos que requieren cuellos líquidos entre ventanas independientes.
///
/// Cada superficie pinta siempre su propio cuerpo. El compositor solo necesita
/// conocer las relaciones entre miembros para dibujar el material que ocupa el
/// espacio vacío entre ellos.
pub fn merged_groups(app: &tauri::AppHandle) -> Vec<PersistedGroup> {
    let Some(state) = app.try_state::<FloatingWorldState>() else {
        return Vec::new();
    };
    let world = state.world.lock_or_recover();
    world
        .groups
        .iter()
        .filter(|group| group.members.len() >= 2)
        .cloned()
        .collect()
}

/// `true` mientras haya un arrastre en curso. El compositor lo consulta para
/// pintar la piel sin interpolar mientras dura.
pub fn is_dragging(app: &tauri::AppHandle) -> bool {
    app.try_state::<FloatingWorldState>()
        .is_some_and(|state| state.drag.lock_or_recover().is_some())
}

pub fn handle_window_moved(app: &tauri::AppHandle, label: &str, position: PhysicalPosition<i32>) {
    let Some(id) = object_from_label(label) else {
        return;
    };
    if id != "pill" {
        remember_window(app, label);
    }
    let drag = app
        .try_state::<FloatingWorldState>()
        .and_then(|state| state.drag.lock_or_recover().clone());
    if let Some(drag) = drag.filter(|drag| drag.anchor == id) {
        let dx = position.x - drag.anchor_x;
        let dy = position.y - drag.anchor_y;
        for member in drag.members.iter().filter(|member| member.id != id) {
            set_object_position(app, &member.id, member.x + dx, member.y + dy);
        }
    }
    crate::liquid_world::emit_snapshot(app);
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
                detached: false,
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

    #[test]
    fn groups_merge_and_detach_without_single_member_shells() {
        let mut world = PersistedWorld {
            objects: vec![
                PersistedObject {
                    id: "meetings".into(),
                    x: 0,
                    y: 0,
                    width: 360.0,
                    height: 260.0,
                    visible: true,
                    detached: false,
                },
                PersistedObject {
                    id: "agents".into(),
                    x: 400,
                    y: 0,
                    width: 640.0,
                    height: 520.0,
                    visible: true,
                    detached: false,
                },
            ],
            ..PersistedWorld::default()
        };
        assert!(merge_groups(&mut world, "pill", "meetings"));
        assert!(merge_groups(&mut world, "meetings", "agents"));
        assert_eq!(world.groups.len(), 1);
        assert_eq!(world.groups[0].members, vec!["agents", "meetings", "pill"]);
        assert!(detach_member(&mut world, "meetings"));
        assert_eq!(world.groups[0].members, vec!["agents", "pill"]);
        assert!(world.objects[0].detached);
        assert!(detach_member(&mut world, "agents"));
        assert!(world.groups.is_empty());
    }

    #[test]
    fn attraction_uses_nearest_edges() {
        let moving = PhysicalRect {
            x: 100,
            y: 100,
            width: 48,
            height: 48,
        };
        let target = PhysicalRect {
            x: 160,
            y: 90,
            width: 360,
            height: 260,
        };
        assert_eq!(rect_gap(moving, target), 12.0);
        assert_eq!(attraction_delta(moving, target, 7), (5, 0));
    }
}
