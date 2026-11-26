//! El editor de código del panel derecho: el `CodeEditor` de gpui-m3 con una pestaña por
//! archivo abierto (como `Tab` de `store.ts` en la referencia). Aquí vive el estado de las pestañas,
//! guardar, recargar si Claude edita el archivo y armar el contexto que el composer manda
//! con la selección. La vista (`view.rs`) y `mod.rs` solo lo enganchan.
//!
//! Los diffs del panel de Cambios siguen siendo del visor (`kit::viewer::Doc`); su botón
//! «Abrir» pasa el archivo a una pestaña de aquí.

use crate::i18n::tf;
use std::ops::Range;
use std::path::{Path, PathBuf};

use gpui::{
    div, prelude::*, px, AnyElement, App, ClickEvent, Context, Entity, KeyBinding, SharedString, Window,
};
use gpui_m3::{Banner, Button, CodeEditor, CodeEditorEvent, Dialog, SyntaxLines};

use super::overlay;
use super::style::t;
use super::{panel, same_path, CodeView, GoToLine, Side};

/// Lo más grande que se abre para editar (el mismo tope que el visor).
const MAX_BYTES: u64 = 4 * 1024 * 1024;

/// Los atajos de Atic Code que dentro del editor no actúan (Ctrl+L, Ctrl+K, Ctrl+P…): se
/// anulan con `NoAction` en el contexto del `CodeEditor`, como hace la terminal. Ctrl+S, Esc
/// y las teclas de edición siguen siendo del editor. Las teclas salen de
/// `shortcuts::editor_keys` (siguen a los atajos que el usuario cambie).

/// El contexto de teclas que pone `CodeEditor` de gpui-m3.
const CONTEXT: &str = "M3CodeEditor";

pub fn bind_keys(cx: &mut App, keys: &[String]) {
    // Ctrl+G (⌘G en macOS) dentro del editor es «Ir a línea»: no se anula como
    // los demás atajos de Atic Code, sino que salta a una línea del archivo.
    cx.bind_keys(keys.iter().filter(|key| key.as_str() != "ctrl-g").map(|key| KeyBinding::new(key, gpui::NoAction, Some(CONTEXT))));
    cx.bind_keys([KeyBinding::new("ctrl-g", super::GoToLine, Some(CONTEXT))]);
}

/// Lo que muestra una pestaña: el editor, una imagen, o el motivo por el que no se puede
/// editar.
pub(super) enum Body {
    Editor(Entity<CodeEditor>),
    /// Una imagen (también un GIF, que se mueve): se mira, no se edita.
    Image,
    Note(String),
}

/// Las imágenes que se muestran en vez de «es un archivo binario». Un SVG es texto: se edita.
pub(super) fn is_image(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "ico")
}

/// Un archivo abierto.
pub(super) struct Open {
    pub path: PathBuf,
    pub body: Body,
    /// El archivo cambió en el disco y el editor tiene cambios propios: no se pisó.
    pub stale: bool,
    /// Un `.md` mirado como vista previa en vez de texto (`preview`).
    pub preview: bool,
}

/// Las pestañas de archivos abiertos y lo que depende de ellas.
#[derive(Default)]
pub(super) struct Tabs {
    pub files: Vec<Open>,
    /// La pestaña elegida (la que se ve cuando el panel derecho está en `Side::File`).
    pub active: Option<usize>,
    /// El archivo con cambios sin guardar que se quiere cerrar (el diálogo).
    pub confirm: Option<PathBuf>,
    pub confirm_last: overlay::Last<PathBuf>,
    /// El diálogo «Ir a línea» del editor.
    pub goto: Option<Entity<gpui_m3::TextField>>,
    /// El archivo cuyo chip de contexto se quitó con la X (vuelve al cambiar de pestaña).
    skip: Option<PathBuf>,
    /// Al dibujar, el foco pasa al editor (se abrió un archivo).
    focus: bool,
}

/// El archivo y las líneas (desde 1) que el composer manda como contexto.
pub(super) struct EditorContext {
    pub path: PathBuf,
    pub lines: (usize, usize),
}

impl Tabs {
    pub fn index_of(&self, path: &Path) -> Option<usize> {
        self.files.iter().position(|f| same_path(&f.path, path))
    }

    /// Hay una pestaña elegida.
    pub fn showing(&self) -> bool {
        self.active.is_some_and(|i| i < self.files.len())
    }

    pub fn current(&self) -> Option<&Open> {
        self.active.and_then(|i| self.files.get(i))
    }

    pub(super) fn select(&mut self, index: usize) {
        if self.active != Some(index) {
            self.skip = None;
        }
        self.active = Some(index);
        self.focus = true;
    }

