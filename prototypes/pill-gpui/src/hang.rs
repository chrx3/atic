//! La letra que cuelga del notch cerrado, como en la pill de Atic
//! (`lyricHang` en `PillSurface.svelte`): con algo sonando que trae letra
//! sincronizada, el tab se ensancha y baja una franja con el verso que suena
//! (grande, hasta dos renglones) y el siguiente apagado.
//!
//! Con el tab acoplado (a cualquier borde) y en reposo: la tira, el vistazo
//! del portapapeles o el de la bandeja la recogen. Arriba cuelga bajo la
//! franja; en un costado va como una banda al lado del tab. Un clic abre Ahora suena (es
//! parte de la carátula: `over_art`). Se apaga desde Ahora suena (botón
//! «Letra», que también oculta los versos del panel) o con `PILL_LYRICS=0`.

use std::time::{Duration, Instant};

use gpui::{
    div, prelude::*, px, Animation, AnimationExt, AnyElement, FontWeight, Hsla,
    SharedString,
};

use crate::anim::segment;
use crate::media::lyrics;
use crate::Pill;

/// El ancho del tab con la letra y el alto que gana bajo la franja.
pub(crate) const HANG_W: f32 = 340.0;
pub(crate) const HANG_H: f32 = 74.0;
/// En un costado la banda es angosta (`SIDE_W`): el verso entra en hasta tres
/// renglones.
const HANG_H_SIDE: f32 = 98.0;
pub(crate) const CURRENT_H: f32 = 40.0;
const CURRENT_H_SIDE: f32 = 58.0;

/// El alto del verso que suena, de la letra que cuelga (`grow` 0) al de los
/// versos del panel (1).
pub(crate) fn current_h(grow: f32) -> f32 {
    CURRENT_H + (crate::media::LYRIC_NOW_H - CURRENT_H) * grow.clamp(0.0, 1.0)
}

/// El alto de la banda: bajo el tab arriba, al lado en un costado.
pub(crate) fn hang_height(side: bool) -> f32 {
    if side {
        HANG_H_SIDE
    } else {
        HANG_H
    }
}

/// Lo que dura el paso de un verso al siguiente (`verse_carousel`).
const CAROUSEL_MS: f32 = 560.0;

/// El verso que suena en un momento de la pista y sus vecinos.
pub(crate) struct Verses {
    /// `None` antes del primer verso.
    pub index: Option<usize>,
    /// Segundos desde que empezó el verso: el carrusel sigue a la pista, no
    /// a cuándo se dibujó (abrir una vista a mitad de verso no lo anima).
    pub since: f32,
    pub prev: String,
    pub now: String,
    pub next: String,
}

/// Los versos de `lines` en `position` (segundos).
pub(crate) fn verses(lines: &[lyrics::Line], position: f32) -> Verses {
    let index = lyrics::index(lines, position);
    let text = |at: Option<usize>| at.and_then(|at| lines.get(at)).map(|l| l.text.clone()).unwrap_or_default();
    Verses {
        index,
        since: index.and_then(|i| lines.get(i)).map_or(f32::MAX, |l| position - l.at),
        prev: text(index.and_then(|i| i.checked_sub(1))),
        now: text(index),
        next: text(Some(index.map_or(0, |i| i + 1))),
    }
}

/// Cómo se ven los versos en cada lugar: la letra que cuelga, el vistazo y
/// el panel de Ahora suena.
#[derive(Clone, Copy)]
pub(crate) struct VerseLook {
    pub now_size: f32,
    pub now_line: f32,
    pub now_h: f32,
    pub now_color: Hsla,
    pub next_size: f32,
    pub next_h: f32,
    pub next_color: Hsla,
    /// Renglones del verso que suena.
    pub clamp: usize,
}

