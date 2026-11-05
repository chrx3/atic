//! La barra lateral, como la de la referencia: «Nueva conversación»,
//! Historial, los proyectos (cada uno se pliega y muestra sus últimas
//! conversaciones) y abajo el perfil, Apariencia y Configuración. Plegada
//! queda el riel de M3. También la página de historial y el menú contextual
//! de una conversación (abrir, renombrar, eliminar). Las listas de conversaciones
//! (`project_convs`, `loose_chats`, `session_row`) son las de los tres estilos: Formal y
//! Glass dibujan sus filas con `view::flat_session_row` y `view::flat_more`.

use crate::i18n::tf;
use gpui::{
    anchored, deferred, div, point, prelude::*, px, AnyElement, ClickEvent, Context, Corner, Corners, Div, ElementId, FontWeight, KeyDownEvent,
    MouseButton, MouseDownEvent, Pixels, Point, SharedString, Window,
};
use gpui_m3::interaction::{focus_ring, focusable, use_focus};
use gpui_m3::{
    apply_reorder, Avatar, Badge, Button, Chip, Dialog, Fab, FavStar, IconButton, LoadingIndicator, MenuItem, NavItem, RailItem, ReorderEvent, ReorderList,
    Tone,
};
use serde_json::{json, Value};

use super::enter::{stagger, Enter};
use super::marks::{bucket_of, Bucket};
use super::style::t;
use super::chat::RowState;
use super::{CodeView, SessionInfo, LOOSE};
use gpui_m3::shapes::ShapeName;

/// El ancho de fábrica de la barra de Expressive (el usuario lo cambia arrastrando).
pub(super) const SIDE_W: f32 = 244.;
const RAIL_W: f32 = super::view::RAIL_M3_W;
const TOP_H: f32 = 48.;
/// Conversaciones a la vista bajo cada proyecto.
const SHOWN: usize = 3;
/// Chats sueltos a la vista.
const LOOSE_SHOWN: usize = 5;
/// Conversaciones activas a la vista en la barra sin carpetas.
const LIST_SHOWN: usize = 20;
/// La repisa «Hechas» de la barra sin carpetas (no es un proyecto: solo una clave).
const ALL_KEY: u64 = u64::MAX - 1;

/// Una conversación sobre la que se abrió el menú contextual o se renombra.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SessionRef {
    pub workspace: u64,
    pub session_id: String,
    pub title: String,
}

/// La etiqueta del estado con la hora detrás («Lista · 12 h»); sin estado, solo la hora.
fn state_meta(state: super::chat::RowState, time: Option<String>) -> Option<String> {
    match (state.label(), time) {
        (Some(label), Some(time)) => Some(format!("{label} · {time}")),
        (Some(label), None) => Some(label.to_string()),
        (None, time) => time,
    }
}

/// «ahora», «5 min», «3 h», «2 d»; acepta segundos o milisegundos.
pub(super) fn ago(time: f64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.);
    let seconds = if time > 1e12 { time / 1000. } else { time };
    let elapsed = (now - seconds).max(0.);
    if elapsed < 60. {
        crate::i18n::t("pill.code.sidebar.now").into()
    } else if elapsed < 3600. {
        format!("{} min", (elapsed / 60.) as u64)
    } else if elapsed < 86400. {
        format!("{} h", (elapsed / 3600.) as u64)
    } else {
        format!("{} d", (elapsed / 86400.) as u64)
    }
}

fn section(label: &'static str, action: impl IntoElement) -> Div {
    div()
        .h(px(30.))
        .mt(px(16.))
        .pl(px(10.))
        .pr(px(4.))
        .flex()
        .items_center()
        .justify_between()
        .text_size(px(12.))
        .font_weight(FontWeight::BOLD)
        .text_color(t().accent)
        .child(label)
        .child(action)
}

/// Una conversación de la barra: su proyecto, la guardada (si la hay) y la abierta (si la hay).
type Conv<'a> = (u64, Option<&'a SessionInfo>, Option<&'a super::Chat>);

/// Las conversaciones de un grupo en el orden de `list` (los ids que no están, en su orden de
/// siempre y arriba: son las nuevas).
fn arrange<'a>(mut convs: Vec<Conv<'a>>, list: &[String]) -> Vec<Conv<'a>> {
    convs.sort_by_key(|(_, info, _)| info.and_then(|info| list.iter().position(|s| *s == info.session_id)).map_or(0, |at| at + 1));
    convs
}

/// La conversación que delegó en esta, si es un subagente.
fn parent_key<'a>(chat: Option<&'a super::Chat>) -> Option<&'a str> {
    chat.and_then(|c| c.delegation.as_ref()).map(|d| d.parent.as_str())
}

/// Saca de los grupos (Fijadas, Trabajando, Activas, Hechas) los subagentes cuyo padre se ve
/// en la barra (en las tres primeras, dentro del tope `limit` de las Activas), o cuyo padre
/// es otro subagente que va anidado: se dibujan bajo él (`subagent_rows`). Los demás quedan
/// en su grupo como cualquier conversación. No toca el orden de los que quedan.
fn nest_subagents<'a>(groups: &mut [Vec<Conv<'a>>; 4], limit: usize) -> Vec<Conv<'a>> {
    let key = |conv: &Conv<'a>| conv.2.map(|c| c.key.clone());
    let mut visible: std::collections::HashSet<String> =
        groups[0].iter().chain(groups[1].iter()).chain(groups[2].iter().take(limit)).filter_map(key).collect();
    let mut nested: Vec<String> = Vec::new();
    // Hijo y nieto: a lo más dos vueltas, más una que no cambia nada.
    loop {
        let found: Vec<String> = groups
            .iter()
            .flatten()
            .filter(|conv| parent_key(conv.2).is_some_and(|parent| visible.contains(parent)))
            .filter_map(key)
            .filter(|child| !nested.contains(child))
            .collect();
        if found.is_empty() {
            break;
        }
        for child in found {
            visible.insert(child.clone());
            nested.push(child);
        }
    }
    let mut out = Vec::new();
    for group in groups.iter_mut() {
        let (inner, kept): (Vec<Conv<'a>>, Vec<Conv<'a>>) = std::mem::take(group).into_iter().partition(|conv| key(conv).is_some_and(|k| nested.contains(&k)));
        *group = kept;
        out.extend(inner);
    }
    out
}

/// La fecha de una sesión en segundos (el sidecar la da en segundos o en milisegundos).
fn seconds(time: Option<f64>) -> f64 {
    let time = time.unwrap_or(0.);
    if time > 1e12 {
        time / 1000.
    } else {
        time
    }
}

