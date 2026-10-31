//! Novedades: lo que trae cada versión de Atic, en una ventana.
//!
//! Sale de `assets/changelog.json`, que va dentro del ejecutable. Cada
//! versión publicada tiene que tener su entrada (lo exige un test y los
//! scripts de release; ver `AGENTS.md`).
//!
//! Se abre sola al arrancar la primera vez después de actualizar, con lo de
//! las versiones que no se habían visto; lo visto queda en
//! `.novedades-vistas` (la carpeta de datos). Una instalación nueva no la ve:
//! tiene la bienvenida. También se abre desde el ícono de la barra de menús
//! o la bandeja («Novedades»). `PILL_OPEN=novedades` la abre igual, para
//! revisarla.

use std::sync::OnceLock;

use gpui::{
    div, prelude::*, px, size, svg, App, Bounds, ClickEvent, Context, FontWeight, SharedString,
    Window, WindowBounds, WindowKind, WindowOptions,
};
use gpui_m3::theme::type_scale::{self, TypeScaleExt};
use serde::Deserialize;

use crate::kit::chrome;
use crate::settings::{hsla, FAINT, MUTED, TEXT};

const TOP_H: f32 = 44.0;
const GUTTER: f32 = 24.0;
const WINDOW_W: f32 = 540.0;
const WINDOW_H: f32 = 660.0;
/// Versiones anteriores a lo nuevo que se muestran de una (el resto, con
/// «Ver todas»).
const OLDER_SHOWN: usize = 4;

#[derive(Clone, Debug, Deserialize)]
pub struct Changelog {
    pub versions: Vec<Release>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Release {
    pub version: String,
    pub date: String,
    pub items: Vec<Entry>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Entry {
    pub kind: Kind,
    pub es: String,
    pub en: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    New,
    Improved,
    Fixed,
}

impl Entry {
    fn text(&self) -> &str {
        if crate::i18n::english() { &self.en } else { &self.es }
    }
}

/// El registro que va dentro del ejecutable.
pub fn changelog() -> &'static Changelog {
    static LOG: OnceLock<Changelog> = OnceLock::new();
    LOG.get_or_init(|| {
        serde_json::from_str(include_str!("../assets/changelog.json")).unwrap_or_else(|error| {
            tracing::warn!(%error, "novedades: changelog.json ilegible");
            Changelog { versions: Vec::new() }
        })
    })
}

fn current() -> &'static str {
    env!("ATIC_VERSION")
}

fn newer(a: &str, b: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> { v.split(['.', '-']).take(3).map(|p| p.parse().unwrap_or(0)).collect() };
    parts(a) > parts(b)
}

/// Las versiones que trae esta actualización: más nuevas que `seen` y no más
/// que la instalada.
fn unseen<'a>(log: &'a Changelog, seen: &str, installed: &str) -> Vec<&'a Release> {
    log.versions.iter().filter(|r| newer(&r.version, seen) && !newer(&r.version, installed)).collect()
}

/// Lo que va arriba, marcado «Nuevo»: lo que no se había visto. Si ya se vio
/// todo (se abrió desde Ajustes), la versión instalada. Nunca una versión
/// más nueva que la instalada (la siguiente ya puede tener su entrada).
fn highlighted<'a>(log: &'a Changelog, seen: Option<&str>, installed: &str) -> Vec<&'a Release> {
    let fresh = match seen {
        Some(seen) => unseen(log, seen, installed),
        None => Vec::new(),
    };
    if !fresh.is_empty() {
        return fresh;
    }
    log.versions.iter().find(|r| !newer(&r.version, installed)).into_iter().collect()
}

fn marker() -> Option<std::path::PathBuf> {
    crate::paths::file(".novedades-vistas")
}

fn seen() -> Option<String> {
    let text = std::fs::read_to_string(marker()?).ok()?;
    Some(text.trim().to_string()).filter(|v| !v.is_empty())
}

fn mark_seen() {
    if let Some(path) = marker() {
        let _ = std::fs::write(path, current());
    }
}

/// Al arrancar: si se actualizó desde la última vez. `welcome`: se está
/// mostrando la bienvenida (instalación nueva), que no se tapa con esto.
pub fn open_if_updated(welcome: bool, cx: &mut App) {
    let forced = std::env::var("PILL_OPEN").as_deref() == Ok("novedades");
    if forced {
        open(cx);
        return;
    }
    if crate::dev::on() {
        return;
    }
    match seen() {
        // Instalación nueva: la bienvenida. Desde aquí cuenta.
        None if welcome => mark_seen(),
        // Venía de una versión sin Novedades: se muestra la de ahora.
        None => open(cx),
        Some(seen) if !unseen(changelog(), &seen, current()).is_empty() => open(cx),
        Some(_) => {}
    }
}

