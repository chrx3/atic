//! Los ajustes del dictado, en la ventana de Ajustes: cómo se activa, con qué
//! micrófono y cómo se transcribe.
//!
//! Es la misma `config.json` de Atic (y la misma llave de Groq del llavero),
//! como los de Reuniones (`meetings/settings.rs`), de donde salen las piezas.
//! `dictation.rs` relee el atajo y el modo cada 5 s: no hace falta reiniciar.

use std::path::PathBuf;

use atic_core::{AppDirs, Config, SecretKind};
use gpui::{div, prelude::*, px, App, ClickEvent, Context, Entity, Window};

use crate::meetings::settings::{
    card, dropdown, heading, key_row, menu_item, menu_list, row, save_config, segmented, switch,
    ConfigPane,
};
use crate::settings::{hsla, MUTED};

/// Sin micrófono propio, el dictado usa el de Reuniones (`dictation.rs`).
const SAME_AS_MEETINGS: &str = "El mismo de Reuniones";

pub struct DictationPane {
    path: PathBuf,
    cfg: Config,
    mic_menu: bool,
    /// `(id, nombre)`. `None` mientras se listan (lo hace un hilo: cpal tarda).
    devices: Option<Vec<(String, String)>>,
    notice: Option<(SecretKind, String, bool)>,
}

/// La sección, o `None` si no se encuentra la carpeta de datos de Atic.
pub fn pane(cx: &mut App) -> Option<Entity<DictationPane>> {
    let path = AppDirs::new().ok()?.config_path();
    Some(cx.new(|cx| DictationPane::new(path, cx)))
}

impl DictationPane {
    fn new(path: PathBuf, cx: &mut Context<Self>) -> Self {
        cx.spawn(async move |view, cx| {
            let devices = cx
                .background_spawn(async {
                    atic_audio::list_input_devices()
                        .map(|list| list.into_iter().map(|d| (d.id, d.name)).collect())
                        .unwrap_or_default()
                })
                .await;
            view.update(cx, |view, cx| {
                view.devices = Some(devices);
                cx.notify();
            })
            .ok();
        })
        .detach();
        #[cfg(target_os = "macos")]
        crate::apple_settings::refresh_if_stale(cx);

        Self {
            cfg: Config::load(&path),
            path,
            mic_menu: false,
            devices: None,
            notice: None,
        }
    }

    fn mic_label(&self) -> String {
        let id = &self.cfg.dictation_mic_device_id;
        if id.trim().is_empty() {
            return SAME_AS_MEETINGS.into();
        }
        self.devices
            .as_ref()
            .and_then(|list| list.iter().find(|(device, _)| device == id))
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| "Micrófono guardado".into())
    }
}

impl ConfigPane for DictationPane {
    fn edit_config(&mut self, cx: &mut Context<Self>, apply: impl FnOnce(&mut Config)) {
        self.cfg = save_config(&self.path, apply);
        cx.notify();
    }

    fn key_notice(&self) -> Option<&(SecretKind, String, bool)> {
        self.notice.as_ref()
    }

    fn set_key_notice(&mut self, notice: (SecretKind, String, bool), cx: &mut Context<Self>) {
        self.notice = Some(notice);
        cx.notify();
    }
}

/// `CmdOrCtrl+Shift+D` como se lee en macOS: `⇧⌘D`.
#[cfg(target_os = "macos")]
pub(crate) fn shortcut_label(shortcut: &str) -> String {
    crate::platform::keys::pretty(shortcut)
}

/// `CmdOrCtrl+Shift+D` como se lee en Windows: `Ctrl+Shift+D`.
#[cfg(not(target_os = "macos"))]
pub(crate) fn shortcut_label(shortcut: &str) -> String {
    shortcut
        .split('+')
        .map(|part| match part.trim().to_ascii_lowercase().as_str() {
            "cmdorctrl" | "commandorcontrol" | "control" => "Ctrl".to_string(),
            "super" | "meta" | "cmd" | "command" => "Win".to_string(),
            _ => part.trim().to_string(),
        })
        .collect::<Vec<_>>()
        .join("+")
}

