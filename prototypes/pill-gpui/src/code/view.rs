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
    canvas, div, prelude::*, px, svg, AnyElement, ClickEvent, ClipboardItem, Context, CursorStyle, Div, DragMoveEvent,
    Focusable, FontWeight, Hsla, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    SharedString, Stateful, Window,
};
use serde_json::Value;

use super::chat::{Item, ToolCall};
use super::config;
use super::backdrop;
use super::enter::Enter;
use super::git::{FileChange, Repo};
use super::{Attachment, CodeView, Menu, Side, COMPOSER, LOOSE};
use crate::kit::chrome;
use super::split::{controls_pane, fit, zone_preview, ChatDrag, Fit};
use super::style::{t, Style};
use crate::kit::Area;

const HEAD_H: f32 = 52.0;
const SIDE_W: f32 = 264.0;
const RIGHT_W: f32 = 380.0;
const DOC_W: f32 = 600.0;
/// Lo menos que puede medir el panel derecho.
const PANEL_MIN_W: f32 = 300.0;
/// Los límites de la barra lateral.
const SIDE_MIN_W: f32 = 200.0;
const SIDE_MAX_W: f32 = 480.0;
/// Lo menos que mide el chat: la barra, el panel derecho y la terminal a la derecha
/// se reparten el resto.
const CHAT_MIN_W: f32 = 420.0;
/// Los rieles (la barra plegada) de Expressive y de Formal y Glass.
pub(super) const RAIL_M3_W: f32 = 80.0;
const RAIL_FLAT_W: f32 = 56.0;
/// Con la barra más angosta que esto, sus filas muestran menos acciones.
pub(super) const SIDE_NARROW_W: f32 = 260.0;

/// Cuánto mide cada columna en este cuadro, repartiendo el ancho de la ventana para que
/// el chat conserve `CHAT_MIN_W`: la barra (o el riel, si no cabe), el panel derecho y la
/// terminal a la derecha. Los `max` son los topes de sus divisores.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Widths {
    pub side: f32,
    pub side_max: f32,
    /// La barra va plegada: la cerró el usuario o la ventana es muy angosta.
    pub rail: bool,
    pub right: f32,
    pub right_max: f32,
    pub term_max: f32,
}

/// El reparto (ver `Widths`). `room` es lo que queda fuera del mínimo del chat; cada
/// `*_want` es lo que pide esa columna (0 si no está), `side_want` ya dentro de sus topes.
pub(super) fn split_widths(room: f32, sidebar_open: bool, side_want: f32, rail_w: f32, right_want: f32, term_want: f32) -> Widths {
    let right_min = if right_want > 0. { PANEL_MIN_W } else { 0. };
    let term_min = if term_want > 0. { TERM_MIN_W } else { 0. };
    // Sin lugar para la barra completa junto a los mínimos de los paneles: el riel.
    let rail = !sidebar_open || room - right_min - term_min < SIDE_MIN_W;
    let side = if rail { rail_w } else { side_want.min(room - right_min - term_min) };
    let right_max = (room - side - term_min).max(PANEL_MIN_W);
    let right = if right_want > 0. { right_want.clamp(PANEL_MIN_W, right_max) } else { 0. };
    let term_max = (room - side - right).max(TERM_MIN_W);
    let term = if term_want > 0. { term_want.min(term_max) } else { 0. };
    let side_max = (room - right - term).clamp(SIDE_MIN_W, SIDE_MAX_W);
    Widths { side, side_max, rail, right, right_max, term_max }
}

/// El mínimo de la terminal a la derecha (`terminal::MIN_WIDTH`).
const TERM_MIN_W: f32 = 280.0;
pub(super) const THREAD_W: f32 = 860.0;
/// La columna de la pantalla de inicio de Expressive.
const HERO_W: f32 = 720.0;
/// Cuántas partes del final del hilo entran animadas.
pub(super) const ENTER_MAX: usize = 3;
const HERO_PLACEHOLDER: &str = "Pregunta lo que quieras · @ para mencionar · / para acciones";
/// Sin proyecto no hay archivos que mencionar.
const HERO_PLACEHOLDER_LOOSE: &str = "Pregunta lo que quieras · / para acciones";
/// Las claves de las anclas del panel derecho (`side_morph`).
const SIDE_CHANGES: u64 = 0;
const SIDE_FILES: u64 = 1;
/// Las sugerencias del inicio: ícono, etiqueta y lo que dejan escrito.
const SUGGESTIONS: [(&str, &str, &str); 4] = [
    ("layers", "Explícame el proyecto", "Explícame cómo está organizado este proyecto y por dónde empezar."),
    ("diff", "Revisa mis cambios", "Revisa mis cambios sin commitear y dime si ves algún problema."),
    ("check", "Escribe pruebas", "Escribe pruebas para "),
    ("search", "Busca errores", "Busca errores probables en "),
];
/// Lo que se muestra del resultado de una herramienta abierta.
const RESULT_LINES: usize = 40;

/// Lo que un panel necesita saber de sí mismo para dibujarse.
#[derive(Clone, Copy)]
struct PaneCtx {
    pane: usize,
    fit: Fit,
    /// Ancho del panel, para repartir el encabezado.
    width: f32,
    /// Lleva los botones de la ventana (el de arriba a la derecha, sin panel derecho).
    controls: bool,
    maximized: bool,
    /// Hay más de un panel: se puede cerrar este.
    closable: bool,
}

impl PaneCtx {
    /// Margen a los lados del hilo y de la caja.
    fn side_pad(&self) -> f32 {
        match self.fit {
            Fit::Wide => 32.,
            Fit::Narrow => 20.,
            Fit::Tight => 14.,
        }
    }
}

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
/// El fondo de un panel del chat; con Liquid Glass por piezas lo pone AppKit
/// (si GPUI lo pintara, taparía el vidrio de la caja de texto).
fn center_fill(el: Div, radius: f32) -> Div {
    if backdrop::pieces() {
        el.child(backdrop::slot(backdrop::Piece::Fill { radius, color: center_bg() }))
    } else {
        el.bg(center_bg())
    }
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
        self.note_frame();
        self.ensure_probe(cx);
        self.load_missing_history(cx);
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
        let agent = self.current_agent().and_then(super::acp::agent).map_or("Claude", |agent| agent.name);
        let placeholder = match (hero, self.mentions_enabled()) {
            (true, true) => HERO_PLACEHOLDER.to_string(),
            (true, false) => HERO_PLACEHOLDER_LOOSE.to_string(),
            _ => format!("Responde a {agent}…"),
        };
        if self.composer.read(cx).placeholder() != placeholder {
            self.composer.update(cx, |area, cx| area.set_placeholder(placeholder, cx));
        }
        let maximized = window.is_maximized();
        self.focus_pending_editor(window, cx);
        self.sync_terminal_dirs(cx);
        let widths = self.layout_widths(window, cx);
        self.widths.set(widths);
        self.terminals.update(cx, |terminals, _| terminals.max_width = Some(widths.term_max));
        // Formal y Glass: el botón de permiso destacado toma el foco (sin quitárselo a la
        // caja si se está escribiendo).
        if let Some(id) = self.perm_autofocus.borrow_mut().take() {
            let handle = self.perm_focus.borrow().get(&id).cloned();
            if let (Some(handle), false) = (handle, self.composer_focused) {
                handle.focus(window);
                self.perm_autofocused.borrow_mut().insert(id);
            }
        }
        // La pestaña «Agentes» a la vista (por clic, búsqueda o al abrir): sus versiones.
        if self.settings_open && self.settings_tab == super::settings_search::AGENTS_TAB && self.agent_versions.is_empty() && !self.versions_loading {
            self.load_agent_versions(cx);
        }
        let right_open = self.side.is_some() || self.doc.is_some() || self.tabs.showing();
        // La configuración de Expressive es un diálogo: sale animado.
        let settings_exit = gpui_m3::motion::presence("settings-presence", self.settings_open, window, cx);
        let pieces = backdrop::pieces();
        // Algo de GPUI encima de la cabecera: los controles nativos se esconden
        // (AppKit los dibuja sobre todo lo de GPUI).
        let mut covered = self.palette_open || self.settings_open || self.close_confirm;
        if pieces {
            // El panel derecho sale del botón que lo abrió (Cambios o Archivos).
            let key = match self.side {
                Some(Side::Changes) => SIDE_CHANGES,
                Some(Side::Files) => SIDE_FILES,
                _ => u64::MAX,
            };
            self.side_morph.borrow_mut().set_open(right_open, key);
        }
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
            // Con Liquid Glass por piezas, el fondo de la ventana lo pone AppKit.
            .when(!pieces, |el| el.bg(t.bg))
            .when(pieces, |el| el.child(backdrop::slot(backdrop::Piece::Fill { radius: 0., color: t.bg })))
            .font_family(t.font)
            .text_color(fg())
            .text_size(px(t.fs))
            .track_focus(&self.focus)
            // Con paneles flotantes, aire alrededor del chat y del panel derecho.
            .when(t.gap > 0., |el| el.py(px(t.gap)).pr(px(t.gap)))
            .map(|el| match (expressive(), self.sidebar_open) {
                (true, _) => el.child(self.sidebar_m3(window, cx)),
                (false, true) if !self.widths.get().rail => el.child(self.sidebar(window, cx)),
                // Plegada por el usuario o porque la ventana no da para la barra completa.
                (false, _) => el.child(self.rail_flat(cx)),
            })
            .child(self.chat_area(window, maximized && !right_open, !right_open, cx))
            .when(self.terminals.read(cx).dock() == super::terminal::Dock::Right, |el| el.child(self.terminals.clone()))
            .when(right_open, |el| el.child(self.right_panel(window, maximized, cx)))
            .when_some(self.menu_layer(window, cx), |el, menu| {
                covered = true;
                el.child(menu)
            })
            .when_some(self.session_menu_layer(window, cx), |el, menu| {
                covered = true;
                el.child(menu)
            })
            .when_some(self.file_menu_layer(window, cx), |el, menu| {
                covered = true;
                el.child(menu)
            })
            .when_some(self.space_menu_layer(window, cx), |el, menu| {
                covered = true;
                el.child(menu)
            })
            .when_some(self.new_space_dialog(window, cx), |el, dialog| {
                covered = true;
                el.child(dialog)
            })
            .when_some(self.close_dialog(window, cx), |el, dialog| {
                covered = true;
                el.child(dialog)
            })
            .when(self.close_confirm, |el| el.child(self.confirm_close_dialog(cx)))
            .when_some(self.pop_layer(window, cx), |el, pop| {
                covered = true;
                el.child(pop)
            })
            .when_some(self.mention_layer(window, cx), |el, list| el.child(list))
            .when_some(self.profile_layer(window, cx), |el, card| {
                covered = true;
                el.child(card)
            })
            .when_some(self.style_menu_layer(window, cx), |el, card| {
                covered = true;
                el.child(card)
            })
            .when_some(self.crop_dialog(window, cx), |el, dialog| {
                covered = true;
                el.child(dialog)
            })
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
            .when(pieces, |el| {
                el.child(crate::glass_pieces::morph_ghost(self.side_morph.clone(), t.r_pane))
            })
            .child(backdrop::commit(t.light))
            .child(crate::native_controls::commit(covered))
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
        .flex_none()
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(7.))
        .rounded(px(t.r_btn.min(15.)))
        .text_size(px(13.))
        .text_color(muted())
        // Con Liquid Glass por piezas, cada píldora es un vidrio del sistema.
        .when(backdrop::pieces(), |el| el.child(backdrop::slot(backdrop::Piece::Glass { radius: t.r_btn.min(15.) })))
        .when(t.style != Style::Formal && !backdrop::pieces(), |el| el.border_1().border_color(line()).bg(t.control))
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