/// Abre Novedades, o la trae al frente.
pub fn open(cx: &mut App) {
    let existing = cx.windows().into_iter().find_map(|w| w.downcast::<WhatsNew>());
    if let Some(handle) = existing {
        if handle.update(cx, |_, window, _| crate::win::bring_to_front(window)).is_ok() {
            return;
        }
    }
    // Lo que no se había visto se marca al abrir: cerrarla de cualquier forma
    // cuenta como visto.
    let since = seen();
    mark_seen();
    let options = WindowOptions {
        titlebar: Some(chrome::titlebar(crate::i18n::t("pill.whatsNew.title"), TOP_H)),
        window_min_size: Some(size(px(420.), px(440.))),
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(WINDOW_W), px(WINDOW_H)), cx))),
        focus: crate::win::FOCUS_ON_OPEN,
        show: true,
        kind: WindowKind::Normal,
        ..Default::default()
    };
    let opened = cx.open_window(options, |window, cx| {
        chrome::setup(window);
        cx.new(|_| WhatsNew { since, all: false, backdrop: None })
    });
    match opened {
        Ok(handle) => {
            let _ = handle.update(cx, |_, window, _| crate::win::bring_to_front(window));
        }
        Err(error) => eprintln!("novedades: no se pudo abrir la ventana: {error}"),
    }
}

pub struct WhatsNew {
    /// La última versión vista antes de abrir: lo posterior va marcado
    /// «Nuevo». `None`: todo es de esta versión.
    since: Option<String>,
    /// Todas las versiones anteriores, no solo las últimas.
    all: bool,
    backdrop: Option<bool>,
}

/// «29 de octubre de 2026» / «October 29, 2026».
fn pretty_date(date: &str) -> String {
    const ES: [&str; 12] = ["enero", "febrero", "marzo", "abril", "mayo", "junio", "julio", "agosto", "septiembre", "octubre", "noviembre", "diciembre"];
    const EN: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    let mut parts = date.split('-').map(|p| p.parse::<usize>().unwrap_or(0));
    let (Some(y), Some(m), Some(d)) = (parts.next(), parts.next(), parts.next()) else {
        return date.to_string();
    };
    if !(1..=12).contains(&m) {
        return date.to_string();
    }
    if crate::i18n::english() {
        format!("{} {d}, {y}", EN[m - 1])
    } else {
        format!("{d} de {} de {y}", ES[m - 1])
    }
}

fn kind_look(kind: Kind) -> (&'static str, &'static str) {
    match kind {
        Kind::New => ("icons/sparkles.svg", "pill.whatsNew.new"),
        Kind::Improved => ("icons/arrow-up-right.svg", "pill.whatsNew.improved"),
        Kind::Fixed => ("icons/check.svg", "pill.whatsNew.fixed"),
    }
}