/// El verso que suena y el siguiente, como un carrusel: al cambiar de verso
/// todo sube un renglón; el que sonaba se apaga arriba, el siguiente crece
/// hasta su lugar y el nuevo siguiente entra por abajo. `id` separa las
/// animaciones de cada lugar.
pub(crate) fn verse_carousel(id: &str, verses: &Verses, look: VerseLook) -> AnyElement {
    let key = verses.index.map_or(0, |i| i + 1);
    // Lo que ya pasó del cambio: si el verso empezó hace rato, en reposo.
    let start = (verses.since * 1000.0 / CAROUSEL_MS).clamp(0.0, 1.0);
    let ease = move |t: f32| gpui::ease_in_out(start + (1.0 - start) * t);
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let mix = move |a: Hsla, b: Hsla, t: f32| Hsla { h: lerp(a.h, b.h, t), s: lerp(a.s, b.s, t), l: lerp(a.l, b.l, t), a: lerp(a.a, b.a, t) };
    let animation = || Animation::new(Duration::from_millis(CAROUSEL_MS as u64));
    let name = |part: &str| SharedString::from(format!("{id}-verse-{part}-{key}"));
    // Sin recortar: el que crece empieza con el alto del siguiente (18) y un
    // verso de dos líneas se cortaba mientras subía. Solo recorta el bloque.
    let slot = |text: String, clamp: usize| {
        let line = div().w_full().text_center().line_clamp(clamp);
        // El «♪» de entre versos, centrado como elemento: con `text_center`
        // solo quedaba pegado a la izquierda. El texto no: en una fila flex
        // no se parte en renglones y se cortaba a los lados.
        let line = if text == "♪" {
            line.flex().justify_center().child(SharedString::from(text))
        } else {
            line.child(SharedString::from(text))
        };
        div().w_full().flex().items_center().justify_center().child(line)
    };
    let shown = |text: &str| if text.trim().is_empty() { "♪".to_string() } else { text.to_string() };
    let next_line = look.next_size * 1.3;

    let gone = slot(verses.prev.clone(), look.clamp)
        .h(px(look.now_h))
        .text_size(px(look.now_size))
        .line_height(px(look.now_line))
        .font_weight(FontWeight::BOLD)
        .text_color(look.now_color)
        .with_animation(name("gone"), animation(), move |el, t| el.opacity(1.0 - ease(t)));
    let now = slot(shown(&verses.now), look.clamp).with_animation(name("now"), animation(), move |el, t| {
        let e = ease(t);
        el.h(px(lerp(look.next_h, look.now_h, e)))
            .text_size(px(lerp(look.next_size, look.now_size, e)))
            .line_height(px(lerp(next_line, look.now_line, e)))
            .font_weight(FontWeight(lerp(FontWeight::NORMAL.0, FontWeight::BOLD.0, e)))
            .text_color(mix(look.next_color, look.now_color, e))
    });
    let next = slot(verses.next.clone(), 1)
        .h(px(look.next_h))
        .text_size(px(look.next_size))
        .line_height(px(next_line))
        .text_color(look.next_color)
        .with_animation(name("next"), animation(), move |el, t| el.opacity(ease(t)));
    div()
        .relative()
        .w_full()
        .h(px(look.now_h + look.next_h))
        .overflow_hidden()
        .child(
            div()
                .relative()
                .w_full()
                .flex()
                .flex_col()
                .child(gone)
                .child(now)
                .child(next)
                .with_animation(name("roll"), animation(), move |el, t| el.top(px(-look.now_h * ease(t)))),
        )
        .into_any_element()
}

impl Pill {
    pub(crate) fn hang_verse(&self) -> Option<Verses> {
        if std::env::var("PILL_LYRICS").is_ok_and(|v| v == "0") || !self.media.show_lyrics() {
            return None;
        }
        let lines = self.media.lyrics()?;
        let (position, _) = self.media.track()?.position_now()?;
        Some(verses(&lines, position))
    }

    /// Cuelga la letra si el tab está en reposo con música que la trae.
    pub(crate) fn update_hang(&mut self, now: Instant) {
        let quiet = self.docked_still()
            && self.live.target() == 1.0
            // En pausa o grabando, la letra no tiene qué seguir.
            && self.music_playing
            && self.rec_clock.is_none()
            && self.strip.target() == 0.0
            && self.peek.target() == 0.0
            // El de Ahora suena no: la letra cede lo que él baja
            // (`hang_amount`) y vuelve a la par cuando sube, sin hueco.
            && (!self.tool_peek_down() || self.media_peek_shown())
            && self.tray.amount(now) <= 0.01
            && !self.dictation_shown();
        let on = quiet && self.hang_verse().is_some();
        self.hang.set(if on { 1.0 } else { 0.0 }, now);
    }

