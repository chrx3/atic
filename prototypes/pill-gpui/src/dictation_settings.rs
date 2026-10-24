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
    card, dropdown, engine_hint, engine_value, heading, key_row, local_model_row, menu_item, menu_list, row,
    save_config, segmented, switch, ConfigPane, engines,
};
use crate::i18n::t;
use crate::settings::{hsla, MUTED};

/// Sin micrófono propio, el dictado usa el de Reuniones (`dictation.rs`).
fn same_as_meetings() -> &'static str {
    t("pill.dictation.sameAsMeetings")
}

pub struct DictationPane {
    path: PathBuf,
    cfg: Config,
    mic_menu: bool,
    /// `(id, nombre)`. `None` mientras se listan (lo hace un hilo: cpal tarda).
    devices: Option<Vec<(String, String)>>,
    notice: Option<(SecretKind, String, bool)>,
    /// El prompt que Groq usa al dictar, para verlo y editarlo.
    prompt: Entity<crate::prompt_editor::PromptEditor>,
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

        let cfg = Config::load(&path);
        // El de Atic se arma de nuevo cada vez que se abre Ajustes.
        let english = crate::prompts::dictation_english(&cfg.language);
        let prompt = cx.new(|cx| crate::prompt_editor::PromptEditor::new(crate::prompts::Kind::Dictation, english, cx));
        Self {
            cfg,
            prompt,
            path,
            mic_menu: false,
            devices: None,
            notice: None,
        }
    }

    fn mic_label(&self) -> String {
        let id = &self.cfg.dictation_mic_device_id;
        if id.trim().is_empty() {
            return same_as_meetings().into();
        }
        self.devices
            .as_ref()
            .and_then(|list| list.iter().find(|(device, _)| device == id))
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| t("pill.meetingsSettings.savedMic").into())
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
        let value = engine_value(&cfg.dictation_backend);
        let apple = value == "apple";
        let groq = value == "groq";

        let mut card = card().child(row(
            t("pill.meetingsSettings.engine"),
            engine_hint(value),
            segmented("dict-engine", &engines(), value, |cfg, v| cfg.dictation_backend = v.into(), cx),
        ));
        if value == "parakeet" {
            card = card.child(local_model_row("dict-local-model", cx));
        }
        #[cfg(target_os = "macos")]
        {
            use crate::apple_settings::{intelligence_line, snapshot, speech_line, status_control};
            let snap = snapshot();
            if apple {
                let speech = speech_line(&snap);
                card = card.child(row(t("pill.meetingsSettings.macosVoice"), speech.hint.clone(), status_control("dict-apple-speech", &speech, cx)));
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
                t("pill.dictation.polishHint").to_string()
            } else {
                ai.hint.clone()
            };
            card = card.child(row(t("pill.dictation.polish"), hint, control));
        }
        if groq {
            card = card.child(row(
                t("pill.meetingsSettings.precision"),
                t("pill.meetingsSettings.precisionHint"),
                segmented(
                    "dict-groq-model",
                    &[("Turbo", "whisper-large-v3-turbo"), (t("pill.meetingsSettings.precisionMax"), "whisper-large-v3")],
                    &cfg.dictation_groq_model,
                    |cfg, v| cfg.dictation_groq_model = v.into(),
                    cx,
                ),
            ));
        }
        card = card.child(row(
            t("pill.meetingsSettings.language"),
            if apple { t("pill.dictation.languageHintApple") } else { t("pill.dictation.languageHint") },
            segmented(
                "dict-language",
                &[(t("pill.meetingsSettings.langSystem"), "system"), (t("pill.meetingsSettings.langDetect"), "auto"), (t("pill.meetingsSettings.langSpanish"), "es"), (t("pill.meetingsSettings.langEnglish"), "en")],
                &cfg.language,
                |cfg, v| cfg.language = v.into(),
                cx,
            ),
        ));
        if groq {
            card = card.child(key_row(self, t("pill.meetingsSettings.groqKey"), SecretKind::GroqApiKey, cx));
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
                t("pill.dictation.mode"),
                if toggle {
                    t("pill.dictation.modeToggleHint")
                } else {
                    t("pill.dictation.modeHoldHint")
                },
                segmented(
                    "dict-mode",
                    &[(t("pill.dictation.modeHold"), "push_to_talk"), (t("pill.dictation.modeToggle"), "toggle")],
                    &cfg.dictation_mode,
                    |cfg, v| cfg.dictation_mode = v.into(),
                    cx,
                ),
            ))
            .child(row(
                t("pill.dictation.shortcut"),
                t("pill.dictation.shortcutHint"),
                div()
                    .flex_none()
                    .text_size(px(13.))
                    .text_color(hsla(MUTED))
                    .child(shortcut_label(&cfg.dictation_shortcut)),
            ))
            .child(row(
                t("pill.meetingsSettings.mic"),
                "",
                dropdown("dict-mic-menu", self.mic_label(), self.mic_menu, cx.listener(
                    |v, _: &ClickEvent, _, cx| {
                        v.mic_menu = !v.mic_menu;
                        cx.notify();
                    },
                )),
            ));
        if self.mic_menu {
            let mut options = vec![(String::new(), same_as_meetings().to_string())];
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
                    div().px(px(12.)).py(px(8.)).text_size(px(12.)).text_color(hsla(MUTED)).child(t("pill.meetingsSettings.searchingMics")),
                );
            }
            activation = activation.child(menu);
        }

        let transcription = self.transcription(cx);

        div()
            .flex()
            .flex_col()
            .child(heading(t("pill.dictation.activation")))
            .child(activation)
            .child(heading(t("pill.meetingsSettings.transcription")))
            .child(transcription)
            // El prompt solo lo usa Groq (Whisper): con la voz de macOS o el
            // modelo local no hay dónde mandarlo.
            .when(engine_value(&self.cfg.dictation_backend) == "groq", |el| {
                el.child(heading(t("pill.prompts.dictationHeading"))).child(card().child(self.prompt.clone()))
            })
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
