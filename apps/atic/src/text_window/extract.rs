//! El texto de una captura con su traducción al lado: lo comparten la ventana
//! «Texto» del estante (`text_window.rs`) y la pestaña Texto del editor de
//! capturas (`photo_editor/`).
//!
//! Lee la imagen con el OCR del sistema y deja el texto, a lo ancho, en un
//! área que se puede corregir. «Traducir» abre a la derecha la traducción:
//! arriba de cada lado su idioma («Inglés (detectado)» → «Español»), que se
//! puede elegir, entre los dos el botón para intercambiarlos y junto al
//! destino la ✕ para volver a una columna. Al corregir el original, la
//! traducción se rehace sola cuando se deja de escribir.
//!
//! Se traduce con IA si la hay (el proveedor de Ajustes o Apple Intelligence)
//! o con la rápida del Mac; con las dos, abajo se elige «Con IA / Rápida»
//! (`translate.rs`). En Windows sin proveedor de IA, «Traducir» queda apagado
//! y su ayuda dice cómo encenderlo.

use std::time::{Duration, Instant};

use atic_capture::Frame;
use gpui::{
    anchored, deferred, div, prelude::*, px, relative, AnyElement, App, ClickEvent, ClipboardItem, Context, Corner, Entity,
    EventEmitter, Focusable, MouseButton, SharedString, Subscription, Window,
};
use gpui_m3::theme::type_scale::{self, TypeScaleExt};
use gpui_m3::ButtonSize;

use super::translate::{self, AiSource, Engine, Engines, Failure};
use crate::i18n::{t, tf};
use crate::text_area::TextArea;
use crate::text_input::Changed;
use crate::theme::{hsla, MUTED, TEXT};

/// Los idiomas que se pueden elegir (los que traduce el Mac).
const LANGUAGES: [&str; 19] = ["ar", "de", "en", "es", "fr", "hi", "id", "it", "ja", "ko", "nl", "pl", "pt", "ru", "th", "tr", "uk", "vi", "zh"];
/// Cuánto se espera después de la última tecla para volver a traducir.
const RETRANSLATE_AFTER: Duration = Duration::from_millis(700);
/// Lo que dura «Copiado» en el botón.
const COPIED_FOR: Duration = Duration::from_millis(1500);
/// El alto de la fila de los idiomas.
const HEAD_H: f32 = 48.0;
const SWAP: f32 = 32.0;

/// «zh-TW» → «zh».
fn base(code: &str) -> &str {
    code.split(['-', '_']).next().unwrap_or(code)
}

/// El nombre de un idioma de la lista, en el de Atic.
fn language_name(code: &str) -> Option<&'static str> {
    Some(match base(code) {
        "ar" => t("pill.textWindow.lang.ar"),
        "de" => t("pill.textWindow.lang.de"),
        "en" => t("pill.textWindow.lang.en"),
        "es" => t("pill.textWindow.lang.es"),
        "fr" => t("pill.textWindow.lang.fr"),
        "hi" => t("pill.textWindow.lang.hi"),
        "id" => t("pill.textWindow.lang.id"),
        "it" => t("pill.textWindow.lang.it"),
        "ja" => t("pill.textWindow.lang.ja"),
        "ko" => t("pill.textWindow.lang.ko"),
        "nl" => t("pill.textWindow.lang.nl"),
        "pl" => t("pill.textWindow.lang.pl"),
        "pt" => t("pill.textWindow.lang.pt"),
        "ru" => t("pill.textWindow.lang.ru"),
        "th" => t("pill.textWindow.lang.th"),
        "tr" => t("pill.textWindow.lang.tr"),
        "uk" => t("pill.textWindow.lang.uk"),
        "vi" => t("pill.textWindow.lang.vi"),
        "zh" => t("pill.textWindow.lang.zh"),
        _ => return None,
    })
}

/// El nombre para mostrar: el de la lista o, si no está, el que dio el Mac
/// (con mayúscula, como los de la lista).
fn display_name(code: &str, fallback: &str) -> String {
    match language_name(code) {
        Some(name) => name.to_string(),
        None => {
            let name = if fallback.is_empty() { code } else { fallback };
            let mut chars = name.chars();
            chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
        }
    }
}

/// El código que se le pide al Mac: el chino, simplificado.
fn request_code(code: &str) -> &str {
    if code == "zh" { "zh-Hans" } else { code }
}