impl WhatsNew {
    /// Una versión: su número y fecha, y lo que trae agrupado por tipo.
    fn release(&self, release: &Release, fresh: bool, index: usize, window: &mut Window, cx: &mut App) -> gpui::AnyElement {
        let expressive = crate::theme::expressive().is_some();
        let accent = hsla(crate::theme::AMBER);
        let head = div()
            .flex()
            .items_baseline()
            .gap(px(10.))
            .child(
                div()
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .when(expressive, |el| el.type_style(type_scale::TITLE_MEDIUM))
                    .child(format!("Atic {}", release.version)),
            )
            .child(div().text_size(px(12.)).text_color(hsla(MUTED)).child(pretty_date(&release.date)))
            .child(div().flex_1())
            .when(fresh, |el| {
                el.child(
                    div()
                        .px(px(8.))
                        .py(px(2.))
                        .rounded_full()
                        .bg(accent.opacity(0.16))
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(accent)
                        .child(crate::i18n::t("pill.whatsNew.badge")),
                )
            });
        let mut groups = Vec::new();
        for kind in [Kind::New, Kind::Improved, Kind::Fixed] {
            let entries: Vec<&Entry> = release.items.iter().filter(|e| e.kind == kind).collect();
            if entries.is_empty() {
                continue;
            }
            let (icon, label) = kind_look(kind);
            let rows = entries.into_iter().map(|entry| {
                div()
                    .flex()
                    .items_start()
                    .gap(px(10.))
                    .child(
                        div()
                            .size(px(18.))
                            .mt(px(1.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(svg().path(icon).size(px(14.)).text_color(if fresh && kind == Kind::New { accent } else { hsla(MUTED) })),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(13.))
                            .line_height(px(20.))
                            .text_color(hsla(TEXT))
                            .when(expressive, |el| el.type_style(type_scale::BODY_MEDIUM))
                            .child(entry.text().to_string()),
                    )
            });
            groups.push(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(div().text_size(px(11.)).text_color(hsla(FAINT)).child(crate::i18n::t(label)))
                    .children(rows),
            );
        }
        let card = crate::settings::card().gap(px(14.)).child(head).children(groups);
        crate::code::enter::Enter::new(SharedString::from(format!("whats-new-{}", release.version)))
            .delay(crate::code::enter::stagger(index, 0.05, 5))
            .from(0., 10.)
            .apply(div(), window, cx)
            .child(card)
            .into_any_element()
    }
}

impl Render for WhatsNew {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::theme::sync_backdrop(window, &mut self.backdrop);
        let m3_scope = crate::theme::scope_m3(cx);
        let expressive = crate::theme::expressive().is_some();
        let log = changelog();
        let installed = current();
        let since = self.since.clone();
        // Lo de esta actualización arriba; lo anterior, debajo y sin marca.
        let fresh = highlighted(log, since.as_deref(), installed);
        let older: Vec<&Release> = log
            .versions
            .iter()
            .filter(|r| !fresh.iter().any(|f| f.version == r.version) && !newer(&r.version, installed))
            .collect();
        let older_total = older.len();
        let older: Vec<&Release> = if self.all { older } else { older.into_iter().take(OLDER_SHOWN).collect() };

        let mut cards = Vec::new();
        for (i, release) in fresh.iter().enumerate() {
            cards.push(self.release(release, true, i, window, cx));
        }
        let before = (!older.is_empty()).then(|| {
            div()
                .mt(px(8.))
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(hsla(MUTED))
                .child(crate::i18n::t("pill.whatsNew.before"))
        });
        let mut older_cards = Vec::new();
        for (i, release) in older.iter().enumerate() {
            older_cards.push(self.release(release, false, fresh.len() + i, window, cx));
        }
        let more = (!self.all && older_total > OLDER_SHOWN).then(|| {
            crate::settings::text_button(
                "whats-new-all",
                crate::i18n::t("pill.whatsNew.showAll"),
                cx.listener(|view, _: &ClickEvent, _, cx| {
                    view.all = true;
                    cx.notify();
                }),
            )
        });

