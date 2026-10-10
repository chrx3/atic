//! El editor de código del panel derecho: el `CodeEditor` de gpui-m3 con una pestaña por
//! archivo abierto (como `Tab` de `store.ts` en la referencia). Aquí vive el estado de las pestañas,
//! guardar, recargar si Claude edita el archivo y armar el contexto que el composer manda
//! con la selección. La vista (`view.rs`) y `mod.rs` solo lo enganchan.
//!
//! Los diffs del panel de Cambios siguen siendo del visor (`space::viewer::Doc`); su botón
//! «Abrir» pasa el archivo a una pestaña de aquí.

use std::ops::Range;
use std::path::{Path, PathBuf};

use gpui::{
    div, prelude::*, px, AnyElement, App, ClickEvent, Context, Entity, KeyBinding, SharedString, Window,
};
use gpui_m3::{Banner, Button, CodeEditor, CodeEditorEvent, Dialog, SyntaxLines, Tab, TabStrip};

use super::overlay;
use super::style::t;
use super::{same_path, CodeView};

/// Lo más grande que se abre para editar (el mismo tope que el visor).
const MAX_BYTES: u64 = 4 * 1024 * 1024;

/// Los atajos de Atic Code que dentro del editor no actúan (Ctrl+L, Ctrl+K, Ctrl+P…): se
/// anulan con `NoAction` en el contexto del `CodeEditor`, como hace la terminal. Ctrl+S, Esc
/// y las teclas de edición siguen siendo del editor.
const EDITOR_KEYS: [&str; 11] =
    ["ctrl-k", "ctrl-p", "ctrl-n", "ctrl-b", "ctrl-u", "ctrl-e", "ctrl-g", "ctrl-o", "ctrl-l", "ctrl-j", "ctrl-enter"];

/// El contexto de teclas que pone `CodeEditor` de gpui-m3.
const CONTEXT: &str = "M3CodeEditor";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys(EDITOR_KEYS.iter().map(|key| KeyBinding::new(key, gpui::NoAction, Some(CONTEXT))));
}

/// Lo que muestra una pestaña: el editor, o el motivo por el que no se puede editar.
pub(super) enum Body {
    Editor(Entity<CodeEditor>),
    Note(String),
}

/// Un archivo abierto.
pub(super) struct Open {
    pub path: PathBuf,
    pub body: Body,
    /// El archivo cambió en el disco y el editor tiene cambios propios: no se pisó.
    pub stale: bool,
}

/// Las pestañas de archivos abiertos y lo que depende de ellas.
#[derive(Default)]
pub(super) struct Tabs {
    pub files: Vec<Open>,
    /// La pestaña a la vista; `None` si se volvió a la lista de archivos.
    pub active: Option<usize>,
    /// El archivo con cambios sin guardar que se quiere cerrar (el diálogo).
    pub confirm: Option<PathBuf>,
    pub confirm_last: overlay::Last<PathBuf>,
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

    /// El editor se ve (hay una pestaña a la vista).
    pub fn showing(&self) -> bool {
        self.active.is_some_and(|i| i < self.files.len())
    }

    pub fn current(&self) -> Option<&Open> {
        self.active.and_then(|i| self.files.get(i))
    }

    /// Vuelve a la lista de archivos sin cerrar nada.
    pub fn hide(&mut self) {
        self.active = None;
    }

    fn select(&mut self, index: usize) {
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
            Body::Note(_) => None,
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

/// El texto del archivo, o por qué no se puede editar.
pub(super) fn read_text(path: &Path) -> Result<String, String> {
    match std::fs::metadata(path) {
        Err(_) => return Err("El archivo ya no existe.".into()),
        Ok(meta) if meta.len() > MAX_BYTES => return Err("El archivo es muy grande para editarlo.".into()),
        Ok(_) => {}
    }
    let bytes = std::fs::read(path).map_err(|error| format!("No se pudo leer: {error}"))?;
    if bytes.iter().take(8192).any(|b| *b == 0) {
        return Err("Es un archivo binario.".into());
    }
    // Un archivo que no es UTF-8 se vería bien pero al guardarlo se dañaría.
    String::from_utf8(bytes).map_err(|_| "El archivo no está en UTF-8: no se puede editar.".into())
}

/// `fs_write_text` de la referencia.
pub(super) fn write_text(path: &Path, text: &str) -> std::io::Result<()> {
    std::fs::write(path, text)
}

fn name_of(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string())
}

// --- Enganche con la vista ---------------------------------------------------------------

impl CodeView {
    /// Abre el archivo en una pestaña del editor (o pasa a la que ya lo tiene).
    pub(super) fn open_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.doc = None;
        self.active_file = Some(path.clone());
        let index = match self.tabs.index_of(&path) {
            Some(index) => index,
            None => {
                let body = self.load_body(&path, cx);
                self.tabs.files.push(Open { path, body, stale: false });
                self.tabs.files.len() - 1
            }
        };
        self.tabs.select(index);
        cx.notify();
    }

