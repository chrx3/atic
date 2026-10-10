//! La barra lateral de Expressive, como la de la referencia: «Nueva conversación»,
//! Historial, los proyectos (cada uno se pliega y muestra sus últimas
//! conversaciones) y abajo el perfil, Apariencia y Configuración. Plegada
//! queda el riel de M3. También la página de historial y el menú contextual
//! de una conversación (abrir, renombrar, eliminar).

use gpui::{
    anchored, deferred, div, point, prelude::*, px, AnyElement, ClickEvent, Context, Corner, Div, FontWeight, MouseButton, MouseDownEvent,
    Pixels, Point, SharedString, Window,
};
use gpui_m3::{Avatar, Badge, Button, Chip, Dialog, Fab, FavStar, IconButton, LoadingIndicator, MenuItem, NavItem, RailItem, Tone};
use serde_json::{json, Value};

use super::style::t;
use super::{CodeView, SessionInfo, LOOSE};

const SIDE_W: f32 = 244.;
const RAIL_W: f32 = 80.;
const TOP_H: f32 = 48.;
/// Conversaciones a la vista bajo cada proyecto.
const SHOWN: usize = 3;
/// Chats sueltos a la vista.
const LOOSE_SHOWN: usize = 5;

/// Una conversación sobre la que se abrió el menú contextual o se renombra.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SessionRef {
    pub workspace: u64,
    pub session_id: String,
    pub title: String,
}

