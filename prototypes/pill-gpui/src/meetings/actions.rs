//! Lo que se hace con una reunión desde su cabecera y su resumen: transcribir,
//! resumir (con el texto dibujándose mientras llega), editar el resumen,
//! copiarlo, mandarlo por correo, exportar, renombrar y eliminar.
//!
//! El trabajo pesado está en `pipeline` y `export`, siempre en hilos; aquí
//! solo el estado de la ventana y los elementos. Cada trabajo se guarda por
//! id de reunión: si se cambia de reunión a medio camino, el resultado igual
//! cae en la suya.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use atic_core::{Recording, Summary, Transcript};
use atic_mailer::OutgoingMail;
use atic_summarize::SummaryTemplate;
use futures::channel::mpsc::UnboundedReceiver;
use futures::StreamExt;
use gpui::{
    actions, div, prelude::*, pulsating_between, px, svg, Animation, AnimationExt, AnyElement, App,
    ClickEvent, ClipboardItem, Context, Entity, Focusable, FontWeight, KeyBinding, SharedString,
    Window,
};

use super::data::Paths;
use super::export::{self, Format};
use super::pipeline::{self, Failure, Job, MailOutcome, Stage, Update};
use super::summary::{self, Kind, Section};
use super::{
    hsla, section_card, Detail, MeetingsView, FAINT, INK, ITEM, MUTED, R_CARD, RED,
    SURFACE_ON, TEXT,
};
use crate::hover::{self, HoverExt};
use crate::text_area::TextArea;
use crate::text_input::TextInput;

actions!(meetings_ops, [FieldConfirm, FieldCancel, Swallow, EditorSave, EditorCancel, DeleteSelected, UndoDelete]);
// Los atajos de la reunión elegida y el recorrido de los menús con teclado.
actions!(meetings_keys, [
    RenameMeeting, OpenExport, CopySummary, OpenMore, ToggleSettings, GenerateSummary,
    MenuPrev, MenuNext, MenuFirst, MenuLast, MenuPick, MenuClose,
]);

/// Un campo de una línea (renombrar, destinatarios): Enter confirma, Esc
/// cancela, y ↑/↓/Tab no cambian de reunión ni de pestaña mientras se escribe.
const FIELD_CONTEXT: &str = "MeetingsField";
/// El editor del resumen: Ctrl+Enter guarda, Esc cancela.
const EDITOR_CONTEXT: &str = "MeetingsEditor";
/// Un menú abierto: tiene el foco mientras está, así ↑/↓ lo recorren en
/// vez de cambiar de reunión.
const MENU_CONTEXT: &str = "MeetingsMenu";

pub fn bind_keys(cx: &mut App) {
    let field = Some(FIELD_CONTEXT);
    let editor = Some(EDITOR_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("enter", FieldConfirm, field),
        KeyBinding::new("escape", FieldCancel, field),
        KeyBinding::new("up", Swallow, field),
        KeyBinding::new("down", Swallow, field),
        KeyBinding::new("tab", Swallow, field),
        KeyBinding::new("ctrl-enter", EditorSave, editor),
        KeyBinding::new("ctrl-s", EditorSave, editor),
        KeyBinding::new("escape", EditorCancel, editor),
        KeyBinding::new("tab", Swallow, editor),
        // Espacio es reproducir en Meetings: en un campo tiene que escribir.
        KeyBinding::new("space", gpui::NoAction, field),
        KeyBinding::new("space", gpui::NoAction, editor),
        // Ctrl+Z dentro de un campo no resucita una reunión borrada.
        KeyBinding::new("ctrl-z", Swallow, field),
        KeyBinding::new("ctrl-z", Swallow, editor),
        // Supr en un campo borra texto (lo toma `TextInput`, más adentro).
        KeyBinding::new("delete", DeleteSelected, Some(super::KEY_CONTEXT)),
        KeyBinding::new("ctrl-z", UndoDelete, Some(super::KEY_CONTEXT)),
    ]);
    // Fuera de los campos de texto (ver `keys_free`): ahí F2 o Ctrl+E no
    // deben sacar a nadie de lo que escribe.
    let meetings = Some(super::KEY_CONTEXT);
    let menu = Some(MENU_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("f2", RenameMeeting, meetings),
        KeyBinding::new("ctrl-e", OpenExport, meetings),
        KeyBinding::new("ctrl-shift-c", CopySummary, meetings),
        KeyBinding::new("ctrl-.", OpenMore, meetings),
        KeyBinding::new("shift-f10", OpenMore, meetings),
        KeyBinding::new("ctrl-,", ToggleSettings, meetings),
        KeyBinding::new("ctrl-enter", GenerateSummary, meetings),
        KeyBinding::new("up", MenuPrev, menu),
        KeyBinding::new("down", MenuNext, menu),
        KeyBinding::new("shift-tab", MenuPrev, menu),
        KeyBinding::new("tab", MenuNext, menu),
        KeyBinding::new("home", MenuFirst, menu),
        KeyBinding::new("end", MenuLast, menu),
        KeyBinding::new("enter", MenuPick, menu),
        KeyBinding::new("space", MenuPick, menu),
        KeyBinding::new("escape", MenuClose, menu),
        // Un Enter de más en un campo no genera un resumen.
        KeyBinding::new("ctrl-enter", Swallow, Some(FIELD_CONTEXT)),
    ]);
}

/// Lo que se ve de los atajos, en menús y globitos.
const KEY_RENAME: &str = "F2";
const KEY_COPY: &str = "Ctrl+Mayús+C";
const KEY_DELETE: &str = "Supr";

const PRIMARY: u32 = 0xe9e9e2;
const PRIMARY_HOVER: u32 = 0xffffff;
const MENU: u32 = 0x2d2d2a;
const MENU_HOVER: u32 = 0x3a3a37;
const FIELD: u32 = 0x31312e;
/// El aviso de deshacer: la capa más clara, encima de la lista.
const TOAST: u32 = 0x363633;
/// Los avisos ok se van solos; los errores se quedan hasta que se actúa.
const NOTICE_TTL: Duration = Duration::from_secs(7);
/// Lo que dura «Deshacer» tras eliminar; recién después se borra de verdad.
const UNDO_TTL: Duration = Duration::from_secs(6);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Menu {
    Export,
    More,
    Template,
    Regenerate,
}

/// Lo que hace un ítem de menú: el mismo camino para el clic y el Enter.
#[derive(Clone)]
enum Cmd {
    Export(Format),
    Rename,
    Retranscribe(String),
    Copy,
    Folder,
    Delete(String),
    Template(SummaryTemplate),
}

struct Entry {
    id: &'static str,
    icon: Option<&'static str>,
    label: &'static str,
    /// A la derecha, tenue: la extensión o el atajo.
    detail: Option<&'static str>,
    cmd: Cmd,
}

struct Running {
    job: Job,
    progress: f32,
    stage: Option<Stage>,
    draft: String,
    sections: Vec<Section>,
}

struct Failed {
    job: Job,
    failure: Failure,
}

struct Notice {
    id: String,
    text: String,
    /// «Mostrar»: el archivo exportado en el Explorador.
    reveal: Option<PathBuf>,
    seq: u64,
}

struct Edit {
    id: String,
    area: Entity<TextArea>,
    previous: Option<Summary>,
}

struct Mail {
    id: String,
    input: Entity<TextInput>,
    error: Option<String>,
    sending: bool,
}

/// Una reunión eliminada que todavía se puede deshacer: solo se esconde de
/// la lista. El borrado real (fila y carpeta) llega al vencer el plazo, al
/// eliminar otra o al soltarse esto (ventana o app cerrada): nunca queda
/// escondida sin borrar ni se pierde el borrado.
struct PendingDelete {
    id: String,
    seq: u64,
    /// `None` una vez deshecho o entregado al hilo que borra.
    paths: Option<Paths>,
}

impl PendingDelete {
    /// El borrado sigue en un hilo; esto ya no borra al soltarse.
    fn hand_over(mut self) -> Option<(Paths, String)> {
        let paths = self.paths.take()?;
        Some((paths, std::mem::take(&mut self.id)))
    }

    fn cancel(mut self) -> String {
        self.paths = None;
        std::mem::take(&mut self.id)
    }
}

impl Drop for PendingDelete {
    fn drop(&mut self) {
        if let Some(paths) = self.paths.take() {
            if let Err(error) = pipeline::delete(&paths, &self.id) {
                eprintln!("reuniones: no se pudo eliminar {} al cerrar: {error}", self.id);
            }
        }
    }
}

