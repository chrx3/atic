//! Lo que se dibuja de Atic Code, con la forma de la referencia: a la izquierda la
//! conversación nueva y los proyectos con sus conversaciones; al centro el
//! chat con su encabezado y la caja de texto con la barra de estado; a la
//! derecha, cuando se abre, los cambios o los archivos.
//!
//! No hay barra de ventana aparte: el encabezado de cada columna arrastra la
//! ventana y los botones de Windows van en la de más a la derecha.
//!
//! El texto de Claude se muestra con un markdown mínimo (párrafos, títulos,
//! listas, bloques de código, `código` y **negrita** en línea). No se puede
//! seleccionar (GPUI no lo trae): cada respuesta y cada bloque tiene «Copiar».

use std::ops::Range;
use std::path::PathBuf;

use gpui::{
    canvas, div, prelude::*, px, svg, AnyElement, ClickEvent, ClipboardItem, Context, Div, Focusable, FontWeight,
    HighlightStyle, Hsla, MouseButton, ScrollWheelEvent, SharedString, Stateful, StyledText, Window,
};
use serde_json::Value;

use super::chat::{Item, Permission, ToolCall};
use super::config;
use super::git::{FileChange, Repo};
use super::{Attachment, CodeView, Menu, SessionInfo, Side, COMPOSER, LOOSE};
use crate::space::chrome;
use crate::space::viewer::LineKind;
use super::style::{t, Style};

const HEAD_H: f32 = 52.0;
const SIDE_W: f32 = 264.0;
const RIGHT_W: f32 = 380.0;
const DOC_W: f32 = 600.0;
const THREAD_W: f32 = 860.0;
/// La columna de la pantalla de inicio de Expressive.
const HERO_W: f32 = 720.0;
const HERO_PLACEHOLDER: &str = "Pregunta lo que quieras · @ para mencionar · / para acciones";
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
/// la referencia. Pendiente, más tenue (se grabará con el próximo mensaje).
fn model_mark(label: String, pending: bool) -> AnyElement {
    rule_mark("icons/cpu.svg", label, pending)
}

/// Un separador con su etiqueta al centro: el cambio de modelo y «Contexto compactado».
// TODO(gpui-m3): Divider con etiqueta (inset, con ícono); por ahora, el de aquí.
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
fn code_bg() -> Hsla {
    let t = t();
    match t.style {
        Style::Formal => t.pane,
        Style::Expressive => t.raised,
        Style::Glass => t.control,
    }
}
/// El fondo del `código` en línea: más marcado que el de un bloque.
fn inline_code_bg() -> Hsla {
    t().control2
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
fn r_pop() -> f32 {
    t().r_pop
}
/// Tarjetas del chat: herramientas, bloques de código, grupos de cambios.
fn r_card() -> f32 {
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
        if self.settings_open && self.claude_info.is_none() && expressive() {
            self.check_claude(cx);
        }
        let t = t();
        self.composer_focused = self.composer.read(cx).focus_handle(cx).is_focused(window);
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
        let placeholder = if self.active_chat().is_none() && t.style == Style::Expressive { HERO_PLACEHOLDER } else { "Responde a Claude…" };
        if self.composer.read(cx).placeholder() != placeholder {
            self.composer.update(cx, |area, cx| area.set_placeholder(placeholder, cx));
        }
        let maximized = window.is_maximized();
        let right_open = self.side.is_some() || self.doc.is_some();
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
            .map(|el| if expressive() { el.child(self.sidebar_m3(cx)) } else { el.child(self.sidebar(cx)) })
            .child(self.center(maximized && !right_open, !right_open, cx))
            .when(right_open, |el| el.child(self.right_panel(maximized, cx)))
            .when_some(self.menu_layer(cx), |el, menu| el.child(menu))
            .when_some(self.session_menu_layer(cx), |el, menu| el.child(menu))
            .when_some(self.space_menu_layer(cx), |el, menu| el.child(menu))
            .when_some(self.new_space_dialog(cx), |el, dialog| el.child(dialog))
            .when_some(self.pop_layer(cx), |el, pop| el.child(pop))
            .when(self.palette_open, |el| el.child(self.palette.clone()))
            .when(self.settings_open, |el| if expressive() { el.child(self.settings_m3(cx)) } else { el.child(self.settings(cx)) })
            .when_some(self.toast.clone(), |el, text| {
                el.child(
                    div()
                        .absolute()
                        .bottom(px(24.))
                        .left_0()
                        .right_0()
                        .flex()
                        .justify_center()
                        .child(gpui_m3::Toast::new(("toast", self.toast_gen as usize), text)),
                )
            })
    }
}

// --- Piezas comunes ----------------------------------------------------------------