    /// Abre el archivo y deja el cursor en esa línea (desde 1): las rutas del chat `a.rs:42`.
    pub(super) fn open_file_at(&mut self, path: PathBuf, line: u32, cx: &mut Context<Self>) {
        self.open_file(path.clone(), cx);
        if let Some(Body::Editor(editor)) = self.tabs.index_of(&path).map(|i| &self.tabs.files[i].body) {
            editor.update(cx, |editor, cx| editor.go_to((line as usize).saturating_sub(1), 0, cx));
        }
    }

    fn load_body(&mut self, path: &Path, cx: &mut Context<Self>) -> Body {
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

    fn tab_dirty(&self, index: usize, cx: &App) -> bool {
        matches!(self.tabs.files.get(index).map(|f| &f.body), Some(Body::Editor(e)) if e.read(cx).is_dirty())
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
                self.show_toast(format!("No se pudo guardar {}: {error}", name_of(&path)), cx);
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
        let read = read_text(&path);
        let editor = match &self.tabs.files[index].body {
            Body::Editor(editor) => Some(editor.clone()),
            Body::Note(_) => None,
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
    /// manda con el mensaje. Nada si se ve un diff, si el editor está oculto o si se quitó.
    pub(super) fn editor_context(&self, cx: &App) -> Option<EditorContext> {
        if self.doc.is_some() {
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

    // --- Lo que se dibuja ---

    /// El cuerpo del panel con el editor: las pestañas, el aviso de «cambió en el disco» y
    /// el texto.
    pub(super) fn editor_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let t = t();
        let strip = self.tab_strip(cx);
        let Some(index) = self.tabs.active.filter(|i| *i < self.tabs.files.len()) else {
            return div().into_any_element();
        };
        let open = &self.tabs.files[index];
        let content = match &open.body {
            Body::Editor(editor) => div()
                .flex_1()
                .min_h(px(0.))
                .relative()
                // Un contenedor de tamaño fijo: el editor llena a su padre.
                .child(div().absolute().top_0().left_0().size_full().child(editor.clone()))
                .into_any_element(),
            Body::Note(note) => div().flex_1().p(px(16.)).text_color(t.muted).child(note.clone()).into_any_element(),
        };
        let stale = open.stale.then(|| {
            Banner::new(SharedString::from(format!("stale-{index}")))
                .warning()
                .icon("info")
                .text("El archivo cambió en el disco")
                .action(Button::new("stale-reload", "Recargar").text().on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    view.reload_discarding(index, cx)
                })))
                .action(Button::new("stale-keep", "Mantener lo mío").text().on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    if let Some(open) = view.tabs.files.get_mut(index) {
                        open.stale = false;
                    }
                    cx.notify();
                })))
        });
        div()
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .bg(super::view::code_bg())
            .border_t_1()
            .border_color(t.border)
            .child(strip)
            .when_some(stale, |el, banner| el.child(div().px(px(8.)).pb(px(6.)).child(banner)))
            .child(content)
            .into_any_element()
    }

    fn tab_strip(&self, cx: &mut Context<Self>) -> AnyElement {
        let tabs: Vec<Tab> = self
            .tabs
            .files
            .iter()
            .enumerate()
            .map(|(index, open)| {
                Tab::new(open.path.to_string_lossy().to_string(), name_of(&open.path))
                    .icon("file")
                    .tooltip(open.path.display().to_string())
                    .dirty(self.tab_dirty(index, cx))
            })
            .collect();
        let on_tab = |cx: &mut Context<Self>, f: fn(&mut CodeView, usize, &mut Context<CodeView>)| {
            let this = cx.weak_entity();
            move |key: SharedString, _: &mut Window, cx: &mut App| {
                let _ = this.update(cx, |view, cx| {
                    if let Some(index) = view.tabs.index_of(Path::new(key.as_ref())) {
                        f(view, index, cx);
                    }
                });
            }
        };
        let mut strip = TabStrip::new("file-tabs")
            .tabs(tabs)
            .on_select(on_tab(cx, |view, index, cx| {
                view.tabs.select(index);
                view.active_file = Some(view.tabs.files[index].path.clone());
                cx.notify();
            }))
            .on_close(on_tab(cx, |view, index, cx| view.request_close(index, cx)));
        if let Some(open) = self.tabs.current() {
            strip = strip.selected(open.path.to_string_lossy().to_string());
        }
        div().flex_none().h(px(40.)).px(px(8.)).flex().items_center().child(strip).into_any_element()
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
                .title("Cambios sin guardar")
                .width(px(420.))
                .on_dismiss(cx.listener(|view, _: &ClickEvent, _, cx| view.answer_close(None, cx)))
                .child(div().text_color(t.muted).child(format!("{name} tiene cambios sin guardar. ¿Qué hacemos con ellos?")))
                .action(Button::new("file-close-cancel", "Cancelar").text().on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.answer_close(None, cx))))
                .action(
                    Button::new("file-close-discard", "Descartar")
                        .text()
                        .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.answer_close(Some(false), cx))),
                )
                .action(
                    Button::new("file-close-save", "Guardar")
                        .filled()
                        .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.answer_close(Some(true), cx))),
                )
                .into_any_element(),
        )
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
        std::fs::remove_dir_all(&dir).ok();
    }
}
