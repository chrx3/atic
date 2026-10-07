//! Los atajos globales de las herramientas: los de `config.json`.
//!
//! Mientras la app de Atic los tenga (está abierta y la pill no es la nativa,
//! ver `dictation::Watch`), aquí solo queda Ctrl+Shift+Space para el lanzador,
//! que no choca con los de ella. Con la pill nativa, Atic ya no los registra y
//! los toma la pill.
//!
//! Van en un hilo propio con `RegisterHotKey`: recibir el `WM_HOTKEY` le da al
//! proceso permiso para tomar el foco, que el lanzador necesita para recibir lo
//! que se escribe. El dictado y la rueda no pasan por aquí: se mantienen
//! apretados y `RegisterHotKey` no avisa al soltar (`dictation.rs` los sondea).

use std::sync::mpsc::Receiver;

use crate::dictation::{Shortcut, Watch, VK_XBUTTON1, VK_XBUTTON2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Clipboard,
    Snippets,
    Capture,
    Board,
    Color,
    Flip,
    Launcher,
    Agents,
    Record,
    Summon,
}

/// El del lanzador mientras Atic tiene los atajos.
const FALLBACK_LAUNCHER: &str = "Ctrl+Shift+Space";

const MOD_ALT: u32 = 0x1;
const MOD_CONTROL: u32 = 0x2;
const MOD_SHIFT: u32 = 0x4;
const MOD_WIN: u32 = 0x8;

/// Un atajo como lo pide `RegisterHotKey`: modificadores y una tecla.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Chord {
    mods: u32,
    key: u32,
}

/// `Alt+V`, `CmdOrCtrl+Shift+4`… Lo que no es una sola tecla con
/// modificadores (los botones del mouse, dos teclas) no se puede registrar.
fn chord(text: &str) -> Option<Chord> {
    let mut mods = 0;
    let mut key = None;
    for vk in Shortcut::parse(text)?.keys {
        match vk {
            0x12 => mods |= MOD_ALT,
            0x11 => mods |= MOD_CONTROL,
            0x10 => mods |= MOD_SHIFT,
            0x5B => mods |= MOD_WIN,
            // `RegisterHotKey` no sabe de botones del mouse (`mouse_button`).
            VK_XBUTTON1 | VK_XBUTTON2 => return None,
            other => {
                if key.replace(other as u32).is_some() {
                    return None;
                }
            }
        }
    }
    Some(Chord { mods, key: key? })
}

/// `MouseX1` o `MouseX2` solo, sin teclas: su tecla virtual.
fn mouse_button(text: &str) -> Option<i32> {
    match Shortcut::parse(text)?.keys.as_slice() {
        [vk @ (VK_XBUTTON1 | VK_XBUTTON2)] => Some(*vk),
        _ => None,
    }
}

fn shortcuts(cfg: &atic_core::Config) -> [(Action, &String); 10] {
    [
        (Action::Clipboard, &cfg.clipboard_shortcut),
        (Action::Snippets, &cfg.snippets_shortcut),
        (Action::Capture, &cfg.screenshot_shortcut),
        (Action::Board, &cfg.board_shortcut),
        (Action::Color, &cfg.color_shortcut),
        (Action::Flip, &cfg.window_flip_shortcut),
        (Action::Launcher, &cfg.launcher_shortcut),
        (Action::Agents, &cfg.agents_shortcut),
        (Action::Record, &cfg.global_shortcut),
        (Action::Summon, &cfg.summon_pill_shortcut),
    ]
}

fn wanted(cfg: &atic_core::Config) -> Vec<(Action, Chord)> {
    shortcuts(cfg)
        .into_iter()
        .filter_map(|(action, text)| chord(text).map(|chord| (action, chord)))
        .collect()
}

/// Las herramientas atadas a un botón lateral del mouse. No se registran: se
/// mira el botón en cada vuelta, como el atajo del dictado.
fn wanted_mouse(cfg: &atic_core::Config) -> Vec<(Action, i32)> {
    shortcuts(cfg)
        .into_iter()
        .filter_map(|(action, text)| mouse_button(text).map(|vk| (action, vk)))
        .collect()
}