    /// Para actualizar los colores del resaltado cuando cambia el tema.
    pub fn editors(&self) -> impl Iterator<Item = &Entity<CodeEditor>> {
        self.files.iter().filter_map(|f| match &f.body {
            Body::Editor(editor) => Some(editor),
            Body::Image | Body::Note(_) => None,
        })
    }
}

// --- Lógica pura -------------------------------------------------------------------------

/// La pestaña a la vista después de cerrar la `closed` de un total de `len` (antes de
/// cerrarla), como `closeTab` de la referencia: si era la activa queda la vecina que ocupa su lugar
/// o, si era la última, la anterior; si era otra, la activa sigue siendo la misma.
pub(super) fn after_close(len: usize, closed: usize, active: Option<usize>) -> Option<usize> {
    let left = len.checked_sub(1)?;
    if left == 0 {
        return None;
    }
    match active? {
        a if a == closed => Some(closed.min(left - 1)),
        a if a > closed => Some(a - 1),
        a => Some(a),
    }
}

/// Qué hacer cuando el archivo cambió en el disco.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Reload {
    /// El texto es el mismo.
    Nothing,
    /// Sin cambios propios: se carga el del disco.
    Replace,
    /// Con cambios propios: no se pisan, se avisa.
    Warn,
}

pub(super) fn reload_decision(dirty: bool, same_text: bool) -> Reload {
    if same_text {
        Reload::Nothing
    } else if dirty {
        Reload::Warn
    } else {
        Reload::Replace
    }
}

/// Las líneas del editor (desde 0, fin excluido) a las del contexto: desde 1 y con el fin
/// incluido. Un cursor sin selección da su propia línea.
pub(super) fn one_based(lines: Range<usize>) -> (usize, usize) {
    (lines.start + 1, lines.end.max(lines.start + 1))
}

/// `12` o `12-20`.
pub(super) fn lines_label(lines: (usize, usize)) -> String {
    if lines.0 == lines.1 {
        lines.0.to_string()
    } else {
        format!("{}-{}", lines.0, lines.1)
    }
}

/// El texto del chip: `nombre:12-20`.
pub(super) fn chip_label(name: &str, lines: (usize, usize)) -> String {
    format!("{name}:{}", lines_label(lines))
}

/// Lo que se le agrega al mensaje con el contexto del editor (`send` de la referencia), salvo que el
/// texto ya mencione el archivo.
pub(super) fn context_note(text: &str, rel: &str, lines: (usize, usize)) -> Option<String> {
    if text.contains(&format!("@{rel}")) {
        return None;
    }
    let word = if lines.0 == lines.1 { "línea" } else { "líneas" };
    Some(format!("(Contexto: @{rel}, {word} {})", lines_label(lines)))
}

/// Se sangra con tabulaciones si más líneas empiezan con una que con espacios.
pub(super) fn uses_tabs(text: &str) -> bool {
    let (mut tabs, mut spaces) = (0usize, 0usize);
    for line in text.lines() {
        if line.starts_with('\t') {
            tabs += 1;
        } else if line.starts_with("  ") {
            spaces += 1;
        }
    }
    tabs > spaces
}

/// Se puede mirar como vista previa: un Markdown.
pub(super) fn is_markdown(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    matches!(ext.as_str(), "md" | "mdx" | "markdown")
}

/// «42» o «42:7» a línea y columna (desde 1), si caen dentro del archivo.
pub(super) fn parse_goto(text: &str, lines: usize) -> Option<(usize, usize)> {
    let (line, col) = match text.trim().split_once(':') {
        Some((line, col)) => (line.trim(), col.trim()),
        None => (text.trim(), "1"),
    };
    let line: usize = line.parse().ok()?;
    let col: usize = if col.is_empty() { 1 } else { col.parse().ok()? };
    if line == 0 || line > lines.max(1) || col == 0 {
        return None;
    }
    Some((line, col))
}

/// El texto del archivo, o por qué no se puede editar.
pub(super) fn read_text(path: &Path) -> Result<String, String> {
    match std::fs::metadata(path) {
        Err(_) => return Err(crate::i18n::t("pill.code.editor.gone").into()),
        Ok(meta) if meta.len() > MAX_BYTES => return Err(crate::i18n::t("pill.code.editor.tooBig").into()),
        Ok(_) => {}
    }
    let bytes = std::fs::read(path).map_err(|error| tf("pill.code.editor.readFailed", &[("error", &error)]))?;
    if bytes.iter().take(8192).any(|b| *b == 0) {
        return Err(crate::i18n::t("pill.code.editor.binary").into());
    }
    // Un archivo que no es UTF-8 se vería bien pero al guardarlo se dañaría.
    String::from_utf8(bytes).map_err(|_| crate::i18n::t("pill.code.editor.notUtf8").into())
}

