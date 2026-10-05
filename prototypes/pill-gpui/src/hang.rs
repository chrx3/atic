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
    div, prelude::*, px, Animation, AnimationExt, AnyElement, FontWeight,
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
const CURRENT_H: f32 = 40.0;
const CURRENT_H_SIDE: f32 = 58.0;

/// El alto de la banda: bajo el tab arriba, al lado en un costado.
pub(crate) fn hang_height(side: bool) -> f32 {
    if side {
        HANG_H_SIDE
    } else {
        HANG_H
    }
}

/// El verso que suena: (índice, actual, siguiente).
type Verse = (Option<usize>, String, String);

impl Pill {
    fn hang_verse(&self) -> Option<Verse> {
        if std::env::var("PILL_LYRICS").is_ok_and(|v| v == "0") || !self.media.show_lyrics() {
            return None;
        }
        let lines = self.media.lyrics()?;
        let (position, _) = self.media.track()?.position_now()?;
        let i = lyrics::index(&lines, position);
        let text = |at: usize| lines.get(at).map(|l| l.text.clone()).unwrap_or_default();
        Some((i, i.map(text).unwrap_or_default(), text(i.map_or(0, |i| i + 1))))
    }

    /// Cuelga la letra si el tab está en reposo con música que la trae.
    pub(crate) fn update_hang(&mut self, now: Instant) {
        let quiet = self.docked_still()
            && self.live.target() == 1.0
            && self.strip.target() == 0.0
            && self.peek.target() == 0.0
            && self.tray.amount(now) <= 0.01
            && !self.dictation_shown();
        let on = quiet && self.hang_verse().is_some();
        self.hang.set(if on { 1.0 } else { 0.0 }, now);
    }

    /// Cuánto cuelga la letra (0 a 1).
    pub(crate) fn hang_amount(&self, now: Instant) -> f32 {
        self.hang.value(now).clamp(0.0, 1.2)
    }

    /// El largo del tab con la letra: al menos `HANG_W`.
    pub(crate) fn hang_length(length: f32, amount: f32) -> f32 {
        length + (HANG_W.max(length) - length) * amount
    }

    /// El cursor está sobre la letra (bajo la franja). Como las filas del
    /// vistazo, no abre la tira: la tira recogería la letra, el tab se
    /// achicaría bajo el cursor y la tira se cerraría, en bucle.
    pub(crate) fn over_hang(&self, p: (f32, f32), now: Instant) -> bool {
        if self.hang.value(now) <= 0.05 {
            return false;
        }
        self.drawer_rect(crate::usage::Drawer::Lyrics, now).is_some_and(|r| r.contains(p, 0.0))
    }

    /// Los versos, bajo la franja del tab estirado.
    pub(crate) fn hang_element(&self, now: Instant) -> Option<AnyElement> {
        let amount = self.hang.value(now);
        if amount <= 0.05 {
            return None;
        }
        let area = self.drawer_rect(crate::usage::Drawer::Lyrics, now)?;
        let (index, now_text, next) = self.hang_verse()?;
        let key = index.map_or(0, |i| i + 1);
        let text = crate::Palette::dark().text;
        let side = self.side_drawers();
        let current = div()
            .id(SharedString::from(format!("hang-lyric-{key}")))
            .relative()
            .h(px(if side { CURRENT_H_SIDE } else { CURRENT_H }))
            .w_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .text_center()
                    .line_clamp(if side { 3 } else { 2 })
                    .text_size(px(14.5))
                    .line_height(px(18.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(text)
                    .child(SharedString::from(if now_text.trim().is_empty() { "♪".into() } else { now_text })),
            )
            .with_animation(
                SharedString::from(format!("hang-lyric-in-{key}")),
                Animation::new(Duration::from_millis(220)).with_easing(gpui::ease_out_quint()),
                |el, t| el.opacity(0.32 + 0.68 * t).top(px(7.0 * (1.0 - t))),
            );
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
                .flex()
                .flex_col()
                .opacity(segment(amount, 0.5, 0.5))
                .font_family("Segoe UI")
                .child(current)
                .child(
                    div()
                        .h(px(18.))
                        .w_full()
                        .text_center()
                        .truncate()
                        .text_size(px(12.))
                        .text_color(text.opacity(0.34))
                        .child(SharedString::from(next)),
                )
                .into_any_element(),
        )
    }
}
