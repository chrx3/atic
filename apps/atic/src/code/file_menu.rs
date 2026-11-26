//! El menú del clic derecho sobre un archivo o carpeta del panel Archivos: abrirlo,
//! mostrarlo en el Explorador, adjuntarlo al mensaje, abrir una terminal ahí y copiar
//! su ruta.

use crate::i18n::{t, tf};
use std::path::{Path, PathBuf};

use gpui::{anchored, deferred, div, point, prelude::*, px, AnyElement, ClickEvent, ClipboardItem, Context, Corner, Entity, MouseButton, Pixels, Point, Window};
use gpui_m3::{Button, Dialog, MenuItem};

use super::file_ops::{self, Removed};
use super::{CodeView, Side};

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
        let new_file = path.clone();
        entries.push((
            entry(t("pill.code.fileMenu.newFile"), "doc.badge.plus"),
            Some(Box::new(move |view: &mut Self, window: &mut Window, cx: &mut Context<Self>| view.new_file_here(new_file.clone(), dir, window, cx))),
        ));
        let new_dir = path.clone();
        entries.push((
            entry(t("pill.code.fileMenu.newFolder"), "folder.badge.plus"),
            Some(Box::new(move |view: &mut Self, window: &mut Window, cx: &mut Context<Self>| view.new_dir_here(new_dir.clone(), dir, window, cx))),
        ));
        entries.push((Item::Separator, None));
        let rename = path.clone();
        entries.push((
            entry(t("pill.code.fileMenu.rename"), "pencil"),
            Some(Box::new(move |view: &mut Self, window: &mut Window, cx: &mut Context<Self>| view.rename_target(rename.clone(), window, cx))),
        ));
        let duplicate = path.clone();
        entries.push((
            entry(t("pill.code.fileMenu.duplicate"), "plus.square.on.square"),
            Some(Box::new(move |view: &mut Self, _: &mut Window, cx: &mut Context<Self>| view.duplicate_target(duplicate.clone(), cx))),
        ));
        let delete_path = path.clone();
        entries.push((
            entry(t("pill.code.fileMenu.delete"), "trash"),
            Some(Box::new(move |view: &mut Self, window: &mut Window, cx: &mut Context<Self>| view.delete_target(delete_path.clone(), window, cx))),
        ));
        entries.push((Item::Separator, None));
        let shown_in = path.clone();
        entries.push((
            entry(if dir { t("pill.code.fileMenu.openFolderFinder") } else { t("pill.code.fileMenu.showFinder") }, "folder"),
            Some(Box::new(move |view: &mut Self, _: &mut Window, cx: &mut Context<Self>| {
                if let Err(error) = reveal(&shown_in, dir) {
                    view.show_toast(tf("pill.code.fileMenu.openAppFailed", &[("app", &t("pill.code.fileMenu.appFinder")), ("error", &error)]), cx);
                }
            })),
        ));
        let here_path = path.clone();
        entries.push((
            entry(t("pill.code.fileMenu.openTerminal"), "terminal"),
            Some(Box::new(move |view: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
                let here = if dir { here_path.clone() } else { here_path.parent().map(Path::to_path_buf).unwrap_or_else(|| here_path.clone()) };
                view.open_terminal_at(here, window, cx)
            })),
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
        let mut menu = gpui_m3::Menu::new("file-menu").width(250.).origin(Corner::TopLeft);
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
        let new_file = path.clone();
        menu = menu.item(item("file-new-file", t("pill.code.fileMenu.newFile"), "file", Box::new(move |view, window, cx| view.new_file_here(new_file.clone(), dir, window, cx))));
        let new_dir = path.clone();
        menu = menu.item(item("file-new-dir", t("pill.code.fileMenu.newFolder"), "folder", Box::new(move |view, window, cx| view.new_dir_here(new_dir.clone(), dir, window, cx))));
        let rename = path.clone();
        menu = menu.separator().item(item("file-rename", t("pill.code.fileMenu.rename"), "pen", Box::new(move |view, window, cx| view.rename_target(rename.clone(), window, cx))));
        let duplicate = path.clone();
        menu = menu.item(item("file-duplicate", t("pill.code.fileMenu.duplicate"), "copy", Box::new(move |view, _, cx| view.duplicate_target(duplicate.clone(), cx))));
        let delete_path = path.clone();
        menu = menu.item(
            MenuItem::new("file-delete", t("pill.code.fileMenu.delete"))
                .icon("trash")
                .danger(true)
                .confirm(t("pill.code.fileMenu.deleteConfirm"))
                .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                    view.file_menu = None;
                    view.delete_target(delete_path.clone(), window, cx);
                    cx.notify();
                })),
        );
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
                                .child(shown.shrink(Corner::TopLeft, menu)),
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