#[derive(Default)]
pub(super) struct Ops {
    jobs: HashMap<String, Running>,
    failures: HashMap<String, Failed>,
    template: Option<SummaryTemplate>,
    menu: Option<Menu>,
    /// El ítem elegido con el teclado (o el último bajo el cursor).
    menu_cursor: Option<usize>,
    /// El foco de los menús abiertos (se crea al primer dibujo).
    menu_focus: Option<gpui::FocusHandle>,
    /// La píldora de las pestañas (ver `motion`).
    pub(super) slide: super::motion::TabSlide,
    /// El menú que se cerró al apretar fuera: si ese apretón fue sobre su
    /// propio botón, el clic no lo vuelve a abrir.
    menu_closed: Option<(Menu, Instant)>,
    rename: Option<(String, Entity<TextInput>)>,
    edit: Option<Edit>,
    mail: Option<Mail>,
    pending_delete: Option<PendingDelete>,
    delete_seq: u64,
    /// Si la app se cierra con un borrado pendiente, se completa antes.
    _quit: Option<gpui::Subscription>,
    /// Las reuniones transcritas sin nadie hablando («Sin voz»). Se calcula
    /// al releer la lista, no al dibujar.
    silent: std::collections::HashSet<String>,
    notice: Option<Notice>,
    notice_seq: u64,
}

impl Ops {
    fn template(&self) -> SummaryTemplate {
        self.template.unwrap_or(SummaryTemplate::SummaryKeyPoints)
    }

    fn running(&self, id: &str, job: Job) -> Option<&Running> {
        self.jobs.get(id).filter(|r| r.job == job)
    }

    /// ¿Se esconde de la lista? (eliminada, a la espera de «Deshacer»).
    pub(super) fn hides(&self, id: &str) -> bool {
        self.pending_delete.as_ref().is_some_and(|p| p.id == id)
    }

    pub(super) fn is_silent(&self, id: &str) -> bool {
        self.silent.contains(id)
    }
}

/// Lo que hace falta para exportar: las rutas (se lee en el hilo) o, con los
/// datos de prueba, lo ya leído en memoria.
enum ExportInput {
    Disk(Paths),
    Loaded(Option<Transcript>, Option<Summary>),
}

fn downloads_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(|home| PathBuf::from(home).join("Downloads"))
        .filter(|dir| dir.is_dir())
        .unwrap_or_else(std::env::temp_dir)
}

/// `acta.pdf` → `acta (2).pdf` si ya existe: el respaldo en Descargas no
/// pisa nada.
fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Reunión");
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    let parent = path.parent().unwrap_or(Path::new("."));
    (2..1000)
        .map(|n| parent.join(format!("{stem} ({n}).{ext}")))
        .find(|p| !p.exists())
        .unwrap_or_else(|| path.to_path_buf())
}

// --- Estado y trabajos ------------------------------------------------------------

impl MeetingsView {
    fn paths(&self) -> Option<Paths> {
        self.source.as_ref().and_then(|s| s.paths()).cloned()
    }

    fn selected_rec(&self) -> Option<&Recording> {
        self.selected.and_then(|ix| self.items.get(ix))
    }

    /// Transcribe una reunión (con Groq). Lo usa también quien acaba de
    /// grabar, para seguir de corrido.
    pub fn start_transcribe(&mut self, id: String, cx: &mut Context<Self>) {
        self.run_transcribe(id, false, cx);
    }

    fn run_transcribe(&mut self, id: String, force_groq: bool, cx: &mut Context<Self>) {
        let Some(paths) = self.paths() else {
            return;
        };
        if self.ops.jobs.contains_key(&id) {
            return;
        }
        self.ops.failures.remove(&id);
        self.ops.jobs.insert(id.clone(), Running::new(Job::Transcribe));
        let rx = pipeline::transcribe(paths, id.clone(), force_groq);
        self.drive(id, rx, cx);
        cx.notify();
    }

    fn start_summarize(&mut self, id: String, cx: &mut Context<Self>) {
        let Some(paths) = self.paths() else {
            return;
        };
        if self.ops.jobs.contains_key(&id) {
            return;
        }
        self.ops.failures.remove(&id);
        self.ops.menu = None;
        if self.ops.edit.as_ref().is_some_and(|e| e.id == id) {
            self.ops.edit = None;
        }
        self.ops.jobs.insert(id.clone(), Running::new(Job::Summarize));
        self.tab = super::Tab::Summary;
        let rx = pipeline::summarize(paths, id.clone(), self.ops.template());
        self.drive(id, rx, cx);
        cx.notify();
    }

