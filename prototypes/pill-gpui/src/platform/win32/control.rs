//! Lo que el celular controla de Windows (`PcCommand` con `feature::CONTROL`):
//! el volumen, silenciar, bloquear la pantalla y pasar diapositivas. Las
//! teclas multimedia y las flechas van con `SendInput`, como el pegado.

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_LEFT, VK_RIGHT,
    VK_VOLUME_DOWN, VK_VOLUME_MUTE, VK_VOLUME_UP,
};

use super::super::control::Action;

pub fn run(action: Action) {
    match action {
        Action::VolumeUp => tap(VK_VOLUME_UP),
        Action::VolumeDown => tap(VK_VOLUME_DOWN),
        Action::Mute => tap(VK_VOLUME_MUTE),
        // Win+L por `SendInput` no bloquea: el sistema lo filtra.
        Action::LockScreen => {
            if let Err(error) = std::process::Command::new("rundll32.exe").args(["user32.dll,LockWorkStation"]).spawn() {
                tracing::warn!(%error, "celular: no se pudo bloquear la pantalla");
            }
        }
        Action::SlideNext => tap(VK_RIGHT),
        Action::SlidePrev => tap(VK_LEFT),
    }
}

/// La tecla, bajar y subir.
fn tap(vk: VIRTUAL_KEY) {
    fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: if up { KEYEVENTF_KEYUP } else { 0 }, time: 0, dwExtraInfo: 0 },
            },
        }
    }
    let inputs = [key(vk, false), key(vk, true)];
    // SAFETY: `inputs` vive durante la llamada y su largo es el que se pasa.
    unsafe {
        SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32);
    }
}
