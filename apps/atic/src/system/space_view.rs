//! Liberar espacio, dentro de Sistema: se abre desde la fila del disco de
//! Rendimiento y ocupa el panel entero (bajo la franja) hasta volver.
//!
//! La barra del disco es la protagonista y todo sale de su estado:
//! - Gris: lo usado que se queda. Verde: lo marcado para liberar, que crece
//!   y se achica con cada casilla. Ámbar: lo que ya está en la Papelera.
//!   Lo que queda de la pista: libre.
//! - Al mover a la Papelera, lo verde se vuelve ámbar desde su borde: el
//!   límite entre los dos tramos corre, no se cambia un color por otro.
//! - Al vaciar la Papelera (lo confirma el Finder), lo ámbar se recoge y la
//!   pista libre crece con la lectura nueva del disco.
//!
//! Los grupos entran uno tras otro a medida que el escaneo los termina.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::{
    div, prelude::*, px, rgb, svg, Animation, AnimationExt, AnyElement, ClickEvent, Context, Hsla,
    SharedString, Window,
};

use super::space::{self, Group, Kind, Note, Scan};
use super::{bytes, proc_action, SystemPanel, DOWN_TINT, ROW_H, SIDE};
use crate::anim::Tween;
use crate::hover::hover_fx;

/// Filas de un grupo abierto antes de «y N más».
const MAX_ITEMS: usize = 6;
const ITEM_H: f32 = 28.0;
/// La barra con su leyenda.
const BAR_BLOCK_H: f32 = 50.0;
const FOOTER_H: f32 = 56.0;
const TRASH_TINT: u32 = 0xe8b04b;
/// Lo que tarda la barra en llegar a cada estado.
const BAR_FOR: Duration = Duration::from_millis(520);

#[derive(Clone, PartialEq)]
pub(super) enum Phase {
    Choosing,
    Moving,
    /// Movido a la Papelera: cuánto y cuántos fallaron.
    Moved(u64, usize),
}

pub(super) struct SpaceView {
    shared: Arc<Mutex<Scan>>,
    groups: Vec<Group>,
    current: Option<Kind>,
    done: bool,
    open: Option<Kind>,
    phase: Phase,
    /// Descargas: `None` sin revisar, `Some(false)` revisando, `Some(true)` listo.
    downloads: Option<bool>,
    /// Lo que había en la Papelera al empezar, más lo movido.
    trash: u64,
    /// Fracciones del disco, animadas: lo marcado y la Papelera.
    chosen: Tween,
    trashed: Tween,
}

impl SpaceView {
    fn chosen_bytes(&self) -> u64 {
        self.groups.iter().map(Group::chosen).sum()
    }
}

fn bar_tween() -> Tween {
    Tween::hover(0.0, BAR_FOR, crate::anim::ease_smooth_out)
}

/// «3 d», «5 h», «12 min».
fn ago_short(secs: u64) -> String {
    if secs >= 86_400 {
        format!("{} d", secs / 86_400)
    } else if secs >= 3600 {
        format!("{} h", secs / 3600)
    } else {
        format!("{} min", (secs / 60).max(1))
    }
}

fn kind_label(kind: Kind) -> &'static str {
    crate::i18n::t(match kind {
        Kind::Builds => "pill.system.space.kindBuilds",
        Kind::Tools => "pill.system.space.kindTools",
        Kind::Apps => "pill.system.space.kindApps",
        Kind::Xcode => "pill.system.space.kindXcode",
        Kind::Installers => "pill.system.space.kindInstallers",
    })
}

fn kind_icon(kind: Kind) -> &'static str {
    match kind {
        Kind::Builds => "icons/code.svg",
        Kind::Tools => "icons/layers.svg",
        Kind::Apps => "icons/layout-grid.svg",
        Kind::Xcode => "icons/smartphone.svg",
        Kind::Installers => "icons/download.svg",
    }
}

/// Cómo se ve una casilla.
#[derive(Clone, Copy, PartialEq)]
enum Check {
    Off,
    Some,
    On,
    Locked,
}

