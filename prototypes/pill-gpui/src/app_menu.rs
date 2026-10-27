//! La barra de menús de macOS mientras hay una ventana normal abierta (Atic
//! Code, Ajustes, Reuniones: la app pasa a «regular», ver `platform/macos/dock.rs`).
//! Sin menús, AppKit dejaba la barra vacía: no había cómo salir de pantalla
//! completa más que con el semáforo verde, que ahí está escondido.
//!
//! Lo mínimo de una app de Mac: Atic (ocultar, salir), Edición y Ventana
//! (minimizar, acercar, pantalla completa con ⌃⌘F). En Windows no hace nada.

#[cfg(target_os = "macos")]
mod mac {
    use crate::i18n::t;
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

        set_menus(cx);
        // Los menús son de AppKit y no se redibujan: al cambiar el idioma en
        // Ajustes (`config_watch.rs`) se vuelven a poner.
        cx.spawn(async move |cx| {
            let mut english = crate::i18n::english();
            loop {
                cx.background_executor().timer(std::time::Duration::from_secs(2)).await;
                if crate::i18n::english() != english {
                    english = crate::i18n::english();
                    if cx.update(set_menus).is_err() {
                        break;
                    }
                }
            }
        })
        .detach();
    }

    fn set_menus(cx: &mut App) {
        cx.set_menus(vec![
            Menu {
                name: "Atic".into(),
                items: vec![
                    MenuItem::os_submenu(t("pill.appMenu.services"), SystemMenuType::Services),
                    MenuItem::separator(),
                    MenuItem::action(t("pill.appMenu.hide"), Hide),
                    MenuItem::action(t("pill.appMenu.hideOthers"), HideOthers),
                    MenuItem::action(t("pill.appMenu.showAll"), ShowAll),
                    MenuItem::separator(),
                    MenuItem::action(t("pill.tray.quit"), Quit),
                ],
            },
            Menu {
                name: t("pill.appMenu.edit").into(),
                items: vec![
                    MenuItem::os_action(t("pill.appMenu.undo"), Undo, OsAction::Undo),
                    MenuItem::os_action(t("pill.appMenu.redo"), Redo, OsAction::Redo),
                    MenuItem::separator(),
                    MenuItem::os_action(t("pill.appMenu.cut"), Cut, OsAction::Cut),
                    MenuItem::os_action(t("pill.appMenu.copy"), Copy, OsAction::Copy),
                    MenuItem::os_action(t("pill.appMenu.paste"), Paste, OsAction::Paste),
                    MenuItem::os_action(t("pill.appMenu.selectAll"), SelectAll, OsAction::SelectAll),
                ],
            },
            Menu {
                name: t("pill.appMenu.window").into(),
                items: vec![
                    MenuItem::action(t("pill.appMenu.minimize"), Minimize),
                    MenuItem::action(t("pill.appMenu.zoom"), Zoom),
                    MenuItem::separator(),
                    MenuItem::action(t("pill.appMenu.fullScreen"), ToggleFullScreen),
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

/// El clic derecho sobre el texto que se puede seleccionar (los mensajes de Atic
/// Code, las salidas de las herramientas, los avisos): Copiar, Copiar todo y
/// Seleccionar todo. En macOS sale el menú del sistema; en Windows, el que dibuja
/// gpui-m3 con estos textos.
pub fn init_text_menu(cx: &mut gpui::App) {
    use crate::i18n::t;
    let labels = || [t("pill.textMenu.copy").into(), t("pill.textMenu.copyAll").into(), t("pill.textMenu.selectAll").into()];
    #[cfg(target_os = "macos")]
    let native = Some(|request: gpui_m3::TextMenuRequest, window: &mut gpui::Window, cx: &mut gpui::App| {
        use crate::platform::popup_menu::{pop_up, Entry, Item};
        use gpui::ClipboardItem;
        let entry = |title: &str, symbol: &'static str| Item::Entry(Entry { symbol: Some(symbol), ..Entry::new(title) });
        let gpui_m3::TextMenuRequest { at, selected, all, select_all } = request;
        let mut items = Vec::new();
        if selected.is_some() {
            items.push(entry(t("pill.textMenu.copy"), "doc.on.doc"));
        }
        items.push(entry(t("pill.textMenu.copyAll"), "doc.on.doc"));
        items.push(entry(t("pill.textMenu.selectAll"), "selection.pin.in.out"));
        // Sin selección no está «Copiar»: las demás se corren una posición.
        let skip = usize::from(selected.is_none());
        let chosen = pop_up(window, at, items, crate::theme::is_light());
        cx.spawn(async move |cx| {
            let Ok(Some(index)) = chosen.await else {
                return;
            };
            let _ = cx.update(|cx| match index + skip {
                0 => cx.write_to_clipboard(ClipboardItem::new_string(selected.unwrap_or_default())),
                1 => cx.write_to_clipboard(ClipboardItem::new_string(all)),
                _ => select_all(cx),
            });
        })
        .detach();
    });
    #[cfg(not(target_os = "macos"))]
    let native: Option<fn(gpui_m3::TextMenuRequest, &mut gpui::Window, &mut gpui::App)> = None;
    gpui_m3::set_text_menu(cx, labels, native);
}
