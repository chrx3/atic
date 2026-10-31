//! El menú del clic derecho sobre un archivo o carpeta del panel Archivos: abrirlo,
//! mostrarlo en el Explorador, adjuntarlo al mensaje, abrir una terminal ahí y copiar
//! su ruta.

use crate::i18n::{t, tf};
use std::path::{Path, PathBuf};

use gpui::{anchored, deferred, div, point, prelude::*, px, AnyElement, ClickEvent, ClipboardItem, Context, Corner, MouseButton, Pixels, Point, Window};
use gpui_m3::MenuItem;

use super::CodeView;

/// Lo que se tocó con el clic derecho: la ruta, si es carpeta y dónde abrir el menú.
#[derive(Clone)]
pub(super) struct FileTarget {
    pub path: PathBuf,
    pub dir: bool,
    pub at: Point<Pixels>,
}

impl CodeView {
    /// Abre el menú: en macOS, el del sistema (`NSMenu`); si no, el dibujado.
    pub(super) fn open_file_menu(&mut self, path: PathBuf, dir: bool, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        #[cfg(target_os = "macos")]
        {
            self.native_file_menu(path, dir, at, window, cx);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = window;
            self.file_menu = Some(FileTarget { path, dir, at });
            cx.notify();
        }
    }

    /// Las mismas entradas que `file_menu_layer`, en un menú nativo de macOS.
    #[cfg(target_os = "macos")]
    fn native_file_menu(&mut self, path: PathBuf, dir: bool, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        use crate::platform::popup_menu::{Entry, Item};
        let entry = |title: &str, symbol: &'static str| Item::Entry(Entry { symbol: Some(symbol), ..Entry::new(title) });
        let mut entries: Vec<(Item, Option<super::NativeAction>)> = Vec::new();
        if !dir {
            let open = path.clone();
            entries.push((entry(t("pill.code.fileMenu.open"), "doc"), Some(Box::new(move |view: &mut Self, _: &mut Window, cx: &mut Context<Self>| view.open_doc(open, false, cx)))));
            let system = path.clone();
            entries.push((
                entry(t("pill.code.fileMenu.openDefault"), "arrow.up.forward.app"),
                Some(Box::new(move |view: &mut Self, _: &mut Window, cx: &mut Context<Self>| {
                    if let Err(error) = crate::paths::open(&system) {
                        view.show_toast(tf("pill.code.fileMenu.openFailed", &[("error", &error)]), cx);
                    }
                })),
            ));
        }
        let shown_in = path.clone();
        entries.push((
            entry(if dir { t("pill.code.fileMenu.openFolderFinder") } else { t("pill.code.fileMenu.showFinder") }, "folder"),
            Some(Box::new(move |view: &mut Self, _: &mut Window, cx: &mut Context<Self>| {
                if let Err(error) = reveal(&shown_in, dir) {
                    view.show_toast(tf("pill.code.fileMenu.openAppFailed", &[("app", &t("pill.code.fileMenu.appFinder")), ("error", &error)]), cx);
                }
            })),
        ));
        let here = if dir { path.clone() } else { path.parent().map(Path::to_path_buf).unwrap_or_else(|| path.clone()) };
        entries.push((
            entry(t("pill.code.fileMenu.openTerminal"), "terminal"),
            Some(Box::new(move |view: &mut Self, window: &mut Window, cx: &mut Context<Self>| view.open_terminal_at(here, window, cx))),
        ));
        entries.push((Item::Separator, None));
        let attach = path.clone();
        entries.push((
            entry(t("pill.code.fileMenu.attach"), "paperclip"),
            Some(Box::new(move |view: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
                view.attach_paths(vec![attach]);
                view.focus_composer(window, cx);
            })),
        ));
        entries.push((Item::Separator, None));
        let full = path.clone();
        entries.push((
            entry(t("pill.code.fileMenu.copyPath"), "doc.on.doc"),
            Some(Box::new(move |view: &mut Self, _: &mut Window, cx: &mut Context<Self>| {
                cx.write_to_clipboard(ClipboardItem::new_string(full.display().to_string()));
                view.show_toast(t("pill.code.fileMenu.pathCopied").to_string(), cx);
            })),
        ));
        entries.push((
            entry(t("pill.code.fileMenu.copyRelative"), "doc.on.doc"),
            Some(Box::new(move |view: &mut Self, _: &mut Window, cx: &mut Context<Self>| {
                let roots: Vec<PathBuf> = view.workspaces.active().map(|w| w.folders.clone()).unwrap_or_default();
                cx.write_to_clipboard(ClipboardItem::new_string(super::relative_to(&path, &roots)));
                view.show_toast(t("pill.code.fileMenu.relativeCopied").to_string(), cx);
            })),
        ));
        self.pop_native_menu(at, entries, window, cx);
    }

    pub(super) fn file_menu_layer(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let shown = self.file_menu_last.show("file-menu-presence", self.file_menu.clone(), window, cx)?;
        let FileTarget { path, dir, at } = shown.value.clone();
        let leaving = shown.leaving;
        let item = |id: &'static str, label: &'static str, icon: &'static str, run: Box<dyn Fn(&mut CodeView, &mut Window, &mut Context<CodeView>)>| {
            MenuItem::new(id, label).icon(icon).on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                view.file_menu = None;
                run(view, window, cx);
                cx.notify();
            }))
        };
        let mut menu = gpui_m3::Menu::new("file-menu").width(250.);
        if !dir {
            let open = path.clone();
            menu = menu.item(item("file-open", t("pill.code.fileMenu.open"), "file", Box::new(move |view, _, cx| view.open_doc(open.clone(), false, cx))));
            let system = path.clone();
            menu = menu.item(item("file-system", t("pill.code.fileMenu.openDefault"), "export", Box::new(move |view, _, cx| {
                if let Err(error) = crate::paths::open(&system) {
                    view.show_toast(tf("pill.code.fileMenu.openFailed", &[("error", &error)]), cx);
                }
            })));
        }
        let shown_in = path.clone();
        menu = menu.item(item(
            "file-reveal",
            match (dir, cfg!(target_os = "macos")) {
                (true, true) => t("pill.code.fileMenu.openFolderFinder"),
                (false, true) => t("pill.code.fileMenu.showFinder"),
                (true, false) => t("pill.code.fileMenu.openFolderExplorer"),
                (false, false) => t("pill.code.fileMenu.showExplorer"),
            },
            "folder",
            Box::new(move |view, _, cx| {
                if let Err(error) = reveal(&shown_in, dir) {
                    let app = if cfg!(target_os = "macos") { t("pill.code.fileMenu.appFinder") } else { t("pill.code.fileMenu.appExplorer") };
                    view.show_toast(tf("pill.code.fileMenu.openAppFailed", &[("app", &app), ("error", &error)]), cx);
                }
            }),
        ));
        let here = if dir { path.clone() } else { path.parent().map(Path::to_path_buf).unwrap_or_else(|| path.clone()) };
        menu = menu.item(item("file-terminal", t("pill.code.fileMenu.openTerminal"), "terminal", Box::new(move |view, window, cx| view.open_terminal_at(here.clone(), window, cx))));
        let attach = path.clone();
        menu = menu.separator().item(item("file-attach", t("pill.code.fileMenu.attach"), "clip", Box::new(move |view, window, cx| {
            view.attach_paths(vec![attach.clone()]);
            view.focus_composer(window, cx);
        })));
        let full = path.clone();
        menu = menu.separator().item(item("file-copy-path", t("pill.code.fileMenu.copyPath"), "copy", Box::new(move |view, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(full.display().to_string()));
            view.show_toast(t("pill.code.fileMenu.pathCopied").to_string(), cx);
        })));
        let relative = path.clone();
        menu = menu.item(item("file-copy-relative", t("pill.code.fileMenu.copyRelative"), "copy", Box::new(move |view, _, cx| {
            let roots: Vec<PathBuf> = view.workspaces.active().map(|w| w.folders.clone()).unwrap_or_default();
            cx.write_to_clipboard(ClipboardItem::new_string(super::relative_to(&relative, &roots)));
            view.show_toast(t("pill.code.fileMenu.relativeCopied").to_string(), cx);
        })));
        Some(
            deferred(
                div()
                    .absolute()
                    .occlude()
                    .inset_0()
                    .when(!leaving, |el| {
                        el.on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|view, _, _, cx| {
                                view.file_menu = None;
                                cx.notify();
                            }),
                        )
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(|view, _, _, cx| {
                                view.file_menu = None;
                                cx.notify();
                            }),
                        )
                    })
                    .child(
                        anchored().position(point(at.x, at.y)).anchor(Corner::TopLeft).snap_to_window_with_margin(px(8.)).child(
                            div()
                                .id("file-menu-card")
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                                .child(shown.wrap(gpui_m3::Exit::Sink, menu)),
                        ),
                    ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }
}

/// Abre el Explorador: en la carpeta, o en la del archivo con este seleccionado.
fn reveal(path: &Path, dir: bool) -> std::io::Result<()> {
    if !cfg!(windows) {
        return if dir { crate::paths::open(path) } else { crate::paths::reveal(path) };
    }
    let mut command = std::process::Command::new("explorer");
    if dir {
        command.arg(path);
    } else {
        // `/select,` va pegado a la ruta y entre comillas propias: el escape de Rust
        // («"/select,C:\a b"») lo rompe con espacios.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.raw_arg(format!("/select,\"{}\"", path.display()));
        }
        #[cfg(not(windows))]
        command.arg(path.parent().unwrap_or(path));
    }
    command.spawn().map(|_| ())
}