// --- Operaciones: crear, renombrar, duplicar y eliminar --------------------------

/// Lo que pide el diálogo del panel Archivos (crear, renombrar o eliminar).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FileOpKind {
    NewFile,
    NewDir,
    Rename,
    Delete,
}

/// El diálogo abierto del panel Archivos: qué hace, dónde y con qué campo.
pub(super) struct FileOpState {
    kind: FileOpKind,
    /// Dónde se crea, o la carpeta de lo que se renombra o elimina.
    dir: PathBuf,
    /// Lo que se renombra o se elimina.
    target: Option<PathBuf>,
    /// El campo del nombre (nada al eliminar, que solo confirma).
    field: Option<Entity<gpui_m3::TextField>>,
}

impl CodeView {
    /// Las carpetas del espacio a la vista, contra las que se valida.
    fn space_roots(&self) -> Vec<PathBuf> {
        self.workspaces.active().map(|w| w.folders.clone()).unwrap_or_default()
    }

    /// Abre el diálogo de crear, renombrar o eliminar (con el campo enfocado).
    pub(super) fn start_file_op(
        &mut self,
        kind: FileOpKind,
        dir: PathBuf,
        target: Option<PathBuf>,
        initial: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let field = (kind != FileOpKind::Delete).then(|| {
            let field = cx.new(|cx| {
                let mut field = gpui_m3::TextField::new(cx)
                    .select_all_on_focus(true)
                    .restore_on_cancel(true)
                    .placeholder(t("pill.code.fileMenu.namePlaceholder"));
                field.set_text(initial.to_string(), cx);
                field
            });
            cx.subscribe(&field, |view: &mut Self, _, event: &gpui_m3::TextFieldEvent, cx| match event {
                gpui_m3::TextFieldEvent::Submitted(_) => view.commit_file_op(cx),
                gpui_m3::TextFieldEvent::Cancelled => view.cancel_file_op(cx),
                gpui_m3::TextFieldEvent::Changed(_) => {}
            })
            .detach();
            field.read(cx).focus(window);
            field
        });
        self.file_menu = None;
        self.file_op = Some(FileOpState { kind, dir, target, field });
        cx.notify();
    }

    /// La carpeta donde crear desde `target` (ella misma si es carpeta).
    fn new_dir_for(path: &Path, dir: bool) -> PathBuf {
        if dir {
            path.to_path_buf()
        } else {
            path.parent().map(Path::to_path_buf).unwrap_or_else(|| path.to_path_buf())
        }
    }

    /// «Nuevo archivo» del menú o de la tira: pide el nombre en su carpeta.
    pub(super) fn new_file_here(&mut self, path: PathBuf, dir: bool, window: &mut Window, cx: &mut Context<Self>) {
        let dir = Self::new_dir_for(&path, dir);
        self.start_file_op(FileOpKind::NewFile, dir, None, "", window, cx);
    }

    /// «Nueva carpeta»: igual, pero crea una carpeta.
    pub(super) fn new_dir_here(&mut self, path: PathBuf, dir: bool, window: &mut Window, cx: &mut Context<Self>) {
        let dir = Self::new_dir_for(&path, dir);
        self.start_file_op(FileOpKind::NewDir, dir, None, "", window, cx);
    }

    /// «Renombrar»: el diálogo parte con el nombre de ahora seleccionado.
    pub(super) fn rename_target(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_else(|| path.clone());
        self.start_file_op(FileOpKind::Rename, dir, Some(path), &name, window, cx);
    }

    /// «Duplicar»: sin diálogo, junto al original («nombre copia.ext»).
    pub(super) fn duplicate_target(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let roots = self.space_roots();
        if !file_ops::within_space(&path, &roots) {
            self.show_toast(t("pill.code.fileMenu.outsideSpace").to_string(), cx);
            return;
        }
        match file_ops::duplicate_path(&path) {
            Ok(dest) => {
                let name = dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.after_fs_change(cx);
                self.file_cursor = Some(dest);
                self.show_toast(tf("pill.code.fileMenu.duplicatedNote", &[("name", &name)]), cx);
            }
            Err(error) => self.show_toast(error, cx),
        }
        cx.notify();
    }

    /// «Eliminar»: pide confirmación (a la papelera en macOS, para siempre en el resto).
    pub(super) fn delete_target(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_else(|| path.clone());
        self.start_file_op(FileOpKind::Delete, dir, Some(path), "", window, cx);
    }