/// `fs_write_text` de la referencia: escribe a un temporal y lo renombra, para no
/// dejar el archivo a medias si algo falla en el camino. Si `path` es un enlace se
/// escribe su destino (el enlace queda), y el archivo nuevo conserva los permisos.
pub(super) fn write_text(path: &Path, text: &str) -> std::io::Result<()> {
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut tmp_name = target.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(".atic-tmp~");
    let tmp = target.with_file_name(tmp_name);
    std::fs::write(&tmp, text)?;
    // En Windows el único permiso es «solo lectura», y con él el renombre falla igual.
    #[cfg(unix)]
    if let Ok(meta) = std::fs::metadata(&target) {
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
    }
    match std::fs::rename(&tmp, &target) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = std::fs::remove_file(&tmp);
            Err(error)
        }
    }
}

fn name_of(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string())
}

// --- Enganche con la vista ---------------------------------------------------------------

impl CodeView {
    /// Abre el archivo en una pestaña del editor (o pasa a la que ya lo tiene).
    pub(super) fn open_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.active_file = Some(path.clone());
        let index = match self.tabs.index_of(&path) {
            Some(index) => index,
            None => {
                let body = self.load_body(&path, cx);
                self.tabs.files.push(Open { path, body, stale: false, preview: false });
                self.tabs.files.len() - 1
            }
        };
        self.tabs.select(index);
        self.set_side(Some(Side::File), cx);
    }

    /// Abre el archivo y deja el cursor en esa línea (desde 1): las rutas del chat `a.rs:42`.
    pub(super) fn open_file_at(&mut self, path: PathBuf, line: u32, cx: &mut Context<Self>) {
        self.open_file(path.clone(), cx);
        if let Some(Body::Editor(editor)) = self.tabs.index_of(&path).map(|i| &self.tabs.files[i].body) {
            editor.update(cx, |editor, cx| editor.go_to((line as usize).saturating_sub(1), 0, cx));
        }
    }

    fn load_body(&mut self, path: &Path, cx: &mut Context<Self>) -> Body {
        if is_image(path) && path.is_file() {
            return Body::Image;
        }
        match read_text(path) {
            Ok(text) => Body::Editor(self.new_editor(path, &text, cx)),
            Err(note) => Body::Note(note),
        }
    }

    fn new_editor(&mut self, path: &Path, text: &str, cx: &mut Context<Self>) -> Entity<CodeEditor> {
        let shown: SharedString = path.display().to_string().into();
        let syntax = SyntaxLines::for_path(&path.to_string_lossy());
        let editor = cx.new(|cx| {
            let mut editor = CodeEditor::new(cx).path(shown).font_size(px(12.5)).hard_tabs(uses_tabs(text)).with_text(text);
            if let Some(syntax) = syntax {
                editor = editor.highlighter(syntax);
            }
            editor
        });
        cx.subscribe(&editor, |view: &mut Self, editor, event: &CodeEditorEvent, cx| match event {
            CodeEditorEvent::Save => {
                if let Some(index) = view.tab_of_editor(&editor) {
                    view.save_tab(index, cx);
                }
            }
            // La pestaña sucia y el chip de contexto siguen al editor.
            CodeEditorEvent::Changed | CodeEditorEvent::SelectionChanged => cx.notify(),
            CodeEditorEvent::Cancelled => {}
        })
        .detach();
        editor
    }

    fn tab_of_editor(&self, editor: &Entity<CodeEditor>) -> Option<usize> {
        self.tabs.files.iter().position(|f| matches!(&f.body, Body::Editor(e) if e == editor))
    }

    pub(super) fn tab_dirty(&self, index: usize, cx: &App) -> bool {
        matches!(self.tabs.files.get(index).map(|f| &f.body), Some(Body::Editor(e)) if e.read(cx).is_dirty())
    }

    /// Cuántas pestañas tienen cambios sin guardar.
    pub(super) fn tabs_dirty(&self, cx: &App) -> usize {
        (0..self.tabs.files.len()).filter(|index| self.tab_dirty(*index, cx)).count()
    }

    /// Guarda todas las pestañas sucias; `false` si alguna falló.
    pub(super) fn save_all_dirty(&mut self, cx: &mut Context<Self>) -> bool {
        let dirty: Vec<usize> = (0..self.tabs.files.len()).filter(|index| self.tab_dirty(*index, cx)).collect();
        dirty.iter().all(|index| self.save_tab(*index, cx))
    }

    /// Ctrl+S: escribe el archivo y, si salió bien, la pestaña deja de estar sucia y git se
    /// pone al día. Si falla, un aviso y el texto sigue sin guardar.
    pub(super) fn save_tab(&mut self, index: usize, cx: &mut Context<Self>) -> bool {
        let Some(open) = self.tabs.files.get(index) else {
            return false;
        };
        let Body::Editor(editor) = &open.body else {
            return false;
        };
        let (editor, path) = (editor.clone(), open.path.clone());
        let text = editor.read(cx).text();
        match write_text(&path, &text) {
            Ok(()) => {
                editor.update(cx, |editor, cx| editor.mark_saved(cx));
                self.tabs.files[index].stale = false;
                self.refresh_changes_now(cx);
                cx.notify();
                true
            }
            Err(error) => {
                self.show_toast(tf("pill.code.editor.saveFailed", &[("name", &name_of(&path)), ("error", &error)]), cx);
                false
            }
        }
    }

    /// Cerrar una pestaña: con cambios sin guardar, antes pregunta.
    pub(super) fn request_close(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.tab_dirty(index, cx) {
            self.tabs.confirm = Some(self.tabs.files[index].path.clone());
            cx.notify();
        } else {
            self.close_tab(index, cx);
        }
    }

    fn close_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.tabs.files.len() {
            return;
        }
        // A la vista en el panel: queda su vecina de la tira (otro archivo, el diff, una consola…).
        let shown_next = self.next_after_close(&panel::Key::File(self.tabs.files[index].path.clone()), cx);
        let next = after_close(self.tabs.files.len(), index, self.tabs.active);
        let was_active = self.tabs.active == Some(index);
        self.tabs.files.remove(index);
        self.tabs.active = next;
        if was_active {
            self.tabs.skip = None;
            if let Some(open) = next.and_then(|i| self.tabs.files.get(i)) {
                self.active_file = Some(open.path.clone());
            }
        }
        self.after_panel_close(shown_next, None, cx);
        cx.notify();
    }

    /// El diálogo respondido: guardar y cerrar, descartar y cerrar, o dejarlo.
    fn answer_close(&mut self, save: Option<bool>, cx: &mut Context<Self>) {
        let Some(path) = self.tabs.confirm.take() else {
            return;
        };
        if let (Some(save), Some(index)) = (save, self.tabs.index_of(&path)) {
            if !save || self.save_tab(index, cx) {
                self.close_tab(index, cx);
            }
        }
        cx.notify();
    }

    /// Claude editó estos archivos: los abiertos sin cambios propios se recargan (con el
    /// cursor donde estaba); los que sí los tienen no se pisan y quedan con el aviso.
    pub(super) fn files_edited(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        for path in paths {
            if let Some(index) = self.tabs.index_of(path) {
                self.reload_tab(index, cx);
            }
        }
    }

    fn reload_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        let path = self.tabs.files[index].path.clone();
        // Una imagen se vuelve a leer sola al dibujarse.
        if matches!(self.tabs.files[index].body, Body::Image) {
            return;
        }
        let read = read_text(&path);
        let editor = match &self.tabs.files[index].body {
            Body::Editor(editor) => Some(editor.clone()),
            Body::Image | Body::Note(_) => None,
        };
        match (editor, read) {
            (Some(editor), Ok(text)) => {
                let dirty = editor.read(cx).is_dirty();
                let same = editor.read(cx).text() == text;
                match reload_decision(dirty, same) {
                    Reload::Nothing => {}
                    Reload::Replace => {
                        replace_text(&editor, &text, cx);
                        self.tabs.files[index].stale = false;
                    }
                    Reload::Warn => self.tabs.files[index].stale = true,
                }
            }
            // Con cambios propios, el archivo desaparecido o ilegible no se lleva el texto.
            (Some(editor), Err(_)) if editor.read(cx).is_dirty() => self.tabs.files[index].stale = true,
            (Some(_), Err(note)) => self.tabs.files[index].body = Body::Note(note),
            (None, Ok(text)) => self.tabs.files[index].body = Body::Editor(self.new_editor(&path, &text, cx)),
            (None, Err(note)) => self.tabs.files[index].body = Body::Note(note),
        }
        cx.notify();
    }

    /// «Recargar» del aviso: se descartan los cambios propios y se lee el disco.
    fn reload_discarding(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(open) = self.tabs.files.get(index) else {
            return;
        };
        let (path, body) = (open.path.clone(), &open.body);
        let Body::Editor(editor) = body else {
            return;
        };
        let editor = editor.clone();
        match read_text(&path) {
            Ok(text) => {
                replace_text(&editor, &text, cx);
                self.tabs.files[index].stale = false;
            }
            Err(error) => self.show_toast(error, cx),
        }
        cx.notify();
    }

    /// Al cambiar de espacio: se cierran las pestañas sin cambios y las demás quedan
    /// guardadas (no se pierde lo escrito), sin mostrarse.
    pub(super) fn leave_workspace_tabs(&mut self, cx: &mut Context<Self>) {
        let dirty: Vec<bool> = (0..self.tabs.files.len()).map(|i| self.tab_dirty(i, cx)).collect();
        let mut keep = dirty.into_iter();
        self.tabs.files.retain(|_| keep.next().unwrap_or(true));
        self.tabs.active = None;
        self.tabs.skip = None;
    }

    /// Ya dibujando: pasa el foco al editor recién abierto.
    pub(super) fn focus_pending_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.tabs.focus) {
            return;
        }
        if let Some(Body::Editor(editor)) = self.tabs.current().map(|f| &f.body) {
            editor.read(cx).focus(window);
        }
    }

    /// Los colores del resaltado vienen del tema: se piden otra vez cuando cambia.
    pub(super) fn refresh_editor_colors(&mut self, cx: &mut Context<Self>) {
        let editors: Vec<_> = self.tabs.editors().cloned().collect();
        for editor in editors {
            editor.update(cx, |editor, cx| editor.refresh_highlights(cx));
        }
    }

    /// El archivo y las líneas del editor a la vista, que el composer muestra como chip y
    /// manda con el mensaje. Nada si el panel muestra otra pestaña (o está cerrado) o si se
    /// quitó.
    pub(super) fn editor_context(&self, cx: &App) -> Option<EditorContext> {
        if self.side != Some(Side::File) {
            return None;
        }
        let open = self.tabs.current()?;
        let Body::Editor(editor) = &open.body else {
            return None;
        };
        if self.tabs.skip.as_deref() == Some(open.path.as_path()) {
            return None;
        }
        Some(EditorContext { path: open.path.clone(), lines: one_based(editor.read(cx).selection().lines()) })
    }

    /// La X del chip de contexto.
    pub(super) fn skip_editor_context(&mut self, cx: &mut Context<Self>) {
        self.tabs.skip = self.tabs.current().map(|f| f.path.clone());
        cx.notify();
    }

    /// «Ir a línea» (Ctrl+G con el editor enfocado, o su botón): pide la línea y salta a ella.
    pub(super) fn go_to_line(&mut self, _: &GoToLine, window: &mut Window, cx: &mut Context<Self>) {
        if self.side != Some(Side::File) {
            return;
        }
        let Some(index) = self.tabs.active.filter(|i| *i < self.tabs.files.len()) else {
            return;
        };
        if !matches!(self.tabs.files[index].body, Body::Editor(_)) {
            return;
        }
        let field = cx.new(|cx| {
            gpui_m3::TextField::new(cx)
                .select_all_on_focus(true)
                .restore_on_cancel(true)
                .placeholder(crate::i18n::t("pill.code.editor.goToPlaceholder"))
        });
        cx.subscribe(&field, |view: &mut Self, _, event: &gpui_m3::TextFieldEvent, cx| match event {
            gpui_m3::TextFieldEvent::Submitted(_) => view.commit_goto(cx),
            gpui_m3::TextFieldEvent::Cancelled => {
                view.tabs.goto = None;
                cx.notify();
            }
            gpui_m3::TextFieldEvent::Changed(_) => {}
        })
        .detach();
        field.read(cx).focus(window);
        self.tabs.goto = Some(field);
        cx.notify();
    }

    fn commit_goto(&mut self, cx: &mut Context<Self>) {
        let text = self.tabs.goto.as_ref().map(|field| field.read(cx).text().trim().to_string()).unwrap_or_default();
        let current = self.tabs.active.filter(|i| *i < self.tabs.files.len());
        let editor = current.and_then(|i| match &self.tabs.files[i].body {
            Body::Editor(editor) => Some(editor.clone()),
            Body::Image | Body::Note(_) => None,
        });
        let Some(editor) = editor else {
            self.tabs.goto = None;
            cx.notify();
            return;
        };
        let lines = editor.read(cx).text().lines().count();
        match parse_goto(&text, lines) {
            Some((line, col)) => {
                editor.update(cx, |editor, cx| editor.go_to(line - 1, col - 1, cx));
                self.tabs.goto = None;
            }
            None => self.show_toast(crate::i18n::t("pill.code.editor.lineInvalid").to_string(), cx),
        }
        cx.notify();
    }

    /// El diálogo «Ir a línea», sobre el panel.
    pub(super) fn goto_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let field = self.tabs.goto.clone()?;
        Some(
            Dialog::new("goto-line")
                .icon("info")
                .title(crate::i18n::t("pill.code.editor.goToTitle"))
                .width(px(320.))
                .on_dismiss(cx.listener(|view, _: &ClickEvent, _, cx| {
                    view.tabs.goto = None;
                    cx.notify();
                }))
                .child(div().child(field))
                .action(Button::new("goto-cancel", crate::i18n::t("pill.code.editor.cancel")).text().on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                    view.tabs.goto = None;
                    cx.notify();
                })))
                .action(
                    Button::new("goto-ok", crate::i18n::t("pill.code.editor.goToLine"))
                        .filled()
                        .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.commit_goto(cx))),
                )
                .into_any_element(),
        )
    }

    /// Alterna el Markdown entre texto y vista previa (solo `.md`).
    fn toggle_md_preview(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(open) = self.tabs.files.get_mut(index) {
            open.preview = !open.preview;
        }
        cx.notify();
    }

    // --- Lo que se dibuja ---

    /// El cuerpo del panel con el editor: el aviso de «cambió en el disco» y el texto (la
    /// pestaña va en la tira del panel, `panel.rs`).
    pub(super) fn editor_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = t();
        let Some(index) = self.tabs.active.filter(|i| *i < self.tabs.files.len()) else {
            return div().into_any_element();
        };
        let open = &self.tabs.files[index];
        let content = match &open.body {
            Body::Editor(editor) if open.preview && is_markdown(&open.path) => {
                let text = editor.read(cx).text();
                div()
                    .id("md-preview")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .p(px(16.))
                    .child(super::view::markdown_live(&format!("md-preview-{}", open.path.display()), &text, false, cx))
                    .into_any_element()
            }
            Body::Editor(editor) => div()
                .flex_1()
                .min_h(px(0.))
                .relative()
                // Un contenedor de tamaño fijo: el editor llena a su padre.
                .child(div().absolute().top_0().left_0().size_full().child(editor.clone()))
                .into_any_element(),
            Body::Image => div()
                .flex_1()
                .min_h(px(0.))
                .relative()
                .child(
                    div()
                        .id(SharedString::from(format!("file-img-open-{}", open.path.display())))
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .p(px(16.))
                        .cursor_pointer()
                        .tooltip(crate::hover::tip(crate::i18n::t("pill.code.editor.zoomImage")))
                        // Un clic la abre en grande, con zoom y arrastre (el visor de los diagramas).
                        .on_click({
                            let path = open.path.clone();
                            cx.listener(move |view, _: &ClickEvent, _, cx| view.open_image_viewer(path.clone(), cx))
                        })
                        .child(
                            // Con id: sin él GPUI no avanza los cuadros de un GIF o un WebP animado.
                            gpui::img(open.path.clone())
                                .id(SharedString::from(format!("file-img-{}", open.path.display())))
                                .size_full()
                                .object_fit(gpui::ObjectFit::Contain)
                                .with_fallback(|| div().into_any_element()),
                        ),
                )
                .into_any_element(),
            Body::Note(note) => div().flex_1().p(px(16.)).text_color(t.muted).child(note.clone()).into_any_element(),
        };
        // Clic derecho sobre lo que no es el editor (una imagen, un binario): el menú del
        // archivo, con «Mostrar en Finder» o en el Explorador. El editor tiene el suyo.
        let content = if matches!(open.body, Body::Editor(_)) {
            content
        } else {
            let path = open.path.clone();
            div()
                .flex_1()
                .min_h(px(0.))
                .flex()
                .flex_col()
                .on_mouse_down(
                    gpui::MouseButton::Right,
                    cx.listener(move |view, event: &gpui::MouseDownEvent, window, cx| view.open_file_menu(path.clone(), false, event.position, window, cx)),
                )
                .child(content)
                .into_any_element()
        };
        let stale = open.stale.then(|| {
            Banner::new(SharedString::from(format!("stale-{index}")))
                .warning()
                .icon("info")
                .text(crate::i18n::t("pill.code.editor.changedOnDisk"))
                .action(Button::new("stale-reload", crate::i18n::t("pill.code.editor.reload")).text().on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    view.reload_discarding(index, cx)
                })))
                .action(Button::new("stale-keep", crate::i18n::t("pill.code.editor.keepMine")).text().on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    if let Some(open) = view.tabs.files.get_mut(index) {
                        open.stale = false;
                    }
                    cx.notify();
                })))
        });
        // La barra del editor: «Ir a línea» y, en un `.md`, alternar texto y vista previa.
        let toolbar = matches!(open.body, Body::Editor(_)).then(|| {
            let preview_label = if open.preview { crate::i18n::t("pill.code.editor.editCode") } else { crate::i18n::t("pill.code.editor.preview") };
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(4.))
                .px(px(8.))
                .py(px(4.))
                .child(div().flex_1())
                .child(Button::new("editor-goto", crate::i18n::t("pill.code.editor.goToLine")).text().on_click(cx.listener(|view, _: &ClickEvent, window, cx| {
                    view.go_to_line(&GoToLine, window, cx)
                })))
                .when(is_markdown(&open.path), |el| {
                    el.child(Button::new("editor-preview", preview_label).text().on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.toggle_md_preview(index, cx)
                    })))
                })
        });
        div()
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .bg(super::view::code_bg())
            .when_some(stale, |el, banner| el.child(div().px(px(8.)).py(px(6.)).child(banner)))
            .when_some(toolbar, |el, bar| el.child(bar))
            .child(content)
            .into_any_element()
    }

    /// «Tiene cambios sin guardar»: Guardar, Descartar o Cancelar (`close` de ContextPanel).
    pub(super) fn close_dialog(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let shown = self.tabs.confirm_last.show("file-close-presence", self.tabs.confirm.clone(), window, cx)?;
        let name = name_of(&shown.value);
        let t = t();
        Some(
            Dialog::new("file-close")
                .exit(shown.progress)
                .icon("info")
                .title(crate::i18n::t("pill.code.editor.unsavedTitle"))
                .width(px(420.))
                .on_dismiss(cx.listener(|view, _: &ClickEvent, _, cx| view.answer_close(None, cx)))
                .child(div().text_color(t.muted).child(tf("pill.code.editor.unsavedBody", &[("name", &name)])))
                .action(Button::new("file-close-cancel", crate::i18n::t("pill.code.editor.cancel")).text().on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.answer_close(None, cx))))
                .action(
                    Button::new("file-close-discard", crate::i18n::t("pill.code.editor.discard"))
                        .text()
                        .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.answer_close(Some(false), cx))),
                )
                .action(
                    Button::new("file-close-save", crate::i18n::t("pill.code.editor.save"))
                        .filled()
                        .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.answer_close(Some(true), cx))),
                )
                .into_any_element(),
        )
    }

    /// «Hay archivos sin guardar» o «hay agentes trabajando» al cerrar la
    /// ventana: Guardar todo, Descartar (o Cerrar igual) o Cancelar.
    pub(super) fn confirm_close_dialog(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = t();
        let dirty = self.tabs_dirty(cx);
        let working = match self.working_chats() {
            0 => None,
            1 => Some(crate::i18n::t("pill.code.editor.workingOne").to_string()),
            n => Some(tf("pill.code.editor.workingMany", &[("n", &n)])),
        };
        let text = match dirty {
            0 => String::new(),
            1 => crate::i18n::t("pill.code.editor.dirtyOne").to_string(),
            n => tf("pill.code.editor.dirtyMany", &[("n", &n)]),
        };
        let dialog = Dialog::new("window-close")
            .icon("info")
            .title(crate::i18n::t("pill.code.editor.closeTitle"))
            .width(px(430.))
            .on_dismiss(cx.listener(|view, _: &ClickEvent, _, cx| {
                view.close_confirm = false;
                cx.notify();
            }))
            .when_some(working, |el, working| el.child(div().text_color(t.muted).child(working)))
            .when(dirty > 0, |el| el.child(div().text_color(t.muted).child(text)))
            .action(Button::new("window-close-cancel", crate::i18n::t("pill.code.editor.cancel")).text().on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                view.close_confirm = false;
                cx.notify();
            })));
        if dirty == 0 {
            return dialog
                .action(
                    Button::new("window-close-anyway", crate::i18n::t("pill.code.editor.closeAnyway"))
                        .filled()
                        .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.close_discard(window, cx))),
                )
                .into_any_element();
        }
        dialog
            .action(
                Button::new("window-close-discard", crate::i18n::t("pill.code.editor.discardClose"))
                    .text()
                    .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.close_discard(window, cx))),
            )
            .action(
                Button::new("window-close-save", crate::i18n::t("pill.code.editor.saveAllClose"))
                    .filled()
                    .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.close_save(window, cx))),
            )
            .into_any_element()
    }
}