impl CodeView {
    /// «Copiar» bajo un mensaje en Formal y Glass: copia y dice «Copiado» con un ✓ durante
    /// 1,4 s, como el botón de Expressive (y se queda a la vista mientras tanto).
    fn flat_copy(&self, index: usize, text: String, group: SharedString, cx: &mut Context<Self>) -> Stateful<Div> {
        let copied = self.copied.is_some_and(|(at, _)| at == index);
        div()
            .id(("copy", index))
            .when(!copied, |el| el.invisible().group_hover(group, |el| el.visible()))
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(6.))
            .h(px(24.))
            .rounded(px(6.))
            .cursor_pointer()
            .text_size(px(12.))
            .text_color(if copied { green() } else { muted() })
            .hover(|el| el.bg(hover_bg()))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                let turn = view.copied.map_or(1, |(_, turn)| turn + 1);
                view.copied = Some((index, turn));
                cx.notify();
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(std::time::Duration::from_millis(1400)).await;
                    let _ = this.update(cx, |view, cx| {
                        if view.copied.is_some_and(|(_, t)| t == turn) {
                            view.copied = None;
                            cx.notify();
                        }
                    });
                })
                .detach();
            }))
            .child(svg().path(if copied { "icons/check.svg" } else { "icons/copy.svg" }).size(px(12.)).text_color(if copied { green() } else { muted() }))
            .child(if copied { "Copiado" } else { "Copiar" })
    }
}

