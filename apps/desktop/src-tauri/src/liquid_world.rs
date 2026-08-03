//! Spike del compositor Pearl Flow persistente.
//!
//! La piel vive en una ventana click-through del tamaño del monitor. La pill y
//! cada herramienta conservan ventanas pequeñas e interactivas por encima. El
//! objetivo de este módulo no es ser todavía el registro final: prueba el
//! contrato más riesgoso (piel global + foco/clics nativos independientes).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{Emitter, Manager};

const WORLD_PREFIX: &str = "liquid-world-";
const OBJECT_PREFIX: &str = "liquid-spike-";
const ROUTE: &str = "/lab/liquid-world";
static SNAPSHOT_REVISION: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiquidWorldObject {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub radius: f64,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiquidWorldSnapshot {
    pub world_label: String,
    pub width: f64,
    pub height: f64,
    pub scale: f64,
    pub revision: u64,
    pub emitted_at_ms: u128,
    pub debug: bool,
    /// Hay un arrastre en curso. La piel debe seguir a la ventana sin
    /// interpolar: el sistema mueve el WebView del contenido al instante y una
    /// transición acá se lee como que el fondo va detrás del texto.
    pub dragging: bool,
    /// Relaciones lógicas entre las superficies. El WebView crea un árbol de
    /// puentes cortos y lo une con los cuerpos en un solo campo SDF.
    pub groups: Vec<crate::floating_world::PersistedGroup>,
    pub objects: Vec<LiquidWorldObject>,
}

#[cfg(windows)]
fn ensure_window(
    app: &tauri::AppHandle,
    label: &str,
    title: &str,
    focusable: bool,
) -> Result<tauri::WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(label) {
        return Ok(window);
    }

    tauri::WebviewWindowBuilder::new(app, label, tauri::WebviewUrl::App(ROUTE.into()))
        .title(title)
        .inner_size(320.0, 240.0)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focusable(focusable)
        .visible(false)
        .build()
        .map_err(|error| error.to_string())
}

/// Windows vuelve a inscribir la ventana en la barra de tareas al mostrarla, así
/// que el `skip_taskbar(true)` del builder no sobrevive al primer `show()`. El
/// compositor es una superficie de fondo, no una ventana del usuario, y debe
/// quedar fuera de la barra y de Alt+Tab de forma estable: eso lo da
/// WS_EX_TOOLWINDOW, que sí es parte del estilo de la ventana.
#[cfg(windows)]
fn keep_out_of_taskbar(window: &tauri::WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
    };
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    // SAFETY: el HWND es válido mientras viva la ventana; ambas llamadas solo
    // leen y escriben el estilo extendido y no retienen el puntero.
    unsafe {
        let current = GetWindowLongPtrW(hwnd.0 as _, GWL_EXSTYLE);
        let updated = (current & !(WS_EX_APPWINDOW as isize)) | WS_EX_TOOLWINDOW as isize;
        if updated != current {
            SetWindowLongPtrW(hwnd.0 as _, GWL_EXSTYLE, updated);
        }
    }
}

#[cfg(windows)]
fn set_physical_bounds(window: &tauri::WebviewWindow, x: i32, y: i32, width: u32, height: u32) {
    let _ = window.set_size(tauri::PhysicalSize::new(width, height));
    let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
}

#[cfg(windows)]
fn ensure_worlds(app: &tauri::AppHandle) -> Result<Vec<tauri::WebviewWindow>, String> {
    let monitors = atic_capture::monitors::enumerate();
    if monitors.is_empty() {
        return Err("no hay monitores para el compositor liquido".into());
    }
    let mut worlds = Vec::with_capacity(monitors.len());
    for monitor in &monitors {
        let label = format!("{WORLD_PREFIX}{}", monitor.id);
        let title = format!("Pearl Flow world {}", monitor.id);
        let world = ensure_window(app, &label, &title, false)?;
        let area = monitor.work_area;
        set_physical_bounds(&world, area.x, area.y, area.width, area.height);
        world
            .set_ignore_cursor_events(true)
            .map_err(|error| error.to_string())?;
        crate::webview_tweaks::disable_browser_accelerator_keys(&world);
        worlds.push(world);
    }
    Ok(worlds)
}