    fn drive(&mut self, id: String, mut rx: UnboundedReceiver<Update>, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            while let Some(update) = rx.next().await {
                let done = matches!(update, Update::Done(_));
                if this.update(cx, |view, cx| view.apply(&id, update, cx)).is_err() || done {
                    break;
                }
            }
        })
        .detach();
    }

    fn apply(&mut self, id: &str, update: Update, cx: &mut Context<Self>) {
        match update {
            Update::Started => self.reload(cx),
            Update::Progress(p) => {
                if let Some(run) = self.ops.jobs.get_mut(id) {
                    run.progress = p;
                }
            }
            Update::Stage(stage) => {
                if let Some(run) = self.ops.jobs.get_mut(id) {
                    run.stage = Some(stage);
                }
            }
            Update::Delta(delta) => {
                if let Some(run) = self.ops.jobs.get_mut(id) {
                    run.draft.push_str(&delta);
                    run.sections = summary::parse(&run.draft, "Resumen");
                }
            }
            Update::Done(result) => {
                let job = self.ops.jobs.remove(id).map(|r| r.job);
                if let (Err(failure), Some(job)) = (result, job) {
                    self.ops.failures.insert(id.to_string(), Failed { job, failure });
                }
                self.reload(cx);
            }
        }
        cx.notify();
    }

    fn notice(&mut self, id: &str, text: impl Into<String>, reveal: Option<PathBuf>, cx: &mut Context<Self>) {
        self.ops.notice_seq += 1;
        let seq = self.ops.notice_seq;
        self.ops.notice = Some(Notice { id: id.to_string(), text: text.into(), reveal, seq });
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(NOTICE_TTL).await;
            let _ = this.update(cx, |view, cx| {
                if view.ops.notice.as_ref().is_some_and(|n| n.seq == seq) {
                    view.ops.notice = None;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn fail_notice(&mut self, id: &str, text: String, cx: &mut Context<Self>) {
        self.ops.failures.insert(id.to_string(), Failed { job: Job::Summarize, failure: Failure::Message(text) });
        cx.notify();
    }

    // --- Menús ---

    fn toggle_menu(&mut self, menu: Menu, cx: &mut Context<Self>) {
        let just_closed = self
            .ops
            .menu_closed
            .is_some_and(|(m, at)| m == menu && at.elapsed() < Duration::from_millis(250));
        self.ops.menu_closed = None;
        self.ops.menu = if self.ops.menu == Some(menu) || just_closed { None } else { Some(menu) };
        self.ops.menu_cursor = None;
        cx.notify();
    }

    fn close_menu(&mut self, cx: &mut Context<Self>) {
        if let Some(menu) = self.ops.menu.take() {
            self.ops.menu_closed = Some((menu, Instant::now()));
            cx.notify();
        }
    }

    /// Abre (o cierra, si ya está) un menú desde el teclado: con el primer
    /// ítem marcado, para seguir con ↑/↓ y Enter.
    fn toggle_menu_keys(&mut self, menu: Menu, cx: &mut Context<Self>) {
        if self.ops.menu == Some(menu) {
            self.ops.menu = None;
        } else if !self.entries(menu).is_empty() {
            self.ops.menu = Some(menu);
            self.ops.menu_cursor = Some(0);
            self.ops.menu_closed = None;
        }
        cx.notify();
    }

    /// «Más» de una reunión desde otro lado (la fila de la lista): la elige
    /// y abre el menú ⋯ de su cabecera. `keyboard`: con el primer ítem
    /// marcado.
    pub(super) fn open_more_menu(&mut self, index: Option<usize>, keyboard: bool, cx: &mut Context<Self>) {
        if self.settings.is_some() {
            return;
        }
        if let Some(ix) = index {
            self.select(ix, cx);
        }
        if self.entries(Menu::More).is_empty() {
            return;
        }
        self.ops.menu = Some(Menu::More);
        self.ops.menu_cursor = keyboard.then_some(0);
        self.ops.menu_closed = None;
        cx.notify();
    }

    /// El foco sigue al menú: lo toma al abrirse (así ↑/↓ y Esc son suyos)
    /// y lo devuelve a la ventana al cerrarse, también cuando se cerró por
    /// un clic o porque lo que mostraba ya no está. Se llama al dibujar.
    pub(super) fn sync_menu_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let handle = self.ops.menu_focus.get_or_insert_with(|| cx.focus_handle()).clone();
        if let Some(menu) = self.ops.menu {
            if self.settings.is_some() || self.entries(menu).is_empty() {
                self.ops.menu = None;
            }
        }
        if self.ops.menu.is_some() {
            if !handle.is_focused(window) {
                window.focus(&handle);
            }
        } else if handle.is_focused(window) {
            window.focus(&self.focus);
        }
    }

    /// Los ítems de cada menú según la reunión elegida: lo que se dibuja y
    /// lo que recorre el teclado salen de aquí.
    fn entries(&self, menu: Menu) -> Vec<Entry> {
        let Some(rec) = self.selected_rec() else {
            return Vec::new();
        };
        let detail = self.detail.as_ref().filter(|d| d.id == rec.id);
        let has_transcript = detail.is_some_and(|d| d.has_transcript);
        let has_summary = detail.is_some_and(|d| !d.sections.is_empty());
        let has_audio = rec.mic_path.is_some() || rec.system_path.is_some();
        let writable = self.paths().is_some();
        let running = self.ops.jobs.contains_key(&rec.id);
        let outside_busy = !running && pipeline::busy(rec.status);
        let entry = |id, icon, label, detail, cmd| Entry { id, icon, label, detail, cmd };
        match menu {
            Menu::Export => {
                if !has_transcript && !has_summary {
                    return Vec::new();
                }
                Format::ALL
                    .into_iter()
                    .map(|format| {
                        let (id, ext) = match format {
                            Format::Markdown => ("export-md", ".md"),
                            Format::Word => ("export-docx", ".docx"),
                            Format::Pdf => ("export-pdf", ".pdf"),
                        };
                        entry(id, None, format.label(), Some(ext), Cmd::Export(format))
                    })
                    .collect()
            }
            Menu::More => {
                let mut items = Vec::new();
                if writable {
                    items.push(entry("more-rename", Some("icons/pencil.svg"), "Renombrar", Some(KEY_RENAME), Cmd::Rename));
                    if has_audio && !running && (has_transcript || outside_busy) {
                        items.push(entry(
                            "more-retranscribe",
                            Some("icons/rotate-ccw.svg"),
                            "Transcribir de nuevo",
                            None,
                            Cmd::Retranscribe(rec.id.clone()),
                        ));
                    }
                }
                if has_summary {
                    items.push(entry("more-copy", Some("icons/copy.svg"), "Copiar resumen", Some(KEY_COPY), Cmd::Copy));
                }
                if writable {
                    items.push(entry("more-folder", Some("icons/folder.svg"), "Abrir carpeta", None, Cmd::Folder));
                    if !running && !outside_busy {
                        items.push(entry("more-delete", Some("icons/trash.svg"), "Eliminar", Some(KEY_DELETE), Cmd::Delete(rec.id.clone())));
                    }
                }
                items
            }
            Menu::Template | Menu::Regenerate => {
                let current = self.ops.template();
                let run = menu == Menu::Regenerate;
                SummaryTemplate::all()
                    .iter()
                    .map(|&template| {
                        let icon = if template == current { "icons/check.svg" } else { "" };
                        entry(template_item_id(template, run), Some(icon), pipeline::template_label(template), None, Cmd::Template(template))
                    })
                    .collect()
            }
        }
    }

    fn run_cmd(&mut self, menu: Menu, cmd: Cmd, window: &mut Window, cx: &mut Context<Self>) {
        match cmd {
            Cmd::Export(format) => self.export(format, cx),
            Cmd::Rename => self.begin_rename(window, cx),
            Cmd::Retranscribe(id) => {
                self.ops.menu = None;
                self.start_transcribe(id, cx);
            }
            Cmd::Copy => self.copy_summary(window, cx),
            Cmd::Folder => {
                self.ops.menu = None;
                self.open_folder();
            }
            Cmd::Delete(id) => self.delete(id, cx),
            Cmd::Template(template) => {
                self.ops.template = Some(template);
                self.ops.menu = None;
                if menu == Menu::Regenerate {
                    if let Some(id) = self.selected_rec().map(|r| r.id.clone()) {
                        self.start_summarize(id, cx);
                    }
                }
            }
        }
        cx.notify();
    }

    fn menu_step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(menu) = self.ops.menu else {
            return;
        };
        let len = self.entries(menu).len();
        self.ops.menu_cursor = super::motion::step_cursor(self.ops.menu_cursor, len, delta);
        cx.notify();
    }

    /// Inicio/Fin: el primero o el último.
    fn menu_edge(&mut self, last: bool, cx: &mut Context<Self>) {
        let Some(menu) = self.ops.menu else {
            return;
        };
        let len = self.entries(menu).len();
        self.ops.menu_cursor = (len > 0).then(|| if last { len - 1 } else { 0 });
        cx.notify();
    }

    fn menu_pick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.ops.menu else {
            return;
        };
        let entries = self.entries(menu);
        if let Some(entry) = self.ops.menu_cursor.and_then(|ix| entries.into_iter().nth(ix)) {
            self.run_cmd(menu, entry.cmd, window, cx);
        }
    }

    // --- Atajos ---

    /// Los atajos de la reunión solo valen con el foco en la ventana (o en
    /// un menú abierto): en un campo de texto, F2 o Ctrl+Enter son de él.
    fn keys_free(&self, window: &Window) -> bool {
        self.settings.is_none()
            && (self.focus.is_focused(window)
                || self.ops.menu_focus.as_ref().is_some_and(|f| f.is_focused(window)))
    }

    /// Se puede generar el resumen de la elegida (lo mismo que muestra el
    /// botón «Generar resumen»).
    fn can_generate(&self) -> bool {
        let Some(rec) = self.selected_rec() else {
            return false;
        };
        let Some(detail) = self.detail.as_ref().filter(|d| d.id == rec.id) else {
            return false;
        };
        let busy = self.ops.jobs.contains_key(&rec.id) || pipeline::busy(rec.status);
        let silent = detail.has_transcript && detail.blocks.is_empty();
        let editing = self.ops.edit.as_ref().is_some_and(|e| e.id == rec.id);
        self.paths().is_some() && detail.has_transcript && detail.sections.is_empty() && !busy && !silent && !editing
    }

    /// Los atajos y el teclado de los menús, en la raíz de la ventana.
    pub(super) fn shortcut_actions(el: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        el.on_action(cx.listener(|v, _: &RenameMeeting, window, cx| {
            if v.keys_free(window) && v.paths().is_some() {
                v.ops.menu = None;
                v.begin_rename(window, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &OpenExport, window, cx| {
            if v.keys_free(window) {
                v.toggle_menu_keys(Menu::Export, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &CopySummary, window, cx| {
            if v.keys_free(window) && v.selected_rec().is_some() {
                v.copy_summary(window, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &OpenMore, window, cx| {
            if v.keys_free(window) {
                v.toggle_menu_keys(Menu::More, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &ToggleSettings, window, cx| {
            // Con los ajustes abiertos el foco está en ellos: se cierran igual.
            if v.settings.is_some() || v.keys_free(window) {
                v.ops.menu = None;
                v.toggle_settings(window, cx);
            }
        }))
        .on_action(cx.listener(|v, _: &GenerateSummary, window, cx| {
            if v.keys_free(window) && v.can_generate() {
                if let Some(id) = v.selected_rec().map(|r| r.id.clone()) {
                    v.start_summarize(id, cx);
                }
            }
        }))
    }

    // --- Renombrar ---

    fn begin_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(rec) = self.selected_rec() else {
            return;
        };
        let (id, title) = (rec.id.clone(), rec.title.clone());
        let input = cx.new(|cx| {
            let mut input = TextInput::new("Nombre de la reunión", hsla(TEXT), hsla(FAINT), hsla(TEXT), cx);
            input.set_text(title, cx);
            input
        });
        window.focus(&input.focus_handle(cx));
        self.ops.rename = Some((id, input));
        self.ops.menu = None;
        cx.notify();
    }

    fn commit_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, input)) = self.ops.rename.take() else {
            return;
        };
        window.focus(&self.focus);
        cx.notify();
        let Some(title) = pipeline::clean_title(input.read(cx).text()) else {
            return;
        };
        let Some(paths) = self.paths() else {
            return;
        };
        let Some(rec) = self.items.iter_mut().find(|r| r.id == id) else {
            return;
        };
        if rec.title == title {
            return;
        }
        // Se ve al tiro; la base se escribe en el hilo.
        rec.title = title.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn({
                    let id = id.clone();
                    async move { pipeline::rename(&paths, &id, &title) }
                })
                .await;
            let _ = this.update(cx, |view, cx| match result {
                Ok(()) => view.reload(cx),
                Err(error) => {
                    view.fail_notice(&id, format!("No se pudo renombrar: {error}"), cx);
                    view.reload(cx);
                }
            });
        })
        .detach();
    }

    fn cancel_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ops.rename.take().is_some() {
            window.focus(&self.focus);
            cx.notify();
        }
    }

    // --- Eliminar ---

    /// Elimina al tiro de la lista, sin preguntar: «Deshacer» la devuelve
    /// durante `UNDO_TTL`. Si había otra pendiente, esa se borra ya.
    fn delete(&mut self, id: String, cx: &mut Context<Self>) {
        let Some(paths) = self.paths() else {
            return;
        };
        self.ops.menu = None;
        let Some(ix) = self.items.iter().position(|r| r.id == id) else {
            return;
        };
        if let Some(previous) = self.ops.pending_delete.take() {
            self.commit_delete(previous, cx);
        }
        if self.ops._quit.is_none() {
            self.ops._quit = Some(cx.on_app_quit(|view, _| {
                // Soltarla la borra (ver `Drop`), en este mismo hilo.
                view.ops.pending_delete = None;
                async {}
            }));
        }
        self.ops.delete_seq += 1;
        let seq = self.ops.delete_seq;
        self.ops.pending_delete = Some(PendingDelete { id: id.clone(), seq, paths: Some(paths) });
        if self.ops.rename.as_ref().is_some_and(|(r, _)| *r == id) {
            self.ops.rename = None;
        }

        // La de abajo toma su lugar (o la de arriba, si era la última). Al
        // cambiar la elegida, el reproductor se detiene (`sync_player`).
        let was_selected = self.selected == Some(ix);
        let selected_id = self.selected_rec().map(|r| r.id.clone());
        self.items.remove(ix);
        let index = if was_selected {
            (!self.items.is_empty()).then(|| ix.min(self.items.len() - 1))
        } else {
            selected_id.and_then(|sid| self.items.iter().position(|r| r.id == sid))
        };
        self.load_detail(index, was_selected);

        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(UNDO_TTL).await;
            let _ = this.update(cx, |view, cx| {
                if view.ops.pending_delete.as_ref().is_some_and(|p| p.seq == seq) {
                    if let Some(pending) = view.ops.pending_delete.take() {
                        view.commit_delete(pending, cx);
                    }
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn commit_delete(&mut self, pending: PendingDelete, cx: &mut Context<Self>) {
        let Some((paths, id)) = pending.hand_over() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn({
                    let id = id.clone();
                    async move { pipeline::delete(&paths, &id) }
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(()) => {
                        view.ops.failures.remove(&id);
                        view.ops.silent.remove(&id);
                        if view.ops.edit.as_ref().is_some_and(|e| e.id == id) {
                            view.ops.edit = None;
                        }
                        if view.ops.mail.as_ref().is_some_and(|m| m.id == id) {
                            view.ops.mail = None;
                        }
                    }
                    // Vuelve a la lista, con el porqué.
                    Err(error) => view.fail_notice(&id, format!("No se pudo eliminar: {error}"), cx),
                }
                view.reload(cx);
            });
        })
        .detach();
    }

    /// «Deshacer»: la reunión vuelve tal cual estaba, elegida.
    fn undo_delete(&mut self, cx: &mut Context<Self>) {
        if self.settings.is_some() {
            return;
        }
        let Some(pending) = self.ops.pending_delete.take() else {
            return;
        };
        let id = pending.cancel();
        self.reload(cx);
        if let Some(ix) = self.items.iter().position(|r| r.id == id) {
            if self.selected != Some(ix) {
                self.load_detail(Some(ix), true);
            }
        }
        cx.notify();
    }

    /// Supr: elimina la elegida (los campos de texto se quedan con su Supr).
    fn delete_selected(&mut self, cx: &mut Context<Self>) {
        if self.settings.is_some() || self.recorder_busy_with_selected() {
            return;
        }
        if let Some(id) = self.selected_rec().map(|r| r.id.clone()) {
            self.delete(id, cx);
        }
    }

    fn recorder_busy_with_selected(&self) -> bool {
        // Un trabajo en curso escribiría en una carpeta que se va a borrar.
        self.selected_rec().is_some_and(|r| self.ops.jobs.contains_key(&r.id) || pipeline::busy(r.status))
    }

    /// Las acciones de teclado de eliminar y deshacer, en la raíz de la ventana.
    pub(super) fn delete_actions(el: gpui::Div, cx: &mut Context<Self>) -> gpui::Div {
        el.on_action(cx.listener(|v, _: &DeleteSelected, _, cx| v.delete_selected(cx)))
            .on_action(cx.listener(|v, _: &UndoDelete, _, cx| v.undo_delete(cx)))
    }

    /// Recalcula qué reuniones transcritas no tienen voz. Lee solo las que
    /// están «Transcrita» (las otras o no tienen texto o ya tienen resumen);
    /// los transcript.json son chicos.
    pub(super) fn refresh_silent(&mut self) {
        let Some(source) = &self.source else {
            return;
        };
        self.ops.silent = self
            .items
            .iter()
            .filter(|r| r.status == atic_core::RecordingStatus::Transcribed)
            .filter(|r| source.transcript(&r.id).ok().flatten().is_some_and(|t| pipeline::is_silent(&t)))
            .map(|r| r.id.clone())
            .collect();
    }

    // --- Leer el resumen guardado (copiar, editar, correo) ---

    /// Lee el resumen en un hilo y sigue con `then` en la ventana.
    fn with_summary(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, Option<Summary>, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let input = self.export_input();
        cx.spawn_in(window, async move |this, cx| {
            let summary = match input {
                ExportInput::Loaded(_, summary) => summary,
                ExportInput::Disk(paths) => {
                    cx.background_spawn(async move { Summary::load(&paths.summary_path(&id)).ok().flatten() })
                        .await
                }
            };
            let _ = this.update_in(cx, |view, window, cx| then(view, summary, window, cx));
        })
        .detach();
    }

    fn export_input(&self) -> ExportInput {
        match (self.paths(), self.source.as_ref(), self.selected_rec()) {
            (Some(paths), _, _) => ExportInput::Disk(paths),
            (None, Some(source), Some(rec)) => ExportInput::Loaded(
                source.transcript(&rec.id).ok().flatten(),
                source.summary(&rec.id).ok().flatten(),
            ),
            _ => ExportInput::Loaded(None, None),
        }
    }

    fn copy_summary(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected_rec().map(|r| r.id.clone()) else {
            return;
        };
        self.ops.menu = None;
        self.with_summary(id.clone(), window, cx, move |view, summary, _, cx| match summary {
            Some(summary) => {
                cx.write_to_clipboard(ClipboardItem::new_string(summary.body));
                view.notice(&id, "Resumen copiado.", None, cx);
            }
            None => view.notice(&id, "No hay resumen guardado que copiar.", None, cx),
        });
    }

    fn begin_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected_rec().map(|r| r.id.clone()) else {
            return;
        };
        self.ops.mail = None;
        self.with_summary(id.clone(), window, cx, move |view, summary, window, cx| {
            let body = summary.as_ref().map(|s| s.body.clone()).unwrap_or_default();
            let area = cx.new(|cx| {
                let mut area = TextArea::new("Escribe el resumen…", hsla(super::BODY), hsla(FAINT), hsla(TEXT), cx);
                area.set_text(&body, cx);
                area
            });
            window.focus(&area.focus_handle(cx));
            view.ops.edit = Some(Edit { id, area, previous: summary });
            cx.notify();
        });
    }

    fn save_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = self.ops.edit.take() else {
            return;
        };
        window.focus(&self.focus);
        let body = edit.area.read(cx).text().to_string();
        let unchanged = edit.previous.as_ref().is_some_and(|s| s.body.trim_end() == body.trim_end());
        let (Some(paths), Some(title)) = (
            self.paths(),
            self.items.iter().find(|r| r.id == edit.id).map(|r| r.title.clone()),
        ) else {
            return;
        };
        if unchanged || body.trim().is_empty() {
            cx.notify();
            return;
        }
        let summary = pipeline::edited_summary(edit.previous.as_ref(), &body, self.ops.template(), &title);
        let id = edit.id;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn({
                    let id = id.clone();
                    async move { pipeline::save_summary(&paths, &id, &summary) }
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(()) => view.notice(&id, "Resumen guardado.", None, cx),
                    Err(error) => view.fail_notice(&id, format!("No se pudo guardar el resumen: {error}"), cx),
                }
                view.reload(cx);
            });
        })
        .detach();
        cx.notify();
    }

    fn cancel_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ops.edit.take().is_some() {
            window.focus(&self.focus);
            cx.notify();
        }
    }

    // --- Correo ---

    fn open_mail(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected_rec().map(|r| r.id.clone()) else {
            return;
        };
        if self.ops.mail.as_ref().is_some_and(|m| m.id == id) {
            self.ops.mail = None;
            window.focus(&self.focus);
            cx.notify();
            return;
        }
        let input = cx.new(|cx| TextInput::new("nombre@empresa.cl, otra@empresa.cl", hsla(TEXT), hsla(FAINT), hsla(TEXT), cx));
        window.focus(&input.focus_handle(cx));
        self.ops.mail = Some(Mail { id, input, error: None, sending: false });
        cx.notify();
    }

    fn close_mail(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ops.mail.take().is_some() {
            window.focus(&self.focus);
            cx.notify();
        }
    }

    fn send_mail(&mut self, cx: &mut Context<Self>) {
        let Some(paths) = self.paths() else {
            return;
        };
        let Some(mail) = self.ops.mail.as_mut().filter(|m| !m.sending) else {
            return;
        };
        let to = match pipeline::parse_recipients(mail.input.read(cx).text()) {
            Ok(to) => to,
            Err(error) => {
                mail.error = Some(error);
                cx.notify();
                return;
            }
        };
        mail.error = None;
        mail.sending = true;
        let id = mail.id.clone();
        let title = self.items.iter().find(|r| r.id == id).map(|r| r.title.clone()).unwrap_or_default();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn({
                    let id = id.clone();
                    async move {
                        // Se manda lo guardado: el archivo y el correo dicen lo mismo.
                        let summary = Summary::load(&paths.summary_path(&id))
                            .map_err(|e| e.to_string())?
                            .ok_or("No hay resumen guardado que mandar.")?;
                        let mail = OutgoingMail {
                            to,
                            subject: pipeline::mail_subject(Some(&summary), &title),
                            body: summary.body.clone(),
                        };
                        pipeline::send_mail(&paths, &mail)
                    }
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                match result {
                    Ok(outcome) => {
                        view.ops.mail = None;
                        let text = match outcome {
                            MailOutcome::Draft(url) => {
                                cx.open_url(&url);
                                "Borrador abierto en tu correo.".to_string()
                            }
                            MailOutcome::Sent(text) => text,
                        };
                        view.notice(&id, text, None, cx);
                    }
                    Err(error) => {
                        if let Some(mail) = view.ops.mail.as_mut() {
                            mail.sending = false;
                            mail.error = Some(error);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    // --- Exportar ---

    fn export(&mut self, format: Format, cx: &mut Context<Self>) {
        let Some(rec) = self.selected_rec().cloned() else {
            return;
        };
        self.ops.menu = None;
        let input = self.export_input();
        let dir = downloads_dir();
        let name = export::file_name(&rec.title, format);
        let picked = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn(async move |this, cx| {
            // Si el diálogo de Windows falla, a Descargas (y se avisa dónde).
            let (target, fallback) = match picked.await {
                Ok(Ok(Some(path))) => (path, false),
                Ok(Ok(None)) => return,
                _ => (unique_path(&dir.join(&name)), true),
            };
            let id = rec.id.clone();
            let result = cx
                .background_spawn(async move {
                    let (transcript, summary) = match input {
                        ExportInput::Loaded(t, s) => (t, s),
                        ExportInput::Disk(paths) => (
                            Transcript::load(&paths.transcript_path(&rec.id))?,
                            Summary::load(&paths.summary_path(&rec.id))?,
                        ),
                    };
                    let doc = export::Document {
                        recording: &rec,
                        transcript: transcript.as_ref(),
                        summary: summary.as_ref(),
                    };
                    export::write(&doc, format, &target)
                })
                .await;
            let _ = this.update(cx, |view, cx| match result {
                Ok(path) => {
                    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let text = if fallback {
                        format!("Guardado en Descargas: {name}")
                    } else {
                        format!("Exportado: {name}")
                    };
                    view.notice(&id, text, Some(path), cx);
                }
                Err(error) => view.fail_notice(&id, format!("No se pudo exportar: {error}"), cx),
            });
        })
        .detach();
        cx.notify();
    }
}

impl Running {
    fn new(job: Job) -> Self {
        Self { job, progress: 0.0, stage: None, draft: String::new(), sections: Vec::new() }
    }
}

// --- Elementos ---------------------------------------------------------------------

impl MeetingsView {
    /// El título de la cabecera: un clic lo vuelve un campo para renombrar.
    pub(super) fn title_view(&self, rec: &Recording, cx: &mut Context<Self>) -> AnyElement {
        if let Some((_, input)) = self.ops.rename.as_ref().filter(|(id, _)| *id == rec.id) {
            return div()
                .key_context(FIELD_CONTEXT)
                .on_action(cx.listener(|v, _: &FieldConfirm, window, cx| v.commit_rename(window, cx)))
                .on_action(cx.listener(|v, _: &FieldCancel, window, cx| v.cancel_rename(window, cx)))
                .on_action(cx.listener(|_, _: &Swallow, _, _| {}))
                .on_mouse_down_out(cx.listener(|v, _, window, cx| v.commit_rename(window, cx)))
                // El texto queda donde estaba el título: el fondo sale hacia afuera.
                .ml(px(-10.))
                .px(px(10.))
                .h(px(32.))
                .flex()
                .items_center()
                .rounded(px(10.))
                .bg(hsla(ITEM))
                .text_size(px(20.))
                .line_height(px(28.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(input.clone())
                .into_any_element();
        }
        let editable = self.paths().is_some();
        let title = div()
            .id("meeting-title")
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(32.))
            .text_size(px(20.))
            .line_height(px(28.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(div().min_w_0().truncate().child(SharedString::from(rec.title.clone())))
            .when(editable, |el| {
                el.cursor_pointer()
                    .tooltip(hover::tip("Renombrar · F2"))
                    .on_click(cx.listener(|v, _: &ClickEvent, window, cx| v.begin_rename(window, cx)))
            });
        if !editable {
            return title.into_any_element();
        }
        title
            .fx("meeting-title-fx", |el, h| {
                el.child(
                    svg()
                        .path("icons/pencil.svg")
                        .size(px(14.))
                        .flex_none()
                        .text_color(hsla(MUTED).opacity(h.over)),
                )
            })
            .into_any_element()
    }

    /// «Reunión eliminada · Deshacer», abajo sobre la lista (el reproductor
    /// queda libre), con una barrita que se vacía en `UNDO_TTL`.
    pub(super) fn undo_toast(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let seq = self.ops.pending_delete.as_ref()?.seq;
        let inset = super::GUTTER + 8.;
        let toast = div()
            .id("undo-toast")
            .absolute()
            .left(px(inset))
            .w(px(super::LIST_W - 16.))
            .h(px(44.))
            .pl(px(14.))
            .pr(px(7.))
            // Dentro del panel de la lista (radio 20, margen 8).
            .rounded(px(12.))
            .bg(hsla(TOAST))
            .occlude()
            .flex()
            .items_center()
            .gap(px(10.))
            .text_size(px(13.))
            .child(svg().path("icons/trash.svg").size(px(14.)).flex_none().text_color(hsla(MUTED)))
            .child(div().flex_1().min_w_0().truncate().child("Reunión eliminada"))
            .child(link_button("undo-delete", "Deshacer", cx.listener(|v, _: &ClickEvent, _, cx| v.undo_delete(cx))))
            .child(
                // La cuenta regresiva: una línea fina que se acorta hacia la
                // izquierda; puntas redondas, lejos de las esquinas.
                div()
                    .absolute()
                    .left(px(14.))
                    .right(px(14.))
                    .bottom(px(5.))
                    .h(px(2.))
                    .rounded_full()
                    .bg(hsla(TEXT).opacity(0.07))
                    .child(
                        div().h_full().rounded_full().bg(hsla(TEXT).opacity(0.32)).with_animation(
                            gpui::ElementId::NamedInteger("undo-count".into(), seq),
                            Animation::new(UNDO_TTL),
                            |el, t| el.w(gpui::relative(1.0 - t)),
                        ),
                    ),
            );
        Some(
            toast
                .with_animation(
                    gpui::ElementId::NamedInteger("undo-in".into(), seq),
                    Animation::new(Duration::from_millis(180)).with_easing(gpui::ease_out_quint()),
                    move |el, t| el.opacity(t).bottom(px(inset - 6. * (1. - t))),
                )
                .into_any_element(),
        )
    }

    /// La fila de acciones de la cabecera, a la derecha.
    pub(super) fn header_actions(&self, rec: &Recording, detail: &Detail, cx: &mut Context<Self>) -> AnyElement {
        let id = rec.id.clone();
        let has_audio = rec.mic_path.is_some() || rec.system_path.is_some();
        let writable = self.paths().is_some();
        let running = self.ops.jobs.get(&id);
        let outside_busy = running.is_none() && pipeline::busy(rec.status);
        let has_summary = !detail.sections.is_empty();
        // Sin voz no hay nada que resumir: «Transcribir de nuevo» queda en ⋯.
        let silent = detail.has_transcript && detail.blocks.is_empty();

        // 2 px: centrada con la línea del título (32 px), no con su borde.
        let mut row = div().mt(px(2.)).flex().flex_none().items_center().gap(px(6.));
        if let Some(run) = running {
            row = row.child(progress_pill(run));
        } else if writable && !outside_busy {
            if !detail.has_transcript && has_audio {
                let id = id.clone();
                row = row.child(primary_pill("meeting-transcribe", "icons/audio-lines.svg", "Transcribir", "", cx.listener(
                    move |v, _: &ClickEvent, _, cx| v.start_transcribe(id.clone(), cx),
                )));
            } else if detail.has_transcript && !has_summary && !silent {
                let id = id.clone();
                row = row.child(primary_pill("meeting-summarize", "icons/sparkles.svg", "Resumir", "Ctrl+Enter", cx.listener(
                    move |v, _: &ClickEvent, _, cx| v.start_summarize(id.clone(), cx),
                )));
            }
        }

        let can_export = detail.has_transcript || has_summary;
        let export_open = self.ops.menu == Some(Menu::Export);
        let export_button = div()
            .id("meeting-export")
            .h(px(28.))
            .pl(px(12.))
            .pr(px(9.))
            .flex()
            .items_center()
            .gap(px(5.))
            .rounded(px(14.))
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .when(!can_export, |el| el.text_color(hsla(FAINT)))
            .when(can_export, |el| {
                el.cursor_pointer()
                    .tooltip(hover::tip("Exportar · Ctrl+E"))
                    .on_click(cx.listener(|v, _: &ClickEvent, _, cx| v.toggle_menu(Menu::Export, cx)))
            })
            .child("Exportar")
            .child(svg().path("icons/chevron-down.svg").size(px(12.)).text_color(hsla(MUTED)))
            .hover_bg("meeting-export-fx", hsla(ITEM), hsla(if can_export { SURFACE_ON } else { ITEM }))
            .lit(export_open);
        let mut export = div().relative().child(export_button);
        if export_open {
            export = export.children(self.menu_panel(Menu::Export, 200., false, cx));
        }
        row = row.child(export);

        let more_open = self.ops.menu == Some(Menu::More);
        let more_button = div()
            .id("meeting-more")
            .size(px(28.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(14.))
            .cursor_pointer()
            .tooltip(hover::tip("Más · Ctrl+."))
            .on_click(cx.listener(|v, _: &ClickEvent, _, cx| v.toggle_menu(Menu::More, cx)))
            .child(svg().path("icons/ellipsis.svg").size(px(14.)).text_color(hsla(TEXT)))
            .hover_bg("meeting-more-fx", hsla(ITEM), hsla(SURFACE_ON))
            .lit(more_open);
        let mut more = div().relative().child(more_button);
        if more_open {
            more = more.children(self.menu_panel(Menu::More, 240., false, cx));
        }
        row.child(more).into_any_element()
    }

    /// Bajo la fecha: el error del último trabajo (con «Reintentar») o un
    /// aviso corto (copiado, exportado…).
    pub(super) fn notice_line(&self, rec: &Recording, cx: &mut Context<Self>) -> Option<AnyElement> {
        if let Some(failed) = self.ops.failures.get(&rec.id) {
            let what = match (failed.job, &failed.failure) {
                (_, Failure::LocalEngine) => String::new(),
                (Job::Transcribe, _) => "No se pudo transcribir. ".into(),
                (Job::Summarize, _) => String::new(),
            };
            let id = rec.id.clone();
            let mut line = div()
                .mt(px(4.))
                .px(px(12.))
                .py(px(7.))
                .rounded(px(12.))
                .bg(hsla(RED).opacity(0.12))
                .flex()
                .items_center()
                .gap(px(10.))
                .text_size(px(12.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_color(hsla(0xf0a497))
                        .child(format!("{what}{}", failed.failure.text())),
                );
            let job = failed.job;
            let retry = matches!(failed.failure, Failure::Message(_)) && self.paths().is_some();
            if failed.failure == Failure::LocalEngine {
                let id = id.clone();
                line = line.child(link_button("notice-groq", "Usar Groq", cx.listener(
                    move |v, _: &ClickEvent, _, cx| v.run_transcribe(id.clone(), true, cx),
                )));
            } else if retry && self.selected_rec().is_some_and(|r| r.id == id) {
                let id = id.clone();
                line = line.child(link_button("notice-retry", "Reintentar", cx.listener(
                    move |v, _: &ClickEvent, _, cx| match job {
                        Job::Transcribe => v.start_transcribe(id.clone(), cx),
                        Job::Summarize => v.start_summarize(id.clone(), cx),
                    },
                )));
            }
            let id = rec.id.clone();
            line = line.child(
                hover::round_button("notice-close", "icons/x.svg", "", false, hsla(TEXT), hsla(MUTED), cx.listener(
                    move |v, _: &ClickEvent, _, cx| {
                        v.ops.failures.remove(&id);
                        cx.notify();
                    },
                )),
            );
            return Some(line.into_any_element());
        }
        let notice = self.ops.notice.as_ref().filter(|n| n.id == rec.id)?;
        let mut line = div()
            .mt(px(4.))
            .flex()
            .items_center()
            .gap(px(10.))
            .text_size(px(12.))
            .text_color(hsla(MUTED))
            .child(div().size(px(6.)).flex_none().rounded_full().bg(hsla(super::GREEN)))
            .child(div().min_w_0().truncate().child(notice.text.clone()));
        if let Some(path) = notice.reveal.clone() {
            line = line.child(link_button("notice-reveal", "Mostrar", move |_, _, cx: &mut App| cx.reveal_path(&path)));
        }
        Some(line.into_any_element())
    }

    /// El menú abierto bajo su botón: entra fundiéndose y bajando 4 px, y
    /// mientras está tiene el foco (↑/↓, Inicio/Fin, Enter, Esc).
    fn menu_panel(&self, menu: Menu, width: f32, left: bool, cx: &mut Context<Self>) -> Option<AnyElement> {
        let entries = self.entries(menu);
        if entries.is_empty() {
            return None;
        }
        let cursor = self.ops.menu_cursor;
        let items = entries.into_iter().enumerate().map(|(ix, entry)| {
            let cmd = entry.cmd;
            menu_item(
                entry.id,
                entry.icon,
                entry.label,
                entry.detail,
                cursor == Some(ix),
                cx.listener(move |v, _: &ClickEvent, window, cx| v.run_cmd(menu, cmd.clone(), window, cx)),
                // El cursor sigue al mouse: nunca quedan dos ítems encendidos.
                cx.listener(move |v, hovered: &bool, _, cx| {
                    if *hovered && v.ops.menu_cursor != Some(ix) {
                        v.ops.menu_cursor = Some(ix);
                        cx.notify();
                    }
                }),
            )
        });
        let id = match menu {
            Menu::Export => "menu-export",
            Menu::More => "menu-more",
            Menu::Template => "menu-template",
            Menu::Regenerate => "menu-regenerate",
        };
        let panel = div()
            .id(id)
            .key_context(MENU_CONTEXT)
            .when_some(self.ops.menu_focus.as_ref(), |el, focus| el.track_focus(focus))
            .on_action(cx.listener(|v, _: &MenuPrev, _, cx| v.menu_step(-1, cx)))
            .on_action(cx.listener(|v, _: &MenuNext, _, cx| v.menu_step(1, cx)))
            .on_action(cx.listener(|v, _: &MenuFirst, _, cx| v.menu_edge(false, cx)))
            .on_action(cx.listener(|v, _: &MenuLast, _, cx| v.menu_edge(true, cx)))
            .on_action(cx.listener(|v, _: &MenuPick, window, cx| v.menu_pick(window, cx)))
            .on_action(cx.listener(|v, _: &MenuClose, _, cx| {
                v.ops.menu = None;
                cx.notify();
            }))
            .absolute()
            .when(left, |el| el.left_0())
            .when(!left, |el| el.right_0())
            .w(px(width))
            .p(px(4.))
            .rounded(px(14.))
            .bg(hsla(MENU))
            .flex()
            .flex_col()
            .gap(px(1.))
            .occlude()
            .on_mouse_down_out(cx.listener(|v, _, _, cx| v.close_menu(cx)))
            .children(items)
            .with_animation(
                "menu-in",
                Animation::new(super::motion::FADE).with_easing(gpui::ease_out_quint()),
                |el, t| el.opacity(t).top(px(34. - super::motion::RISE * (1. - t))),
            );
        Some(gpui::deferred(panel).with_priority(1).into_any_element())
    }

    // --- La pestaña Resumen ---

    pub(super) fn summary_view(&self, rec: &Recording, detail: &Detail, cx: &mut Context<Self>) -> AnyElement {
        if let Some(edit) = self.ops.edit.as_ref().filter(|e| e.id == rec.id) {
            return self.editor(edit, cx);
        }
        if let Some(run) = self.ops.running(&rec.id, Job::Summarize) {
            return streaming(run);
        }
        let writable = self.paths().is_some();
        if detail.sections.is_empty() {
            let busy = self.ops.jobs.contains_key(&rec.id) || pipeline::busy(rec.status);
            let silent = detail.has_transcript && detail.blocks.is_empty();
            let (title, hint) = if rec.status == atic_core::RecordingStatus::Summarizing {
                ("Resumiendo…", "Aparecerá aquí a medida que se escribe.")
            } else if silent {
                ("No hay nada que resumir", "No se escuchó a nadie en esta grabación.")
            } else if detail.has_transcript && writable && !busy {
                ("Esta reunión todavía no tiene resumen", "Elige cómo lo quieres y se escribe aquí mismo.")
            } else if detail.has_transcript {
                ("Esta reunión todavía no tiene resumen", "Espera a que termine lo que está en curso.")
            } else {
                ("Sin resumen", "Primero hay que transcribirla.")
            };
            let can_generate = detail.has_transcript && writable && !busy && !silent;
            let mut column = div()
                .flex_1()
                .py(px(48.))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(6.))
                .child(div().text_size(px(14.)).font_weight(FontWeight::MEDIUM).child(title))
                .child(div().text_size(px(13.)).text_color(hsla(MUTED)).child(hint));
            if can_generate {
                let id = rec.id.clone();
                column = column.child(
                    div()
                        .pt(px(14.))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(self.template_picker(Menu::Template, cx))
                        .child(primary_pill("summary-generate", "icons/sparkles.svg", "Generar resumen", "Ctrl+Enter", cx.listener(
                            move |v, _: &ClickEvent, _, cx| v.start_summarize(id.clone(), cx),
                        ))),
                );
            }
            return column.into_any_element();
        }

        let mut column = div().flex().flex_col().gap(px(10.));
        // Arriba: de qué es el resumen y lo que se hace con él.
        let mut bar = div().flex().items_center().gap(px(2.)).child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(12.))
                .text_color(hsla(FAINT))
                .child(detail.summary_meta.clone().unwrap_or_default()),
        );
        if writable {
            bar = bar
                .child(hover::round_button("summary-edit", "icons/pencil.svg", "Editar", false, hsla(TEXT), hsla(MUTED), cx.listener(
                    |v, _: &ClickEvent, window, cx| v.begin_edit(window, cx),
                )))
                .child(hover::round_button("summary-copy", "icons/copy.svg", "Copiar · Ctrl+Mayús+C", false, hsla(TEXT), hsla(MUTED), cx.listener(
                    |v, _: &ClickEvent, window, cx| v.copy_summary(window, cx),
                )))
                .child(hover::round_button(
                    "summary-mail",
                    "icons/mail.svg",
                    "Enviar por correo",
                    self.ops.mail.as_ref().is_some_and(|m| m.id == rec.id),
                    hsla(TEXT),
                    hsla(MUTED),
                    cx.listener(|v, _: &ClickEvent, window, cx| v.open_mail(window, cx)),
                ))
                .child(self.regenerate_button(cx));
        } else {
            bar = bar.child(hover::round_button("summary-copy", "icons/copy.svg", "Copiar · Ctrl+Mayús+C", false, hsla(TEXT), hsla(MUTED), cx.listener(
                |v, _: &ClickEvent, window, cx| v.copy_summary(window, cx),
            )));
        }
        column = column.child(bar);
        if let Some(mail) = self.ops.mail.as_ref().filter(|m| m.id == rec.id) {
            column = column.child(self.mail_card(mail, rec, detail, cx));
        }
        if let Some(subject) = &detail.subject {
            column = column.child(
                div().text_size(px(13.)).text_color(hsla(MUTED)).child(format!("Asunto: {subject}")),
            );
        }
        for (ix, section) in detail.sections.iter().enumerate() {
            let lead = ix == 0 && section.kind == Kind::Summary;
            column = column.child(section_card(section, lead, false));
        }
        column.into_any_element()
    }

    fn template_picker(&self, menu: Menu, cx: &mut Context<Self>) -> AnyElement {
        let open = self.ops.menu == Some(menu);
        let button = div()
            .id("summary-template")
            .h(px(28.))
            .pl(px(12.))
            .pr(px(9.))
            .flex()
            .items_center()
            .gap(px(5.))
            .rounded(px(14.))
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .cursor_pointer()
            .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| v.toggle_menu(menu, cx)))
            .child(pipeline::template_label(self.ops.template()))
            .child(svg().path("icons/chevron-down.svg").size(px(12.)).text_color(hsla(MUTED)))
            .hover_bg("summary-template-fx", hsla(ITEM), hsla(SURFACE_ON))
            .lit(open);
        let mut wrap = div().relative().child(button);
        if open {
            // Bajo el selector, a la izquierda: elegir solo elige.
            wrap = wrap.children(self.menu_panel(Menu::Template, 220., true, cx));
        }
        wrap.into_any_element()
    }

    fn regenerate_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let open = self.ops.menu == Some(Menu::Regenerate);
        let mut wrap = div().relative().child(hover::round_button(
            "summary-regenerate",
            "icons/rotate-cw.svg",
            "Volver a generar",
            open,
            hsla(TEXT),
            hsla(MUTED),
            cx.listener(|v, _: &ClickEvent, _, cx| v.toggle_menu(Menu::Regenerate, cx)),
        ));
        if open {
            // A la derecha: elegir ya genera de nuevo.
            wrap = wrap.children(self.menu_panel(Menu::Regenerate, 220., false, cx));
        }
        wrap.into_any_element()
    }

    fn mail_card(&self, mail: &Mail, rec: &Recording, detail: &Detail, cx: &mut Context<Self>) -> AnyElement {
        let subject = detail
            .subject
            .clone()
            .unwrap_or_else(|| format!("Seguimiento: {}", rec.title));
        div()
            .p(px(14.))
            .rounded(px(R_CARD))
            .bg(hsla(ITEM))
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .key_context(FIELD_CONTEXT)
                    .on_action(cx.listener(|v, _: &FieldConfirm, _, cx| v.send_mail(cx)))
                    .on_action(cx.listener(|v, _: &FieldCancel, window, cx| v.close_mail(window, cx)))
                    .on_action(cx.listener(|_, _: &Swallow, _, _| {}))
                    .h(px(34.))
                    .px(px(12.))
                    .rounded(px(10.))
                    .bg(hsla(FIELD))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .text_size(px(13.))
                    .child(div().flex_none().text_color(hsla(MUTED)).child("Para"))
                    .child(mail.input.clone()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(if mail.error.is_some() { hsla(0xf0a497) } else { hsla(FAINT) })
                            .child(mail.error.clone().unwrap_or_else(|| format!("Asunto: {subject}"))),
                    )
                    .child(ghost_pill("mail-cancel", "Cancelar", cx.listener(
                        |v, _: &ClickEvent, window, cx| v.close_mail(window, cx),
                    )))
                    .child(if mail.sending {
                        busy_pill("Enviando…").into_any_element()
                    } else {
                        primary_pill("mail-send", "icons/mail.svg", "Enviar", "Enter", cx.listener(
                            |v, _: &ClickEvent, _, cx| v.send_mail(cx),
                        ))
                        .into_any_element()
                    }),
            )
            .into_any_element()
    }

    fn editor(&self, edit: &Edit, cx: &mut Context<Self>) -> AnyElement {
        div()
            .key_context(EDITOR_CONTEXT)
            .on_action(cx.listener(|v, _: &EditorSave, window, cx| v.save_edit(window, cx)))
            .on_action(cx.listener(|v, _: &EditorCancel, window, cx| v.cancel_edit(window, cx)))
            .on_action(cx.listener(|_, _: &Swallow, _, _| {}))
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .h(px(440.))
                    .p(px(16.))
                    .rounded(px(R_CARD))
                    .bg(hsla(ITEM))
                    .text_size(px(14.))
                    .line_height(px(22.))
                    .child(edit.area.clone()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.))
                            .text_color(hsla(FAINT))
                            .child("Markdown simple: ## títulos, - listas, - [ ] tareas · Ctrl+Enter guarda"),
                    )
                    .child(ghost_pill("edit-cancel", "Cancelar", cx.listener(
                        |v, _: &ClickEvent, window, cx| v.cancel_edit(window, cx),
                    )))
                    .child(primary_pill("edit-save", "icons/check.svg", "Guardar", "Ctrl+Enter", cx.listener(
                        |v, _: &ClickEvent, window, cx| v.save_edit(window, cx),
                    ))),
            )
            .into_any_element()
    }
}

/// El resumen mientras llega: lo escrito hasta ahora, con el punto de la
/// última sección latiendo, y en qué va.
fn streaming(run: &Running) -> AnyElement {
    let status = run.stage.map(|s| s.label()).unwrap_or_else(|| {
        if run.draft.is_empty() { "Preparando…".into() } else { "Escribiendo…".into() }
    });
    if run.sections.is_empty() {
        return div()
            .flex_1()
            .py(px(48.))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(9.))
                    .child(pulse_dot("summary-wait-dot", super::AMBER))
                    .child(div().text_size(px(14.)).font_weight(FontWeight::MEDIUM).child("Resumiendo…")),
            )
            .child(div().text_size(px(13.)).text_color(hsla(MUTED)).child(status))
            .into_any_element();
    }
    let last = run.sections.len() - 1;
    let mut column = div().flex().flex_col().gap(px(10.));
    for (ix, section) in run.sections.iter().enumerate() {
        let lead = ix == 0 && section.kind == Kind::Summary;
        column = column.child(section_card(section, lead, ix == last));
    }
    column
        .child(div().pt(px(2.)).text_size(px(12.)).text_color(hsla(FAINT)).child(status))
        .into_any_element()
}

