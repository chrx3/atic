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

use crate::dictation::{Shortcut, Watch};

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
            other => {
                if key.replace(other as u32).is_some() {
                    return None;
                }
            }
        }
    }
    Some(Chord { mods, key: key? })
}

fn wanted(cfg: &atic_core::Config) -> Vec<(Action, Chord)> {
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
    .into_iter()
    .filter_map(|(action, text)| chord(text).map(|chord| (action, chord)))
    .collect()
}

#[cfg(windows)]
pub fn spawn(watch: Watch) -> Receiver<Action> {
    use std::time::{Duration, Instant};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_NOREPEAT,
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
        loop {
            // Hasta la primera revisión no se sabe si Atic los tiene: registrar
            // antes fallaría con los suyos.
            if watch.checked() && checked.is_none_or(|at| at.elapsed() >= RECHECK) {
                checked = Some(Instant::now());
                let next = if watch.atic_owns_shortcuts() {
                    chord(FALLBACK_LAUNCHER).map(|c| vec![(Action::Launcher, c)]).unwrap_or_default()
                } else {
                    atic_core::AppDirs::new()
                        .map(|dirs| wanted(&atic_core::Config::load(&dirs.config_path())))
                        .unwrap_or_default()
                };
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
    fn los_atajos_salen_de_la_config() {
        let mut cfg = atic_core::Config::default();
        cfg.clipboard_shortcut = "Alt+V".into();
        cfg.color_shortcut = "Mouse5".into();
        let actions: Vec<_> = wanted(&cfg).into_iter().map(|(action, _)| action).collect();
        assert!(actions.contains(&Action::Clipboard));
        assert!(!actions.contains(&Action::Color));
    }
}
