//! Lo que el celular controla de Windows: el volumen, silenciar, bloquear la
//! pantalla, pasar diapositivas y suspender (`PcCommand`), y el panel táctil y
//! el teclado (`InputEvent`). Las teclas y el mouse van con `SendInput`, como
//! el pegado.

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
    MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
    MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, MOUSEINPUT, MOUSE_EVENT_FLAGS,
    VIRTUAL_KEY, VK_BACK, VK_DOWN, VK_ESCAPE, VK_F11, VK_LEFT, VK_LWIN, VK_MENU, VK_RETURN, VK_RIGHT, VK_SPACE, VK_TAB,
    VK_UP, VK_VOLUME_DOWN, VK_VOLUME_MUTE, VK_VOLUME_UP,
};

use super::super::control::{Action, InputEvent, MouseButton, PcKey};

const VK_B: VIRTUAL_KEY = 0x42;
const VK_D: VIRTUAL_KEY = 0x44;

pub fn run(action: Action) {
    match action {
        Action::VolumeUp => tap(VK_VOLUME_UP),
        Action::VolumeDown => tap(VK_VOLUME_DOWN),
        Action::Mute => tap(VK_VOLUME_MUTE),
        // Win+L por `SendInput` no bloquea: el sistema lo filtra.
        Action::LockScreen => spawn("rundll32.exe", &["user32.dll,LockWorkStation"], "bloquear la pantalla"),
        Action::SlideNext => tap(VK_RIGHT),
        Action::SlidePrev => tap(VK_LEFT),
        // `SetSuspendState` de rundll32 hiberna si la hibernación está prendida;
        // el de .NET suspende siempre.
        Action::Sleep => spawn(
            "powershell.exe",
            &[
                "-NoProfile",
                "-WindowStyle",
                "Hidden",
                "-Command",
                "Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.Application]::SetSuspendState('Suspend', $false, $false)",
            ],
            "suspender",
        ),
    }
}

pub fn input(event: InputEvent) {
    match event {
        // Relativo: pasa por la aceleración del mouse, como un panel táctil.
        InputEvent::Move { dx, dy } => send(&[mouse(MOUSEEVENTF_MOVE, dx.round() as i32, dy.round() as i32, 0)]),
        InputEvent::Scroll { dx, dy } => scroll(dx, dy),
        InputEvent::Click { button } => {
            let (down, up) = button_flags(button);
            send(&[mouse(down, 0, 0, 0), mouse(up, 0, 0, 0)]);
        }
        InputEvent::Button { button, down } => {
            let (d, u) = button_flags(button);
            send(&[mouse(if down { d } else { u }, 0, 0, 0)]);
        }
        InputEvent::Text { text } => type_text(&text),
        InputEvent::Key { key: pc_key } => match pc_key {
            PcKey::Enter => tap(VK_RETURN),
            PcKey::Escape => tap(VK_ESCAPE),
            PcKey::Backspace => tap(VK_BACK),
            PcKey::Tab => tap(VK_TAB),
            PcKey::Space => tap(VK_SPACE),
            PcKey::Up => tap(VK_UP),
            PcKey::Down => tap(VK_DOWN),
            PcKey::Left => tap(VK_LEFT),
            PcKey::Right => tap(VK_RIGHT),
            PcKey::AppSwitch => chord(VK_MENU, VK_TAB),
            PcKey::Fullscreen => tap(VK_F11),
            PcKey::ShowDesktop => chord(VK_LWIN, VK_D),
            PcKey::BlackScreen => tap(VK_B),
        },
    }
}

fn spawn(program: &str, args: &[&str], what: &str) {
    if let Err(error) = std::process::Command::new(program).args(args).spawn() {
        tracing::warn!(%error, "celular: no se pudo {what}");
    }
}

fn button_flags(button: MouseButton) -> (MOUSE_EVENT_FLAGS, MOUSE_EVENT_FLAGS) {
    match button {
        MouseButton::Left => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
        MouseButton::Right => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
        MouseButton::Middle => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
    }
}

/// En píxeles, a unidades de rueda: 120 es un salto (unas 3 líneas, ~40 px).
fn scroll(dx: f32, dy: f32) {
    let wy = (-dy * 3.0).round() as i32;
    let wx = (dx * 3.0).round() as i32;
    let mut inputs = Vec::new();
    if wy != 0 {
        inputs.push(mouse(MOUSEEVENTF_WHEEL, 0, 0, wy));
    }
    if wx != 0 {
        inputs.push(mouse(MOUSEEVENTF_HWHEEL, 0, 0, wx));
    }
    send(&inputs);
}

fn mouse(flags: MOUSE_EVENT_FLAGS, dx: i32, dy: i32, data: i32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT { dx, dy, mouseData: data as u32, dwFlags: flags, time: 0, dwExtraInfo: 0 },
        },
    }
}

fn key(vk: VIRTUAL_KEY, scan: u16, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    }
}

/// La tecla, bajar y subir.
fn tap(vk: VIRTUAL_KEY) {
    send(&[key(vk, 0, 0), key(vk, 0, KEYEVENTF_KEYUP)]);
}

/// Un modificador apretado alrededor de la tecla.
fn chord(modifier: VIRTUAL_KEY, vk: VIRTUAL_KEY) {
    send(&[key(modifier, 0, 0), key(vk, 0, 0), key(vk, 0, KEYEVENTF_KEYUP), key(modifier, 0, KEYEVENTF_KEYUP)]);
}

/// Texto tal cual, sin pasar por la distribución del teclado.
fn type_text(text: &str) {
    let inputs: Vec<INPUT> = text
        .encode_utf16()
        .flat_map(|unit| [key(0, unit, KEYEVENTF_UNICODE), key(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP)])
        .collect();
    send(&inputs);
}

fn send(inputs: &[INPUT]) {
    if inputs.is_empty() {
        return;
    }
    // SAFETY: `inputs` vive durante la llamada y su largo es el que se pasa.
    unsafe {
        SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32);
    }
}