#[cfg(windows)]
pub fn spawn(watch: Watch) -> Receiver<Action> {
    use std::time::{Duration, Instant};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_NOREPEAT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{PeekMessageW, MSG, PM_REMOVE, WM_HOTKEY};

    /// Cada cuánto se relee la config: un atajo cambiado en Ajustes, o Atic
    /// que se abrió o se cerró.
    const RECHECK: Duration = Duration::from_secs(3);
    /// Cada cuánto se miran los mensajes: un atajo no necesita más.
    const POLL: Duration = Duration::from_millis(25);

    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("atajos".into()).spawn(move || {
        // `ok[i]`: si el registro con id `i + 1` resultó.
        let mut current: Vec<(Action, Chord)> = Vec::new();
        let mut ok: Vec<bool> = Vec::new();
        let mut checked: Option<Instant> = None;
        // Los botones del mouse y si estaban apretados en la vuelta anterior.
        let mut mouse: Vec<(Action, i32, bool)> = Vec::new();
        loop {
            // Hasta la primera revisión no se sabe si Atic los tiene: registrar
            // antes fallaría con los suyos.
            if watch.checked() && checked.is_none_or(|at| at.elapsed() >= RECHECK) {
                checked = Some(Instant::now());
                let cfg = (!watch.atic_owns_shortcuts())
                    .then(|| atic_core::AppDirs::new().ok())
                    .flatten()
                    .map(|dirs| atic_core::Config::load(&dirs.config_path()));
                let next = match &cfg {
                    Some(cfg) => wanted(cfg),
                    None => chord(FALLBACK_LAUNCHER).map(|c| vec![(Action::Launcher, c)]).unwrap_or_default(),
                };
                let next_mouse = cfg.as_ref().map(wanted_mouse).unwrap_or_default();
                if next_mouse.len() != mouse.len()
                    || next_mouse.iter().zip(&mouse).any(|(a, b)| (a.0, a.1) != (b.0, b.1))
                {
                    mouse = next_mouse.into_iter().map(|(action, vk)| (action, vk, true)).collect();
                }
                // Un atajo que otra app tenía puede haber quedado libre (Atic
                // que se reinicia sin los suyos): se reintenta.
                if next != current || ok.contains(&false) {
                    for id in 1..=current.len() {
                        unsafe {
                            let _ = UnregisterHotKey(None, id as i32);
                        }
                    }
                    ok = next
                        .iter()
                        .enumerate()
                        .map(|(i, (action, chord))| {
                            let mods = HOT_KEY_MODIFIERS(chord.mods) | MOD_NOREPEAT;
                            let result = unsafe { RegisterHotKey(None, i as i32 + 1, mods, chord.key) };
                            if let Err(error) = &result {
                                if next != current {
                                    eprintln!("[atajos] no se pudo registrar {action:?} (¿lo usa otra app?): {error}");
                                }
                            }
                            result.is_ok()
                        })
                        .collect();
                    if next != current {
                        let names: Vec<_> = next.iter().map(|(action, _)| format!("{action:?}")).collect();
                        println!("[atajos] {}", names.join(", "));
                    }
                    current = next;
                }
            }
            // Un botón del mouse cuenta al bajar. Empieza como «apretado» para
            // no disparar con el botón ya abajo al cambiar la config.
            for (action, vk, was_down) in mouse.iter_mut() {
                let down = unsafe { GetAsyncKeyState(*vk) } < 0;
                if down && !*was_down && tx.send(*action).is_err() {
                    return;
                }
                *was_down = down;
            }
            let mut msg = MSG::default();
            while unsafe { PeekMessageW(&mut msg, None, WM_HOTKEY, WM_HOTKEY, PM_REMOVE) }.as_bool() {
                let Some((action, _)) = current.get(msg.wParam.0.wrapping_sub(1)) else {
                    continue;
                };
                if tx.send(*action).is_err() {
                    return;
                }
            }
            std::thread::sleep(POLL);
        }
    });
    if let Err(error) = spawned {
        eprintln!("[atajos] no se pudo arrancar el hilo: {error}");
    }
    rx
}

#[cfg(not(windows))]
pub fn spawn(_watch: Watch) -> Receiver<Action> {
    std::sync::mpsc::channel().1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_como_los_escribe_atic() {
        assert_eq!(chord("Alt+V"), Some(Chord { mods: MOD_ALT, key: 'V' as u32 }));
        assert_eq!(
            chord("CmdOrCtrl+Shift+4"),
            Some(Chord { mods: MOD_CONTROL | MOD_SHIFT, key: '4' as u32 })
        );
        assert_eq!(chord("CmdOrCtrl+Space"), Some(Chord { mods: MOD_CONTROL, key: 0x20 }));
        assert_eq!(chord("Ctrl+F9"), Some(Chord { mods: MOD_CONTROL, key: 0x78 }));
    }

    #[test]
    fn lo_que_no_se_puede_registrar_se_salta() {
        assert_eq!(chord("Ctrl+Shift"), None);
        assert_eq!(chord("Ctrl+A+B"), None);
        assert_eq!(chord("Mouse4"), None);
        assert_eq!(chord(""), None);
    }

    #[test]
    fn los_botones_del_mouse_van_aparte() {
        assert_eq!(chord("MouseX1"), None);
        assert_eq!(mouse_button("MouseX1"), Some(VK_XBUTTON1));
        assert_eq!(mouse_button("MouseX2"), Some(VK_XBUTTON2));
        assert_eq!(mouse_button("Ctrl+MouseX1"), None);
        let mut cfg = atic_core::Config::default();
        cfg.screenshot_shortcut = "MouseX2".into();
        assert_eq!(wanted_mouse(&cfg), vec![(Action::Capture, VK_XBUTTON2)]);
        assert!(!wanted(&cfg).iter().any(|(action, _)| *action == Action::Capture));
    }

    #[test]
    fn los_atajos_salen_de_la_config() {
        let mut cfg = atic_core::Config::default();
        cfg.clipboard_shortcut = "Alt+V".into();
        cfg.color_shortcut = "Mouse5".into();
        let actions: Vec<_> = wanted(&cfg).into_iter().map(|(action, _)| action).collect();
        assert!(actions.contains(&Action::Clipboard));
        assert!(!actions.contains(&Action::Color));
    }
}