/// Dos proyectos con las mismas carpetas leen las mismas sesiones: en la lista de todos, cada
/// conversación va una vez, en el proyecto de su chat abierto si lo tiene (`opened_in`).
fn dedupe_sessions<'a>(all: Vec<(u64, &'a SessionInfo)>, opened_in: impl Fn(&str) -> Option<u64>) -> Vec<(u64, &'a SessionInfo)> {
    let mut out: Vec<(u64, &'a SessionInfo)> = Vec::with_capacity(all.len());
    for (workspace, info) in all {
        match out.iter_mut().find(|(_, seen)| seen.session_id == info.session_id) {
            Some(seen) => {
                if opened_in(&info.session_id) == Some(workspace) {
                    seen.0 = workspace;
                }
            }
            None => out.push((workspace, info)),
        }
    }
    out
}

/// El título pequeño de un grupo de conversaciones (Fijadas, Trabajando, Activas). `muted` y
/// no `faint`: sobre el fondo claro de Formal y de Liquid Glass `faint` no se lee.
fn group_label(text: impl Into<SharedString>) -> Div {
    div().px(px(10.)).pt(px(8.)).pb(px(2.)).text_size(px(11.)).font_weight(FontWeight::MEDIUM).text_color(t().muted).child(text.into())
}

fn hint(text: &'static str) -> Div {
    div().px(px(10.)).py(px(4.)).text_size(px(12.)).text_color(t().faint).child(text)
}

/// El pie de la lista sin carpetas: «Hechas» y «Pospuestas» justo sobre el perfil. Abiertas
/// crecen hacia arriba hasta un tope y se desplazan solas, sin empujar el perfil fuera.
pub(super) fn shelf_dock(id: &'static str, shelves: Div) -> gpui::Stateful<Div> {
    div().id(id).flex_none().max_h(px(280.)).overflow_y_scroll().px(px(8.)).pt(px(4.)).border_t_1().border_color(t().border.opacity(0.6)).child(shelves)
}

impl CodeView {
    pub(super) fn sidebar_m3(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        // Plegada por el usuario o porque la ventana no da para la barra completa.
        if !self.sidebar_open || self.widths.get().rail {
            return self.rail(cx);
        }
        let top = div()
            .h(px(TOP_H))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.))
            .px(px(10.))
            .child(crate::kit::chrome::lights())
            .child(crate::kit::chrome::drag(TOP_H))
            .child(IconButton::new("side-search", "search").size(px(32.)).tooltip(crate::keymap::hint(crate::i18n::t("pill.code.sidebar.commandsTip"))).on_click(cx.listener(
                |view, _: &ClickEvent, window, cx| view.open_palette(window, cx),
            )))
            .child(IconButton::new("side-hide", "panel-left").size(px(32.)).tooltip(crate::keymap::hint(crate::i18n::t("pill.code.sidebar.sidebarTip"))).on_click(cx.listener(
                |view, _: &ClickEvent, _, cx| {
                    view.sidebar_open = false;
                    cx.notify();
                },
            )));
        let on_new = cx.listener(|view, _: &ClickEvent, window, cx| view.new_conversation(window, cx));
        let nav = div()
            .flex()
            .flex_col()
            .gap(px(1.))
            .px(px(8.))
            .pb(px(4.))
            .child(div().flex().mt(px(2.)).mb(px(8.)).child(Fab::new("chat-new", "plus").label(crate::i18n::t("pill.code.sidebar.newConversation")).hover_radius(22.).on_click(on_new)))
            .child(NavItem::new("nav-history", crate::i18n::t("pill.code.sidebar.history")).icon("history").selected(self.history_page).on_click(cx.listener(
                |view, _: &ClickEvent, _, cx| {
                    view.history_page = true;
                    view.load_history(cx);
                    cx.notify();
                },
            )));

        let list_mode = self.configs.sidebar_list;
        let layout = IconButton::new("side-layout", if list_mode { "folder" } else { "layers" })
            .size(px(32.))
            .tooltip(if list_mode { crate::i18n::t("pill.code.sidebar.layoutFolders") } else { crate::i18n::t("pill.code.sidebar.layoutList") })
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.toggle_sidebar_list(cx)));
        let new_space = IconButton::new("ws-create", "folder-plus")
            .size(px(32.))
            .tooltip(crate::i18n::t("pill.code.sidebar.newSpace"))
            .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.open_new_space(window, cx)));
        let actions = div().flex().items_center().child(layout).child(new_space);
        let scroll = div().id("side-scroll").flex_1().min_h(px(0.)).overflow_y_scroll().px(px(8.)).pb(px(12.));
        let mut bottom = None;
        let scroll = if list_mode {
            // Sin carpetas: todas las conversaciones en una lista, cada una con su proyecto arriba.
            let (list, shelves) = self.conversation_list(window, cx);
            bottom = shelves;
            scroll.child(section(crate::i18n::t("pill.code.sidebar.conversations"), actions)).child(list)
        } else {
            // Arrastrar reordena dentro del grupo: los favoritos (0) se quedan con los favoritos.
            // Solo el encabezado del proyecto lo mueve: sus conversaciones se arrastran al chat.
            let mut projects = ReorderList::new("projects").gap(px(0.)).grab_height(px(44.));
            for id in self.sidebar_ids() {
                if let Some(workspace) = self.workspaces.get(id) {
                    let group = usize::from(!self.configs.favorites.contains(&id));
                    projects = projects.row(format!("project-{id}"), group, self.project_m3(workspace.id, workspace.name.clone(), workspace.collapsed, window, cx));
                }
            }
            if self.workspaces.list().is_empty() {
                projects = projects.item(hint(crate::i18n::t("pill.code.sidebar.addFolderHint")));
            }
            let view = cx.entity().downgrade();
            let projects = projects.on_reorder(move |event, _, cx| {
                view.update(cx, |view, cx| view.reorder_spaces(event, cx)).ok();
            });
            scroll
                .child(section(crate::i18n::t("pill.code.sidebar.projects"), actions))
                .child(projects)
                .child(section(
                    crate::i18n::t("pill.code.sidebar.chats"),
                    IconButton::new("loose-new", "plus")
                        .size(px(32.))
                        .tooltip(crate::i18n::t("pill.code.sidebar.newLooseChat"))
                        .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.new_loose_chat(window, cx))),
                ))
                .child(self.loose_chats(window, cx))
        };
        let (width, resize) = self.side_width(cx);
        // La barra entra deslizándose desde la izquierda con resorte (`m3-side`, `motion.css:444`).
        Enter::new("side-enter")
            .from(-24., 0.)
            .apply(div(), window, cx)
            .relative()
            .w(px(width))
            .flex_none()
            .flex()
            .flex_col()
            .child(top)
            .child(nav)
            .child(scroll)
            .children(bottom.map(|shelves| shelf_dock("side-shelves", shelves)))
            .child(div().p(px(8.)).child(self.side_profile(cx)))
            .child(resize)
            .into_any_element()
    }

    /// Los espacios en el orden de la barra: favoritos primero y luego el que dejó el usuario.
    pub(super) fn sidebar_ids(&self) -> Vec<u64> {
        let ids: Vec<u64> = self.workspaces.list().iter().map(|w| w.id).collect();
        super::config::sidebar_order(&ids, &self.configs.favorites, &self.configs.order)
    }

    /// Un espacio se soltó en otro lugar de la barra: se guarda el orden en `code-claude.json`.
    pub(super) fn reorder_spaces(&mut self, event: ReorderEvent, cx: &mut Context<Self>) {
        let mut order = self.sidebar_ids();
        apply_reorder(&mut order, event);
        self.configs.set_order(order);
        cx.notify();
    }

    /// Pliega o despliega un espacio; al abrirlo carga sus conversaciones si faltan.
    pub(super) fn toggle_project(&mut self, id: u64, cx: &mut Context<Self>) {
        self.workspaces.toggle(id);
        if self.workspaces.get(id).is_some_and(|w| !w.collapsed) && !self.history.contains_key(&id) {
            self.load_history_for(id, cx);
        }
        cx.notify();
    }

    fn project_m3(&self, id: u64, name: String, collapsed: bool, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let active = self.workspaces.active_id() == Some(id);
        let in_list = self.active_chat().is_some_and(|c| c.workspace == id);
        let group = SharedString::from(format!("project-{id}"));
        let new_here = IconButton::new(("project-new", id as usize), "plus").size(px(22.)).tooltip(crate::i18n::t("pill.code.sidebar.newConversation")).on_click(cx.listener(
            move |view, _: &ClickEvent, window, cx| {
                cx.stop_propagation();
                view.history_page = false;
                view.new_chat(id, window, cx);
            },
        ));
        let edit_here = IconButton::new(("project-edit", id as usize), "folder-open").size(px(22.)).tooltip(crate::i18n::t("pill.code.sidebar.editSpaceEllipsis")).on_click(cx.listener(
            move |view, _: &ClickEvent, window, cx| {
                cx.stop_propagation();
                view.open_edit_space(id, window, cx);
            },
        ));
        if self.renaming_space == Some(id) {
            return div().mt(px(2.)).child(self.space_rename_field.clone());
        }
        let favorite = self.configs.favorites.contains(&id);
        let toggle_favorite = cx.listener(move |view, _: &bool, _, cx| {
            view.configs.toggle_favorite(id);
            cx.notify();
        });
        let star = FavStar::new(("project-fav", id as usize), favorite)
            .hover_group(group.clone())
            .on_toggle(move |on, window, cx| toggle_favorite(&on, window, cx));
        // El avatar gira 40° con el cursor sobre toda la fila, no solo sobre él (`motion.css:770`).
        let row_hover = window.use_keyed_state(ElementId::from(("project-hover", id as usize)), cx, |_, _| false);
        let hovered = *row_hover.read(cx);
        let row = NavItem::new(("project", id as usize), name.clone())
            .group(group.clone())
            .leading(Avatar::new(name).size(px(22.)).hover_spin(("project-spin", id as usize)).hovered(hovered))
            .selected(active && !self.history_page && !in_list && collapsed)
            .trailing(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .child(star)
                    .child(div().invisible().group_hover(group, |el| el.visible()).flex().items_center().gap(px(2.)).child(edit_here).child(new_here)),
            )
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.toggle_project(id, cx)));
        // Con Tab llega el foco a la fila y Enter o Espacio la pliegan (`Sidebar.tsx:207`).
        // `NavItem` no toma foco: la envuelve un contenedor que sí.
        let focus_id = ElementId::from(("project-focus", id as usize));
        let (focus, handle, ring_visible) = use_focus(&focus_id, window, cx);
        let ring = focus_ring(&focus_id, ring_visible, Corners::all(px(17.)), window, cx);
        let row = focusable(div().id(focus_id).relative(), &focus, &handle)
            .on_hover(move |on, _, cx| {
                row_hover.update(cx, |hovered, cx| {
                    if *hovered != *on {
                        *hovered = *on;
                        cx.notify();
                    }
                })
            })
            .on_key_down(cx.listener(move |view, event: &KeyDownEvent, _, cx| {
                let key = event.keystroke.key.as_str();
                if !event.keystroke.modifiers.modified() && matches!(key, "enter" | "space") {
                    cx.stop_propagation();
                    view.toggle_project(id, cx);
                }
            }))
            .children(ring)
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |view, event: &MouseDownEvent, window, cx| view.open_space_menu(id, event.position, window, cx)),
            )
            .child(row);
        let mut block = div().mt(px(2.)).flex().flex_col().child(row);
        if !collapsed {
            block = block.child(self.project_convs(id, window, cx));
        }
        block
    }

    /// Los chats sueltos (sin proyecto), al final de la barra: los cinco
    /// últimos y «Ver todos (n)», como en la referencia.
    pub(super) fn loose_chats(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let list = div().flex().flex_col().gap(px(1.)).pb(px(4.));
        let live: Vec<&super::Chat> = self.chats.iter().filter(|c| c.workspace == LOOSE && self.unlisted(c)).collect();
        let sessions: Vec<(u64, &SessionInfo)> = self.visible_sessions(LOOSE).into_iter().map(|info| (LOOSE, info)).collect();
        let total = live.len() + sessions.len();
        if total == 0 {
            return list.child(hint(crate::i18n::t("pill.code.sidebar.looseHint")));
        }
        let limit = if self.loose_all { usize::MAX } else { LOOSE_SHOWN };
        let (mut list, active, settled) = self.grouped_convs(list, live, sessions, limit, false, window, cx);
        if active > LOOSE_SHOWN {
            let label = if self.loose_all { crate::i18n::t("pill.code.sidebar.seeLess").to_string() } else { tf("pill.code.sidebar.seeAll", &[("total", &active)]) };
            let toggle = cx.listener(|view, _: &ClickEvent, _, cx| {
                view.loose_all = !view.loose_all;
                cx.notify();
            });
            list = list.child(if super::view::expressive() {
                NavItem::new("loose-all", label).dense(true).muted(true).on_click(toggle).into_any_element()
            } else {
                super::view::flat_more("loose-all", label, toggle)
            });
        }
        let list = self.shelf(list, LOOSE, settled, false, false, window, cx);
        self.shelf(list, LOOSE, self.snoozed_convs(Some(LOOSE)), true, false, window, cx)
    }

    /// La barra sin carpetas: las conversaciones de todos los proyectos (y los chats sueltos)
    /// en una sola lista, las más recientes primero y cada una con su proyecto arriba. Fijadas,
    /// Trabajando, Activas y Hechas funcionan igual que dentro de cada proyecto.
    /// Devuelve la lista y, aparte, las repisas «Hechas» y «Pospuestas», que van al pie de la
    /// barra, sobre el perfil, para que no queden pegadas a las activas.
    pub(super) fn conversation_list(&self, window: &mut Window, cx: &mut Context<Self>) -> (Div, Option<Div>) {
        let list = div().flex().flex_col().gap(px(1.)).pb(px(4.));
        let shelves = div().flex().flex_col().gap(px(1.));
        let live: Vec<&super::Chat> = self.chats.iter().filter(|c| self.unlisted(c)).collect();
        let sessions = self.scoped_sessions(None);
        let total = live.len() + sessions.len();
        if total == 0 {
            return (list.child(hint(if self.history.is_empty() { crate::i18n::t("pill.code.sidebar.loading") } else { crate::i18n::t("pill.code.sidebar.noConversations") })), None);
        }
        let (mut list, active, settled) = self.grouped_convs(list, live, sessions, LIST_SHOWN, true, window, cx);
        if active > LIST_SHOWN {
            let all = cx.listener(|view, _: &ClickEvent, _, cx| {
                view.history_page = true;
                cx.notify();
            });
            let label = tf("pill.code.sidebar.seeAllChats", &[("total", &total)]);
            list = list.child(if super::view::expressive() {
                NavItem::new("list-all", label).dense(true).muted(true).on_click(all).into_any_element()
            } else {
                super::view::flat_more("list-all", label, all)
            });
        }
        let snoozed = self.snoozed_convs(None);
        if settled.is_empty() && snoozed.is_empty() {
            return (list, None);
        }
        let shelves = self.shelf(shelves, ALL_KEY, settled, false, true, window, cx);
        (list, Some(self.shelf(shelves, ALL_KEY, snoozed, true, true, window, cx)))
    }

    /// Las conversaciones guardadas que se ven en la barra: las de un proyecto (`Some`) o las de
    /// todos, con los chats sueltos, de la más reciente a la más antigua (`None`).
    fn scoped_sessions(&self, scope: Option<u64>) -> Vec<(u64, &SessionInfo)> {
        match scope {
            Some(workspace) => self.visible_sessions(workspace).into_iter().map(|info| (workspace, info)).collect(),
            None => {
                let mut all: Vec<(u64, &SessionInfo)> = self
                    .workspaces
                    .list()
                    .iter()
                    .map(|w| w.id)
                    .chain(std::iter::once(LOOSE))
                    .flat_map(|id| self.visible_sessions(id).into_iter().map(move |info| (id, info)))
                    .collect();
                all.sort_by(|a, b| seconds(b.1.modified).total_cmp(&seconds(a.1.modified)));
                dedupe_sessions(all, |session| self.chats.iter().find(|c| c.session_id.as_deref() == Some(session)).map(|c| c.workspace))
            }
        }
    }

    /// Las pospuestas (salvo la que está abierta), para su repisa.
    fn snoozed_convs(&self, scope: Option<u64>) -> Vec<Conv<'_>> {
        let spaces: Vec<u64> = match scope {
            Some(workspace) => vec![workspace],
            None => self.workspaces.list().iter().map(|w| w.id).chain(std::iter::once(LOOSE)).collect(),
        };
        let mut out: Vec<Conv<'_>> = spaces
            .into_iter()
            .flat_map(|workspace| self.history.get(&workspace).into_iter().flatten().map(move |info| (workspace, info)))
            .filter(|(_, info)| self.is_snoozed(&info.session_id))
            .filter_map(|(workspace, info)| {
                let chat = self.chats.iter().find(|c| c.session_id.as_deref() == Some(info.session_id.as_str()));
                // La abierta sigue a la vista entre las demás: no va a la repisa.
                let open = chat.is_some_and(|c| self.active.as_deref() == Some(c.key.as_str()));
                (!open).then_some((workspace, Some(info), chat))
            })
            .collect();
        out.sort_by(|a, b| seconds(b.1.and_then(|i| i.modified)).total_cmp(&seconds(a.1.and_then(|i| i.modified))));
        out
    }

    /// El grupo de la barra en que cae una conversación guardada.
    fn bucket_for(&self, workspace: u64, session: &str) -> Option<Bucket> {
        let info = self.history.get(&workspace)?.iter().find(|i| i.session_id == session)?;
        Some(self.bucket_of_session(info))
    }

    fn bucket_of_session(&self, info: &SessionInfo) -> Bucket {
        let chat = self.chats.iter().find(|c| c.session_id.as_deref() == Some(info.session_id.as_str()));
        let working = chat.is_some_and(|c| c.in_working_group());
        bucket_of(self.is_bookmarked(&info.session_id), working, self.is_settled(&info.session_id))
    }

    /// Los ids de un grupo (Fijadas o Activas) en el orden en que se ven, para moverlos.
    fn group_sequence(&self, scope: Option<u64>, bucket: Bucket) -> Vec<String> {
        let list = if bucket == Bucket::Pinned { self.marks.pinned() } else { self.marks.order() };
        let mut ids: Vec<&SessionInfo> = self.scoped_sessions(scope).into_iter().map(|(_, info)| info).filter(|info| self.bucket_of_session(info) == bucket).collect();
        ids.sort_by_key(|info| list.iter().position(|s| *s == info.session_id).map_or(0, |at| at + 1));
        ids.into_iter().map(|info| info.session_id.clone()).collect()
    }

    /// ¿Puede esta conversación subir (`-1`) o bajar (`1`) dentro de su grupo?
    fn can_move(&self, workspace: u64, session: &str, scope: Option<u64>, delta: i32) -> bool {
        match self.bucket_for(workspace, session) {
            Some(bucket @ (Bucket::Pinned | Bucket::Active)) => super::marks::moved_ids(&self.group_sequence(scope, bucket), session, delta).is_some(),
            _ => false,
        }
    }

    /// Sube o baja una conversación dentro de su grupo (Fijadas o Activas) y guarda el orden.
    pub(super) fn move_session(&mut self, workspace: u64, session: &str, scope: Option<u64>, delta: i32, cx: &mut Context<Self>) {
        let Some(bucket) = self.bucket_for(workspace, session) else {
            return;
        };
        let Some(next) = super::marks::moved_ids(&self.group_sequence(scope, bucket), session, delta) else {
            return;
        };
        match bucket {
            Bucket::Pinned => self.marks.set_pinned_order(&next),
            Bucket::Active => self.marks.set_active_order(&next),
            _ => return,
        }
        cx.notify();
    }

    /// Se soltó una conversación sobre la fila `target`: queda antes o después de ella según la
    /// mitad donde se soltó, y pasa a su grupo (Fijadas o Activas) si venía de otro. Sobre una
    /// «hecha», la marca como hecha. En la barra con carpetas solo cuenta dentro del mismo proyecto.
    fn drop_on_row(&mut self, drag: &super::split::ChatDrag, target: &str, target_workspace: u64, cx: &mut Context<Self>) {
        let before = self.side_drop.take().filter(|(row, _)| row == target).map_or(true, |(_, before)| before);
        self.dragging_chat = false;
        let Some(dragged) = drag.session.as_ref().map(|s| s.session_id.clone()) else {
            cx.notify();
            return;
        };
        let scope = if self.configs.sidebar_list { None } else { Some(target_workspace) };
        if dragged == target || scope.is_some_and(|workspace| workspace != drag.workspace) {
            cx.notify();
            return;
        }
        match self.bucket_for(target_workspace, target) {
            Some(Bucket::Pinned) => {
                if !self.marks.is_bookmarked(&dragged) {
                    self.marks.toggle_bookmark(&dragged);
                }
                self.place(scope, Bucket::Pinned, &dragged, target, before);
            }
            Some(Bucket::Active) => {
                if self.marks.is_bookmarked(&dragged) {
                    self.marks.toggle_bookmark(&dragged);
                }
                self.marks.unsettle(&dragged);
                self.place(scope, Bucket::Active, &dragged, target, before);
            }
            Some(Bucket::Settled) => self.marks.settle(&dragged),
            _ => {}
        }
        cx.notify();
    }

    /// Deja `dragged` junto a `target` dentro del grupo `bucket` y guarda el orden.
    fn place(&mut self, scope: Option<u64>, bucket: Bucket, dragged: &str, target: &str, before: bool) {
        let mut sequence = self.group_sequence(scope, bucket);
        sequence.retain(|id| id != dragged);
        let Some(at) = sequence.iter().position(|id| id == target) else {
            return;
        };
        sequence.insert(if before { at } else { at + 1 }, dragged.to_string());
        match bucket {
            Bucket::Pinned => self.marks.set_pinned_order(&sequence),
            Bucket::Active => self.marks.set_active_order(&sequence),
            _ => {}
        }
    }

    /// Se soltó una conversación sobre la repisa «Hechas»: queda hecha.
    fn drop_settle(&mut self, drag: &super::split::ChatDrag, cx: &mut Context<Self>) {
        self.dragging_chat = false;
        self.side_drop = None;
        if let Some(session) = drag.session.as_ref() {
            self.settle_session(&session.session_id, cx);
        } else {
            cx.notify();
        }
    }

    /// Se soltó una conversación sobre el título «Fijadas»: queda fijada, arriba de todas.
    fn drop_pin_top(&mut self, drag: &super::split::ChatDrag, cx: &mut Context<Self>) {
        self.dragging_chat = false;
        self.side_drop = None;
        if let Some(session) = drag.session.as_ref() {
            let id = session.session_id.clone();
            if !self.marks.is_bookmarked(&id) {
                self.marks.toggle_bookmark(&id);
            }
            let scope = if self.configs.sidebar_list { None } else { Some(drag.workspace) };
            let mut sequence = self.group_sequence(scope, Bucket::Pinned);
            sequence.retain(|other| *other != id);
            sequence.insert(0, id);
            self.marks.set_pinned_order(&sequence);
        }
        cx.notify();
    }

    /// El título de un grupo que también acepta soltar encima («Fijadas»).
    fn pin_zone(&self, text: &'static str, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("pin-zone")
            .on_drop::<super::split::ChatDrag>(cx.listener(|view, drag: &super::split::ChatDrag, _, cx| view.drop_pin_top(drag, cx)))
            .child(group_label(text))
            .into_any_element()
    }

    /// Abre una conversación en un panel a la derecha del que está enfocado.
    pub(super) fn open_beside(&mut self, target: &SessionRef, window: &mut Window, cx: &mut Context<Self>) {
        let key = self.chats.iter().find(|c| c.session_id.as_deref() == Some(target.session_id.as_str())).map(|c| c.key.clone());
        let info = SessionInfo { session_id: target.session_id.clone(), title: target.title.clone(), modified: None, branch: None };
        let drag = super::split::ChatDrag { key, session: Some(info), workspace: target.workspace, title: target.title.clone().into() };
        let pane = self.panes.focused;
        self.drop_chat(&drag, pane, super::split::Zone::Right, window, cx);
    }

    /// Copia el id de la sesión al portapapeles.
    pub(super) fn copy_session_id(&mut self, session: &str, cx: &mut Context<Self>) {
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(session.to_string()));
        self.show_toast(crate::i18n::t("pill.code.sidebar.copiedId"), cx);
    }

    /// Cuánto falta para que vuelva una conversación pospuesta, si lo está.
    fn wake_label(&self, info: Option<&SessionInfo>) -> Option<String> {
        let until = self.marks.snoozed_until(&info?.session_id)?;
        let left = until.saturating_sub(super::marks::now_secs());
        (left > 0).then(|| tf("pill.code.sidebar.snoozedBack", &[("t", &super::marks::duration_label(left))]))
    }

    /// El nombre del proyecto de una conversación, para la barra sin carpetas.
    fn project_name(&self, workspace: u64) -> String {
        if workspace == LOOSE {
            return crate::i18n::t("pill.code.sidebar.noProject").to_string();
        }
        self.workspaces.get(workspace).map(|w| w.name.clone()).unwrap_or_default()
    }

    /// Cambia la barra entre carpetas por proyecto y una sola lista de conversaciones.
    pub(super) fn toggle_sidebar_list(&mut self, cx: &mut Context<Self>) {
        self.configs.set_sidebar_list(!self.configs.sidebar_list);
        if self.configs.sidebar_list {
            // La lista junta las de todos los proyectos, estén abiertos o no.
            self.load_history(cx);
        }
        cx.notify();
    }

    /// Las conversaciones guardadas de un espacio que se ven en la barra: las pospuestas se
    /// esconden (siguen en el Historial), salvo la que está abierta.
    /// Una conversación abierta que el historial todavía no lista: la nueva sin sesión y la
    /// que ya recibió su sesión pero espera a que el historial se recargue (al terminar el
    /// turno). Sin esto, desaparecía de la barra durante todo el primer turno.
    fn unlisted(&self, chat: &super::Chat) -> bool {
        if chat.items.is_empty() {
            return false;
        }
        match chat.session_id.as_deref() {
            None => true,
            Some(id) => chat.live && !self.history.values().flatten().any(|info| info.session_id == id),
        }
    }

    fn visible_sessions(&self, workspace: u64) -> Vec<&SessionInfo> {
        self.history
            .get(&workspace)
            .into_iter()
            .flatten()
            .filter(|info| !self.is_snoozed(&info.session_id) || self.chats.iter().any(|c| c.session_id.as_deref() == Some(info.session_id.as_str()) && self.active.as_deref() == Some(c.key.as_str())))
            .collect()
    }

    /// Reparte las conversaciones de un espacio en la barra: Fijadas arriba, Trabajando (el
    /// agente está respondiendo; si pide un permiso o una respuesta vuelve a Activas, que es
    /// donde se nota), y Activas, con tope `limit`. Devuelve la lista, cuántas Activas hay y
    /// las Hechas, que van aparte (`settled_shelf`).
    /// Con `listed`, cada fila lleva su proyecto arriba (la barra sin carpetas).
    #[allow(clippy::too_many_arguments)]
    fn grouped_convs<'a>(&'a self, mut list: Div, live: Vec<&'a super::Chat>, sessions: Vec<(u64, &'a SessionInfo)>, limit: usize, listed: bool, window: &mut Window, cx: &mut Context<Self>) -> (Div, usize, Vec<Conv<'a>>) {
        let working = |chat: Option<&super::Chat>| chat.is_some_and(|c| c.in_working_group());
        let mut groups: [Vec<Conv<'a>>; 4] = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
        for chat in live {
            groups[bucket_of(false, working(Some(chat)), false) as usize].push((chat.workspace, None, Some(chat)));
        }
        for (workspace, info) in sessions {
            let chat = self.chats.iter().find(|c| c.session_id.as_deref() == Some(info.session_id.as_str()));
            let bucket = bucket_of(self.is_bookmarked(&info.session_id), working(chat), self.is_settled(&info.session_id));
            groups[bucket as usize].push((workspace, Some(info), chat));
        }
        let [pinned, running, active, settled] = groups;
        // Las fijadas y las activas siguen el orden del usuario (lo nuevo va arriba); las que
        // esperan una respuesta suben al tope de las activas.
        let pinned = arrange(pinned, self.marks.pinned());
        let mut active = arrange(active, self.marks.order());
        active.sort_by_key(|(_, _, chat)| !chat.is_some_and(|c| !c.permissions.is_empty()));
        // Los subagentes van bajo su padre si este se ve; si no, quedan donde estaban.
        let mut groups = [pinned, running, active, settled];
        let nested = nest_subagents(&mut groups, limit);
        let [pinned, running, active, settled] = groups;
        debug_assert_eq!([Bucket::Pinned, Bucket::Working, Bucket::Active, Bucket::Settled].map(|b| b as usize), [0, 1, 2, 3]);
        let mut shown = 0;
        // Con grupos por encima, las Activas también llevan su título.
        let titled = !pinned.is_empty() || !running.is_empty();
        if !pinned.is_empty() {
            list = list.child(self.pin_zone(crate::i18n::t("pill.code.sidebar.pinned"), cx));
        } else if listed && self.dragging_chat {
            // Sin fijadas aún, mientras se arrastra aparece dónde soltar para fijar.
            list = list.child(self.pin_zone(crate::i18n::t("pill.code.sidebar.dropToPin"), cx));
        }
        for (workspace, info, chat) in pinned {
            list = list.child(self.session_row(workspace, info, chat, shown, listed, window, cx));
            shown += 1;
            list = list.children(self.subagent_rows(chat, &nested, &mut shown, listed, window, cx));
        }
        if !running.is_empty() {
            list = list.child(group_label(crate::i18n::t("pill.code.sidebar.working")));
        }
        for (workspace, info, chat) in running {
            list = list.child(self.session_row(workspace, info, chat, shown, listed, window, cx));
            shown += 1;
            list = list.children(self.subagent_rows(chat, &nested, &mut shown, listed, window, cx));
        }
        let active_count = active.len();
        if titled && active_count > 0 {
            list = list.child(group_label(crate::i18n::t("pill.code.sidebar.active")));
        }
        for (workspace, info, chat) in active.into_iter().take(limit) {
            list = list.child(self.session_row(workspace, info, chat, shown, listed, window, cx));
            shown += 1;
            list = list.children(self.subagent_rows(chat, &nested, &mut shown, listed, window, cx));
        }
        (list, active_count, settled)
    }

    /// Los subagentes de `parent` bajo su fila: con sangría, una línea guía a la izquierda y el
    /// logo de su agente; su estado es el de cualquier fila (trabajando, pide permiso, sin
    /// leer). Los nietos van bajo su padre, un nivel más adentro.
    #[allow(clippy::too_many_arguments)]
    fn subagent_rows<'a>(&'a self, parent: Option<&super::Chat>, nested: &[Conv<'a>], shown: &mut usize, listed: bool, window: &mut Window, cx: &mut Context<Self>) -> Option<Div> {
        let parent = parent?;
        let mine: Vec<Conv<'a>> = nested.iter().copied().filter(|(_, _, chat)| parent_key(*chat) == Some(parent.key.as_str())).collect();
        if mine.is_empty() {
            return None;
        }
        let tokens = t();
        let mut block = div().ml(px(14.)).pl(px(6.)).border_l_1().border_color(tokens.border).flex().flex_col().gap(px(1.));
        for (workspace, info, chat) in mine {
            let row = self.session_row(workspace, info, chat, *shown, listed, window, cx);
            *shown += 1;
            let logo = chat.and_then(|c| c.delegation.as_ref()).and_then(|d| super::delegate::agent_logo(&d.backend));
            block = block.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .when_some(logo, |el, path| el.child(gpui::svg().path(path).size(px(13.)).flex_none().text_color(tokens.muted)))
                    .child(div().flex_1().min_w(px(0.)).child(row)),
            );
            block = block.children(self.subagent_rows(chat, nested, shown, listed, window, cx));
        }
        Some(block)
    }

    /// Una repisa plegada al final de un espacio («Hechas (n)» o «Pospuestas (n)»): al abrirla
    /// muestra sus filas. `key` dice de qué espacio es (o `ALL_KEY` en la barra sin carpetas).
    #[allow(clippy::too_many_arguments)]
    fn shelf(&self, mut list: Div, key: u64, items: Vec<Conv<'_>>, snoozed: bool, listed: bool, window: &mut Window, cx: &mut Context<Self>) -> Div {
        if items.is_empty() {
            return list;
        }
        let open = if snoozed { self.snoozed_open.contains(&key) } else { self.settled_open.contains(&key) };
        let (shelf_label, hide_label) = if snoozed { ("pill.code.sidebar.snoozedShelf", "pill.code.sidebar.snoozedHide") } else { ("pill.code.sidebar.settledShelf", "pill.code.sidebar.settledHide") };
        let label = if open { crate::i18n::t(hide_label).to_string() } else { tf(shelf_label, &[("n", &items.len())]) };
        let toggle = cx.listener(move |view, _: &ClickEvent, _, cx| {
            let set = if snoozed { &mut view.snoozed_open } else { &mut view.settled_open };
            if !set.remove(&key) {
                set.insert(key);
            }
            cx.notify();
        });
        let id = SharedString::from(format!("shelf-{}-{key}", u8::from(snoozed)));
        let header = if super::view::expressive() {
            NavItem::new(id.clone(), label).dense(true).muted(true).on_click(toggle).into_any_element()
        } else {
            super::view::flat_more(id.clone(), label, toggle)
        };
        // Soltar una conversación sobre «Hechas» la marca como hecha.
        list = list.child(if snoozed {
            header
        } else {
            div()
                .id(SharedString::from(format!("{id}-drop")))
                .on_drop::<super::split::ChatDrag>(cx.listener(|view, drag: &super::split::ChatDrag, _, cx| view.drop_settle(drag, cx)))
                .child(header)
                .into_any_element()
        });
        if open {
            for (shown, (workspace, info, chat)) in items.into_iter().enumerate() {
                list = list.child(self.session_row(workspace, info, chat, shown, listed, window, cx));
            }
        }
        list
    }

    /// Las conversaciones de un proyecto: primero las abiertas sin guardar, luego el historial.
    pub(super) fn project_convs(&self, workspace: u64, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let m3 = super::view::expressive();
        let list = div().flex().flex_col().gap(px(1.)).pt(px(1.)).pb(px(4.)).pl(px(if m3 { 26. } else { 22. }));
        let live: Vec<&super::Chat> = self.chats.iter().filter(|c| c.workspace == workspace && self.unlisted(c)).collect();
        let sessions: Vec<(u64, &SessionInfo)> = self.visible_sessions(workspace).into_iter().map(|info| (workspace, info)).collect();
        let total = live.len() + sessions.len();
        if total == 0 {
            return list.child(hint(if self.history.contains_key(&workspace) { crate::i18n::t("pill.code.sidebar.noConversations") } else { crate::i18n::t("pill.code.sidebar.loading") }));
        }
        let (mut list, active, settled) = self.grouped_convs(list, live, sessions, SHOWN, false, window, cx);
        if active > SHOWN {
            let all = cx.listener(move |view, _: &ClickEvent, _, cx| {
                view.select_workspace(workspace, cx);
                view.history_page = true;
                cx.notify();
            });
            let label = tf("pill.code.sidebar.seeAllChats", &[("total", &total)]);
            list = list.child(if m3 {
                NavItem::new(("project-all", workspace as usize), label).dense(true).muted(true).on_click(all).into_any_element()
            } else {
                super::view::flat_more(SharedString::from(format!("project-all-{workspace}")), label, all)
            });
        }
        let list = self.shelf(list, workspace, settled, false, false, window, cx);
        self.shelf(list, workspace, self.snoozed_convs(Some(workspace)), true, false, window, cx)
    }

    /// Una conversación de la barra. `index` es su lugar en la lista: las filas entran desde el
    /// borde una tras otra, 30 ms entre sí (`m3-row-in`, `motion.css:727`).
    fn session_row(&self, workspace: u64, info: Option<&SessionInfo>, chat: Option<&super::Chat>, index: usize, listed: bool, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let title = chat.filter(|c| info.is_none() || !super::chat::is_default_title(&c.title)).map(|c| c.title.clone()).or_else(|| info.map(|i| i.title.clone())).unwrap_or_default();
        let running = chat.is_some_and(|c| c.in_working_group() || !c.permissions.is_empty());
        let unread = !running && chat.is_some_and(|c| c.unread);
        let state = match chat {
            Some(chat) => chat.row_state(),
            None => info.and_then(|i| self.session_ends.get(&i.session_id)).copied().unwrap_or(super::chat::RowState::Idle),
        };
        let on = !self.history_page && chat.is_some_and(|c| self.active.as_deref() == Some(c.key.as_str()));
        // El borrador sin enviar de una conversación que no es la activa (la activa lo tiene en la caja).
        let draft = chat.filter(|_| !on).and_then(|c| self.drafts.get(&c.key)).and_then(|text| draft_preview(text));
        let has_draft = draft.is_some();
        let key = chat.map(|c| c.key.clone());
        let session = info.cloned();
        let row_id = SharedString::from(match (&session, &key) {
            (Some(s), _) => format!("session-{}", s.session_id),
            (None, Some(k)) => format!("chat-{k}"),
            _ => "session".into(),
        });
        let enter_key = key.clone().or_else(|| session.as_ref().map(|s| s.session_id.clone())).unwrap_or_default();
        let open_key = key.clone();
        let open_session = session.clone();
        let (drop_key, drop_session) = (key.clone(), session.clone());
        let session_id = session.as_ref().map(|s| s.session_id.clone()).or_else(|| chat.and_then(|c| c.session_id.clone()));
        let bookmarked = session_id.is_some_and(|s| self.is_bookmarked(&s));
        let open = cx.listener({
            let (open_key, open_session) = (open_key.clone(), open_session.clone());
            move |view, _: &ClickEvent, window, cx| {
                view.history_page = false;
                match (&open_key, &open_session) {
                    (Some(key), _) => view.select_chat(key.clone(), window, cx),
                    (None, Some(info)) => view.open_session(workspace, info.clone(), window, cx),
                    _ => {}
                }
            }
        });
        // Formal y Glass: la misma información con sus piezas (y «×» en las abiertas).
        let flat_row = (!listed && !super::view::expressive()).then(|| {
            let waiting = chat.is_some_and(|c| !c.permissions.is_empty());
            let meta = state_meta(state, (!running).then(|| info.and_then(|i| i.modified).map(ago)).flatten());
            let close = chat.filter(|_| info.is_none()).map(|c| {
                let key = c.key.clone();
                cx.listener(move |view, _: &ClickEvent, _, cx| {
                    cx.stop_propagation();
                    view.close_chat(&key, cx);
                })
            });
            super::view::flat_session_row(self.side_glide(cx), row_id.clone(), title.clone(), on, bookmarked, running, waiting, unread, has_draft, meta, close, open)
        });
        // Sin carpetas: el proyecto arriba, el título y la rama, en los tres estilos.
        let listed_row = listed.then(|| {
            let (listed_key, listed_session) = (open_key.clone(), open_session.clone());
            let open = cx.listener(move |view, _: &ClickEvent, window, cx| {
                view.history_page = false;
                match (&listed_key, &listed_session) {
                    (Some(key), _) => view.select_chat(key.clone(), window, cx),
                    (None, Some(info)) => view.open_session(workspace, info.clone(), window, cx),
                    _ => {}
                }
            });
            let waiting = chat.is_some_and(|c| !c.permissions.is_empty());
            let meta = self.wake_label(info).or_else(|| (!running).then(|| info.and_then(|i| i.modified).map(ago)).flatten());
            let close = chat.filter(|_| info.is_none()).map(|c| {
                let key = c.key.clone();
                cx.listener(move |view, _: &ClickEvent, _, cx| {
                    cx.stop_propagation();
                    view.close_chat(&key, cx);
                })
            });
            self.listed_row(row_id.clone(), workspace, title.clone(), info.and_then(|i| i.branch.clone()), meta, on, bookmarked, running, waiting, unread, state, has_draft, close, open, info.map(|i| SessionRef { workspace, session_id: i.session_id.clone(), title: title.clone() }), cx)
        });
        let mut row = NavItem::new(row_id.clone(), title.clone())
            .when(bookmarked, |row| row.icon("bookmark"))
            .dense(true)
            .muted(!on && !unread && !running)
            .emphasized(unread)
            .selected(on)
            .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                view.history_page = false;
                match (&open_key, &open_session) {
                    (Some(key), _) => view.select_chat(key.clone(), window, cx),
                    (None, Some(info)) => view.open_session(workspace, info.clone(), window, cx),
                    _ => {}
                }
            }));
        if running {
            row = row.trailing(LoadingIndicator::new().size(px(18.)));
        } else if unread {
            row = row.trailing(Badge::dot().size(px(10.)).strong(Tone::Tertiary).pop(SharedString::from(format!("{row_id}-dot"))));
        } else if has_draft {
            // Un lápiz tenue: hay algo escrito sin enviar (el globito de la fila lo muestra).
            row = row.trailing(gpui::svg().path("icons/pencil.svg").size(px(14.)).text_color(t().faint));
        } else if let Some(meta) = state_meta(state, info.and_then(|i| i.modified).map(ago)) {
            row = row.meta(meta);
        }
        let renaming = !self.rename_in_header && self.renaming.as_ref().zip(session.as_ref()).is_some_and(|(r, s)| r.session_id == s.session_id);
        if renaming {
            return div().child(self.rename_field.clone()).into_any_element();
        }
        let drag = super::split::ChatDrag { key: key.clone(), session: session.clone(), workspace, title: title.clone().into() };
        let target = session.map(|s| SessionRef { workspace, session_id: s.session_id, title });
        let row: AnyElement = match (listed_row, flat_row) {
            (Some(listed), _) => listed,
            (None, Some(flat)) => flat,
            (None, None) => row.into_any_element(),
        };
        // Arrastrar una conversación sobre ésta la deja antes o después (según la mitad), y la
        // pasa a su grupo si venía de otro. Una línea del acento marca dónde caerá.
        let drop_id = info.map(|i| i.session_id.clone());
        let drop_line = self.side_drop.as_ref().filter(|(row, _)| self.dragging_chat && Some(row) == drop_id.as_ref()).map(|(_, before)| *before);
        // La entrada se ata a la conversación abierta si la hay (su clave no cambia cuando le
        // llega el id de sesión) y, si no, al id de la sesión guardada.
        Enter::new(SharedString::from(format!("conv-in-{enter_key}")))
            .from(-12., 0.)
            .delay(stagger(index, 0.03, 6))
            .apply(self.chat_drag(div().id(SharedString::from(format!("drag-{row_id}"))), drag, cx), window, cx)
            .relative()
            // Soltar archivos o imágenes sobre una conversación que no está a la vista la abre
            // y los deja adjuntos en su caja, listos para escribir y mandar.
            .rounded(px(12.))
            .drag_over::<gpui::ExternalPaths>(|el, _, _, _| el.bg(t().accent.opacity(0.14)))
            .on_drop(cx.listener({
                move |view, paths: &gpui::ExternalPaths, window, cx| {
                    view.history_page = false;
                    match (&drop_key, &drop_session) {
                        (Some(key), _) => view.select_chat(key.clone(), window, cx),
                        (None, Some(info)) => view.open_session(workspace, info.clone(), window, cx),
                        _ => {}
                    }
                    view.take_files(paths.paths().to_vec(), cx);
                    view.focus_composer(window, cx);
                }
            }))
            .when_some(draft, |el, preview| el.tooltip(crate::hover::tip_text(tf("pill.code.sidebar.draftTip", &[("text", &preview)]).into())))
            .when_some(target, |el, target| {
                el.on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |view, event: &MouseDownEvent, window, cx| view.open_session_menu(target.clone(), event.position, window, cx)),
                )
            })
            .when_some(drop_id, |el, id| {
                let (hover_id, target_id) = (id.clone(), id);
                el.on_drag_move::<super::split::ChatDrag>(cx.listener(move |view, event: &gpui::DragMoveEvent<super::split::ChatDrag>, _, cx| {
                    let saved = event.drag(cx).session.is_some();
                    let want = (saved && event.bounds.contains(&event.event.position)).then(|| (hover_id.clone(), event.event.position.y < event.bounds.center().y));
                    match want {
                        Some(want) if view.side_drop.as_ref() != Some(&want) => {
                            view.side_drop = Some(want);
                            cx.notify();
                        }
                        None if view.side_drop.as_ref().is_some_and(|(row, _)| *row == hover_id) => {
                            view.side_drop = None;
                            cx.notify();
                        }
                        _ => {}
                    }
                }))
                .on_drop::<super::split::ChatDrag>(cx.listener(move |view, drag: &super::split::ChatDrag, _, cx| view.drop_on_row(drag, &target_id, workspace, cx)))
            })
            .when_some(drop_line, |el, before| {
                let line = div().absolute().left(px(6.)).right(px(6.)).h(px(2.)).rounded_full().bg(t().accent);
                el.child(if before { line.top(px(-1.)) } else { line.bottom(px(-1.)) })
            })
            .child(row)
            .into_any_element()
    }

    /// La fila de la barra sin carpetas: arriba el proyecto (su inicial, su nombre y el estado o
    /// la hora), debajo el título y, si la hay, la rama. Usa pares de color que ya contrastan:
    /// la elegida en Expressive lleva `accent_soft` con `on_accent_soft`; en Formal y Glass,
    /// la píldora de la barra con `text`. Lo secundario va en `muted`, y la hora y la rama en
    /// `faint` salvo en claro fuera de Expressive, donde `faint` no se lee.
    #[allow(clippy::too_many_arguments)]
    fn listed_row(
        &self,
        id: SharedString,
        workspace: u64,
        title: String,
        branch: Option<String>,
        meta: Option<String>,
        on: bool,
        bookmarked: bool,
        running: bool,
        waiting: bool,
        unread: bool,
        state: super::chat::RowState,
        has_draft: bool,
        close: Option<impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static>,
        open: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
        session: Option<SessionRef>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tokens = t();
        let m3 = super::view::expressive();
        let picked = on && m3;
        // Las «hechas» se leen más tenues (`muted`), pero siguen contrastando.
        let done = !running && session.as_ref().is_some_and(|s| self.is_settled(&s.session_id));
        let ink = if picked {
            tokens.on_accent_soft
        } else if done {
            tokens.muted
        } else {
            tokens.text
        };
        let second = if picked { tokens.on_accent_soft } else { tokens.muted };
        let quiet = if picked {
            tokens.on_accent_soft
        } else if tokens.light && !m3 {
            tokens.muted
        } else {
            tokens.faint
        };
        let hot = if picked { tokens.on_accent_soft } else { tokens.accent };
        let name = self.project_name(workspace);
        let group = SharedString::from(format!("{id}-listed"));
        let (chip_bg, chip_fg) = if picked { (tokens.raised, tokens.text) } else { (tokens.accent_soft, tokens.on_accent_soft) };
        // Al pasar el cursor, una barrita flotante con fijar, marcar como hecha y el menú. Flota
        // sobre la hora y el estado, así que su base es `raised` (no `control`, que en vidrio
        // deja ver lo de abajo) y sus íconos van en `text`; el activo, en el acento.
        let has_actions = session.is_some();
        let actions = session.map(|target| {
            let settled = self.is_settled(&target.session_id);
            let button = |key: &'static str, icon: &'static str, tip: &'static str, lit: bool| {
                div()
                    .id(SharedString::from(format!("{id}-{key}")))
                    .size(px(22.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .cursor_pointer()
                    .hover(|el| el.bg(tokens.hover))
                    .tooltip(crate::hover::tip(tip))
                    .child(gpui::svg().path(icon).size(px(13.)).text_color(if lit { tokens.accent } else { tokens.text }))
            };
            let (pin_id, settle_id) = (target.session_id.clone(), target.session_id.clone());
            let pin = button("pin", "icons/pin.svg", if bookmarked { crate::i18n::t("pill.code.sidebar.removeBookmarkTip") } else { crate::i18n::t("pill.code.sidebar.addBookmarkTip") }, bookmarked).on_click(cx.listener(
                move |view, _: &ClickEvent, _, cx| {
                    cx.stop_propagation();
                    view.toggle_bookmark(&pin_id, cx);
                },
            ));
            let settle = button("settle", "icons/circle-check.svg", if settled { crate::i18n::t("pill.code.sidebar.unsettle") } else { crate::i18n::t("pill.code.sidebar.settle") }, settled).on_click(cx.listener(
                move |view, _: &ClickEvent, _, cx| {
                    cx.stop_propagation();
                    if settled {
                        view.unsettle_session(&settle_id, cx);
                    } else {
                        view.settle_session(&settle_id, cx);
                    }
                },
            ));
            let more = button("more", "icons/ellipsis.svg", crate::i18n::t("pill.code.sidebar.moreTip"), false).on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                cx.stop_propagation();
                view.open_session_menu(target.clone(), event.position(), window, cx);
            }));
            div()
                .absolute()
                .top(px(5.))
                .right(px(6.))
                .invisible()
                .group_hover(group.clone(), |el| el.visible())
                .flex()
                .items_center()
                .gap(px(1.))
                .p(px(2.))
                .rounded_full()
                .bg(tokens.raised)
                .border_1()
                .border_color(tokens.border)
                .child(pin)
                .child(settle)
                .child(more)
        });
        let chip = div()
            .size(px(16.))
            .flex_none()
            .rounded(px(5.))
            .flex()
            .items_center()
            .justify_center()
            .bg(chip_bg)
            .text_color(chip_fg)
            .text_size(px(8.5))
            .font_weight(FontWeight::BOLD)
            .child(if workspace == LOOSE { gpui_m3::Icon::new("chat").size(px(10.)).color(chip_fg).into_any_element() } else { initials(&name).into_any_element() });
        // El estado va abajo a la derecha, en negrita, lejos de la barrita de acciones; la hora
        // queda arriba, donde la tapa la barrita sin perder nada importante.
        let live = waiting || running || state.calls_you();
        let label = if live {
            Some(state.label().unwrap_or(if waiting { crate::i18n::t("pill.code.sidebar.needsInput") } else { crate::i18n::t("pill.code.sidebar.working") }))
        } else {
            state.label()
        };
        let status = label.map(|label| {
            // «Lista» o «Se detuvo» en el acento mientras no la hayas mirado.
            let color = if live || unread { hot } else { second };
            // Su marca: un círculo que, mientras pide que lo mires, se vuelve otra forma de M3
            // (con resorte, de ida y de vuelta) según cómo quedó.
            let lit = live || unread;
            let ink = if !lit {
                second
            } else if picked {
                tokens.accent_soft
            } else {
                tokens.on_accent
            };
            let glyph = |icon: &'static str| gpui_m3::Icon::new(icon).size(px(9.)).color(ink).into_any_element();
            let mark = match state {
                RowState::Done => Some((glyph("check"), ShapeName::SoftBurst)),
                RowState::Asking | RowState::NeedsYou => Some((div().text_size(px(9.5)).font_weight(FontWeight::EXTRA_BOLD).text_color(ink).child("?").into_any_element(), ShapeName::Puffy)),
                RowState::Stopped => Some((glyph("pause"), ShapeName::Square)),
                RowState::Working | RowState::Idle => None,
            };
            let mark = mark.map(|(child, other)| {
                gpui_m3::Shape::new(ShapeName::Circle)
                    .size(px(15.))
                    .color(if lit { hot } else { second.opacity(0.18) })
                    .breathe_on_hover(SharedString::from(format!("{id}-mark")), other)
                    .hovered(lit)
                    .child(child)
            });
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(5.))
                .text_size(px(11.))
                .font_weight(FontWeight::BOLD)
                .text_color(color)
                .when(running && !waiting, |el| el.child(if m3 { LoadingIndicator::new().size(px(12.)).into_any_element() } else { div().size(px(6.)).rounded_full().bg(hot).into_any_element() }))
                .children(mark)
                .child(label)
        });
        let time = meta.map(|meta| div().flex_none().text_size(px(11.)).text_color(quiet).child(meta));
        div()
            .id(id.clone())
            .group(group.clone())
            .relative()
            .w_full()
            .px(px(10.))
            .py(px(7.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .rounded(px(if m3 { 14. } else { 8. }))
            .cursor_pointer()
            .when(picked, |el| el.bg(tokens.accent_soft))
            .when(!on, |el| el.hover(|el| el.bg(tokens.hover)))
            // En Formal y Glass, el fondo de la elegida es la píldora que se desliza (`side_glide`).
            .when(!m3, |el| el.child(crate::glide::mark_on(self.side_glide(cx), super::view::side_key(&id), on, Some(8.))))
            .on_click(open)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(chip)
                    .child(div().flex_1().min_w(px(0.)).truncate().text_size(px(11.5)).text_color(second).child(name))
                    // Con la barrita de acciones encima, la hora se esconde (sin mover nada).
                    .children(time.map(|time| if has_actions { time.group_hover(group.clone(), |el| el.invisible()) } else { time })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_size(px(13.))
                            .font_weight(if unread {
                                FontWeight::SEMIBOLD
                            } else if running || picked {
                                FontWeight::MEDIUM
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(ink)
                            .child(title),
                    )
                    .when(has_draft, |el| el.child(gpui::svg().path("icons/pencil.svg").size(px(11.)).flex_none().text_color(quiet)))
                    .when(unread, |el| el.child(div().size(px(7.)).flex_none().rounded_full().bg(hot)))
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
                                    .hover(|el| el.bg(tokens.hover))
                                    .tooltip(crate::hover::tip(crate::i18n::t("pill.code.view.close")))
                                    .on_click(close)
                                    .child(gpui::svg().path("icons/x.svg").size(px(11.)).text_color(second)),
                            ),
                        )
                    }),
            )
            .when(branch.is_some() || status.is_some(), |el| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .text_size(px(11.))
                        .text_color(quiet)
                        .when_some(branch, |el, branch| el.child(gpui::svg().path("icons/git-branch.svg").size(px(10.)).flex_none().text_color(quiet)).child(div().min_w(px(0.)).truncate().child(branch)))
                        .child(div().flex_1())
                        .children(status),
                )
            })
            .children(actions)
            .into_any_element()
    }

    /// Pie de la barra: el perfil, Apariencia y Configuración.
    fn side_profile(&self, cx: &mut Context<Self>) -> Div {
        let name = self.profile_name();
        div()
            .flex()
            .items_center()
            .gap(px(4.))
            .child(
                div()
                    .id("profile-btn")
                    .flex_1()
                    .min_w(px(0.))
                    .h(px(44.))
                    .pl(px(6.))
                    .pr(px(8.))
                    .flex()
                    .items_center()
                    .gap(px(9.))
                    .rounded(px(22.))
                    .hover(|el| el.bg(t().control).rounded(px(16.)))
                    .cursor_pointer()
                    .tooltip(crate::hover::tip(crate::i18n::t("pill.code.sidebar.yourProfile")))
                    .on_click(cx.listener(|view, event: &ClickEvent, window, cx| view.toggle_profile(event.position(), window, cx)))
                    .child(self.profile_avatar("side-avatar", px(30.), cx))
                    .child(div().min_w(px(0.)).truncate().font_weight(FontWeight::MEDIUM).child(name)),
            )
            .child(IconButton::new("side-style", "palette").size(px(32.)).tooltip(crate::i18n::t("pill.code.sidebar.appearance")).on_click(cx.listener(|view, event: &ClickEvent, _, cx| {
                view.toggle_style_menu(event.position(), cx);
            })))
            .child(IconButton::new("side-settings", "gear").size(px(32.)).tooltip(crate::keymap::hint(crate::i18n::t("pill.code.sidebar.settingsTip"))).on_click(cx.listener(
                |view, _: &ClickEvent, _, cx| {
                    view.settings_open = !view.settings_open;
                    cx.notify();
                },
            )))
    }

    /// La barra plegada: el riel de M3.
    fn rail(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .w(px(RAIL_W))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.))
            .pb(px(8.))
            .child(div().h(px(crate::kit::chrome::RAIL_TOP)).w_full().child(crate::kit::chrome::drag(crate::kit::chrome::RAIL_TOP)))
            .child(IconButton::new("rail-menu", "panel-left").size(px(40.)).tooltip(crate::keymap::hint(crate::i18n::t("pill.code.sidebar.sidebarTip"))).on_click(cx.listener(
                |view, _: &ClickEvent, _, cx| {
                    view.sidebar_open = true;
                    cx.notify();
                },
            )))
            .child(
                div().mt(px(4.)).mb(px(12.)).child(
                    Fab::new("rail-fab", "plus").on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.new_conversation(window, cx))),
                ),
            )
            .child(RailItem::new("rail-chat", "chat", crate::i18n::t("pill.code.sidebar.chat")).selected(!self.history_page).on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                view.history_page = false;
                cx.notify();
            })))
            .child(RailItem::new("rail-history", "history", crate::i18n::t("pill.code.sidebar.history")).selected(self.history_page).on_click(cx.listener(
                |view, _: &ClickEvent, _, cx| {
                    view.history_page = true;
                    view.load_history(cx);
                    cx.notify();
                },
            )))
            .child(RailItem::new("rail-search", "search", crate::i18n::t("pill.code.sidebar.search")).selected(self.palette_open).on_click(cx.listener(
                |view, _: &ClickEvent, window, cx| view.open_palette(window, cx),
            )))
            .child(div().flex_1().w_full().child(crate::kit::chrome::drag(200.)))
            .child(IconButton::new("rail-style", "palette").size(px(40.)).tooltip(crate::i18n::t("pill.code.sidebar.appearance")).on_click(cx.listener(|view, event: &ClickEvent, _, cx| {
                view.toggle_style_menu(event.position(), cx);
            })))
            .child(IconButton::new("rail-settings", "gear").size(px(40.)).tooltip(crate::keymap::hint(crate::i18n::t("pill.code.sidebar.settingsTip"))).on_click(cx.listener(
                |view, _: &ClickEvent, _, cx| {
                    view.settings_open = !view.settings_open;
                    cx.notify();
                },
            )))
            .child(
                div()
                    .id("rail-avatar")
                    .mt(px(6.))
                    .cursor_pointer()
                    .tooltip(crate::hover::tip(crate::i18n::t("pill.code.sidebar.openSidebar")))
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                        view.sidebar_open = true;
                        cx.notify();
                    }))
                    .child(self.profile_avatar("rail-avatar-shape", px(36.), cx)),
            )
            .into_any_element()
    }

    /// La página de historial: las conversaciones de todos los espacios y de los chats
    /// sueltos, de la más reciente a la más vieja, cada una con el nombre de su espacio.
    /// (Antes solo se veía el espacio activo, y una conversación de otro no aparecía.)
    pub(super) fn history_view(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let query = self.history_search.read(cx).text().trim().to_lowercase();
        let marked = self.history_marked;
        let spaces: Vec<(u64, String)> = self
            .workspaces
            .list()
            .iter()
            .map(|w| (w.id, w.name.clone()))
            .chain(std::iter::once((super::LOOSE, crate::i18n::t("pill.code.sidebar.chats").to_string())))
            .collect();
        let mut sessions: Vec<(u64, String, SessionInfo)> = spaces
            .iter()
            .flat_map(|(id, name)| self.history.get(id).into_iter().flatten().map(move |s| (*id, name.clone(), s.clone())))
            .filter(|(_, _, s)| !marked || self.is_bookmarked(&s.session_id))
            .filter(|(_, name, s)| query.is_empty() || s.title.to_lowercase().contains(&query) || name.to_lowercase().contains(&query))
            .collect();
        sessions.sort_by(|a, b| b.2.modified.unwrap_or(0.).total_cmp(&a.2.modified.unwrap_or(0.)));
        let several = spaces.iter().filter(|(id, _)| self.history.get(id).is_some_and(|l| !l.is_empty())).count() > 1;
        let filters = div().mb(px(10.)).flex().items_center().gap(px(8.)).child(div().flex_1().child(self.history_search.clone())).child(
            Chip::new("history-marked", crate::i18n::t("pill.code.sidebar.bookmarks")).icon("bookmark").filter(marked).on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                view.history_marked = !view.history_marked;
                cx.notify();
            })),
        );
        let none_marked = marked && sessions.is_empty();
        let muted = t().muted;
        let mut list = div().flex().flex_col().gap(px(2.));
        // Las filas entran una tras otra, 25 ms entre sí (`motion.css:784`).
        let none = sessions.is_empty();
        for (index, (workspace, name, info)) in sessions.into_iter().enumerate() {
            let enter = Enter::new(SharedString::from(format!("history-in-{}", info.session_id))).delay(stagger(index, 0.025, 8));
            list = list.child(enter.apply(div(), window, cx).child(self.history_row(workspace, info, several.then_some(name), cx)));
        }
        let empty = if none_marked {
            Some(crate::i18n::t("pill.code.sidebar.noBookmarks"))
        } else if none && query.is_empty() {
            Some(crate::i18n::t("pill.code.sidebar.noneYet"))
        } else {
            None
        };
        div()
            .id("history")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .child(
                // Sin entrada propia: la página ya la mueve `page-swap` y cada fila entra sola;
                // con tres, las filas bajaban en diagonal y con la opacidad multiplicada.
                div()
                    .w_full()
                    .px(px(32.))
                    .pt(px(8.))
                    .pb(px(32.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(filters)
                    .child(list)
                    .when_some(empty, |el, text| el.child(div().p(px(16.)).text_center().text_color(muted).child(text))),
            )
            .into_any_element()
    }

    fn history_row(&self, workspace: u64, info: SessionInfo, space: Option<String>, cx: &mut Context<Self>) -> AnyElement {
        let t = t();
        let renaming = !self.rename_in_header && self.renaming.as_ref().is_some_and(|r| r.session_id == info.session_id);
        if renaming {
            return div().mx(px(8.)).my(px(6.)).child(self.rename_field.clone()).into_any_element();
        }
        let id = info.session_id.clone();
        let bookmarked = self.is_bookmarked(&id);
        let mark_id = id.clone();
        let group = SharedString::from(format!("history-{id}"));
        let confirming = self.confirm_delete.as_deref() == Some(id.as_str());
        let target = SessionRef { workspace, session_id: id.clone(), title: info.title.clone() };
        let (open_info, rename_target, delete_target, menu_target) = (info.clone(), target.clone(), target.clone(), target);
        div()
            .id(SharedString::from(format!("history-row-{id}")))
            .group(group.clone())
            .flex()
            .items_center()
            .gap(px(8.))
            .pr(px(6.))
            .rounded(px(14.))
            .hover(|el| el.bg(t.hover))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |view, event: &MouseDownEvent, window, cx| view.open_session_menu(menu_target.clone(), event.position, window, cx)),
            )
            .on_hover(cx.listener(move |view, hovered: &bool, _, cx| {
                if !hovered && view.confirm_delete.is_some() {
                    view.confirm_delete = None;
                    cx.notify();
                }
            }))
            .child(
                div()
                    .id(SharedString::from(format!("history-open-{id}")))
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .px(px(12.))
                    .py(px(8.))
                    .cursor_pointer()
                    .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                        view.history_page = false;
                        view.open_session(workspace, open_info.clone(), window, cx);
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .when(bookmarked, |el| el.child(gpui_m3::Icon::new("bookmark").size(px(14.)).color(t.accent)))
                            .child(div().min_w(px(0.)).truncate().font_weight(FontWeight::MEDIUM).child(info.title.clone())),
                    )
                    .when(info.modified.is_some() || info.branch.is_some() || space.is_some(), |el| {
                        // «espacio · hace X · rama», como el historial de la referencia más el espacio.
                        let meta = [space.clone(), info.modified.map(ago), info.branch.clone()].into_iter().flatten().collect::<Vec<_>>().join(" · ");
                        el.child(div().min_w(px(0.)).overflow_hidden().whitespace_nowrap().text_size(px(12.)).text_color(t.muted).child(meta))
                    }),
            )
            .child(
                div()
                    .flex()
                    .invisible()
                    .group_hover(group, |el| el.visible())
                    .when(confirming, |el| el.visible())
                    .child(
                        IconButton::new(SharedString::from(format!("history-mark-{id}")), "bookmark")
                            .size(px(32.))
                            .selected(bookmarked)
                            .tooltip(if bookmarked { crate::i18n::t("pill.code.sidebar.removeBookmarkTip") } else { crate::i18n::t("pill.code.sidebar.addBookmarkTip") })
                            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.toggle_bookmark(&mark_id, cx))),
                    )
                    .child(IconButton::new(SharedString::from(format!("history-rename-{id}")), "pen").size(px(32.)).tooltip(crate::i18n::t("pill.code.sidebar.rename")).on_click(
                        cx.listener(move |view, _: &ClickEvent, window, cx| view.start_rename(rename_target.clone(), window, cx)),
                    ))
                    .child(
                        div()
                            .when(confirming, |el| el.rounded(px(16.)).bg(t.bad.opacity(0.14)))
                            .child(
                                IconButton::new(SharedString::from(format!("history-delete-{id}")), "trash")
                                    .size(px(32.))
                                    .tooltip(if confirming { crate::i18n::t("pill.code.sidebar.clickAgainDelete") } else { crate::i18n::t("pill.code.sidebar.delete") })
                                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                        if view.confirm_delete.as_deref() == Some(delete_target.session_id.as_str()) {
                                            view.confirm_delete = None;
                                            view.delete_session(delete_target.clone(), cx);
                                        } else {
                                            view.confirm_delete = Some(delete_target.session_id.clone());
                                            cx.notify();
                                        }
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// Abre el menú del clic derecho sobre una conversación: en macOS, el del
    /// sistema; si no, el dibujado (`session_menu_layer`).
    pub(super) fn open_session_menu(&mut self, target: SessionRef, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        #[cfg(target_os = "macos")]
        {
            use crate::platform::popup_menu::{Entry, Item};
            let bookmarked = self.is_bookmarked(&target.session_id);
            let snoozed = self.is_snoozed(&target.session_id);
            let snooze_id = target.session_id.clone();
            let settled = self.is_settled(&target.session_id);
            let settle_id = target.session_id.clone();
            let scope = if self.configs.sidebar_list { None } else { Some(target.workspace) };
            let can_up = self.can_move(target.workspace, &target.session_id, scope, -1);
            let can_down = self.can_move(target.workspace, &target.session_id, scope, 1);
            let (move_workspace, move_id, copy_id, beside) = (target.workspace, target.session_id.clone(), target.session_id.clone(), target.clone());
            let (open, rename, mark, delete) = (target.clone(), target.clone(), target.clone(), target);
            let mut entries: Vec<(Item, Option<super::NativeAction>)> = vec![
                (Item::Entry(Entry { symbol: Some("bubble.left"), ..Entry::new(crate::i18n::t("pill.code.sidebar.open")) }), Some(Box::new(move |view: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
                    view.history_page = false;
                    let info = SessionInfo { session_id: open.session_id.clone(), title: open.title.clone(), modified: None, branch: None };
                    view.open_session(open.workspace, info, window, cx);
                }))),
                (Item::Entry(Entry { symbol: Some("pencil"), ..Entry::new(crate::i18n::t("pill.code.sidebar.rename")) }), Some(Box::new(move |view: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
                    view.start_rename(rename, window, cx);
                }))),
                (
                    Item::Entry(Entry {
                        symbol: Some(if bookmarked { "bookmark.slash" } else { "bookmark" }),
                        ..Entry::new(if bookmarked { crate::i18n::t("pill.code.sidebar.removeBookmark") } else { crate::i18n::t("pill.code.sidebar.addBookmark") })
                    }),
                    Some(Box::new(move |view: &mut Self, _: &mut Window, cx: &mut Context<Self>| view.toggle_bookmark(&mark.session_id, cx))),
                ),
                (Item::Separator, None),
                (Item::Entry(Entry { symbol: Some("trash"), ..Entry::new(crate::i18n::t("pill.code.sidebar.deleteEllipsis")) }), Some(Box::new(move |view: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
                    let title = delete.title.clone();
                    view.confirm_native(
                        &tf("pill.code.sidebar.deleteTitle", &[("title", &title)]),
                        crate::i18n::t("pill.code.sidebar.deleteBody"),
                        crate::i18n::t("pill.code.sidebar.delete"),
                        Box::new(move |view, _, cx| view.delete_session(delete, cx)),
                        window,
                        cx,
                    );
                }))),
            ];
            // Posponer (o dejar de posponer) va justo después del marcador.
            let snooze_entries: Vec<(Item, Option<super::NativeAction>)> = if snoozed {
                let action: super::NativeAction = Box::new(move |view: &mut Self, _: &mut Window, cx: &mut Context<Self>| view.unsnooze_session(&snooze_id, cx));
                vec![(Item::Entry(Entry { symbol: Some("clock"), ..Entry::new(crate::i18n::t("pill.code.sidebar.unsnooze")) }), Some(action))]
            } else {
                [("pill.code.sidebar.snoozeHour", super::marks::SNOOZE_HOUR), ("pill.code.sidebar.snoozeDay", super::marks::SNOOZE_DAY), ("pill.code.sidebar.snoozeWeek", super::marks::SNOOZE_WEEK)]
                    .into_iter()
                    .map(|(label, secs)| {
                        let id = snooze_id.clone();
                        let action: super::NativeAction = Box::new(move |view: &mut Self, _: &mut Window, cx: &mut Context<Self>| view.snooze_session(&id, secs, cx));
                        (Item::Entry(Entry { symbol: Some("clock"), ..Entry::new(crate::i18n::t(label)) }), Some(action))
                    })
                    .collect()
            };
            let settle_action: super::NativeAction = Box::new(move |view: &mut Self, _: &mut Window, cx: &mut Context<Self>| {
                if settled {
                    view.unsettle_session(&settle_id, cx)
                } else {
                    view.settle_session(&settle_id, cx)
                }
            });
            let settle_label = if settled { crate::i18n::t("pill.code.sidebar.unsettle") } else { crate::i18n::t("pill.code.sidebar.settle") };
            let mut extra = vec![(Item::Entry(Entry { symbol: Some("checkmark.circle"), ..Entry::new(settle_label) }), Some(settle_action))];
            let mut at = 0;
            for (allowed, delta, symbol, label) in [(can_up, -1, "arrow.up", "pill.code.sidebar.moveUp"), (can_down, 1, "arrow.down", "pill.code.sidebar.moveDown")] {
                if allowed {
                    let id = move_id.clone();
                    let action: super::NativeAction = Box::new(move |view: &mut Self, _: &mut Window, cx: &mut Context<Self>| view.move_session(move_workspace, &id, scope, delta, cx));
                    extra.insert(at, (Item::Entry(Entry { symbol: Some(symbol), ..Entry::new(crate::i18n::t(label)) }), Some(action)));
                    at += 1;
                }
            }
            extra.extend(snooze_entries);
            let copy_action: super::NativeAction = Box::new(move |view: &mut Self, _: &mut Window, cx: &mut Context<Self>| view.copy_session_id(&copy_id, cx));
            extra.push((Item::Entry(Entry { symbol: Some("doc.on.doc"), ..Entry::new(crate::i18n::t("pill.code.sidebar.copyId")) }), Some(copy_action)));
            let beside_action: super::NativeAction = Box::new(move |view: &mut Self, window: &mut Window, cx: &mut Context<Self>| view.open_beside(&beside, window, cx));
            extra.push((Item::Entry(Entry { symbol: Some("rectangle.split.2x1"), ..Entry::new(crate::i18n::t("pill.code.sidebar.openBeside")) }), Some(beside_action)));
            if move_workspace != LOOSE && self.workspaces.get(move_workspace).is_some() {
                let edit_action: super::NativeAction = Box::new(move |view: &mut Self, window: &mut Window, cx: &mut Context<Self>| view.open_edit_space(move_workspace, window, cx));
                extra.push((Item::Entry(Entry { symbol: Some("folder.badge.gearshape"), ..Entry::new(crate::i18n::t("pill.code.sidebar.editSpaceEllipsis")) }), Some(edit_action)));
            }
            entries.splice(3..3, extra);
            self.pop_native_menu(at, entries, window, cx);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = window;
            self.session_menu = Some((target, at));
            cx.notify();
        }
    }

    /// Abre el menú del clic derecho sobre un espacio: en macOS, el del sistema;
    /// si no, el dibujado (`space_menu_layer`).
    pub(super) fn open_space_menu(&mut self, id: u64, at: Point<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        #[cfg(target_os = "macos")]
        {
            use crate::platform::popup_menu::{Entry, Item};
            let favorite = self.configs.favorites.contains(&id);
            let name = self.workspaces.get(id).map(|w| w.name.clone()).unwrap_or_default();
            let entries: Vec<(Item, Option<super::NativeAction>)> = vec![
                (Item::Entry(Entry { symbol: Some("pencil"), ..Entry::new(crate::i18n::t("pill.code.sidebar.rename")) }), Some(Box::new(move |view: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
                    view.start_space_rename(id, window, cx);
                }))),
                (
                    Item::Entry(Entry {
                        symbol: Some(if favorite { "star.slash" } else { "star" }),
                        ..Entry::new(if favorite { crate::i18n::t("pill.code.sidebar.removeFavorite") } else { crate::i18n::t("pill.code.sidebar.markFavorite") })
                    }),
                    Some(Box::new(move |view: &mut Self, _: &mut Window, _: &mut Context<Self>| view.configs.toggle_favorite(id))),
                ),
                (Item::Entry(Entry { symbol: Some("folder.badge.gearshape"), ..Entry::new(crate::i18n::t("pill.code.sidebar.editSpaceEllipsis")) }), Some(Box::new(move |view: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
                    view.open_edit_space(id, window, cx);
                }))),
                (Item::Entry(Entry { symbol: Some("folder.badge.plus"), ..Entry::new(crate::i18n::t("pill.code.sidebar.addFolders")) }), Some(Box::new(move |view: &mut Self, _: &mut Window, cx: &mut Context<Self>| {
                    view.pick_folders(Some(id), cx);
                }))),
                (Item::Separator, None),
                (Item::Entry(Entry { symbol: Some("xmark"), ..Entry::new(crate::i18n::t("pill.code.sidebar.removeFromListEllipsis")) }), Some(Box::new(move |view: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
                    view.confirm_native(
                        &tf("pill.code.sidebar.removeTitle", &[("name", &name)]),
                        crate::i18n::t("pill.code.sidebar.removeBody"),
                        crate::i18n::t("pill.code.sidebar.remove"),
                        Box::new(move |view, _, cx| view.remove_workspace(id, cx)),
                        window,
                        cx,
                    );
                }))),
            ];
            self.pop_native_menu(at, entries, window, cx);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = window;
            self.space_menu = Some((id, at));
            cx.notify();
        }
    }

    /// El menú del clic derecho sobre una conversación.
    pub(super) fn session_menu_layer(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let shown = self.session_menu_last.show("session-menu-presence", self.session_menu.clone(), window, cx)?;
        let (target, at): (SessionRef, Point<Pixels>) = shown.value.clone();
        let leaving = shown.leaving;
        let (open, rename, mark, delete) = (target.clone(), target.clone(), target.clone(), target);
        let bookmarked = self.is_bookmarked(&mark.session_id);
        let snoozed = self.is_snoozed(&mark.session_id);
        let settle = settle_item(cx, self.is_settled(&mark.session_id), &mark.session_id);
        let scope = if self.configs.sidebar_list { None } else { Some(mark.workspace) };
        let mut moves = Vec::new();
        if self.can_move(mark.workspace, &mark.session_id, scope, -1) {
            moves.push(move_item(cx, "session-up", crate::i18n::t("pill.code.sidebar.moveUp"), "chevron-up", mark.workspace, &mark.session_id, scope, -1));
        }
        if self.can_move(mark.workspace, &mark.session_id, scope, 1) {
            moves.push(move_item(cx, "session-down", crate::i18n::t("pill.code.sidebar.moveDown"), "chevron-down", mark.workspace, &mark.session_id, scope, 1));
        }
        let copy_id = {
            let session = mark.session_id.clone();
            MenuItem::new("session-copy-id", crate::i18n::t("pill.code.sidebar.copyId")).icon("copy").on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                view.session_menu = None;
                view.copy_session_id(&session, cx);
            }))
        };
        let beside = {
            let target = mark.clone();
            MenuItem::new("session-beside", crate::i18n::t("pill.code.sidebar.openBeside")).icon("panel-right").on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                view.session_menu = None;
                view.open_beside(&target, window, cx);
            }))
        };
        let snooze_items: Vec<MenuItem> = if snoozed {
            vec![snooze_item(cx, "session-unsnooze", crate::i18n::t("pill.code.sidebar.unsnooze"), None, &mark.session_id)]
        } else {
            [("session-snooze-hour", "pill.code.sidebar.snoozeHour", super::marks::SNOOZE_HOUR), ("session-snooze-day", "pill.code.sidebar.snoozeDay", super::marks::SNOOZE_DAY), ("session-snooze-week", "pill.code.sidebar.snoozeWeek", super::marks::SNOOZE_WEEK)]
                .into_iter()
                .map(|(id, label, secs)| snooze_item(cx, id, crate::i18n::t(label), Some(secs), &mark.session_id))
                .collect()
        };
        let menu = gpui_m3::Menu::new("session-menu")
            .width(230.)
            .item(MenuItem::new("session-open", crate::i18n::t("pill.code.sidebar.open")).icon("chevron-right").on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                view.session_menu = None;
                view.history_page = false;
                let info = SessionInfo { session_id: open.session_id.clone(), title: open.title.clone(), modified: None, branch: None };
                view.open_session(open.workspace, info, window, cx);
            })))
            .item(MenuItem::new("session-rename", crate::i18n::t("pill.code.sidebar.rename")).icon("pen").on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                view.session_menu = None;
                view.start_rename(rename.clone(), window, cx);
            })))
            .item(
                MenuItem::new("session-mark", if bookmarked { crate::i18n::t("pill.code.sidebar.removeBookmark") } else { crate::i18n::t("pill.code.sidebar.addBookmark") }).icon("bookmark").on_click(cx.listener(
                    move |view, _: &ClickEvent, _, cx| {
                        view.session_menu = None;
                        view.toggle_bookmark(&mark.session_id, cx);
                    },
                )),
            );
        // Editar el proyecto de la conversación (sus carpetas y su nombre); los chats sueltos no tienen.
        let edit_project = (mark.workspace != LOOSE && self.workspaces.get(mark.workspace).is_some()).then(|| {
            let workspace = mark.workspace;
            MenuItem::new("session-edit-project", crate::i18n::t("pill.code.sidebar.editSpaceEllipsis")).icon("folder-open").on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                view.session_menu = None;
                view.open_edit_space(workspace, window, cx);
            }))
        });
        let menu = moves.into_iter().chain(std::iter::once(settle)).chain(snooze_items).chain([copy_id, beside]).chain(edit_project).fold(menu, |menu, item| menu.item(item));
        let menu = menu
            .item(MenuItem::new("session-delete", crate::i18n::t("pill.code.sidebar.delete")).icon("trash").danger(true).confirm(crate::i18n::t("pill.code.sidebar.confirmDelete")).on_click(cx.listener(
                move |view, _: &ClickEvent, _, cx| {
                    view.session_menu = None;
                    view.delete_session(delete.clone(), cx);
                },
            )));
        Some(
            deferred(
                div()
                    .absolute()
                    .occlude().inset_0()
                    .when(!leaving, |el| {
                        el.on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|view, _, _, cx| {
                                view.session_menu = None;
                                cx.notify();
                            }),
                        )
                    })
                    .child(
                        anchored().position(point(at.x, at.y)).anchor(Corner::TopLeft).snap_to_window_with_margin(px(8.)).child(
                            div()
                                .id("session-menu-card")
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .child(shown.wrap(gpui_m3::Exit::Sink, menu)),
                        ),
                    ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    /// El menú del clic derecho sobre un espacio: renombrar, agregar carpetas, quitar.
    pub(super) fn space_menu_layer(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let shown = self.space_menu_last.show("space-menu-presence", self.space_menu, window, cx)?;
        let (id, at) = shown.value;
        let leaving = shown.leaving;
        let favorite = self.configs.favorites.contains(&id);
        let menu = gpui_m3::Menu::new("space-menu")
            .width(220.)
            .item(MenuItem::new("space-rename", crate::i18n::t("pill.code.sidebar.rename")).icon("pen").on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                view.space_menu = None;
                view.start_space_rename(id, window, cx);
            })))
            .item(
                MenuItem::new("space-fav", if favorite { crate::i18n::t("pill.code.sidebar.removeFavorite") } else { crate::i18n::t("pill.code.sidebar.markFavorite") }).icon("star").on_click(cx.listener(
                    move |view, _: &ClickEvent, _, cx| {
                        view.space_menu = None;
                        view.configs.toggle_favorite(id);
                        cx.notify();
                    },
                )),
            )
            .item(MenuItem::new("space-edit", crate::i18n::t("pill.code.sidebar.editSpaceEllipsis")).icon("folder-open").on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                view.space_menu = None;
                view.open_edit_space(id, window, cx);
            })))
            .item(MenuItem::new("space-add", crate::i18n::t("pill.code.sidebar.addFolders")).icon("folder-plus").on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                view.space_menu = None;
                view.pick_folders(Some(id), cx);
            })))
            .item(
                MenuItem::new("space-remove", crate::i18n::t("pill.code.sidebar.removeFromList")).icon("x").danger(true).confirm(crate::i18n::t("pill.code.sidebar.confirmRemove")).on_click(cx.listener(
                    move |view, _: &ClickEvent, _, cx| {
                        view.space_menu = None;
                        view.remove_workspace(id, cx);
                    },
                )),
            );
        Some(
            deferred(
                div()
                    .absolute()
                    .occlude().inset_0()
                    .when(!leaving, |el| {
                        el.on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|view, _, _, cx| {
                                view.space_menu = None;
                                cx.notify();
                            }),
                        )
                    })
                    .child(
                        anchored().position(point(at.x, at.y)).anchor(Corner::TopLeft).snap_to_window_with_margin(px(8.)).child(
                            div()
                                .id("space-menu-card")
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .child(shown.wrap(gpui_m3::Exit::Sink, menu)),
                        ),
                    ),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    fn start_space_rename(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self.workspaces.get(id).map(|w| w.name.clone()) else {
            return;
        };
        self.renaming_space = Some(id);
        self.space_rename_had_focus = false;
        self.space_rename_field.update(cx, |field, cx| field.set_text(name, cx));
        self.space_rename_field.read(cx).focus(window);
        cx.notify();
    }

    /// Guarda el nombre del espacio (vacío vuelve al que sale de las carpetas).
    pub(super) fn commit_space_rename(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.renaming_space.take() else {
            return;
        };
        let name = self.space_rename_field.read(cx).text().to_string();
        if self.workspaces.get(id).is_some_and(|w| w.name != name.trim()) {
            self.workspaces.rename(id, &name);
        }
        cx.notify();
    }

    pub(super) fn cancel_space_rename(&mut self, cx: &mut Context<Self>) {
        self.renaming_space = None;
        cx.notify();
    }

    /// Guarda los renombres cuando su campo pierde el foco, como la referencia.
    pub(super) fn commit_renames_on_blur(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.renaming.is_some() {
            if self.rename_field.read(cx).is_focused(window) {
                self.rename_had_focus = true;
            } else if self.rename_had_focus {
                self.commit_rename(cx);
            }
        }
        if self.renaming_space.is_some() {
            if self.space_rename_field.read(cx).is_focused(window) {
                self.space_rename_had_focus = true;
            } else if self.space_rename_had_focus {
                self.commit_space_rename(cx);
            }
        }
    }

    /// El diálogo «Nuevo espacio»: un nombre y sus carpetas (`Overlays.tsx` de la referencia).
    pub(super) fn open_new_space(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.new_space = Some(Vec::new());
        self.editing_space = None;
        self.space_name_field.update(cx, |field, cx| field.set_text("", cx));
        self.space_name_field.read(cx).focus(window);
        cx.notify();
        // Lo que hay en VS Code y Cursor se busca aparte: lee varios archivos.
        cx.spawn(async move |this, cx| {
            let found = cx.background_spawn(async move { super::vscode::detected() }).await;
            let _ = this.update(cx, |view, cx| {
                view.vscode_found = found;
                cx.notify();
            });
        })
        .detach();
    }

    /// Deja un espacio de VS Code en el diálogo «Nuevo espacio», con su nombre, para crearlo.
    fn use_imported(&mut self, imported: super::vscode::Imported, window: &mut Window, cx: &mut Context<Self>) {
        self.new_space = Some(imported.folders);
        self.editing_space = None;
        self.space_name_field.update(cx, |field, cx| field.set_text(&imported.name, cx));
        self.space_name_field.read(cx).focus(window);
        cx.notify();
    }

    /// «Editar proyecto…»: el mismo diálogo con su nombre y sus carpetas, para agregar o quitar.
    pub(super) fn open_edit_space(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(space) = self.workspaces.get(id) else {
            return;
        };
        let (name, folders) = (space.name.clone(), space.folders.clone());
        self.new_space = Some(folders);
        self.editing_space = Some(id);
        self.space_name_field.update(cx, |field, cx| field.set_text(&name, cx));
        self.space_name_field.read(cx).focus(window);
        cx.notify();
    }

    /// Elige un `.code-workspace` de VS Code y deja sus carpetas en el diálogo
    /// «Nuevo espacio», con el nombre del archivo, para revisarlas y crear.
    pub(super) fn import_vscode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(gpui::PathPromptOptions { files: true, directories: false, multiple: false, prompt: Some(crate::i18n::t("pill.code.sidebar.import").into()) });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(file) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update_in(cx, |view, window, cx| match super::vscode::read(&file) {
                Ok(imported) => {
                    let note = super::vscode::missing_note(&imported.folders);
                    view.use_imported(imported, window, cx);
                    view.show_toast(note.map_or_else(|| crate::i18n::t("pill.code.sidebar.imported").to_string(), |n| tf("pill.code.sidebar.importedNote", &[("note", &n)])), cx);
                }
                Err(error) => view.show_toast(error, cx),
            });
        })
        .detach();
    }

    fn add_new_space_folders(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(gpui::PathPromptOptions { files: false, directories: true, multiple: true, prompt: Some(crate::i18n::t("pill.code.sidebar.add").into()) });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let _ = this.update(cx, |view, cx| {
                if let Some(folders) = view.new_space.as_mut() {
                    for path in paths {
                        if !folders.iter().any(|f| crate::kit::workspaces::same(f, &path)) {
                            folders.push(path);
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn create_space(&mut self, cx: &mut Context<Self>) {
        let Some(folders) = self.new_space.clone().filter(|f| !f.is_empty()) else {
            return;
        };
        self.new_space = None;
        let name = self.space_name_field.read(cx).text().trim().to_string();
        if let Some(id) = self.editing_space.take() {
            self.workspaces.set_folders(id, folders);
            self.workspaces.rename(id, &name);
            cx.notify();
            return;
        }
        let id = self.workspaces.create(folders);
        if !name.is_empty() {
            self.workspaces.rename(id, &name);
        }
        self.history_page = false;
        self.select_workspace(id, cx);
    }

    pub(super) fn new_space_dialog(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let shown = self.new_space_last.show("new-space-presence", self.new_space.clone(), window, cx)?;
        let folders = &shown.value;
        let progress = shown.progress;
        let editing = self.editing_space.is_some();
        let t = t();
        let auto = crate::kit::workspaces::name_for(folders);
        let mut list = div().flex().flex_col().gap(px(2.));
        for (index, folder) in folders.iter().enumerate() {
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .pl(px(12.))
                    .pr(px(4.))
                    .h(px(40.))
                    .rounded(px(14.))
                    .bg(t.hover)
                    .child(gpui_m3::Icon::new("folder").size(px(16.)).color(t.accent))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(div().truncate().font_weight(FontWeight::MEDIUM).text_color(t.text).child(crate::kit::workspaces::short_name(folder)))
                            .child(div().truncate().text_size(px(11.5)).text_color(t.muted).child(folder.display().to_string())),
                    )
                    .child(IconButton::new(("new-space-remove", index), "x").size(px(28.)).tooltip(crate::i18n::t("pill.code.sidebar.remove")).on_click(cx.listener(
                        move |view, _: &ClickEvent, _, cx| {
                            if let Some(folders) = view.new_space.as_mut() {
                                if index < folders.len() {
                                    folders.remove(index);
                                }
                            }
                            cx.notify();
                        },
                    ))),
            );
        }
        if folders.is_empty() {
            list = list.child(div().py(px(10.)).text_color(t.muted).child(crate::i18n::t("pill.code.sidebar.pickFolders")));
        }
        // Sin carpetas elegidas, los espacios de VS Code y Cursor que todavía no están en Atic.
        let offered: Vec<&super::vscode::Imported> = if editing || !folders.is_empty() {
            Vec::new()
        } else {
            self.vscode_found.iter().filter(|found| !self.workspaces.list().iter().any(|w| found.folders.iter().all(|f| w.contains(f)))).take(4).collect()
        };
        let mut from_vscode = div().flex().flex_col().gap(px(2.));
        for (index, found) in offered.iter().enumerate() {
            let imported = (*found).clone();
            let detail = tf("pill.code.sidebar.vscodeFolders", &[("n", &found.folders.len()), ("names", &found.folders.iter().map(|f| crate::kit::workspaces::short_name(f)).collect::<Vec<_>>().join(", "))]);
            from_vscode = from_vscode.child(
                div()
                    .id(("new-space-found", index))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .pl(px(12.))
                    .pr(px(10.))
                    .h(px(44.))
                    .rounded(px(14.))
                    .cursor_pointer()
                    .hover(|el| el.bg(t.hover))
                    .child(gpui_m3::Icon::new("import").size(px(16.)).color(t.accent))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .child(div().truncate().font_weight(FontWeight::MEDIUM).text_color(t.text).child(found.name.clone()))
                            .child(div().truncate().text_size(px(11.5)).text_color(t.muted).child(detail)),
                    )
                    .child(div().flex_none().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(t.accent).child(crate::i18n::t("pill.code.sidebar.use")))
                    .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                        if let Some(note) = super::vscode::missing_note(&imported.folders) {
                            view.show_toast(note, cx);
                        }
                        view.use_imported(imported.clone(), window, cx);
                    })),
            );
        }
        let hint = if folders.is_empty() { crate::i18n::t("pill.code.sidebar.noNameFolders").to_string() } else { tf("pill.code.sidebar.nameAuto", &[("auto", &auto)]) };
        Some(
            Dialog::new("new-space")
                .exit(progress)
                .title(crate::i18n::t(if editing { "pill.code.sidebar.editSpace" } else { "pill.code.sidebar.newSpace" }))
                .width(px(460.))
                .on_dismiss(cx.listener(|view, _: &ClickEvent, _, cx| {
                    view.new_space = None;
                    cx.notify();
                }))
                .child(self.space_name_field.clone())
                .child(div().mt(px(-8.)).px(px(4.)).text_size(px(12.)).text_color(t.muted).child(hint))
                .child(div().text_size(px(14.)).font_weight(FontWeight::BOLD).text_color(t.accent).child(crate::i18n::t("pill.code.sidebar.folders")))
                .child(list)
                .when(!offered.is_empty(), |el| {
                    el.child(div().text_size(px(14.)).font_weight(FontWeight::BOLD).text_color(t.accent).child(crate::i18n::t("pill.code.sidebar.fromVscode"))).child(from_vscode)
                })
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(8.))
                        .child(
                            Button::new("new-space-add", crate::i18n::t("pill.code.sidebar.addFolders"))
                                .tonal()
                                .icon("folder-plus")
                                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.add_new_space_folders(cx))),
                        )
                        .when(!editing, |el| {
                            el.child(
                                Button::new("new-space-vscode", crate::i18n::t(if offered.is_empty() { "pill.code.sidebar.importVscode" } else { "pill.code.sidebar.importVscodeFile" }))
                                    .tonal()
                                    .icon("import")
                                    .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.import_vscode(window, cx))),
                            )
                        }),
                )
                .action(Button::new("new-space-cancel", crate::i18n::t("pill.code.sidebar.cancel")).text().on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                    view.new_space = None;
                    cx.notify();
                })))
                .action(
                    Button::new("new-space-create", crate::i18n::t(if editing { "pill.code.sidebar.save" } else { "pill.code.sidebar.create" }))
                        .filled()
                        .disabled(folders.is_empty())
                        .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.create_space(cx))),
                )
                .into_any_element(),
        )
    }

    /// Renombrar la conversación visible desde su título (`TopBar` de la referencia).
    pub(super) fn start_header_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.active_chat() else {
            return;
        };
        let Some(session_id) = chat.session_id.clone() else {
            return;
        };
        let target = SessionRef { workspace: chat.workspace, session_id, title: chat.title.clone() };
        self.start_rename(target, window, cx);
        self.rename_in_header = true;
    }

    /// Abre o cierra el menú rápido de Apariencia (`StyleMenu` de la referencia).
    pub(super) fn toggle_style_menu(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        if self.was_dismissed(super::Dismissed::Style) {
            return;
        }
        self.style_menu = if self.style_menu.is_some() { None } else { Some(at) };
        cx.notify();
    }

    /// El menú rápido de Apariencia: estilo, modo y acento en un popover junto
    /// al botón; es el mismo contenido de la pestaña de Configuración.
    pub(super) fn style_menu_layer(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let shown = self.style_menu_last.show("style-menu-presence", self.style_menu, window, cx)?;
        let (at, leaving) = (shown.value, shown.leaving);
        let t = t();
        let content = if super::view::expressive() { self.appearance_tab(cx) } else { self.appearance_tab_flat(cx) };
        let card = gpui_m3::Popover::new("style-menu")
            .width(px(500.))
            .padding(px(18.))
            .radius(px(28.))
            .gap(px(14.))
            .max_h(px(620.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(div().flex_1().text_size(px(20.)).font_weight(FontWeight(750.)).text_color(t.text).child(crate::i18n::t("pill.code.sidebar.appearance")))
                    .child(IconButton::new("style-menu-close", "x").size(px(32.)).tooltip(crate::i18n::t("pill.code.sidebar.close")).on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                        view.style_menu = None;
                        cx.notify();
                    }))),
            )
            .child(content);
        Some(
            deferred(
                div()
                    .absolute()
                    .occlude().inset_0()
                    .when(!leaving, |el| {
                        el.on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|view, _, _, cx| {
                                view.style_menu = None;
                                view.note_dismiss(super::Dismissed::Style);
                                cx.notify();
                            }),
                        )
                    })
                    .child(
                        anchored()
                            .position(point(at.x - px(16.), at.y + px(18.)))
                            .anchor(Corner::BottomLeft)
                            .snap_to_window_with_margin(px(8.))
                            .child(
                                div()
                                    .id("style-menu-wrap")
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                    .child(shown.wrap(gpui_m3::Exit::Rise, card)),
                            ),
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    fn start_rename(&mut self, target: SessionRef, window: &mut Window, cx: &mut Context<Self>) {
        let title = target.title.clone();
        self.renaming = Some(target);
        self.rename_in_header = false;
        self.rename_had_focus = false;
        self.rename_field.update(cx, |field, cx| field.set_text(title, cx));
        self.rename_field.read(cx).focus(window);
        cx.notify();
    }

    /// Guarda el nombre nuevo (vacío o igual no cambia nada).
    pub(super) fn commit_rename(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.renaming.take() else {
            return;
        };
        self.rename_in_header = false;
        let title = self.rename_field.read(cx).text().trim().to_string();
        cx.notify();
        if title.is_empty() || title == target.title {
            return;
        }
        let Some(dir) = self.workspace_dir(target.workspace) else {
            return;
        };
        if let Some(chat) = self.chats.iter_mut().find(|c| c.session_id.as_deref() == Some(target.session_id.as_str())) {
            chat.title = title.clone();
        }
        let workspace = target.workspace;
        self.request("renameSession", json!({ "sessionId": target.session_id, "title": title, "dir": dir }), cx, move |view, reply, cx| {
            if let Err(error) = reply {
                view.show_toast(error, cx);
            }
            view.load_history_for(workspace, cx);
        });
    }

    pub(super) fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        self.renaming = None;
        self.rename_in_header = false;
        cx.notify();
    }

    fn delete_session(&mut self, target: SessionRef, cx: &mut Context<Self>) {
        let Some(dir) = self.workspace_dir(target.workspace) else {
            return;
        };
        let keys: Vec<String> = self.chats.iter().filter(|c| c.session_id.as_deref() == Some(target.session_id.as_str())).map(|c| c.key.clone()).collect();
        for key in keys {
            self.close_chat(&key, cx);
        }
        self.marks.forget(&target.session_id);
        let workspace = target.workspace;
        self.request("deleteSession", json!({ "sessionId": target.session_id, "dir": dir }), cx, move |view, reply, cx| {
            match reply {
                Ok(_) => view.show_toast(crate::i18n::t("pill.code.sidebar.deleted"), cx),
                Err(error) => view.show_toast(error, cx),
            }
            view.load_history_for(workspace, cx);
        });
    }

    /// «Nueva conversación»: en el proyecto activo y fuera del historial.
    pub(super) fn new_conversation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.history_page = false;
        if let Some(id) = self.active_workspace() {
            self.new_chat(id, window, cx);
        }
        cx.notify();
    }
}