/// Un número chico sobre un botón compacto del encabezado (cambios, terminales).
fn count_badge(n: usize) -> gpui::Div {
    div()
        .absolute()
        .top(px(1.))
        .right(px(1.))
        .min_w(px(15.))
        .h(px(15.))
        .px(px(4.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(accent())
        .text_color(on_accent())
        .text_size(px(9.5))
        .font_weight(FontWeight::SEMIBOLD)
        .child(n.to_string())
}

/// Una conversación de la barra en Formal y Glass (`sidebar::session_row` la pide): su
/// título, el marcador si está marcada y, al final, si trabaja (punto), si espera un
/// permiso (punto ámbar), si tiene algo sin leer (punto de acento) o hace cuánto se usó;
/// las abiertas sin guardar llevan «×» para cerrarlas.
#[allow(clippy::too_many_arguments)]
pub(super) fn flat_session_row(
    id: SharedString,
    title: String,
    on: bool,
    bookmarked: bool,
    running: bool,
    waiting: bool,
    unread: bool,
    meta: Option<String>,
    close: Option<impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static>,
    open: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> AnyElement {
    let group = SharedString::from(format!("{id}-row"));
    let dot = if waiting {
        Some(amber())
    } else if running || unread {
        Some(accent())
    } else {
        None
    };
    div()
        .id(id.clone())
        .group(group.clone())
        .h(px(30.))
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(8.))
        .rounded(px(7.))
        .cursor_pointer()
        .text_size(px(13.))
        .when(on, |el| el.bg(selected_bg()).text_color(fg()))
        .when(!on, |el| el.text_color(if unread || running { fg() } else { muted() }).hover(|el| el.bg(hover_bg())))
        .when(unread, |el| el.font_weight(FontWeight::SEMIBOLD))
        .on_click(open)
        .when(bookmarked, |el| el.child(svg().path("icons/pin.svg").size(px(11.)).flex_none().text_color(accent())))
        .child(div().flex_1().min_w(px(0.)).truncate().child(title))
        .when_some(meta, |el, meta| el.child(div().flex_none().text_size(px(11.)).text_color(faint()).child(meta)))
        .when_some(dot, |el, dot| el.child(div().size(px(7.)).flex_none().rounded_full().bg(dot)))
        .when_some(close, |el, close| {
            el.child(
                div().invisible().group_hover(group, |el| el.visible()).child(
                    div()
                        .id(SharedString::from(format!("{id}-close")))
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
        })
        .into_any_element()
}

/// «Ver todas (n)» o «Ver menos» bajo una lista de la barra, en Formal y Glass.
pub(super) fn flat_more(id: impl Into<gpui::ElementId>, label: String, on_click: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static) -> AnyElement {
    div()
        .id(id)
        .h(px(28.))
        .px(px(10.))
        .flex()
        .items_center()
        .rounded(px(7.))
        .cursor_pointer()
        .text_size(px(12.5))
        .text_color(faint())
        .hover(|el| el.bg(hover_bg()).text_color(fg()))
        .on_click(on_click)
        .child(label)
        .into_any_element()
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

/// Un chip de mensaje en cola: el relojito, el texto corto, «Enviar ahora» (o «Enviar»
/// si el turno ya se detuvo) y la «×» que lo quita.
fn queued_chip(
    id: u64,
    name: String,
    tip: SharedString,
    busy: bool,
    send: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
    remove: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> AnyElement {
    let t = t();
    let (send_label, send_tip) =
        if busy { ("Enviar ahora", "Detener la respuesta y enviar este mensaje") } else { ("Enviar", "Enviar este mensaje") };
    if expressive() {
        return div()
            .flex()
            .items_center()
            .gap(px(2.))
            .child(
                gpui_m3::Chip::input(("queued", id as usize), name)
                    .leading(svg().path("icons/history.svg").size(px(16.)).text_color(muted()))
                    .on_remove(remove),
            )
            .child(
                gpui_m3::Button::new(("queued-send", id as usize), send_label)
                    .variant(gpui_m3::ButtonVariant::Ghost)
                    .size(gpui_m3::ButtonSize::Tiny)
                    .icon("up")
                    .tooltip(send_tip)
                    .on_click(send),
            )
            .into_any_element();
    }
    div()
        .id(("queued-chip", id as usize))
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
        .tooltip(crate::hover::tip_text(tip))
        .child(svg().path("icons/history.svg").size(px(13.)).text_color(muted()))
        .child(div().max_w(px(260.)).truncate().child(name))
        .child(
            div()
                .id(("queued-send", id as usize))
                .h(px(22.))
                .px(px(8.))
                .flex()
                .items_center()
                .gap(px(4.))
                .rounded_full()
                .cursor_pointer()
                .text_color(accent())
                .hover(|el| el.bg(hover_bg()))
                .tooltip(crate::hover::tip(send_tip))
                .on_click(send)
                .child(svg().path("icons/arrow-up.svg").size(px(11.)).text_color(accent()))
                .child(send_label),
        )
        .child(
            div()
                .id(("queued-x", id as usize))
                .size(px(22.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .cursor_pointer()
                .hover(|el| el.bg(hover_bg()))
                .tooltip(crate::hover::tip("Quitar de la cola"))
                .on_click(remove)
                .child(svg().path("icons/x.svg").size(px(11.)).text_color(muted())),
        )
        .into_any_element()
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
    /// La barra plegada de Formal y Glass: una columna de íconos (el riel de Expressive
    /// con las piezas de estos estilos).
    fn rail_flat(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = t();
        let item = |id: &'static str, icon: &'static str, tip: &'static str, on: bool| {
            icon_button(id, icon, tip).size(px(36.)).rounded(px(r_ctl().min(12.))).when(on, |el| el.bg(accent_soft()))
        };
        div()
            .w(px(RAIL_FLAT_W))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.))
            .pb(px(10.))
            .bg(side_bg())
            .when(t.gap == 0., |el| el.border_r_1().border_color(line()))
            .child(div().h(px(chrome::RAIL_TOP)).w_full().child(chrome::drag(chrome::RAIL_TOP)))
            .child(item("rail-menu", "icons/panel-left.svg", crate::keymap::hint("Barra lateral (Ctrl+B)"), false).on_click(cx.listener(
                |view, _: &ClickEvent, _, cx| {
                    view.sidebar_open = true;
                    cx.notify();
                },
            )))
            .child(item("rail-new", "icons/plus.svg", "Nueva conversación", false).on_click(cx.listener(
                |view, _: &ClickEvent, window, cx| {
                    view.history_page = false;
                    view.new_conversation(window, cx);
                },
            )))
            .child(item("rail-chat", "icons/bubble.svg", "Chat", !self.history_page).on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                view.history_page = false;
                cx.notify();
            })))
            .child(item("rail-history", "icons/history.svg", "Historial", self.history_page).on_click(cx.listener(
                |view, _: &ClickEvent, _, cx| {
                    view.history_page = !view.history_page;
                    if view.history_page {
                        view.load_history(cx);
                    }
                    cx.notify();
                },
            )))
            .child(item("rail-search", "icons/search.svg", crate::keymap::hint("Comandos (Ctrl+K)"), self.palette_open).on_click(cx.listener(
                |view, _: &ClickEvent, window, cx| view.open_palette(window, cx),
            )))
            .child(div().flex_1().w_full().child(chrome::drag(200.)))
            .child(item("rail-style", "icons/highlighter.svg", "Apariencia", false).on_click(cx.listener(|view, event: &ClickEvent, _, cx| {
                view.toggle_style_menu(event.position(), cx);
            })))
            .child(
                div()
                    .id("rail-profile")
                    .cursor_pointer()
                    .tooltip(crate::hover::tip("Tu perfil"))
                    .on_click(cx.listener(|view, event: &ClickEvent, window, cx| view.toggle_profile(event.position(), window, cx)))
                    .child(self.profile_avatar("rail-avatar", px(30.), cx)),
            )
            .child(item("rail-settings", "icons/settings-2.svg", crate::keymap::hint("Configuración (Ctrl+,)"), self.settings_open).on_click(cx.listener(
                |view, _: &ClickEvent, _, cx| {
                    view.settings_open = !view.settings_open;
                    cx.notify();
                },
            )))
    }

    fn sidebar(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Como en Expressive: cada proyecto se pliega con sus conversaciones, se reordena
        // arrastrándolo (los favoritos con los favoritos) y lleva su estrella y su «+».
        let mut projects = gpui_m3::ReorderList::new("projects").gap(px(2.)).grab_height(px(36.));
        for workspace in self.sidebar_ids().into_iter().filter_map(|id| self.workspaces.get(id)) {
            let group = usize::from(!self.configs.favorites.contains(&workspace.id));
            projects = projects.row(format!("project-{}", workspace.id), group, self.project_flat(workspace.id, workspace.name.clone(), workspace.collapsed, window, cx));
        }
        if self.workspaces.list().is_empty() {
            projects = projects.item(
                div()
                    .px(px(10.))
                    .py(px(6.))
                    .text_size(px(13.))
                    .text_color(muted())
                    .child("Agrega una carpeta para empezar."),
            );
        }
        let view = cx.entity().downgrade();
        let projects = projects.on_reorder(move |event, _, cx| {
            view.update(cx, |view, cx| view.reorder_spaces(event, cx)).ok();
        });
        let new_on = self.active.is_none() && self.workspaces.active().is_some();
        let t = t();
        let on_new = cx.listener(|view, _: &ClickEvent, window, cx| {
            view.history_page = false;
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
        // Con la barra angosta, el nombre corto (entero, sin recortar).
        let settings_label = if self.widths.get().side < SIDE_NARROW_W { "Configuración" } else { "Configuración de Claude" };
        let settings_row = if t.style == Style::Expressive {
            gpui_m3::NavItem::new("code-settings", settings_label).icon("gear").on_click(on_settings).into_any_element()
        } else {
            nav_row("code-settings", "icons/settings-2.svg", settings_label, false, false)
                .on_click(on_settings)
                .into_any_element()
        };
        let (width, resize) = self.side_width(cx);
        div()
            .relative()
            .w(px(width))
            .flex_none()
            .flex()
            .flex_col()
            .bg(side_bg())
            .when(t.gap == 0., |el| el.border_r_1().border_color(line()))
            // Liquid Glass por piezas: la barra es un vidrio que flota, como en macOS.
            .when(backdrop::pieces(), |el| el.mx(px(t.gap)).child(backdrop::slot(backdrop::Piece::Glass { radius: t.r_pane })))
            .child(
                div()
                    .h(px(HEAD_H))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .pl(px(16.))
                    .pr(px(8.))
                    .child(chrome::lights())
                    // El logo y el nombre también mueven la ventana: son la barra de título.
                    // Con la barra angosta, el nombre se recorta: la lupa y plegar no se salen.
                    .child(chrome::movable(div().min_w(px(0.)).flex_shrink().flex().items_center().gap(px(10.)).h_full()
                        .child(div().flex_none().child(chrome::logo(20.0)))
                        .child(div().min_w(px(0.)).truncate().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child("Atic Code"))))
                    .child(chrome::drag(HEAD_H))
                    // Buscar (la paleta) y plegar la barra, como en Expressive.
                    .child(icon_action("side-search", "icons/search.svg", crate::keymap::hint("Comandos (Ctrl+K)"), cx.listener(
                        |view, _: &ClickEvent, window, cx| view.open_palette(window, cx),
                    )))
                    .child(icon_action("side-hide", "icons/panel-left.svg", crate::keymap::hint("Barra lateral (Ctrl+B)"), cx.listener(
                        |view, _: &ClickEvent, _, cx| {
                            view.sidebar_open = false;
                            cx.notify();
                        },
                    ))),
            )
            .child(div().px(px(10.)).pt(px(4.)).child(new_chat))
            .child(
                div().px(px(10.)).pt(px(2.)).child(
                    nav_row("nav-history", "icons/history.svg", "Historial", self.history_page, false).on_click(cx.listener(
                        |view, _: &ClickEvent, _, cx| {
                            view.history_page = true;
                            view.load_history(cx);
                            cx.notify();
                        },
                    )),
                ),
            )
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
                    .child(self.loose_chats(window, cx)),
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
                            .child(div().flex_1().min_w(px(0.)).truncate().child(self.profile_name()))
                            // Apariencia, como en el pie de Expressive.
                            .child(icon_action("side-style", "icons/highlighter.svg", "Apariencia", cx.listener(|view, event: &ClickEvent, _, cx| {
                                cx.stop_propagation();
                                view.toggle_style_menu(event.position(), cx);
                            }))),
                    )
                    .child(settings_row),
            )
            .child(resize)
    }

    /// Un proyecto de la barra en Formal y Glass, como `project_m3`: un clic lo pliega o
    /// despliega (también con Tab y Enter o Espacio), al pasar el cursor su estrella de
    /// favorito, «+» para una conversación nueva aquí, agregar carpeta y quitar; el clic
    /// derecho abre su menú.
    fn project_flat(&self, id: u64, name: String, collapsed: bool, window: &mut Window, cx: &mut Context<Self>) -> Div {
        if self.renaming_space == Some(id) {
            return div().child(self.space_rename_field.clone());
        }
        let active = self.workspaces.active_id() == Some(id);
        let in_list = self.active_chat().is_some_and(|c| c.workspace == id);
        let selected = active && !self.history_page && !in_list && collapsed;
        let group = SharedString::from(format!("ws-row-{id}"));
        let favorite = self.configs.favorites.contains(&id);
        let toggle_favorite = cx.listener(move |view, _: &bool, _, cx| {
            view.configs.toggle_favorite(id);
            cx.notify();
        });
        let star = gpui_m3::FavStar::new(("project-fav", id as usize), favorite)
            .hover_group(group.clone())
            .on_toggle(move |on, window, cx| toggle_favorite(&on, window, cx));
        // Las acciones aparecen con el cursor y el nombre se recorta para dejarles lugar
        // (sin cursor no ocupan nada); un favorito deja su estrella siempre a la vista. Con
        // la barra angosta quedan la estrella y «+» (lo demás está en el clic derecho).
        let (star_inline, star_hover) = if favorite { (Some(star), None) } else { (None, Some(star)) };
        let narrow = self.widths.get().side < SIDE_NARROW_W;
        // El cursor sobre la fila se guarda como estado (y redibuja): mostrar u ocultar
        // con `group_hover` cambiaría el layout entre prepaint y paint, y GPUI no lo admite.
        let row_hover = window.use_keyed_state(gpui::ElementId::from(("project-flat-hover", id as usize)), cx, |_, _| false);
        let hovered = *row_hover.read(cx);
        let actions = div()
            .flex_none()
            .flex()
            .items_center()
            .children(star_hover)
            .child(icon_action(
                ("project-new", id),
                "icons/plus.svg",
                "Nueva conversación",
                cx.listener(move |view, _: &ClickEvent, window, cx| {
                    cx.stop_propagation();
                    view.history_page = false;
                    view.new_chat(id, window, cx);
                }),
            ))
            .when(!narrow, |el| {
                el.child(icon_action(
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
                ))
            });
        let chevron = svg()
            .path("icons/chevron-right.svg")
            .size(px(12.))
            .flex_none()
            .text_color(faint())
            .when(!collapsed, |el| el.with_transformation(gpui::Transformation::rotate(gpui::percentage(0.25))));
        let row = nav_row(("ws", id), if collapsed { "icons/folder.svg" } else { "icons/folder-open.svg" }, name, selected, false)
            .group(group)
            .pl(px(6.))
            .children(star_inline)
            .when(hovered, |el| el.child(actions))
            .on_hover(move |on, _, cx| {
                row_hover.update(cx, |hovered, cx| {
                    if *hovered != *on {
                        *hovered = *on;
                        cx.notify();
                    }
                })
            })
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.toggle_project(id, cx)));
        // Con Tab llega el foco a la fila y Enter o Espacio la pliegan, como en Expressive.
        let focus_id = gpui::ElementId::from(("project-focus", id as usize));
        let (focus, handle, ring_visible) = gpui_m3::interaction::use_focus(&focus_id, window, cx);
        let ring = gpui_m3::interaction::focus_ring(&focus_id, ring_visible, gpui::Corners::all(px(r_btn().min(18.))), window, cx);
        let row = gpui_m3::interaction::focusable(div().id(focus_id).relative(), &focus, &handle)
            .on_key_down(cx.listener(move |view, event: &gpui::KeyDownEvent, _, cx| {
                let key = event.keystroke.key.as_str();
                if !event.keystroke.modifiers.modified() && matches!(key, "enter" | "space") {
                    cx.stop_propagation();
                    view.toggle_project(id, cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |view, event: &MouseDownEvent, window, cx| view.open_space_menu(id, event.position, window, cx)),
            )
            .children(ring)
            .child(div().flex().items_center().child(div().w(px(14.)).flex_none().flex().justify_center().child(chevron)).child(div().flex_1().min_w(px(0.)).child(row)));
        let mut block = div().flex().flex_col().child(row);
        if !collapsed {
            block = block.child(self.project_convs(id, window, cx));
        }
        block
    }

    // --- Centro: encabezado, chat y caja de texto ---------------------------------------

    fn center(&self, ctx: PaneCtx, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let chat = self.active_chat();
        let (maximized, controls) = (ctx.maximized, ctx.controls);
        // Según el ancho del panel, los botones pasan a solo ícono (con su tooltip) y,
        // más angosto aún, se quitan los que tienen atajo.
        let compact = ctx.fit != Fit::Wide;
        let tight = ctx.fit == Fit::Tight;
        let pane = ctx.pane;
        let has_workspace = self.workspaces.active().is_some();
        let loose = chat.is_some_and(|c| c.workspace == LOOSE);
        let title = if self.history_page {
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
                .flex()
                .flex_none()
                .items_center()
                .gap(px(7.))
                .rounded(px(r_btn().min(17.)))
                .cursor_pointer()
                .when(compact, |el| el.w(px(34.)).justify_center().tooltip(crate::hover::tip(label)))
                .when(!compact, |el| el.px(px(12.)))
                .when(on, |el| el.bg(accent_soft()).text_color(on_accent_soft()))
                .when(!on, |el| el.text_color(fg()).hover(|el| el.bg(hover_bg())))
                .child(svg().path(icon).size(px(15.)).text_color(if on { on_accent_soft() } else { muted() }))
                .when(!compact, |el| el.child(label))
        };
        let glass = t().style == Style::Glass;
        // En Liquid Glass por piezas (macOS 26), Terminal y Cambios/Archivos son
        // controles de AppKit: el vidrio, el brillo y la selección del sistema.
        let native = cfg!(target_os = "macos") && glass && backdrop::pieces();
        let terminal = if native { self.terminal_native(compact, window, cx) } else { self.terminal_button(compact, cx) };
        let native_tabs = (native && has_workspace && !loose && !tight).then(|| self.side_tabs_native(compact, changed, window, cx));
        // Con la conversación empezada, el título se renombra con un clic (como en la referencia).
        let renamable = !self.history_page && chat.is_some_and(|c| c.session_id.is_some() && !c.items.is_empty());
        let renaming_here = self.rename_in_header && self.renaming.is_some();
        let title_el: AnyElement = if renaming_here {
            div().w(px((ctx.width - 220.).clamp(120., 320.))).child(self.rename_field.clone()).into_any_element()
        } else if renamable {
            div()
                .id("header-title")
                .group("header-title")
                .max_w(px(420.))
                .min_w(px(0.))
                .flex_shrink()
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
            chrome::movable(div().max_w(px(420.)).min_w(px(0.)).flex_shrink().truncate().font_weight(FontWeight::SEMIBOLD).child(title)).into_any_element()
        };
        let header = div()
            .h(px(HEAD_H))
            .flex_none()
            .flex()
            .items_center()
            .overflow_hidden()
            .gap(px(if tight { 8. } else { 14. }))
            .pl(px(if compact { 14. } else { 22. }))
            .child(title_el)
            .when_some(project.filter(|_| !compact), |el, project| el.child(chrome::movable(div().flex_none().text_color(faint()).child(project))))
            .child(chrome::drag(HEAD_H))
            .child(terminal)
            .when_some(native_tabs, |el, tabs| el.child(tabs))
            .when(has_workspace && !loose && !tight && !native, |el| {
                el.child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(px(4.))
                        .mr(px(4.))
                        // En vidrio, las pestañas van juntas en una cápsula.
                        .when(glass && backdrop::pieces(), |el| el.p(px(3.)).rounded(px(20.)).child(backdrop::slot(backdrop::Piece::Glass { radius: 20. })))
                        .when(glass && !backdrop::pieces(), |el| el.p(px(3.)).rounded(px(20.)).border_1().border_color(line()).bg(t().control))
                        .when(expressive(), |el| {
                            let mut changes = gpui_m3::Button::new("tab-changes", "Cambios")
                                .icon("branch")
                                .text()
                                .size(gpui_m3::ButtonSize::Small)
                                .icon_only(compact)
                                .tooltip(crate::keymap::hint("Cambios (Ctrl+G)"))
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
                                    .icon_only(compact)
                                    .tooltip(crate::keymap::hint("Archivos (Ctrl+E)"))
                                    .selected(self.side == Some(Side::Files))
                                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.toggle_side(Side::Files, cx))),
                            )
                        })
                        .when(!expressive(), |el| el.child(
                            header_tab("tab-changes", "icons/git-branch.svg", "Cambios", self.side == Some(Side::Changes))
                                .relative()
                                .child(crate::glass_pieces::morph_anchor(self.side_morph.clone(), SIDE_CHANGES, 17.))
                                // En compacto el número queda en una burbuja, como en Expressive.
                                .when(changed > 0 && compact, |el| el.relative().child(count_badge(changed)))
                                .when(changed > 0 && !compact, |el| {
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
                                .relative()
                                .child(crate::glass_pieces::morph_anchor(self.side_morph.clone(), SIDE_FILES, 17.))
                                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.toggle_side(Side::Files, cx))),
                        )),
                )
            })
            .when(ctx.closable, |el| {
                el.child(div().flex_none().child(icon_action(
                    ("pane-close", pane),
                    "icons/x.svg",
                    "Cerrar este panel",
                    cx.listener(move |view, _: &ClickEvent, window, cx| view.close_pane(pane, window, cx)),
                )))
            })
            .child(div().flex_none().w(px(if controls { 4. } else { 10. })))
            .when(controls, |el| el.child(chrome::controls_colored(maximized, HEAD_H, fg(), hover_bg())));

        // Sin conversación: la invitación a empezar (el hilo de verdad es una lista virtual,
        // `thread_list.rs`).
        let empty = chat.is_none().then(|| {
            let text = if has_workspace {
                "¿En qué trabajamos? Claude Code tiene acceso a las carpetas de este proyecto."
            } else {
                "Crea un proyecto con el botón de carpeta, junto a «Proyectos»."
            };
            div()
                .pt(px(140.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(14.))
                .child(svg().path("icons/agents/claude.svg").size(px(40.)).text_color(faint()))
                .child(div().text_size(px(15.)).text_color(muted()).child(text))
        });
        let error = self.error.clone();
        let t = t();
        // Sin conversación, el inicio de la referencia con la caja al centro, en los tres
        // estilos (`Hero`, `Chat.tsx:199`); las formas animadas son solo de Expressive.
        let history = self.history_page;
        // Un chat suelto vacío también muestra el inicio, como en la referencia.
        let hero = !history && chat.is_none_or(|c| c.workspace == LOOSE && c.items.is_empty() && c.session_id.is_none());
        div()
            .flex_1()
            .min_w(px(0.))
            .flex()
            .flex_col()
            .map(|el| center_fill(el, t.r_pane))
            .when(t.gap > 0., |el| {
                el.rounded(px(t.r_pane)).overflow_hidden().when(t.style == Style::Glass, |el| el.border_1().border_color(t.highlight.opacity(0.35)))
            })
            .child(header)
            .when(hero, |el| el.child(self.hero(has_workspace, ctx.fit, window, cx)))
            .when(history, |el| el.child(self.history_view(window, cx)))
            .when(!hero && !history, |el| {
                // Al cambiar de conversación, la otra entra subiendo 14 px con resorte
                // (`animateIn(.., "swap")`, `Thread.tsx:227`).
                let swap = Enter::new(("thread-swap", pane)).from(0., 14.).on_change(chat.map_or(0, |c| key_hash(&c.key)));
                let body = match (chat, empty) {
                    (Some(chat), _) => self.thread_list(chat, loose && chat.items.is_empty(), self.thread_side(&ctx), cx),
                    (None, Some(empty)) => empty.into_any_element(),
                    (None, None) => div().into_any_element(),
                };
                el.child(swap.apply(div().id(("pane-thread", pane)).flex_1().min_h(px(0.)), window, cx).child(body))
            })
            .when_some(error, |el, error| {
                el.child(
                    div().w_full().when(super::style::chat_width() > 0., |el| el.max_w(px(super::style::chat_width())).mx_auto()).px(px(32.)).child(
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
            .when((has_workspace || loose) && !hero && !history, |el| el.child(self.composer_box(chat.is_some_and(|c| c.busy), false, ctx.fit, window, cx)))
            // La terminal va bajo el chat, como en la referencia (`Chat.tsx`); a la
            // derecha la pone `render`, junto al chat.
            .when(self.terminals.read(cx).dock() == super::terminal::Dock::Bottom, |el| el.child(self.terminals.clone()))
    }

    /// El margen lateral del hilo: el del estilo más lo que haga falta para dejar la
    /// columna en el ancho de lectura elegido (`style::chat_width`), centrada.
    fn thread_side(&self, ctx: &PaneCtx) -> f32 {
        let side = ctx.side_pad();
        let width = super::style::chat_width();
        if width > 0. { side + (ctx.width - width).max(0.) / 2. } else { side }
    }

    /// Los paneles del chat (`split.rs`): cada uno con su conversación, en filas y
    /// columnas, con las rayas para cambiarles el tamaño y, mientras se arrastra una
    /// conversación de la barra, las zonas donde soltarla.
    fn chat_area(&mut self, window: &mut Window, maximized: bool, controls: bool, cx: &mut Context<Self>) -> impl IntoElement {
        if self.dragging_chat && !cx.has_active_drag() {
            self.dragging_chat = false;
            self.pane_state.hint = None;
        }
        self.sync_panes();
        let gap = super::split::gap();
        let area = self.pane_area(window);
        let rects = self.panes.layout(area, gap);
        let multi = rects.len() > 1;
        let corner = controls_pane(&rects);
        let focused = self.panes.focused;
        // El activo se dibuja al final: anota las marcas de su hilo (`flag_bounds`) después de
        // que los demás no puedan pisarlas.
        let mut order: Vec<&(usize, Area)> = rects.iter().collect();
        order.sort_by_key(|(pane, _)| *pane == focused);

        let bounds = self.pane_state.bounds.clone();
        let view = cx.weak_entity();
        let mut root = div()
            .id("chat-area")
            .relative()
            .flex_1()
            .min_w(px(0.))
            // Una raya agarrada sigue al cursor aunque se salga de ella.
            .on_mouse_move(cx.listener(|view, event: &MouseMoveEvent, window, cx| {
                if view.drag_resize(event.position, event.pressed_button == Some(MouseButton::Left), window) {
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|view, _: &MouseUpEvent, _, cx| {
                    if view.pane_state.resizing.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|view, _: &MouseUpEvent, _, cx| {
                    if view.pane_state.resizing.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            // Mide el área de los paneles; si cambia de tamaño, se vuelve a repartir.
            .child(
                canvas(
                    move |b, _, cx| {
                        let changed = bounds.get().is_none_or(|old| old.size != b.size);
                        bounds.set(Some(b));
                        if changed {
                            let view = view.clone();
                            cx.defer(move |cx| {
                                let _ = view.update(cx, |_, cx| cx.notify());
                            });
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            );
        for (pane, rect) in order {
            let (pane, rect) = (*pane, *rect);
            let ctx = PaneCtx {
                pane,
                fit: fit(rect.w),
                width: rect.w,
                controls: controls && corner == Some(pane),
                maximized,
                closable: multi,
            };
            let content = if pane == focused { self.center(ctx, window, cx).into_any_element() } else { self.split_pane(ctx, window, cx) };
            let mut wrapper = div().id(("pane", pane)).absolute().left(px(rect.x)).top(px(rect.y)).w(px(rect.w)).h(px(rect.h)).flex().child(content);
            if pane != focused {
                // Un clic en cualquier parte del panel lo vuelve el activo, antes de que lo
                // reciba lo que haya debajo (un botón del hilo, por ejemplo).
                wrapper = wrapper.capture_any_mouse_down(cx.listener(move |view, _: &MouseDownEvent, window, cx| view.focus_pane(pane, window, cx)));
            }
            if self.dragging_chat {
                wrapper = wrapper.child(self.drop_overlay(pane, rect, cx));
            }
            root = root.child(wrapper);
        }
        for (index, divider) in self.panes.dividers(area, gap).into_iter().enumerate() {
            let cursor = match divider.axis {
                crate::kit::panes::Axis::Row => CursorStyle::ResizeLeftRight,
                crate::kit::panes::Axis::Column => CursorStyle::ResizeUpDown,
            };
            let grip = divider.grip;
            root = root.child(
                div()
                    .id(("pane-divider", index))
                    .absolute()
                    .left(px(grip.x))
                    .top(px(grip.y))
                    .w(px(grip.w))
                    .h(px(grip.h))
                    .cursor(cursor)
                    .hover(|el| el.bg(accent().opacity(0.3)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |view, _: &MouseDownEvent, _, cx| {
                            view.start_resize(&divider);
                            cx.notify();
                        }),
                    ),
            );
        }
        root
    }

    /// Las zonas de un panel para soltar una conversación: el contorno, y sobre la que
    /// está bajo el cursor, lo que ocuparía.
    fn drop_overlay(&self, pane: usize, rect: Area, cx: &mut Context<Self>) -> Stateful<Div> {
        let radius = t().r_pane.max(12.);
        let hint = self.pane_state.hint.filter(|(p, _)| *p == pane).map(|(_, zone)| zone);
        let mut overlay = div()
            .id(("pane-drop", pane))
            .absolute()
            .occlude()
            .inset_0()
            .rounded(px(radius))
            .border_2()
            .border_color(accent().opacity(0.35))
            .on_drag_move::<ChatDrag>(cx.listener(move |view, event: &DragMoveEvent<ChatDrag>, _, cx| {
                let want = event.bounds.contains(&event.event.position).then(|| (pane, view.zone_under(rect, event.event.position)));
                match want {
                    Some(want) if view.pane_state.hint != Some(want) => {
                        view.pane_state.hint = Some(want);
                        cx.notify();
                    }
                    None if view.pane_state.hint.is_some_and(|(p, _)| p == pane) => {
                        view.pane_state.hint = None;
                        cx.notify();
                    }
                    _ => {}
                }
            }))
            .on_drop(cx.listener(move |view, drag: &ChatDrag, window, cx| {
                let zone = view.zone_under(rect, window.mouse_position());
                view.drop_chat(drag, pane, zone, window, cx);
            }));
        if let Some(zone) = hint {
            let preview = zone_preview(rect.w, rect.h, zone);
            overlay = overlay.child(
                div()
                    .absolute()
                    .left(px(preview.x))
                    .top(px(preview.y))
                    .w(px(preview.w))
                    .h(px(preview.h))
                    .p(px(10.))
                    .child(
                        div()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(radius))
                            .border_2()
                            .border_color(accent().opacity(0.6))
                            .bg(accent().opacity(0.14))
                            .text_color(accent())
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(zone.label()),
                    ),
            );
        }
        overlay
    }

    /// Un panel que no es el activo: su título, el hilo entero y una caja que, con un
    /// clic en cualquier parte del panel, lo vuelve el activo.
    fn split_pane(&self, ctx: PaneCtx, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let t = t();
        let pane = ctx.pane;
        let key = self.panes.card_in(pane);
        let chat = key.as_ref().and_then(|k| self.chats.iter().find(|c| &c.key == k));
        let title: SharedString = chat.map_or_else(|| "Nueva conversación".into(), |c| c.title.clone().into());
        let header = div()
            .h(px(HEAD_H))
            .flex_none()
            .flex()
            .items_center()
            .overflow_hidden()
            .gap(px(10.))
            .pl(px(if ctx.fit == Fit::Wide { 22. } else { 14. }))
            .child(chrome::movable(div().min_w(px(0.)).flex_shrink().truncate().font_weight(FontWeight::SEMIBOLD).text_color(muted()).child(title)))
            .child(chrome::drag(HEAD_H))
            .child(div().flex_none().mr(px(if ctx.controls { 4. } else { 14. })).child(icon_action(
                ("pane-close", pane),
                "icons/x.svg",
                "Cerrar este panel",
                cx.listener(move |view, _: &ClickEvent, window, cx| view.close_pane(pane, window, cx)),
            )))
            .when(ctx.controls, |el| el.child(chrome::controls_colored(ctx.maximized, HEAD_H, fg(), hover_bg())));
        let body = match chat {
            Some(chat) => self.thread_list(chat, false, self.thread_side(&ctx), cx),
            None => div().pt(px(80.)).px(px(16.)).text_center().text_color(faint()).child("Sin conversación: arrastra una desde la barra o haz clic para escribir aquí.").into_any_element(),
        };
        let _ = window;
        // La caja de este panel: al enfocarlo, la caja de verdad pasa aquí.
        let reply = div()
            .id(("pane-reply", pane))
            .mx(px(16.))
            .mb(px(14.))
            .h(px(52.))
            .px(px(18.))
            .flex()
            .flex_none()
            .items_center()
            .overflow_hidden()
            .rounded(px(if expressive() { 26. } else { r_card() }))
            .bg(t.control)
            .text_color(faint())
            .cursor_text()
            .hover(|el| el.bg(t.control2))
            .child(div().min_w(px(0.)).truncate().child({
                let agent = chat.map_or("Claude", |c| c.agent_name());
                if chat.is_some_and(|c| c.busy) {
                    format!("{agent} está trabajando… · clic para escribir aquí")
                } else {
                    format!("Responde a {agent}… · clic para escribir aquí")
                }
            }));
        div()
            .flex_1()
            .min_w(px(0.))
            .flex()
            .flex_col()
            .map(|el| center_fill(el, t.r_pane))
            .when(t.gap > 0., |el| el.rounded(px(t.r_pane)).overflow_hidden().when(t.style == Style::Glass, |el| el.border_1().border_color(t.highlight.opacity(0.35))))
            .child(header)
            .child(div().id(("pane-thread", pane)).flex_1().min_h(px(0.)).child(body))
            .child(reply)
            .into_any_element()
    }

    /// Lo que va al final del hilo: el modelo del próximo mensaje, las solicitudes de
    /// permiso (todas, como en la referencia) y «Trabajando…».
    pub(super) fn thread_footer(&self, chat: &super::Chat, cx: &mut Context<Self>) -> AnyElement {
        let mut footer = div().flex().flex_col().gap(px(if expressive() { 12. } else { 16. }));
        // Se eligió otro modelo con la conversación empezada: se grabará con el próximo mensaje.
        if let Some(pending) = chat.pending_model(&self.models, &self.configs.get(chat.workspace)) {
            footer = footer.child(model_mark(format!("model-next-{}", chat.key), format!("{pending} en el próximo mensaje"), true));
        }
        if !chat.permissions.is_empty() {
            footer = footer.child(self.permission_cards(chat, cx));
        }
        if chat.working() {
            footer = footer.child(
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
                            el.child(div().size(px(7.)).rounded_full().bg(accent())).child(format!("{} está trabajando…", chat.agent_name()))
                        }
                    }),
            );
        }
        footer.into_any_element()
    }

    /// «Terminal» como control de AppKit (Liquid Glass por piezas).
    fn terminal_native(&self, compact: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let terminals = self.terminals.read(cx);
        let (on, count) = (terminals.shown(), terminals.count());
        let segment = crate::native_controls::Segment {
            label: if count > 1 { format!("Terminal {count}") } else { "Terminal".into() },
            symbol: "terminal",
            tip: crate::keymap::hint("Terminal (Ctrl+J)").into(),
            selected: on,
        };
        crate::native_controls::segmented("terminal", vec![segment], compact, t().light, window, cx, |view, _, window, cx| {
            view.toggle_terminal(window, cx)
        })
        .into_any_element()
    }

    /// Cambios y Archivos como un control segmentado de AppKit. El panel derecho
    /// sigue saliendo de su segmento (`side_morph`).
    fn side_tabs_native(&self, compact: bool, changed: usize, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        use crate::native_controls::Segment;
        let segments = vec![
            Segment {
                label: if changed > 0 { format!("Cambios {changed}") } else { "Cambios".into() },
                symbol: "arrow.triangle.branch",
                tip: if changed > 0 && compact {
                    crate::keymap::hint_owned(format!("Cambios: {changed} (Ctrl+G)"))
                } else {
                    crate::keymap::hint("Cambios (Ctrl+G)").into()
                },
                selected: self.side == Some(Side::Changes),
            },
            Segment {
                label: "Archivos".into(),
                symbol: "folder",
                tip: crate::keymap::hint("Archivos (Ctrl+E)").into(),
                selected: self.side == Some(Side::Files),
            },
        ];
        let anchor = |key| div().flex_1().h_full().relative().child(crate::glass_pieces::morph_anchor(self.side_morph.clone(), key, 17.));
        crate::native_controls::segmented("side-tabs", segments, compact, t().light, window, cx, |view, index, _, cx| {
            view.toggle_side(if index == 0 { Side::Changes } else { Side::Files }, cx)
        })
        .mr(px(4.))
        .child(div().absolute().top_0().left_0().size_full().flex().child(anchor(SIDE_CHANGES)).child(anchor(SIDE_FILES)))
        .into_any_element()
    }

    /// «Terminal» en la barra superior, con cuántas hay si son varias (`Chat.tsx:82`).
    fn terminal_button(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let terminals = self.terminals.read(cx);
        let (on, count) = (terminals.shown(), terminals.count());
        if expressive() {
            let mut button = gpui_m3::Button::new("tab-terminal", "Terminal")
                .icon("terminal")
                .text()
                .size(gpui_m3::ButtonSize::Small)
                .icon_only(compact)
                .tooltip(crate::keymap::hint("Terminal (Ctrl+J)"))
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
            .flex()
            .flex_none()
            .items_center()
            .when(compact, |el| el.w(px(34.)).justify_center())
            .when(!compact, |el| el.px(px(12.)))
            .gap(px(7.))
            .rounded(px(r_btn().min(17.)))
            .cursor_pointer()
            .tooltip(crate::hover::tip(crate::keymap::hint("Terminal (Ctrl+J)")))
            .when(on, |el| el.bg(accent_soft()).text_color(on_accent_soft()))
            .when(!on, |el| el.text_color(fg()).hover(|el| el.bg(hover_bg())))
            .child(svg().path("icons/square-terminal.svg").size(px(15.)).text_color(if on { on_accent_soft() } else { muted() }))
            .when(!compact, |el| el.child("Terminal"))
            .when(count > 1 && !compact, |el| el.child(div().text_size(px(11.)).text_color(faint()).child(count.to_string())))
            .when(count > 1 && compact, |el| el.relative().child(count_badge(count)))
            .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.toggle_terminal(window, cx)))
            .into_any_element()
    }

    /// El inicio: las formas (solo Expressive), «¿Qué construimos hoy?», el proyecto,
    /// la caja de texto y las sugerencias. Formal y Glass usan sus tokens: el proyecto
    /// es un chip sin borde y las sugerencias, chips con borde (de vidrio en Glass).
    fn hero(&self, has_workspace: bool, fit: Fit, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        let m3 = expressive();
        let t = t();
        let title = div()
            .mb(px(if m3 { 18. } else { 14. }))
            .text_center()
            .text_size(px(if m3 { 32. } else { 26. }))
            .line_height(px(if m3 { 40. } else { 32. }))
            .font_weight(FontWeight(if m3 { 620. } else { 600. }))
            .child("¿Qué construimos hoy?");
        let loose = self.in_loose_chat();
        let project = if loose {
            "Sin proyecto".to_string()
        } else {
            self.workspaces.active().map(|w| w.name.clone()).unwrap_or_else(|| "Elegir proyecto".into())
        };
        // En Expressive, tonal; en Formal y Glass, el `chip ghost` de la referencia.
        let (picker_fg, picker_icon) = if m3 { (scheme.on_primary_container, scheme.on_primary_container) } else { (fg(), muted()) };
        let picker = div()
            .id("hero-project")
            .h(px(if m3 { 32. } else { 28. }))
            .pl(px(8.))
            .pr(px(10.))
            .flex()
            .items_center()
            .gap(px(6.))
            .text_color(picker_fg)
            .text_size(px(13.))
            .cursor_pointer()
            .map(|el| {
                if m3 {
                    el.rounded(px(8.)).bg(scheme.primary_container).font_weight(FontWeight::SEMIBOLD)
                } else {
                    el.rounded(px(r_chip())).font_weight(FontWeight::MEDIUM).hover(|el| el.bg(hover_bg()))
                }
            })
            .on_click(cx.listener(|view, event: &ClickEvent, window, cx| {
                // En macOS, el menú del sistema (`NSMenu`); en los demás, el dibujado.
                if cfg!(target_os = "macos") {
                    view.native_project_menu(event.position(), window, cx);
                } else {
                    view.toggle_menu(Menu::Project, event.position(), cx);
                }
            }))
            .child(Icon::new(if loose { "chat" } else { "folder" }).size(px(14.)).color(picker_icon))
            .child(project)
            .child(Icon::new("chevron-down").size(px(12.)).color(picker_icon));
        let mut suggestions = div().mt(px(16.)).flex().flex_wrap().justify_center().gap(px(8.));
        for (index, (icon, label, prompt)) in SUGGESTIONS.iter().enumerate() {
            let prefill = cx.listener(move |view, _: &ClickEvent, window, cx| view.prefill(prompt, window, cx));
            if m3 {
                suggestions = suggestions.child(gpui_m3::Chip::new(("suggest", index), *label).icon(*icon).on_click(prefill));
                continue;
            }
            // `.suggestion` de la referencia: con borde en Formal; en Glass, de vidrio.
            let glass = t.style == Style::Glass;
            suggestions = suggestions.child(
                div()
                    .id(("suggest", index))
                    .h(px(32.))
                    .pl(px(10.))
                    .pr(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .text_size(px(12.5))
                    .font_weight(FontWeight::MEDIUM)
                    .cursor_pointer()
                    .map(|el| {
                        if glass && backdrop::pieces() {
                            el.rounded_full().child(backdrop::slot(backdrop::Piece::Glass { radius: 16. })).text_color(fg())
                        } else if glass {
                            el.rounded_full().bg(t.raised).border_1().border_color(t.highlight.opacity(0.35)).text_color(fg())
                        } else {
                            el.rounded(px(r_chip())).border_1().border_color(line()).text_color(muted())
                        }
                    })
                    .hover(|el| el.bg(hover_bg()).text_color(fg()))
                    .on_click(prefill)
                    .child(Icon::new(*icon).size(px(15.)).color(accent()))
                    .child(*label),
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
                    .when(m3, |el| el.child(rise("hero-shapes", 0., div().child(shapes), window, cx)))
                    .child(title)
                    .child(rise("hero-picker", 0.06, div().flex().child(picker), window, cx))
                    .when(has_workspace || loose, |el| {
                        let composer = self.composer_box(false, true, fit, window, cx);
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

    pub(super) fn item(&self, chat: &super::Chat, index: usize, item: &Item, flagged: bool, fresh: bool, cx: &mut Context<Self>) -> AnyElement {
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
                    let mut thumb = gpui_m3::ImageThumb::new(SharedString::from(format!("thumb-{key}-{index}-{n}")), image.clone())
                        .size(px(96.));
                    if let Some((width, height)) = super::chat::image_size(image) {
                        thumb = thumb.aspect(width, height);
                    }
                    thumb
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
                let copy_button = self.flat_copy(index, text.clone(), group.clone(), cx);
                div()
                .group(group.clone())
                .relative()
                .flex()
                .justify_end()
                .child(
                    div()
                        // Con «Completo» la burbuja guarda el tope de siempre: una línea de lado a lado no se lee.
                        .max_w(px(if super::style::chat_width() > 0. { super::style::chat_width() } else { THREAD_W } * 0.78))
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
                            .child(self.flat_copy(index, copy.clone(), group.clone(), cx))
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
            // Las herramientas: la misma tarjeta en los tres estilos (`tools.rs`).
            Item::Tool(tool) => self.tool_m3(tool, fresh, cx),
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
            Item::Task(note) => self.task_note(key, index, note, cx),
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

    /// Terminó un subagente o una tarea: el estado, su nombre y lo que costó en una
    /// línea; lo que respondió, plegado (abre como el razonamiento).
    fn task_note(&self, key: &str, index: usize, note: &super::chat::TaskNote, cx: &mut Context<Self>) -> AnyElement {
        let (icon, color) = match note.status.as_str() {
            "failed" | "error" => ("icons/circle-x.svg", red()),
            "stopped" | "killed" | "cancelled" | "canceled" => ("icons/circle-dot.svg", muted()),
            "event" => ("icons/activity.svg", muted()),
            _ => ("icons/circle-check.svg", green()),
        };
        let meta = note.meta();
        let row = div()
            .flex_1()
            .min_w(px(0.))
            .flex()
            .items_center()
            .gap(px(6.))
            .child(gpui_m3::Icon::new(icon).size(px(13.)).color(color))
            .child(div().min_w(px(0.)).truncate().text_size(px(13.)).text_color(fg()).child(note.heading()))
            .when(!meta.is_empty(), |el| el.child(div().flex_none().text_size(px(12.)).text_color(faint()).child(meta)));
        let Some(result) = note.result.as_deref() else {
            return row.into_any_element();
        };
        let id = format!("task-note-{key}-{index}");
        let open = self.expanded.contains(&id);
        let toggle = id.clone();
        if expressive() {
            let flip = cx.listener(move |view, _: &bool, _, cx| {
                if !view.expanded.remove(&toggle) {
                    view.expanded.insert(toggle.clone());
                }
                cx.notify();
            });
            return gpui_m3::ExpandableCard::new(SharedString::from(id))
                .plain(true)
                .open(open)
                .header(row)
                .on_toggle(move |open, window, cx| flip(&open, window, cx))
                .child(markdown(&format!("{key}-{index}-task"), result, cx))
                .into_any_element();
        }
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
                    .child(row),
            )
            .when(open, |el| el.child(div().ml(px(6.)).pl(px(14.)).border_l_2().border_color(line()).child(markdown(&format!("{key}-{index}-task"), result, cx))))
            .into_any_element()
    }

    fn composer_box(&self, busy: bool, hero: bool, fit: Fit, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let side = match fit {
            Fit::Wide => 32.,
            Fit::Narrow => 20.,
            Fit::Tight => 14.,
        };
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
        // El esfuerzo (de Claude o del agente ACP); sin elegir, nada.
        let effort = (!self.effort_now().is_empty()).then(|| self.effort_text());
        // Con un agente ACP, el chip dice cuál es y su modelo, con nombre legible.
        let acp = self.current_agent().and_then(super::acp::agent);
        let acp_model = self
            .acp_cfg()
            .and_then(|cfg| cfg.options.iter().find(|o| o.category == "model"))
            .and_then(|option| option.choices.iter().find(|c| c.value == option.current))
            .map(|choice| super::acp::model_entry(&choice.value, &choice.name).0);
        let model = match (acp, acp_model) {
            (Some(agent), Some(model)) => format!("{} · {model}", agent.name),
            (Some(agent), None) => agent.name.to_string(),
            (None, _) => model,
        };
        let mode_color = match self.mode_id().as_str() {
            "acceptEdits" | "build" | "agent" => accent(),
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
                Attachment::Image { name, image, size, .. } => {
                    attachments = attachments.child(
                        gpui_m3::ImageThumb::new(("attachment-thumb", index), image.clone())
                            .aspect(size.0, size.1)
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
        // Los mensajes escritos mientras el turno corre: salen solos al terminar (los
        // manda `flush_queue`); cada chip deja enviarlo ya (interrumpiendo) o quitarlo.
        let queued_row: Option<AnyElement> = {
            let chat_busy = self.active_chat().is_some_and(|chat| chat.busy);
            let list: Vec<(u64, String)> = self
                .active_chat()
                .map(|chat| chat.queued.iter().map(|m| (m.id, m.text.split_whitespace().collect::<Vec<_>>().join(" "))).collect())
                .unwrap_or_default();
            (!list.is_empty()).then(|| {
                let mut row = div().flex().flex_wrap().items_center().gap(px(6.)).pb(px(8.));
                for (id, text) in list {
                    let short: String = text.chars().take(42).collect();
                    let short = if text.chars().count() > 42 { format!("{short}…") } else { short };
                    let tip: SharedString = text.clone().into();
                    let send = cx.listener(move |view, _: &ClickEvent, _, cx| view.send_queued_now(id, cx));
                    let remove = cx.listener(move |view, _: &ClickEvent, _, cx| view.dequeue(id, cx));
                    let label = if chat_busy { format!("En cola: {short}") } else { format!("Pendiente: {short}") };
                    let chip = queued_chip(id, label, tip, chat_busy, send, remove);
                    row = row.child(chip_in(format!("queued-in-{id}"), chip, window, cx));
                }
                row.into_any_element()
            })
        };
        if t.style == Style::Expressive {
            let composer = self.composer_m3(busy, ready, attachments, model, effort, branch, fit, cx);
            return if hero {
                composer
            } else {
                div()
                    .w_full()
                    .when(super::style::chat_width() > 0., |el| el.max_w(px(super::style::chat_width())).mx_auto())
                    .px(px(side))
                    .pb(px(16.))
                    .when_some(queued_row, |el, row| el.child(row))
                    .child(composer)
            };
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
                // Por piezas, el vidrio del sistema es el fondo, el borde y la sombra.
                Style::Glass if backdrop::pieces() => el.rounded(px(24.)).child(backdrop::slot(backdrop::Piece::Glass { radius: 24. })),
                Style::Glass => el.rounded(px(24.)).border_1().border_color(t.highlight.opacity(0.5)).bg(t.raised).shadow(float_shadow()),
            })
            .when(has_chips, |el| el.child(attachments))
            .when_some(queued_row, |el, row| el.child(row))
            .child(super::mention::with_keys(div().key_context(COMPOSER).on_action(cx.listener(Self::send)).h(input_h), cx).child(self.composer.clone()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        tool_button("composer-attach", "icons/plus.svg", crate::keymap::hint("Adjuntar archivos o imágenes (también Ctrl+V)"), false)
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
                            .min_w(px(0.))
                            .flex_shrink()
                            .overflow_hidden()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .rounded(px(t.r_btn.min(16.)))
                            .cursor_pointer()
                            .when(menu_open(Menu::Model), |el| el.bg(t.sel))
                            .when(!menu_open(Menu::Model), |el| el.hover(|el| el.bg(hover_bg())))
                            .tooltip(crate::hover::tip("Modelo y esfuerzo"))
                            .on_click(cx.listener(|view, event: &ClickEvent, _, cx| view.toggle_menu(Menu::Model, event.position(), cx)))
                            .child(div().min_w(px(0.)).truncate().font_weight(FontWeight::SEMIBOLD).child(model))
                            .when_some(effort, |el, effort| el.child(div().flex_none().text_color(faint()).child(effort)))
                            .child(svg().path("icons/chevron-down.svg").size(px(12.)).text_color(faint())),
                    )
                    .child(div().flex_1())
                    .when(starting && fit != Fit::Tight, |el| el.child(div().mr(px(8.)).text_size(px(12.)).text_color(faint()).child("Conectando…")))
                    // El contexto usado: abre «Cuenta y uso», como en Expressive.
                    .when(self.current_agent().is_none(), |el| el.child(div().mr(px(6.)).child(self.context_ring(cx))))
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
            // En el inicio la columna ya la pone `hero` (`HERO_W`).
            .when(!hero, |el| el.when(super::style::chat_width() > 0., |el| el.max_w(px(super::style::chat_width())).mx_auto()).px(px(side)).pb(px(12.)))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(self.banners(cx))
            .child(self.drop_zone("composer-drop", px(if t.style == Style::Glass { 24. } else { r_card() }), card, cx))
            .child(
                // Una sola línea: si no cabe, se recorta el resumen del turno en vez de
                // mandar «agentes» y Remote Control a una fila que queda fuera de la ventana.
                div()
                    .flex()
                    .items_center()
                    .min_w(px(0.))
                    .overflow_hidden()
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
        effort: Option<String>,
        branch: Option<String>,
        fit: Fit,
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
        // Sin `truncate`: GPUI 0.2 mide en cero un texto que se puede cortar y el botón
        // quedaba en «Medio». En un panel angosto se saca el esfuerzo en vez de cortar.
        let mut model_button = gpui_m3::Button::new("composer-model", model)
            .size(gpui_m3::ButtonSize::Compact)
            .open(model_open)
            .on_click(cx.listener(|view, event: &ClickEvent, _, cx| view.toggle_menu(Menu::Model, event.position(), cx)));
        if let Some(effort) = effort.filter(|_| fit == Fit::Wide) {
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
                    .tooltip(crate::keymap::hint("Adjuntar (Ctrl+U)"))
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
            .when(starting && fit != Fit::Tight, |el| el.child(div().flex_none().mr(px(6.)).text_size(px(12.)).text_color(t.muted).child("Conectando…")))
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
        let mode_id = self.mode_id();
        let plan = mode_id == "plan";
        let (shape, color) = match mode_id.as_str() {
            "acceptEdits" | "build" | "agent" => (gpui_m3::DotShape::Circle, scheme.primary),
            "plan" => (gpui_m3::DotShape::Diamond, scheme.tertiary),
            "bypassPermissions" => (gpui_m3::DotShape::Square, scheme.error),
            _ => (gpui_m3::DotShape::Square, scheme.outline),
        };
        let mode_tip = if self.current_agent().is_some() { "Modo del agente (clic para cambiar)" } else { "Modo de permisos (clic para cambiar)" };
        let mode = div().id("status-mode-wrap").tooltip(crate::hover::tip(mode_tip)).child(
            gpui_m3::Chip::new("status-mode", self.mode_label_now(&config))
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
            .when_some(branch, |el, branch| el.child(Chip::new("status-branch", branch).icon("branch").muted(true).inert(true).max_w(px(200.))))
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
                .tooltip(crate::hover::tip(if self.current_agent().is_some() { "Modo del agente" } else { "Modo de permisos" }))
                .on_click(cx.listener(|view, event: &ClickEvent, window, cx| {
                    // En macOS, el menú del sistema (`NSMenu`, con su Liquid Glass); los modos de
                    // un agente ACP siguen con el dibujado.
                    if cfg!(target_os = "macos") && !expressive() && view.current_agent().is_none() {
                        view.native_mode_menu(event.position(), window, cx);
                    } else {
                        view.toggle_menu(Menu::Mode, event.position(), cx);
                    }
                }))
                .child(div().size(px(8.)).rounded_full().bg(mode_color))
                .child(self.mode_label_now(config)),
        )
        .when_some(last_turn, |el, turn| {
            el.child(
                pill("status-turn")
                    .flex_shrink()
                    .min_w(px(0.))
                    .tooltip(crate::hover::tip_text(turn.clone().into()))
                    .child(svg().path("icons/history.svg").size(px(13.)).flex_none().text_color(muted()))
                    .child(div().min_w(px(0.)).truncate().child(turn)),
            )
        })
        .when_some(self.session_duration(), |el, duration| {
            el.child(
                pill("status-duration")
                    .tooltip(crate::hover::tip("Duración de la sesión"))
                    .child(svg().path("icons/coffee.svg").size(px(13.)).text_color(muted()))
                    .child(duration),
            )
        })
        .children(self.flat_agent_pills(cx))
        .when_some(self.status_text().filter(|_| self.configs.status_line), |el, line| {
            el.child(div().ml_auto().min_w(px(0.)).truncate().font_family(mono()).text_size(px(11.)).text_color(faint()).child(line))
        })
    }

    /// Cuánto lleva abierta la conversación a la vista («12m», «1h 5m»).
    fn session_duration(&self) -> Option<String> {
        self.active_chat().filter(|chat| !chat.items.is_empty()).map(|chat| {
            let minutes = chat.seen_at.elapsed().as_secs() / 60;
            if minutes >= 60 { format!("{}h {}m", minutes / 60, minutes % 60) } else { format!("{minutes}m") }
        })
    }

    /// Formal y Glass: el mapa de agentes, Remote Control y el aviso de límite, con las
    /// píldoras de estos estilos (en Expressive son los chips de `agent_chips`). Remote
    /// Control y el límite son de Claude: con un agente ACP no van.
    fn flat_agent_pills(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        use super::usage::Pop;
        let t = t();
        let chat = self.active_chat();
        let running = chat.map(super::chat::Chat::running_tasks).unwrap_or(0);
        let label = if running == 1 { "1 agente".to_string() } else { format!("{running} agentes") };
        let agents_on = self.pop == Some(Pop::Agents);
        let mut pills = vec![pill("agents-chip")
            .cursor_pointer()
            .when(agents_on, |el| el.bg(t.sel).text_color(fg()))
            .hover(|el| el.bg(hover_bg()))
            .tooltip(crate::hover::tip("Mapa de agentes: subagentes y tareas en segundo plano"))
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.toggle_pop(Pop::Agents, cx)))
            .child(svg().path("icons/layers.svg").size(px(13.)).text_color(if running > 0 { accent() } else { muted() }))
            .child(label)
            .into_any_element()];
        if self.current_agent().is_some() {
            return pills;
        }
        let config = self.config();
        let connected = chat.is_some_and(|c| c.remote_url.is_some() || c.remote_state.as_deref() == Some("connected"));
        let (dot, tip) = match (config.remote_control, connected) {
            (false, _) => (faint(), "Activar Remote Control: seguir tus conversaciones desde claude.ai/code y la app móvil"),
            (true, true) => (green(), "Conectada: esta conversación aparece en claude.ai/code y la app móvil (clic para desactivar)"),
            (true, false) => (amber(), "Remote Control activo; conectando esta conversación…"),
        };
        pills.push(
            pill("rc-chip")
                .cursor_pointer()
                .when(config.remote_control, |el| el.text_color(fg()))
                .hover(|el| el.bg(hover_bg()))
                .tooltip(crate::hover::tip(tip))
                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                    let on = !view.config().remote_control;
                    view.set_config(|c| c.remote_control = on, cx);
                    view.show_toast(
                        if on { "Remote Control activado: tus conversaciones aparecen en claude.ai/code y la app" } else { "Remote Control desactivado" },
                        cx,
                    );
                }))
                .child(div().size(px(7.)).rounded_full().bg(dot))
                .child("Remote Control")
                .into_any_element(),
        );
        if let Some(alert) = &self.rate_alert {
            let tip = if alert.rejected { "Límite alcanzado: clic para ver cuándo se reinicia" } else { "Te acercas al límite de uso: clic para ver el detalle" };
            let color = if alert.rejected { red() } else { amber() };
            pills.push(
                pill("rate-chip")
                    .cursor_pointer()
                    .text_color(color)
                    .hover(|el| el.bg(hover_bg()))
                    .tooltip(crate::hover::tip(tip))
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.toggle_pop(Pop::Usage, cx)))
                    .child(svg().path("icons/gauge.svg").size(px(13.)).text_color(color))
                    .child(format!("{} · {}%", alert.label, alert.percent.round()))
                    .into_any_element(),
            );
        }
        pills
    }

    /// La línea de estado de la conversación visible (la referencia la muestra solo con una).
    fn status_text(&self) -> Option<String> {
        let chat = self.active_chat()?;
        Some(super::status_line(&self.model_label(&self.chat_model()), chat.context, chat.total_cost, chat.last_duration_ms))
    }

    // --- Derecha: cambios, archivos y visor -----------------------------------------

    fn right_panel(&mut self, window: &mut Window, maximized: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let fade = if backdrop::pieces() { self.side_morph.borrow().fade() } else { 1. };
        let editing = self.doc.is_none() && self.tabs.showing();
        let doc_open = self.doc.is_some() || editing;
        // El ancho que dejó el usuario (uno para las listas y otro para el visor).
        let Widths { right: width, right_max: max_w, .. } = self.widths.get();
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
                    .when(doc_open, |el| el.max_w(px((width - 330.).max(80.))).truncate())
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
                let open = cx.listener(|view, _: &ClickEvent, _, cx| {
                    if let Some(doc) = view.doc.take() {
                        view.open_file(doc.path, cx);
                    }
                });
                if expressive() {
                    el.child(gpui_m3::Button::new("doc-toggle", "Abrir").icon("pen").outlined().size(gpui_m3::ButtonSize::Small).on_click(open))
                } else {
                    el.child(chip("doc-toggle", "Abrir", false).on_click(open))
                }
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
        // El borde izquierdo se arrastra para cambiar el ancho; se guarda al soltar.
        let this = cx.weak_entity();
        let resize = gpui_m3::ResizeHandle::width("panel-resize", px(width))
            .min(px(PANEL_MIN_W))
            .max(px(max_w))
            .invert(true)
            .on_resize({
                let this = this.clone();
                move |size, _, cx| {
                    let _ = this.update(cx, |view, cx| {
                        view.set_panel_width(doc_open, f32::from(size), false);
                        cx.notify();
                    });
                }
            })
            .on_resize_end(move |size, _, cx| {
                let _ = this.update(cx, |view, cx| {
                    view.set_panel_width(doc_open, f32::from(size), true);
                    cx.notify();
                });
            });
        // En Expressive el panel entra desde la derecha con resorte (`m3-panel`, `motion.css:444`).
        let enter = Enter::new("right-panel-enter").from(32., 0.);
        if expressive() { enter.apply(div(), window, cx) } else { div() }
            .relative()
            .w(px(width))
            .flex_none()
            .flex()
            .flex_col()
            .map(|el| {
                if !backdrop::pieces() {
                    return el.bg(t.pane);
                }
                // Liquid Glass por piezas: un vidrio que sale del botón de la cabecera; el
                // contenido aparece cuando ya casi llegó.
                el.child(crate::glass_pieces::morph_slot(self.side_morph.clone(), t.r_pane))
            })
            .map(|el| {
                if t.gap > 0. {
                    el.ml(px(t.gap)).rounded(px(t.r_pane)).overflow_hidden().when(t.style == Style::Glass && !backdrop::pieces(), |el| {
                        el.border_1().border_color(t.highlight.opacity(0.35))
                    })
                } else {
                    el.border_l_1().border_color(line())
                }
            })
            .child(header.opacity(fade))
            .child(div().flex_1().min_h(px(0.)).flex().flex_col().opacity(fade).child(body))
            .child(div().absolute().top_0().bottom_0().left_0().w(px(8.)).child(resize))
    }

    /// Reparte el ancho de la ventana entre las columnas (`Widths`) para este cuadro.
    fn layout_widths(&self, window: &Window, cx: &mut Context<Self>) -> Widths {
        let t = t();
        let viewport = f32::from(window.viewport_size().width);
        // Con paneles flotantes, el aire entre ellos tampoco es del chat.
        let room = viewport - CHAT_MIN_W - t.gap * 4.;
        let side_default = if expressive() { super::sidebar::SIDE_W } else { SIDE_W };
        let side_want = self.configs.sidebar_width.unwrap_or(side_default).clamp(SIDE_MIN_W, SIDE_MAX_W);
        let rail_w = if expressive() { RAIL_M3_W } else { RAIL_FLAT_W };
        let right_open = self.side.is_some() || self.doc.is_some() || self.tabs.showing();
        let doc_open = self.doc.is_some() || self.tabs.showing();
        let right_want = match (right_open, doc_open) {
            (false, _) => 0.,
            (true, true) => self.configs.doc_width.unwrap_or(DOC_W),
            (true, false) => self.configs.panel_width.unwrap_or(RIGHT_W),
        };
        let term_want = self.terminals.read(cx).right_width().unwrap_or(0.);
        split_widths(room, self.sidebar_open, side_want, rail_w, right_want, term_want)
    }

    /// El ancho de la barra lateral (el del reparto) y el divisor de su borde derecho,
    /// que se arrastra para cambiarlo y lo guarda al soltar.
    pub(super) fn side_width(&self, cx: &mut Context<Self>) -> (f32, AnyElement) {
        let Widths { side: width, side_max: max_w, .. } = self.widths.get();
        let this = cx.weak_entity();
        let resize = gpui_m3::ResizeHandle::width("side-resize", px(width))
            .min(px(SIDE_MIN_W))
            .max(px(max_w))
            .on_resize({
                let this = this.clone();
                move |size, _, cx| {
                    let _ = this.update(cx, |view, cx| {
                        view.configs.sidebar_width = Some(f32::from(size).round());
                        cx.notify();
                    });
                }
            })
            .on_resize_end(move |size, _, cx| {
                let _ = this.update(cx, |view, cx| {
                    view.configs.sidebar_width = Some(f32::from(size).round());
                    view.configs.save();
                    cx.notify();
                });
            });
        (width, div().absolute().top_0().bottom_0().right_0().w(px(8.)).child(resize).into_any_element())
    }

    /// El ancho del panel derecho mientras se arrastra su borde; al soltar, se guarda.
    fn set_panel_width(&mut self, doc_open: bool, width: f32, save: bool) {
        let width = Some(width.round());
        if doc_open {
            self.configs.doc_width = width;
        } else {
            self.configs.panel_width = width;
        }
        if save {
            self.configs.save();
        }
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
            let menu_path = file.path.clone();
            let on_menu = cx.listener(move |view, event: &MouseDownEvent, window, cx| view.open_file_menu(menu_path.clone(), false, event.position, window, cx));
            let selected = self.active_file.as_ref().is_some_and(|p| super::same_path(p, &file.path));
            let color = kind_color(super::files::kind_of(&name));
            if expressive() {
                list = list.child(div().on_mouse_down(MouseButton::Right, on_menu).child(
                    gpui_m3::TreeRow::new(("found", row), name.clone())
                        .icon("file")
                        .when_some(color, |r, c| r.icon_color(c))
                        .dimmed(name.starts_with('.'))
                        .selected(selected)
                        .guides(false)
                        .tooltip(file.path.display().to_string())
                        .trailing(div().max_w(px(160.)).truncate().text_size(px(11.5)).text_color(faint()).child(dir))
                        .on_click(open),
                ));
                continue;
            }
            list = list.child(
                div()
                    .id(("found", row))
                    .tooltip(crate::hover::tip_text(SharedString::from(file.path.display().to_string())))
                    .h(px(30.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(selected, |el| el.bg(selected_bg()))
                    .hover(|el| el.bg(hover_bg()))
                    .on_mouse_down(MouseButton::Right, on_menu)
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
                let menu_path = row.path.clone();
                list = list.child(
                    div().on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |view, event: &MouseDownEvent, window, cx| view.open_file_menu(menu_path.clone(), dir, event.position, window, cx)),
                    ).child(
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
                    ),
                );
                continue;
            }
            let path = row.path.clone();
            let menu_path = row.path.clone();
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
                    .tooltip(crate::hover::tip_text(SharedString::from(row.path.display().to_string())))
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
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |view, event: &MouseDownEvent, window, cx| view.open_file_menu(menu_path.clone(), dir, event.position, window, cx)),
                    )
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
fn doc_diff(doc: &crate::kit::viewer::Doc, lines: &std::sync::Arc<Vec<crate::kit::viewer::DiffLine>>) -> std::rc::Rc<gpui_m3::Diff> {
    thread_local! {
        static LAST: std::cell::RefCell<Option<(std::sync::Arc<Vec<crate::kit::viewer::DiffLine>>, std::rc::Rc<gpui_m3::Diff>)>> =
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

    #[test]
    fn el_chat_conserva_su_minimo_al_repartir_el_ancho() {
        // Ventana ancha: cada columna mide lo que pidió.
        let wide = split_widths(1600. - CHAT_MIN_W, true, 300., 56., 600., 0.);
        assert_eq!((wide.side, wide.right, wide.rail), (300., 600., false));
        // Una barra muy ancha no deja al panel derecho bajo su mínimo, y su divisor lo sabe.
        let crowded = split_widths(1000. - CHAT_MIN_W, true, 480., 56., 600., 0.);
        assert_eq!(crowded.side, 1000. - CHAT_MIN_W - PANEL_MIN_W);
        assert_eq!(crowded.right, PANEL_MIN_W);
        assert!(crowded.side + crowded.right <= 1000. - CHAT_MIN_W);
        // Sin lugar para la barra completa: el riel.
        let narrow = split_widths(800. - CHAT_MIN_W, true, 264., 56., 380., 0.);
        assert!(narrow.rail);
        assert_eq!(narrow.side, 56.);
        // La terminal a la derecha también entra en la cuenta.
        let term = split_widths(1400. - CHAT_MIN_W, true, 264., 56., 380., 900.);
        assert_eq!(term.term_max, 1400. - CHAT_MIN_W - 264. - 380.);
    }
}

/// El aviso de un chat suelto todavía vacío.
pub(super) fn loose_notice() -> AnyElement {
    div()
        .pt(px(140.))
        .flex()
        .flex_col()
        .items_center()
        .gap(px(14.))
        .child(svg().path("icons/agents/claude.svg").size(px(40.)).text_color(faint()))
        .child(div().text_size(px(15.)).text_color(muted()).child("Chat sin proyecto: una conversación general, fuera de tus carpetas."))
        .into_any_element()
}
