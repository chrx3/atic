//! El menú del clic derecho sobre un archivo o carpeta del panel Archivos: abrirlo,
//! mostrarlo en el Explorador, adjuntarlo al mensaje, abrir una terminal ahí y copiar
//! su ruta.

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
    pub(super) fn open_file_menu(&mut self, path: PathBuf, dir: bool, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.file_menu = Some(FileTarget { path, dir, at });
        cx.notify();
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
            menu = menu.item(item("file-open", "Abrir", "file", Box::new(move |view, _, cx| view.open_doc(open.clone(), false, cx))));
            let system = path.clone();
            menu = menu.item(item("file-system", "Abrir con la app predeterminada", "export", Box::new(move |view, _, cx| {
                if let Err(error) = std::process::Command::new("explorer").arg(&system).spawn() {
                    view.show_toast(format!("No se pudo abrir: {error}"), cx);
                }
            })));
        }
        let shown_in = path.clone();
        menu = menu.item(item(
            "file-reveal",
            if dir { "Abrir la carpeta en el Explorador" } else { "Mostrar en el Explorador" },
            "folder",
            Box::new(move |view, _, cx| {
                if let Err(error) = reveal(&shown_in, dir) {
                    view.show_toast(format!("No se pudo abrir el Explorador: {error}"), cx);
                }
            }),
        ));
        let here = if dir { path.clone() } else { path.parent().map(Path::to_path_buf).unwrap_or_else(|| path.clone()) };
        menu = menu.item(item("file-terminal", "Abrir una terminal aquí", "terminal", Box::new(move |view, window, cx| view.open_terminal_at(here.clone(), window, cx))));
        let attach = path.clone();
        menu = menu.separator().item(item("file-attach", "Adjuntar al mensaje", "clip", Box::new(move |view, window, cx| {
            view.attach_paths(vec![attach.clone()]);
            view.focus_composer(window, cx);
        })));
        let full = path.clone();
        menu = menu.separator().item(item("file-copy-path", "Copiar ruta", "copy", Box::new(move |view, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(full.display().to_string()));
            view.show_toast("Ruta copiada".to_string(), cx);
        })));
        let relative = path.clone();
        menu = menu.item(item("file-copy-relative", "Copiar ruta relativa", "copy", Box::new(move |view, _, cx| {
            let roots: Vec<PathBuf> = view.workspaces.active().map(|w| w.folders.clone()).unwrap_or_default();
            cx.write_to_clipboard(ClipboardItem::new_string(super::relative_to(&relative, &roots)));
            view.show_toast("Ruta relativa copiada".to_string(), cx);
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
