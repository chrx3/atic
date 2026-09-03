//! Recientes del launcher: apps abiertas ahora + las que se lanzaron acá.
//!
//! El idle del Spotlight no puede ser una barra muda. Windows no ofrece un
//! “últimas usadas” limpio (UserAssist va cifrado); lo que sí es honesto es
//! enumerar ventanas visibles (abierta desde cuándo) y recordar lo que Atic
//! mismo abrió (usada hace).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::launcher::{hit_from_entry, index, ensure_index_populated, LauncherHit, LauncherKind};
use atic_core::MutexExt;

const RECENTS_LIMIT: usize = 8;
const STORE_LIMIT: usize = 24;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RecentRecord {
    id: String,
    at: u64,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn load(path: &Path) -> Vec<RecentRecord> {
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    serde_json::from_slice::<Vec<RecentRecord>>(&bytes).unwrap_or_default()
}

fn save(path: &Path, items: &[RecentRecord]) {
    if let Ok(bytes) = serde_json::to_vec(items) {
        let _ = std::fs::write(path, bytes);
    }
}

/// Anota que el usuario abrió esta entrada desde el launcher.
pub fn touch(path: &Path, id: &str) {
    if id.trim().is_empty() {
        return;
    }
    let mut items = load(path);
    items.retain(|r| r.id != id);
    items.insert(
        0,
        RecentRecord {
            id: id.to_string(),
            at: now_ms(),
        },
    );
    items.truncate(STORE_LIMIT);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    save(path, &items);
}

/// ¿Este proceso es una app que el usuario abre, no chrome del SO?
pub fn skip_process_stem(stem: &str) -> bool {
    matches!(
        stem,
        "atic"
            | "atic-desktop"
            | "explorer"
            | "dwm"
            | "searchhost"
            | "searchui"
            | "startmenuexperiencehost"
            | "runtimebroker"
            | "applicationframehost"
            | "systemsettings"
            | "textinputhost"
            | "lockapp"
            | "shellhost"
            | "sihost"
            | "taskhostw"
            | "conhost"
            | "dllhost"
            | "svchost"
            | "msedgewebview2"
            | "securityhealthsystray"
            | "widgets"
            | "widgetservice"
            | "phonexperiencehost"
    )
}

/// Relaciona un `.exe` en ejecución con una entrada del índice (título o id).
pub fn process_fits_app(stem: &str, window_title: &str, app_title: &str, app_id: &str) -> bool {
    let stem_n = crate::launcher::normalize(stem.trim_end_matches(".exe"));
    if stem_n.len() < 3 {
        return false;
    }
    let title_n = crate::launcher::normalize(app_title);
    let id_n = crate::launcher::normalize(app_id);
    if title_n == stem_n {
        return true;
    }
    if contains_token(&title_n, &stem_n) {
        return true;
    }
    if stem_n.len() >= 4 && contains_token(&stem_n, &title_n) {
        return true;
    }
    if stem_n.len() >= 4 && id_n.contains(&stem_n) {
        return true;
    }
    let win_n = crate::launcher::normalize(window_title);
    if win_n.is_empty() || title_n.len() < 4 {
        return false;
    }
    let head = win_n.split(" - ").next().unwrap_or(&win_n);
    head == title_n || contains_token(head, &title_n) || contains_token(&title_n, head)
}

fn contains_token(hay: &str, needle: &str) -> bool {
    if needle.is_empty() || hay.len() < needle.len() {
        return false;
    }
    if hay == needle {
        return true;
    }
    if hay.starts_with(needle) && hay.as_bytes().get(needle.len()) == Some(&b' ') {
        return true;
    }
    if hay.ends_with(needle) {
        let i = hay.len() - needle.len();
        if i > 0 && hay.as_bytes().get(i - 1) == Some(&b' ') {
            return true;
        }
    }
    hay.contains(&format!(" {needle} "))
}

struct RunningApp {
    stem: String,
    title: String,
    started_ms: u64,
    foreground: bool,
}

#[cfg(windows)]
fn running_apps() -> Vec<RunningApp> {
    windows::collect()
}

#[cfg(not(windows))]
fn running_apps() -> Vec<RunningApp> {
    Vec::new()
}

/// Recientes: primero las que están abiertas (con tiempo de proceso),
/// después las que Atic lanzó y ya no se ven.
pub fn list(store: &Path) -> Vec<LauncherHit> {
    ensure_index_populated();
    let persisted = load(store);
    let running = running_apps();
    let guard = index().lock_or_recover();

    struct Acc {
        hit: LauncherHit,
        running: bool,
        foreground: bool,
        opened_at: Option<u64>,
        last_used_at: Option<u64>,
    }

    let mut by_id: HashMap<String, Acc> = HashMap::new();

    for proc in &running {
        let Some(entry) = guard.iter().find(|e| {
            e.kind == LauncherKind::App
                && process_fits_app(&proc.stem, &proc.title, &e.title, &e.id)
        }) else {
            continue;
        };
        let acc = by_id.entry(entry.id.clone()).or_insert_with(|| Acc {
            hit: hit_from_entry(entry),
            running: true,
            foreground: false,
            opened_at: Some(proc.started_ms),
            last_used_at: None,
        });
        acc.running = true;
        acc.foreground |= proc.foreground;
        acc.opened_at = Some(
            acc.opened_at
                .map(|t| t.min(proc.started_ms))
                .unwrap_or(proc.started_ms),
        );
        if proc.foreground {
            acc.last_used_at = Some(now_ms());
        }
    }

    for rec in persisted {
        if let Some(acc) = by_id.get_mut(&rec.id) {
            let used = acc.last_used_at.unwrap_or(0).max(rec.at);
            acc.last_used_at = Some(used);
            continue;
        }
        let Some(entry) = guard.iter().find(|e| e.id == rec.id) else {
            continue;
        };
        by_id.insert(
            rec.id.clone(),
            Acc {
                hit: hit_from_entry(entry),
                running: false,
                foreground: false,
                opened_at: None,
                last_used_at: Some(rec.at),
            },
        );
    }

    let mut rows: Vec<Acc> = by_id.into_values().collect();
    rows.sort_by(|a, b| {
        b.foreground
            .cmp(&a.foreground)
            .then(b.running.cmp(&a.running))
            .then(
                b.last_used_at
                    .unwrap_or(0)
                    .cmp(&a.last_used_at.unwrap_or(0)),
            )
            .then(b.opened_at.unwrap_or(0).cmp(&a.opened_at.unwrap_or(0)))
    });
    rows.into_iter()
        .take(RECENTS_LIMIT)
        .map(|mut acc| {
            acc.hit.running = Some(acc.running);
            acc.hit.foreground = Some(acc.foreground);
            acc.hit.opened_at = acc.opened_at;
            acc.hit.last_used_at = acc.last_used_at;
            acc.hit
        })
        .collect()
}

pub fn store_path(data_dir: &Path) -> PathBuf {
    data_dir.join("launcher-recents.json")
}

#[cfg(windows)]
mod windows {
    use super::RunningApp;
    use std::path::Path;
    use windows_sys::Win32::Foundation::{CloseHandle, BOOL, FILETIME, HWND, LPARAM};
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetForegroundWindow, GetWindow, GetWindowLongPtrW, GetWindowTextLengthW,
        GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, GWL_EXSTYLE,
        GW_OWNER,
    };

    const WS_EX_TOOLWINDOW: isize = 0x0000_0080;
    const WS_EX_NOACTIVATE: isize = 0x0800_0000;

    struct Collect {
        self_pid: u32,
        foreground_pid: u32,
        apps: Vec<RunningApp>,
    }

    pub fn collect() -> Vec<RunningApp> {
        let fg = unsafe { GetForegroundWindow() };
        let mut foreground_pid = 0u32;
        if !fg.is_null() {
            unsafe { GetWindowThreadProcessId(fg, &mut foreground_pid) };
        }
        let mut state = Collect {
            self_pid: std::process::id(),
            foreground_pid,
            apps: Vec::new(),
        };
        unsafe {
            let _ = EnumWindows(Some(on_window), &mut state as *mut Collect as LPARAM);
        }
        state.apps
    }

    unsafe extern "system" fn on_window(hwnd: HWND, param: LPARAM) -> BOOL {
        let state = &mut *(param as *mut Collect);
        if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
            return 1;
        }
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        if ex & WS_EX_TOOLWINDOW != 0 || ex & WS_EX_NOACTIVATE != 0 {
            return 1;
        }
        if !GetWindow(hwnd, GW_OWNER).is_null() {
            return 1;
        }
        let length = GetWindowTextLengthW(hwnd);
        if length < 1 {
            return 1;
        }
        let mut title = vec![0u16; length as usize + 1];
        let copied = GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32);
        if copied <= 0 {
            return 1;
        }
        title.truncate(copied as usize);
        let title = String::from_utf16_lossy(&title);

        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 || pid == state.self_pid {
            return 1;
        }
        let Some((path, started_ms)) = process_info(pid) else {
            return 1;
        };
        let stem = Path::new(&path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        if super::skip_process_stem(&stem) {
            return 1;
        }
        state.apps.push(RunningApp {
            stem,
            title,
            started_ms,
            foreground: pid == state.foreground_pid,
        });
        1
    }

    unsafe fn process_info(pid: u32) -> Option<(String, u64)> {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut buffer = vec![0u16; 1024];
        let mut length = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length);
        let mut create = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let mut exit = create;
        let mut kernel = create;
        let mut user = create;
        let times = GetProcessTimes(handle, &mut create, &mut exit, &mut kernel, &mut user);
        CloseHandle(handle);
        if ok == 0 || length == 0 {
            return None;
        }
        buffer.truncate(length as usize);
        let path = String::from_utf16_lossy(&buffer);
        let started_ms = if times != 0 {
            filetime_to_unix_ms(create)
        } else {
            0
        };
        Some((path, started_ms))
    }

    fn filetime_to_unix_ms(ft: FILETIME) -> u64 {
        let ticks = ((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64;
        ticks
            .saturating_sub(116_444_736_000_000_000)
            .saturating_div(10_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chrome_exe_encuentra_google_chrome() {
        assert!(process_fits_app(
            "chrome",
            "Grok - Google Chrome",
            "Google Chrome",
            "app:C:\\Chrome.lnk"
        ));
    }

    #[test]
    fn code_exe_encuentra_vscode() {
        assert!(process_fits_app(
            "code",
            "launcher.rs — atic",
            "Visual Studio Code",
            "app:Code.lnk"
        ));
    }

    #[test]
    fn no_confunde_stems_cortos() {
        assert!(!process_fits_app("id", "Idle", "Adobe InDesign", "app:id.lnk"));
    }

    #[test]
    fn code_no_es_barcode() {
        assert!(!process_fits_app(
            "code",
            "Barcode Scanner",
            "Barcode",
            "app:Barcode.lnk"
        ));
    }

    #[test]
    fn skip_chrome_del_sistema() {
        assert!(skip_process_stem("explorer"));
        assert!(skip_process_stem("msedgewebview2"));
        assert!(!skip_process_stem("chrome"));
    }

    #[test]
    fn touch_pone_el_id_adelante_y_sin_duplicar() {
        let dir = std::env::temp_dir().join(format!(
            "atic-recents-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("launcher-recents.json");
        touch(&path, "app:a");
        touch(&path, "app:b");
        touch(&path, "app:a");
        let items = load(&path);
        let ids: Vec<&str> = items.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["app:a", "app:b"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
