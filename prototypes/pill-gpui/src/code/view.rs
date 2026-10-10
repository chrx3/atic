//! Lo que se dibuja de Atic Code, con la forma de la referencia: a la izquierda la
//! conversación nueva y los proyectos con sus conversaciones; al centro el
//! chat con su encabezado y la caja de texto con la barra de estado; a la
//! derecha, cuando se abre, los cambios o los archivos.
//!
//! No hay barra de ventana aparte: el encabezado de cada columna arrastra la
//! ventana y los botones de Windows van en la de más a la derecha.
//!
//! El texto de Claude se muestra con el `Markdown` de gpui-m3 (listas, citas, tablas,
//! enlaces, bloques de código resaltados). No se puede seleccionar (GPUI no lo trae):
//! cada respuesta y cada bloque tiene «Copiar».

use std::path::PathBuf;

use gpui::{
    canvas, div, prelude::*, px, svg, AnyElement, ClickEvent, ClipboardItem, Context, Div, Focusable, FontWeight,
    Hsla, MouseButton, MouseDownEvent, ScrollWheelEvent, SharedString, Stateful, Window,
};
use serde_json::Value;

use super::chat::{Item, ToolCall};
use super::config;
use super::enter::Enter;
use super::git::{FileChange, Repo};
use super::{Attachment, CodeView, Menu, SessionInfo, Side, COMPOSER, LOOSE};
use crate::space::chrome;
use super::style::{t, Style};

const HEAD_H: f32 = 52.0;
const SIDE_W: f32 = 264.0;
const RIGHT_W: f32 = 380.0;
const DOC_W: f32 = 600.0;
const THREAD_W: f32 = 860.0;
/// La columna de la pantalla de inicio de Expressive.
const HERO_W: f32 = 720.0;
/// Cuántas partes del final del hilo entran animadas.
const ENTER_MAX: usize = 3;
const HERO_PLACEHOLDER: &str = "Pregunta lo que quieras · @ para mencionar · / para acciones";
/// Sin proyecto no hay archivos que mencionar.
const HERO_PLACEHOLDER_LOOSE: &str = "Pregunta lo que quieras · / para acciones";
/// Las sugerencias del inicio: ícono, etiqueta y lo que dejan escrito.
const SUGGESTIONS: [(&str, &str, &str); 4] = [
    ("layers", "Explícame el proyecto", "Explícame cómo está organizado este proyecto y por dónde empezar."),
    ("diff", "Revisa mis cambios", "Revisa mis cambios sin commitear y dime si ves algún problema."),
    ("check", "Escribe pruebas", "Escribe pruebas para "),
    ("search", "Busca errores", "Busca errores probables en "),
];
/// Lo que se muestra del resultado de una herramienta abierta.
const RESULT_LINES: usize = 40;
/// Conversaciones anteriores que se listan bajo un proyecto.
const HISTORY_SHOWN: usize = 12;

fn fg() -> Hsla {
    t().text
}
fn muted() -> Hsla {
    t().muted
}
fn faint() -> Hsla {
    t().faint
}
fn on_accent() -> Hsla {
    t().on_accent
}
/// La barra lateral: sobre el fondo de la ventana cuando los paneles flotan.
fn side_bg() -> Hsla {
    let t = t();
    if t.gap > 0. { gpui::transparent_black() } else { t.pane }
}
fn center_bg() -> Hsla {
    t().editor
}
/// La marca de cambio de modelo: una línea con el nombre al centro, como en
/// la referencia. Pendiente, más tenue (se grabará con el próximo mensaje). En
/// Expressive, el `DividerLabel` de gpui-m3 con ondas y su píldora.
fn model_mark(id: String, label: String, pending: bool) -> AnyElement {
    if expressive() {
        return gpui_m3::DividerLabel::new(SharedString::from(id), label).pill().icon("cpu").pending(pending).into_any_element();
    }
    rule_mark("icons/cpu.svg", label, pending)
}

/// «Contexto compactado»: en Expressive, el `DividerLabel` sencillo.
fn compact_mark(id: String) -> AnyElement {
    if expressive() {
        return gpui_m3::DividerLabel::new(SharedString::from(id), "Contexto compactado").into_any_element();
    }
    rule_mark("icons/layers.svg", "Contexto compactado".into(), false)
}

/// Un separador con su etiqueta al centro, para Formal y Liquid Glass.
fn rule_mark(icon: &'static str, label: String, pending: bool) -> AnyElement {
    let rule = || div().flex_1().h(px(1.)).bg(line());
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(px(12.))
        .text_color(faint())
        .when(pending, |el| el.opacity(0.7))
        .child(rule())
        .child(svg().path(icon).size(px(12.)).text_color(faint()))
        .child(label)
        .child(rule())
        .into_any_element()
}

fn line() -> Hsla {
    t().border
}
fn hover_bg() -> Hsla {
    t().hover
}
fn selected_bg() -> Hsla {
    t().sel
}
fn card_bg() -> Hsla {
    t().raised
}
pub(super) fn code_bg() -> Hsla {
    let t = t();
    match t.style {
        Style::Formal => t.pane,
        Style::Expressive => t.raised,
        Style::Glass => t.control,
    }
}
fn code_text() -> Hsla {
    t().text
}
fn accent() -> Hsla {
    t().accent
}
fn accent_soft() -> Hsla {
    t().accent_soft
}
fn on_accent_soft() -> Hsla {
    t().on_accent_soft
}
fn accent_line() -> Hsla {
    let t = t();
    if t.style == Style::Expressive { t.accent_soft } else { t.accent.opacity(0.35) }
}
fn green() -> Hsla {
    t().ok
}
fn red() -> Hsla {
    t().bad
}
fn amber() -> Hsla {
    t().warn
}
fn added_bg() -> Hsla {
    t().add
}
fn removed_bg() -> Hsla {
    t().del
}

// Radios según el estilo.
fn r_ctl() -> f32 {
    t().r_ctl
}
fn r_btn() -> f32 {
    t().r_btn
}
fn r_chip() -> f32 {
    t().r_chip
}
/// Tarjetas del chat: herramientas, bloques de código, grupos de cambios.
pub(super) fn r_card() -> f32 {
    match t().style {
        Style::Formal => 8.,
        Style::Expressive => 16.,
        Style::Glass => 14.,
    }
}
fn mono() -> &'static str {
    t().mono
}

impl Render for CodeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_style(window, cx);
        self.run_palette(window, cx);
        self.ensure_probe(cx);
        if self.settings_open && self.claude_info.is_none() {
            self.check_claude(cx);
        }
        let t = t();
        self.composer_focused = self.composer.read(cx).focus_handle(cx).is_focused(window);
        // Si la caja pierde el foco, la lista de @-menciones se cierra.
        if self.mention.is_some() && !self.composer_focused {
            self.close_mention(cx);
        }
        self.pick_focused_other(window, cx);
        self.commit_renames_on_blur(window, cx);
        // El título de la ventana, como en la referencia: «espacio — Atic Code».
        let window_title = match (self.in_loose_chat(), self.workspaces.active()) {
            (true, _) => "Chats — Atic Code".to_string(),
            (false, Some(workspace)) => format!("{} — Atic Code", workspace.name),
            (false, None) => "Atic Code".to_string(),
        };
        if window_title != self.window_title {
            window.set_window_title(&window_title);
            self.window_title = window_title;
        }
        // Sin conversación empezada (el inicio), la caja invita a escribir; la «@»
        // solo se anuncia donde funciona: con un proyecto abierto.
        let hero = self.active_chat().is_none_or(|c| c.workspace == LOOSE && c.items.is_empty() && c.session_id.is_none());
        let placeholder = match (hero && t.style == Style::Expressive, self.mentions_enabled()) {
            (true, true) => HERO_PLACEHOLDER,
            (true, false) => HERO_PLACEHOLDER_LOOSE,
            _ => "Responde a Claude…",
        };
        if self.composer.read(cx).placeholder() != placeholder {
            self.composer.update(cx, |area, cx| area.set_placeholder(placeholder, cx));
        }
        let maximized = window.is_maximized();
        self.focus_pending_editor(window, cx);
        let right_open = self.side.is_some() || self.doc.is_some() || self.tabs.showing();
        // La configuración de Expressive es un diálogo: sale animado.
        let settings_exit = gpui_m3::motion::presence("settings-presence", self.settings_open, window, cx);
        div()
            .id("atic-code")
            .key_context("AticCode")
            .on_action(cx.listener(Self::close_menu))
            .on_action(cx.listener(Self::open_palette_action))
            .on_action(cx.listener(Self::new_conversation_action))
            .on_action(cx.listener(Self::toggle_sidebar))
            .on_action(cx.listener(Self::toggle_settings))
            .on_action(cx.listener(Self::attach_action))
            .on_action(cx.listener(Self::show_files_action))
            .on_action(cx.listener(Self::show_changes_action))
            .on_action(cx.listener(Self::open_folder_action))
            .on_action(cx.listener(Self::clear_conversation_action))
            .on_action(cx.listener(Self::toggle_terminal_action))
            .on_action(cx.listener(Self::submit_answers_action))
            .on_action(cx.listener(Self::paste))
            .size_full()
            .flex()
            .bg(t.bg)
            .font_family(t.font)
            .text_color(fg())
            .text_size(px(t.fs))
            .track_focus(&self.focus)
            // Con paneles flotantes, aire alrededor del chat y del panel derecho.
            .when(t.gap > 0., |el| el.py(px(t.gap)).pr(px(t.gap)))
            .map(|el| if expressive() { el.child(self.sidebar_m3(window, cx)) } else { el.child(self.sidebar(cx)) })
            .child(self.chat_area(window, maximized && !right_open, !right_open, cx))
            .when(right_open, |el| el.child(self.right_panel(window, maximized, cx)))
            .when_some(self.menu_layer(window, cx), |el, menu| el.child(menu))
            .when_some(self.session_menu_layer(window, cx), |el, menu| el.child(menu))
            .when_some(self.space_menu_layer(window, cx), |el, menu| el.child(menu))
            .when_some(self.new_space_dialog(window, cx), |el, dialog| el.child(dialog))
            .when_some(self.close_dialog(window, cx), |el, dialog| el.child(dialog))
            .when_some(self.pop_layer(window, cx), |el, pop| el.child(pop))
            .when_some(self.mention_layer(window, cx), |el, list| el.child(list))
            .when_some(self.profile_layer(window, cx), |el, card| el.child(card))
            .when_some(self.style_menu_layer(window, cx), |el, card| el.child(card))
            .when_some(self.crop_dialog(window, cx), |el, dialog| el.child(dialog))
            .when(self.palette_open, |el| el.child(self.palette.clone()))
            .when(!expressive() && self.settings_open, |el| el.child(self.settings_flat(cx)))
            .when_some(settings_exit.filter(|_| expressive()), |el, progress| el.child(self.settings_m3(progress, window, cx)))
            .when_some(self.toast_last.show("toast-presence", self.toast.clone(), window, cx), |el, shown| {
                let text = shown.value.clone();
                el.child(
                    div().absolute().bottom(px(24.)).left_0().right_0().flex().justify_center().child(shown.wrap(
                        gpui_m3::Exit::Sink,
                        gpui_m3::Toast::new(("toast", self.toast_gen as usize), text),
                    )),
                )
            })
            // El círculo del cambio de tema (`theme_reveal`); no dibuja nada si no hay uno en curso.
            .child(gpui_m3::ThemeReveal::new())
    }
}

// --- Piezas comunes ----------------------------------------------------------------

/// Un hijo de la pantalla de inicio: sube 18 px con resorte, tras `delay` segundos.
fn rise(id: &'static str, delay: f32, el: Div, window: &mut Window, cx: &mut gpui::App) -> Div {
    Enter::new(id).from(0., 18.).delay(delay).spring(gpui_m3::Spring::SPATIAL).apply(el, window, cx)
}

/// Un chip del composer entra con el resorte rápido (`m3-chip-in`, `motion.css:706`): GPUI no
/// escala, así que sube 8 px y se funde.
fn chip_in(id: String, chip: AnyElement, window: &mut Window, cx: &mut gpui::App) -> Div {
    let enter = Enter::new(SharedString::from(id)).from(0., 8.).spring(gpui_m3::Spring::SPATIAL_FAST);
    if expressive() { enter.apply(div(), window, cx).child(chip) } else { div().child(chip) }
}

/// Un número estable por clave de conversación, para saber cuándo cambió la visible.
fn key_hash(key: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish()
}

pub(super) fn icon_button(id: impl Into<gpui::ElementId>, icon: &'static str, tip: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(28.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded(px(r_ctl().min(14.)))
        .cursor_pointer()
        .hover(|el| el.bg(hover_bg()))
        .tooltip(crate::hover::tip(tip))
        .child(svg().path(icon).size(px(15.)).text_color(muted()))
}

pub(super) fn chip(id: impl Into<gpui::ElementId>, label: impl Into<SharedString>, on: bool) -> Stateful<Div> {
    div()
        .id(id)
        .px(px(12.))
        .h(px(30.))
        .flex()
        .items_center()
        .flex_none()
        .rounded(px(r_chip()))
        .border_1()
        .text_size(px(13.))
        .cursor_pointer()
        .when(on, |el| el.bg(accent_soft()).border_color(accent_line()).text_color(on_accent_soft()))
        .when(!on, |el| el.border_color(line()).text_color(fg()).hover(|el| el.bg(hover_bg())))
        .child(label.into())
}

/// Una píldora de la barra de estado o de la caja de texto: en Formal es
/// texto suelto; en los otros estilos, una cápsula con borde.
fn pill(id: impl Into<gpui::ElementId>) -> Stateful<Div> {
    let t = t();
    div()
        .id(id)
        .h(px(30.))
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(7.))
        .rounded(px(t.r_btn.min(15.)))
        .text_size(px(13.))
        .text_color(muted())
        .when(t.style != Style::Formal, |el| el.border_1().border_color(line()).bg(t.control))
}