    /// Cuánto cuelga la letra (0 a 1): lo que no ocupa el vistazo de Ahora
    /// suena, que trae los mismos versos.
    pub(crate) fn hang_amount(&self, now: Instant) -> f32 {
        self.hang.value(now).clamp(0.0, 1.2) * (1.0 - self.tool_peek_media_amount(now))
    }

    /// El largo del tab con la letra: al menos `HANG_W`.
    pub(crate) fn hang_length(length: f32, amount: f32) -> f32 {
        length + (HANG_W.max(length) - length) * amount
    }

    /// El cursor está sobre la letra (bajo la franja). Como las filas del
    /// vistazo, no abre la tira: la tira recogería la letra, el tab se
    /// achicaría bajo el cursor y la tira se cerraría, en bucle.
    pub(crate) fn over_hang(&self, p: (f32, f32), now: Instant) -> bool {
        if self.hang_amount(now) <= 0.05 {
            return false;
        }
        self.drawer_rect(crate::usage::Drawer::Lyrics, now).is_some_and(|r| r.contains(p, 0.0))
    }

    /// Los versos, bajo la franja del tab estirado.
    pub(crate) fn hang_element(&self, now: Instant) -> Option<AnyElement> {
        let amount = self.hang.value(now);
        // Con el vistazo de Ahora suena, la letra no se apaga ni se encoge:
        // viaja hasta el lugar de sus versos (`hang_rides_media_peek`).
        let ride = self.hang_rides_media_peek(now);
        let media = if ride.is_some() { 0.0 } else { self.tool_peek_media_amount(now) };
        if amount * (1.0 - media) <= 0.05 {
            return None;
        }
        let mut area = match ride {
            Some(y) => {
                let mut area = self.drawers_area(now)?;
                area.y = y;
                area.h = HANG_H;
                area
            }
            None => self.drawer_rect(crate::usage::Drawer::Lyrics, now)?,
        };
        // Mientras el notch se cierra, la letra va pegada a su borde de abajo
        // (donde estaban los versos del panel) y sube con él hasta colgar.
        if self.panel_visible() {
            if let crate::PillShape::Tab { rect, .. } = self.shape(now) {
                area.y = area.y.max(rect.y + rect.h - area.h);
            }
        }
        let side = self.side_drawers();
        let lines = self.verse_lines(side, "hang", 0.0)?;
        // Cerrándose el notch, aparece a medida que se va el panel (que
        // tiene sus propios versos): si no, se ven dos letras encima.
        let fade = if self.panel_visible() { 1.0 - self.panel_content_amount() } else { 1.0 };
        // Sin fondo propio: el tab entero se vuelve casi opaco (`CONTENT_TINT`).
        Some(
            div()
                .absolute()
                .left(px(area.x))
                .top(px(area.y))
                .w(px(area.w))
                .h(px(area.h))
                .overflow_hidden()
                .px(px(18.))
                .when(side, |el| el.pt(px(10.)))
                // Frente al vistazo de Ahora suena, se funde parejo con él.
                .opacity(segment(amount, 0.5, 0.5) * (1.0 - media) * fade)
                .font_family(crate::fonts::UI)
                .child(lines)
                .into_any_element(),
        )
    }

    /// El verso que suena y el siguiente (`verse_carousel`). Lo usan la letra
    /// que cuelga y el vistazo de Ahora suena; `id` separa sus animaciones.
    /// `grow` (0 a 1) lo lleva al tamaño de los versos del panel
    /// (`MediaPanel::render_lyrics`): el vistazo crece mientras se transforma
    /// en él, así al cruzarse las dos letras coinciden.
    pub(crate) fn verse_lines(&self, side: bool, id: &'static str, grow: f32) -> Option<AnyElement> {
        let lerp = |a: f32, b: f32| a + (b - a) * grow.clamp(0.0, 1.0);
        let verses = self.hang_verse()?;
        let text = crate::Palette::dark().text;
        let look = VerseLook {
            now_size: lerp(14.5, 17.),
            now_line: lerp(18., 21.),
            now_h: if side { CURRENT_H_SIDE } else { current_h(grow) },
            now_color: text,
            next_size: lerp(12., 12.5),
            next_h: 18.,
            next_color: text.opacity(lerp(0.34, 0.32)),
            clamp: if side { 3 } else { 2 },
        };
        Some(verse_carousel(id, &verses, look))
    }
}
