//! Las filas de la lista al pasar el cursor.
//!
//! Dos cosas, como el portapapeles: la hora·duración y el estado se cruzan
//! con acciones rápidas (▶ y la carpeta), y si el cursor se queda un rato
//! aparece un vistazo al lado con el comienzo del resumen (o de la
//! transcripción) sin tener que elegir la reunión.
//!
//! El vistazo se lee del disco fuera del hilo de UI y queda guardado por id
//! hasta el próximo `reload`. Quieto no pide cuadros: el fundido de entrada es
//! una animación de una vez y el resto lo mueve `on_hover`.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use atic_core::Recording;
use chrono::Local;
use gpui::{
    anchored, deferred, div, point, prelude::*, px, svg, Animation, AnimationExt, AnyElement,
    ClickEvent, Context, Corner, ElementId, FontWeight, SharedString,
};

use super::data::Source;
use super::summary::{self, Block as SummaryBlock, Kind, Section};
use super::text::{self, Block};
use super::{
    hsla, MeetingsView, BODY, FAINT, MUTED, R_CARD, SURFACE, SURFACE_HOVER, SURFACE_ON, TEXT,
};
use crate::hover::{self, mix, HoverExt};

/// Cuánto tiene que quedarse el cursor para que salga el vistazo: menos y
/// aparecería al cruzar la lista de pasada.
const DWELL: Duration = Duration::from_millis(450);
const PEEK_W: f32 = 300.0;
/// Del borde derecho de la fila al vistazo: el margen de la lista, el hueco
/// entre paneles y un poco más, para que quede dentro del detalle.
const PEEK_GAP: f32 = 30.0;
/// Lo que se muestra del texto: unas tres líneas de la tarjeta (~42 letras
/// cada una, menos lo que se pierde al partir palabras). El «…» se pone aquí:
/// el de GPUI con `line_clamp` mide el ancho por tres sin contar los cortes
/// de línea y deja la cuarta línea cortada a media palabra.
const PEEK_CHARS: usize = 110;
/// Un ▶ apretado en otra fila espera a que se lean sus pistas: hasta aquí.
const PLAY_WAIT: Duration = Duration::from_secs(3);
const PLAY_POLL: Duration = Duration::from_millis(30);

/// El texto del vistazo.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Preview {
    Summary(String),
    Transcript(String),
    /// Hay transcripción, pero solo silencio o ruido.
    Silent,
    Nothing,
}

#[derive(Default)]
pub(super) struct RowPeeks {
    /// La fila bajo el cursor (por id) y desde qué paso.
    hovered: Option<String>,
    step: u64,
    /// La fila con el vistazo abierto.
    shown: Option<String>,
    cache: HashMap<String, Preview>,
    loading: HashSet<String>,
    /// Sube con cada `reload`: una lectura que llega después se descarta.
    epoch: u64,
}

impl RowPeeks {
    /// Tras releer la lista: lo guardado puede estar viejo.
    pub(super) fn clear(&mut self) {
        self.cache.clear();
        self.loading.clear();
        self.epoch += 1;
    }
}

/// Elige qué mostrar: el primer párrafo del Resumen; si no, el primero de
/// cualquier sección; si el resumen es solo listas, sus primeros puntos; y
/// sin resumen, el comienzo de la transcripción.
pub(super) fn pick(sections: &[Section], blocks: &[Block], has_transcript: bool) -> Preview {
    let paragraph = |s: &Section| {
        s.blocks.iter().find_map(|b| match b {
            SummaryBlock::Paragraph(p) if !p.trim().is_empty() => Some(p.clone()),
            _ => None,
        })
    };
    let lead = sections.iter().find(|s| s.kind == Kind::Summary);
    let from_summary = lead
        .and_then(paragraph)
        .or_else(|| sections.iter().find_map(paragraph))
        .or_else(|| {
            let items: Vec<&str> = sections
                .iter()
                .flat_map(|s| &s.blocks)
                .filter_map(|b| match b {
                    SummaryBlock::List { items, .. } => Some(items),
                    _ => None,
                })
                .flatten()
                .map(|i| i.text.trim())
                .filter(|t| !t.is_empty())
                .take(3)
                .collect();
            (!items.is_empty()).then(|| items.join(" · "))
        });
    if let Some(text) = from_summary {
        return Preview::Summary(clip(&text, PEEK_CHARS));
    }
    let mut text = String::new();
    for block in blocks {
        if text.chars().count() >= PEEK_CHARS {
            break;
        }
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(block.text.trim());
    }
    match (text.trim().is_empty(), has_transcript) {
        (false, _) => Preview::Transcript(clip(text.trim(), PEEK_CHARS)),
        (true, true) => Preview::Silent,
        (true, false) => Preview::Nothing,
    }
}