/// La sombra de lo que flota (menús, la caja de texto en vidrio).
pub(super) fn float_shadow() -> Vec<gpui::BoxShadow> {
    let t = t();
    vec![gpui::BoxShadow {
        color: t.shadow,
        offset: gpui::point(px(0.), px(12.)),
        blur_radius: px(32.),
        spread_radius: px(0.),
    }]
}

/// Expressive usa los componentes de gpui-m3; Formal y Liquid Glass, los de aquí.
pub(super) fn expressive() -> bool {
    t().style == Style::Expressive
}

/// gpui-m3 pide los íconos por nombre (`plus`), no por ruta (`icons/plus.svg`).
pub(super) fn m3_icon(path: &str) -> &str {
    match path {
        "icons/rotate-cw.svg" => "refresh",
        _ => path.strip_prefix("icons/").and_then(|name| name.strip_suffix(".svg")).unwrap_or(path),
    }
}

/// `icon_button` con su acción; en Expressive, el `IconButton` de gpui-m3.
fn icon_action(
    id: impl Into<gpui::ElementId>,
    icon: &'static str,
    tip: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> AnyElement {
    if expressive() {
        gpui_m3::IconButton::new(id, m3_icon(icon)).size(px(28.)).tooltip(tip).on_click(on_click).into_any_element()
    } else {
        icon_button(id, icon, tip).on_click(on_click).into_any_element()
    }
}

/// La fila de una lista lateral: ícono, texto y lo que venga al final.
/// `on` la marca con fondo; `accented` solo pinta el ícono y el texto.
fn nav_row(
    id: impl Into<gpui::ElementId>,
    icon: &'static str,
    label: impl Into<SharedString>,
    on: bool,
    accented: bool,
) -> Stateful<Div> {
    let tint = on || accented;
    div()
        .id(id)
        .h(px(36.))
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(10.))
        .rounded(px(r_btn().min(18.)))
        .cursor_pointer()
        .when(on, |el| el.bg(selected_bg()))
        .when(!on, |el| el.hover(|el| el.bg(hover_bg())))
        .child(svg().path(icon).size(px(16.)).flex_none().text_color(if tint { accent() } else { muted() }))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .truncate()
                .text_color(if accented { accent() } else { fg() })
                .when(accented, |el| el.font_weight(FontWeight::MEDIUM))
                .child(label.into()),
        )
}

