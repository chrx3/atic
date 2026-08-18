//! Resolver y enfocar la ventana de la TUI del agente.
//!
//! El JSONL no trae pid. Lo honesto en el MVP: si hay un solo `claude.exe` y
//! una sola presencia, se atan. Cualquier otra combinación no se adivina.
//! La decisión (a qué HWND, si el unread puede bajar) vive acá y se testea
//! con un fake; el Win32 queda atrás del trait.

use serde::Serialize;

use super::presence::{self, PresenceWindow};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FocusKind {
    Focused,
    Console,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresenceFocusResult {
    pub kind: FocusKind,
}

/// Cómo se habla con el SO. El fake de tests no toca Win32.
pub trait WindowFocus {
    fn force(&self, hwnd: isize) -> bool;
    fn is_own(&self, hwnd: isize) -> bool;
    fn flash(&self, hwnd: isize);
}

pub fn focus_presence(window: Option<&PresenceWindow>, api: &impl WindowFocus) -> FocusKind {
    let Some(w) = window else {
        return FocusKind::None;
    };
    if api.is_own(w.hwnd) {
        return FocusKind::Console;
    }
    if api.force(w.hwnd) {
        FocusKind::Focused
    } else {
        api.flash(w.hwnd);
        FocusKind::None
    }
}

/// Un pid de agente y una presencia → se pueden atar. Si no, `None`.
pub fn unique_attach(
    agent_pids: &[u32],
    presence_ids: &[String],
    hwnd_for_pid: impl Fn(u32) -> Option<PresenceWindow>,
) -> Option<(String, PresenceWindow)> {
    if agent_pids.len() != 1 || presence_ids.len() != 1 {
        return None;
    }
    let win = hwnd_for_pid(agent_pids[0])?;
    Some((presence_ids[0].clone(), win))
}

pub fn focus_id(id: &str) -> PresenceFocusResult {
    if !super::PAGER_ENABLED {
        return PresenceFocusResult {
            kind: FocusKind::None,
        };
    }
    let Some(mut presence) = presence::get(id) else {
        return PresenceFocusResult {
            kind: FocusKind::None,
        };
    };
    if presence.window.as_ref().is_none_or(|w| !hwnd_alive(w.hwnd)) {
        if let Some(win) = resolve_unique_for(&presence.backend_id, id) {
            presence::set_window(id, win.clone());
            presence.window = Some(win);
        }
    }
    PresenceFocusResult {
        kind: focus_with(&presence.window),
    }
}

/// Ata la última ventana externa (no Atic) a esta presencia y la enfoca.
pub fn bind_id(id: &str) -> PresenceFocusResult {
    if !super::PAGER_ENABLED {
        return PresenceFocusResult {
            kind: FocusKind::None,
        };
    }
    if presence::get(id).is_none() {
        return PresenceFocusResult {
            kind: FocusKind::None,
        };
    }
    let Some(win) = last_external_window() else {
        return PresenceFocusResult {
            kind: FocusKind::None,
        };
    };
    presence::set_window(id, win.clone());
    PresenceFocusResult {
        kind: focus_with(&Some(win)),
    }
}

/// Decisión pura: un HWND ajeno se puede atar; el de Atic no.
pub fn bind_from_hwnd(
    hwnd: Option<isize>,
    api: &impl WindowFocus,
    pid_of: impl Fn(isize) -> u32,
) -> Option<PresenceWindow> {
    let hwnd = hwnd.filter(|&h| h != 0)?;
    if api.is_own(hwnd) {
        return None;
    }
    Some(PresenceWindow {
        pid: pid_of(hwnd),
        hwnd,
    })
}

fn last_external_window() -> Option<PresenceWindow> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
        let hwnd = crate::clipboard_history::last_external_hwnd()?;
        let api = Win32Focus;
        bind_from_hwnd(Some(hwnd as isize), &api, |h| {
            let mut pid = 0u32;
            unsafe {
                GetWindowThreadProcessId(h as _, &mut pid);
            }
            pid
        })
    }
    #[cfg(not(windows))]
    {
        None
    }
}

fn focus_with(window: &Option<PresenceWindow>) -> FocusKind {
    #[cfg(windows)]
    {
        focus_presence(window.as_ref(), &Win32Focus)
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        FocusKind::None
    }
}

pub fn attach_unique_claude() {
    attach_unique_backend("claude-code");
}

/// Un proceso de este backend y una presencia → se atan. Si hay más, no.
pub fn attach_unique_backend(backend_id: &str) {
    let snap = presence::snapshot();
    let of: Vec<_> = snap.iter().filter(|p| p.backend_id == backend_id).collect();
    if of.len() != 1 {
        return;
    }
    let p = of[0];
    if p.window.as_ref().is_some_and(|w| hwnd_alive(w.hwnd)) {
        return;
    }
    if let Some(win) = resolve_unique_for(backend_id, &p.id) {
        presence::set_window(&p.id, win);
    }
}

