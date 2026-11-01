//! El teclado del editor: quién maneja cada tecla cuando hay un control enfocado, el modo
//! «se navega con el teclado» (para mostrar el anillo de foco solo entonces), la hoja de
//! atajos y las teclas que llevan los globitos.
//!
//! **Foco visible.** GPUI 0.2.2 no tiene `focus_visible`: un `div` enfocado con un clic y uno
//! enfocado con Tab se ven igual. `RecordingEditor::kbd_nav` lo reemplaza: se enciende cuando
//! *el teclado mueve el foco* (Tab, o una flecha sobre una opción) y se apaga con el primer clic
//! del mouse. El anillo se dibuja solo con `kbd_nav`, así que una pestaña recién clicada no se ve
//! como «seleccionada con aro». Y mientras el foco vino del mouse, Espacio y las flechas siguen
//! siendo los atajos del editor (reproducir, mover el cabezal): un control solo las toma si el foco
//! llegó con el teclado.

use std::rc::Rc;

use gpui::{div, prelude::*, px, AnyElement, ClickEvent, Context, Div, Hsla, KeyDownEvent, Modifiers, MouseButton, MouseDownEvent, SharedString, Stateful, Window};

use super::export_panel::{dialog_top, DIALOG_MARGIN, SCRIM_ALPHA, SHADOW_ALPHA};
use super::fluid;
use super::palette::{Ed, RING_W, T_BODY, T_SMALL, T_TITLE};
use super::{RecordingEditor, HELP_W, KEY_CAP_H};
use crate::i18n::t;

/// Lo que hace una tecla sobre un control enfocado (una opción, un interruptor, un botón).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum KeyAction {
    /// Espacio o Intro: lo mismo que un clic.
    Activate,
    /// El foco pasa al control siguiente / anterior (una flecha dentro de un grupo de opciones).
    Next,
    Prev,
    /// Esc: el teclado vuelve al editor.
    Leave,
}

/// Qué hace `key` sobre un control enfocado. `kbd_nav`: el foco llegó con el teclado; si no (se
/// enfocó con un clic), solo Esc es del control y el resto sigue siendo atajo del editor.
pub(super) fn control_key(key: &str, m: &Modifiers, kbd_nav: bool) -> Option<KeyAction> {
    if m.platform || m.control || m.alt {
        return None;
    }
    match key {
        "escape" => Some(KeyAction::Leave),
        "space" | "enter" if kbd_nav => Some(KeyAction::Activate),
        "left" | "up" if kbd_nav && !m.shift => Some(KeyAction::Prev),
        "right" | "down" if kbd_nav && !m.shift => Some(KeyAction::Next),
        _ => None,
    }
}

/// Tab (+1) o Shift+Tab (−1): pasar el foco al control siguiente o al anterior.
pub(super) fn tab_step(key: &str, m: &Modifiers) -> Option<i8> {
    if key != "tab" || m.platform || m.control || m.alt {
        return None;
    }
    Some(if m.shift { -1 } else { 1 })
}

/// ¿Es «?» (la hoja de atajos)? Según el teclado llega como la tecla «?» o como «/» con Shift.
pub(super) fn is_help_key(key: &str, key_char: Option<&str>, m: &Modifiers) -> bool {
    if m.platform || m.control || m.alt {
        return false;
    }
    key_char == Some("?") || key == "?" || (key == "/" && m.shift)
}

// --- La hoja de atajos ------------------------------------------------------------------------

/// Una fila de la hoja: las teclas (cada una es un cuadrito; «space» se traduce) y la clave del texto.
pub(super) struct Shortcut {
    pub keys: &'static [&'static str],
    pub label: &'static str,
}

/// Todos los atajos del editor, en el orden en que se muestran.
pub(super) const SHORTCUTS: [Shortcut; 17] = [
    Shortcut { keys: &["space"], label: "pill.editor.keys.play" },
    Shortcut { keys: &["←", "→"], label: "pill.editor.keys.seek" },
    Shortcut { keys: &["Home", "End"], label: "pill.editor.keys.ends" },
    Shortcut { keys: &["S"], label: "pill.editor.keys.cut" },
    Shortcut { keys: &["Z"], label: "pill.editor.keys.zoom" },
    Shortcut { keys: &["T"], label: "pill.editor.keys.text" },
    Shortcut { keys: &["V"], label: "pill.editor.keys.speed" },
    Shortcut { keys: &["B"], label: "pill.editor.keys.blur" },
    Shortcut { keys: &["A"], label: "pill.editor.keys.auto" },
    Shortcut { keys: &["+", "−", "0"], label: "pill.editor.keys.view" },
    Shortcut { keys: &["⌫"], label: "pill.editor.keys.delete" },
    Shortcut { keys: &["Esc"], label: "pill.editor.keys.escape" },
    Shortcut { keys: &["⌘", "Z"], label: "pill.editor.keys.undo" },
    Shortcut { keys: &["⇧", "⌘", "Z"], label: "pill.editor.keys.redo" },
    Shortcut { keys: &["⌘", "E"], label: "pill.editor.keys.export" },
    Shortcut { keys: &["Tab"], label: "pill.editor.keys.tab" },
    Shortcut { keys: &["?"], label: "pill.editor.keys.help" },
];

