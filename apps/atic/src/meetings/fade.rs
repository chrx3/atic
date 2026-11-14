//! El fundido de lo que llega en vivo: el resumen mientras se escribe y los subtítulos de
//! la grabación. Cada trozo entra en `FADE_SECS` con una curva que parte transparente y
//! frena al final, como el texto de Atic Code (`code/chat.rs`, `stream_fades`).
//!
//! La opacidad sale del instante en que llegó cada trozo, no de un reloj de los elementos:
//! se redibuja con lo que llega y, después del último trozo, unos ticks más en la grilla
//! compartida (`motion::tick_delay`) hasta que termina (`MeetingsView::fade_tick`). Nada
//! pide cuadros propios ni sigue andando en reposo.

use std::ops::Range;
use std::time::Instant;

use gpui::{AnyElement, HighlightStyle, IntoElement, SharedString, StyledText};

use super::summary::{Block, Section};

/// Lo que tarda en entrar un trozo: ~9 cuadros a 15 por segundo, para que se vea.
pub(super) const FADE_SECS: f32 = 0.6;

/// La opacidad de un trozo a los `t` segundos de llegar: sale rápido desde 0 y se posa.
pub(super) fn fade_at(t: f32) -> f32 {
    let x = (t / FADE_SECS).clamp(0., 1.);
    1. - (1. - x).powi(3)
}

/// Los trozos que llegaron a un texto que crece: desde qué byte y cuándo.
#[derive(Clone, Debug, Default)]
pub(super) struct Arrivals {
    list: Vec<(usize, Instant)>,
    len: usize,
}

impl Arrivals {
    /// El texto mide ahora `len`: lo que pasa del largo anterior llegó a la hora `now`. Si
    /// se achicó (el modelo rehízo la última línea), lo que se fue deja de contar.
    pub(super) fn grow(&mut self, len: usize, now: Instant) {
        self.list
            .retain(|&(start, at)| start < len && now.saturating_duration_since(at).as_secs_f32() < FADE_SECS);
        if len > self.len {
            self.list.push((self.len, now));
        }
        self.len = len;
    }

    /// Los trozos que todavía entran: (byte donde empiezan, opacidad ahora). Cada uno llega
    /// hasta donde empieza el siguiente.
    pub(super) fn chunks(&self, now: Instant) -> Vec<(usize, f32)> {
        self.list
            .iter()
            .filter_map(|&(start, at)| {
                let t = now.saturating_duration_since(at).as_secs_f32();
                (t < FADE_SECS).then(|| (start, fade_at(t)))
            })
            .collect()
    }

    pub(super) fn fading(&self, now: Instant) -> bool {
        self.list.iter().any(|&(_, at)| now.saturating_duration_since(at).as_secs_f32() < FADE_SECS)
    }
}

/// Lo que cae de los trozos `chunks` (inicio y opacidad, en orden) en el texto que ocupa
/// `leaf` del total, en bytes relativos al texto. `skip`: un trozo que ya se funde entero
/// por otro lado (la tarjeta nueva que lo trae).
pub(super) fn ranges(chunks: &[(usize, f32)], leaf: Range<usize>, skip: Option<usize>) -> Vec<(Range<usize>, f32)> {
    let mut out = Vec::new();
    for (ix, &(start, opacity)) in chunks.iter().enumerate() {
        let end = chunks.get(ix + 1).map_or(usize::MAX, |next| next.0);
        let (from, to) = (start.max(leaf.start), end.min(leaf.end));
        if Some(ix) == skip || from >= to || opacity >= 1. {
            continue;
        }
        out.push((from - leaf.start..to - leaf.start, opacity));
    }
    out
}

/// `ix` llevado al inicio del carácter que lo contiene (los trozos se cuentan en bytes y el
/// texto se puede haber rehecho).
fn floor_char(text: &str, ix: usize) -> usize {
    let mut ix = ix.min(text.len());
    while !text.is_char_boundary(ix) {
        ix -= 1;
    }
    ix
}

/// `text` con los rangos de `ranges` atenuados a su opacidad.
pub(super) fn faded_text(text: SharedString, ranges: &[(Range<usize>, f32)]) -> AnyElement {
    if ranges.is_empty() {
        return text.into_any_element();
    }
    let highlights: Vec<(Range<usize>, HighlightStyle)> = ranges
        .iter()
        .filter_map(|(range, opacity)| {
            let (start, end) = (floor_char(&text, range.start), floor_char(&text, range.end));
            (start < end).then(|| (start..end, HighlightStyle { fade_out: Some(1. - opacity), ..Default::default() }))
        })
        .collect();
    StyledText::new(text).with_highlights(highlights).into_any_element()
}