pub(crate) fn agent_tui_pids(backend_id: &str) -> Vec<u32> {
    #[cfg(windows)]
    {
        agent_pids(backend_id)
    }
    #[cfg(not(windows))]
    {
        let _ = backend_id;
        Vec::new()
    }
}

fn resolve_unique_for(backend_id: &str, presence_id: &str) -> Option<PresenceWindow> {
    #[cfg(windows)]
    {
        let pids = agent_pids(backend_id);
        unique_attach(&pids, &[presence_id.to_string()], hwnd_for_agent).map(|(_, w)| w)
    }
    #[cfg(not(windows))]
    {
        let _ = (backend_id, presence_id);
        None
    }
}

pub fn hwnd_alive(hwnd: isize) -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::HWND;
        use windows_sys::Win32::UI::WindowsAndMessaging::IsWindow;
        if hwnd == 0 {
            return false;
        }
        unsafe { IsWindow(hwnd as HWND) != 0 }
    }
    #[cfg(not(windows))]
    {
        hwnd != 0
    }
}

#[cfg(windows)]
struct Win32Focus;

#[cfg(windows)]
impl WindowFocus for Win32Focus {
    fn force(&self, hwnd: isize) -> bool {
        use windows_sys::Win32::Foundation::HWND;
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, IsWindow};
        let hwnd = hwnd as HWND;
        unsafe {
            if IsWindow(hwnd) == 0 {
                return false;
            }
            crate::clipboard_history::force_foreground(hwnd);
            GetForegroundWindow() == hwnd
        }
    }

    fn is_own(&self, hwnd: isize) -> bool {
        use windows_sys::Win32::Foundation::HWND;
        crate::clipboard_history::is_own_app_hwnd(hwnd as HWND)
    }

    fn flash(&self, hwnd: isize) {
        use windows_sys::Win32::Foundation::HWND;
        use windows_sys::Win32::UI::WindowsAndMessaging::{FLASHW_TRAY, FLASHWINFO, FlashWindowEx};
        let mut info = FLASHWINFO {
            cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
            hwnd: hwnd as HWND,
            dwFlags: FLASHW_TRAY,
            uCount: 3,
            dwTimeout: 0,
        };
        unsafe {
            let _ = FlashWindowEx(&mut info);
        }
    }
}

#[cfg(windows)]
fn agent_exe(backend_id: &str) -> Option<&'static str> {
    match backend_id {
        "claude-code" => Some("claude.exe"),
        "codex" => Some("codex.exe"),
        "cursor" => Some("cursor-agent.exe"),
        "opencode" => Some("opencode.exe"),
        _ => None,
    }
}

#[cfg(windows)]
fn agent_pids(backend_id: &str) -> Vec<u32> {
    let Some(exe) = agent_exe(backend_id) else {
        return Vec::new();
    };
    let snap = process_snapshot();
    let pids: Vec<u32> = snap
        .iter()
        .filter(|(_, _, name)| name == exe)
        .map(|(pid, _, _)| *pid)
        .collect();
    if backend_id == "cursor" {
        exclude_ide_children(
            &pids,
            &snap,
            &["cursor.exe", "atic.exe", "atic-desktop.exe"],
        )
    } else {
        pids
    }
}

