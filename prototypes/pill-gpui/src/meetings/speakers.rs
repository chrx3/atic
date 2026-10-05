//! Ponerle nombre a un hablante: un clic en «Yo» / «Los demás» / el nombre de
//! un bloque lo vuelve un campo; Enter guarda, Esc cancela. Cambia todos los
//! tramos de ese mismo hablante (misma pista y mismo nombre previo), como la
//! web, y se guarda en `transcript.json`.

use atic_core::{Speaker, Transcript};
use gpui::{
    actions, div, prelude::*, px, App, ClickEvent, Context, ElementId, Entity, Focusable, FontWeight, KeyBinding,
    NoAction, SharedString, Window,
};

use super::text::Block;
use super::{hsla, MeetingsView, BLUE, FAINT, LILAC, SURFACE_ON, TEXT};
use crate::hover::{self, HoverExt};
use crate::text_input::TextInput;

actions!(meetings_speaker, [SpeakerConfirm, SpeakerCancel, SpeakerSwallow]);

/// El campo del nombre: sus teclas no llegan al reproductor ni a la lista.
const SPEAKER_CONTEXT: &str = "MeetingsSpeaker";

pub fn bind_keys(cx: &mut App) {
    let field = Some(SPEAKER_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("enter", SpeakerConfirm, field),
        KeyBinding::new("escape", SpeakerCancel, field),
        KeyBinding::new("up", SpeakerSwallow, field),
        KeyBinding::new("down", SpeakerSwallow, field),
        KeyBinding::new("tab", SpeakerSwallow, field),
        KeyBinding::new("ctrl-z", SpeakerSwallow, field),
        KeyBinding::new("ctrl-f", SpeakerSwallow, field),
        // Sin acción: Espacio escribe un espacio en vez de reproducir.
        KeyBinding::new("space", NoAction, field),
    ]);
}

fn speaker_of(me: bool) -> Speaker {
    if me {
        Speaker::Me
    } else {
        Speaker::Others
    }
}

/// El nombre que se guarda: vacío o igual al de siempre («Yo», «Los demás»)
/// es volver al de siempre.
pub fn stored_name(me: bool, name: &str) -> Option<String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    (!name.is_empty() && name != speaker_of(me).label()).then_some(name)
}

/// Renombra todos los tramos del hablante que hoy se ve como `old_label` en
/// esa pista. Devuelve cuántos cambiaron.
pub fn rename(transcript: &mut Transcript, me: bool, old_label: &str, name: &str) -> usize {
    let speaker = speaker_of(me);
    let stored = stored_name(me, name);
    let mut changed = 0;
    for segment in &mut transcript.segments {
        if segment.speaker == speaker && segment.speaker_label() == old_label && segment.speaker_name != stored {
            segment.speaker_name = stored.clone();
            changed += 1;
        }
    }
    changed
}

/// El campo abierto: en qué bloque, de quién y el campo.
pub struct Editing {
    block: usize,
    me: bool,
    old_label: String,
    input: Entity<TextInput>,
}

#[derive(Default)]
pub struct Speakers {
    editing: Option<Editing>,
}

impl MeetingsView {
    fn begin_speaker(&mut self, block: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(b) = self.detail.as_ref().and_then(|d| d.blocks.get(block)) else {
            return;
        };
        let (me, old_label) = (b.me, b.label.clone());
        let input = cx.new(|cx| {
            let mut input = TextInput::new(speaker_of(me).label(), hsla(TEXT), hsla(FAINT), hsla(TEXT), cx);
            input.set_text(old_label.clone(), cx);
            input
        });
        window.focus(&input.focus_handle(cx));
        self.speakers.editing = Some(Editing { block, me, old_label, input });
        cx.notify();
    }