pub(super) fn pulse_dot(id: &'static str, color: u32) -> impl IntoElement {
    div().size(px(7.)).flex_none().rounded_full().bg(hsla(color)).with_animation(
        id,
        Animation::new(Duration::from_millis(1100)).repeat().with_easing(pulsating_between(0.3, 1.0)),
        |el, t| el.opacity(t),
    )
}

fn progress_pill(run: &Running) -> AnyElement {
    let (label, value) = match run.job {
        Job::Transcribe if run.progress > 0.0 => (
            format!("Transcribiendo {} %", (run.progress * 100.0).round() as i32),
            Some(run.progress),
        ),
        Job::Transcribe => ("Transcribiendo…".to_string(), None),
        Job::Summarize => ("Resumiendo…".to_string(), None),
    };
    let mut pill = div()
        .h(px(28.))
        .px(px(12.))
        .flex()
        .items_center()
        .gap(px(8.))
        .rounded(px(14.))
        .bg(hsla(ITEM))
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(hsla(MUTED))
        .child(pulse_dot("job-dot", super::AMBER))
        .child(label);
    if let Some(value) = value {
        // La barra va con puntas redondas en ambos lados: con el relleno
        // más ancho que alto nunca queda una esquina cuadrada.
        let fill = (56.0 * value.clamp(0.0, 1.0)).max(4.0);
        pill = pill.child(
            div()
                .w(px(56.))
                .h(px(4.))
                .rounded_full()
                .bg(hsla(0x3a3a37))
                .child(div().w(px(fill)).h_full().rounded_full().bg(hsla(super::AMBER))),
        );
    }
    pill.into_any_element()
}