impl DictationPane {
    /// En Windows dicta Groq; en Mac, por omisión la voz de macOS, con Groq
    /// para elegir y Apple Intelligence para pulir.
    fn transcription(&self, cx: &mut Context<Self>) -> gpui::Div {
        let cfg = self.cfg.clone();
        #[cfg(target_os = "macos")]
        let apple = cfg.dictation_backend != "groq";
        #[cfg(not(target_os = "macos"))]
        let apple = false;

        let mut card = card();
        #[cfg(target_os = "macos")]
        {
            use crate::apple_settings::{intelligence_line, snapshot, speech_line, status_control};
            card = card.child(row(
                "Motor",
                if apple { "En tu Mac, sin internet ni llave." } else { "En la nube de Groq, con tu llave." },
                segmented(
                    "dict-engine",
                    &[("Voz de macOS", "apple"), ("Groq", "groq")],
                    if apple { "apple" } else { "groq" },
                    |cfg, v| cfg.dictation_backend = v.into(),
                    cx,
                ),
            ));
            let snap = snapshot();
            if apple {
                let speech = speech_line(&snap);
                card = card.child(row("Voz de macOS", speech.hint.clone(), status_control("dict-apple-speech", &speech, cx)));
            }
            // Pulir sirve con cualquier motor: lo hace Apple Intelligence.
            let ai = intelligence_line(&snap);
            let control = if ai.ready {
                switch("dict-polish", cfg.dictation_polish, cx.listener(|v, _: &ClickEvent, _, cx| {
                    v.edit_config(cx, |cfg| cfg.dictation_polish = !cfg.dictation_polish)
                }))
                .into_any_element()
            } else {
                status_control("dict-apple-ai", &ai, cx)
            };
            let hint = if ai.ready {
                "Corrige puntuación y quita muletillas antes de pegar. Suma cerca de un segundo y puede cambiar alguna palabra.".to_string()
            } else {
                ai.hint.clone()
            };
            card = card.child(row("Pulir con Apple Intelligence", hint, control));
        }
        #[cfg(not(target_os = "macos"))]
        if cfg.dictation_backend != "groq" {
            card = card.child(row(
                "Motor",
                "Atic dicta con Whisper local, pero la pill solo dicta con Groq.",
                crate::meetings::settings::button(gpui::SharedString::from("dict-use-groq"), "Usar Groq", cx.listener(
                    |v, _: &ClickEvent, _, cx| v.edit_config(cx, |cfg| cfg.dictation_backend = "groq".into()),
                )),
            ));
        }
        if !apple {
            card = card.child(row(
                "Precisión",
                "Turbo es casi igual de bueno y bastante más rápido.",
                segmented(
                    "dict-groq-model",
                    &[("Turbo", "whisper-large-v3-turbo"), ("Máxima", "whisper-large-v3")],
                    &cfg.dictation_groq_model,
                    |cfg, v| cfg.dictation_groq_model = v.into(),
                    cx,
                ),
            ));
        }
        card = card.child(row(
            "Idioma",
            if apple { "El mismo de Reuniones. La voz de macOS no detecta el idioma: Detectar usa el del sistema." } else { "El mismo de Reuniones." },
            segmented(
                "dict-language",
                &[("Del sistema", "system"), ("Detectar", "auto"), ("Español", "es"), ("Inglés", "en")],
                &cfg.language,
                |cfg, v| cfg.language = v.into(),
                cx,
            ),
        ));
        if !apple {
            card = card.child(key_row(self, "Llave de Groq", SecretKind::GroqApiKey, cx));
        }
        card
    }
}

impl Render for DictationPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let cfg = &self.cfg;
        let toggle = cfg.dictation_mode == "toggle";

        let mut activation = card()
            .child(row(
                "Modo",
                if toggle {
                    "Una vez para empezar y otra para terminar."
                } else {
                    "Hablas mientras mantienes el atajo."
                },
                segmented(
                    "dict-mode",
                    &[("Mantener", "push_to_talk"), ("Alternar", "toggle")],
                    &cfg.dictation_mode,
                    |cfg, v| cfg.dictation_mode = v.into(),
                    cx,
                ),
            ))
            .child(row(
                "Atajo",
                "Por ahora se cambia en los Ajustes de Atic.",
                div()
                    .flex_none()
                    .text_size(px(13.))
                    .text_color(hsla(MUTED))
                    .child(shortcut_label(&cfg.dictation_shortcut)),
            ))
            .child(row(
                "Micrófono",
                "",
                dropdown("dict-mic-menu", self.mic_label(), self.mic_menu, cx.listener(
                    |v, _: &ClickEvent, _, cx| {
                        v.mic_menu = !v.mic_menu;
                        cx.notify();
                    },
                )),
            ));
        if self.mic_menu {
            let mut options = vec![(String::new(), SAME_AS_MEETINGS.to_string())];
            options.extend(self.devices.iter().flatten().cloned());
            let mut menu = menu_list();
            for (ix, (id, name)) in options.into_iter().enumerate() {
                let on = cfg.dictation_mic_device_id == id;
                menu = menu.child(menu_item(("dict-mic-opt", ix), name, on, cx.listener(
                    move |v, _: &ClickEvent, _, cx| {
                        v.mic_menu = false;
                        let id = id.clone();
                        v.edit_config(cx, move |cfg| cfg.dictation_mic_device_id = id);
                    },
                )));
            }
            if self.devices.is_none() {
                menu = menu.child(
                    div().px(px(12.)).py(px(8.)).text_size(px(12.)).text_color(hsla(MUTED)).child("Buscando micrófonos…"),
                );
            }
            activation = activation.child(menu);
        }

        let transcription = self.transcription(cx);

        div()
            .flex()
            .flex_col()
            .child(heading("Cómo se activa"))
            .child(activation)
            .child(heading("Transcripción"))
            .child(transcription)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn shortcuts_read_as_on_windows() {
        assert_eq!(shortcut_label("CmdOrCtrl+Shift+D"), "Ctrl+Shift+D");
        assert_eq!(shortcut_label("Alt+X"), "Alt+X");
        assert_eq!(shortcut_label("Super+Space"), "Win+Space");
    }
}