#[cfg(windows)]
pub fn open_worlds(app: &tauri::AppHandle) -> Result<(), String> {
    for world in ensure_worlds(app)? {
        let _ = world.show();
        // Después de `show()`: al mostrarla, tao reescribe el estilo extendido
        // desde su propio estado y pisa cualquier cambio anterior.
        keep_out_of_taskbar(&world);
    }
    if let Some(pill) = app.get_webview_window("pill") {
        let _ = pill.set_always_on_top(true);
        if pill.is_visible().unwrap_or(false) {
            let _ = pill.show();
        }
    }
    emit_snapshot(app);
    Ok(())
}

#[cfg(not(windows))]
pub fn open_worlds(_app: &tauri::AppHandle) -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
pub fn open_spike(app: &tauri::AppHandle) -> Result<(), String> {
    let monitors = atic_capture::monitors::enumerate();
    let primary = monitors
        .iter()
        .find(|monitor| monitor.is_primary)
        .or_else(|| monitors.first())
        .ok_or("no hay monitores para el compositor líquido")?;
    let work = primary.work_area;

    let worlds = ensure_worlds(app)?;

    let pill = ensure_window(app, "liquid-spike-pill", "Pearl Flow pill", true)?;
    let meetings = ensure_window(app, "liquid-spike-meetings", "Pearl Flow meetings", true)?;
    let agents = ensure_window(app, "liquid-spike-agents", "Pearl Flow agents", true)?;

    let center_x = work.x + work.width as i32 / 2;
    let center_y = work.y + work.height as i32 / 2;
    set_physical_bounds(&pill, center_x - 360, center_y - 24, 48, 48);
    set_physical_bounds(&meetings, center_x - 300, center_y - 90, 280, 180);
    set_physical_bounds(&agents, center_x - 10, center_y - 130, 360, 260);

    // La piel nunca participa del input. Las ventanas pequeñas son las únicas
    // regiones que pueden recibir foco o comenzar un drag nativo.
    crate::webview_tweaks::disable_browser_accelerator_keys(&pill);
    crate::webview_tweaks::disable_browser_accelerator_keys(&meetings);
    crate::webview_tweaks::disable_browser_accelerator_keys(&agents);

    for world in &worlds {
        let _ = world.show();
    }
    // Mostrar el contenido después conserva la piel detrás dentro del grupo
    // always-on-top. El spike medirá si Windows mantiene ese contrato al mover.
    for window in [&pill, &meetings, &agents] {
        let _ = window.show();
    }
    let _ = agents.set_focus();
    emit_snapshot(app);
    Ok(())
}

#[cfg(not(windows))]
pub fn open_spike(_app: &tauri::AppHandle) -> Result<(), String> {
    Err("el spike del compositor está disponible en Windows".into())
}

fn radius_for(label: &str) -> f64 {
    match label {
        "pill" | "liquid-spike-pill" => 24.0,
        "pearl-tool-agents" | "liquid-spike-agents" => 16.0,
        label if label.starts_with(crate::floating_world::TOOL_WINDOW_PREFIX) => 15.0,
        _ => 18.0,
    }
}