fn template_item_id(template: SummaryTemplate, run: bool) -> &'static str {
    match (template, run) {
        (SummaryTemplate::SummaryKeyPoints, false) => "pick-key-points",
        (SummaryTemplate::ExecutiveMinutes, false) => "pick-minutes",
        (SummaryTemplate::ActionItems, false) => "pick-actions",
        (SummaryTemplate::FollowupEmail, false) => "pick-email",
        (SummaryTemplate::SummaryKeyPoints, true) => "regen-key-points",
        (SummaryTemplate::ExecutiveMinutes, true) => "regen-minutes",
        (SummaryTemplate::ActionItems, true) => "regen-actions",
        (SummaryTemplate::FollowupEmail, true) => "regen-email",
    }
}

fn primary_pill(
    id: &'static str,
    icon: &'static str,
    label: &'static str,
    // El atajo, en el globito (vacío: sin globito).
    keys: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .when(!keys.is_empty(), |el| el.tooltip(hover::tip(keys)))
        .h(px(28.))
        .pl(px(11.))
        .pr(px(13.))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .rounded(px(14.))
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(hsla(INK))
        .cursor_pointer()
        .on_click(on_click)
        .child(svg().path(icon).size(px(13.)).text_color(hsla(INK)))
        .child(label)
        .hover_bg(id, hsla(PRIMARY), hsla(PRIMARY_HOVER))
}

