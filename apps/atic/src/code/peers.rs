//! Los mensajes entre sesiones de Claude de esta máquina, como una conversación:
//!
//! - Lo que escribió otra sesión (`<cross-session-message>`) es una tarjeta con su nombre,
//!   su modo y el mensaje en Markdown, no una burbuja tuya con el XML.
//! - Lo que esta sesión le mandó a otra (`SendMessage`) es la misma tarjeta, con «Mensaje a
//!   claude-6d», el resumen y si se entregó. Si responde a una que le escribió, el
//!   destinatario es la dirección de su tubería (`uds:\\.\pipe\…`): se muestra su nombre.
//!
//! Un mensaje largo se ve recortado, con «Mostrar todo». Los colores salen de los tokens del
//! estilo, así que se leen en Formal, Expressive y Liquid Glass, en claro y en oscuro.

use gpui::{div, prelude::*, px, AnyElement, ClickEvent, Context, Div, FontWeight, SharedString};
use gpui_m3::{Card, Icon, Tone};
use serde_json::Value;

use super::chat::{Chat, Item, PeerNote, ToolCall};
use super::style::t;
use super::CodeView;
use crate::i18n::t as tr;

/// Desde cuántos caracteres un mensaje se ve recortado hasta «Mostrar todo».
const LONG_CHARS: usize = 700;
/// El alto del mensaje recortado.
const CLIPPED_H: f32 = 190.;

/// El destinatario de un `SendMessage` para mostrar: sin el sufijo ` [3e31c9]` que pone
/// `ListAgents`, «todas» para `*` y, si es la dirección de una tubería, el nombre de la sesión
/// que escribió desde ella (o «Otra sesión»).
pub(super) fn recipient(to: &str, items: &[Item]) -> String {
    let to = to.trim();
    if to == "*" {
        return tr("pill.code.peer.everyone").to_string();
    }
    if to.starts_with("uds:") || to.contains("\\pipe\\") {
        return items
            .iter()
            .rev()
            .find_map(|item| match item {
                Item::Peer(note) if note.from == to && !note.name.is_empty() => Some(note.name.clone()),
                _ => None,
            })
            .unwrap_or_else(|| tr("pill.code.peer.anotherSession").to_string());
    }
    match to.rfind(" [") {
        Some(at) if to.ends_with(']') && at > 0 => to[..at].to_string(),
        _ => to.to_string(),
    }
}

/// Cómo terminó un `SendMessage`: `Ok(())` si se entregó, `Err(texto)` si no.
pub(super) fn delivery(tool: &ToolCall) -> Option<Result<(), String>> {
    let result = tool.result.as_deref()?;
    if tool.is_error {
        return Some(Err(result.trim().to_string()));
    }
    // El resultado llega como JSON (`{"success":true,"message":…}`); si no, se da por entregado.
    let json = serde_json::from_str::<Value>(result.trim()).ok();
    match json.as_ref().and_then(|v| v.get("success")).and_then(Value::as_bool) {
        Some(false) => Some(Err(json.as_ref().and_then(|v| v.get("message")).and_then(Value::as_str).unwrap_or(result).to_string())),
        _ => Some(Ok(())),
    }
}

/// Un chip chico de texto (el modo de la sesión, «reenviado»).
fn chip(text: String) -> Div {
    let tokens = t();
    div().flex_none().px(px(7.)).py(px(1.)).rounded(px(6.)).bg(tokens.hover).text_size(px(11.)).text_color(tokens.muted).child(text)
}

impl CodeView {
    /// Lo que escribió otra sesión.
    pub(super) fn peer_note(&self, chat: &Chat, index: usize, note: &PeerNote, _fresh: bool, cx: &mut Context<Self>) -> AnyElement {
        let tokens = t();
        let header = div()
            .flex()
            .items_center()
            .gap(px(7.))
            .min_w(px(0.))
            .text_size(px(12.5))
            .child(Icon::new("chat").size(px(14.)).color(tokens.accent))
            .child(div().flex_none().font_weight(FontWeight::SEMIBOLD).text_color(tokens.text).child(note.label()))
            .child(div().flex_none().text_color(tokens.muted).child(tr("pill.code.peer.wrote")))
            .when_some(note.mode.clone(), |el, mode| el.child(chip(mode)))
            .when(note.relayed, |el| el.child(chip(tr("pill.code.peer.relayed").to_string())));
        self.peer_card(format!("peer-{}-{index}", chat.key), note.label(), header, &note.body, true, cx)
    }