impl SystemPanel {
    pub(super) fn space_open(&self) -> bool {
        self.space.is_some()
    }

    /// Abre Liberar espacio y empieza a medir en otro hilo.
    pub(super) fn open_space(&mut self, cx: &mut Context<Self>) {
        let shared = Arc::new(Mutex::new(Scan::default()));
        self.space = Some(SpaceView {
            shared: shared.clone(),
            groups: Vec::new(),
            current: Some(Kind::Builds),
            done: false,
            open: None,
            phase: Phase::Choosing,
            downloads: None,
            trash: 0,
            chosen: bar_tween(),
            trashed: bar_tween(),
        });
        cx.background_spawn({
            let shared = shared.clone();
            async move { space::scan(shared) }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let trash = cx.background_spawn(async { space::trash_size() }).await;
            let _ = this.update(cx, |panel, cx| {
                panel.set_trash(trash);
                cx.notify();
            });
            // Lo que el escaneo va terminando, cada 200 ms.
            loop {
                cx.background_executor().timer(Duration::from_millis(200)).await;
                let go = this
                    .update(cx, |panel, cx| {
                        let Some(view) = panel.space.as_mut() else {
                            return false;
                        };
                        if !Arc::ptr_eq(&view.shared, &shared) {
                            return false;
                        }
                        let scan = shared.lock().map(|s| s.clone()).unwrap_or_default();
                        for group in scan.groups.into_iter().skip(view.groups.len()) {
                            view.groups.push(group);
                        }
                        view.current = scan.current;
                        view.done = scan.done;
                        panel.retarget_bar();
                        cx.notify();
                        !scan.done
                    })
                    .unwrap_or(false);
                if !go {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    pub(super) fn close_space(&mut self, cx: &mut Context<Self>) {
        if self.space.take().is_some() {
            self.space_closes += 1;
            cx.notify();
        }
    }

    /// El total del disco del sistema, de Rendimiento.
    fn disk(&self) -> Option<(u64, u64)> {
        let disk = self.state.vitals.as_ref()?.disks.first()?;
        Some((disk.free, disk.total.max(1)))
    }

    fn set_trash(&mut self, trash: u64) {
        if let Some(view) = self.space.as_mut() {
            view.trash = trash;
        }
        self.retarget_bar();
    }

    /// La barra va hacia lo marcado y lo que hay en la Papelera.
    fn retarget_bar(&mut self) {
        let total = self.disk().map_or(1, |(_, t)| t) as f32;
        let reduce = self.reduce_motion;
        let Some(view) = self.space.as_mut() else {
            return;
        };
        let now = Instant::now();
        let chosen = if view.phase == Phase::Choosing { view.chosen_bytes() as f32 / total } else { 0.0 };
        let trashed = view.trash as f32 / total;
        for (tween, target) in [(&mut view.chosen, chosen), (&mut view.trashed, trashed)] {
            if reduce {
                tween.snap(target);
            } else {
                tween.set(target, now);
            }
        }
    }

    fn toggle_group(&mut self, kind: Kind, cx: &mut Context<Self>) {
        let Some(view) = self.space.as_mut() else {
            return;
        };
        if view.phase != Phase::Choosing {
            return;
        }
        if let Some(group) = view.groups.iter_mut().find(|g| g.kind == kind) {
            let all = group.items.iter().filter(|i| !i.locked()).all(|i| i.checked);
            for item in group.items.iter_mut().filter(|i| !i.locked()) {
                item.checked = !all;
            }
        }
        self.retarget_bar();
        cx.notify();
    }

    fn toggle_item(&mut self, kind: Kind, index: usize, cx: &mut Context<Self>) {
        let Some(view) = self.space.as_mut() else {
            return;
        };
        if view.phase != Phase::Choosing {
            return;
        }
        if let Some(item) = view.groups.iter_mut().find(|g| g.kind == kind).and_then(|g| g.items.get_mut(index)) {
            if !item.locked() {
                item.checked = !item.checked;
            }
        }
        self.retarget_bar();
        cx.notify();
    }

    fn move_to_trash(&mut self, cx: &mut Context<Self>) {
        let Some(view) = self.space.as_mut() else {
            return;
        };
        let picked: Vec<_> = view
            .groups
            .iter()
            .flat_map(|g| g.items.iter().filter(|i| i.checked && !i.locked()))
            .map(|i| (i.path.clone(), i.size))
            .collect();
        if picked.is_empty() || !view.done || view.phase != Phase::Choosing {
            return;
        }
        view.phase = Phase::Moving;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let (moved, errors) = cx.background_spawn(async move { space::trash(&picked) }).await;
            for error in &errors {
                eprintln!("sistema: liberar espacio: {error}");
            }
            let _ = this.update(cx, |panel, cx| {
                if let Some(view) = panel.space.as_mut() {
                    view.phase = Phase::Moved(moved, errors.len());
                    view.trash += moved;
                    // Lo movido ya no está donde estaba: fuera de la lista.
                    for group in &mut view.groups {
                        group.items.retain(|i| !i.checked || i.locked());
                    }
                    view.groups.retain(|g| !g.items.is_empty());
                }
                panel.retarget_bar();
                cx.notify();
            });
        })
        .detach();
    }

    /// Revisa Descargas (pide permiso la primera vez) y suma su grupo.
    fn scan_downloads(&mut self, cx: &mut Context<Self>) {
        let Some(view) = self.space.as_mut() else {
            return;
        };
        if view.downloads.is_some() {
            return;
        }
        view.downloads = Some(false);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let group = cx.background_spawn(async { space::scan_installers() }).await;
            let _ = this.update(cx, |panel, cx| {
                if let Some(view) = panel.space.as_mut() {
                    view.downloads = Some(true);
                    if let Some(group) = group {
                        view.groups.push(group);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// La fila para revisar Descargas: hasta que se revisa, y solo con el
    /// escaneo terminado y sin haber movido nada.
    fn downloads_row_shown(view: &SpaceView) -> bool {
        view.done && view.phase == Phase::Choosing && view.downloads != Some(true)
    }

    fn empty_trash(&mut self, cx: &mut Context<Self>) {
        if let Err(error) = space::empty_trash() {
            eprintln!("sistema: vaciar la papelera: {error}");
        }
        // El Finder pregunta: se relee la Papelera y el disco hasta que cambie.
        cx.spawn(async move |this, cx| {
            for wait in [3u64, 5, 10, 20] {
                cx.background_executor().timer(Duration::from_secs(wait)).await;
                let trash = cx.background_spawn(async { space::trash_size() }).await;
                let go = this
                    .update(cx, |panel, cx| {
                        panel.set_trash(trash);
                        panel.refresh(cx);
                        cx.notify();
                        panel.space.is_some() && trash > 0
                    })
                    .unwrap_or(false);
                if !go {
                    break;
                }
            }
        })
        .detach();
    }

    pub(super) fn space_height(&self) -> f32 {
        let Some(view) = self.space.as_ref() else {
            return 0.0;
        };
        let mut rows = view.groups.len() as f32 * ROW_H;
        if let Some(group) = view.open.and_then(|k| view.groups.iter().find(|g| g.kind == k)) {
            let shown = group.items.len().min(MAX_ITEMS) + (group.items.len() > MAX_ITEMS) as usize;
            rows += shown as f32 * ITEM_H;
        }
        let nothing = view.done && view.groups.is_empty() && !Self::downloads_row_shown(view);
        if !view.done || nothing || Self::downloads_row_shown(view) {
            rows += ROW_H;
        }
        ROW_H + BAR_BLOCK_H + rows + FOOTER_H
    }

    pub(super) fn render_space(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let now = Instant::now();
        let disk = self.disk();
        let c = &self.colors;
        let (text, muted, faint, ink, fill, track) = (c.text, c.muted, c.faint, c.ink, c.fill, c.track);
        let Some(view) = self.space.as_ref() else {
            return div().into_any_element();
        };
        if view.chosen.is_running(now) || view.trashed.is_running(now) {
            window.request_animation_frame();
        }
        let green: Hsla = rgb(DOWN_TINT).into();
        let amber: Hsla = rgb(TRASH_TINT).into();

        // Arriba: volver, el título y cómo está el disco.
        let back = cx.listener(|panel, _: &ClickEvent, _, cx| panel.close_space(cx));
        let status: SharedString = match (view.done, view.current, disk) {
            (false, Some(kind), _) => crate::i18n::tf("pill.system.space.scanning", &[("what", &kind_label(kind).to_lowercase())]).into(),
            (_, _, Some((free, total))) => crate::i18n::tf("pill.system.diskFree", &[("free", &bytes(free)), ("total", &bytes(total))]).into(),
            _ => SharedString::default(),
        };
        let header = div()
            .h(px(ROW_H))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(hover_fx("space-back-fx", move |h| {
                div()
                    .id("space-back")
                    .size(px(26.))
                    .ml(px(-4.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(13.))
                    .bg(h.mix(gpui::white().opacity(0.0), gpui::white().opacity(0.1)))
                    .cursor_pointer()
                    .on_click(back)
                    .child(
                        svg()
                            .path("icons/chevron-right.svg")
                            .size(px(14.))
                            .text_color(h.mix(muted, text))
                            .with_transformation(gpui::Transformation::rotate(gpui::percentage(0.5))),
                    )
                    .into_any_element()
            }))
            .child(div().text_size(px(13.)).text_color(text).child(crate::i18n::t("pill.system.space.title")))
            .child(div().flex_1())
            .child(div().text_size(px(10.)).text_color(faint).child(status));

        // La barra: usado que se queda | Papelera | a liberar | libre.
        let (free, total) = disk.unwrap_or((0, 1));
        let used = 1.0 - free as f32 / total as f32;
        let chosen = view.chosen.value(now).clamp(0.0, used);
        let trashed = view.trashed.value(now).clamp(0.0, used - chosen);
        let keep = (used - chosen - trashed).max(0.0);
        let segment = |width: f32, color: Hsla| div().h_full().w(gpui::relative(width)).bg(color);
        let bar = div()
            .h(px(10.))
            .w_full()
            .flex()
            .rounded(px(5.))
            .overflow_hidden()
            .bg(track)
            .child(segment(keep, fill.opacity(0.55)))
            .child(segment(trashed, amber))
            .child(segment(chosen, green));
        let legend_dot = |color: Hsla, label: &'static str, amount: u64| {
            div()
                .flex()
                .items_center()
                .gap(px(5.))
                .child(div().size(px(6.)).rounded(px(3.)).bg(color))
                .child(label)
                .child(div().text_color(text).child(bytes(amount)))
        };
        let chosen_bytes = view.chosen_bytes();
        let legend = div()
            .h(px(18.))
            .mt(px(8.))
            .flex()
            .items_center()
            .gap(px(14.))
            .text_size(px(10.))
            .text_color(muted)
            .when(view.phase == Phase::Choosing, |el| el.child(legend_dot(green, crate::i18n::t("pill.system.space.toFree"), chosen_bytes)))
            .when(view.trash >= 1 << 20, |el| el.child(legend_dot(amber, crate::i18n::t("pill.system.space.inTrash"), view.trash)))
            .child(div().flex_1())
            .child(legend_dot(track.opacity(1.0), crate::i18n::t("pill.system.space.free"), free));
        let bar_block = div().h(px(BAR_BLOCK_H)).pt(px(6.)).flex().flex_col().child(bar).child(legend);

        // Los grupos.
        let groups = view.groups.clone();
        let open = view.open;
        let phase = view.phase.clone();
        let mut rows: Vec<AnyElement> = Vec::new();
        for (gi, group) in groups.iter().enumerate() {
            let kind = group.kind;
            let free_items: Vec<_> = group.items.iter().filter(|i| !i.locked()).collect();
            let check = if free_items.is_empty() {
                Check::Locked
            } else if free_items.iter().all(|i| i.checked) {
                Check::On
            } else if free_items.iter().any(|i| i.checked) {
                Check::Some
            } else {
                Check::Off
            };
            let is_open = open == Some(kind);
            let toggle_open = cx.listener(move |panel, _: &ClickEvent, _, cx| {
                if let Some(view) = panel.space.as_mut() {
                    view.open = if view.open == Some(kind) { None } else { Some(kind) };
                }
                cx.notify();
            });
            let toggle_check = cx.listener(move |panel, _: &ClickEvent, _, cx| {
                cx.stop_propagation();
                panel.toggle_group(kind, cx);
            });
            let mut count = match group.items.len() {
                1 => crate::i18n::t("pill.system.space.itemsOne").to_string(),
                n => crate::i18n::tf("pill.system.space.items", &[("n", &n)]),
            };
            if group.in_use() > 0 {
                count = format!("{count} · {}", crate::i18n::tf("pill.system.space.nInUse", &[("n", &group.in_use())]));
            }
            let size = bytes(group.size());
            let label = kind_label(kind);
            let icon = kind_icon(kind);
            let row_id = SharedString::from(format!("space-group-{gi}"));
            let row = hover_fx(SharedString::from(format!("space-group-{gi}-fx")), move |h| {
                div()
                    .id(row_id)
                    .h(px(ROW_H))
                    .mx(px(-6.))
                    .px(px(6.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .rounded(px(10.))
                    .bg(h.mix(gpui::white().opacity(if is_open { 0.04 } else { 0.0 }), gpui::white().opacity(0.06)))
                    .cursor_pointer()
                    .on_click(toggle_open)
                    .child(checkbox(SharedString::from(format!("space-check-{gi}")), check, 16., text, ink, faint, toggle_check))
                    .child(svg().path(icon).size(px(15.)).text_color(muted))
                    .child(div().flex_none().text_size(px(12.)).text_color(text).child(label))
                    .child(div().flex_1().min_w_0().truncate().text_size(px(10.)).text_color(faint).child(count))
                    .child(div().flex_none().text_size(px(12.)).text_color(text).child(size))
                    .child(
                        svg()
                            .path("icons/chevron-right.svg")
                            .size(px(12.))
                            .text_color(faint)
                            .with_transformation(gpui::Transformation::rotate(gpui::percentage(if is_open { 0.25 } else { 0.0 }))),
                    )
                    .into_any_element()
            });
            rows.push(
                crate::code::enter::Enter::new(SharedString::from(format!("space-in-{gi}")))
                    .from(0., -6.)
                    .apply(div(), window, cx)
                    .child(row)
                    .into_any_element(),
            );
            if is_open {
                for (ii, item) in group.items.iter().take(MAX_ITEMS).enumerate() {
                    let check = if item.locked() {
                        Check::Locked
                    } else if item.checked {
                        Check::On
                    } else {
                        Check::Off
                    };
                    let note = match &item.note {
                        Note::None => String::new(),
                        Note::Used(secs) => crate::i18n::tf("pill.system.space.usedAgo", &[("time", &ago_short(*secs))]),
                        Note::InUse(app) => crate::i18n::tf("pill.system.space.inUse", &[("app", app)]),
                        Note::Keep => crate::i18n::t("pill.system.space.keep").to_string(),
                        Note::Redownload => crate::i18n::t("pill.system.space.redownload").to_string(),
                    };
                    let toggle = cx.listener(move |panel, _: &ClickEvent, _, cx| panel.toggle_item(kind, ii, cx));
                    let locked = item.locked();
                    let name = item.name.clone();
                    let size = bytes(item.size);
                    let item_id = SharedString::from(format!("space-item-{gi}-{ii}"));
                    let line = hover_fx(SharedString::from(format!("space-item-{gi}-{ii}-fx")), move |h| {
                        div()
                            .id(item_id.clone())
                            .h(px(ITEM_H))
                            .mx(px(-6.))
                            .pl(px(32.))
                            .pr(px(6.))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .rounded(px(8.))
                            .bg(h.mix(gpui::white().opacity(0.0), gpui::white().opacity(if locked { 0.0 } else { 0.05 })))
                            .when(!locked, |el| el.cursor_pointer().on_click(toggle))
                            .child(checkbox(SharedString::from(format!("{item_id}-check")), check, 14., text, ink, faint, |_, _, _| {}))
                            .child(div().flex_1().min_w_0().truncate().text_size(px(11.)).text_color(if locked { faint } else { text }).child(name))
                            .child(div().flex_none().text_size(px(10.)).text_color(faint).child(note))
                            .child(div().w(px(56.)).flex_none().text_right().text_size(px(11.)).text_color(muted).child(size))
                            .into_any_element()
                    });
                    rows.push(
                        crate::code::enter::Enter::new(SharedString::from(format!("space-item-in-{gi}-{ii}")))
                            .delay(crate::code::enter::stagger(ii, 0.025, MAX_ITEMS))
                            .from(0., -4.)
                            .apply(div(), window, cx)
                            .child(line)
                            .into_any_element(),
                    );
                }
                if group.items.len() > MAX_ITEMS {
                    rows.push(
                        div()
                            .h(px(ITEM_H))
                            .pl(px(26.))
                            .flex()
                            .items_center()
                            .text_size(px(10.))
                            .text_color(faint)
                            .child(crate::i18n::tf("pill.system.space.more", &[("n", &(group.items.len() - MAX_ITEMS))]))
                            .into_any_element(),
                    );
                }
            }
        }
        if !view.done {
            // Mientras mide: un punto que respira, sin números que salten.
            rows.push(
                div()
                    .h(px(ROW_H))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .text_size(px(11.))
                    .text_color(muted)
                    .child(
                        div().w(px(16.)).flex().justify_center().child(
                            div().size(px(7.)).rounded(px(4.)).bg(green).with_animation(
                                "space-breathe",
                                Animation::new(Duration::from_millis(1100)).repeat().with_easing(gpui::ease_in_out),
                                |el, t| el.opacity(0.25 + 0.75 * (1.0 - (2.0 * t - 1.0).abs())),
                            ),
                        ),
                    )
                    .child(view.current.map(kind_label).unwrap_or_default())
                    .into_any_element(),
            );
        }
        if Self::downloads_row_shown(view) {
            let scanning = view.downloads == Some(false);
            let ask = cx.listener(|panel, _: &ClickEvent, _, cx| panel.scan_downloads(cx));
            rows.push(
                hover_fx("space-downloads-fx", move |h| {
                    let fg = if scanning { faint } else { h.mix(muted, text) };
                    div()
                        .id("space-downloads")
                        .h(px(ROW_H))
                        .mx(px(-6.))
                        .px(px(6.))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .rounded(px(10.))
                        .bg(h.mix(gpui::white().opacity(0.0), gpui::white().opacity(if scanning { 0.0 } else { 0.06 })))
                        .when(!scanning, |el| el.cursor_pointer().on_click(ask))
                        .text_size(px(11.))
                        .text_color(fg)
                        .child(div().size(px(16.)).flex_none().flex().items_center().justify_center().child(svg().path("icons/plus.svg").size(px(12.)).text_color(fg)))
                        .child(svg().path("icons/download.svg").size(px(15.)).text_color(fg))
                        .child(crate::i18n::t(if scanning { "pill.system.space.checkingDownloads" } else { "pill.system.space.findInstallers" }))
                        .into_any_element()
                })
                .into_any_element(),
            );
        }
        if view.done && groups.is_empty() && !Self::downloads_row_shown(view) {
            rows.push(
                div()
                    .h(px(ROW_H))
                    .flex()
                    .items_center()
                    .text_size(px(11.))
                    .text_color(muted)
                    .child(crate::i18n::t("pill.system.space.nothing"))
                    .into_any_element(),
            );
        }

        // Abajo: la acción, que dice exactamente qué va a pasar.
        let footer = match phase {
            Phase::Choosing | Phase::Moving => {
                let ready = view.done && chosen_bytes > 0 && phase == Phase::Choosing;
                let label: SharedString = if phase == Phase::Moving {
                    crate::i18n::t("pill.system.space.moving").into()
                } else if !view.done {
                    crate::i18n::t("pill.system.space.pick").into()
                } else {
                    crate::i18n::tf("pill.system.space.move", &[("size", &bytes(chosen_bytes))]).into()
                };
                let go = cx.listener(|panel, _: &ClickEvent, _, cx| panel.move_to_trash(cx));
                div()
                    .h(px(FOOTER_H))
                    .flex()
                    .items_center()
                    .child(hover_fx("space-go-fx", move |h| {
                        let bg = if ready { h.mix(fill, gpui::white()) } else { gpui::white().opacity(0.06) };
                        let inset = 2.0 * h.press * ready as u8 as f32;
                        div()
                            .id("space-go")
                            .w_full()
                            .h(px(36.))
                            .relative()
                            .when(ready, |el| el.cursor_pointer().on_click(go))
                            .child(
                                div()
                                    .absolute()
                                    .inset(px(inset))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .gap(px(8.))
                                    .rounded(px(18. - inset))
                                    .bg(bg)
                                    .text_size(px(12.))
                                    .text_color(if ready { ink } else { faint })
                                    .child(svg().path("icons/trash.svg").size(px(14.)).text_color(if ready { ink } else { faint }))
                                    .child(label),
                            )
                            .into_any_element()
                    }))
                    .into_any_element()
            }
            Phase::Moved(moved, failed) => {
                let mut note = crate::i18n::tf("pill.system.space.moved", &[("size", &bytes(moved))]);
                if failed > 0 {
                    note = format!("{note} {}", crate::i18n::tf("pill.system.space.failedSome", &[("n", &failed)]));
                }
                div()
                    .h(px(FOOTER_H))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().flex_1().min_w_0().text_size(px(11.)).text_color(muted).child(note))
                    .child(proc_action(
                        "space-done".into(),
                        crate::i18n::t("pill.system.space.done"),
                        text,
                        ink,
                        false,
                        cx.listener(|panel, _: &ClickEvent, _, cx| panel.close_space(cx)),
                    ))
                    .when(view.trash >= 1 << 20, |el| {
                        el.child(proc_action(
                            "space-empty".into(),
                            crate::i18n::t("pill.system.space.empty"),
                            amber,
                            ink,
                            true,
                            cx.listener(|panel, _: &ClickEvent, _, cx| panel.empty_trash(cx)),
                        ))
                    })
                    .into_any_element()
            }
        };

        div()
            .px(px(SIDE + 4.))
            .flex()
            .flex_col()
            .child(header)
            .child(bar_block)
            .children(rows)
            .child(footer)
            .into_any_element()
    }
}

/// Una casilla redonda: marcada se llena; a medias, una raya; bloqueada,
/// solo el borde punteado de lo que no se puede tocar.
fn checkbox(
    id: SharedString,
    check: Check,
    size: f32,
    text: Hsla,
    ink: Hsla,
    faint: Hsla,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> AnyElement {
    let filled = matches!(check, Check::On | Check::Some);
    div()
        .id(id)
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(size / 2.0))
        .when(filled, |el| el.bg(text))
        .when(!filled, |el| {
            el.border_1().border_color(if check == Check::Locked { faint.opacity(0.5) } else { faint })
        })
        .when(check != Check::Locked, |el| el.on_click(on_click))
        .when(check == Check::On, |el| el.child(svg().path("icons/check.svg").size(px(size * 0.65)).text_color(ink)))
        .when(check == Check::Some, |el| el.child(div().w(px(size * 0.45)).h(px(2.)).rounded(px(1.)).bg(ink)))
        .into_any_element()
}