/// El largo del resumen tal como se dibuja: el título de cada sección y después sus
/// párrafos y puntos, en ese orden (el mismo de `Tail` en `section_card`).
pub(super) fn summary_len(sections: &[Section]) -> usize {
    sections
        .iter()
        .map(|section| {
            section.title.len()
                + section
                    .blocks
                    .iter()
                    .map(|block| match block {
                        Block::Paragraph(text) => text.len(),
                        Block::List { items, .. } => items.iter().map(|item| item.text.len()).sum(),
                    })
                    .sum::<usize>()
        })
        .sum()
}

/// Recorre los textos de un resumen en orden y dice qué parte de cada uno todavía entra.
pub(super) struct Tail {
    chunks: Vec<(usize, f32)>,
    at: usize,
    skip: Option<usize>,
}

impl Tail {
    pub(super) fn new(chunks: Vec<(usize, f32)>) -> Self {
        Self { chunks, at: 0, skip: None }
    }

    /// Sin nada que entre: el resumen guardado.
    pub(super) fn still() -> Self {
        Self::new(Vec::new())
    }

    /// Empieza una tarjeta: si su título llegó en un trozo que todavía entra, la tarjeta
    /// entera se funde con él (su fondo no aparece de golpe) y ese trozo no se vuelve a
    /// atenuar adentro. Devuelve la opacidad de la tarjeta.
    pub(super) fn card(&mut self) -> f32 {
        self.skip = None;
        let at = self.at;
        let found = self.chunks.iter().enumerate().find(|&(ix, &(start, _))| {
            let end = self.chunks.get(ix + 1).map_or(usize::MAX, |next| next.0);
            start <= at && at < end
        });
        match found {
            Some((ix, &(_, opacity))) => {
                self.skip = Some(ix);
                opacity
            }
            None => 1.,
        }
    }

    /// Los rangos que todavía entran del texto siguiente (y avanza por él).
    pub(super) fn text(&mut self, text: &str) -> Vec<(Range<usize>, f32)> {
        let leaf = self.at..self.at + text.len();
        self.at = leaf.end;
        if self.chunks.is_empty() {
            return Vec::new();
        }
        ranges(&self.chunks, leaf, self.skip)
    }
}

/// Los subtítulos de la grabación: cada línea con lo que le llegó. Las líneas se reconocen
/// por su texto, así que una que sube (otra entra abajo) o que pasa de parcial a firme no
/// vuelve a fundirse; la parcial que crece funde solo lo nuevo.
#[derive(Default)]
pub(super) struct Lines {
    shown: Vec<(String, Arrivals)>,
}

impl Lines {
    /// Anota las líneas que se ven ahora (`texts`, de arriba abajo).
    pub(super) fn observe(&mut self, texts: &[&str], now: Instant) {
        if self.shown.len() == texts.len() && self.shown.iter().zip(texts).all(|((shown, _), text)| shown == text) {
            return;
        }
        let mut old: Vec<Option<(String, Arrivals)>> = std::mem::take(&mut self.shown).into_iter().map(Some).collect();
        // La última de antes es la que pudo crecer o pasar a firme.
        let last = old.len().checked_sub(1);
        for text in texts {
            let same = old.iter().position(|o| o.as_ref().is_some_and(|(shown, _)| shown == text));
            let arrivals = match same {
                Some(ix) => {
                    let (_, mut arrivals) = old[ix].take().unwrap_or_default();
                    arrivals.grow(text.len(), now);
                    arrivals
                }
                None => match last.and_then(|ix| old[ix].take().map(|o| (ix, o))) {
                    Some((ix, (shown, mut arrivals))) => {
                        let common = shown.char_indices().zip(text.char_indices()).take_while(|((_, a), (_, b))| a == b).count();
                        let prefix = shown.char_indices().nth(common).map_or(shown.len(), |(at, _)| at);
                        if prefix == 0 {
                            // No tiene que ver: la de antes queda para otra línea.
                            old[ix] = Some((shown, arrivals));
                            arrivals = Arrivals::default();
                        } else {
                            arrivals.grow(prefix, now);
                        }
                        arrivals.grow(text.len(), now);
                        arrivals
                    }
                    None => {
                        let mut arrivals = Arrivals::default();
                        arrivals.grow(text.len(), now);
                        arrivals
                    }
                },
            };
            self.shown.push((text.to_string(), arrivals));
        }
    }

    /// Lo que todavía entra de la línea `ix`, en rangos de su texto.
    pub(super) fn ranges(&self, ix: usize, now: Instant) -> Vec<(Range<usize>, f32)> {
        let Some((text, arrivals)) = self.shown.get(ix) else {
            return Vec::new();
        };
        ranges(&arrivals.chunks(now), 0..text.len(), None)
    }