// --- Las teclas de los globitos ---------------------------------------------------------------

/// Las teclas con nombre que un globito puede traer tras « · » aunque midan más de `KEY_MAX` letras.
const NAMED_KEYS: [&str; 6] = ["Espacio", "Space", "Esc", "Supr", "Del", "Intro"];
/// Lo más largo (en caracteres) que es una tecla suelta o un atajo como «⇧⌘Z».
const KEY_MAX: usize = 4;

/// Separa la tecla del final de un globito: «Cortar el tramo · S» → («Cortar el tramo», Some("S")).
/// Solo si lo que sigue a « · » parece una tecla (corto, sin espacios, o una con nombre): una
/// explicación larga con « · » no se toca.
pub(super) fn split_hint(text: &'static str) -> (&'static str, Option<&'static str>) {
    match text.rsplit_once(" · ") {
        Some((head, tail)) if !head.is_empty() && !tail.contains(' ') && (tail.chars().count() <= KEY_MAX || NAMED_KEYS.contains(&tail)) => (head, Some(tail)),
        _ => (text, None),
    }
}

/// El globito de un botón del editor: el texto y, si lo trae, su tecla como cuadrito aparte.
pub(super) fn tip(text: &'static str) -> impl Fn(&mut Window, &mut gpui::App) -> gpui::AnyView + 'static {
    let (body, key) = split_hint(text);
    crate::hover::tip_keyed(body, key)
}

// --- El foco: anillo, Tab y teclas de un control ---------------------------------------------

impl RecordingEditor {
    /// Pasa el foco al control siguiente (`dir` > 0) o al anterior, y entra en modo teclado.
    pub(super) fn step_focus(&mut self, dir: i8, window: &mut Window, cx: &mut Context<Self>) {
        self.kbd_nav = true;
        if dir >= 0 {
            window.focus_next();
        } else {
            window.focus_prev();
        }
        cx.notify();
    }

    /// Un clic del mouse apaga el modo teclado (el anillo se va).
    pub(super) fn mouse_nav(&mut self, cx: &mut Context<Self>) {
        if self.kbd_nav {
            self.kbd_nav = false;
            cx.notify();
        }
    }

    /// Hace `el` alcanzable con Tab y le da el anillo de foco (solo en modo teclado). El elemento
    /// lleva un borde transparente de `RING_W` para no moverse al enfocarse.
    /// `ring` es el color del anillo: el acento, o el del texto sobre un relleno de acento.
    pub(super) fn ringed(&self, ring: Hsla, el: Stateful<Div>) -> Stateful<Div> {
        let el = el.border(px(RING_W)).border_color(gpui::transparent_black()).tab_index(0);
        if self.kbd_nav {
            el.focus(move |s| s.border_color(ring))
        } else {
            el
        }
    }

    /// Un control con su clic y su teclado: `act` corre con el clic y, con el foco llegado por teclado, con
    /// Espacio o Intro; las flechas pasan el foco de una opción a la de al lado.
    pub(super) fn interactive(&self, ring: Hsla, el: Stateful<Div>, act: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static, cx: &mut Context<Self>) -> Stateful<Div> {
        let act = Rc::new(act);
        let on_click = act.clone();
        self.ringed(ring, el)
            .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| on_click(view, window, cx)))
            .on_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| {
                match control_key(&event.keystroke.key, &event.keystroke.modifiers, view.kbd_nav) {
                    Some(KeyAction::Activate) => {
                        act(view, window, cx);
                        cx.stop_propagation();
                    }
                    Some(KeyAction::Next) => {
                        view.step_focus(1, window, cx);
                        cx.stop_propagation();
                    }
                    Some(KeyAction::Prev) => {
                        view.step_focus(-1, window, cx);
                        cx.stop_propagation();
                    }
                    // Esc devuelve el teclado al editor; la tecla sigue su camino (suelta lo elegido).
                    Some(KeyAction::Leave) => window.focus(&view.focus),
                    None => {}
                }
            }))
    }
}

// --- La hoja de atajos ------------------------------------------------------------------------