/// Corta en un espacio antes de `max` caracteres y deja «…». Sin saltos de
/// línea: el vistazo es un solo párrafo.
pub(super) fn clip(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let cut: String = flat.chars().take(max).collect();
    let cut = match cut.rfind(' ') {
        Some(at) if at > max / 2 => &cut[..at],
        _ => cut.as_str(),
    };
    format!("{}…", cut.trim_end_matches([',', ';', ':', '.', ' ']))
}

/// Lee lo de una reunión para el vistazo (en un hilo aparte).
fn read_preview(source: &Source, id: &str) -> Preview {
    let summary = source.summary(id).ok().flatten();
    let sections = summary
        .as_ref()
        .map(|s| summary::parse(&s.body, "Resumen"))
        .unwrap_or_default();
    if !sections.is_empty() {
        return pick(&sections, &[], false);
    }
    let transcript = source.transcript(id).ok().flatten();
    let blocks = transcript.as_ref().map(|t| text::blocks(&t.segments)).unwrap_or_default();
    pick(&[], &blocks, transcript.is_some())
}

impl MeetingsView {
    /// El cursor entra o sale de una fila.
    pub(super) fn row_hover(&mut self, id: &str, hovered: bool, cx: &mut Context<Self>) {
        let peeks = &mut self.row_peeks;
        if !hovered {
            // De una fila a otra llegan las dos noticias en cualquier orden:
            // solo se apaga lo que es de esta fila.
            if peeks.hovered.as_deref() == Some(id) {
                peeks.hovered = None;
            }
            if peeks.shown.as_deref() == Some(id) {
                peeks.shown = None;
                cx.notify();
            }
            return;
        }
        peeks.hovered = Some(id.to_string());
        peeks.step += 1;
        let (id, step) = (id.to_string(), peeks.step);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(DWELL).await;
            let _ = this.update(cx, |v, cx| {
                let peeks = &v.row_peeks;
                if peeks.step != step || peeks.hovered.as_deref() != Some(id.as_str()) {
                    return;
                }
                v.row_peeks.shown = Some(id.clone());
                v.load_preview(id, cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn load_preview(&mut self, id: String, cx: &mut Context<Self>) {
        let peeks = &mut self.row_peeks;
        if peeks.cache.contains_key(&id) || !peeks.loading.insert(id.clone()) {
            return;
        }
        // `Source` no se clona: se arma otra igual (son solo rutas).
        let source = match self.source.as_ref() {
            Some(source) => match source.paths() {
                Some(paths) => Source::Atic(paths.clone()),
                None => Source::Demo,
            },
            None => return,
        };
        let epoch = peeks.epoch;
        cx.spawn(async move |this, cx| {
            let read_id = id.clone();
            let preview = cx
                .background_spawn(async move { read_preview(&source, &read_id) })
                .await;
            let _ = this.update(cx, |v, cx| {
                let peeks = &mut v.row_peeks;
                if peeks.epoch != epoch {
                    return;
                }
                peeks.loading.remove(&id);
                peeks.cache.insert(id.clone(), preview);
                if peeks.shown.as_deref() == Some(id.as_str()) {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// ▶ de una fila: la elige y suena desde el inicio. Si es otra reunión,
    /// sus pistas se leen al elegirla: se reintenta hasta que estén.
    fn play_row(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.select(ix, cx);
        if self.play_from_start(cx) {
            return;
        }
        let Some(id) = self.items.get(ix).map(|r| r.id.clone()) else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let mut waited = Duration::ZERO;
            while waited < PLAY_WAIT {
                cx.background_executor().timer(PLAY_POLL).await;
                waited += PLAY_POLL;
                let done = this.update(cx, |v, cx| {
                    // Si entretanto se eligió otra, ya no se reproduce nada.
                    let still = v.selected.and_then(|i| v.items.get(i)).is_some_and(|r| r.id == id);
                    !still || v.play_from_start(cx)
                });
                if !matches!(done, Ok(false)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// Las acciones rápidas de una fila; `None` si no hay ninguna.
    fn row_actions(&self, ix: usize, rec: &Recording, cx: &mut Context<Self>) -> Option<gpui::Div> {
        // Sin leer el disco: si la base dice que tiene pistas, se ofrece. Lo
        // que se está grabando no es una fila todavía (va en `live_card`).
        let real = self.source.as_ref().is_some_and(|s| s.paths().is_some());
        let audio = real && (rec.mic_path.is_some() || rec.system_path.is_some());
        // «Más» abre el menú ⋯ de la cabecera (renombrar, carpeta, eliminar…).
        let more = real;
        if !audio && !more {
            return None;
        }
        Some(
            div()
                .flex()
                .items_center()
                .gap(px(2.))
                .when(audio, |el| {
                    el.child(quick_button(
                        ElementId::NamedInteger("meeting-quick-play".into(), ix as u64),
                        "icons/play.svg",
                        "Reproducir",
                        cx.listener(move |v, _: &ClickEvent, _, cx| {
                            cx.stop_propagation();
                            v.play_row(ix, cx);
                        }),
                    ))
                })
                .when(more, |el| {
                    el.child(quick_button(
                        ElementId::NamedInteger("meeting-quick-more".into(), ix as u64),
                        "icons/ellipsis.svg",
                        "Más · Ctrl+.",
                        cx.listener(move |v, _: &ClickEvent, _, cx| {
                            cx.stop_propagation();
                            v.open_more_menu(Some(ix), false, cx);
                        }),
                    ))
                }),
        )
    }

    /// Termina una fila: fondo con transición, la línea de abajo (`meta`)
    /// cruzándose con las acciones y, si toca, el vistazo al lado.
    pub(super) fn row_reveal(
        &self,
        ix: usize,
        rec: &Recording,
        on: bool,
        row: gpui::Stateful<gpui::Div>,
        meta: gpui::Div,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let actions = self.row_actions(ix, rec, cx);
        let peek = (!on && self.row_peeks.shown.as_deref() == Some(rec.id.as_str()))
            .then(|| self.peek_card(ix, rec));
        let (rest, over) = if on { (SURFACE_ON, SURFACE_ON) } else { (SURFACE, SURFACE_HOVER) };
        let id = rec.id.clone();
        row.relative()
            .on_hover(cx.listener(move |v, hovered: &bool, _, cx| v.row_hover(&id, *hovered, cx)))
            .fx(ElementId::NamedInteger("meeting-fx".into(), ix as u64), move |el, h| {
                let (rest, over) = (hsla(rest), hsla(over));
                let mut pressed = over;
                pressed.a = (over.a + 0.06).min(1.0);
                let bg = mix(mix(rest, over, h.t), pressed, h.press);
                // Lo que se va se corre un poco a la izquierda y lo que llega
                // entra desde la derecha: se leen como un solo gesto.
                let line = match actions {
                    Some(actions) => div()
                        .relative()
                        .child(meta.opacity(1.0 - h.over).ml(px(-4. * h.over)))
                        .child(
                            div()
                                .absolute()
                                .right(px(-6. + -4. * (1.0 - h.over)))
                                .top(px(-6.))
                                .bottom(px(-6.))
                                .flex()
                                .items_center()
                                .opacity(h.over)
                                .when(h.over < 0.05, |el| el.invisible())
                                .child(actions),
                        ),
                    None => div().child(meta),
                };
                el.bg(bg).child(line).children(peek)
            })
            .into_any_element()
    }

    /// La tarjeta del vistazo, anclada al borde derecho de la fila y pintada
    /// encima de todo (sin el recorte del scroll de la lista).
    fn peek_card(&self, ix: usize, rec: &Recording) -> AnyElement {
        let today = Local::now().date_naive();
        let local = rec.started_at.with_timezone(&Local);
        let when = format!(
            "{} · {} · {}",
            text::full_date(local.date_naive(), today),
            local.format("%H:%M"),
            text::duration(rec.duration_secs)
        );
        let (label, body, quiet): (Option<&str>, Option<String>, bool) =
            match self.row_peeks.cache.get(&rec.id) {
                Some(Preview::Summary(t)) => (Some("Resumen"), Some(t.clone()), false),
                Some(Preview::Transcript(t)) => (Some("Transcripción"), Some(t.clone()), false),
                Some(Preview::Silent) => (None, Some("No se entendió nada".into()), true),
                Some(Preview::Nothing) => (None, Some("Sin transcribir".into()), true),
                // Aún leyendo: título y fecha; el texto llega enseguida.
                None => (None, None, false),
            };
        let card = div()
            .w(px(PEEK_W))
            .p(px(16.))
            .rounded(px(R_CARD))
            .bg(hsla(PEEK))
            .shadow_lg()
            .flex()
            .flex_col()
            .gap(px(3.))
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(19.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(hsla(TEXT))
                    .line_clamp(2)
                    .text_ellipsis()
                    .child(SharedString::from(rec.title.clone())),
            )
            .child(div().text_size(px(12.)).text_color(hsla(MUTED)).child(when))
            .when_some(body, |el, body| {
                el.child(
                    div()
                        .pt(px(9.))
                        .flex()
                        .flex_col()
                        .gap(px(3.))
                        .when_some(label, |el, label| {
                            el.child(
                                div()
                                    .text_size(px(11.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(hsla(FAINT))
                                    .child(label),
                            )
                        })
                        .child(
                            div()
                                .text_size(px(13.))
                                .line_height(px(20.))
                                .text_color(hsla(if quiet { MUTED } else { BODY }))
                                .line_clamp(3)
                                .text_ellipsis()
                                .child(body),
                        ),
                )
            })
            .with_animation(
                ElementId::NamedInteger("meeting-peek-in".into(), ix as u64),
                Animation::new(hover::ENTER).with_easing(gpui::ease_out_quint()),
                |el, t| el.opacity(t).ml(px(6. * (1. - t))),
            );
        // Un punto sin tamaño en la esquina de arriba a la derecha de la fila;
        // desde ahí se ancla la tarjeta, que no se sale de la ventana.
        div()
            .absolute()
            .top_0()
            .right_0()
            .child(
                deferred(
                    anchored()
                        .anchor(Corner::TopLeft)
                        .offset(point(px(PEEK_GAP), px(-4.)))
                        .snap_to_window_with_margin(px(16.))
                        .child(card),
                )
                .with_priority(1),
            )
            .into_any_element()
    }
}

/// El fondo de la tarjeta: un escalón sobre el panel de detalle que tapa.
const PEEK: u32 = 0x2a2a27;

fn quick_button(
    id: ElementId,
    icon: &'static str,
    tip: &'static str,
    on_click: impl Fn(&ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    let fx_id = ElementId::Name(format!("{id}-fx").into());
    hover::hover_fx(fx_id, move |h| {
        div()
            .id(id)
            .size(px(26.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(13.))
            .cursor_pointer()
            .bg(hsla(TEXT).opacity(0.10 * h.t + 0.04 * h.press))
            .tooltip(hover::tip(tip))
            .on_click(on_click)
            .child(svg().path(icon).size(px(13.)).text_color(h.mix(hsla(MUTED), hsla(TEXT))))
            .into_any_element()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::summary::Item;

    fn section(kind: Kind, blocks: Vec<SummaryBlock>) -> Section {
        Section { title: String::new(), kind, blocks }
    }

    fn block(text: &str) -> Block {
        Block { me: false, label: "Otra persona".into(), start_ms: 0, text: text.into() }
    }

    #[test]
    fn prefiere_el_primer_parrafo_del_resumen() {
        let sections = vec![
            section(Kind::Topics, vec![SummaryBlock::Paragraph("Temas".into())]),
            section(
                Kind::Summary,
                vec![
                    SummaryBlock::Paragraph("  ".into()),
                    SummaryBlock::Paragraph("Se acordó el plan.".into()),
                    SummaryBlock::Paragraph("Otro".into()),
                ],
            ),
        ];
        assert_eq!(pick(&sections, &[block("hola")], true), Preview::Summary("Se acordó el plan.".into()));
    }

    #[test]
    fn un_resumen_de_puras_listas_muestra_sus_puntos() {
        let items = ["Uno", "Dos", "Tres", "Cuatro"]
            .map(|t| Item { text: t.into(), checked: None })
            .to_vec();
        let sections = vec![section(Kind::Tasks, vec![SummaryBlock::List { ordered: false, items }])];
        assert_eq!(pick(&sections, &[], false), Preview::Summary("Uno · Dos · Tres".into()));
    }

    #[test]
    fn sin_resumen_va_la_transcripcion() {
        let blocks = [block("Hola a todos."), block("  Partamos.")];
        assert_eq!(pick(&[], &blocks, true), Preview::Transcript("Hola a todos. Partamos.".into()));
        assert_eq!(pick(&[], &[], true), Preview::Silent);
        assert_eq!(pick(&[], &[], false), Preview::Nothing);
    }

    #[test]
    fn corta_en_una_palabra_y_sin_saltos() {
        assert_eq!(clip("una\n\ndos   tres", 50), "una dos tres");
        let long = "palabra ".repeat(40);
        let out = clip(&long, 30);
        assert!(out.ends_with('…'));
        assert!(out.chars().count() <= 31);
        assert!(!out.contains("palabr…"));
        // Sin espacios cerca del corte, corta donde cae (con tildes, sin
        // partir un carácter).
        assert_eq!(clip(&"á".repeat(10), 4), "áááá…");
    }
}