    fn cancel_file_op(&mut self, cx: &mut Context<Self>) {
        self.file_op = None;
        cx.notify();
    }

    /// El diálogo respondió «Aceptar» (o Enter en el campo): crea, renombra o elimina.
    fn commit_file_op(&mut self, cx: &mut Context<Self>) {
        let Some(op) = self.file_op.take() else {
            return;
        };
        let roots = self.space_roots();
        if !file_ops::within_space(&op.dir, &roots)
            || op.target.as_ref().is_some_and(|target| !file_ops::within_space(target, &roots))
        {
            self.show_toast(t("pill.code.fileMenu.outsideSpace").to_string(), cx);
            cx.notify();
            return;
        }
        let name = op.field.as_ref().map(|field| field.read(cx).text().trim().to_string()).unwrap_or_default();
        match op.kind {
            FileOpKind::NewFile | FileOpKind::NewDir => {
                let made = if op.kind == FileOpKind::NewFile {
                    file_ops::create_file(&op.dir, &name)
                } else {
                    file_ops::create_dir(&op.dir, &name)
                };
                match made {
                    Ok(path) => {
                        let is_file = op.kind == FileOpKind::NewFile;
                        self.after_fs_change(cx);
                        self.file_cursor = Some(path.clone());
                        if is_file {
                            self.open_file(path, cx);
                        }
                    }
                    // El nombre no vale o ya existe: el diálogo sigue para corregirlo.
                    Err(error) => {
                        self.file_op = Some(op);
                        self.show_toast(error, cx);
                    }
                }
            }
            FileOpKind::Rename => {
                let Some(target) = op.target.clone() else {
                    return;
                };
                match file_ops::rename_path(&target, &name) {
                    Ok(dest) => {
                        self.explorer.retarget(&target, &dest);
                        self.retarget_tabs(&target, &dest);
                        self.after_fs_change(cx);
                        self.file_cursor = Some(dest.clone());
                        let shown = dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        self.show_toast(tf("pill.code.fileMenu.renamedNote", &[("name", &shown)]), cx);
                    }
                    Err(error) => {
                        self.file_op = Some(op);
                        self.show_toast(error, cx);
                    }
                }
            }
            FileOpKind::Delete => {
                let Some(target) = op.target.clone() else {
                    return;
                };
                match file_ops::remove_path(&target) {
                    Ok(Removed::Trashed) => {
                        self.close_tabs_under(&target, cx);
                        self.after_fs_change(cx);
                        self.show_toast(t("pill.code.fileMenu.trashedNote").to_string(), cx);
                    }
                    Ok(Removed::Gone) => {
                        self.close_tabs_under(&target, cx);
                        self.after_fs_change(cx);
                        self.show_toast(t("pill.code.fileMenu.deletedNote").to_string(), cx);
                    }
                    Err(error) => {
                        self.file_op = Some(op);
                        self.show_toast(error, cx);
                    }
                }
            }
        }
        cx.notify();
    }