/// Quita procesos cuyo ancestro es el IDE / Atic: no son TUI.
pub fn exclude_ide_children(
    agent_pids: &[u32],
    processes: &[(u32, u32, String)],
    skip_exe: &[&str],
) -> Vec<u32> {
    let tree: std::collections::HashMap<u32, (u32, String)> = processes
        .iter()
        .map(|(pid, ppid, name)| (*pid, (*ppid, name.clone())))
        .collect();
    agent_pids
        .iter()
        .copied()
        .filter(|pid| {
            let mut current = *pid;
            let mut seen = std::collections::HashSet::new();
            while seen.insert(current) {
                let Some((ppid, name)) = tree.get(&current) else {
                    break;
                };
                if current != *pid && skip_exe.iter().any(|s| *s == name.as_str()) {
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

#[cfg(windows)]
fn process_snapshot() -> Vec<(u32, u32, String)> {
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return Vec::new();
        }
        let mut entry: PROCESSENTRY32W = zeroed();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut out = Vec::new();
        if Process32FirstW(snap, &mut entry) != 0 {
            loop {
                let end = entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..end]).to_ascii_lowercase();
                out.push((entry.th32ProcessID, entry.th32ParentProcessID, name));
                if Process32NextW(snap, &mut entry) == 0 {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        out
    }
}

#[cfg(windows)]
struct HwndsByPid {
    by_pid: std::collections::HashMap<u32, Vec<isize>>,
}

#[cfg(windows)]
fn hwnd_for_agent(pid: u32) -> Option<PresenceWindow> {
    use std::collections::{HashMap, HashSet};
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GW_OWNER, GetWindow, GetWindowThreadProcessId, IsWindowVisible,
    };

    let tree: HashMap<u32, u32> = process_snapshot()
        .into_iter()
        .map(|(id, ppid, _)| (id, ppid))
        .collect();

    let mut state = HwndsByPid {
        by_pid: HashMap::new(),
    };

    unsafe extern "system" fn enum_proc(
        hwnd: HWND,
        lparam: windows_sys::Win32::Foundation::LPARAM,
    ) -> i32 {
        let state = &mut *(lparam as *mut HwndsByPid);
        if IsWindowVisible(hwnd) == 0 {
            return 1;
        }
        let owner = GetWindow(hwnd, GW_OWNER);
        if !owner.is_null() {
            return 1;
        }
        let mut wpid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut wpid);
        if wpid != 0 {
            state.by_pid.entry(wpid).or_default().push(hwnd as isize);
        }
        1
    }

    unsafe {
        let _ = EnumWindows(Some(enum_proc), &mut state as *mut HwndsByPid as _);
    }

    let api = Win32Focus;
    let mut current = pid;
    let mut seen = HashSet::new();
    while seen.insert(current) {
        if let Some(hwnds) = state.by_pid.get(&current) {
            for &hwnd in hwnds {
                if !api.is_own(hwnd) {
                    return Some(PresenceWindow { pid: current, hwnd });
                }
            }
        }
        match tree.get(&current).copied() {
            Some(ppid) if ppid != 0 && ppid != current => current = ppid,
            _ => break,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct Fake {
        own: Vec<isize>,
        force_ok: Vec<isize>,
        flashed: Mutex<Vec<isize>>,
    }

    impl WindowFocus for Fake {
        fn force(&self, hwnd: isize) -> bool {
            self.force_ok.contains(&hwnd)
        }
        fn is_own(&self, hwnd: isize) -> bool {
            self.own.contains(&hwnd)
        }
        fn flash(&self, hwnd: isize) {
            self.flashed.lock().unwrap().push(hwnd);
        }
    }

    fn win(pid: u32, hwnd: isize) -> PresenceWindow {
        PresenceWindow { pid, hwnd }
    }

    #[test]
    fn unique_un_pid_una_sesion_ata() {
        let got = unique_attach(&[10], &["s1".into()], |_| Some(win(10, 99)));
        assert_eq!(got, Some(("s1".into(), win(10, 99))));
    }

    #[test]
    fn unique_dos_pids_no_adivina() {
        assert!(unique_attach(&[10, 11], &["s1".into()], |_| Some(win(10, 99))).is_none());
    }

    #[test]
    fn unique_dos_sesiones_no_adivina() {
        assert!(unique_attach(&[10], &["s1".into(), "s2".into()], |_| Some(win(10, 99))).is_none());
    }

    #[test]
    fn exclude_hijo_del_ide() {
        let snap = vec![
            (10, 1, "cursor-agent.exe".into()),
            (1, 0, "cursor.exe".into()),
            (20, 2, "cursor-agent.exe".into()),
            (2, 0, "windowsterminal.exe".into()),
        ];
        let kept = exclude_ide_children(&[10, 20], &snap, &["cursor.exe"]);
        assert_eq!(kept, vec![20]);
    }

    #[test]
    fn focus_sin_ventana_no_baja_aviso() {
        let fake = Fake {
            own: vec![],
            force_ok: vec![],
            flashed: Mutex::new(vec![]),
        };
        assert_eq!(focus_presence(None, &fake), FocusKind::None);
        assert!(fake.flashed.lock().unwrap().is_empty());
    }

    #[test]
    fn focus_propio_es_consola() {
        let fake = Fake {
            own: vec![7],
            force_ok: vec![],
            flashed: Mutex::new(vec![]),
        };
        assert_eq!(focus_presence(Some(&win(1, 7)), &fake), FocusKind::Console);
    }

    #[test]
    fn focus_ok_confirma() {
        let fake = Fake {
            own: vec![],
            force_ok: vec![9],
            flashed: Mutex::new(vec![]),
        };
        assert_eq!(focus_presence(Some(&win(1, 9)), &fake), FocusKind::Focused);
    }

    #[test]
    fn focus_fallido_parpadea_y_no_confirma() {
        let fake = Fake {
            own: vec![],
            force_ok: vec![],
            flashed: Mutex::new(vec![]),
        };
        assert_eq!(focus_presence(Some(&win(1, 9)), &fake), FocusKind::None);
        assert_eq!(*fake.flashed.lock().unwrap(), vec![9]);
    }

    #[test]
    fn bind_salta_ventana_propia() {
        let fake = Fake {
            own: vec![7],
            force_ok: vec![],
            flashed: Mutex::new(vec![]),
        };
        assert!(bind_from_hwnd(Some(7), &fake, |_| 1).is_none());
    }

    #[test]
    fn bind_toma_ventana_ajena() {
        let fake = Fake {
            own: vec![7],
            force_ok: vec![],
            flashed: Mutex::new(vec![]),
        };
        assert_eq!(bind_from_hwnd(Some(9), &fake, |_| 42), Some(win(42, 9)));
    }
}