    fn cancel_speaker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.speakers.editing.take().is_some() {
            window.focus(&self.focus);
            cx.notify();
        }
    }

    fn commit_speaker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editing) = self.speakers.editing.take() else {
            return;
        };
        window.focus(&self.focus);
        cx.notify();
        let name = editing.input.read(cx).text().to_string();
        let label = stored_name(editing.me, &name).unwrap_or_else(|| speaker_of(editing.me).label().into());
        if label == editing.old_label {
            return;
        }
        let (Some(paths), Some(detail)) = (self.source.as_ref().and_then(|s| s.paths()).cloned(), self.detail.as_mut())
        else {
            return;
        };
        // Se ve al tiro; el archivo se escribe en el hilo y luego se relee.
        for block in detail.blocks.iter_mut().filter(|b| b.me == editing.me && b.label == editing.old_label) {
            block.label = label.clone();
        }
        let id = detail.id.clone();
        let (me, old_label) = (editing.me, editing.old_label);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    let path = paths.transcript_path(&id);
                    let mut transcript =
                        Transcript::load(&path)?.ok_or_else(|| anyhow::anyhow!("no hay transcripción"))?;
                    if rename(&mut transcript, me, &old_label, &name) > 0 {
                        transcript.save(&path)?;
                    }
                    anyhow::Ok(())
                })
                .await;
            let _ = this.update(cx, |view, cx| {
                if let Err(error) = result {
                    eprintln!("reuniones: no se pudo renombrar al hablante: {error}");
                }
                // Misma reunión: se relee sin mover la pestaña ni el scroll.
                if view.detail.as_ref().is_some_and(|d| d.blocks.len() > 0) {
                    view.load_detail(view.selected, false);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// La etiqueta de hablante de un bloque: un clic la vuelve un campo.
    pub(super) fn speaker_label(&self, ix: usize, block: &Block, cx: &mut Context<Self>) -> gpui::AnyElement {
        let color = hsla(if block.me { BLUE } else { LILAC });
        if let Some(editing) = self.speakers.editing.as_ref().filter(|e| e.block == ix) {
            return div()
                .key_context(SPEAKER_CONTEXT)
                .on_action(cx.listener(|v, _: &SpeakerConfirm, window, cx| v.commit_speaker(window, cx)))
                .on_action(cx.listener(|v, _: &SpeakerCancel, window, cx| v.cancel_speaker(window, cx)))
                .on_action(cx.listener(|_, _: &SpeakerSwallow, _, _| {}))
                .on_mouse_down_out(cx.listener(|v, _, window, cx| v.commit_speaker(window, cx)))
                // El texto queda donde estaba la etiqueta: el fondo sale hacia afuera.
                .ml(px(-8.))
                .my(px(-3.))
                .px(px(8.))
                .h(px(25.))
                .w(px(220.))
                .flex()
                .items_center()
                .rounded(px(8.))
                .bg(hsla(SURFACE_ON))
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(editing.input.clone())
                .into_any_element();
        }
        let label = div()
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(color)
            .child(SharedString::from(block.label.clone()));
        if self.source.as_ref().and_then(|s| s.paths()).is_none() {
            return label.into_any_element();
        }
        let id = ElementId::NamedInteger("meeting-speaker".into(), ix as u64);
        let fx = ElementId::NamedInteger("meeting-speaker-fx".into(), ix as u64);
        div()
            .flex()
            .child(
                label
                    .id(id)
                    .ml(px(-6.))
                    .px(px(6.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .tooltip(hover::tip("Cambiar nombre"))
                    .on_click(cx.listener(move |v, _: &ClickEvent, window, cx| v.begin_speaker(ix, window, cx)))
                    .fx(fx, move |el, h| el.bg(h.mix(hsla(SURFACE_ON).opacity(0.0), hsla(SURFACE_ON)))),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atic_core::Segment;

    fn seg(me: bool, name: Option<&str>, text: &str) -> Segment {
        Segment {
            start_ms: 0,
            end_ms: 1,
            speaker: speaker_of(me),
            speaker_name: name.map(Into::into),
            text: text.into(),
        }
    }

    #[test]
    fn renombra_a_todos_los_del_mismo_hablante() {
        let mut t = Transcript {
            language: None,
            segments: vec![
                seg(false, None, "a"),
                seg(true, None, "b"),
                seg(false, None, "c"),
                seg(false, Some("Ana"), "d"),
            ],
        };
        assert_eq!(rename(&mut t, false, "Los demás", "  Pedro  Soto "), 2);
        let labels: Vec<&str> = t.segments.iter().map(|s| s.speaker_label()).collect();
        // «Yo» y la que ya se llamaba Ana no se tocan.
        assert_eq!(labels, vec!["Pedro Soto", "Yo", "Pedro Soto", "Ana"]);
    }

    #[test]
    fn un_nombre_previo_se_cambia_solo_en_su_pista() {
        let mut t = Transcript {
            language: None,
            segments: vec![seg(false, Some("Ana"), "a"), seg(true, Some("Ana"), "b")],
        };
        assert_eq!(rename(&mut t, false, "Ana", "Ana María"), 1);
        assert_eq!(t.segments[0].speaker_label(), "Ana María");
        assert_eq!(t.segments[1].speaker_label(), "Ana");
    }

    #[test]
    fn vacio_o_el_de_siempre_vuelve_al_de_siempre() {
        let mut t = Transcript { language: None, segments: vec![seg(true, Some("Carlos"), "a")] };
        assert_eq!(rename(&mut t, true, "Carlos", "   "), 1);
        assert_eq!(t.segments[0].speaker_name, None);
        assert_eq!(stored_name(false, "Los demás"), None);
        assert_eq!(stored_name(true, "Yo"), None);
        // Sin cambios no se cuenta (no hace falta guardar).
        assert_eq!(rename(&mut t, true, "Yo", "Yo"), 0);
    }
}