    /// El diálogo de crear, renombrar o eliminar, sobre el panel.
    pub(super) fn file_op_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let op = self.file_op.as_ref()?;
        let (id, icon, title) = match op.kind {
            FileOpKind::NewFile => ("file-new-file", "file", t("pill.code.fileMenu.newFileTitle")),
            FileOpKind::NewDir => ("file-new-dir", "folder", t("pill.code.fileMenu.newFolderTitle")),
            FileOpKind::Rename => ("file-rename", "pen", t("pill.code.fileMenu.renameTitle")),
            FileOpKind::Delete => ("file-delete", "trash", t("pill.code.fileMenu.deleteTitle")),
        };
        let name = op.target.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let body: AnyElement = match &op.field {
            Some(field) => div().child(field.clone()).into_any_element(),
            None => {
                let text = if cfg!(target_os = "macos") {
                    tf("pill.code.fileMenu.deleteBodyTrash", &[("name", &name)])
                } else {
                    tf("pill.code.fileMenu.deleteBodyGone", &[("name", &name)])
                };
                div().text_color(super::style::t().muted).child(text).into_any_element()
            }
        };
        let ok_label = match op.kind {
            FileOpKind::NewFile | FileOpKind::NewDir => t("pill.code.fileMenu.create"),
            FileOpKind::Rename => t("pill.code.fileMenu.rename"),
            FileOpKind::Delete => t("pill.code.fileMenu.delete"),
        };
        Some(
            Dialog::new(id)
                .icon(icon)
                .title(title)
                .width(px(400.))
                .on_dismiss(cx.listener(|view, _: &ClickEvent, _, cx| view.cancel_file_op(cx)))
                .child(body)
                .action(Button::new("file-op-cancel", t("pill.code.editor.cancel")).text().on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.cancel_file_op(cx))))
                .action(
                    Button::new("file-op-ok", ok_label)
                        .filled()
                        .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.commit_file_op(cx))),
                )
                .into_any_element(),
        )
    }

    /// Cierra las pestañas del editor que muestran `removed` o algo de dentro
    /// (lo eliminado ya no se puede editar).
    fn close_tabs_under(&mut self, removed: &Path, cx: &mut Context<Self>) {
        let mut indices: Vec<usize> = self
            .tabs
            .files
            .iter()
            .enumerate()
            .filter(|(_, open)| open.path.as_path() == removed || open.path.starts_with(removed))
            .map(|(index, _)| index)
            .collect();
        indices.sort_unstable_by(|a, b| b.cmp(a));
        for index in indices {
            self.request_close(index, cx);
        }
        if self.active_file.as_ref().is_some_and(|p| p.as_path() == removed || p.starts_with(removed)) {
            self.active_file = None;
        }
        if self.file_cursor.as_ref().is_some_and(|p| p.as_path() == removed || p.starts_with(removed)) {
            self.file_cursor = None;
        }
    }

    /// Tras renombrar `old` a `new`: las pestañas, el visor y el resaltado siguen al archivo.
    fn retarget_tabs(&mut self, old: &Path, new: &Path) {
        let moved = |path: &Path| {
            if path == old {
                Some(new.to_path_buf())
            } else {
                path.strip_prefix(old).ok().map(|rest| new.join(rest))
            }
        };
        for open in &mut self.tabs.files {
            if let Some(dest) = moved(&open.path) {
                open.path = dest;
            }
        }
        if let Some(doc) = self.doc.as_mut() {
            if let Some(dest) = moved(&doc.path) {
                doc.path = dest;
                doc.reload();
            }
        }
        if let Some(active) = self.active_file.as_mut() {
            if let Some(dest) = moved(active) {
                *active = dest;
            }
        }
        if let Some(cursor) = self.file_cursor.as_mut() {
            if let Some(dest) = moved(cursor) {
                *cursor = dest;
            }
        }
    }

    /// Tras crear, renombrar, duplicar o eliminar: se relee el árbol y el índice
    /// de búsqueda (se arma de nuevo al buscar).
    fn after_fs_change(&mut self, cx: &mut Context<Self>) {
        self.explorer.refresh();
        self.file_index = None;
        if !self.file_search.read(cx).text().trim().is_empty() {
            self.ensure_index(cx);
        }
        self.refresh_changes(cx);
    }

    /// Alterna una carpeta del árbol y lo guarda para este espacio.
    pub(super) fn toggle_tree(&mut self, path: &Path, root: bool) {
        self.explorer.toggle(path, root);
        self.save_tree_state();
    }

    /// Guarda las carpetas abiertas del espacio a la vista.
    pub(super) fn save_tree_state(&mut self) {
        if let Some(id) = self.active_workspace() {
            self.tree_state.set_open(id, self.explorer.open_paths());
        }
    }

    /// Devuelve lo abierto y el interruptor de ignorados guardados.
    pub(super) fn restore_tree_state(&mut self) {
        self.explorer.set_show_ignored(self.tree_state.show_ignored());
        if let Some(id) = self.active_workspace() {
            self.explorer.set_open_paths(&self.tree_state.open_for(id));
        }
    }

    /// El interruptor «Mostrar ignorados»: se guarda y el árbol se relee.
    /// (Lo llama el `Switch` del panel, que ya redibuja al cambiar.)
    pub(super) fn set_show_ignored(&mut self, show: bool) {
        self.tree_state.set_show_ignored(show);
        self.explorer.set_show_ignored(show);
    }

    /// Mira cada ~2 s si cambió algo de lo abierto del árbol (solo con la
    /// ventana activa y la pestaña Archivos a la vista): el mtime de esas
    /// carpetas, sin recorrer nada más.
    pub(super) fn watch_files(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(std::time::Duration::from_secs(2)).await;
            let alive = this.update(cx, |view, cx| view.poll_tree_files(cx));
            if alive.is_err() {
                break;
            }
        })
        .detach();
    }

    fn poll_tree_files(&mut self, cx: &mut Context<Self>) {
        if !self.window_active || self.side != Some(Side::Files) {
            return;
        }
        if self.explorer.poll() {
            self.after_fs_change(cx);
            cx.notify();
        }
    }
}