/// Hasta dos iniciales en mayúscula; «?» si no hay nombre.
pub(super) fn initials(name: &str) -> String {
    let letters: String = name.split_whitespace().take(2).filter_map(|word| word.chars().next()).flat_map(char::to_uppercase).collect();
    if letters.is_empty() || name == "Tu nombre" { "?".into() } else { letters }
}

/// El nombre de `git config user.name`, para el perfil.
pub(super) fn git_user_name() -> Option<String> {
    let mut command = std::process::Command::new("git");
    command.args(["config", "--global", "user.name"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let output = command.output().ok()?;
    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!name.is_empty()).then_some(name)
}

/// La fecha de la última actividad de una sesión del sidecar.
pub(super) fn modified_of(session: &Value) -> Option<f64> {
    session.get("lastModified").and_then(Value::as_f64)
}

/// El comienzo de un borrador para el globito de la fila: sin espacios de más, en una
/// línea y con «…» si se corta. `None` si el borrador está en blanco.
fn draft_preview(draft: &str) -> Option<String> {
    const MAX: usize = 140;
    let flat = draft.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.is_empty() {
        return None;
    }
    if flat.chars().count() <= MAX {
        return Some(flat);
    }
    let cut: String = flat.chars().take(MAX).collect();
    Some(format!("{}…", cut.trim_end()))
}

/// La fila del menú de una conversación que la sube o la baja un lugar dentro de su grupo.
#[allow(clippy::too_many_arguments)]
fn move_item(cx: &mut Context<CodeView>, id: &'static str, label: &'static str, icon: &'static str, workspace: u64, session: &str, scope: Option<u64>, delta: i32) -> MenuItem {
    let session = session.to_string();
    MenuItem::new(id, label).icon(icon).on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
        view.session_menu = None;
        view.move_session(workspace, &session, scope, delta, cx);
    }))
}