    pub(super) fn fading(&self, now: Instant) -> bool {
        self.shown.iter().any(|(_, arrivals)| arrivals.fading(now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn la_curva_parte_transparente_y_se_ve_entrar() {
        assert_eq!(fade_at(0.), 0.);
        assert!(fade_at(FADE_SECS / 2.) < 0.95);
        assert_eq!(fade_at(FADE_SECS), 1.);
    }

    #[test]
    fn lo_que_llega_entra_y_despues_queda_quieto() {
        let t0 = Instant::now();
        let mut arrivals = Arrivals::default();
        arrivals.grow(4, t0);
        arrivals.grow(10, t0 + ms(200));
        let chunks = arrivals.chunks(t0 + ms(200));
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].0, 0);
        assert!(chunks[0].1 > 0.5 && chunks[0].1 < 1.);
        assert_eq!(chunks[1], (4, 0.));
        assert!(arrivals.fading(t0 + ms(700)));
        // Pasado el fundido del último, nada se anima ni pide cuadros.
        assert!(arrivals.chunks(t0 + ms(850)).is_empty());
        assert!(!arrivals.fading(t0 + ms(850)));
    }

    #[test]
    fn si_el_texto_se_achica_lo_que_se_fue_no_cuenta() {
        let t0 = Instant::now();
        let mut arrivals = Arrivals::default();
        arrivals.grow(10, t0);
        arrivals.grow(20, t0 + ms(50));
        arrivals.grow(8, t0 + ms(60));
        arrivals.grow(12, t0 + ms(70));
        let starts: Vec<usize> = arrivals.chunks(t0 + ms(70)).iter().map(|c| c.0).collect();
        assert_eq!(starts, vec![0, 8]);
    }

    #[test]
    fn cada_texto_recibe_su_parte_de_los_trozos() {
        // Un trozo desde 0 (casi entero) y otro recién llegado desde 7.
        let chunks = [(0, 0.9), (7, 0.)];
        // Un texto de 0 a 5: solo el primero.
        assert_eq!(ranges(&chunks, 0..5, None), vec![(0..5, 0.9)]);
        // Uno de 5 a 12: el final del primero y el comienzo del segundo.
        assert_eq!(ranges(&chunks, 5..12, None), vec![(0..2, 0.9), (2..7, 0.)]);
        // Saltando el primero (su tarjeta ya se funde entera).
        assert_eq!(ranges(&chunks, 5..12, Some(0)), vec![(2..7, 0.)]);
        // Los que ya terminaron no se dibujan.
        assert!(ranges(&[(0, 1.)], 0..5, None).is_empty());
    }

    #[test]
    fn una_tarjeta_nueva_se_funde_entera() {
        // «Resumen» + «Hola» ya estaban; «Tareas» llegó recién.
        let mut tail = Tail::new(vec![(11, 0.2)]);
        assert_eq!(tail.card(), 1.);
        assert!(tail.text("Resumen").is_empty());
        assert!(tail.text("Hola").is_empty());
        assert_eq!(tail.card(), 0.2);
        // Su título no se atenúa dos veces.
        assert!(tail.text("Tareas").is_empty());
        let mut still = Tail::still();
        assert_eq!(still.card(), 1.);
        assert!(still.text("Hola").is_empty());
    }

    #[test]
    fn el_largo_del_resumen_cuenta_titulos_y_puntos() {
        let sections = vec![Section {
            title: "Resumen".into(),
            kind: super::super::summary::Kind::Summary,
            blocks: vec![
                Block::Paragraph("Hola".into()),
                Block::List { ordered: false, items: vec![super::super::summary::Item { text: "uno".into(), checked: None }] },
            ],
        }];
        assert_eq!(summary_len(&sections), 7 + 4 + 3);
    }

    #[test]
    fn las_lineas_no_se_funden_de_nuevo_al_subir_o_quedar_firmes() {
        let t0 = Instant::now();
        let mut lines = Lines::default();
        lines.observe(&["Hola"], t0);
        // La parcial crece: solo lo nuevo entra.
        lines.observe(&["Hola, cómo"], t0 + ms(700));
        assert_eq!(lines.ranges(0, t0 + ms(700)), vec![(4..11, 0.)]);
        // Pasa a firme y empieza otra parcial: la de arriba no vuelve a fundirse.
        lines.observe(&["Hola, cómo estás", "Bien"], t0 + ms(1400));
        assert_eq!(lines.ranges(0, t0 + ms(1400)), vec![(11..18, 0.)]);
        assert_eq!(lines.ranges(1, t0 + ms(1400)), vec![(0..4, 0.)]);
        // Sube una línea (la de arriba se va): las demás siguen como estaban.
        lines.observe(&["Bien"], t0 + ms(2100));
        assert!(lines.ranges(0, t0 + ms(2100)).is_empty());
        assert!(!lines.fading(t0 + ms(2100)));
    }

    #[test]
    fn el_corte_no_parte_un_caracter() {
        assert_eq!(floor_char("año", 2), 1);
        assert_eq!(floor_char("año", 9), 4);
    }
}