fn ghost_pill(
    id: &'static str,
    label: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .h(px(28.))
        .px(px(13.))
        .flex()
        .flex_none()
        .items_center()
        .rounded(px(14.))
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .on_click(on_click)
        .child(label)
        .hover_bg(id, hsla(TEXT).opacity(0.0), hsla(TEXT).opacity(0.08))
}

fn busy_pill(label: &'static str) -> impl IntoElement {
    div()
        .h(px(28.))
        .px(px(13.))
        .flex()
        .flex_none()
        .items_center()
        .gap(px(8.))
        .rounded(px(14.))
        .bg(hsla(SURFACE_ON))
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(hsla(MUTED))
        .child(pulse_dot("busy-dot", super::AMBER))
        .child(label)
}

fn link_button(
    id: &'static str,
    label: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .flex_none()
        .h(px(24.))
        .px(px(10.))
        .flex()
        .items_center()
        .rounded(px(12.))
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(hsla(TEXT))
        .cursor_pointer()
        .on_click(on_click)
        .child(label)
        .hover_bg(id, hsla(TEXT).opacity(0.06), hsla(TEXT).opacity(0.14))
}

/// Una fila de menú: ícono (o su hueco, para alinear), texto y, a la
/// derecha, un detalle tenue (la extensión o el atajo). `lit`: la marcada
/// con el teclado.
fn menu_item(
    id: &'static str,
    icon: Option<&'static str>,
    label: &'static str,
    detail: Option<&'static str>,
    lit: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_hover: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .h(px(32.))
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(10.))
        .rounded(px(10.))
        .text_size(px(13.))
        .text_color(hsla(TEXT))
        .cursor_pointer()
        .on_click(on_click)
        .on_hover(on_hover)
        .when_some(icon, |el, icon| {
            el.child(if icon.is_empty() {
                div().size(px(14.)).flex_none().into_any_element()
            } else {
                svg().path(icon).size(px(14.)).flex_none().text_color(hsla(MUTED)).into_any_element()
            })
        })
        .child(div().flex_1().min_w_0().truncate().child(label))
        .when_some(detail, |el, detail| {
            el.child(div().flex_none().text_size(px(12.)).text_color(hsla(FAINT)).child(detail))
        })
        .hover_bg(id, hsla(MENU), hsla(MENU_HOVER))
        .lit(lit)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_respaldo_no_pisa_archivos() {
        let dir = std::env::temp_dir().join(format!("meetings-unique-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let first = dir.join("acta.pdf");
        assert_eq!(unique_path(&first), first);
        std::fs::write(&first, b"x").unwrap();
        assert_eq!(unique_path(&first), dir.join("acta (2).pdf"));
        std::fs::write(dir.join("acta (2).pdf"), b"x").unwrap();
        assert_eq!(unique_path(&first), dir.join("acta (3).pdf"));
        std::fs::remove_dir_all(dir).ok();
    }
}