/// «ahora», «5 min», «3 h», «2 d»; acepta segundos o milisegundos.
pub(super) fn ago(time: f64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.);
    let seconds = if time > 1e12 { time / 1000. } else { time };
    let elapsed = (now - seconds).max(0.);
    if elapsed < 60. {
        "ahora".into()
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
        .mt(px(12.))
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

fn hint(text: &'static str) -> Div {
    div().px(px(10.)).py(px(4.)).text_size(px(12.)).text_color(t().faint).child(text)
}

impl CodeView {
    pub(super) fn sidebar_m3(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.sidebar_open {
            return self.rail(cx);
        }
        let top = div()
            .h(px(TOP_H))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.))
            .px(px(10.))
            .child(crate::space::chrome::drag(TOP_H))
            .child(IconButton::new("side-search", "search").size(px(32.)).tooltip("Comandos (Ctrl+K)").on_click(cx.listener(
                |view, _: &ClickEvent, window, cx| view.open_palette(window, cx),
            )))
            .child(IconButton::new("side-hide", "panel-left").size(px(32.)).tooltip("Barra lateral (Ctrl+B)").on_click(cx.listener(
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
            .child(div().flex().mt(px(2.)).mb(px(8.)).child(Fab::new("chat-new", "plus").label("Nueva conversación").hover_radius(22.).on_click(on_new)))
            .child(NavItem::new("nav-history", "Historial").icon("history").selected(self.history_page).on_click(cx.listener(
                |view, _: &ClickEvent, _, cx| {
                    view.history_page = true;
                    view.load_history(cx);
                    cx.notify();
                },
            )));

        let mut projects = div().flex().flex_col();
        // TODO(gpui-m3): ReorderList/DragHandle para reordenar los espacios arrastrando
        // (Sidebar.tsx:313-351 de la referencia); por ahora, favoritos primero y el orden de la lista.
        let ids: Vec<u64> = self.workspaces.list().iter().map(|w| w.id).collect();
        for id in super::config::sidebar_order(&ids, &self.configs.favorites) {
            if let Some(workspace) = self.workspaces.get(id) {
                projects = projects.child(self.project_m3(workspace.id, workspace.name.clone(), workspace.collapsed, cx));
            }
        }
        if self.workspaces.list().is_empty() {
            projects = projects.child(hint("Agrega una carpeta para empezar."));
        }
        let scroll = div()
            .id("side-scroll")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .px(px(8.))
            .pb(px(12.))
            .child(section(
                "Proyectos",
                IconButton::new("ws-create", "folder-plus")
                    .size(px(32.))
                    .tooltip("Nuevo espacio")
                    .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.open_new_space(window, cx))),
            ))
            .child(projects)
            .child(section(
                "Chats",
                IconButton::new("loose-new", "plus")
                    .size(px(32.))
                    .tooltip("Nuevo chat sin proyecto")
                    .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.new_loose_chat(window, cx))),
            ))
            .child(self.loose_chats(cx));
        div()
            .w(px(SIDE_W))
            .flex_none()
            .flex()
            .flex_col()
            .child(top)
            .child(nav)
            .child(scroll)
            .child(div().p(px(8.)).child(self.side_profile(cx)))
            .into_any_element()
    }

    fn project_m3(&self, id: u64, name: String, collapsed: bool, cx: &mut Context<Self>) -> Div {
        let active = self.workspaces.active_id() == Some(id);
        let in_list = self.active_chat().is_some_and(|c| c.workspace == id);
        let group = SharedString::from(format!("project-{id}"));
        let new_here = IconButton::new(("project-new", id as usize), "plus").size(px(22.)).tooltip("Nueva conversación").on_click(cx.listener(
            move |view, _: &ClickEvent, window, cx| {
                cx.stop_propagation();
                view.history_page = false;
                view.new_chat(id, window, cx);
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
        let row = NavItem::new(("project", id as usize), name.clone())
            .group(group.clone())
            .leading(Avatar::new(name).size(px(22.)))
            .selected(active && !self.history_page && !in_list && collapsed)
            .trailing(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .child(star)
                    .child(div().invisible().group_hover(group, |el| el.visible()).child(new_here)),
            )
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                view.workspaces.toggle(id);
                if view.workspaces.get(id).is_some_and(|w| !w.collapsed) && !view.history.contains_key(&id) {
                    view.load_history_for(id, cx);
                }
                cx.notify();
            }));
        let row = div()
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |view, event: &MouseDownEvent, _, cx| {
                    view.space_menu = Some((id, event.position));
                    cx.notify();
                }),
            )
            .child(row);
        let mut block = div().mt(px(2.)).flex().flex_col().child(row);
        if !collapsed {
            block = block.child(self.project_convs(id, cx));
        }
        block
    }

    /// Los chats sueltos (sin proyecto), al final de la barra: los cinco
    /// últimos y «Ver todos (n)», como en la referencia.
    fn loose_chats(&self, cx: &mut Context<Self>) -> Div {
        let mut list = div().flex().flex_col().gap(px(1.)).pb(px(4.));
        let live: Vec<&super::Chat> = self.chats.iter().filter(|c| c.workspace == LOOSE && c.session_id.is_none() && !c.items.is_empty()).collect();
        let sessions: Vec<&SessionInfo> = self.history.get(&LOOSE).into_iter().flatten().collect();
        let total = live.len() + sessions.len();
        if total == 0 {
            return list.child(hint("Chats que no son de ningún proyecto"));
        }
        let limit = if self.loose_all { usize::MAX } else { LOOSE_SHOWN };
        let mut shown = 0;
        for chat in live.into_iter().take(limit) {
            list = list.child(self.session_row(LOOSE, None, Some(chat), cx));
            shown += 1;
        }
        for info in sessions.into_iter().take(limit.saturating_sub(shown)) {
            let chat = self.chats.iter().find(|c| c.session_id.as_deref() == Some(info.session_id.as_str()));
            list = list.child(self.session_row(LOOSE, Some(info), chat, cx));
        }
        if total > LOOSE_SHOWN {
            let label = if self.loose_all { "Ver menos".to_string() } else { format!("Ver todos ({total})") };
            list = list.child(NavItem::new("loose-all", label).dense(true).muted(true).on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                view.loose_all = !view.loose_all;
                cx.notify();
            })));
        }
        list
    }

    /// Las conversaciones de un proyecto: primero las abiertas sin guardar, luego el historial.
    fn project_convs(&self, workspace: u64, cx: &mut Context<Self>) -> Div {
        let mut list = div().flex().flex_col().gap(px(1.)).pt(px(1.)).pb(px(4.)).pl(px(26.));
        let live: Vec<&super::Chat> = self.chats.iter().filter(|c| c.workspace == workspace && c.session_id.is_none() && !c.items.is_empty()).collect();
        let sessions: Vec<&SessionInfo> = self.history.get(&workspace).into_iter().flatten().collect();
        let total = live.len() + sessions.len();
        if total == 0 {
            return list.child(hint(if self.history.contains_key(&workspace) { "Sin conversaciones" } else { "Cargando…" }));
        }
        let mut shown = 0;
        for chat in live.into_iter().take(SHOWN) {
            list = list.child(self.session_row(workspace, None, Some(chat), cx));
            shown += 1;
        }
        for info in sessions.into_iter().take(SHOWN - shown.min(SHOWN)) {
            let chat = self.chats.iter().find(|c| c.session_id.as_deref() == Some(info.session_id.as_str()));
            list = list.child(self.session_row(workspace, Some(info), chat, cx));
        }
        if total > SHOWN {
            list = list.child(
                NavItem::new(("project-all", workspace as usize), format!("Ver todas ({total})")).dense(true).muted(true).on_click(cx.listener(
                    move |view, _: &ClickEvent, _, cx| {
                        view.select_workspace(workspace, cx);
                        view.history_page = true;
                        cx.notify();
                    },
                )),
            );
        }
        list
    }

    fn session_row(&self, workspace: u64, info: Option<&SessionInfo>, chat: Option<&super::Chat>, cx: &mut Context<Self>) -> AnyElement {
        let title = chat.filter(|c| info.is_none() || c.title != "Nueva conversación").map(|c| c.title.clone()).or_else(|| info.map(|i| i.title.clone())).unwrap_or_default();
        let running = chat.is_some_and(|c| c.busy || !c.permissions.is_empty());
        let unread = !running && chat.is_some_and(|c| c.unread);
        let on = !self.history_page && chat.is_some_and(|c| self.active.as_deref() == Some(c.key.as_str()));
        let key = chat.map(|c| c.key.clone());
        let session = info.cloned();
        let row_id = SharedString::from(match (&session, &key) {
            (Some(s), _) => format!("session-{}", s.session_id),
            (None, Some(k)) => format!("chat-{k}"),
            _ => "session".into(),
        });
        let open_key = key.clone();
        let open_session = session.clone();
        let session_id = session.as_ref().map(|s| s.session_id.clone()).or_else(|| chat.and_then(|c| c.session_id.clone()));
        let bookmarked = session_id.is_some_and(|s| self.is_bookmarked(&s));
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
        } else if let Some(modified) = info.and_then(|i| i.modified) {
            row = row.meta(ago(modified));
        }
        let renaming = !self.rename_in_header && self.renaming.as_ref().zip(session.as_ref()).is_some_and(|(r, s)| r.session_id == s.session_id);
        if renaming {
            return div().child(self.rename_field.clone()).into_any_element();
        }
        let target = session.map(|s| SessionRef { workspace, session_id: s.session_id, title });
        div()
            .when_some(target, |el, target| {
                el.on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |view, event: &MouseDownEvent, _, cx| {
                        view.session_menu = Some((target.clone(), event.position));
                        cx.notify();
                    }),
                )
            })
            .child(row)
            .into_any_element()
    }

    /// Pie de la barra: el perfil, Apariencia y Configuración.
    fn side_profile(&self, cx: &mut Context<Self>) -> Div {
        let name = self.user_name.clone().unwrap_or_else(|| "Tu nombre".into());
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
                    .tooltip(crate::hover::tip("Tu perfil"))
                    .child(gpui_m3::Shape::new(gpui_m3::ShapeName::Cookie9).size(px(30.)).color(gpui_m3::Theme::of(cx).tertiary_container).child(
                        div().text_size(px(11.)).font_weight(FontWeight::SEMIBOLD).text_color(gpui_m3::Theme::of(cx).on_tertiary_container).child(initials(&name)),
                    ))
                    .child(div().min_w(px(0.)).truncate().font_weight(FontWeight::MEDIUM).child(name)),
            )
            .child(IconButton::new("side-style", "palette").size(px(32.)).tooltip("Apariencia").on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                view.open_appearance(cx);
            })))
            .child(IconButton::new("side-settings", "gear").size(px(32.)).tooltip("Configuración (Ctrl+,)").on_click(cx.listener(
                |view, _: &ClickEvent, _, cx| {
                    view.settings_open = !view.settings_open;
                    cx.notify();
                },
            )))
    }

    /// La barra plegada: el riel de M3.
    fn rail(&self, cx: &mut Context<Self>) -> AnyElement {
        let name = self.user_name.clone().unwrap_or_else(|| "Tu nombre".into());
        div()
            .w(px(RAIL_W))
            .flex_none()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.))
            .pb(px(8.))
            .child(div().h(px(12.)).w_full().child(crate::space::chrome::drag(12.)))
            .child(IconButton::new("rail-menu", "panel-left").size(px(40.)).tooltip("Barra lateral (Ctrl+B)").on_click(cx.listener(
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
            .child(RailItem::new("rail-chat", "chat", "Chat").selected(!self.history_page).on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                view.history_page = false;
                cx.notify();
            })))
            .child(RailItem::new("rail-history", "history", "Historial").selected(self.history_page).on_click(cx.listener(
                |view, _: &ClickEvent, _, cx| {
                    view.history_page = true;
                    view.load_history(cx);
                    cx.notify();
                },
            )))
            .child(RailItem::new("rail-search", "search", "Buscar").selected(self.palette_open).on_click(cx.listener(
                |view, _: &ClickEvent, window, cx| view.open_palette(window, cx),
            )))
            .child(div().flex_1().w_full().child(crate::space::chrome::drag(200.)))
            .child(IconButton::new("rail-style", "palette").size(px(40.)).tooltip("Apariencia").on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                view.open_appearance(cx);
            })))
            .child(IconButton::new("rail-settings", "gear").size(px(40.)).tooltip("Configuración (Ctrl+,)").on_click(cx.listener(
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
                    .tooltip(crate::hover::tip("Abrir la barra lateral"))
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                        view.sidebar_open = true;
                        cx.notify();
                    }))
                    .child(Avatar::new(name).size(px(36.))),
            )
            .into_any_element()
    }

    /// La página de historial del proyecto activo, con búsqueda.
    pub(super) fn history_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let query = self.history_search.read(cx).text().trim().to_lowercase();
        let workspace = self.workspaces.active_id();
        let marked = self.history_marked;
        let sessions: Vec<SessionInfo> = workspace
            .and_then(|id| self.history.get(&id))
            .into_iter()
            .flatten()
            .filter(|s| !marked || self.is_bookmarked(&s.session_id))
            .filter(|s| query.is_empty() || s.title.to_lowercase().contains(&query))
            .cloned()
            .collect();
        let filters = div().mb(px(10.)).flex().items_center().gap(px(8.)).child(div().flex_1().child(self.history_search.clone())).child(
            Chip::new("history-marked", "Marcadores").icon("bookmark").filter(marked).on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                view.history_marked = !view.history_marked;
                cx.notify();
            })),
        );
        let none_marked = marked && sessions.is_empty();
        let muted = t().muted;
        let mut list = div().flex().flex_col().gap(px(2.));
        for info in sessions {
            list = list.child(self.history_row(workspace.unwrap_or_default(), info, cx));
        }
        let empty = match workspace {
            None => Some("Abre un proyecto para ver sus conversaciones."),
            Some(id) if self.history.get(&id).is_none_or(|l| l.is_empty()) => Some("Todavía no hay conversaciones en este proyecto."),
            _ if none_marked => Some("No hay conversaciones con marcador. Agrégalas desde el menú de la conversación o con el clic derecho."),
            _ => None,
        };
        div()
            .id("history")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .child(
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

    fn history_row(&self, workspace: u64, info: SessionInfo, cx: &mut Context<Self>) -> AnyElement {
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
                cx.listener(move |view, event: &MouseDownEvent, _, cx| {
                    view.session_menu = Some((menu_target.clone(), event.position));
                    cx.notify();
                }),
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
                    .when(info.modified.is_some() || info.branch.is_some(), |el| {
                        // «hace X · rama», como el historial de la referencia.
                        let meta = [info.modified.map(ago), info.branch.clone()].into_iter().flatten().collect::<Vec<_>>().join(" · ");
                        el.child(div().text_size(px(12.)).text_color(t.muted).child(meta))
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
                            .tooltip(if bookmarked { "Quitar de marcadores" } else { "Agregar a marcadores" })
                            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.toggle_bookmark(&mark_id, cx))),
                    )
                    .child(IconButton::new(SharedString::from(format!("history-rename-{id}")), "pen").size(px(32.)).tooltip("Renombrar").on_click(
                        cx.listener(move |view, _: &ClickEvent, window, cx| view.start_rename(rename_target.clone(), window, cx)),
                    ))
                    .child(
                        div()
                            .when(confirming, |el| el.rounded(px(16.)).bg(t.bad.opacity(0.14)))
                            .child(
                                IconButton::new(SharedString::from(format!("history-delete-{id}")), "trash")
                                    .size(px(32.))
                                    .tooltip(if confirming { "Clic otra vez para eliminar" } else { "Eliminar" })
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

    /// El menú del clic derecho sobre una conversación.
    pub(super) fn session_menu_layer(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let shown = self.session_menu_last.show("session-menu-presence", self.session_menu.clone(), window, cx)?;
        let (target, at): (SessionRef, Point<Pixels>) = shown.value.clone();
        let leaving = shown.leaving;
        let (open, rename, mark, delete) = (target.clone(), target.clone(), target.clone(), target);
        let bookmarked = self.is_bookmarked(&mark.session_id);
        let menu = gpui_m3::Menu::new("session-menu")
            .width(190.)
            .item(MenuItem::new("session-open", "Abrir").icon("chevron-right").on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                view.session_menu = None;
                view.history_page = false;
                let info = SessionInfo { session_id: open.session_id.clone(), title: open.title.clone(), modified: None, branch: None };
                view.open_session(open.workspace, info, window, cx);
            })))
            .item(MenuItem::new("session-rename", "Renombrar").icon("pen").on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                view.session_menu = None;
                view.start_rename(rename.clone(), window, cx);
            })))
            .item(
                MenuItem::new("session-mark", if bookmarked { "Quitar marcador" } else { "Agregar marcador" }).icon("bookmark").on_click(cx.listener(
                    move |view, _: &ClickEvent, _, cx| {
                        view.session_menu = None;
                        view.toggle_bookmark(&mark.session_id, cx);
                    },
                )),
            )
            .item(MenuItem::new("session-delete", "Eliminar").icon("trash").danger(true).confirm("¿Eliminar? Clic para confirmar").on_click(cx.listener(
                move |view, _: &ClickEvent, _, cx| {
                    view.session_menu = None;
                    view.delete_session(delete.clone(), cx);
                },
            )));
        Some(
            deferred(
                div()
                    .absolute()
                    .inset_0()
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
            .item(MenuItem::new("space-rename", "Renombrar").icon("pen").on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                view.space_menu = None;
                view.start_space_rename(id, window, cx);
            })))
            .item(
                MenuItem::new("space-fav", if favorite { "Quitar de favoritos" } else { "Marcar como favorito" }).icon("star").on_click(cx.listener(
                    move |view, _: &ClickEvent, _, cx| {
                        view.space_menu = None;
                        view.configs.toggle_favorite(id);
                        cx.notify();
                    },
                )),
            )
            .item(MenuItem::new("space-add", "Agregar carpetas…").icon("folder-plus").on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                view.space_menu = None;
                view.pick_folders(Some(id), cx);
            })))
            .item(
                MenuItem::new("space-remove", "Quitar de la lista").icon("x").danger(true).confirm("No borra archivos. Clic para confirmar").on_click(cx.listener(
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
                    .inset_0()
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
        self.space_name_field.update(cx, |field, cx| field.set_text("", cx));
        self.space_name_field.read(cx).focus(window);
        cx.notify();
    }

    fn add_new_space_folders(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(gpui::PathPromptOptions { files: false, directories: true, multiple: true, prompt: Some("Agregar".into()) });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let _ = this.update(cx, |view, cx| {
                if let Some(folders) = view.new_space.as_mut() {
                    for path in paths {
                        if !folders.iter().any(|f| crate::space::workspaces::same(f, &path)) {
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
        let t = t();
        let auto = crate::space::workspaces::name_for(folders);
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
                            .child(div().truncate().font_weight(FontWeight::MEDIUM).text_color(t.text).child(crate::space::workspaces::short_name(folder)))
                            .child(div().truncate().text_size(px(11.5)).text_color(t.muted).child(folder.display().to_string())),
                    )
                    .child(IconButton::new(("new-space-remove", index), "x").size(px(28.)).tooltip("Quitar").on_click(cx.listener(
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
            list = list.child(div().py(px(10.)).text_color(t.muted).child("Elige una o más carpetas: Claude trabaja en todas."));
        }
        let hint = if folders.is_empty() { "Sin nombre, se usa el de las carpetas.".to_string() } else { format!("Sin nombre, se llama «{auto}».") };
        Some(
            Dialog::new("new-space")
                .exit(progress)
                .title("Nuevo espacio")
                .width(px(460.))
                .on_dismiss(cx.listener(|view, _: &ClickEvent, _, cx| {
                    view.new_space = None;
                    cx.notify();
                }))
                .child(self.space_name_field.clone())
                .child(div().mt(px(-8.)).px(px(4.)).text_size(px(12.)).text_color(t.muted).child(hint))
                .child(div().text_size(px(14.)).font_weight(FontWeight::BOLD).text_color(t.accent).child("Carpetas"))
                .child(list)
                .child(
                    div().flex().child(
                        Button::new("new-space-add", "Agregar carpetas…")
                            .tonal()
                            .icon("folder-plus")
                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.add_new_space_folders(cx))),
                    ),
                )
                .action(Button::new("new-space-cancel", "Cancelar").text().on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                    view.new_space = None;
                    cx.notify();
                })))
                .action(
                    Button::new("new-space-create", "Crear")
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

    /// Apariencia abre la configuración en su pestaña.
    pub(super) fn open_appearance(&mut self, cx: &mut Context<Self>) {
        self.settings_tab = 1;
        self.settings_open = true;
        cx.notify();
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
                Ok(_) => view.show_toast("Conversación eliminada", cx),
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
fn initials(name: &str) -> String {
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
