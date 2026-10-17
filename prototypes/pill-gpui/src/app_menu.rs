//! La barra de menús de macOS mientras hay una ventana normal abierta (Atic
//! Code, Ajustes, Reuniones: la app pasa a «regular», ver `platform/macos/dock.rs`).
//! Sin menús, AppKit dejaba la barra vacía: no había cómo salir de pantalla
//! completa más que con el semáforo verde, que ahí está escondido.
//!
//! Lo mínimo de una app de Mac: Atic (ocultar, salir), Edición y Ventana
//! (minimizar, acercar, pantalla completa con ⌃⌘F). En Windows no hace nada.

#[cfg(target_os = "macos")]
mod mac {
    use gpui::{actions, App, KeyBinding, Keystroke, Menu, MenuItem, OsAction, SystemMenuType};

    actions!(
        atic_app,
        [
            Hide,
            HideOthers,
            ShowAll,
            Quit,
            Undo,
            Redo,
            Cut,
            Copy,
            Paste,
            SelectAll,
            Minimize,
            Zoom,
            ToggleFullScreen,
        ]
    );

    pub fn init(cx: &mut App) {
        // Sin contexto: valen en cualquier ventana y pierden ante los atajos de
        // cada vista. ⌃⌘F es el de macOS para pantalla completa.
        cx.bind_keys([
            KeyBinding::new("ctrl-cmd-f", ToggleFullScreen, None),
            KeyBinding::new("cmd-m", Minimize, None),
        ]);

        cx.on_action(|_: &Hide, cx| cx.hide());
        cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
        cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_action(|_: &Minimize, cx| on_window(cx, |window, _| window.minimize_window()));
        cx.on_action(|_: &Zoom, cx| on_window(cx, |window, _| window.zoom_window()));
        cx.on_action(|_: &ToggleFullScreen, cx| on_window(cx, |window, _| window.toggle_fullscreen()));
        // Cada campo de texto tiene sus propias acciones de copiar y pegar (los de
        // la pill, los de gpui-m3, el editor y la terminal): el menú manda la tecla
        // y la resuelve quien tenga el foco. Estas acciones no llevan tecla, así
        // que no se llaman a sí mismas.
        cx.on_action(|_: &Undo, cx| keystroke(cx, "cmd-z"));
        cx.on_action(|_: &Redo, cx| keystroke(cx, "cmd-shift-z"));
        cx.on_action(|_: &Cut, cx| keystroke(cx, "cmd-x"));
        cx.on_action(|_: &Copy, cx| keystroke(cx, "cmd-c"));
        cx.on_action(|_: &Paste, cx| keystroke(cx, "cmd-v"));
        cx.on_action(|_: &SelectAll, cx| keystroke(cx, "cmd-a"));

        cx.set_menus(vec![
            Menu {
                name: "Atic".into(),
                items: vec![
                    MenuItem::os_submenu("Servicios", SystemMenuType::Services),
                    MenuItem::separator(),
                    MenuItem::action("Ocultar Atic", Hide),
                    MenuItem::action("Ocultar otras", HideOthers),
                    MenuItem::action("Mostrar todo", ShowAll),
                    MenuItem::separator(),
                    MenuItem::action("Salir de Atic", Quit),
                ],
            },
            Menu {
                name: "Edición".into(),
                items: vec![
                    MenuItem::os_action("Deshacer", Undo, OsAction::Undo),
                    MenuItem::os_action("Rehacer", Redo, OsAction::Redo),
                    MenuItem::separator(),
                    MenuItem::os_action("Cortar", Cut, OsAction::Cut),
                    MenuItem::os_action("Copiar", Copy, OsAction::Copy),
                    MenuItem::os_action("Pegar", Paste, OsAction::Paste),
                    MenuItem::os_action("Seleccionar todo", SelectAll, OsAction::SelectAll),
                ],
            },
            Menu {
                name: "Ventana".into(),
                items: vec![
                    MenuItem::action("Minimizar", Minimize),
                    MenuItem::action("Acercar", Zoom),
                    MenuItem::separator(),
                    MenuItem::action("Entrar o salir de pantalla completa", ToggleFullScreen),
                ],
            },
        ]);
    }

    /// La ventana activa de la app (la del frente).
    fn on_window(cx: &mut App, work: impl FnOnce(&mut gpui::Window, &mut App)) {
        if let Some(handle) = cx.active_window() {
            let _ = handle.update(cx, |_, window, cx| work(window, cx));
        }
    }

    fn keystroke(cx: &mut App, key: &str) {
        let Ok(keystroke) = Keystroke::parse(key) else {
            return;
        };
        on_window(cx, |window, cx| {
            window.dispatch_keystroke(keystroke, cx);
        });
    }
}

#[cfg(target_os = "macos")]
pub use mac::init;

#[cfg(not(target_os = "macos"))]
pub fn init(_: &mut gpui::App) {}