/// La fila del menú de una conversación que la manda a «Hechas», o la devuelve a las activas.
fn settle_item(cx: &mut Context<CodeView>, settled: bool, session: &str) -> MenuItem {
    let session = session.to_string();
    let (id, label) = if settled { ("session-unsettle", crate::i18n::t("pill.code.sidebar.unsettle")) } else { ("session-settle", crate::i18n::t("pill.code.sidebar.settle")) };
    MenuItem::new(id, label).icon("check").on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
        view.session_menu = None;
        if settled {
            view.unsettle_session(&session, cx);
        } else {
            view.settle_session(&session, cx);
        }
    }))
}

/// Una fila del menú de una conversación para posponerla `secs` segundos, o, con `None`,
/// para dejar de posponerla.
fn snooze_item(cx: &mut Context<CodeView>, id: &'static str, label: &'static str, secs: Option<u64>, session: &str) -> MenuItem {
    let session = session.to_string();
    MenuItem::new(id, label).icon("history").on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
        view.session_menu = None;
        match secs {
            Some(secs) => view.snooze_session(&session, secs, cx),
            None => view.unsnooze_session(&session, cx),
        }
    }))
}

#[cfg(test)]
mod draft_tests {
    use super::draft_preview;

    #[test]
    fn un_borrador_en_blanco_no_se_marca() {
        assert_eq!(draft_preview(""), None);
        assert_eq!(draft_preview("  \n\t "), None);
    }