fn icon_button(id: impl Into<gpui::ElementId>, icon: &'static str, tip: &'static str) -> Stateful<Div> {
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

fn chip(id: impl Into<gpui::ElementId>, label: impl Into<SharedString>, on: bool) -> Stateful<Div> {
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

/// `chip` con su acción; en Expressive, el chip de filtro de gpui-m3.
fn chip_action(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    on: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> AnyElement {
    if expressive() {
        gpui_m3::Chip::new(id, label).filter(on).on_click(on_click).into_any_element()
    } else {
        chip(id, label, on).on_click(on_click).into_any_element()
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

fn file_name(path: &std::path::Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string())
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mark {
    Code,
    Bold,
}

/// `código` y **negrita** en una línea: el texto sin las marcas y dónde va cada
/// estilo. Una marca sin cerrar se deja como está.
fn inline(text: &str) -> (String, Vec<(Range<usize>, Mark)>) {
    let mut out = String::with_capacity(text.len());
    let mut marks = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let tick = rest.find('`');
        let bold = rest.find("**");
        let (at, mark, open) = match (tick, bold) {
            (Some(t), Some(b)) if b < t => (b, Mark::Bold, "**"),
            (Some(t), _) => (t, Mark::Code, "`"),
            (None, Some(b)) => (b, Mark::Bold, "**"),
            (None, None) => break,
        };
        let after = &rest[at + open.len()..];
        let Some(close) = after.find(open).filter(|c| *c > 0) else {
            out.push_str(&rest[..at + open.len()]);
            rest = after;
            continue;
        };
        out.push_str(&rest[..at]);
        let start = out.len();
        out.push_str(&after[..close]);
        marks.push((start..out.len(), mark));
        rest = &after[close + open.len()..];
    }
    out.push_str(rest);
    (out, marks)
}

fn rich(text: &str) -> StyledText {
    let (clean, marks) = inline(text);
    let highlights: Vec<(Range<usize>, HighlightStyle)> = marks
        .into_iter()
        .map(|(range, mark)| {
            let style = match mark {
                Mark::Bold => HighlightStyle { font_weight: Some(FontWeight::SEMIBOLD), ..Default::default() },
                Mark::Code => HighlightStyle {
                    background_color: Some(inline_code_bg()),
                    color: Some(accent()),
                    ..Default::default()
                },
            };
            (range, style)
        })
        .collect();
    StyledText::new(clean).with_highlights(highlights)
}

/// Un markdown mínimo: títulos, listas, párrafos y bloques de código.
pub(super) fn markdown(id: &str, text: &str) -> Div {
    let mut out = div().flex().flex_col().gap(px(10.)).min_w(px(0.));
    let mut paragraph: Vec<&str> = Vec::new();
    let mut code: Option<(String, Vec<&str>)> = None;
    let mut blocks = 0usize;
    let flush = |out: Div, paragraph: &mut Vec<&str>| -> Div {
        if paragraph.is_empty() {
            return out;
        }
        let mut block = div().flex().flex_col().gap(px(4.));
        for line in paragraph.drain(..) {
            let trimmed = line.trim_start();
            let bullet = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* "));
            block = match bullet {
                Some(item) => block.child(
                    div()
                        .flex()
                        .gap(px(8.))
                        .pl(px(if line.len() - trimmed.len() >= 2 { 18. } else { 2. }))
                        .child(div().flex_none().text_color(muted()).child("•"))
                        .child(div().flex_1().min_w(px(0.)).child(rich(item))),
                ),
                None => block.child(div().child(rich(line))),
            };
        }
        out.child(block.text_color(fg()).line_height(px(23.)))
    };
    for line in text.lines() {
        if let Some(lang) = line.trim_start().strip_prefix("```") {
            match code.take() {
                Some((lang, lines)) => {
                    blocks += 1;
                    out = out.child(code_block(format!("{id}-{blocks}"), &lang, &lines.join("\n")));
                }
                None => {
                    out = flush(out, &mut paragraph);
                    code = Some((lang.trim().to_string(), Vec::new()));
                }
            }
            continue;
        }
        if let Some((_, lines)) = &mut code {
            lines.push(line);
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            out = flush(out, &mut paragraph);
        } else if trimmed.starts_with('#') {
            out = flush(out, &mut paragraph);
            let title = trimmed.trim_start_matches('#').trim().to_string();
            out = out.child(div().pt(px(6.)).text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child(rich(&title)));
        } else {
            paragraph.push(line);
        }
    }
    if let Some((lang, lines)) = code {
        blocks += 1;
        out = out.child(code_block(format!("{id}-{blocks}"), &lang, &lines.join("\n")));
    }
    flush(out, &mut paragraph)
}

/// Un bloque de código con su encabezado: el lenguaje y «Copiar».
fn code_block(id: String, lang: &str, text: &str) -> Div {
    let copy = text.to_string();
    div()
        .rounded(px(r_card()))
        .border_1()
        .border_color(line())
        .bg(code_bg())
        .overflow_hidden()
        .child(
            div()
                .h(px(32.))
                .px(px(14.))
                .flex()
                .items_center()
                .border_b_1()
                .border_color(line())
                .text_size(px(12.))
                .text_color(muted())
                .child(div().flex_1().font_family(mono()).child(if lang.is_empty() { "código".to_string() } else { lang.to_string() }))
                .child(
                    div()
                        .id(SharedString::from(format!("copy-{id}")))
                        .px(px(6.))
                        .rounded(px(5.))
                        .cursor_pointer()
                        .hover(|el| el.text_color(fg()))
                        .on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(copy.clone())))
                        .child("Copiar"),
                ),
        )
        .child(mono_body(text))
}

fn mono_body(text: &str) -> Div {
    div()
        .px(px(14.))
        .py(px(10.))
        .font_family(mono())
        .text_size(px(12.5))
        .line_height(px(20.))
        .text_color(code_text())
        .child(text.to_string())
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
        _ => "",
    };
    let mut summary = tool.summary();
    if matches!(tool.name.as_str(), "Read" | "Edit" | "MultiEdit" | "Write" | "NotebookEdit") {
        summary = summary.rsplit(['\\', '/']).next().unwrap_or(&summary).to_string();
    }
    (verb, summary)
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
        for workspace in self.workspaces.list() {
            let id = workspace.id;
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
                    .child(actions)
                    .into_any_element()
            });
            if selected {
                projects = projects.children(self.chat_rows(id, cx));
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
                        cx.listener(|view, _: &ClickEvent, _, cx| view.pick_folders(None, cx)),
                    )),
            )
            .child(div().id("code-projects").flex_1().min_h(px(0.)).overflow_y_scroll().px(px(10.)).pb(px(12.)).child(projects))
            .child(div().p(px(10.)).when(t.gap == 0., |el| el.border_t_1().border_color(line())).child(settings_row))
    }

    fn chat_rows(&self, workspace: u64, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut rows = Vec::new();
        let row = |id: SharedString, label: String, on: bool| {
            div()
                .id(id)
                .ml(px(26.))
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
                rows.push(
                    div()
                        .ml(px(26.))
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
            rows.push(
                row(row_id, chat.title.clone(), on)
                    .group("chat-row")
                    .on_click(select)
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
                div().ml(px(36.)).h(px(28.)).flex().items_center().text_size(px(13.)).text_color(faint()).child("Sin conversaciones").into_any_element(),
            );
        }
        for info in history {
            let session = info.clone();
            let id = SharedString::from(format!("hist-{}", info.session_id));
            let on_open = cx.listener(move |view, _: &ClickEvent, window, cx| view.open_session(workspace, session.clone(), window, cx));
            rows.push(if expressive() {
                div()
                    .ml(px(26.))
                    .child(gpui_m3::NavItem::new(id, info.title.clone()).dense(true).on_click(on_open))
                    .into_any_element()
            } else {
                row(id, info.title.clone(), false).text_color(muted()).on_click(on_open).into_any_element()
            });
        }
        rows
    }

    // --- Centro: encabezado, chat y caja de texto ---------------------------------------

    fn center(&self, maximized: bool, controls: bool, cx: &mut Context<Self>) -> impl IntoElement {
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
        let mut thread = div()
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
                for (index, item) in chat.items.iter().enumerate() {
                    let flagged = self.is_flagged(chat, index);
                    thread = thread.child(self.item(&chat.key, index, item, flagged, cx));
                }
                // Se eligió otro modelo con la conversación empezada: se grabará con el próximo mensaje.
                if let Some(pending) = chat.pending_model(&self.models, &self.configs.get(chat.workspace)) {
                    thread = thread.child(model_mark(format!("{pending} en el próximo mensaje"), true));
                }
                // En Expressive, como en la referencia, las solicitudes van al final del hilo, todas.
                if expressive() && !chat.permissions.is_empty() {
                    thread = thread.child(self.permission_cards_m3(chat, cx));
                }
                if chat.busy && chat.permissions.is_empty() {
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
            .when(hero, |el| el.child(self.hero(has_workspace, cx)))
            .when(history, |el| el.child(self.history_view(cx)))
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
            .when_some(chat.filter(|c| !c.permissions.is_empty() && !expressive()), |el, chat| {
                el.child(self.permission_card(&chat.key, &chat.permissions[0], cx))
            })
            .when((has_workspace || loose) && !hero && !history, |el| el.child(self.composer_box(chat.is_some_and(|c| c.busy), false, cx)))
    }

    /// El inicio de Expressive: las formas, «¿Qué construimos hoy?», el proyecto,
    /// la caja de texto y las sugerencias.
    fn hero(&self, has_workspace: bool, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_m3::{Icon, Shape, ShapeName};
        let scheme = gpui_m3::Theme::of(cx).clone();
        let shapes = div()
            .h(px(84.))
            .mb(px(4.))
            .flex()
            .justify_center()
            .child(
                div().h_full().flex().items_end().pb(px(6.)).mr(px(-8.)).child(
                    Shape::new(ShapeName::Sunny).size(px(30.)).color(scheme.secondary_container).breathe(ShapeName::Cookie12, 5.).spin(18.),
                ),
            )
            .child(
                div().h_full().flex().items_center().child(
                    Shape::new(ShapeName::Cookie9)
                        .size(px(68.))
                        .color(scheme.primary_container)
                        .breathe(ShapeName::SoftBurst, 7.)
                        .spin(40.)
                        .child(Icon::new("spark").size(px(28.)).color(scheme.on_primary_container)),
                ),
            )
            .child(
                div().h_full().flex().items_start().pt(px(4.)).ml(px(-10.)).child(
                    Shape::new(ShapeName::Clover4).size(px(40.)).color(scheme.tertiary_container).breathe(ShapeName::Cookie4, 9.).spin(-26.),
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
                    .child(shapes)
                    .child(title)
                    .child(div().flex().child(picker))
                    .when(has_workspace || loose, |el| el.child(self.composer_box(false, true, cx)))
                    .when(has_workspace && !loose, |el| el.child(suggestions)),
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

    fn item(&self, key: &str, index: usize, item: &Item, flagged: bool, cx: &mut Context<Self>) -> AnyElement {
        let row = match item {
            Item::User { .. } | Item::Text(_) => self.message(key, index, item, flagged, cx),
            Item::Compact => rule_mark("icons/layers.svg", "Contexto compactado".into(), false),
            _ => self.other_item(key, index, item, cx),
        };
        self.flag_frame(index, flagged, row)
    }

    /// Un mensaje del usuario o una respuesta, con sus acciones (copiar y marcar).
    fn message(&self, key: &str, index: usize, item: &Item, flagged: bool, cx: &mut Context<Self>) -> AnyElement {
        let group = SharedString::from(format!("msg-{index}"));
        let flag = self.flag_button(index, flagged, cx);
        let actions = |el: Div| {
            el.when(!flagged, |el| el.invisible().group_hover(group.clone(), |el| el.visible()))
        };
        match item {
            Item::User { text, .. } if expressive() => div()
                .group(group.clone())
                .flex()
                .flex_col()
                .items_end()
                .child(gpui_m3::Bubble::new(gpui_m3::BubbleKind::User, text.clone()))
                .child(actions(div().mt(px(2.)).mb(px(-4.)).child(flag)))
                .into_any_element(),
            Item::User { text, .. } => div()
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
                        .child(text.clone()),
                )
                .child(actions(div().absolute().right(px(-6.)).bottom(px(-24.)).child(flag)))
                .into_any_element(),
            Item::Text(text) if expressive() => {
                let copy = text.clone();
                div()
                    .group(group.clone())
                    .flex()
                    .flex_col()
                    .child(markdown(&format!("{key}-{index}"), text))
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
                    .child(markdown(&format!("{key}-{index}"), text))
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

    fn other_item(&self, key: &str, index: usize, item: &Item, cx: &mut Context<Self>) -> AnyElement {
        match item {
            Item::Notice { text, error: false } if expressive() => {
                gpui_m3::Bubble::new(gpui_m3::BubbleKind::Notice, text.clone()).into_any_element()
            }
            Item::Thinking(text) if expressive() => {
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
                    .header(div().text_size(px(12.5)).text_color(muted()).child("Razonamiento"))
                    .on_toggle(move |open, window, cx| flip(&open, window, cx))
                    .child(div().text_size(px(12.5)).text_color(muted()).child(markdown(&format!("{key}-{index}-think"), text)))
                    .into_any_element()
            }
            Item::Tool(tool) if expressive() => self.tool_m3(tool, cx),
            Item::Thinking(text) => {
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
                            .child("Razonamiento"),
                    )
                    .when(open, |el| {
                        el.child(
                            div()
                                .ml(px(6.))
                                .pl(px(14.))
                                .border_l_2()
                                .border_color(line())
                                .text_size(px(13.))
                                .line_height(px(20.))
                                .text_color(muted())
                                .child(text.clone()),
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
            Item::Model(name) => model_mark(format!("Cambiado a {name}"), false),
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
        let counts = edit_counts(tool);
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
                        let lines: Vec<&str> = result.lines().collect();
                        let mut shown = lines.iter().take(RESULT_LINES).copied().collect::<Vec<_>>().join("\n");
                        if lines.len() > RESULT_LINES {
                            shown.push_str(&format!("\n… {} líneas más", lines.len() - RESULT_LINES));
                        }
                        el.child(
                            div()
                                .rounded(px(8.))
                                .bg(code_bg())
                                .child(mono_body(&shown).when(tool.is_error, |el| el.text_color(red()))),
                        )
                    }),
            );
        }
        card.into_any_element()
    }

    fn permission_card(&self, key: &str, permission: &Permission, cx: &mut Context<Self>) -> impl IntoElement {
        let tool = ToolCall {
            id: permission.request_id.clone(),
            name: permission.tool.clone(),
            partial: String::new(),
            input: Some(permission.input.clone()),
            result: None,
            is_error: false,
        };
        let title = permission.title.clone().unwrap_or_else(|| {
            let (verb, summary) = tool_label(&tool);
            if verb.is_empty() {
                format!("Claude quiere usar {}", permission.tool)
            } else {
                format!("Claude quiere {}: {summary}", verb.to_lowercase())
            }
        });
        let button = |id: &'static str, label: &'static str, primary: bool| {
            div()
                .id(id)
                .px(px(16.))
                .h(px(36.))
                .flex()
                .items_center()
                .rounded(px(r_btn().min(18.)))
                .cursor_pointer()
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(px(13.5))
                .when(primary, |el| el.bg(accent()).text_color(on_accent()))
                .when(!primary, |el| el.border_1().border_color(line()).text_color(t().on_attention).hover(|el| el.bg(hover_bg())))
                .child(label)
        };
        let (k1, r1) = (key.to_string(), permission.request_id.clone());
        let (k2, r2) = (k1.clone(), r1.clone());
        let (k3, r3) = (k1.clone(), r1.clone());
        let allow = cx.listener(move |view, _: &ClickEvent, _, cx| view.answer(&k1, &r1, true, false, cx));
        let always = cx.listener(move |view, _: &ClickEvent, _, cx| view.answer(&k2, &r2, true, true, cx));
        let deny = cx.listener(move |view, _: &ClickEvent, _, cx| view.answer(&k3, &r3, false, false, cx));
        let wrap = div().w_full().max_w(px(THREAD_W)).mx_auto().px(px(32.)).mb(px(10.));
        wrap.child(
            div()
                .rounded(px(r_card()))
                .bg(t().attention)
                .text_color(t().on_attention)
                .flex()
                .overflow_hidden()
                .map(|el| match t().style {
                    // Formal: borde y la franja de acento a la izquierda, como la referencia.
                    Style::Formal => el.border_1().border_color(line()).child(div().w(px(3.)).flex_none().bg(accent())),
                    Style::Expressive => el,
                    Style::Glass => el.border_1().border_color(t().highlight.opacity(0.4)).shadow(float_shadow()),
                })
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .p(px(16.))
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                        .child(tool_input(&tool))
                        .when_some(permission.description.clone(), |el, text| {
                            el.child(div().text_size(px(13.)).text_color(muted()).child(text))
                        })
                        .child(
                            div()
                                .flex()
                                .gap(px(8.))
                                .child(button("perm-allow", "Permitir", true).on_click(allow))
                                .when(permission.suggestions.is_some(), |el| {
                                    el.child(button("perm-always", "Permitir siempre", false).on_click(always))
                                })
                                .child(button("perm-deny", "Rechazar", false).on_click(deny)),
                        ),
                ),
        )
    }

    fn composer_box(&self, busy: bool, hero: bool, cx: &mut Context<Self>) -> Div {
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
        let mut attachments = div().flex().flex_wrap().gap(px(6.));
        for (index, attachment) in self.attachments.iter().enumerate() {
            let (icon, name) = match attachment {
                Attachment::Image { name, .. } => ("icons/image.svg", name.clone()),
                Attachment::File(path) => ("icons/file.svg", file_name(path)),
            };
            let remove = cx.listener(move |view, _: &ClickEvent, _, cx| view.remove_attachment(index, cx));
            if expressive() {
                attachments = attachments.child(
                    gpui_m3::Chip::input(("attachment", index), name)
                        .leading(svg().path(icon).size(px(16.)).text_color(muted()))
                        .on_remove(remove),
                );
                continue;
            }
            attachments = attachments.child(
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
                    .child(svg().path(icon).size(px(13.)).text_color(muted()))
                    .child(div().max_w(px(220.)).truncate().child(name))
                    .child(
                        div()
                            .id(("attachment-x", index))
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
                    ),
            );
        }
        let ready = !self.attachments.is_empty() || !self.composer.read(cx).text().trim().is_empty();
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
            .when(!self.attachments.is_empty(), |el| el.child(attachments))
            .child(div().key_context(COMPOSER).on_action(cx.listener(Self::send)).h(px(60.)).child(self.composer.clone()))
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
            .child(card)
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
            .when(!self.attachments.is_empty(), |el| el.child(attachments.px(px(12.)).pt(px(10.))))
            .child(
                div()
                    .key_context(COMPOSER)
                    .on_action(cx.listener(Self::send))
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
        div().flex().flex_col().child(card).child(under)
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

    fn right_panel(&mut self, maximized: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let doc_open = self.doc.is_some();
        let width = if doc_open { DOC_W } else { RIGHT_W };
        let (title, count): (String, Option<usize>) = match (&self.doc, self.side) {
            (Some(doc), _) => (file_name(&doc.path), None),
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
                let show_diff = self.doc.as_ref().is_some_and(|d| d.show_diff);
                el.child(chip("doc-toggle", if show_diff { "Ver archivo" } else { "Ver cambios" }, false).on_click(cx.listener(
                    |view, _: &ClickEvent, _, cx| {
                        if let Some(doc) = &mut view.doc {
                            doc.show_diff = !doc.show_diff;
                        }
                        cx.notify();
                    },
                )))
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
                if doc_open { "Cerrar archivo" } else { "Cerrar panel" },
                cx.listener(|view, _: &ClickEvent, _, cx| {
                    if view.doc.take().is_none() {
                        view.side = None;
                    }
                    cx.notify();
                }),
            ))
            .child(div().w(px(6.)))
            .child(chrome::controls_colored(maximized, HEAD_H, fg(), hover_bg()));
        let body = if doc_open {
            self.doc_body().into_any_element()
        } else if self.side == Some(Side::Changes) {
            self.changes_list(cx).into_any_element()
        } else {
            let roots: Vec<PathBuf> = self.workspaces.active().map(|w| w.folders.clone()).unwrap_or_default();
            self.files(&roots, cx).into_any_element()
        };
        let t = t();
        div()
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
                    .hover(|el| el.bg(hover_bg()))
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
                    .child(svg().path(icon).size(px(15.)).flex_none().text_color(if dir { accent() } else { muted() }))
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
        if let (Some(diff), true) = (&doc.diff, doc.show_diff) {
            let lines = diff.clone();
            let gutter = lines.iter().filter_map(|l| l.number).max().unwrap_or(1).to_string().len() as f32 * 8.0 + 24.0;
            return body.child(
                gpui::uniform_list(("code-doc", doc.id as usize), lines.len(), move |range, _, _| {
                    range
                        .map(|index| {
                            let line = &lines[index];
                            let (bg, fg, mark) = match line.kind {
                                LineKind::Added => (Some(added_bg()), green(), "+"),
                                LineKind::Removed => (Some(removed_bg()), red(), "-"),
                                LineKind::Hunk => (Some(accent_soft()), accent(), ""),
                                LineKind::Context => (None, code_text(), ""),
                            };
                            div()
                                .h(px(LINE_H))
                                .flex()
                                .items_center()
                                .when_some(bg, |el, bg| el.bg(bg))
                                .child(
                                    div()
                                        .w(px(gutter))
                                        .flex_none()
                                        .pr(px(10.))
                                        .flex()
                                        .justify_end()
                                        .text_color(faint())
                                        .child(line.number.map(|n| n.to_string()).unwrap_or_default()),
                                )
                                .child(div().w(px(14.)).flex_none().text_color(fg).child(mark))
                                .child(div().flex_1().min_w(px(0.)).truncate().text_color(fg).child(line.text.clone()))
                                .into_any_element()
                        })
                        .collect()
                })
                .size_full(),
            );
        }
        let lines = doc.lines.clone();
        let gutter = lines.len().max(1).to_string().len() as f32 * 8.0 + 24.0;
        body.child(
            gpui::uniform_list(("code-doc", doc.id as usize), lines.len(), move |range, _, _| {
                range
                    .map(|index| {
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
                            .child(div().flex_1().min_w(px(0.)).truncate().text_color(code_text()).child(lines[index].clone()))
                            .into_any_element()
                    })
                    .collect()
            })
            .size_full(),
        )
    }

    // --- Configuración de Claude ----------------------------------------------------

    fn settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let config = self.config();
        let workspace = self.workspaces.active().map(|w| w.name.clone());
        let row = |label: &'static str, chips: Div| {
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).text_color(fg()).child(label))
                .child(chips.flex().flex_wrap().gap(px(6.)))
        };
        let (shown, more) = config::split_models(&self.models, &config.model);
        let visible: Vec<usize> = if self.more_models { (0..self.models.len()).collect() } else { shown };
        let mut models = div();
        for index in visible {
            let (id, name) = &self.models[index];
            let id = id.clone();
            models = models.child(chip_action(("set-model", index), name.clone(), config.model == id, cx.listener(
                move |view, _: &ClickEvent, _, cx| {
                    let id = id.clone();
                    view.set_defaults(|c| c.model = id, cx)
                },
            )));
        }
        if !more.is_empty() {
            let label = if self.more_models { "Menos modelos".to_string() } else { format!("Más modelos ({})", more.len()) };
            models = models.child(
                div()
                    .id("set-more-models")
                    .px(px(10.))
                    .h(px(30.))
                    .flex()
                    .items_center()
                    .rounded(px(8.))
                    .cursor_pointer()
                    .text_size(px(13.))
                    .text_color(accent())
                    .hover(|el| el.bg(hover_bg()))
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                        view.more_models = !view.more_models;
                        cx.notify();
                    }))
                    .child(label),
            );
        }
        let mut modes = div();
        for (id, label) in config::MODES {
            modes = modes.child(
                chip_action(SharedString::from(format!("set-mode-{id}")), label, config.permission_mode == id, cx.listener(move |view, _: &ClickEvent, _, cx| view.set_config(|c| c.permission_mode = id.into(), cx))),
            );
        }
        let mut efforts = div().child(
            chip_action("set-effort-default", "Predeterminado", config.effort.is_empty(), cx.listener(|view, _: &ClickEvent, _, cx| view.set_defaults(|c| c.effort = String::new(), cx))),
        );
        for (id, label) in config::EFFORTS {
            efforts = efforts.child(
                chip_action(SharedString::from(format!("set-effort-{id}")), label, config.effort == id, cx.listener(move |view, _: &ClickEvent, _, cx| view.set_defaults(|c| c.effort = id.into(), cx))),
            );
        }
        let thinking = if expressive() {
            let toggle = cx.listener(|view, on: &bool, _, cx| view.set_config(|c| c.thinking = *on, cx));
            div().items_center().child(gpui_m3::Switch::new("set-think", config.thinking).on_toggle(move |on, window, cx| toggle(&on, window, cx))).child(
                div().text_size(px(13.)).text_color(muted()).child(if config.thinking { "Se muestra" } else { "Oculto" }),
            )
        } else {
            div()
                .child(chip_action("set-think-on", "Mostrar", config.thinking, cx.listener(|view, _: &ClickEvent, _, cx| view.set_config(|c| c.thinking = true, cx))))
                .child(chip_action("set-think-off", "Ocultar", !config.thinking, cx.listener(|view, _: &ClickEvent, _, cx| view.set_config(|c| c.thinking = false, cx))))
        };
        let (style_now, mode_now) = (self.configs.style, self.configs.mode);
        let mut styles = div();
        for (style, label) in super::style::Style::ALL {
            styles = styles.child(
                chip_action(SharedString::from(format!("set-style-{label}")), label, style_now == style, cx.listener(
                    move |view, _: &ClickEvent, window, cx| {
                        let mode = view.configs.mode;
                        view.set_appearance(style, mode, window, cx)
                    },
                )),
            );
        }
        let mut modes_ui = div();
        for (mode, label) in super::style::Mode::ALL {
            modes_ui = modes_ui.child(
                chip_action(SharedString::from(format!("set-ui-mode-{label}")), label, mode_now == mode, cx.listener(
                    move |view, _: &ClickEvent, window, cx| {
                        let style = view.configs.style;
                        view.set_appearance(style, mode, window, cx)
                    },
                )),
            );
        }
        let appearance = div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .pb(px(16.))
            .border_b_1()
            .border_color(line())
            .child(row("Estilo", styles))
            .child(row("Modo de color", modes_ui));
        let claude = match &self.claude_path {
            Some(path) => format!("Claude Code: {}", path.display()),
            None => "No se encontró Claude Code. Instálalo e inicia sesión con `claude`.".into(),
        };
        let body: AnyElement = if workspace.is_some() {
            div()
                .flex()
                .flex_col()
                .gap(px(18.))
                .child(row("Modelo", models))
                .child(row("Permisos", modes))
                .child(row("Esfuerzo", efforts))
                .child(row("Razonamiento", thinking))
                .child(div().text_size(px(12.5)).line_height(px(19.)).text_color(muted()).child(
                    "Vale para las conversaciones nuevas del proyecto. Modelo, permisos y razonamiento se aplican también a las abiertas; el esfuerzo, desde la próxima.",
                ))
                .into_any_element()
        } else {
            div().text_color(muted()).child("Crea un proyecto para configurarlo.").into_any_element()
        };
        div()
            .id("settings-backdrop")
            .absolute()
            .inset_0()
            .bg(gpui::black().opacity(0.35))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| {
                    view.settings_open = false;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .id("settings-card")
                    .w(px(560.))
                    .p(px(22.))
                    .rounded(px(r_pop()))
                    .shadow(float_shadow())
                    .border_1()
                    .border_color(line())
                    .bg(card_bg())
                    .flex()
                    .flex_col()
                    .gap(px(20.))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .child(div().flex_1().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).child(match &workspace {
                                Some(name) => format!("Configuración · {name}"),
                                None => "Configuración".into(),
                            }))
                            .child(icon_action(
                                "settings-close",
                                "icons/x.svg",
                                "Cerrar",
                                cx.listener(|view, _: &ClickEvent, _, cx| {
                                    view.settings_open = false;
                                    cx.notify();
                                }),
                            )),
                    )
                    .child(appearance)
                    .child(body)
                    .child(div().pt(px(4.)).border_t_1().border_color(line()).text_size(px(12.)).text_color(faint()).child(claude)),
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