/// El idioma de Atic, destino de la traducción mientras no se elija otro.
fn ui_language() -> &'static str {
    if crate::i18n::english() { "en" } else { "es" }
}

/// El otro idioma de Atic: el destino si el texto ya está en el primero.
fn other_language(code: &str) -> &'static str {
    if base(code) == "es" { "en" } else { "es" }
}

/// Si este equipo podría traducir (el Mac, o un proveedor de IA en Ajustes):
/// para el tamaño de la ventana al abrirla. Lo que hay de verdad lo sabe cada
/// vista al rato ([`translate::resolve`]).
pub fn translates() -> bool {
    translate::maybe_available()
}

/// El nombre de la IA que traduce, para las ayudas y los errores.
fn ai_name(ai: &AiSource) -> String {
    match ai {
        AiSource::Provider { name } => name.clone(),
        AiSource::Apple => "Apple Intelligence".into(),
    }
}

/// Ajustes del Sistema donde se bajan los idiomas para traducir en el Mac.
fn open_translation_settings() {
    #[cfg(target_os = "macos")]
    crate::platform::apple::open_translation_settings();
}

/// Cómo va la lectura de la imagen.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reading {
    Busy,
    Ready,
    Empty,
    Failed,
}

/// Cada lado: el original (izquierda) y la traducción (derecha).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Side {
    Source,
    Target,
}

/// Lo que se ofrece junto a un problema de la traducción.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fix {
    Retry,
    /// Falta bajar los idiomas del Mac: «Abrir Ajustes» del sistema.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    AppleSettings,
    /// La llave o el modelo del proveedor: «Abrir Ajustes» de Atic.
    AiSettings,
    /// El texto ya está en el idioma de destino: elegir otro.
    PickLanguage,
}

/// Por qué no se tradujo, en el lugar de la traducción.
struct Problem {
    message: String,
    fix: Fix,
}

/// Se abrió o se cerró la traducción: quien contiene la vista puede cambiar de ancho.
pub struct TranslationToggled;

pub struct TextExtract {
    reading: Reading,
    /// La segunda columna, con la traducción: solo si se pidió con «Traducir».
    show_translation: bool,
    original: Entity<TextArea>,
    translation: Entity<TextArea>,
    /// Con qué se puede traducir; `None` mientras se averigua (llavero, red).
    engines: Option<Engines>,
    /// La forma elegida con «Con IA / Rápida» (recordada entre ventanas).
    chosen: Option<Engine>,
    /// Con qué va o fue el último pedido: «Traduciendo con IA…».
    active: Option<Engine>,
    /// El idioma del original que eligió el usuario; `None`, el que se detecte.
    from: Option<String>,
    /// El de la traducción.
    to: String,
    /// Lo eligió el usuario: el Mac no lo cambia aunque el texto ya esté en ese.
    to_chosen: bool,
    /// Lo que se detectó en la última traducción: código y nombre.
    detected: Option<(String, String)>,
    /// Lo detectó la IA (en Windows no hay otro detector).
    detected_by_ai: bool,
    /// El original del que salió la traducción que se ve.
    translated_from: Option<String>,
    translating: bool,
    problem: Option<Problem>,
    /// Sube con cada pedido: una respuesta vieja no pisa a una nueva.
    request: u64,
    /// Sube con cada cambio del original, para esperar a que se deje de escribir.
    edits: u64,
    /// El menú de idiomas abierto, y el último que se cerró con un clic afuera
    /// (ese mismo clic, si cae en su botón, no lo vuelve a abrir).
    menu: Option<Side>,
    menu_closed: Option<(Side, Instant)>,
    _changed: Subscription,
}

/// Texto, texto apagado (el marcador) y acento de las áreas, según el tema.
fn colors() -> (gpui::Hsla, gpui::Hsla, gpui::Hsla) {
    crate::theme::expressive().map_or_else(|| (hsla(TEXT), hsla(MUTED), hsla(TEXT)), |t| (t.text, t.muted, t.accent))
}

impl EventEmitter<TranslationToggled> for TextExtract {}