/// Carga `text` en el editor dejando el cursor (o la selección) donde estaba.
fn replace_text(editor: &Entity<CodeEditor>, text: &str, cx: &mut App) {
    let selection = editor.read(cx).selection();
    editor.update(cx, |editor, cx| {
        editor.set_text(text, cx);
        editor.select(selection, cx);
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn las_imagenes_se_miran_y_el_svg_se_edita() {
        assert!(is_image(Path::new("/x/gato.GIF")) && is_image(Path::new("a.png")) && is_image(Path::new("b.jpeg")));
        assert!(!is_image(Path::new("logo.svg")) && !is_image(Path::new("main.rs")) && !is_image(Path::new("sin_extension")));
    }

    #[test]
    fn el_markdown_se_puede_mirar_como_vista_previa() {
        assert!(is_markdown(Path::new("notas.md")) && is_markdown(Path::new("README.MDX")));
        assert!(!is_markdown(Path::new("main.rs")) && !is_markdown(Path::new("nota.txt")));
    }

    #[test]
    fn ir_a_linea_acepta_linea_y_columna_si_existen() {
        assert_eq!(parse_goto("42", 100), Some((42, 1)));
        assert_eq!(parse_goto(" 7:3 ", 100), Some((7, 3)));
        assert_eq!(parse_goto("100", 100), Some((100, 1)));
        // Fuera del archivo, cero o texto: no vale.
        assert_eq!(parse_goto("0", 100), None);
        assert_eq!(parse_goto("101", 100), None);
        assert_eq!(parse_goto("hola", 100), None);
        assert_eq!(parse_goto("12:0", 100), None);
        assert_eq!(parse_goto("", 100), None);
    }

    use super::*;

    #[test]
    fn al_cerrar_la_activa_queda_la_vecina() {
        // Tres pestañas: cerrar la del medio deja la que ocupa su lugar.
        assert_eq!(after_close(3, 1, Some(1)), Some(1));
        // Cerrar la última deja la anterior.
        assert_eq!(after_close(3, 2, Some(2)), Some(1));
        // Cerrar la primera deja la que pasa a ser primera.
        assert_eq!(after_close(3, 0, Some(0)), Some(0));
        // La única: no queda ninguna.
        assert_eq!(after_close(1, 0, Some(0)), None);
    }

    #[test]
    fn al_cerrar_otra_la_activa_sigue_siendo_la_misma() {
        // La activa estaba después de la cerrada: su posición baja una.
        assert_eq!(after_close(3, 0, Some(2)), Some(1));
        // Antes de la cerrada: no cambia.
        assert_eq!(after_close(3, 2, Some(0)), Some(0));
        // Con el editor oculto sigue oculto.
        assert_eq!(after_close(3, 1, None), None);
    }

    #[test]
    fn recarga_si_esta_limpio_y_avisa_si_hay_cambios_propios() {
        assert_eq!(reload_decision(false, false), Reload::Replace);
        assert_eq!(reload_decision(true, false), Reload::Warn);
        // Un texto igual al del disco no cambia nada, ni avisa.
        assert_eq!(reload_decision(false, true), Reload::Nothing);
        assert_eq!(reload_decision(true, true), Reload::Nothing);
    }

    #[test]
    fn el_contexto_cuenta_las_lineas_desde_uno() {
        // Un cursor en la línea 12 (desde 0: 11..12).
        assert_eq!(one_based(11..12), (12, 12));
        // Seleccionar de la línea 12 a la 20.
        assert_eq!(one_based(11..20), (12, 20));
        // Un rango vacío no deja el fin antes del inicio.
        assert_eq!(one_based(4..4), (5, 5));
        assert_eq!(lines_label((12, 12)), "12");
        assert_eq!(lines_label((12, 20)), "12-20");
        assert_eq!(chip_label("mod.rs", (12, 20)), "mod.rs:12-20");
    }

    #[test]
    fn el_mensaje_lleva_el_contexto_salvo_que_ya_mencione_el_archivo() {
        assert_eq!(context_note("explícame esto", "src/a.rs", (12, 20)).as_deref(), Some("(Contexto: @src/a.rs, líneas 12-20)"));
        assert_eq!(context_note("qué hace esto", "src/a.rs", (7, 7)).as_deref(), Some("(Contexto: @src/a.rs, línea 7)"));
        assert_eq!(context_note("mira @src/a.rs", "src/a.rs", (1, 1)), None);
    }

    #[test]
    fn detecta_si_se_sangra_con_tabulaciones() {
        assert!(uses_tabs("func main() {\n\tfmt.Println()\n\tx := 1\n}\n"));
        assert!(!uses_tabs("fn main() {\n    let x = 1;\n    let y = 2;\n}\n"));
        assert!(!uses_tabs(""));
    }

    #[test]
    fn lee_texto_y_explica_lo_que_no_se_puede_editar() {
        let dir = std::env::temp_dir().join(format!("atic-editor-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let text = dir.join("a.txt");
        std::fs::write(&text, "hola\r\nmundo\r\n").unwrap();
        assert_eq!(read_text(&text).as_deref(), Ok("hola\r\nmundo\r\n"));
        let binary = dir.join("b.bin");
        std::fs::write(&binary, [1u8, 0, 2]).unwrap();
        assert_eq!(read_text(&binary), Err("Es un archivo binario.".into()));
        let latin = dir.join("c.txt");
        std::fs::write(&latin, [0x68u8, 0xe9]).unwrap();
        assert!(read_text(&latin).unwrap_err().contains("UTF-8"));
        assert_eq!(read_text(&dir.join("nada.txt")), Err("El archivo ya no existe.".into()));
        // Guardar escribe el texto tal cual (con sus saltos de línea).
        write_text(&text, "otro\n").unwrap();
        assert_eq!(read_text(&text).as_deref(), Ok("otro\n"));
        // Y no deja el temporal.
        assert!(!dir.join("a.txt.atic-tmp~").exists());
        std::fs::remove_dir_all(&dir).ok();
    }
}