    /// Lo que esta sesión le mandó a otra (`SendMessage`).
    pub(super) fn send_message_card(&self, chat: &Chat, tool: &ToolCall, cx: &mut Context<Self>) -> AnyElement {
        let tokens = t();
        let input = tool.input.clone().unwrap_or(Value::Null);
        let field = |name: &str| input.get(name).and_then(Value::as_str).map(str::trim).unwrap_or_default().to_string();
        let to = recipient(&field("to"), &chat.items);
        let summary = field("summary");
        // `message` puede venir como texto o, en los equipos, como un objeto.
        let body = match input.get("message") {
            Some(Value::String(text)) => text.clone(),
            Some(other) if !other.is_null() => format!("```json\n{}\n```", serde_json::to_string_pretty(other).unwrap_or_default()),
            _ => String::new(),
        };
        let m3 = super::view::expressive();
        let (status, failed): (AnyElement, Option<String>) = match delivery(tool) {
            None if m3 => (super::spinners::slot(14.).into_any_element(), None),
            None => (gpui::svg().path("icons/circle-dot.svg").size(px(13.)).text_color(tokens.warn).into_any_element(), None),
            Some(Ok(())) => (Icon::new("check").size(px(13.)).color(tokens.ok).into_any_element(), None),
            Some(Err(error)) => (Icon::new("x").size(px(13.)).color(tokens.bad).into_any_element(), Some(error)),
        };
        let header = div()
            .flex()
            .items_center()
            .gap(px(7.))
            .min_w(px(0.))
            .text_size(px(12.5))
            .child(Icon::new("export").size(px(14.)).color(tokens.muted))
            .child(div().flex_none().text_color(tokens.muted).child(tr("pill.code.peer.messageTo")))
            .child(div().flex_none().font_weight(FontWeight::SEMIBOLD).text_color(tokens.text).child(to.clone()))
            .when(!summary.is_empty(), |el| el.child(div().min_w(px(0.)).truncate().text_color(tokens.muted).child(format!("· {summary}"))))
            .child(div().flex_1())
            .child(div().size(px(16.)).flex_none().flex().items_center().justify_center().child(status));
        let card = self.peer_card(format!("peer-sent-{}", tool.id), format!("{} {to}", tr("pill.code.peer.messageTo")), header, &body, false, cx);
        match failed {
            Some(error) => div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(card)
                .child(super::tools::output(format!("{}-err", tool.id), &error, 6, None, true))
                .into_any_element(),
            None => card,
        }
    }

    /// La tarjeta de un mensaje: la cabecera y el texto. En Expressive es la `Card` de gpui-m3,
    /// tonal si viene de otra sesión (`incoming`); en Formal y Glass, el contenedor con borde de
    /// las herramientas. Un texto largo va recortado hasta «Mostrar todo»; abierto, `label` es
    /// su título en la cabecera flotante (`fold_bar.rs`).
    fn peer_card(&self, id: String, label: String, header: Div, body: &str, incoming: bool, cx: &mut Context<Self>) -> AnyElement {
        let tokens = t();
        let long = body.chars().count() > LONG_CHARS || body.lines().count() > 12;
        let open = self.expanded.contains(&id);
        let clipped = long && !open;
        let fold = (long && open).then(|| super::fold_bar::Fold::new(id.clone(), label));
        let toggle = id.clone();
        let more = long.then(|| {
            div().flex().child(
                div()
                    .id(SharedString::from(format!("{id}-more")))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .h(px(26.))
                    .px(px(8.))
                    .ml(px(-8.))
                    .rounded(px(8.))
                    .cursor_pointer()
                    .text_size(px(12.))
                    .text_color(tokens.accent)
                    .hover(|el| el.bg(tokens.hover))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        if !view.expanded.remove(&toggle) {
                            view.expanded.insert(toggle.clone());
                        }
                        cx.notify();
                    }))
                    .child(gpui::svg().path(if open { "icons/chevron-up.svg" } else { "icons/chevron-down.svg" }).size(px(13.)).text_color(tokens.accent))
                    .child(if open { tr("pill.code.peer.showLess") } else { tr("pill.code.peer.showAll") }),
            )
        });
        let text = (!body.trim().is_empty()).then(|| {
            div()
                .min_w(px(0.))
                .when(clipped, |el| el.max_h(px(CLIPPED_H)).overflow_hidden())
                .text_size(px(13.5))
                .child(super::view::markdown(&format!("{id}-md"), body, cx))
        });
        if super::view::expressive() {
            let card = if incoming { Card::tonal(Tone::Tertiary) } else { Card::plain() };
            let card = card.padding_xy(px(16.), px(12.)).radius(px(20.)).gap(px(6.)).child(header).children(text).children(more).into_any_element();
            return super::fold_bar::track(card, fold, super::fold_bar::HEAD);
        }
        let card = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .px(px(14.))
            .py(px(10.))
            .rounded(px(tokens.r_ctl.max(8.).min(14.)))
            .border_1()
            .border_color(if incoming { tokens.accent.opacity(0.45) } else { tokens.border })
            .bg(tokens.raised)
            .child(header)
            .children(text)
            .children(more)
            .into_any_element();
        super::fold_bar::track(card, fold, super::fold_bar::HEAD)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(name: &str, from: &str) -> Item {
        Item::Peer(PeerNote { name: name.into(), from: from.into(), body: "hola".into(), ..Default::default() })
    }

    #[test]
    fn el_destinatario_se_lee_como_nombre() {
        let pipe = r"uds:\\.\pipe\LOCAL\cc-msg-2096";
        let items = vec![peer("claude-b2", pipe)];
        // A una tubería se le pone el nombre de la sesión que escribió desde ella.
        assert_eq!(recipient(pipe, &items), "claude-b2");
        // El sufijo que agrega ListAgents se va; un nombre suelto queda igual.
        assert_eq!(recipient("imp [3e31c9]", &items), "imp");
        assert_eq!(recipient("claude-6d", &items), "claude-6d");
        assert_ne!(recipient(r"uds:\\.\pipe\LOCAL\otra", &items), "claude-b2");
    }

    #[test]
    fn la_entrega_sale_del_json_del_resultado() {
        let mut tool = ToolCall::new("t".into(), "SendMessage".into(), None);
        assert_eq!(delivery(&tool), None);
        tool.result = Some(r#"{"success":true,"message":"“X” → imp","msg_id":"1"}"#.into());
        assert_eq!(delivery(&tool), Some(Ok(())));
        tool.result = Some(r#"{"success":false,"message":"No such agent"}"#.into());
        assert_eq!(delivery(&tool), Some(Err("No such agent".into())));
        tool.result = Some("boom".into());
        tool.is_error = true;
        assert_eq!(delivery(&tool), Some(Err("boom".into())));
    }
}