        let hero = div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .mb(px(6.))
            .child(
                div()
                    .text_size(px(24.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .when(expressive, |el| el.type_style(type_scale::HEADLINE_MEDIUM))
                    .child(crate::i18n::t("pill.whatsNew.heading")),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(hsla(MUTED))
                    .when(expressive, |el| el.type_style(type_scale::BODY_MEDIUM))
                    .child(crate::i18n::tf("pill.whatsNew.installed", &[("version", &installed)])),
            );

        let content = div()
            .id("whats-new-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(GUTTER))
            .pt(px(6.))
            .pb(px(GUTTER))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .child(hero)
                    .children(cards)
                    .children(before)
                    .children(older_cards)
                    .children(more.map(|button| div().flex().justify_center().child(button))),
            );

        let close = cx.listener(|_, _: &ClickEvent, window, _| window.remove_window());
        let footer = div()
            .flex_none()
            .px(px(GUTTER))
            .py(px(14.))
            .flex()
            .justify_end()
            .border_t_1()
            .border_color(hsla(TEXT).opacity(0.06))
            .child(if crate::theme::styled().is_some() {
                gpui_m3::Button::new("whats-new-done", crate::i18n::t("pill.whatsNew.done")).filled().on_click(close).into_any_element()
            } else {
                div()
                    .id("whats-new-done")
                    .px(px(18.))
                    .py(px(8.))
                    .rounded_full()
                    .bg(hsla(crate::theme::FILL))
                    .text_color(hsla(crate::theme::INK))
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .cursor_pointer()
                    .on_click(close)
                    .child(crate::i18n::t("pill.whatsNew.done"))
                    .into_any_element()
            });

        let top = div()
            .h(px(TOP_H))
            .flex_none()
            .pl(px(GUTTER - 8.0))
            .flex()
            .items_center()
            .gap(px(10.))
            .child(chrome::lights())
            .child(chrome::logo(22.0))
            .child(
                div()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .when(expressive, |el| el.type_style(type_scale::TITLE_MEDIUM))
                    .child(crate::i18n::t("pill.whatsNew.title")),
            )
            .child(chrome::drag(TOP_H))
            .child(chrome::controls(window.is_maximized(), TOP_H));

        let pieces = crate::glass_pieces::active_windows();
        div()
            .size_full()
            .flex()
            .flex_col()
            .when(!pieces, |el| el.bg(crate::theme::window_bg()))
            .when(pieces, |el| {
                el.child(crate::glass_pieces::slot(crate::glass_pieces::Piece::Fill { radius: 0., color: crate::theme::window_bg() }))
            })
            .font_family(crate::theme::font())
            .text_color(hsla(TEXT))
            .child(top)
            .child(content)
            .child(footer)
            .child(crate::glass_pieces::commit(crate::theme::is_light()))
            .children(m3_scope)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_registro_se_lee_y_esta_ordenado() {
        let log: Changelog = serde_json::from_str(include_str!("../assets/changelog.json")).expect("changelog.json válido");
        assert!(!log.versions.is_empty());
        for pair in log.versions.windows(2) {
            assert!(newer(&pair[0].version, &pair[1].version), "{} debe ir antes que {}", pair[0].version, pair[1].version);
        }
        for release in &log.versions {
            assert!(!release.items.is_empty(), "{} sin novedades", release.version);
            assert_eq!(release.date.len(), 10, "{}: fecha AAAA-MM-DD", release.version);
            for entry in &release.items {
                assert!(!entry.es.trim().is_empty() && !entry.en.trim().is_empty(), "{}: falta un idioma", release.version);
            }
        }
    }

    /// Cada versión que se publica trae su entrada: si se sube la versión
    /// en `Cargo.toml` sin escribir sus novedades, esto falla.
    #[test]
    fn la_version_actual_tiene_sus_novedades() {
        let log: Changelog = serde_json::from_str(include_str!("../assets/changelog.json")).unwrap();
        assert!(
            log.versions.iter().any(|r| r.version == current()),
            "falta la entrada de {} en assets/changelog.json",
            current()
        );
    }

    #[test]
    fn lo_nuevo_es_lo_posterior_a_lo_visto() {
        let entry = || Entry { kind: Kind::New, es: "a".into(), en: "a".into() };
        let release = |v: &str| Release { version: v.into(), date: "2026-10-31".into(), items: vec![entry()] };
        let log = Changelog { versions: vec![release("0.6.0"), release("0.5.5"), release("0.5.4"), release("0.5.3")] };
        let versions = |seen: &str, installed: &str| -> Vec<String> {
            unseen(&log, seen, installed).iter().map(|r| r.version.clone()).collect()
        };
        assert_eq!(versions("0.5.3", "0.5.5"), ["0.5.5", "0.5.4"]);
        // Lo que todavía no se instala no se muestra.
        assert_eq!(versions("0.5.4", "0.5.5"), ["0.5.5"]);
        assert!(versions("0.5.5", "0.5.5").is_empty());
    }

    #[test]
    fn sin_nada_nuevo_se_ve_la_instalada_y_no_la_que_viene() {
        let entry = || Entry { kind: Kind::New, es: "a".into(), en: "a".into() };
        let release = |v: &str| Release { version: v.into(), date: "2026-10-31".into(), items: vec![entry()] };
        let log = Changelog { versions: vec![release("0.5.5"), release("0.5.4"), release("0.5.3")] };
        let versions = |seen: Option<&str>| -> Vec<String> {
            highlighted(&log, seen, "0.5.4").iter().map(|r| r.version.clone()).collect()
        };
        // Ya vista (abierta desde Ajustes) o sin marca: la instalada.
        assert_eq!(versions(Some("0.5.4")), ["0.5.4"]);
        assert_eq!(versions(None), ["0.5.4"]);
        // Venía de 0.5.3: lo nuevo es 0.5.4, no la 0.5.5 que aún no sale.
        assert_eq!(versions(Some("0.5.3")), ["0.5.4"]);
    }

    #[test]
    fn fechas_legibles() {
        assert_eq!(pretty_date("2026-10-29"), "29 de octubre de 2026");
        assert_eq!(pretty_date("mal"), "mal");
    }
}