/// El texto de una tecla de la hoja: «space» se traduce (Espacio / Space); el resto son símbolos.
fn key_name(key: &'static str) -> &'static str {
    if key == "space" {
        t("pill.editor.keys.space")
    } else {
        key
    }
}

impl RecordingEditor {
    /// La hoja de atajos («?»): una tarjeta chica, del mismo estilo y con las mismas entradas y salidas que el
    /// diálogo de exportar, anclada arriba. `None` si está cerrada (y ya terminó de salir).
    pub(super) fn help_overlay(&self, pal: &Ed, window: &Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.help_open;
        let scrim = self.fluid.toggle("help-scrim", open, fluid::SCRIM_IN, fluid::DIALOG_OUT);
        let card_in = self.fluid.toggle("help-card", open, fluid::CARD_IN, fluid::DIALOG_OUT);
        if !open && scrim <= 0.0 && card_in <= 0.0 {
            return None;
        }
        let rows: Vec<AnyElement> = SHORTCUTS
            .iter()
            .map(|shortcut| {
                let caps = shortcut.keys.iter().map(|key| {
                    div()
                        .h(px(KEY_CAP_H))
                        .min_w(px(KEY_CAP_H))
                        .px(px(6.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(5.))
                        .bg(pal.control)
                        .font_family(pal.mono)
                        .text_size(px(T_SMALL))
                        .text_color(pal.text)
                        .child(key_name(key))
                });
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .h(px(28.))
                    .child(div().flex_1().min_w_0().truncate().text_size(px(T_BODY)).text_color(pal.text).child(t(shortcut.label)))
                    .child(div().flex().flex_none().items_center().gap(px(3.)).children(caps))
                    .into_any_element()
            })
            .collect();
        let mut card_bg = if pal.glass { pal.bg } else { pal.panel };
        card_bg.a = 1.0;
        let shadow = gpui::BoxShadow { color: gpui::black().opacity(SHADOW_ALPHA), offset: gpui::point(px(0.), px(10.)), blur_radius: px(30.), spread_radius: px(0.) };
        let header = div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.))
            .pl(px(18.))
            .pr(px(10.))
            .pt(px(14.))
            .pb(px(8.))
            .child(div().flex_1().min_w_0().text_size(px(T_TITLE)).font_weight(gpui::FontWeight::SEMIBOLD).child(t("pill.editor.keys.title")))
            .child(self.button(pal, "help-close", "icons/x.svg", None, t("pill.editor.exportCloseTip"), |view, _, cx| {
                view.help_open = false;
                cx.notify();
            }, cx));
        let card = div()
            .id("help-card")
            .relative()
            .top(px(self.fluid.lift(card_in, fluid::CARD_RISE)))
            .opacity(card_in)
            .w(px(HELP_W))
            .max_w_full()
            .max_h_full()
            .flex()
            .flex_col()
            .rounded(px(pal.r_panel))
            .bg(card_bg)
            .border_1()
            .border_color(pal.border.opacity(0.6))
            .shadow(vec![shadow])
            .overflow_hidden()
            .text_color(pal.text)
            .font_family(pal.font)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(header)
            .child(div().id("help-rows").flex_1().min_h_0().overflow_y_scroll().px(px(18.)).pb(px(16.)).flex().flex_col().children(rows));
        Some(
            div()
                .id("help-overlay")
                .absolute()
                .inset_0()
                .occlude()
                .flex()
                .justify_center()
                .items_start()
                .pt(px(dialog_top(f32::from(window.viewport_size().height))))
                .px(px(DIALOG_MARGIN))
                .pb(px(DIALOG_MARGIN))
                .bg(gpui::black().opacity(SCRIM_ALPHA * scrim))
                .when(open, |el| {
                    el.on_mouse_down(MouseButton::Left, cx.listener(|view, _: &MouseDownEvent, _, cx| {
                        view.help_open = false;
                        cx.notify();
                    }))
                })
                .child(card)
                .when(!open, |el| el.child(div().absolute().inset_0().occlude()))
                .into_any_element(),
        )
    }

    /// El enlace «Atajos» del pie de la barra lateral: abre la hoja (también con «?»).
    pub(super) fn help_link(&self, pal: &Ed, cx: &mut Context<Self>) -> AnyElement {
        let el = div()
            .id("editor-help")
            .h(px(28.))
            .px(px(8.))
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .rounded(px(pal.r_ctl))
            .cursor_pointer()
            .text_size(px(T_BODY))
            .tooltip(tip(t("pill.editor.keys.helpTip")));
        let (sub, text, control, hover) = (pal.sub, pal.text, pal.control, pal.hover);
        use crate::hover::HoverExt;
        self.interactive(pal.accent, el, |view, _, cx| {
            view.help_open = true;
            cx.notify();
        }, cx)
        .fx(SharedString::from("editor-help-fx"), move |el, h| {
            el.bg(h.mix(hover.opacity(0.0), hover))
                .text_color(h.mix(sub, text))
                .child(super::key_cap("?", sub, control))
                .child(t("pill.editor.keys.link"))
        })
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mods(shift: bool, command: bool) -> Modifiers {
        Modifiers { shift, platform: command, ..Default::default() }
    }

    #[test]
    fn con_el_foco_del_mouse_espacio_y_flechas_siguen_siendo_del_editor() {
        let none = mods(false, false);
        for key in ["space", "enter", "left", "right", "up", "down"] {
            assert_eq!(control_key(key, &none, false), None, "{key}");
        }
        // Esc siempre es del control: devuelve el teclado al editor.
        assert_eq!(control_key("escape", &none, false), Some(KeyAction::Leave));
        assert_eq!(control_key("escape", &none, true), Some(KeyAction::Leave));
    }

    #[test]
    fn con_el_foco_del_teclado_el_control_toma_espacio_intro_y_flechas() {
        let none = mods(false, false);
        assert_eq!(control_key("space", &none, true), Some(KeyAction::Activate));
        assert_eq!(control_key("enter", &none, true), Some(KeyAction::Activate));
        assert_eq!(control_key("left", &none, true), Some(KeyAction::Prev));
        assert_eq!(control_key("up", &none, true), Some(KeyAction::Prev));
        assert_eq!(control_key("right", &none, true), Some(KeyAction::Next));
        assert_eq!(control_key("down", &none, true), Some(KeyAction::Next));
        assert_eq!(control_key("a", &none, true), None);
        // ⇧← es «5 s atrás» del editor, no pasar de opción; con ⌘ nada es del control.
        assert_eq!(control_key("left", &mods(true, false), true), None);
        assert_eq!(control_key("space", &mods(false, true), true), None);
    }

    #[test]
    fn tab_va_adelante_y_con_shift_atras() {
        assert_eq!(tab_step("tab", &mods(false, false)), Some(1));
        assert_eq!(tab_step("tab", &mods(true, false)), Some(-1));
        assert_eq!(tab_step("tab", &mods(false, true)), None);
        assert_eq!(tab_step("space", &mods(false, false)), None);
    }

    #[test]
    fn la_hoja_de_atajos_se_abre_con_signo_de_pregunta() {
        let none = mods(false, false);
        assert!(is_help_key("/", Some("?"), &mods(true, false)));
        assert!(is_help_key("?", None, &none));
        assert!(is_help_key("/", None, &mods(true, false)));
        assert!(!is_help_key("/", Some("/"), &none));
        assert!(!is_help_key("z", Some("z"), &none));
        assert!(!is_help_key("/", Some("?"), &mods(true, true)));
    }

    #[test]
    fn la_tecla_se_separa_del_final_del_globito() {
        assert_eq!(split_hint("Quitar un tramo desde el cabezal (después lo estiras) · S"), ("Quitar un tramo desde el cabezal (después lo estiras)", Some("S")));
        assert_eq!(split_hint("Atrás 5 s · ⇧←"), ("Atrás 5 s", Some("⇧←")));
        assert_eq!(split_hint("Deshacer el último cambio · ⌘Z"), ("Deshacer el último cambio", Some("⌘Z")));
        assert_eq!(split_hint("Reproducir o pausar · Espacio"), ("Reproducir o pausar", Some("Espacio")));
        assert_eq!(split_hint("Quitar · Supr"), ("Quitar", Some("Supr")));
        // Una explicación con « · » no es una tecla.
        let long = "Arrastra · doble clic restablece · ←/→ ajustan (⇧ más)";
        assert_eq!(split_hint(long), (long, None));
        assert_eq!(split_hint("Mostrar en Finder"), ("Mostrar en Finder", None));
        assert_eq!(split_hint("Cerrar la vista previa · Cerrar"), ("Cerrar la vista previa · Cerrar", None));
        assert_eq!(split_hint(" · S"), (" · S", None));
    }

    #[test]
    fn el_anclaje_del_dialogo_baja_un_poco_pero_nunca_pega_al_borde() {
        assert_eq!(dialog_top(1000.0), 120.0);
        assert_eq!(dialog_top(100.0), DIALOG_MARGIN);
        assert_eq!(dialog_top(0.0), DIALOG_MARGIN);
    }

    #[test]
    fn la_hoja_no_repite_teclas_ni_textos() {
        let mut labels: Vec<_> = SHORTCUTS.iter().map(|s| s.label).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), SHORTCUTS.len());
        // Cada letra de herramienta aparece una sola vez como atajo de una sola tecla.
        let mut singles: Vec<_> = SHORTCUTS.iter().filter(|s| s.keys.len() == 1).map(|s| s.keys[0]).collect();
        singles.sort_unstable();
        singles.dedup();
        assert_eq!(singles.len(), SHORTCUTS.iter().filter(|s| s.keys.len() == 1).count());
        assert!(SHORTCUTS.iter().all(|s| !s.keys.is_empty()));
    }
}