/// Las líneas que suma y quita una edición, para el encabezado de la tarjeta.
fn edit_counts(tool: &ToolCall) -> Option<(usize, usize)> {
    let input = tool.input.as_ref()?;
    let count = |name: &str| input.get(name).and_then(Value::as_str).map(|t| t.lines().count());
    match tool.name.as_str() {
        "Edit" => Some((count("new_string")?, count("old_string")?)),
        "Write" => Some((count("content")?, 0)),
        _ => None,
    }
}

/// El input de una herramienta, como mejor se lee: el comando, las líneas de
/// una edición, el contenido escrito o el JSON.
pub(super) fn tool_input(tool: &ToolCall) -> AnyElement {
    let Some(input) = &tool.input else {
        return div().into_any_element();
    };
    let field = |name: &str| input.get(name).and_then(Value::as_str);
    let framed = |inner: Div| div().rounded(px(8.)).border_1().border_color(line()).bg(code_bg()).overflow_hidden().child(inner);
    if let Some(command) = field("command") {
        return framed(
            div()
                .px(px(12.))
                .py(px(9.))
                .flex()
                .gap(px(8.))
                .font_family(mono())
                .text_size(px(12.5))
                .child(div().flex_none().text_color(accent()).child("$"))
                .child(div().flex_1().min_w(px(0.)).text_color(code_text()).child(command.to_string())),
        )
        .into_any_element();
    }
    if let (Some(old), Some(new)) = (field("old_string"), field("new_string")) {
        let mut out = div().py(px(6.)).font_family(mono()).text_size(px(12.5)).line_height(px(20.)).flex().flex_col();
        for line in old.lines().take(RESULT_LINES) {
            out = out.child(div().px(px(12.)).bg(removed_bg()).text_color(red()).child(format!("- {line}")));
        }
        for line in new.lines().take(RESULT_LINES) {
            out = out.child(div().px(px(12.)).bg(added_bg()).text_color(green()).child(format!("+ {line}")));
        }
        return framed(out).into_any_element();
    }
    if let Some(content) = field("content") {
        let lines: Vec<&str> = content.lines().take(RESULT_LINES).collect();
        return framed(mono_body(&lines.join("\n"))).into_any_element();
    }
    if tool.name == "TodoWrite" {
        return todo_rows(&todos(tool)).into_any_element();
    }
    let text = serde_json::to_string_pretty(input).unwrap_or_default();
    let lines: Vec<&str> = text.lines().take(RESULT_LINES).collect();
    framed(mono_body(&lines.join("\n"))).into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_markdown_en_linea_quita_las_marcas() {
        let (text, marks) = inline("Ahora `Workspace::open` detecta **la extensión** y listo");
        assert_eq!(text, "Ahora Workspace::open detecta la extensión y listo");
        // Rangos en bytes: la «ó» ocupa dos.
        assert_eq!(marks, vec![(6..21, Mark::Code), (30..43, Mark::Bold)]);
    }

    #[test]
    fn una_marca_sin_cerrar_queda_como_esta() {
        let (text, marks) = inline("usa ` para el código y 2 ** 3");
        assert_eq!(text, "usa ` para el código y 2 ** 3");
        assert!(marks.is_empty());
    }
}