#[tauri::command]
pub fn liquid_world_snapshot(
    label: String,
    app: tauri::AppHandle,
) -> Result<LiquidWorldSnapshot, String> {
    if !label.starts_with(WORLD_PREFIX) {
        return Err("etiqueta de compositor liquido invalida".into());
    }
    let world = app
        .get_webview_window(&label)
        .ok_or("el compositor líquido no está abierto")?;
    let origin = world.outer_position().map_err(|error| error.to_string())?;
    let size = world.outer_size().map_err(|error| error.to_string())?;
    let scale = world.scale_factor().map_err(|error| error.to_string())?;
    let logical = |value: i32| f64::from(value) / scale;
    let world_right = origin.x + size.width as i32;
    let world_bottom = origin.y + size.height as i32;

    let object_labels: Vec<String> = app
        .webview_windows()
        .keys()
        .filter(|candidate| {
            candidate.as_str() == "pill"
                || candidate.starts_with(crate::floating_world::TOOL_WINDOW_PREFIX)
                || candidate.starts_with(OBJECT_PREFIX)
        })
        .cloned()
        .collect();

    let groups = crate::floating_world::merged_groups(&app);
    let merged = groups
        .iter()
        .flat_map(|group| group.members.iter().cloned())
        .collect::<std::collections::HashSet<_>>();

    let objects = object_labels
        .into_iter()
        .filter_map(|label| {
            let id = label
                .trim_start_matches(OBJECT_PREFIX)
                .trim_start_matches(crate::floating_world::TOOL_WINDOW_PREFIX)
                .to_string();
            // Solo los miembros agrupados entran al campo global. Cada ventana
            // mantiene además un cuerpo local para que el contenido nunca se
            // despegue durante el arrastre; la piel global se oculta al mover.
            if label.starts_with(crate::floating_world::TOOL_WINDOW_PREFIX) && !merged.contains(&id)
            {
                return None;
            }
            if label == "pill" && !merged.contains(&id) {
                return None;
            }
            let window = app.get_webview_window(&label)?;
            if !window.is_visible().unwrap_or(false) {
                return None;
            }
            let position = window.outer_position().ok()?;
            let size = window.outer_size().ok()?;
            let right = position.x + size.width as i32;
            let bottom = position.y + size.height as i32;
            if right <= origin.x
                || position.x >= world_right
                || bottom <= origin.y
                || position.y >= world_bottom
            {
                return None;
            }
            let logical_width = f64::from(size.width) / scale;
            let logical_height = f64::from(size.height) / scale;
            let (inset_x, inset_y, width, height) = if label == "pill" && logical_width <= 80.0 {
                ((logical_width - 48.0) / 2.0, 8.0, 48.0, 48.0)
            } else if label == "pill" {
                ((logical_width - 154.0) / 2.0, 32.0, 154.0, 50.0)
            } else {
                (0.0, 0.0, logical_width, logical_height)
            };
            Some(LiquidWorldObject {
                id,
                x: logical(position.x - origin.x) + inset_x,
                y: logical(position.y - origin.y) + inset_y,
                width,
                height,
                radius: radius_for(&label),
            })
        })
        .collect::<Vec<_>>();

    Ok(LiquidWorldSnapshot {
        world_label: label,
        width: f64::from(size.width) / scale,
        height: f64::from(size.height) / scale,
        scale,
        revision: SNAPSHOT_REVISION.fetch_add(1, Ordering::Relaxed) + 1,
        emitted_at_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        debug: std::env::var_os("ATIC_LIQUID_WORLD_SPIKE").is_some(),
        dragging: crate::floating_world::is_dragging(&app),
        groups,
        objects,
    })
}

pub fn emit_snapshot(app: &tauri::AppHandle) {
    let labels: Vec<String> = app
        .webview_windows()
        .keys()
        .filter(|label| label.starts_with(WORLD_PREFIX))
        .cloned()
        .collect();
    for label in labels {
        let Ok(snapshot) = liquid_world_snapshot(label, app.clone()) else {
            continue;
        };
        let _ = app.emit("liquid-world-updated", snapshot);
    }
}

#[tauri::command]
pub fn report_liquid_world_metrics(label: String, mut samples: Vec<f64>) {
    samples.retain(|sample| sample.is_finite() && *sample >= 0.0);
    if samples.is_empty() {
        return;
    }
    samples.sort_by(f64::total_cmp);
    let p95_index = ((samples.len() - 1) as f64 * 0.95).round() as usize;
    let p95 = samples[p95_index];
    let average = samples.iter().sum::<f64>() / samples.len() as f64;
    tracing::info!(
        target: "liquid_world",
        %label,
        count = samples.len(),
        average_ms = format_args!("{average:.2}"),
        p95_ms = format_args!("{p95:.2}"),
        "métrica SDF del compositor"
    );
}

#[tauri::command]
pub fn report_liquid_world_focus(label: String, focused: bool, active: String) {
    tracing::info!(
        target: "liquid_world",
        %label,
        focused,
        %active,
        "estado de foco del objeto líquido"
    );
}

pub fn is_spike_object(label: &str) -> bool {
    label.starts_with(OBJECT_PREFIX)
}

#[tauri::command]
pub fn close_liquid_world_spike(app: tauri::AppHandle) {
    let labels: Vec<String> = app
        .webview_windows()
        .keys()
        .filter(|label| label.starts_with(WORLD_PREFIX) || label.starts_with(OBJECT_PREFIX))
        .cloned()
        .collect();
    for label in labels {
        if let Some(window) = app.get_webview_window(&label) {
            let _ = window.close();
        }
    }
}