impl TextExtract {
    /// Lee el texto de lo que dé `load` (en otro hilo). La traducción espera a «Traducir».
    pub fn new(load: impl FnOnce() -> Result<Frame, String> + Send + 'static, cx: &mut Context<Self>) -> Self {
        let (text, muted, accent) = colors();
        let original = cx.new(|cx| TextArea::new("", text, muted, accent, cx));
        let translation = cx.new(|cx| TextArea::new("", text, muted, accent, cx));
        let changed = cx.subscribe(&original, |view, _, _: &Changed, cx| view.original_edited(cx));
        cx.spawn(async move |this, cx| {
            let text = cx
                .background_spawn(async move {
                    let frame = load()?;
                    crate::ocr::recognize(frame.width(), frame.height(), &frame.bgra)
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                view.reading = match text {
                    Ok(text) if !text.trim().is_empty() => {
                        view.original.update(cx, |area, cx| area.set_text(text.trim(), cx));
                        Reading::Ready
                    }
                    Ok(_) => Reading::Empty,
                    Err(error) => {
                        tracing::warn!(%error, "texto: no se pudo leer la imagen");
                        Reading::Failed
                    }
                };
                cx.notify();
            });
        })
        .detach();
        // Qué hay para traducir: lee el llavero y pregunta a Ollama o al Mac.
        cx.spawn(async move |this, cx| {
            let engines = cx.background_spawn(async move { translate::resolve() }).await;
            let _ = this.update(cx, |view, cx| {
                view.engines = Some(engines);
                // «Traducir» se tocó antes de saberlo: ahora sí.
                if view.show_translation {
                    view.translate(cx);
                }
                cx.notify();
            });
        })
        .detach();
        Self {
            reading: Reading::Busy,
            show_translation: false,
            original,
            translation,
            engines: None,
            chosen: translate::remembered(),
            active: None,
            from: None,
            to: ui_language().to_string(),
            to_chosen: false,
            detected: None,
            detected_by_ai: false,
            translated_from: None,
            translating: false,
            problem: None,
            request: 0,
            edits: 0,
            menu: None,
            menu_closed: None,
            _changed: changed,
        }
    }

    /// Si se está escribiendo en una de las áreas: quien la contiene deja las
    /// letras para ella (la Pizarra no las toma como atajos).
    pub fn has_focus(&self, window: &Window, cx: &App) -> bool {
        self.original.read(cx).focus_handle(cx).is_focused(window) || self.translation.read(cx).focus_handle(cx).is_focused(window)
    }

    /// Si se ve la columna de la traducción.
    pub fn shows_translation(&self) -> bool {
        self.show_translation
    }

    /// Con qué se traduce ahora: la elegida si está, si no la que haya.
    fn engine(&self) -> Option<Engine> {
        self.engines.as_ref()?.pick(self.chosen)
    }

    /// Ya se sabe que en este equipo no hay con qué traducir.
    fn cannot_translate(&self) -> bool {
        self.engines.as_ref().is_some_and(|engines| !engines.any())
    }

    /// Elige «Con IA» o «Rápida», la recuerda y traduce de nuevo.
    fn choose(&mut self, engine: Engine, cx: &mut Context<Self>) {
        if self.engine() == Some(engine) {
            return;
        }
        self.chosen = Some(engine);
        translate::remember(engine);
        if self.show_translation {
            self.translate(cx);
        }
        cx.notify();
    }

    /// Abre o cierra la columna de la traducción. Al abrirla traduce, salvo
    /// que la que ya había siga al día; al cerrarla se olvida lo pendiente.
    fn set_translation(&mut self, show: bool, cx: &mut Context<Self>) {
        if self.show_translation == show || (show && self.cannot_translate()) {
            return;
        }
        self.show_translation = show;
        self.menu = None;
        if show {
            if self.stale(cx) || self.problem.is_some() {
                self.translate(cx);
            }
        } else {
            self.request += 1;
            self.translating = false;
        }
        cx.emit(TranslationToggled);
        cx.notify();
    }

    /// La traducción que se ve no es la del original de ahora.
    fn stale(&self, cx: &App) -> bool {
        self.translated_from.as_deref() != Some(self.original.read(cx).text())
    }

