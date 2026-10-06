//! Las secciones Pill y Portapapeles de la ventana de Ajustes.
//!
//! Lo que es de la pill (la letra, el orden de las herramientas, lo borrado en
//! el portapapeles) se cambia en la pill abierta, no en un archivo: ella lo
//! tiene en memoria y lo guarda sola. Lo que es de Atic va a `config.json`.

use std::path::PathBuf;

use atic_core::{AppDirs, Config};
use gpui::{div, prelude::*, px, App, ClickEvent, Context, Entity, SharedString, Window, WindowHandle};

use crate::dictation_settings::shortcut_label;
use crate::meetings::settings::save_config;
use crate::settings::{card, hsla, row, switch_row, text_button, MUTED};
use crate::Pill;

fn pill(cx: &App) -> Option<WindowHandle<Pill>> {
    cx.windows().into_iter().find_map(|w| w.downcast::<Pill>())
}

fn no_pill() -> impl IntoElement {
    card().child(
        div()
            .text_size(px(13.))
            .text_color(hsla(MUTED))
            .child("La pill no está abierta."),
    )
}

// --- Pill ----------------------------------------------------------------------------

pub struct PillPane;

impl Render for PillPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(handle) = pill(cx) else {
            return no_pill().into_any_element();
        };
        let Ok(lyrics) = handle.read(cx).map(|pill| pill.media.show_lyrics()) else {
            return no_pill().into_any_element();
        };
        card()
            .child(row(
                "Herramientas",
                "Cuáles se ven, cuáles van detrás de «Más» y cuáles no. Se ordenan en la pill, acoplada a un borde.",
                None,
                div().flex().child(text_button(
                    "pill-customize",
                    "Ordenar en la pill",
                    move |_: &ClickEvent, _, cx: &mut App| {
                        let _ = handle.update(cx, |pill, window, cx| pill.open_customize(window, cx));
                    },
                )),
            ))
            .child(switch_row(
                "pill-lyrics",
                "Letra de las canciones",
                "Cuelga del notch el verso que suena. También se cambia en Ahora suena.",
                lyrics,
                cx.listener(move |_, _: &ClickEvent, _, cx| {
                    let _ = handle.update(cx, |pill, _, cx| {
                        pill.media.set_show_lyrics(!lyrics);
                        cx.notify();
                    });
                    cx.notify();
                }),
            ))
            .into_any_element()
    }
}

// --- Portapapeles --------------------------------------------------------------------

pub struct ClipboardPane {
    path: PathBuf,
    cfg: Config,
}

/// La sección, o `None` si no se encuentra la carpeta de datos de Atic.
pub fn clipboard_pane(cx: &mut App) -> Option<Entity<ClipboardPane>> {
    let path = AppDirs::new().ok()?.config_path();
    Some(cx.new(|_| ClipboardPane { cfg: Config::load(&path), path }))
}

impl Render for ClipboardPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let on = self.cfg.clipboard_history;
        let handle = pill(cx);
        let hidden = handle
            .and_then(|handle| handle.read(cx).ok().map(|pill| pill.panel.read(cx).hidden_count()))
            .unwrap_or(0);

        card()
            .child(switch_row(
                "clip-history",
                "Guardar lo que copias",
                "Lo captura Atic y se salta lo que marcan los gestores de contraseñas. Atic aplica el cambio al reiniciarse.",
                on,
                cx.listener(move |pane, _: &ClickEvent, _, cx| {
                    pane.cfg = save_config(&pane.path, |cfg| cfg.clipboard_history = !on);
                    cx.notify();
                }),
            ))
            .child(row(
                "Atajo",
                "Por ahora se cambia en los Ajustes de Atic.",
                Some(shortcut_label(&self.cfg.clipboard_shortcut).into()),
                div(),
            ))
            .child(row(
                "Borradas en la pill",
                "Borrar aquí solo las oculta: el historial sigue siendo de Atic.",
                Some(SharedString::from(hidden.to_string())),
                div().flex().when(hidden > 0, |el| {
                    el.child(text_button(
                        "clip-unhide",
                        "Volver a mostrarlas",
                        cx.listener(move |_, _: &ClickEvent, _, cx| {
                            if let Some(handle) = handle {
                                let _ = handle.update(cx, |pill, _, cx| {
                                    pill.panel.update(cx, |panel, cx| panel.unhide_all(cx));
                                });
                            }
                            cx.notify();
                        }),
                    ))
                }),
            ))
    }
}