    #[test]
    fn el_borrador_corto_queda_en_una_linea() {
        assert_eq!(draft_preview("  hola\n\nmundo  "), Some("hola mundo".to_string()));
    }

    #[test]
    fn el_borrador_largo_se_corta_con_puntos() {
        let long = "á".repeat(300);
        let preview = draft_preview(&long).unwrap();
        assert_eq!(preview.chars().count(), 141);
        assert!(preview.ends_with('…'));
    }
}

#[cfg(test)]
mod subagent_tests {
    use super::{nest_subagents, Conv};
    use crate::code::chat::Chat;
    use crate::code::delegate::Delegation;

    fn chat(key: &str, parent: Option<&str>) -> Chat {
        let mut chat = Chat::new(key.into(), 0, std::path::PathBuf::from("."));
        chat.delegation = parent.map(|parent| Delegation {
            parent: parent.into(),
            root: parent.into(),
            depth: 1,
            role: None,
            backend: "codex".into(),
            permission_mode: None,
            external: false,
            started: std::time::Instant::now(),
            wake: false,
            waiters: Vec::new(),
            last: None,
        });
        chat
    }

    fn conv(chat: &Chat) -> Conv<'_> {
        (0, None, Some(chat))
    }

    fn keys(convs: &[Conv<'_>]) -> Vec<String> {
        convs.iter().filter_map(|conv| conv.2.map(|c| c.key.clone())).collect()
    }

    #[test]
    fn los_hijos_van_bajo_su_padre_si_se_ve() {
        let (padre, hijo, nieto, otro, huerfano) = (chat("c1", None), chat("c2", Some("c1")), chat("c3", Some("c2")), chat("c4", None), chat("c5", Some("c9")));
        // El hijo trabaja (otro grupo que el padre) y el nieto está en las Hechas.
        let mut groups = [vec![], vec![conv(&hijo)], vec![conv(&padre), conv(&otro), conv(&huerfano)], vec![conv(&nieto)]];
        let nested = nest_subagents(&mut groups, 10);
        assert_eq!(keys(&nested), vec!["c2", "c3"]);
        assert!(groups[1].is_empty() && groups[3].is_empty());
        // El de un padre que no está queda como cualquier conversación, en su lugar.
        assert_eq!(keys(&groups[2]), vec!["c1", "c4", "c5"]);
    }

    #[test]
    fn el_hijo_de_un_padre_fuera_del_tope_se_ve_suelto() {
        let (padre, hijo) = (chat("c1", None), chat("c2", Some("c1")));
        let otros: Vec<Chat> = (10..13).map(|n| chat(&format!("c{n}"), None)).collect();
        let mut active: Vec<Conv<'_>> = otros.iter().map(conv).collect();
        active.push(conv(&padre));
        let mut groups = [vec![conv(&hijo)], vec![], active, vec![]];
        // Con tope 3 el padre no se ve: el hijo sigue en las Fijadas.
        assert!(nest_subagents(&mut groups, 3).is_empty());
        assert_eq!(keys(&groups[0]), vec!["c2"]);
    }
}

#[cfg(test)]
mod dedupe_tests {
    use super::{dedupe_sessions, SessionInfo};

    fn info(id: &str) -> SessionInfo {
        SessionInfo { session_id: id.into(), title: String::new(), modified: None, branch: None }
    }

    #[test]
    fn una_sesion_de_dos_proyectos_iguales_va_una_vez() {
        let (a, b) = (info("s1"), info("s2"));
        let all = vec![(1, &a), (2, &a), (1, &b)];
        let out: Vec<(u64, &str)> = dedupe_sessions(all, |_| None).into_iter().map(|(w, i)| (w, i.session_id.as_str())).collect();
        assert_eq!(out, vec![(1, "s1"), (1, "s2")]);
    }

    #[test]
    fn queda_en_el_proyecto_de_su_chat_abierto() {
        let a = info("s1");
        let out = dedupe_sessions(vec![(1, &a), (2, &a)], |_| Some(2));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, 2);
    }
}