    fn original_edited(&mut self, cx: &mut Context<Self>) {
        self.edits += 1;
        if self.cannot_translate() || !self.show_translation {
            return;
        }
        let edits = self.edits;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(RETRANSLATE_AFTER).await;
            let _ = this.update(cx, |view, cx| {
                if view.edits == edits && view.show_translation && view.stale(cx) {
                    view.translate(cx);
                }
            });
        })
        .detach();
    }

    fn translate(&mut self, cx: &mut Context<Self>) {
        let source = self.original.read(cx).text().to_string();
        self.request += 1;
        self.problem = None;
        let Some(engines) = self.engines.clone() else {
            // Todavía no se sabe con qué: se traduce apenas se sepa.
            self.translating = true;
            cx.notify();
            return;
        };
        let Some(engine) = engines.pick(self.chosen) else {
            self.translating = false;
            self.problem = Some(Problem { message: t("pill.textWindow.needsAi").to_string(), fix: Fix::AiSettings });
            cx.notify();
            return;
        };
        if source.trim().is_empty() {
            self.translating = false;
            self.translation.update(cx, |area, cx| area.set_text("", cx));
            self.translated_from = Some(source);
            cx.notify();
            return;
        }
        let request = self.request;
        self.translating = true;
        self.active = Some(engine);
        cx.notify();
        let from = self.from.clone();
        let to = self.to.clone();
        let or = (!self.to_chosen).then(|| other_language(&to));
        let ai = engines.ai.clone();
        cx.spawn(async move |this, cx| {
            let text = source.clone();
            let result = cx
                .background_spawn(async move {
                    translate::translate(engine, ai.as_ref(), &text, from.as_deref().map(request_code), request_code(&to), or)
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                if view.request != request {
                    return;
                }
                view.translating = false;
                match result {
                    Ok(done) => {
                        if view.from.is_none() && !done.from_code.is_empty() {
                            view.detected = Some((base(&done.from_code).to_string(), done.from_name.clone()));
                            view.detected_by_ai = done.detected_by_ai;
                        }
                        // Ya estaba en el idioma elegido: vuelve tal cual. Mejor decirlo.
                        if !done.from_code.is_empty() && base(&done.from_code) == base(&done.to_code) {
                            // «ya está en español», «already in Spanish».
                            let name = display_name(&done.to_code, &done.to_name);
                            let lang = if crate::i18n::english() { name } else { name.to_lowercase() };
                            view.translated_from = None;
                            view.translation.update(cx, |area, cx| area.set_text("", cx));
                            view.problem = Some(Problem {
                                message: tf("pill.textWindow.sameLanguage", &[("lang", &lang)]),
                                fix: Fix::PickLanguage,
                            });
                            cx.notify();
                            return;
                        }
                        view.translation.update(cx, |area, cx| area.set_text(done.text.trim_end(), cx));
                        view.translated_from = Some(source);
                        // El texto ya estaba en el idioma de Atic y se tradujo al otro.
                        if !view.to_chosen && !done.to_code.is_empty() {
                            view.to = base(&done.to_code).to_string();
                        }
                    }
                    Err(failure) => {
                        view.translated_from = None;
                        view.translation.update(cx, |area, cx| area.set_text("", cx));
                        view.problem = Some(view.problem_for(failure));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Lo que se dice (y se ofrece) cuando no se pudo traducir.
    fn problem_for(&mut self, failure: Failure) -> Problem {
        let (message, fix) = match failure {
            Failure::NotInstalled { from, to } => (tf("pill.textWindow.notInstalled", &[("from", &from), ("to", &to)]), Fix::AppleSettings),
            Failure::Unsupported { from, to } => (tf("pill.textWindow.unsupported", &[("from", &from), ("to", &to)]), Fix::PickLanguage),
            Failure::UnknownLanguage => {
                self.detected = None;
                (t("pill.textWindow.unknownLanguage").to_string(), Fix::Retry)
            }
            Failure::TimedOut => {
                tracing::warn!("texto: la traducción no terminó a tiempo");
                (t("pill.textWindow.translateTimeout").to_string(), Fix::Retry)
            }
            Failure::Offline { provider } => (tf("pill.textWindow.aiOffline", &[("provider", &provider)]), Fix::Retry),
            Failure::BadKey { provider } => (tf("pill.textWindow.aiKey", &[("provider", &provider)]), Fix::AiSettings),
            Failure::Limit { provider } => (tf("pill.textWindow.aiLimit", &[("provider", &provider)]), Fix::Retry),
            Failure::BadModel { provider } => (tf("pill.textWindow.aiModel", &[("provider", &provider)]), Fix::AiSettings),
            Failure::Failed(error) => {
                tracing::warn!(%error, "texto: no se pudo traducir");
                (t("pill.textWindow.translateFailed").to_string(), Fix::Retry)
            }
        };
        Problem { message, fix }
    }

    /// Elige el idioma de un lado (`None` en el original: detectarlo) y traduce de nuevo.
    fn pick(&mut self, side: Side, code: Option<&'static str>, cx: &mut Context<Self>) {
        self.menu = None;
        match side {
            Side::Source => {
                let code = code.map(str::to_string);
                if self.from == code {
                    cx.notify();
                    return;
                }
                self.from = code;
            }
            Side::Target => {
                let Some(code) = code else { return };
                self.to_chosen = true;
                if self.to == code && self.problem.is_none() && !self.stale(cx) {
                    cx.notify();
                    return;
                }
                self.to = code.to_string();
            }
        }
        self.translate(cx);
    }

    /// El idioma del original: el elegido o el detectado.
    fn source_language(&self) -> Option<String> {
        self.from.clone().or_else(|| self.detected.as_ref().map(|(code, _)| code.clone()))
    }

    /// Intercambia los idiomas. Con una traducción al día, también los textos
    /// (al instante: la traducción de vuelta es el original); si no, traduce.
    fn swap(&mut self, cx: &mut Context<Self>) {
        let Some(source) = self.source_language() else {
            return;
        };
        let original = self.original.read(cx).text().to_string();
        let translated = self.translation.read(cx).text().to_string();
        let fresh = !self.translating && self.problem.is_none() && !self.stale(cx) && !translated.trim().is_empty();
        self.from = Some(std::mem::replace(&mut self.to, source));
        self.to_chosen = true;
        self.menu = None;
        if fresh {
            // Cancela lo pendiente: un pedido en camino o la espera tras teclear.
            self.request += 1;
            self.edits += 1;
            self.original.update(cx, |area, cx| area.set_text(&translated, cx));
            self.translation.update(cx, |area, cx| area.set_text(&original, cx));
            self.translated_from = Some(translated);
            cx.notify();
        } else {
            self.translate(cx);
        }
    }

    fn copy(&self, side: Side, cx: &mut Context<Self>) {
        let area = if side == Side::Source { &self.original } else { &self.translation };
        let text = area.read(cx).text().to_string();
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    fn open_menu(&mut self, side: Side, cx: &mut Context<Self>) {
        // El clic afuera que lo cerró era en este mismo botón: queda cerrado.
        if self.menu_closed.is_some_and(|(closed, at)| closed == side && at.elapsed() < Duration::from_millis(250)) {
            self.menu_closed = None;
            return;
        }
        self.menu = if self.menu == Some(side) { None } else { Some(side) };
        cx.notify();
    }

    // --- La vista ------------------------------------------------------------------

    /// Lo que va en lugar del texto mientras se lee, si no hay o si falló.
    fn status(&self, s: &gpui_m3::Scheme) -> Option<AnyElement> {
        let (icon, message) = match self.reading {
            Reading::Ready => return None,
            Reading::Busy => (None, t("pill.textWindow.reading")),
            Reading::Empty => (Some("icons/scan-text.svg"), t("pill.textWindow.empty")),
            Reading::Failed => (Some("icons/circle-x.svg"), t("pill.textWindow.readFailed")),
        };
        Some(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(14.))
                .px(px(24.))
                .when(self.reading == Reading::Busy, |el| el.child(gpui_m3::LoadingIndicator::new().size(px(36.))))
                .children(icon.map(|icon| gpui::svg().path(icon).size(px(28.)).text_color(s.on_surface_variant)))
                .child(div().text_center().text_color(s.on_surface_variant).type_style(type_scale::BODY_MEDIUM).child(message))
                .into_any_element(),
        )
    }

    /// El botón con el idioma de un lado y, abierto, su menú debajo.
    fn picker(&self, side: Side, s: &gpui_m3::Scheme, cx: &mut Context<Self>) -> AnyElement {
        let label: SharedString = match side {
            Side::Source => match (&self.from, &self.detected) {
                (Some(code), _) => display_name(code, code).into(),
                (None, Some((code, name))) => {
                    let key = if self.detected_by_ai { "pill.textWindow.detectedAi" } else { "pill.textWindow.detected" };
                    tf(key, &[("lang", &display_name(code, name))]).into()
                }
                (None, None) => t("pill.textWindow.detect").into(),
            },
            Side::Target => display_name(&self.to, &self.to).into(),
        };
        let open = self.menu == Some(side);
        let id = if side == Side::Source { "text-lang-from" } else { "text-lang-to" };
        let chip = gpui_m3::Chip::new(id, label)
            .open(open)
            .max_w(px(220.))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.open_menu(side, cx)));
        let menu = open.then(|| {
            deferred(
                anchored().anchor(Corner::TopLeft).snap_to_window_with_margin(px(8.)).child(
                    div()
                        .id(("text-lang-menu", side as usize))
                        .mt(px(4.))
                        .occlude()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_mouse_down_out(cx.listener(move |view, _, _, cx| {
                            if view.menu == Some(side) {
                                view.menu = None;
                                view.menu_closed = Some((side, Instant::now()));
                                cx.notify();
                            }
                        }))
                        .child(self.menu(side, s, cx)),
                ),
            )
            .with_priority(2)
        });
        div().flex().flex_col().min_w_0().child(chip).children(menu).into_any_element()
    }

    fn menu(&self, side: Side, _: &gpui_m3::Scheme, cx: &mut Context<Self>) -> gpui_m3::Menu {
        let mut menu = gpui_m3::Menu::new(("text-lang-list", side as usize)).width(220.).max_h(px(320.)).origin(Corner::TopLeft).stagger(false);
        let current = match side {
            Side::Source => self.from.as_deref(),
            Side::Target => Some(self.to.as_str()),
        };
        if side == Side::Source {
            menu = menu
                .item(
                    gpui_m3::MenuItem::new("text-lang-detect", t("pill.textWindow.detect"))
                        .selected(current.is_none())
                        .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.pick(Side::Source, None, cx))),
                )
                .separator();
        }
        // En orden alfabético según el idioma de Atic.
        let mut languages: Vec<(&'static str, &'static str)> =
            LANGUAGES.iter().filter_map(|&code| language_name(code).map(|name| (code, name))).collect();
        languages.sort_by_key(|(_, name)| name.to_lowercase());
        for (i, (code, name)) in languages.into_iter().enumerate() {
            let on = current.is_some_and(|current| base(current) == code);
            menu = menu.item(
                gpui_m3::MenuItem::new(("text-lang-item", side as usize * 100 + i), name)
                    .selected(on)
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.pick(side, Some(code), cx))),
            );
        }
        menu
    }

    fn copy_button(&self, side: Side, enabled: bool, cx: &mut Context<Self>) -> AnyElement {
        let (id, tip) = match side {
            Side::Source => ("text-copy-original", "pill.textWindow.copyOriginalTip"),
            Side::Target => ("text-copy-translation", "pill.textWindow.copyTranslationTip"),
        };
        gpui_m3::Button::new(id, t("pill.textWindow.copy"))
            .text()
            .size(ButtonSize::Compact)
            .icon("icons/copy.svg")
            .tooltip(t(tip))
            .disabled(!enabled)
            .confirm(t("pill.textWindow.copied"), COPIED_FOR)
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.copy(side, cx)))
            .into_any_element()
    }

    /// El área de un lado, alineada con el texto del botón de su idioma.
    fn area(area: &Entity<TextArea>) -> gpui::Div {
        div().flex_1().min_h_0().px(px(16.)).pt(px(2.)).type_style(type_scale::BODY_MEDIUM).child(area.clone())
    }

    /// Lo que va a la derecha: la traducción, o lo que pasa con ella.
    fn target_body(&self, s: &gpui_m3::Scheme, cx: &mut Context<Self>) -> gpui::Div {
        if let Some(problem) = self.problem.as_ref() {
            let action = match problem.fix {
                Fix::AppleSettings => gpui_m3::Button::new("text-settings", t("pill.textWindow.openSettings"))
                    .tonal()
                    .size(ButtonSize::Compact)
                    .icon("icons/settings-2.svg")
                    .on_click(|_, _, _| open_translation_settings()),
                Fix::AiSettings => gpui_m3::Button::new("text-ai-settings", t("pill.textWindow.openSettings"))
                    .tonal()
                    .size(ButtonSize::Compact)
                    .icon("icons/settings-2.svg")
                    .on_click(|_, _, cx| cx.defer(|cx| crate::settings::open_at(crate::settings::Section::Meetings, cx))),
                Fix::PickLanguage => gpui_m3::Button::new("text-pick-language", t("pill.textWindow.pickLanguage"))
                    .tonal()
                    .size(ButtonSize::Compact)
                    .icon("icons/languages.svg")
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.open_menu(Side::Target, cx))),
                Fix::Retry => gpui_m3::Button::new("text-retry", t("pill.textWindow.retry"))
                    .tonal()
                    .size(ButtonSize::Compact)
                    .icon("icons/rotate-cw.svg")
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.translate(cx))),
            };
            return div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(14.))
                .px(px(20.))
                .child(div().text_center().text_color(s.on_surface_variant).type_style(type_scale::BODY_MEDIUM).child(problem.message.clone()))
                .child(action);
        }
        let empty = self.translation.read(cx).text().is_empty();
        if self.translating && empty {
            return div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(12.))
                .child(gpui_m3::LoadingIndicator::new().size(px(32.)))
                .child(div().text_color(s.on_surface_variant).type_style(type_scale::BODY_MEDIUM).child(
                    if self.active == Some(Engine::Ai) { t("pill.textWindow.translatingAi") } else { t("pill.textWindow.translating") },
                ));
        }
        // Retraduciendo: la anterior queda apagada hasta que llegue la nueva.
        Self::area(&self.translation).when(self.translating || self.stale(cx), |el| el.opacity(0.5))
    }

    fn pane_footer(&self, side: Side, enabled: bool, cx: &mut Context<Self>) -> gpui::Div {
        let picker = if side == Side::Target { self.engine_picker(cx) } else { None };
        div()
            .h(px(48.))
            .flex_none()
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(8.))
            .children(picker)
            .child(div().flex_1())
            .child(self.copy_button(side, enabled, cx))
    }

    /// «Con IA / Rápida», bajo la traducción: solo si hay de las dos.
    fn engine_picker(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let engines = self.engines.as_ref().filter(|engines| engines.both())?;
        let ai = engines.ai.as_ref().map(ai_name).unwrap_or_default();
        let selected = usize::from(self.engine() == Some(Engine::Quick));
        let segments = [gpui_m3::Segment::label(t("pill.textWindow.engineAi")), gpui_m3::Segment::label(t("pill.textWindow.engineQuick"))];
        let view = cx.entity().downgrade();
        let picker = gpui_m3::SegmentedButtons::new("text-engine", segments, selected).dense(true).sliding(true).on_change(move |index, _, cx| {
            let engine = if index == 0 { Engine::Ai } else { Engine::Quick };
            let _ = view.update(cx, |view, cx| view.choose(engine, cx));
        });
        Some(
            div()
                .id("text-engine-tip")
                .pl(px(6.))
                .tooltip(gpui_m3::Tooltip::text(tf("pill.textWindow.engineTip", &[("ai", &ai)])))
                .child(picker)
                .into_any_element(),
        )
    }

    /// La ayuda de «Traducir»: con qué va a traducir o cómo encenderlo.
    fn translate_tip(&self) -> String {
        let ai = self.engines.as_ref().and_then(|engines| engines.ai.as_ref());
        match (self.engine(), ai) {
            (Some(Engine::Ai), Some(AiSource::Provider { name })) => tf("pill.textWindow.translateTipAi", &[("ai", name)]),
            (Some(Engine::Ai), Some(AiSource::Apple)) => t("pill.textWindow.translateTipApple").to_string(),
            (Some(Engine::Quick), _) => t("pill.textWindow.translateTip").to_string(),
            _ if self.cannot_translate() => t("pill.textWindow.needsAi").to_string(),
            // Todavía se averigua con qué (un instante al abrir).
            _ => t("pill.textWindow.translate").to_string(),
        }
    }

    /// El original solo, a lo ancho: Copiar y «Traducir» (apagado si en este
    /// equipo no hay con qué, con la ayuda de cómo encenderlo).
    fn single(&self, has_original: bool, cx: &mut Context<Self>) -> gpui::Div {
        let translate = gpui_m3::Button::new("text-translate", t("pill.textWindow.translate"))
            .tonal()
            .size(ButtonSize::Compact)
            .icon("icons/languages.svg")
            .tooltip(self.translate_tip())
            .disabled(!has_original || self.cannot_translate())
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.set_translation(true, cx)));
        let footer = div()
            .h(px(52.))
            .flex_none()
            .px(px(8.))
            .flex()
            .items_center()
            .justify_end()
            .gap(px(8.))
            .child(translate)
            .child(self.copy_button(Side::Source, has_original, cx));
        div().flex_1().min_w_0().flex().flex_col().pt(px(14.)).child(Self::area(&self.original)).child(footer)
    }
}