/// Un chip con un archivo y su «×»: el de Expressive es el `Chip::input` de gpui-m3.
fn file_chip(
    id: (&'static str, usize),
    name: String,
    remove: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> AnyElement {
    let t = t();
    if expressive() {
        return gpui_m3::Chip::input(id, name)
            .leading(svg().path("icons/file.svg").size(px(16.)).text_color(muted()))
            .on_remove(remove)
            .into_any_element();
    }
    div()
        .h(px(30.))
        .pl(px(10.))
        .pr(px(4.))
        .flex()
        .items_center()
        .gap(px(6.))
        .rounded(px(t.r_chip.min(15.)))
        .bg(t.control)
        .border_1()
        .border_color(line())
        .text_size(px(12.5))
        .child(svg().path("icons/file.svg").size(px(13.)).text_color(muted()))
        .child(div().max_w(px(220.)).truncate().child(name))
        .child(
            div()
                .id((SharedString::from(format!("{}-x", id.0)), id.1))
                .size(px(22.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .cursor_pointer()
                .hover(|el| el.bg(hover_bg()))
                .tooltip(crate::hover::tip("Quitar"))
                .on_click(remove)
                .child(svg().path("icons/x.svg").size(px(11.)).text_color(muted())),
        )
        .into_any_element()
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string())
}

/// El markdown de una respuesta, del razonamiento, del plan o del resultado de un
/// subagente: el `Markdown` de gpui-m3 en los tres estilos (`style::apply_m3` le pasa los
/// colores de cada uno). Los enlaces abren el navegador; un `código` en línea que parece
/// una ruta abre el archivo en el visor (relativa al espacio de la conversación). Durante
/// el streaming, gpui-m3 guarda lo ya interpretado por texto y sigue el bloque de código
/// desde su última línea completa.
pub(super) fn markdown(id: &str, text: &str, cx: &mut Context<CodeView>) -> AnyElement {
    let view = cx.entity().downgrade();
    gpui_m3::Markdown::new(SharedString::from(id.to_string()), text.to_string())
        .on_link(|url, _, cx| {
            if super::files::is_safe_link(url) {
                cx.open_url(url);
            }
        })
        .on_path(move |path, _, cx| {
            let path = path.to_string();
            let _ = view.update(cx, |view, cx| view.open_ref(&path, cx));
        })
        .into_any_element()
}

/// Verbo en español y argumento de una herramienta de Claude Code.
fn tool_label(tool: &ToolCall) -> (&'static str, String) {
    let verb = match tool.name.as_str() {
        "Read" => "Leer",
        "Edit" | "MultiEdit" | "NotebookEdit" => "Editar",
        "Write" => "Escribir",
        "Bash" | "PowerShell" => "Ejecutar",
        "BashOutput" => "Salida",
        "KillShell" | "KillBash" => "Detener",
        "Grep" | "Glob" => "Buscar",
        "WebFetch" => "Abrir",
        "WebSearch" => "Buscar en la web",
        "Task" | "Agent" => "Agente",
        "TodoWrite" => "Tareas",
        "ExitPlanMode" => "Plan",
        "AskUserQuestion" => "Pregunta",
        "Artifact" => "Artifact",
        _ => "",
    };
    let mut summary = tool.summary();
    if matches!(tool.name.as_str(), "Read" | "Edit" | "MultiEdit" | "Write" | "NotebookEdit") {
        summary = summary.rsplit(['\\', '/']).next().unwrap_or(&summary).to_string();
    }
    (verb, summary)
}

/// La última línea no vacía de lo que lleva escrito un razonamiento (mira solo el
/// final: es barato aunque el texto sea largo).
pub(super) fn last_line(text: &str) -> String {
    let mut from = text.len().saturating_sub(240);
    while !text.is_char_boundary(from) {
        from += 1;
    }
    let tail = text[from..].trim_end();
    tail.rsplit('\n').next().unwrap_or_default().trim().to_string()
}

/// Las tareas de `TodoWrite`: texto y estado.
pub(super) fn todos(tool: &ToolCall) -> Vec<(String, String)> {
    tool.input
        .as_ref()
        .and_then(|input| input.get("todos"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|todo| {
            let text = |name: &str| todo.get(name).and_then(Value::as_str).unwrap_or_default().to_string();
            let status = text("status");
            let shown = if status == "in_progress" { todo.get("activeForm").and_then(Value::as_str).map(str::to_string) } else { None };
            (shown.unwrap_or_else(|| text("content")), status)
        })
        .collect()
}

// --- Barra lateral -----------------------------------------------------------------

impl CodeView {
    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.workspaces.active_id();
        let mut projects = div().flex().flex_col().gap(px(2.));
        // Favoritos primero y en el orden guardado, igual que en Expressive.
        for workspace in self.sidebar_ids().into_iter().filter_map(|id| self.workspaces.get(id)) {
            let id = workspace.id;
            // Renombrando (desde el menú del clic derecho): el campo ocupa la fila.
            if self.renaming_space == Some(id) {
                projects = projects.child(div().child(self.space_rename_field.clone()));
                continue;
            }
            let selected = active == Some(id);
            let icon = if selected { "icons/folder-open.svg" } else { "icons/folder.svg" };
            let actions = div()
                .flex()
                .invisible()
                .group_hover("ws-row", |el| el.visible())
                .child(icon_action(
                    ("ws-add", id),
                    "icons/folder-plus.svg",
                    "Agregar carpeta",
                    cx.listener(move |view, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        view.pick_folders(Some(id), cx);
                    }),
                ))
                .child(icon_action(
                    ("ws-remove", id),
                    "icons/x.svg",
                    "Quitar de la lista (no borra archivos)",
                    cx.listener(move |view, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        view.remove_workspace(id, cx);
                    }),
                ));
            let select = cx.listener(move |view, _: &ClickEvent, _, cx| view.select_workspace(id, cx));
            projects = projects.child(if expressive() {
                gpui_m3::NavItem::new(("ws", id), workspace.name.clone())
                    .group("ws-row")
                    .leading(gpui_m3::Avatar::new(workspace.name.clone()))
                    .selected(selected)
                    .trailing(actions)
                    .on_click(select)
                    .into_any_element()
            } else {
                nav_row(("ws", id), icon, workspace.name.clone(), false, selected)
                    .group("ws-row")
                    .on_click(select)
                    // El clic derecho abre el menú del espacio (renombrar, favorito, carpetas, quitar).
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |view, event: &MouseDownEvent, _, cx| {
                            view.space_menu = Some((id, event.position));
                            cx.notify();
                        }),
                    )
                    .child(actions)
                    .into_any_element()
            });
            if selected {
                projects = projects.children(self.chat_rows(id, 26., cx));
            }
        }
        if self.workspaces.list().is_empty() {
            projects = projects.child(
                div()
                    .px(px(10.))
                    .py(px(6.))
                    .text_size(px(13.))
                    .text_color(muted())
                    .child("Agrega una carpeta para empezar."),
            );
        }
        let new_on = self.active.is_none() && self.workspaces.active().is_some();
        let t = t();
        let on_new = cx.listener(|view, _: &ClickEvent, window, cx| {
            if let Some(id) = view.active_workspace() {
                view.new_chat(id, window, cx);
            }
        });
        // En Expressive, «Nueva conversación» es el FAB extendido de M3.
        let new_chat = if t.style == Style::Expressive {
            gpui_m3::Fab::new("chat-new", "plus").label("Nueva conversación").on_click(on_new).into_any_element()
        } else {
            nav_row("chat-new", "icons/plus.svg", "Nueva conversación", new_on, false).on_click(on_new).into_any_element()
        };
        let on_settings = cx.listener(|view, _: &ClickEvent, _, cx| {
            view.settings_open = true;
            cx.notify();
        });
        let settings_row = if t.style == Style::Expressive {
            gpui_m3::NavItem::new("code-settings", "Configuración de Claude").icon("gear").on_click(on_settings).into_any_element()
        } else {
            nav_row("code-settings", "icons/settings-2.svg", "Configuración de Claude", false, false)
                .on_click(on_settings)
                .into_any_element()
        };
        div()
            .w(px(SIDE_W))
            .flex_none()
            .flex()
            .flex_col()
            .bg(side_bg())
            .when(t.gap == 0., |el| el.border_r_1().border_color(line()))
            .child(
                div()
                    .h(px(HEAD_H))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .pl(px(16.))
                    .child(chrome::logo(20.0))
                    .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child("Atic Code"))
                    .child(chrome::drag(HEAD_H)),
            )
            .child(div().px(px(10.)).pt(px(4.)).child(new_chat))
            .child(
                div()
                    .px(px(20.))
                    .pt(px(18.))
                    .pb(px(6.))
                    .flex()
                    .items_center()
                    .child(div().flex_1().text_size(px(12.5)).text_color(muted()).child("Proyectos"))
                    .child(icon_action(
                        "ws-create",
                        "icons/folder-plus.svg",
                        "Nuevo proyecto con carpetas",
                        cx.listener(|view, _: &ClickEvent, window, cx| view.open_new_space(window, cx)),
                    )),
            )
            .child(
                div()
                    .id("code-projects")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .px(px(10.))
                    .pb(px(12.))
                    .child(projects)
                    // Los chats sin proyecto, igual que la sección «Chats» de Expressive.
                    .child(
                        div()
                            .px(px(10.))
                            .pt(px(18.))
                            .pb(px(6.))
                            .flex()
                            .items_center()
                            .child(div().flex_1().text_size(px(12.5)).text_color(muted()).child("Chats"))
                            .child(icon_action(
                                "loose-new",
                                "icons/plus.svg",
                                "Nuevo chat sin proyecto",
                                cx.listener(|view, _: &ClickEvent, window, cx| view.new_loose_chat(window, cx)),
                            )),
                    )
                    .child(div().flex().flex_col().gap(px(2.)).children(self.chat_rows(LOOSE, 0., cx))),
            )
            .child(
                div()
                    .p(px(10.))
                    .when(t.gap == 0., |el| el.border_t_1().border_color(line()))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    // El perfil (nombre y foto): su tarjeta y el recorte son los de Expressive.
                    .child(
                        div()
                            .id("profile-btn")
                            .h(px(40.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .rounded(px(r_btn().min(18.)))
                            .cursor_pointer()
                            .hover(|el| el.bg(hover_bg()))
                            .tooltip(crate::hover::tip("Tu perfil"))
                            .on_click(cx.listener(|view, event: &ClickEvent, window, cx| view.toggle_profile(event.position(), window, cx)))
                            .child(self.profile_avatar("side-avatar", px(26.), cx))
                            .child(div().flex_1().min_w(px(0.)).truncate().child(self.profile_name())),
                    )
                    .child(settings_row),
            )
    }

    fn chat_rows(&self, workspace: u64, indent: f32, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut rows = Vec::new();
        let row = |id: SharedString, label: String, on: bool| {
            div()
                .id(id)
                .ml(px(indent))
                .h(px(30.))
                .px(px(10.))
                .flex()
                .items_center()
                .gap(px(8.))
                .rounded(px(7.))
                .cursor_pointer()
                .text_size(px(13.))
                .when(on, |el| el.bg(selected_bg()).text_color(fg()))
                .when(!on, |el| el.text_color(fg()).hover(|el| el.bg(hover_bg())))
                .child(div().flex_1().min_w(px(0.)).truncate().child(label))
        };
        let open: Vec<&super::Chat> = self.chats.iter().filter(|c| c.workspace == workspace).collect();
        for chat in &open {
            let key = chat.key.clone();
            let close_key = key.clone();
            let on = self.active.as_deref() == Some(chat.key.as_str());
            let dot = if !chat.permissions.is_empty() {
                Some(amber())
            } else if chat.busy {
                Some(accent())
            } else {
                None
            };
            let row_id = SharedString::from(format!("chat-{key}"));
            let select = cx.listener(move |view, _: &ClickEvent, window, cx| view.select_chat(key.clone(), window, cx));
            let close_id = SharedString::from(format!("chat-close-{close_key}"));
            let close = cx.listener(move |view, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                view.close_chat(&close_key, cx);
            });
            let dot = dot.map(|dot| div().size(px(7.)).flex_none().rounded_full().bg(dot));
            if expressive() {
                let trailing = div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .children(dot)
                    .child(div().invisible().group_hover("chat-row", |el| el.visible()).child(
                        gpui_m3::IconButton::new(close_id, "x").size(px(22.)).tooltip("Cerrar").on_click(close),
                    ));
                let drag = super::split::ChatDrag { key: Some(chat.key.clone()), session: None, workspace, title: chat.title.clone().into() };
                let wrap = div().id(SharedString::from(format!("drag-chat-{}", chat.key)));
                rows.push(
                    self.chat_drag(wrap, drag, cx)
                        .ml(px(indent))
                        .child(
                            gpui_m3::NavItem::new(row_id, chat.title.clone())
                                .group("chat-row")
                                .dense(true)
                                .selected(on)
                                .trailing(trailing)
                                .on_click(select),
                        )
                        .into_any_element(),
                );
                continue;
            }
            // Clic derecho: Abrir, Renombrar, Marcador y Eliminar (solo con la sesión ya creada).
            let target = chat.session_id.clone().map(|session_id| super::sidebar::SessionRef { workspace, session_id, title: chat.title.clone() });
            if target.as_ref().is_some_and(|t| self.renaming.as_ref().is_some_and(|r| !self.rename_in_header && r.session_id == t.session_id)) {
                rows.push(div().ml(px(indent)).child(self.rename_field.clone()).into_any_element());
                continue;
            }
            let drag = super::split::ChatDrag { key: Some(chat.key.clone()), session: None, workspace, title: chat.title.clone().into() };
            rows.push(
                self.chat_drag(row(row_id, chat.title.clone(), on), drag, cx)
                    .group("chat-row")
                    .on_click(select)
                    .when_some(target, |el, target| {
                        el.on_mouse_down(
                            gpui::MouseButton::Right,
                            cx.listener(move |view, event: &gpui::MouseDownEvent, _, cx| {
                                view.session_menu = Some((target.clone(), event.position));
                                cx.notify();
                            }),
                        )
                    })
                    .children(dot)
                    .child(
                        div().invisible().group_hover("chat-row", |el| el.visible()).child(
                            div()
                                .id(close_id)
                                .size(px(18.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(5.))
                                .hover(|el| el.bg(hover_bg()))
                                .tooltip(crate::hover::tip("Cerrar"))
                                .on_click(close)
                                .child(svg().path("icons/x.svg").size(px(11.)).text_color(muted())),
                        ),
                    )
                    .into_any_element(),
            );
        }
        let history: Vec<&SessionInfo> = self
            .history
            .get(&workspace)
            .into_iter()
            .flatten()
            .filter(|s| !open.iter().any(|c| c.session_id.as_deref() == Some(s.session_id.as_str())))
            .take(HISTORY_SHOWN)
            .collect();
        if open.is_empty() && history.is_empty() {
            rows.push(
                div().ml(px(indent + 10.)).h(px(28.)).flex().items_center().text_size(px(13.)).text_color(faint()).child("Sin conversaciones").into_any_element(),
            );
        }
        for info in history {
            let session = info.clone();
            let id = SharedString::from(format!("hist-{}", info.session_id));
            let on_open = cx.listener(move |view, _: &ClickEvent, window, cx| view.open_session(workspace, session.clone(), window, cx));
            let drag = super::split::ChatDrag { key: None, session: Some(info.clone()), workspace, title: info.title.clone().into() };
            rows.push(if expressive() {
                self.chat_drag(div().id(SharedString::from(format!("drag-hist-{}", info.session_id))), drag, cx)
                    .ml(px(indent))
                    .child(gpui_m3::NavItem::new(id, info.title.clone()).dense(true).on_click(on_open))
                    .into_any_element()
            } else {
                let target = super::sidebar::SessionRef { workspace, session_id: info.session_id.clone(), title: info.title.clone() };
                if self.renaming.as_ref().is_some_and(|r| !self.rename_in_header && r.session_id == target.session_id) {
                    div().ml(px(indent)).child(self.rename_field.clone()).into_any_element()
                } else {
                    self.chat_drag(row(id, info.title.clone(), false), drag, cx)
                        .text_color(muted())
                        .on_click(on_open)
                        .on_mouse_down(
                            gpui::MouseButton::Right,
                            cx.listener(move |view, event: &gpui::MouseDownEvent, _, cx| {
                                view.session_menu = Some((target.clone(), event.position));
                                cx.notify();
                            }),
                        )
                        .into_any_element()
                }
            });
        }
        rows
    }

    // --- Centro: encabezado, chat y caja de texto ---------------------------------------

    fn center(&self, window: &mut Window, maximized: bool, controls: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let chat = self.active_chat();
        let has_workspace = self.workspaces.active().is_some();
        let loose = chat.is_some_and(|c| c.workspace == LOOSE);
        let title = if expressive() && self.history_page {
            "Historial".into()
        } else {
            chat.map(|c| c.title.clone()).unwrap_or_else(|| "Nueva conversación".into())
        };
        let project = if loose { Some("Chat sin proyecto".to_string()) } else { self.workspaces.active().map(|w| w.name.clone()) };
        let changed = self.changed_files();
        let header_tab = |id: &'static str, icon: &'static str, label: &'static str, on: bool| {
            div()
                .id(id)
                .h(px(34.))
                .px(px(12.))
                .flex()
                .items_center()
                .gap(px(7.))
                .rounded(px(r_btn().min(17.)))
                .cursor_pointer()
                .when(on, |el| el.bg(accent_soft()).text_color(on_accent_soft()))
                .when(!on, |el| el.text_color(fg()).hover(|el| el.bg(hover_bg())))
                .child(svg().path(icon).size(px(15.)).text_color(if on { on_accent_soft() } else { muted() }))
                .child(label)
        };
        let glass = t().style == Style::Glass;
        // Con la conversación empezada, el título se renombra con un clic (como en la referencia).
        let renamable = !self.history_page && chat.is_some_and(|c| c.session_id.is_some() && !c.items.is_empty());
        let renaming_here = self.rename_in_header && self.renaming.is_some();
        let title_el: AnyElement = if renaming_here {
            div().w(px(320.)).child(self.rename_field.clone()).into_any_element()
        } else if renamable {
            div()
                .id("header-title")
                .group("header-title")
                .max_w(px(420.))
                .flex()
                .items_center()
                .gap(px(6.))
                .cursor_pointer()
                .tooltip(crate::hover::tip("Renombrar conversación"))
                .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.start_header_rename(window, cx)))
                .child(div().min_w(px(0.)).truncate().font_weight(FontWeight::SEMIBOLD).child(title))
                .child(
                    div()
                        .invisible()
                        .group_hover("header-title", |el| el.visible())
                        .child(gpui_m3::Icon::new("pen").size(px(12.)).color(faint())),
                )
                .into_any_element()
        } else {
            div().max_w(px(420.)).truncate().font_weight(FontWeight::SEMIBOLD).child(title).into_any_element()
        };
        let header = div()
            .h(px(HEAD_H))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(14.))
            .pl(px(22.))
            .child(title_el)
            .when_some(project, |el, project| el.child(div().flex_none().text_color(faint()).child(project)))
            .child(chrome::drag(HEAD_H))
            .child(self.terminal_button(cx))
            .when(has_workspace && !loose, |el| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .mr(px(if controls { 4. } else { 14. }))
                        // En vidrio, las pestañas van juntas en una cápsula.
                        .when(glass, |el| el.p(px(3.)).rounded(px(20.)).border_1().border_color(line()).bg(t().control))
                        .when(expressive(), |el| {
                            let mut changes = gpui_m3::Button::new("tab-changes", "Cambios")
                                .icon("branch")
                                .text()
                                .size(gpui_m3::ButtonSize::Small)
                                .selected(self.side == Some(Side::Changes))
                                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.toggle_side(Side::Changes, cx)));
                            if changed > 0 {
                                changes = changes.trailing(gpui_m3::Badge::new(changed.to_string()));
                            }
                            el.child(changes).child(
                                gpui_m3::Button::new("tab-files", "Archivos")
                                    .icon("folder")
                                    .text()
                                    .size(gpui_m3::ButtonSize::Small)
                                    .selected(self.side == Some(Side::Files))
                                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.toggle_side(Side::Files, cx))),
                            )
                        })
                        .when(!expressive(), |el| el.child(
                            header_tab("tab-changes", "icons/git-branch.svg", "Cambios", self.side == Some(Side::Changes))
                                .when(changed > 0, |el| {
                                    el.child(
                                        div()
                                            .min_w(px(20.))
                                            .h(px(20.))
                                            .px(px(6.))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded_full()
                                            .bg(accent())
                                            .text_size(px(11.))
                                            .text_color(on_accent())
                                            .child(changed.to_string()),
                                    )
                                })
                                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.toggle_side(Side::Changes, cx))),
                        )
                        .child(
                            header_tab("tab-files", "icons/folder.svg", "Archivos", self.side == Some(Side::Files))
                                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.toggle_side(Side::Files, cx))),
                        )),
                )
            })
            .when(controls, |el| el.child(chrome::controls_colored(maximized, HEAD_H, fg(), hover_bg())));

        // En Expressive el texto usa todo el ancho, como en la referencia; solo la caja se limita.
        // Al cambiar de conversación, la otra entra subiendo 14 px con resorte (`animateIn(.., "swap")`,
        // `Thread.tsx:227`); no se anima al abrir la aplicación.
        let swap = Enter::new("thread-swap").from(0., 14.).on_change(chat.map_or(0, |c| key_hash(&c.key)));
        let mut thread = if expressive() { swap.apply(div(), window, cx) } else { div() }
            .w_full()
            .when(!expressive(), |el| el.max_w(px(THREAD_W)).mx_auto())
            .px(px(32.))
            .pt(px(if expressive() { 20. } else { 12. }))
            .pb(px(if expressive() { 24. } else { 28. }))
            .flex()
            .flex_col()
            .gap(px(if expressive() { 12. } else { 16. }));
        match chat {
            Some(chat) => {
                self.flag_bounds.borrow_mut().clear();
                if loose && chat.items.is_empty() {
                    thread = thread.child(
                        div()
                            .pt(px(140.))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(14.))
                            .child(svg().path("icons/agents/claude.svg").size(px(40.)).text_color(faint()))
                            .child(div().text_size(px(15.)).text_color(muted()).child("Chat sin proyecto: una conversación general, fuera de tus carpetas.")),
                    );
                }
                // Las partes nuevas entran con resorte: solo las últimas tres, para que un
                // historial recién abierto no se anime entero (`motion.ts` de la referencia).
                let total = chat.items.len();
                for (index, item) in chat.items.iter().enumerate() {
                    if chat.hidden(index) {
                        continue;
                    }
                    let flagged = self.is_flagged(chat, index);
                    let fresh = index + ENTER_MAX >= total;
                    thread = thread.child(self.item(chat, index, item, flagged, fresh, cx));
                }
                // Se eligió otro modelo con la conversación empezada: se grabará con el próximo mensaje.
                if let Some(pending) = chat.pending_model(&self.models, &self.configs.get(chat.workspace)) {
                    thread = thread.child(model_mark(format!("model-next-{}", chat.key), format!("{pending} en el próximo mensaje"), true));
                }
                // Como en la referencia, las solicitudes van al final del hilo, todas.
                if !chat.permissions.is_empty() {
                    thread = thread.child(self.permission_cards(chat, cx));
                }
                if chat.working() {
                    thread = thread.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(13.))
                            .text_color(muted())
                            .map(|el| {
                                if expressive() {
                                    el.text_size(px(12.5)).child(gpui_m3::LoadingIndicator::new().size(px(16.))).child("Trabajando…")
                                } else {
                                    el.child(div().size(px(7.)).rounded_full().bg(accent())).child("Claude está trabajando…")
                                }
                            }),
                    );
                }
            }
            None => {
                let text = if has_workspace {
                    "¿En qué trabajamos? Claude Code tiene acceso a las carpetas de este proyecto."
                } else {
                    "Crea un proyecto con el botón de carpeta, junto a «Proyectos»."
                };
                thread = thread.child(
                    div()
                        .pt(px(140.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(14.))
                        .child(svg().path("icons/agents/claude.svg").size(px(40.)).text_color(faint()))
                        .child(div().text_size(px(15.)).text_color(muted()).child(text)),
                );
            }
        }
        let error = self.error.clone();
        let t = t();
        // Sin conversación, Expressive muestra el inicio de la referencia con la caja al centro.
        let history = expressive() && self.history_page;
        // Un chat suelto vacío también muestra el inicio, como en la referencia.
        let hero = expressive() && !history && chat.is_none_or(|c| c.workspace == LOOSE && c.items.is_empty() && c.session_id.is_none());
        div()
            .flex_1()
            .min_w(px(0.))
            .flex()
            .flex_col()
            .bg(center_bg())
            .when(t.gap > 0., |el| {
                el.rounded(px(t.r_pane)).overflow_hidden().when(t.style == Style::Glass, |el| el.border_1().border_color(t.highlight.opacity(0.35)))
            })
            .child(header)
            .when(hero, |el| el.child(self.hero(has_workspace, window, cx)))
            .when(history, |el| el.child(self.history_view(window, cx)))
            .when(!hero && !history, |el| el.child(
                div()
                    .id("code-thread")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scroll()
                    .track_scroll(&self.thread)
                    .on_scroll_wheel(cx.listener(|view, event: &ScrollWheelEvent, window, cx| {
                        let delta = event.delta.pixel_delta(window.line_height()).y;
                        if delta > px(0.) {
                            view.follow = false;
                        } else {
                            let offset = view.thread.offset().y;
                            let max = view.thread.max_offset().height;
                            view.follow = -offset >= max - px(24.);
                        }
                        cx.notify();
                    }))
                    .child(thread),
            ))
            .when_some(error, |el, error| {
                el.child(
                    div().w_full().max_w(px(THREAD_W)).mx_auto().px(px(32.)).child(
                        div()
                            .id("code-error")
                            .mb(px(8.))
                            .p(px(12.))
                            .rounded(px(r_card()))
                            .bg(removed_bg())
                            .text_color(red())
                            .text_size(px(13.))
                            .cursor_pointer()
                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                view.error = None;
                                cx.notify();
                            }))
                            .child(format!("{error}  ·  clic para cerrar")),
                    ),
                )
            })
            .when((has_workspace || loose) && !hero && !history, |el| el.child(self.composer_box(chat.is_some_and(|c| c.busy), false, window, cx)))
            // La terminal va bajo el chat, como en la referencia (`Chat.tsx`).
            .child(self.terminals.clone())
    }

    /// El chat, con la conversación de al lado si hay una (`split.rs`) y, mientras se
    /// arrastra una de la barra, las dos mitades donde soltarla.
    fn chat_area(&mut self, window: &mut Window, maximized: bool, controls: bool, cx: &mut Context<Self>) -> impl IntoElement {
        if self.dragging_chat && !cx.has_active_drag() {
            self.dragging_chat = false;
        }
        let gap = t().gap.max(6.);
        let split = self.split_chat().map(|c| c.key.clone());
        let mut area = div().id("chat-area").relative().flex_1().min_w(px(0.)).flex();
        area = match split {
            None => area.child(self.center(window, maximized, controls, cx)),
            // Los botones de la ventana van en el panel de la derecha.
            Some(key) if self.split_left => {
                area.child(self.split_pane(&key, false, false, window, cx)).child(div().w(px(gap)).flex_none()).child(self.center(window, maximized, controls, cx))
            }
            Some(key) => area.child(self.center(window, false, false, cx)).child(div().w(px(gap)).flex_none()).child(self.split_pane(&key, controls, maximized, window, cx)),
        };
        if self.dragging_chat {
            let half = |left: bool, cx: &mut Context<Self>| {
                let label = if left { "Abrir a la izquierda" } else { "Abrir a la derecha" };
                div()
                    .id(if left { "drop-left" } else { "drop-right" })
                    .flex_1()
                    .h_full()
                    .p(px(10.))
                    .child(
                        div()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(t().r_pane.max(12.)))
                            .border_2()
                            .border_color(accent().opacity(0.35))
                            .text_color(accent())
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(label),
                    )
                    .drag_over::<super::split::ChatDrag>(|style, _, _, _| style.bg(accent().opacity(0.10)))
                    .on_drop(cx.listener(move |view, drag: &super::split::ChatDrag, window, cx| view.open_split(drag, left, window, cx)))
            };
            area = area.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .rounded(px(t().r_pane))
                    .child(half(true, cx))
                    .child(half(false, cx)),
            );
        }
        area
    }

    /// La conversación de al lado: su título, el hilo entero y una caja que, con
    /// un clic, la vuelve la activa.
    fn split_pane(&self, key: &str, controls: bool, maximized: bool, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = t();
        let Some(chat) = self.chats.iter().find(|c| c.key == key) else {
            return div().into_any_element();
        };
        let header = div()
            .h(px(HEAD_H))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.))
            .pl(px(22.))
            .child(div().max_w(px(320.)).truncate().font_weight(FontWeight::SEMIBOLD).text_color(muted()).child(chat.title.clone()))
            .child(chrome::drag(HEAD_H))
            .child(
                div()
                    .mr(px(if controls { 4. } else { 14. }))
                    .child(icon_action("split-close", "icons/x.svg", "Cerrar este lado", cx.listener(|view, _: &ClickEvent, _, cx| view.close_split(cx)))),
            )
            .when(controls, |el| el.child(chrome::controls_colored(maximized, HEAD_H, fg(), hover_bg())));
        let mut thread = div().w_full().px(px(32.)).pt(px(16.)).pb(px(20.)).flex().flex_col().gap(px(if expressive() { 12. } else { 16. }));
        let total = chat.items.len();
        for (index, item) in chat.items.iter().enumerate() {
            if chat.hidden(index) {
                continue;
            }
            let flagged = self.is_flagged(chat, index);
            thread = thread.child(self.item(chat, index, item, flagged, index + ENTER_MAX >= total, cx));
        }
        if !chat.permissions.is_empty() {
            thread = thread.child(self.permission_cards(chat, cx));
        }
        if chat.working() {
            thread = thread.child(
                div().flex().items_center().gap(px(8.)).text_size(px(12.5)).text_color(muted()).child(gpui_m3::LoadingIndicator::new().size(px(16.))).child("Trabajando…"),
            );
        }
        let _ = window;
        // La caja de la otra: un clic la vuelve la activa y la caja de verdad pasa aquí.
        let reply = div()
            .id("split-reply")
            .mx(px(16.))
            .mb(px(14.))
            .h(px(52.))
            .px(px(18.))
            .flex()
            .flex_none()
            .items_center()
            .rounded(px(if expressive() { 26. } else { r_card() }))
            .bg(t.control)
            .text_color(faint())
            .cursor_text()
            .hover(|el| el.bg(t.control2))
            .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.focus_split(window, cx)))
            .child(if chat.busy { "Claude está trabajando… · clic para escribir aquí" } else { "Responde a Claude… · clic para escribir aquí" });
        div()
            .flex_1()
            .min_w(px(0.))
            .flex()
            .flex_col()
            .bg(center_bg())
            .when(t.gap > 0., |el| el.rounded(px(t.r_pane)).overflow_hidden().when(t.style == Style::Glass, |el| el.border_1().border_color(t.highlight.opacity(0.35))))
            .child(header)
            .child(div().id("split-thread").flex_1().min_h(px(0.)).overflow_y_scroll().track_scroll(&self.split_thread).child(thread))
            .child(reply)
            .into_any_element()
    }

    /// «Terminal» en la barra superior, con cuántas hay si son varias (`Chat.tsx:82`).
    fn terminal_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let terminals = self.terminals.read(cx);
        let (on, count) = (terminals.shown(), terminals.count());
        if expressive() {
            let mut button = gpui_m3::Button::new("tab-terminal", "Terminal")
                .icon("terminal")
                .text()
                .size(gpui_m3::ButtonSize::Small)
                .selected(on)
                .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.toggle_terminal(window, cx)));
            if count > 1 {
                button = button.trailing(gpui_m3::Badge::new(count.to_string()));
            }
            return button.into_any_element();
        }
        div()
            .id("tab-terminal")
            .h(px(34.))
            .px(px(12.))
            .flex()
            .items_center()
            .gap(px(7.))
            .rounded(px(r_btn().min(17.)))
            .cursor_pointer()
            .tooltip(crate::hover::tip("Terminal (Ctrl+J)"))
            .when(on, |el| el.bg(accent_soft()).text_color(on_accent_soft()))
            .when(!on, |el| el.text_color(fg()).hover(|el| el.bg(hover_bg())))
            .child(svg().path("icons/square-terminal.svg").size(px(15.)).text_color(if on { on_accent_soft() } else { muted() }))
            .child("Terminal")
            .when(count > 1, |el| el.child(div().text_size(px(11.)).text_color(faint()).child(count.to_string())))
            .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.toggle_terminal(window, cx)))
            .into_any_element()
    }

    /// El inicio de Expressive: las formas, «¿Qué construimos hoy?», el proyecto,
    /// la caja de texto y las sugerencias.
    fn hero(&self, has_workspace: bool, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_m3::{Icon, Shape, ShapeName};
        let scheme = gpui_m3::Theme::of(cx).clone();
        // Al pasar el cursor por el grupo, cada blob se vuelve una forma M3 y regresa.
        let hovered = self.hero_hover;
        let shapes = div()
            .id("hero-shapes-hover")
            .on_hover(cx.listener(|view, on: &bool, _, cx| {
                view.hero_hover = *on;
                cx.notify();
            }))
            .h(px(84.))
            .mb(px(4.))
            .flex()
            .justify_center()
            .child(
                div().h_full().flex().items_end().pb(px(6.)).mr(px(-8.)).child(
                    Shape::new(ShapeName::Pebble).size(px(30.)).color(scheme.secondary_container).breathe(ShapeName::Blob2, 5.).spin(18.).breathe_on_hover("hero-shape-a", ShapeName::Sunny).hovered(hovered),
                ),
            )
            .child(
                div().h_full().flex().items_center().child(
                    // Formas blandas, sin lóbulos ni puntas: respiran de un blob a otro.
                    Shape::new(ShapeName::Blob)
                        .size(px(68.))
                        .color(scheme.primary_container)
                        .breathe(ShapeName::Blob2, 7.)
                        .breathe_on_hover("hero-shape-b", ShapeName::Cookie9)
                        .hovered(hovered)
                        .spin(40.)
                        .child(Icon::new("spark").size(px(28.)).color(scheme.on_primary_container)),
                ),
            )
            .child(
                div().h_full().flex().items_start().pt(px(4.)).ml(px(-10.)).child(
                    Shape::new(ShapeName::Blob2).size(px(40.)).color(scheme.tertiary_container).breathe(ShapeName::Pebble, 9.).spin(-26.).breathe_on_hover("hero-shape-c", ShapeName::Clover4).hovered(hovered),
                ),
            );
        let title = div()
            .mb(px(18.))
            .text_center()
            .text_size(px(32.))
            .line_height(px(40.))
            .font_weight(FontWeight(620.))
            .child("¿Qué construimos hoy?");
        let loose = self.in_loose_chat();
        let project = if loose {
            "Sin proyecto".to_string()
        } else {
            self.workspaces.active().map(|w| w.name.clone()).unwrap_or_else(|| "Elegir proyecto".into())
        };
        let picker = div()
            .id("hero-project")
            .h(px(32.))
            .pl(px(8.))
            .pr(px(10.))
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded(px(8.))
            .bg(scheme.primary_container)
            .text_color(scheme.on_primary_container)
            .text_size(px(13.))
            .font_weight(FontWeight::SEMIBOLD)
            .cursor_pointer()
            .on_click(cx.listener(|view, event: &ClickEvent, _, cx| view.toggle_menu(Menu::Project, event.position(), cx)))
            .child(Icon::new(if loose { "chat" } else { "folder" }).size(px(14.)).color(scheme.on_primary_container))
            .child(project)
            .child(Icon::new("chevron-down").size(px(12.)).color(scheme.on_primary_container));
        let mut suggestions = div().mt(px(16.)).flex().flex_wrap().justify_center().gap(px(8.));
        for (index, (icon, label, prompt)) in SUGGESTIONS.iter().enumerate() {
            suggestions = suggestions.child(
                gpui_m3::Chip::new(("suggest", index), *label)
                    .icon(*icon)
                    .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| view.prefill(prompt, window, cx))),
            );
        }
        div()
            .id("code-hero")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .px(px(24.))
            .flex()
            .flex_col()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(HERO_W))
                    .mx_auto()
                    .pb(px(96.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    // Los hijos suben uno tras otro con resorte (`m3-rise`, `motion.css:391`); el
                    // título no (en la referencia anima su peso y su ancho aparte).
                    .child(rise("hero-shapes", 0., div().child(shapes), window, cx))
                    .child(title)
                    .child(rise("hero-picker", 0.06, div().flex().child(picker), window, cx))
                    .when(has_workspace || loose, |el| {
                        let composer = self.composer_box(false, true, window, cx);
                        el.child(rise("hero-composer", 0.12, composer, window, cx))
                    })
                    .when(has_workspace && !loose, |el| el.child(rise("hero-suggestions", 0.18, suggestions, window, cx))),
            )
    }


    /// Una fila marcada lleva otro fondo (y destella al saltar a ella); se
    /// anota dónde quedó para «Siguiente mensaje marcado».
    fn flag_frame(&self, index: usize, flagged: bool, el: AnyElement) -> AnyElement {
        let flash = self.flash == Some(index);
        if !flagged && !flash {
            return el;
        }
        let bounds = self.flag_bounds.clone();
        div()
            .relative()
            .mx(px(-8.))
            .px(px(8.))
            .py(px(6.))
            .rounded(px(r_card()))
            .bg(if flash { accent().opacity(0.22) } else { accent_soft().opacity(0.5) })
            .child(
                canvas(move |b, _, _| {
                    bounds.borrow_mut().insert(index, b);
                }, |_, _, _, _| {})
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .child(el)
            .into_any_element()
    }

    /// El botón «Marcar» bajo un mensaje: se ve al pasar el cursor, o siempre si está marcado.
    fn flag_button(&self, index: usize, flagged: bool, cx: &mut Context<Self>) -> AnyElement {
        let toggle = cx.listener(move |view, _: &ClickEvent, _, cx| view.toggle_flag(index, cx));
        let label = if flagged { "Marcado" } else { "Marcar" };
        if expressive() {
            return gpui_m3::Button::new(("flag", index), label)
                .variant(gpui_m3::ButtonVariant::Ghost)
                .size(gpui_m3::ButtonSize::Tiny)
                .icon("bookmark")
                .selected(flagged)
                .label_on_hover(true)
                .on_click(toggle)
                .into_any_element();
        }
        div()
            .id(("flag", index))
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(6.))
            .h(px(24.))
            .rounded(px(6.))
            .cursor_pointer()
            .text_size(px(12.))
            .text_color(if flagged { accent() } else { muted() })
            .hover(|el| el.bg(hover_bg()))
            .on_click(toggle)
            .child(gpui_m3::Icon::new("bookmark").size(px(12.)).color(if flagged { accent() } else { muted() }))
            .child(label)
            .into_any_element()
    }

    fn item(&self, chat: &super::Chat, index: usize, item: &Item, flagged: bool, fresh: bool, cx: &mut Context<Self>) -> AnyElement {
        let key = chat.key.as_str();
        let row = match item {
            Item::User { .. } | Item::Text(_) => self.message(key, index, item, flagged, fresh, cx),
            Item::Compact => compact_mark(format!("compact-{key}-{index}")),
            _ => self.other_item(chat, index, item, fresh, cx),
        };
        self.flag_frame(index, flagged, row)
    }

    /// Un mensaje del usuario o una respuesta, con sus acciones (copiar y marcar).
    fn message(&self, key: &str, index: usize, item: &Item, flagged: bool, fresh: bool, cx: &mut Context<Self>) -> AnyElement {
        let group = SharedString::from(format!("msg-{index}"));
        let flag = self.flag_button(index, flagged, cx);
        let actions = |el: Div| {
            el.when(!flagged, |el| el.invisible().group_hover(group.clone(), |el| el.visible()))
        };
        // Las imágenes que llevó el mensaje, como miniaturas sobre su texto.
        let thumbs = |images: &[std::sync::Arc<gpui::Image>]| {
            (!images.is_empty()).then(|| {
                div().flex().flex_wrap().gap(px(6.)).children(images.iter().enumerate().map(|(n, image)| {
                    gpui_m3::ImageThumb::new(SharedString::from(format!("thumb-{key}-{index}-{n}")), image.clone()).size(px(64.))
                }))
            })
        };
        match item {
            Item::User { text, images, .. } if expressive() => {
                let copy = text.clone();
                div()
                    .group(group.clone())
                    .flex()
                    .flex_col()
                    .items_end()
                    .child(
                        gpui_m3::Bubble::custom(gpui_m3::BubbleKind::User)
                            .when(fresh, |b| b.entrance(SharedString::from(format!("bubble-{key}-{index}")), 0.))
                            .when_some(thumbs(images), |el, row| el.child(row.mb(px(if text.is_empty() { 0. } else { 8. }))))
                            .child(text.clone()),
                    )
                    .child(
                        // Copiar y Marcar bajo el mensaje, a la derecha como la burbuja.
                        div()
                            .mt(px(2.))
                            .mb(px(-4.))
                            .flex()
                            .gap(px(2.))
                            .child(
                                div().invisible().group_hover(group.clone(), |el| el.visible()).child(
                                    gpui_m3::Button::new(("copy", index), "Copiar")
                                        .variant(gpui_m3::ButtonVariant::Ghost)
                                        .size(gpui_m3::ButtonSize::Tiny)
                                        .icon("copy")
                                        .label_on_hover(true)
                                        .confirm("Copiado", std::time::Duration::from_millis(1400))
                                        .on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))),
                                ),
                            )
                            .child(actions(div().child(flag))),
                    )
                    .into_any_element()
            }
            Item::User { text, images, .. } => {
                let copy = text.clone();
                let copy_button = div()
                    .id(("copy", index))
                    .invisible()
                    .group_hover(group.clone(), |el| el.visible())
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .px(px(6.))
                    .h(px(24.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .text_size(px(12.))
                    .text_color(muted())
                    .hover(|el| el.bg(hover_bg()))
                    .on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(copy.clone())))
                    .child(svg().path("icons/copy.svg").size(px(12.)).text_color(muted()))
                    .child("Copiar");
                div()
                .group(group.clone())
                .relative()
                .flex()
                .justify_end()
                .child(
                    div()
                        .max_w(px(THREAD_W * 0.78))
                        .px(px(16.))
                        .py(px(11.))
                        .rounded(px(14.))
                        .bg(accent_soft())
                        .border_1()
                        .border_color(accent_line())
                        .line_height(px(22.))
                        .when_some(thumbs(images), |el, row| el.child(row.mb(px(8.))))
                        .child(text.clone()),
                )
                .child(div().absolute().right(px(-6.)).bottom(px(-24.)).flex().gap(px(2.)).child(copy_button).child(actions(div().child(flag))))
                .into_any_element()
            }
            Item::Text(text) if expressive() => {
                let copy = text.clone();
                div()
                    .group(group.clone())
                    .flex()
                    .flex_col()
                    .child(markdown(&format!("{key}-{index}"), text, cx))
                    .child(
                        div()
                            .ml(px(-6.))
                            .mt(px(2.))
                            .mb(px(-4.))
                            .flex()
                            .gap(px(2.))
                            .child(
                                div().invisible().group_hover(group.clone(), |el| el.visible()).child(
                                    gpui_m3::Button::new(("copy", index), "Copiar")
                                        .variant(gpui_m3::ButtonVariant::Ghost)
                                        .size(gpui_m3::ButtonSize::Tiny)
                                        .icon("copy")
                                        .label_on_hover(true)
                                        .confirm("Copiado", std::time::Duration::from_millis(1400))
                                        .on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))),
                                ),
                            )
                            .child(actions(div().child(flag))),
                    )
                    .into_any_element()
            }
            Item::Text(text) => {
                let copy = text.clone();
                div()
                    .group(group.clone())
                    .relative()
                    .child(markdown(&format!("{key}-{index}"), text, cx))
                    .child(
                        // Flota bajo la respuesta: no deja un hueco cuando no se ve.
                        div()
                            .absolute()
                            .left(px(-6.))
                            .bottom(px(-24.))
                            .flex()
                            .gap(px(2.))
                            .child(
                                div()
                                    .id(("copy", index))
                                    .invisible()
                                    .group_hover(group.clone(), |el| el.visible())
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .px(px(6.))
                                    .h(px(24.))
                                    .rounded(px(6.))
                                    .cursor_pointer()
                                    .text_size(px(12.))
                                    .text_color(muted())
                                    .hover(|el| el.bg(hover_bg()))
                                    .on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(copy.clone())))
                                    .child(svg().path("icons/copy.svg").size(px(12.)).text_color(muted()))
                                    .child("Copiar"),
                            )
                            .child(actions(div().child(flag))),
                    )
                    .into_any_element()
            }
            _ => div().into_any_element(),
        }
    }

    fn other_item(&self, chat: &super::Chat, index: usize, item: &Item, fresh: bool, cx: &mut Context<Self>) -> AnyElement {
        let key = chat.key.as_str();
        match item {
            // Los avisos entran después de lo que los causó, con un retardo de 0,12 s (como en la referencia).
            Item::Notice { text, error } if expressive() => {
                let kind = if *error { gpui_m3::BubbleKind::Error } else { gpui_m3::BubbleKind::Notice };
                gpui_m3::Bubble::new(kind, text.clone())
                    .when(fresh, |b| b.entrance(SharedString::from(format!("notice-{key}-{index}")), 0.12))
                    .into_any_element()
            }
            Item::Thinking(text) if expressive() => {
                let live = chat.is_streaming(index);
                let id = format!("think-{key}-{index}");
                let open = self.expanded.contains(&id);
                let toggle = id.clone();
                let flip = cx.listener(move |view, _: &bool, _, cx| {
                    if !view.expanded.remove(&toggle) {
                        view.expanded.insert(toggle.clone());
                    }
                    cx.notify();
                });
                gpui_m3::ExpandableCard::new(SharedString::from(id))
                    .plain(true)
                    .open(open)
                    .header(div().text_size(px(12.5)).text_color(muted()).child(if live { "Razonando…" } else { "Razonamiento" }))
                    // En vivo y cerrado, la última línea de lo que va pensando.
                    .when(live, |card| card.preview(div().text_size(px(12.5)).text_color(faint()).child(last_line(text))))
                    .on_toggle(move |open, window, cx| flip(&open, window, cx))
                    .child(div().opacity(0.75).child(markdown(&format!("{key}-{index}-think"), text, cx)))
                    .into_any_element()
            }
            Item::Tool(tool) if expressive() => self.tool_m3(tool, fresh, cx),
            Item::Thinking(text) => {
                let live = chat.is_streaming(index);
                let id = format!("think-{key}-{index}");
                let open = self.expanded.contains(&id);
                let toggle = id.clone();
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .id(SharedString::from(id))
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .cursor_pointer()
                            .text_size(px(13.))
                            .text_color(muted())
                            .hover(|el| el.text_color(fg()))
                            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                if !view.expanded.remove(&toggle) {
                                    view.expanded.insert(toggle.clone());
                                }
                                cx.notify();
                            }))
                            .child(
                                svg()
                                    .path(if open { "icons/chevron-down.svg" } else { "icons/chevron-right.svg" })
                                    .size(px(13.))
                                    .text_color(muted()),
                            )
                            .child(if live { "Razonando…" } else { "Razonamiento" })
                            // En vivo y cerrado, la última línea de lo que va pensando (como en la referencia).
                            .when(live && !open, |el| {
                                el.child(div().flex_1().min_w(px(0.)).truncate().text_size(px(12.5)).text_color(faint()).child(last_line(text)))
                            }),
                    )
                    .when(open, |el| {
                        el.child(
                            div()
                                .ml(px(6.))
                                .pl(px(14.))
                                .border_l_2()
                                .border_color(line())
                                .opacity(0.75)
                                .child(markdown(&format!("{key}-{index}-think"), text, cx)),
                        )
                    })
                    .into_any_element()
            }
            Item::Tool(tool) => self.tool(tool, cx),
            Item::Turn(summary) => div()
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(px(12.))
                .text_color(faint())
                .child(svg().path("icons/check.svg").size(px(12.)).text_color(faint()))
                .child(summary.clone())
                .into_any_element(),
            Item::Model(name) => model_mark(format!("model-{key}-{index}"), format!("Cambiado a {name}"), false),
            Item::Notice { text, error } => div()
                .px(px(12.))
                .py(px(9.))
                .rounded(px(9.))
                .bg(if *error { removed_bg() } else { code_bg() })
                .text_size(px(13.))
                .text_color(if *error { red() } else { amber() })
                .child(text.clone())
                .into_any_element(),
            Item::User { .. } | Item::Text(_) | Item::Compact => div().into_any_element(),
        }
    }

    fn tool(&self, tool: &ToolCall, cx: &mut Context<Self>) -> AnyElement {
        let open = self.expanded.contains(&tool.id);
        let (verb, summary) = tool_label(tool);
        let (icon, color) = match (&tool.result, tool.is_error) {
            (None, _) => ("icons/circle-dot.svg", amber()),
            (Some(_), true) => ("icons/circle-x.svg", red()),
            (Some(_), false) => ("icons/check.svg", green()),
        };
        let counts = tool.input.as_ref().and_then(|input| super::edits::edit_stats(&tool.name, input)).map(|s| (s.added, s.removed));
        let todo_list = (tool.name == "TodoWrite").then(|| todos(tool));
        let id = tool.id.clone();
        let mut card = div().rounded(px(r_card())).border_1().border_color(line()).bg(card_bg()).flex().flex_col().overflow_hidden().child(
            div()
                .id(SharedString::from(format!("tool-{}", tool.id)))
                .h(px(40.))
                .px(px(14.))
                .flex()
                .items_center()
                .gap(px(10.))
                .cursor_pointer()
                .hover(|el| el.bg(hover_bg()))
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    if !view.expanded.remove(&id) {
                        view.expanded.insert(id.clone());
                    }
                    cx.notify();
                }))
                .child(svg().path(icon).size(px(14.)).flex_none().text_color(color))
                .child(div().flex_none().font_weight(FontWeight::SEMIBOLD).child(if verb.is_empty() { tool.name.clone() } else { verb.to_string() }))
                .child(
                    div()
                        .flex_shrink()
                        .min_w(px(0.))
                        .truncate()
                        .font_family(mono())
                        .text_size(px(12.5))
                        .text_color(accent())
                        .child(summary),
                )
                .when_some(counts, |el, (added, removed)| {
                    el.child(div().flex_none().font_family(mono()).text_size(px(12.)).text_color(green()).child(format!("+{added}")))
                        .child(div().flex_none().font_family(mono()).text_size(px(12.)).text_color(red()).child(format!("-{removed}")))
                })
                .when_some(todo_list.as_ref(), |el, list| {
                    let done = list.iter().filter(|(_, s)| s == "completed").count();
                    el.child(div().flex_none().text_color(faint()).child(format!("{done}/{}", list.len())))
                })
                .child(div().flex_1())
                .child(
                    svg()
                        .path(if open { "icons/chevron-down.svg" } else { "icons/chevron-right.svg" })
                        .size(px(13.))
                        .text_color(faint()),
                ),
        );
        if let Some(list) = todo_list.filter(|_| !open) {
            card = card.child(todo_rows(&list));
        } else if open {
            card = card.child(
                div()
                    .px(px(12.))
                    .pb(px(12.))
                    .pt(px(2.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(tool_input(tool))
                    .when_some(tool.result.as_ref().filter(|r| tool.name != "TodoWrite" && !r.is_empty()), |el, result| {
                        el.child(super::tools::output(format!("{}-result", tool.id), result, RESULT_LINES, None, tool.is_error))
                    }),
            );
        }
        card.into_any_element()
    }

    fn composer_box(&self, busy: bool, hero: bool, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let t = t();
        let starting = self.active_chat().is_some_and(|c| c.starting);
        let config = self.config();
        // El modelo y el esfuerzo de la conversación visible (cada una tiene los suyos).
        let chat_model = self.chat_model();
        let model = if chat_model.is_empty() {
            self.active_chat().and_then(|c| c.model.clone()).map(|m| self.model_label(&m)).unwrap_or_else(|| "Predeterminado".into())
        } else {
            self.model_label(&chat_model)
        };
        let chat_effort = self.chat_effort();
        let effort = (!chat_effort.is_empty()).then(|| config::effort_label(&chat_effort));
        let mode_color = match config.permission_mode.as_str() {
            "acceptEdits" => accent(),
            "plan" => green(),
            "bypassPermissions" => red(),
            _ => faint(),
        };
        let branch = self.repos.iter().find_map(|r| r.branch.clone());
        let last_turn = self.active_chat().and_then(|c| {
            c.items.iter().rev().find_map(|i| match i {
                Item::Turn(summary) => Some(summary.clone()),
                _ => None,
            })
        });
        let menu_open = |menu: Menu| self.menu.is_some_and(|(open, _)| open == menu);
        let tool_button = |id: &'static str, icon: &'static str, tip: &'static str, on: bool| {
            div()
                .id(id)
                .size(px(32.))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .rounded(px(t.r_btn.min(16.)))
                .cursor_pointer()
                .when(on, |el| el.bg(t.sel))
                .when(!on, |el| el.hover(|el| el.bg(hover_bg())))
                .tooltip(crate::hover::tip(tip))
                .child(svg().path(icon).size(px(16.)).text_color(muted()))
        };
        let mut attachments = div().flex().flex_wrap().items_center().gap(px(6.));
        for (index, attachment) in self.attachments.iter().enumerate() {
            let remove = cx.listener(move |view, _: &ClickEvent, _, cx| view.remove_attachment(index, cx));
            // Las imágenes, con su miniatura y la «×» al pasar el cursor (como en la referencia).
            let name = match attachment {
                Attachment::Image { name, image, .. } => {
                    attachments = attachments.child(
                        gpui_m3::ImageThumb::new(("attachment-thumb", index), image.clone())
                            .remove_label(format!("Quitar {name}"))
                            .on_remove(remove),
                    );
                    continue;
                }
                Attachment::File(path) => file_name(path),
            };
            let chip = file_chip(("attachment", index), name.clone(), remove);
            attachments = attachments.child(chip_in(format!("chip-in-{index}-{name}"), chip, window, cx));
        }
        // El archivo del editor a la vista y sus líneas: irán con el mensaje; la «×» lo quita.
        if let Some(ctx) = self.editor_context(cx) {
            let label = super::editor::chip_label(&file_name(&ctx.path), ctx.lines);
            let remove = cx.listener(|view, _: &ClickEvent, _, cx| view.skip_editor_context(cx));
            let chip = file_chip(("editor-context", 0), label.clone(), remove);
            attachments = attachments.child(chip_in(format!("chip-in-context-{label}"), chip, window, cx));
        }
        let ready = !self.attachments.is_empty() || !self.composer.read(cx).text().trim().is_empty();
        // La caja crece con el texto hasta 240 px, como la de Expressive.
        let input_h = self.composer.read(cx).content_height(3).min(px(240.));
        let has_chips = !self.attachments.is_empty() || self.editor_context(cx).is_some();
        if t.style == Style::Expressive {
            let composer = self.composer_m3(busy, ready, attachments, model, effort, branch, cx);
            return if hero { composer } else { div().w_full().max_w(px(THREAD_W)).mx_auto().px(px(32.)).pb(px(16.)).child(composer) };
        }
        // La caja: en Formal, con borde sobre el chat; en Expressive, tonal sin
        // borde; en vidrio, flotando con su sombra y el borde de luz.
        let bounds = self.composer_bounds.clone();
        let card = div()
            .relative()
            .child(canvas(move |b, _, _| bounds.set(Some(b)), |_, _, _, _| {}).absolute().top_0().left_0().size_full())
            .p(px(14.))
            .pb(px(10.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .map(|el| match t.style {
                Style::Formal | Style::Expressive => el.rounded(px(r_card())).border_1().border_color(line()).bg(t.editor),
                Style::Glass => el.rounded(px(24.)).border_1().border_color(t.highlight.opacity(0.5)).bg(t.raised).shadow(float_shadow()),
            })
            .when(has_chips, |el| el.child(attachments))
            .child(super::mention::with_keys(div().key_context(COMPOSER).on_action(cx.listener(Self::send)).h(input_h), cx).child(self.composer.clone()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        tool_button("composer-attach", "icons/plus.svg", "Adjuntar archivos o imágenes (también Ctrl+V)", false)
                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.pick_attachments(cx))),
                    )
                    .child(
                        tool_button("composer-actions", "icons/square-terminal.svg", "Comandos y acciones (/)", menu_open(Menu::Actions))
                            .on_click(cx.listener(|view, event: &ClickEvent, _, cx| view.toggle_menu(Menu::Actions, event.position(), cx))),
                    )
                    .child(
                        div()
                            .id("composer-model")
                            .h(px(32.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .rounded(px(t.r_btn.min(16.)))
                            .cursor_pointer()
                            .when(menu_open(Menu::Model), |el| el.bg(t.sel))
                            .when(!menu_open(Menu::Model), |el| el.hover(|el| el.bg(hover_bg())))
                            .tooltip(crate::hover::tip("Modelo y esfuerzo"))
                            .on_click(cx.listener(|view, event: &ClickEvent, _, cx| view.toggle_menu(Menu::Model, event.position(), cx)))
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(model))
                            .when_some(effort, |el, effort| el.child(div().text_color(faint()).child(effort)))
                            .child(svg().path("icons/chevron-down.svg").size(px(12.)).text_color(faint())),
                    )
                    .child(div().flex_1())
                    .when(starting, |el| el.child(div().mr(px(8.)).text_size(px(12.)).text_color(faint()).child("Conectando…")))
                    .child(if busy && !ready {
                        div()
                            .id("composer-stop")
                            .size(px(38.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(t.r_btn.min(19.)))
                            .bg(accent())
                            .cursor_pointer()
                            .tooltip(crate::hover::tip("Detener"))
                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.interrupt(cx)))
                            .child(div().size(px(12.)).rounded(px(3.)).bg(on_accent()))
                    } else {
                        div()
                            .id("composer-send")
                            .size(px(38.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(t.r_btn.min(19.)))
                            .when(ready, |el| el.bg(accent()))
                            .when(!ready, |el| el.bg(t.control2))
                            .cursor_pointer()
                            .tooltip(crate::hover::tip("Enviar (Enter)"))
                            .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.send(&super::Send, window, cx)))
                            .child(svg().path("icons/arrow-up.svg").size(px(18.)).text_color(if ready { on_accent() } else { faint() }))
                    })
            );
        div()
            .w_full()
            .max_w(px(THREAD_W))
            .mx_auto()
            .px(px(32.))
            .pb(px(12.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(self.drop_zone("composer-drop", px(if t.style == Style::Glass { 24. } else { r_card() }), card, cx))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(6.))
                    .px(px(4.))
                    .map(|el| self.status_pills(el, branch, mode_color, &config, last_turn, cx)),
            )
    }

    /// La caja de texto de Expressive, como el compositor de la referencia: tonal, crece
    /// con el texto hasta 240 px y debajo lleva la rama, el modo y la duración.
    #[allow(clippy::too_many_arguments)]
    fn composer_m3(
        &self,
        busy: bool,
        ready: bool,
        attachments: Div,
        model: String,
        effort: Option<&'static str>,
        branch: Option<String>,
        cx: &mut Context<Self>,
    ) -> Div {
        use gpui_m3::{Chip, IconButton};
        let t = t();
        let scheme = *gpui_m3::Theme::of(cx);
        let starting = self.active_chat().is_some_and(|c| c.starting);
        let config = self.config();
        let menu_open = |menu: Menu| self.menu.is_some_and(|(open, _)| open == menu);
        let input_h = self.composer.read(cx).content_height(2).min(px(240.));
        let model_open = menu_open(Menu::Model);
        let mut model_button = gpui_m3::Button::new("composer-model", model)
            .size(gpui_m3::ButtonSize::Compact)
            .open(model_open)
            .on_click(cx.listener(|view, event: &ClickEvent, _, cx| view.toggle_menu(Menu::Model, event.position(), cx)));
        if let Some(effort) = effort {
            model_button = model_button.sublabel(effort);
        }
        let action = if busy && !ready {
            IconButton::new("composer-stop", "stop")
                .filled()
                .size(px(36.))
                .tooltip("Detener (Esc)")
                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.interrupt(cx)))
        } else {
            IconButton::new("composer-send", "up")
                .filled()
                .hover_morph(true)
                .size(px(36.))
                .disabled(!ready)
                .launch(self.sends)
                .tooltip("Enviar")
                .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.send(&super::Send, window, cx)))
        };
        let bar = div()
            .flex()
            .items_center()
            .gap(px(2.))
            .pl(px(10.))
            .pr(px(8.))
            .pt(px(4.))
            .pb(px(8.))
            .child(
                IconButton::new("composer-attach", "plus")
                    .size(px(32.))
                    .tooltip("Adjuntar (Ctrl+U)")
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.pick_attachments(cx))),
            )
            .child(
                IconButton::new("composer-actions", "slash")
                    .size(px(32.))
                    .selected(menu_open(Menu::Actions))
                    .tooltip("Acciones (/)")
                    .on_click(cx.listener(|view, event: &ClickEvent, _, cx| view.toggle_menu(Menu::Actions, event.position(), cx))),
            )
            .child(model_button)
            .child(div().flex_1())
            .when(starting, |el| el.child(div().mr(px(6.)).text_size(px(12.)).text_color(t.muted).child("Conectando…")))
            .child(self.context_ring(cx))
            .child(div().ml(px(2.)).child(action));
        let bounds = self.composer_bounds.clone();
        let card = div()
            .relative()
            .flex()
            .flex_col()
            .rounded(px(26.))
            .bg(if self.composer_focused { t.control2 } else { t.control })
            .child(canvas(move |b, _, _| bounds.set(Some(b)), |_, _, _, _| {}).absolute().top_0().left_0().size_full())
            .when(!self.attachments.is_empty() || self.editor_context(cx).is_some(), |el| el.child(attachments.px(px(12.)).pt(px(10.))))
            .child(
                super::mention::with_keys(div().key_context(COMPOSER).on_action(cx.listener(Self::send)), cx)
                    .px(px(20.))
                    .pt(px(15.))
                    .pb(px(2.))
                    .text_size(px(13.5))
                    .line_height(px(20.))
                    .child(div().h(input_h).child(self.composer.clone())),
            )
            .child(bar);

        // El modo: cicla al clic y su punto cambia de forma y color.
        let plan = config.permission_mode == "plan";
        let (shape, color) = match config.permission_mode.as_str() {
            "acceptEdits" => (gpui_m3::DotShape::Circle, scheme.primary),
            "plan" => (gpui_m3::DotShape::Diamond, scheme.tertiary),
            "bypassPermissions" => (gpui_m3::DotShape::Square, scheme.error),
            _ => (gpui_m3::DotShape::Square, scheme.outline),
        };
        let mode = div().id("status-mode-wrap").tooltip(crate::hover::tip("Modo de permisos (clic para cambiar)")).child(
            gpui_m3::Chip::new("status-mode", config.mode_label())
                .tone(if plan { gpui_m3::Tone::Tertiary } else { gpui_m3::Tone::Secondary })
                .leading(gpui_m3::MorphDot::new("mode-dot", shape).size(px(7.)).color(color))
                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.cycle_mode(cx))),
        );
        let duration = self.active_chat().filter(|chat| !chat.items.is_empty()).map(|chat| {
            let minutes = chat.seen_at.elapsed().as_secs() / 60;
            if minutes >= 60 { format!("{}h {}m", minutes / 60, minutes % 60) } else { format!("{minutes}m") }
        });
        let under = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(4.))
            .px(px(4.))
            .pt(px(6.))
            .when_some(branch, |el, branch| el.child(Chip::new("status-branch", branch).icon("branch").muted(true).inert(true)))
            .child(mode)
            .when_some(duration, |el, duration| {
                el.child(
                    div()
                        .id("status-duration-wrap")
                        .tooltip(crate::hover::tip("Duración de la sesión"))
                        .child(Chip::new("status-duration", duration).icon("history").muted(true).inert(true)),
                )
            })
            .children(self.agent_chips(cx))
            .when_some(self.status_text().filter(|_| self.configs.status_line), |el, line| {
                el.child(div().ml_auto().min_w(px(0.)).truncate().font_family(gpui_m3::theme::MONO_FONT_FAMILY).text_size(px(11.)).text_color(t.faint).child(line))
            });
        div().flex().flex_col().child(self.banners(cx)).child(self.drop_zone("composer-drop", px(26.), card, cx)).child(under)
    }

    /// Los avisos sobre la caja (Expressive): el Artifact recién publicado y la
    /// conversación larga (`ArtifactOffer` y `LongChatNotice` de la referencia). Cada uno
    /// se queda en pantalla mientras sale.
    fn banners(&self, cx: &mut Context<Self>) -> Div {
        use gpui_m3::{Banner, Button, Chip, Exit, Icon, Presence};
        let Some(chat) = self.active_chat() else {
            return div();
        };
        let key = chat.key.clone();
        let mut column = div().flex().flex_col().gap(px(8.)).pb(px(8.));
        if let Some((offer, shown)) = chat.artifact.clone() {
            let close = |key: String| {
                cx.listener(move |view, _: &ClickEvent, _, cx| {
                    if let Some(chat) = view.chats.iter_mut().find(|c| c.key == key) {
                        if let Some((_, shown)) = chat.artifact.as_mut() {
                            *shown = false;
                        }
                    }
                    cx.notify();
                })
            };
            let url = offer.url.clone();
            let open = cx.listener({
                let key = key.clone();
                move |view, _: &ClickEvent, _, cx| {
                    cx.open_url(&url);
                    if let Some(chat) = view.chats.iter_mut().find(|c| c.key == key) {
                        if let Some((_, shown)) = chat.artifact.as_mut() {
                            *shown = false;
                        }
                    }
                    cx.notify();
                }
            });
            let url = offer.url.clone();
            let copy = cx.listener(move |view, _: &ClickEvent, _, cx| {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(url.clone()));
                view.show_toast("Enlace copiado", cx);
            });
            column = column.child(
                Presence::new(SharedString::from(format!("offer-{key}")), shown).exit(Exit::Sink).child(
                    Banner::new(SharedString::from(format!("offer-{key}-{}", offer.serial)))
                        .icon("spark")
                        .title(offer.title.clone())
                        .text("Artifact publicado · ¿abrirlo en el navegador?")
                        .action(Button::new("offer-open", "Abrir").filled().trailing(Icon::new("export").size(px(13.))).on_click(open))
                        .action(Button::new("offer-copy", "Copiar enlace").text().on_click(copy))
                        .dismiss_label("Cerrar")
                        .on_dismiss(close(key.clone())),
                ),
            );
        }
        let level = chat.long_level();
        let mut notice = Banner::new(SharedString::from(format!("long-{key}-{level:?}")))
            .icon("gauge")
            .text(level.map(|l| l.text()).unwrap_or_default())
            .action(Chip::new("long-new", "Chat nuevo").on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.new_conversation(window, cx))))
            .action(Chip::new("long-compact", "Compactar").on_click(cx.listener(|view, _: &ClickEvent, window, cx| {
                view.composer.update(cx, |area, cx| area.set_text("/compact", cx));
                view.send(&super::Send, window, cx);
            })))
            .dismiss_label("Ocultar aviso")
            .on_dismiss(cx.listener(move |view, _: &ClickEvent, _, cx| {
                if let Some(chat) = view.chats.iter_mut().find(|c| c.key == key) {
                    if let Some(level) = chat.long_level() {
                        chat.long_dismissed.push(level);
                    }
                }
                cx.notify();
            }));
        notice = if matches!(level, Some(super::chat::LongLevel::Soft)) { notice.subtle() } else { notice.warning() };
        column.child(Presence::new(SharedString::from(format!("long-{}", chat.key)), level.is_some()).exit(Exit::Sink).child(notice))
    }

    /// La caja de texto acepta archivos arrastrados desde el Explorador, con
    /// «Suelta para adjuntar» encima mientras pasan (como en la referencia).
    fn drop_zone(&self, id: &'static str, radius: gpui::Pixels, card: Div, cx: &mut Context<Self>) -> AnyElement {
        gpui_m3::DropZone::new(id)
            .label("Suelta para adjuntar")
            .radius(radius)
            .on_drop(cx.listener(|view, paths: &gpui::ExternalPaths, _, cx| view.take_files(paths.paths().to_vec(), cx)))
            .child(card)
            .into_any_element()
    }

    /// La barra de estado de Formal y Liquid Glass.
    fn status_pills(
        &self,
        el: Div,
        branch: Option<String>,
        mode_color: Hsla,
        config: &config::ClaudeConfig,
        last_turn: Option<String>,
        cx: &mut Context<Self>,
    ) -> Div {
        let t = t();
        let menu_open = |menu: Menu| self.menu.is_some_and(|(open, _)| open == menu);
        el.when_some(branch, |el, branch| {
            el.child(pill("status-branch").child(svg().path("icons/git-branch.svg").size(px(13.)).text_color(muted())).child(branch))
        })
        .child(
            pill("status-mode")
                .cursor_pointer()
                .text_color(fg())
                .when(menu_open(Menu::Mode), |el| el.bg(t.sel))
                .hover(|el| el.bg(hover_bg()))
                .tooltip(crate::hover::tip("Modo de permisos"))
                .on_click(cx.listener(|view, event: &ClickEvent, _, cx| view.toggle_menu(Menu::Mode, event.position(), cx)))
                .child(div().size(px(8.)).rounded_full().bg(mode_color))
                .child(config.mode_label()),
        )
        .when_some(last_turn, |el, turn| {
            el.child(pill("status-turn").child(svg().path("icons/history.svg").size(px(13.)).text_color(muted())).child(turn))
        })
        .when_some(self.status_text().filter(|_| self.configs.status_line), |el, line| {
            el.child(div().ml_auto().min_w(px(0.)).truncate().font_family(mono()).text_size(px(11.)).text_color(faint()).child(line))
        })
    }

    /// La línea de estado de la conversación visible (la referencia la muestra solo con una).
    fn status_text(&self) -> Option<String> {
        let chat = self.active_chat()?;
        Some(super::status_line(&self.model_label(&self.chat_model()), chat.context, chat.total_cost, chat.last_duration_ms))
    }

    // --- Derecha: cambios, archivos y visor -----------------------------------------

    fn right_panel(&mut self, window: &mut Window, maximized: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let editing = self.doc.is_none() && self.tabs.showing();
        let doc_open = self.doc.is_some() || editing;
        let width = if doc_open { DOC_W } else { RIGHT_W };
        let (title, count): (String, Option<usize>) = match (&self.doc, self.side) {
            (Some(doc), _) => (file_name(&doc.path), None),
            (None, _) if editing => (self.tabs.current().map(|f| file_name(&f.path)).unwrap_or_default(), None),
            (None, Some(Side::Changes)) => ("Cambios".into(), Some(self.changed_files()).filter(|n| *n > 0)),
            _ => ("Archivos".into(), None),
        };
        let header = div()
            .h(px(HEAD_H))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.))
            .pl(px(16.))
            .child(
                div()
                    .when(doc_open, |el| el.max_w(px(width - 330.)).truncate())
                    .when(!doc_open, |el| el.flex_none())
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .when_some(count, |el, count| {
                el.child(
                    div()
                        .min_w(px(20.))
                        .h(px(20.))
                        .px(px(6.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(accent_soft())
                        .text_size(px(11.))
                        .text_color(accent())
                        .child(count.to_string()),
                )
            })
            .when(doc_open && self.doc.as_ref().is_some_and(|d| d.diff.is_some()), |el| {
                // «Abrir» (la referencia): del diff al archivo en el editor.
                el.child(chip("doc-toggle", "Abrir", false).on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                    if let Some(doc) = view.doc.take() {
                        view.open_file(doc.path, cx);
                    }
                })))
            })
            .child(chrome::drag(HEAD_H))
            .when(!doc_open && self.side == Some(Side::Changes), |el| {
                // Mientras se actualiza, el indicador de carga en lugar del botón.
                if self.refreshing {
                    el.child(div().size(px(28.)).flex().items_center().justify_center().child(gpui_m3::LoadingIndicator::new().size(px(20.))))
                } else {
                    el.child(icon_action(
                        "changes-refresh",
                        "icons/rotate-cw.svg",
                        "Actualizar",
                        cx.listener(|view, _: &ClickEvent, _, cx| view.refresh_changes_now(cx)),
                    ))
                }
            })
            .child(icon_action(
                "side-close",
                "icons/x.svg",
                if editing {
                    "Volver a Archivos"
                } else if doc_open {
                    "Cerrar archivo"
                } else {
                    "Cerrar panel"
                },
                cx.listener(|view, _: &ClickEvent, _, cx| {
                    if view.doc.take().is_some() {
                    } else if view.tabs.showing() {
                        // Las pestañas siguen abiertas: se vuelve a la lista de archivos.
                        view.tabs.hide();
                        view.side.get_or_insert(Side::Files);
                    } else {
                        view.side = None;
                    }
                    cx.notify();
                }),
            ))
            .child(div().w(px(6.)))
            .child(chrome::controls_colored(maximized, HEAD_H, fg(), hover_bg()));
        let body = if self.doc.is_some() {
            self.doc_body().into_any_element()
        } else if editing {
            self.editor_body(cx)
        } else if self.side == Some(Side::Changes) {
            self.changes_list(cx).into_any_element()
        } else {
            let roots: Vec<PathBuf> = self.workspaces.active().map(|w| w.folders.clone()).unwrap_or_default();
            self.files(&roots, cx).into_any_element()
        };
        let t = t();
        // En Expressive el panel entra desde la derecha con resorte (`m3-panel`, `motion.css:444`).
        let enter = Enter::new("right-panel-enter").from(32., 0.);
        if expressive() { enter.apply(div(), window, cx) } else { div() }
            .w(px(width))
            .flex_none()
            .flex()
            .flex_col()
            .bg(t.pane)
            .map(|el| {
                if t.gap > 0. {
                    el.ml(px(t.gap)).rounded(px(t.r_pane)).overflow_hidden().when(t.style == Style::Glass, |el| {
                        el.border_1().border_color(t.highlight.opacity(0.35))
                    })
                } else {
                    el.border_l_1().border_color(line())
                }
            })
            .child(header)
            .child(body)
    }

    /// El panel Archivos: la búsqueda (con el índice) y el árbol o los resultados.
    fn files(&mut self, roots: &[PathBuf], cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.file_search.read(cx).text().trim().to_string();
        let body = if query.is_empty() { self.file_tree(roots, cx).into_any_element() } else { self.file_results(&query, roots.len() > 1, cx) };
        div()
            .flex_1()
            .min_h(px(0.))
            .flex()
            .flex_col()
            .child(div().px(px(10.)).pb(px(8.)).child(self.file_search.clone()))
            .child(body)
    }

    fn file_results(&self, query: &str, several: bool, cx: &mut Context<Self>) -> AnyElement {
        let mut list = div().id("file-results").flex_1().min_h(px(0.)).overflow_y_scroll().px(px(8.)).pb(px(12.)).flex().flex_col();
        let Some((_, index)) = self.file_index.as_ref() else {
            return list.child(div().p(px(10.)).text_size(px(13.)).text_color(muted()).child("Buscando…")).into_any_element();
        };
        let found = super::files::search(index, query, super::files::MAX_RESULTS);
        if found.is_empty() {
            return list.child(div().p(px(10.)).text_size(px(13.)).text_color(muted()).child("Sin resultados")).into_any_element();
        }
        for (row, file) in found.into_iter().enumerate() {
            let name = file.name().to_string();
            let dir = file.dir(several);
            let path = file.path.clone();
            let open = cx.listener(move |view, _: &ClickEvent, _, cx| view.open_doc(path.clone(), false, cx));
            let selected = self.active_file.as_ref().is_some_and(|p| super::same_path(p, &file.path));
            let color = kind_color(super::files::kind_of(&name));
            if expressive() {
                list = list.child(
                    gpui_m3::TreeRow::new(("found", row), name.clone())
                        .icon("file")
                        .when_some(color, |r, c| r.icon_color(c))
                        .dimmed(name.starts_with('.'))
                        .selected(selected)
                        .guides(false)
                        .tooltip(file.path.display().to_string())
                        .trailing(div().max_w(px(160.)).truncate().text_size(px(11.5)).text_color(faint()).child(dir))
                        .on_click(open),
                );
                continue;
            }
            list = list.child(
                div()
                    .id(("found", row))
                    .h(px(30.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(selected, |el| el.bg(selected_bg()))
                    .hover(|el| el.bg(hover_bg()))
                    .on_click(open)
                    .child(svg().path("icons/file.svg").size(px(14.)).flex_none().text_color(color.unwrap_or_else(muted)))
                    .child(div().flex_none().text_color(fg()).when(name.starts_with('.'), |el| el.opacity(0.55)).child(name))
                    .child(div().min_w(px(0.)).truncate().text_size(px(11.5)).text_color(faint()).child(dir)),
            );
        }
        list.into_any_element()
    }

    fn file_tree(&mut self, roots: &[PathBuf], cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.explorer.rows(roots);
        let mut list = div().id("explorer").flex_1().min_h(px(0.)).overflow_y_scroll().px(px(8.)).pb(px(12.));
        let m3 = expressive();
        for (index, row) in rows.into_iter().enumerate() {
            if m3 {
                // El árbol de M3: sangría con guías, ícono con el color del tipo,
                // los ocultos atenuados y el archivo abierto resaltado.
                let path = row.path.clone();
                let (root, dir) = (row.depth == 0, row.dir);
                let color = if dir { Some(accent()) } else { kind_color(super::files::kind_of(&row.name)) };
                let selected = !dir && self.active_file.as_ref().is_some_and(|p| super::same_path(p, &row.path));
                list = list.child(
                    gpui_m3::TreeRow::new(("file", index), row.name.clone())
                        .depth(row.depth)
                        .expanded(dir.then_some(row.open))
                        .icon(match (dir, row.open) {
                            (true, true) => "folder-open",
                            (true, false) => "folder",
                            _ => "file",
                        })
                        .when_some(color, |r, c| r.icon_color(c))
                        .root(root)
                        .dimmed(row.name.starts_with('.'))
                        .selected(selected)
                        .tooltip(row.path.display().to_string())
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                            if dir {
                                view.explorer.toggle(&path, root);
                                cx.notify();
                            } else {
                                view.open_doc(path.clone(), false, cx);
                            }
                        })),
                );
                continue;
            }
            let path = row.path.clone();
            let root = row.depth == 0;
            let dir = row.dir;
            let chevron = if !dir {
                None
            } else if row.open {
                Some("icons/chevron-down.svg")
            } else {
                Some("icons/chevron-right.svg")
            };
            let icon = match (dir, row.open) {
                (true, true) => "icons/folder-open.svg",
                (true, false) => "icons/folder.svg",
                _ => "icons/file.svg",
            };
            // Como la referencia: el ícono lleva el color del tipo y el archivo abierto se resalta.
            let icon_color = if dir { accent() } else { kind_color(super::files::kind_of(&row.name)).unwrap_or_else(muted) };
            let selected = !dir && self.active_file.as_ref().is_some_and(|p| super::same_path(p, &row.path));
            list = list.child(
                div()
                    .id(("file", index))
                    .h(px(30.))
                    .pl(px(6. + row.depth as f32 * 16.))
                    .pr(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .text_size(px(13.5))
                    .when(selected, |el| el.bg(selected_bg()))
                    .when(!selected, |el| el.hover(|el| el.bg(hover_bg())))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        if dir {
                            view.explorer.toggle(&path, root);
                            cx.notify();
                        } else {
                            view.open_doc(path.clone(), false, cx);
                        }
                    }))
                    .child(div().w(px(14.)).flex_none().when_some(chevron, |el, chevron| {
                        el.child(svg().path(chevron).size(px(12.)).text_color(faint()))
                    }))
                    .child(svg().path(icon).size(px(15.)).flex_none().text_color(icon_color))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_color(fg())
                            .when(root, |el| el.font_weight(FontWeight::SEMIBOLD))
                            .when(row.name.starts_with('.'), |el| el.opacity(0.55))
                            .child(row.name),
                    ),
            );
        }
        list
    }

    fn changes_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut list = div().id("changes").flex_1().min_h(px(0.)).overflow_y_scroll().px(px(10.)).pb(px(12.)).flex().flex_col().gap(px(10.));
        let total = self.changed_files();
        let added: usize = self.repos.iter().flat_map(|r| &r.files).map(|f| f.added).sum();
        let removed: usize = self.repos.iter().flat_map(|r| &r.files).map(|f| f.removed).sum();
        if self.repos.is_empty() {
            let text = if self.refreshing { "Buscando cambios…" } else { "Sin carpetas que revisar." };
            return list.child(div().p(px(8.)).text_size(px(13.)).text_color(muted()).child(text));
        }
        // Sin nada que confirmar: «Todo al día», como en la referencia (si alguna carpeta es un repo).
        if total == 0 && self.repos.iter().any(|r| r.is_repo) {
            return list.child(
                div()
                    .pt(px(48.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .size(px(48.))
                            .mb(px(6.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(added_bg())
                            .child(svg().path("icons/check.svg").size(px(22.)).text_color(green())),
                    )
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Todo al día"))
                    .child(div().text_size(px(13.)).text_color(muted()).child("No hay cambios sin confirmar.")),
            );
        }
        if total == 0 {
            for (index, repo) in self.repos.iter().enumerate() {
                list = list.child(self.repo_group(index, repo, cx));
            }
            return list;
        }
        list = list.child(
            div()
                .px(px(6.))
                .flex()
                .items_center()
                .text_size(px(13.))
                .child(div().flex_1().text_color(muted()).child(if total == 1 {
                    "1 archivo cambiado".to_string()
                } else {
                    format!("{total} archivos cambiados")
                }))
                .child(div().font_family(mono()).text_size(px(12.)).text_color(green()).child(format!("+{added}")))
                .child(div().ml(px(6.)).font_family(mono()).text_size(px(12.)).text_color(red()).child(format!("−{removed}"))),
        );
        for (repo_index, repo) in self.repos.iter().enumerate() {
            list = list.child(self.repo_group(repo_index, repo, cx));
        }
        list
    }

    /// Un repositorio del panel de cambios: se pliega si tiene cambios; si no,
    /// dice «Al día» o que la carpeta no es un repositorio git.
    fn repo_group(&self, index: usize, repo: &Repo, cx: &mut Context<Self>) -> AnyElement {
        let count = repo.files.len();
        let clean = repo.is_repo && count == 0;
        let open = count > 0 && !self.collapsed_repos.contains(&repo.root);
        let header = div()
            .flex_1()
            .min_w(px(0.))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(div().min_w(px(0.)).truncate().font_weight(FontWeight::SEMIBOLD).child(repo.name.clone()))
            .when(count > 0, |el| {
                el.child(
                    div()
                        .min_w(px(20.))
                        .h(px(20.))
                        .px(px(6.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(accent_soft())
                        .text_size(px(11.))
                        .text_color(accent())
                        .child(count.to_string()),
                )
            })
            .child(div().flex_1())
            .when(clean, |el| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .text_size(px(12.))
                        .text_color(green())
                        .child(svg().path("icons/check.svg").size(px(12.)).text_color(green()))
                        .child("Al día"),
                )
            })
            .when_some(repo.branch.clone(), |el, branch| {
                el.child(
                    div()
                        .h(px(24.))
                        .px(px(8.))
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(px(5.))
                        .rounded(px(6.))
                        .bg(code_bg())
                        .text_size(px(12.))
                        .text_color(muted())
                        .child(svg().path("icons/git-branch.svg").size(px(12.)).text_color(muted()))
                        .child(branch),
                )
            });
        let mut rows = div().flex().flex_col().pb(px(6.));
        for (file_index, file) in repo.files.iter().enumerate() {
            rows = rows.child(change_row(index * 10_000 + file_index, file, cx));
        }
        let root = repo.root.clone();
        let toggle = cx.listener(move |view, open: &bool, _, cx| {
            if *open {
                view.collapsed_repos.remove(&root);
            } else {
                view.collapsed_repos.insert(root.clone());
            }
            cx.notify();
        });
        if expressive() && count > 0 {
            return gpui_m3::ExpandableCard::new(("repo", index))
                .open(open)
                .header(header)
                .on_toggle(move |open, window, cx| toggle(&open, window, cx))
                .child(rows)
                .into_any_element();
        }
        let chevron = if open { "icons/chevron-down.svg" } else { "icons/chevron-right.svg" };
        div()
            .rounded(px(r_card()))
            .border_1()
            .border_color(line())
            .bg(card_bg())
            .flex()
            .flex_col()
            .child(
                div()
                    .id(("repo-head", index))
                    .h(px(42.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .when(count > 0, |el| el.cursor_pointer().on_click(move |_, window, cx| toggle(&!open, window, cx)))
                    .child(if count > 0 {
                        svg().path(chevron).size(px(12.)).flex_none().text_color(faint()).into_any_element()
                    } else {
                        svg().path("icons/folder.svg").size(px(14.)).flex_none().text_color(faint()).into_any_element()
                    })
                    .child(header),
            )
            .when(!repo.is_repo, |el| el.child(div().px(px(14.)).pb(px(12.)).text_size(px(13.)).text_color(muted()).child("No es un repositorio git.")))
            .when(open, |el| el.child(rows))
            .into_any_element()
    }

    fn doc_body(&self) -> impl IntoElement {
        const LINE_H: f32 = 20.0;
        let doc = self.doc.as_ref().expect("visor abierto");
        let body = div().flex_1().min_h(px(0.)).font_family(mono()).text_size(px(12.5)).bg(code_bg()).border_t_1().border_color(line());
        if let Some(note) = &doc.note {
            return body.child(div().p(px(16.)).text_color(muted()).child(note.clone()));
        }
        if let (Some(lines), true) = (&doc.diff, doc.show_diff) {
            // El diff de gpui-m3: números viejo y nuevo, tramos sin cambios plegados y
            // «Mostrar todo». Se interpreta una vez por cambio del archivo.
            let diff = doc_diff(doc, lines);
            let name = file_name(&doc.path);
            return body.child(
                div()
                    .id(("code-doc-diff", doc.id as usize))
                    .size_full()
                    .overflow_y_scroll()
                    .p(px(10.))
                    .child(gpui_m3::DiffView::from_diff(("code-doc-diff-view", doc.id as usize), diff).path(name)),
            );
        }
        let lines = doc.lines.clone();
        let syntax = super::highlight::doc_syntax(&doc.lines, &doc.path);
        let gutter = lines.len().max(1).to_string().len() as f32 * 8.0 + 24.0;
        body.child(
            gpui::uniform_list(("code-doc", doc.id as usize), lines.len(), move |range, _, cx| {
                // Los colores del resaltado, solo de las filas a la vista.
                let palette = gpui_m3::SyntaxPalette::of(cx);
                range
                    .map(|index| {
                        let text = lines[index].clone();
                        let styled = match syntax.borrow_mut().as_mut() {
                            Some(syntax) => gpui::StyledText::new(text).with_highlights(super::highlight::line_styles(syntax, &lines, index, &palette)),
                            None => gpui::StyledText::new(text),
                        };
                        div()
                            .h(px(LINE_H))
                            .flex()
                            .items_center()
                            .child(
                                div()
                                    .w(px(gutter))
                                    .flex_none()
                                    .pr(px(12.))
                                    .flex()
                                    .justify_end()
                                    .text_color(faint())
                                    .child((index + 1).to_string()),
                            )
                            .child(div().flex_1().min_w(px(0.)).truncate().text_color(code_text()).child(styled))
                            .into_any_element()
                    })
                    .collect()
            })
            .size_full(),
        )
    }
}

/// El color del ícono según el tipo de archivo (los de la referencia, que pinta el
/// ícono y deja el nombre en el color del texto).
fn kind_color(kind: super::files::Kind) -> Option<Hsla> {
    use super::files::Kind;
    match kind {
        Kind::Code => Some(accent()),
        Kind::Style => Some(gpui::rgb(0xc678dd).into()),
        Kind::Doc => Some(gpui::rgb(0x56b6c2).into()),
        Kind::Config => Some(amber()),
        Kind::Image => Some(green()),
        Kind::Other => None,
    }
}

/// El diff del visor: el `git diff` del archivo, o todo agregado si git no lo conoce. Se
/// guarda el último (la tarjeta se dibuja a cada cuadro) y se rehace cuando el visor
/// recarga el archivo, que cambia el `Arc` de las líneas.
fn doc_diff(doc: &crate::space::viewer::Doc, lines: &std::sync::Arc<Vec<crate::space::viewer::DiffLine>>) -> std::rc::Rc<gpui_m3::Diff> {
    thread_local! {
        static LAST: std::cell::RefCell<Option<(std::sync::Arc<Vec<crate::space::viewer::DiffLine>>, std::rc::Rc<gpui_m3::Diff>)>> =
            const { std::cell::RefCell::new(None) };
    }
    LAST.with(|last| {
        let mut last = last.borrow_mut();
        if let Some((of, diff)) = last.as_ref() {
            if std::sync::Arc::ptr_eq(of, lines) {
                return diff.clone();
            }
        }
        let diff = std::rc::Rc::new(match &doc.patch {
            Some(patch) => gpui_m3::Diff::from_unified(patch),
            None => gpui_m3::Diff::new("", &doc.lines.join("
")),
        });
        *last = Some((lines.clone(), diff.clone()));
        diff
    })
}

fn change_row(id: usize, file: &FileChange, cx: &mut Context<CodeView>) -> impl IntoElement {
    let (fg, bg) = match file.status {
        'A' | 'N' => (green(), added_bg()),
        'D' => (red(), removed_bg()),
        _ => (amber(), code_bg()),
    };
    let path = file.path.clone();
    let deleted = file.status == 'D';
    div()
        .id(("change", id))
        .mx(px(6.))
        .h(px(44.))
        .px(px(8.))
        .flex()
        .items_center()
        .gap(px(10.))
        .rounded(px(7.))
        .when(!deleted, |el| el.cursor_pointer().hover(|el| el.bg(hover_bg())))
        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
            if !deleted {
                view.open_doc(path.clone(), true, cx);
            }
        }))
        .child(
            div()
                .size(px(22.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(5.))
                .bg(bg)
                .font_family(mono())
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(fg)
                .child(file.status.to_string()),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .child(div().truncate().text_size(px(13.5)).child(file.name.clone()))
                .when(!file.dir.is_empty(), |el| el.child(div().truncate().text_size(px(11.5)).text_color(faint()).child(file.dir.clone()))),
        )
        .when(file.added + file.removed > 0, |el| el.child(gpui_m3::DiffBar::new(file.added, file.removed)))
        .when(file.added > 0, |el| el.child(div().font_family(mono()).text_size(px(12.)).text_color(green()).child(format!("+{}", file.added))))
        .when(file.removed > 0, |el| el.child(div().font_family(mono()).text_size(px(12.)).text_color(red()).child(format!("−{}", file.removed))))
}

pub(super) fn todo_rows(list: &[(String, String)]) -> impl IntoElement {
    let mut rows = div().px(px(14.)).pb(px(12.)).pt(px(2.)).flex().flex_col().gap(px(6.)).border_t_1().border_color(line());
    for (text, status) in list {
        let (icon, color, strike) = match status.as_str() {
            "completed" => ("icons/check.svg", green(), true),
            "in_progress" => ("icons/circle-dot.svg", accent(), false),
            _ => ("icons/circle.svg", faint(), false),
        };
        rows = rows.child(
            div()
                .pt(px(4.))
                .flex()
                .items_center()
                .gap(px(10.))
                .child(svg().path(icon).size(px(14.)).flex_none().text_color(color))
                .child(
                    div()
                        .text_size(px(13.5))
                        .text_color(if strike { faint() } else if status == "in_progress" { accent() } else { fg() })
                        .when(strike, |el| el.line_through())
                        .child(text.clone()),
                ),
        );
    }
    rows
}

/// El input de una herramienta, como mejor se lee: el comando, las líneas de
/// una edición, el contenido escrito o el JSON.
pub(super) fn tool_input(tool: &ToolCall) -> AnyElement {
    let Some(input) = &tool.input else {
        return div().into_any_element();
    };
    let field = |name: &str| input.get(name).and_then(Value::as_str);
    // Lo que se escribe en la terminal, con el `$` de siempre.
    if let Some(command) = field("command") {
        return super::tools::output(format!("{}-command", tool.id), command, 8, Some("$"), false).into_any_element();
    }
    // Edit, MultiEdit y Write: el diff de gpui-m3 con números de línea, tramos sin cambios
    // plegados y «Mostrar todo». MultiEdit, un diff por edición en el orden en que se aplican;
    // Write, todo agregado (pasadas 40 filas, «Mostrar todo»).
    let diffs = super::edits::edit_diffs(&tool.name, input);
    if !diffs.is_empty() {
        let total = diffs.len();
        let mut column = div().flex().flex_col().gap(px(8.));
        for (n, edit) in diffs.iter().enumerate() {
            let view = gpui_m3::DiffView::new(SharedString::from(format!("{}-diff-{n}", tool.id)), &edit.old, &edit.new)
                .when(tool.name == "Write", |view| view.max_rows(RESULT_LINES));
            column = column
                .when(total > 1, |el| el.child(div().text_size(px(11.5)).text_color(faint()).child(format!("Cambio {} de {total}", n + 1))))
                .child(view);
        }
        return column.into_any_element();
    }
    // NotebookEdit: el código o texto nuevo de la celda; otro `content`, tal cual.
    let text = field("new_source").filter(|_| tool.name == "NotebookEdit").or_else(|| field("content"));
    if let Some(text) = text {
        return super::tools::output(format!("{}-text", tool.id), text, RESULT_LINES, None, false).into_any_element();
    }
    if tool.name == "TodoWrite" {
        return todo_rows(&todos(tool)).into_any_element();
    }
    let json = serde_json::to_string_pretty(input).unwrap_or_default();
    super::tools::output(format!("{}-json", tool.id), &json, RESULT_LINES, None, false).into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_vista_previa_del_razonamiento_es_su_ultima_linea() {
        assert_eq!(last_line("primero\nsegundo\ntercero  \n\n"), "tercero");
        assert_eq!(last_line(""), "");
        // Con un texto largo mira solo el final, sin cortar una letra en dos.
        let long = format!("{}\nfinal con ñ", "á".repeat(500));
        assert_eq!(last_line(&long), "final con ñ");
        assert!(!last_line(&"ñ".repeat(300)).is_empty());
    }
}