impl Render for TextExtract {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let s = *gpui_m3::Theme::of(cx);
        // El tema puede cambiar con la vista abierta (claro/oscuro).
        let (text, muted, accent) = colors();
        self.original.update(cx, |area, cx| area.set_colors(text, muted, accent, cx));
        self.translation.update(cx, |area, cx| area.set_colors(text, muted, accent, cx));

        let card = div().flex_1().min_h_0().w_full().relative().flex().rounded(gpui_m3::corner::L).bg(s.surface_container);
        // Toma el alto que le deja quien la contiene (una columna flex), sin pasarse:
        // con `size_full` el pie con Copiar quedaba bajo el borde del panel.
        let root = div().flex_1().min_h_0().w_full().flex().flex_col().font_family(crate::theme::font()).text_color(s.on_surface);
        if let Some(status) = self.status(&s) {
            return root.child(card.child(status));
        }
        let has_original = !self.original.read(cx).text().trim().is_empty();
        if !self.show_translation {
            // Sin traducción pedida: solo el original, a lo ancho.
            return root.child(card.child(self.single(has_original, cx)));
        }

        let head = |side: Side, picker: AnyElement| {
            // El botón de intercambiar va sobre la división: cada idioma le deja lugar.
            let (pad_left, pad_right) = if side == Side::Source { (10.0, SWAP / 2.0 + 8.0) } else { (SWAP / 2.0 + 10.0, 6.0) };
            div().h(px(HEAD_H)).flex_none().pl(px(pad_left)).pr(px(pad_right)).flex().items_center().gap(px(4.)).child(picker)
        };
        let close = gpui_m3::IconButton::new("text-translation-close", "icons/x.svg")
            .size(px(SWAP))
            .tooltip(t("pill.textWindow.closeTranslation"))
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.set_translation(false, cx)));
        let source = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(head(Side::Source, self.picker(Side::Source, &s, cx)))
            .child(Self::area(&self.original))
            .child(self.pane_footer(Side::Source, has_original, cx));
        let has_translation = !self.translating && self.problem.is_none() && !self.translation.read(cx).text().trim().is_empty();
        let target = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(head(Side::Target, self.picker(Side::Target, &s, cx)).child(div().flex_1()).child(close))
            .child(self.target_body(&s, cx))
            .child(self.pane_footer(Side::Target, has_translation, cx));
        let divider = div().w(px(1.)).flex_none().my(px(HEAD_H)).bg(s.outline_variant);
        let can_swap = self.source_language().is_some() && !self.translating;
        let swap = div()
            .absolute()
            .top(px((HEAD_H - SWAP) / 2.0))
            .left(relative(0.5))
            .ml(px(-SWAP / 2.0))
            .rounded_full()
            .bg(s.surface_container)
            .child(
                gpui_m3::IconButton::new("text-swap", "icons/arrow-left-right.svg")
                    .size(px(SWAP))
                    .tooltip(t("pill.textWindow.swap"))
                    .disabled(!can_swap)
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.swap(cx))),
            );
        root.child(card.child(source).child(divider).child(target).child(swap))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn los_idiomas_tienen_nombre_y_se_reconocen_por_su_base() {
        for code in LANGUAGES {
            assert!(language_name(code).is_some(), "{code} sin nombre");
        }
        assert_eq!(base("zh-TW"), "zh");
        assert_eq!(language_name("zh-Hant"), language_name("zh"));
        assert_eq!(request_code("zh"), "zh-Hans");
        assert_eq!(request_code("en"), "en");
        // Uno que no está en la lista toma el nombre del Mac, con mayúscula.
        assert_eq!(display_name("la", "latín"), "Latín");
        assert_eq!(display_name("xx", ""), "Xx");
        assert_eq!(other_language("es-CL"), "en");
        assert_eq!(other_language("en"), "es");
    }
}
