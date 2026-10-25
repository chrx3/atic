//! Los ajustes de Reuniones: qué se graba, cómo se transcribe, con qué se
//! resume y cómo se manda el correo.
//!
//! Es la misma `config.json` de Atic (y las mismas llaves del llavero): lo que
//! se cambia aquí lo ve Atic y al revés. Cada cambio se guarda al tiro,
//! releyendo el archivo antes para no pisar lo que Atic haya escrito entre
//! medio.

use atic_core::{secrets, Config, SecretKind};
use gpui::{
    actions, div, prelude::*, px, svg, App, ClickEvent, Context, Entity, EventEmitter,
    FocusHandle, Focusable, FontWeight, KeyBinding, SharedString, Subscription, Window,
};

use atic_summarize::{ProviderInfo, PROVIDERS};

use super::data::Paths;
use super::{hsla, INK, ITEM, MUTED, SURFACE, SURFACE_ON, TEXT, FAINT, GREEN, RED};
use crate::hover::HoverExt;
use crate::i18n::{t, tf};
use gpui_m3::theme::type_scale::{self, TypeScaleExt};
use crate::text_input::{self, TextInput};

actions!(meetings_settings, [CloseSettings]);

const KEY_CONTEXT: &str = "MeetingsSettings";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", CloseSettings, Some(KEY_CONTEXT))]);
}

/// Un campo de texto más oscuro que la tarjeta, como un pozo.
const FIELD: u32 = crate::theme::SUNKEN;
const FIELD_W: f32 = 300.0;

pub enum SettingsEvent {
    Close,
}

/// Una vista de ajustes que edita `config.json` y las llaves del llavero.
/// Las piezas de abajo (`segmented`, `key_row`…) sirven a cualquiera que la
/// implemente: Reuniones y Dictado en la ventana de Ajustes.
pub(crate) trait ConfigPane: Sized + 'static {
    /// Relee, cambia y guarda: Atic puede haber escrito la config entre medio.
    fn edit_config(&mut self, cx: &mut Context<Self>, apply: impl FnOnce(&mut Config));
    /// El aviso bajo la llave que se acaba de tocar.
    fn key_notice(&self) -> Option<&(SecretKind, String, bool)>;
    fn set_key_notice(&mut self, notice: (SecretKind, String, bool), cx: &mut Context<Self>);
}

/// Relee `config.json`, aplica el cambio y lo guarda. Devuelve lo guardado.
pub(crate) fn save_config(path: &std::path::Path, apply: impl FnOnce(&mut Config)) -> Config {
    let mut cfg = Config::load(path);
    apply(&mut cfg);
    if let Err(error) = cfg.save(path) {
        eprintln!("ajustes: no se pudo guardar la configuración: {error}");
    }
    cfg
}

/// El proveedor guardado o, si no se conoce, el primero (Claude).
fn provider(id: &str) -> &'static ProviderInfo {
    atic_summarize::find_provider(id).unwrap_or(&PROVIDERS[0])
}

fn provider_name(p: &ProviderInfo) -> &'static str {
    match p.id {
        "claude" => "Claude",
        "custom" => t("pill.meetingsSettings.providerOpenAi"),
        _ => p.display_name,
    }
}

fn provider_key(p: &ProviderInfo) -> Option<SecretKind> {
    if p.needs_api_key {
        SecretKind::for_summary_provider(p.id)
    } else {
        None
    }
}

/// Los modelos que se ofrecen en el desplegable.
#[derive(Default)]
struct Models {
    provider: String,
    list: Vec<String>,
    /// Vino del proveedor recién; si no, es la sugerida del catálogo.
    live: bool,
    loading: bool,
    /// Por qué no se pudo pedir (sin llave, sin conexión…).
    problem: Option<String>,
}

/// Le pide la lista al proveedor (`GET /models`, `/api/tags`): Groq y el resto
/// apagan modelos sin avisar y el catálogo fijo queda viejo. Sin llave o sin
/// conexión, la lista sugerida del catálogo. Bloquea: va en un hilo.
fn fetch_models(p: &'static ProviderInfo, base_url: &str) -> (Vec<String>, bool, Option<String>) {
    let base = if base_url.trim().is_empty() { p.default_base_url } else { base_url };
    let key = provider_key(p).and_then(|kind| secrets::get_secret(kind).ok().flatten());
    let fallback = atic_summarize::order_models(
        p.suggested_models.iter().map(|m| m.to_string()).collect(),
        p.default_model,
    );
    if p.needs_api_key && key.is_none() {
        return (fallback, false, Some(t("pill.meetingsSettings.pasteKeyForModels").into()));
    }
    match atic_summarize::list_remote_models(p.kind, p.id, base, key.as_deref()) {
        Ok(list) if !list.is_empty() => (atic_summarize::order_models(list, p.default_model), true, None),
        Ok(_) => (fallback, false, Some(t("pill.meetingsSettings.noModels").into())),
        Err(error) => (fallback, false, Some(tf("pill.meetingsSettings.modelsFailed", &[("error", &error)]))),
    }
}

/// La llave tal como se muestra: el principio y el final, nunca entera.
pub fn key_preview(key: &str) -> String {
    let chars: Vec<char> = key.trim().chars().collect();
    if chars.len() <= 10 {
        return "••••••".into();
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 3..].iter().collect();
    format!("{head}••••{tail}")
}

/// Lo que se puede pegar como llave: una sola línea sin espacios, de largo
/// razonable. Evita guardar por error un párrafo que estaba en el
/// portapapeles.
pub fn looks_like_key(text: &str) -> bool {
    let text = text.trim();
    (16..=512).contains(&text.len()) && !text.chars().any(char::is_whitespace)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Menu {
    Mic,
    Provider,
    Model,
}

struct Device {
    id: String,
    name: String,
}

pub struct SettingsView {
    paths: Paths,
    cfg: Config,
    focus: FocusHandle,
    menu: Option<Menu>,
    /// `None` mientras se listan (lo hace un hilo: cpal tarda).
    devices: Option<Vec<Device>>,
    model: Entity<TextInput>,
    base_url: Entity<TextInput>,
    smtp_host: Entity<TextInput>,
    smtp_port: Entity<TextInput>,
    smtp_user: Entity<TextInput>,
    smtp_from: Entity<TextInput>,
    /// Un aviso corto bajo la llave que se acaba de tocar.
    notice: Option<(SecretKind, String, bool)>,
    models: Models,
    /// Los prompts del resumen: los de Atic y los propios (`prompt_editor`).
    prompts: Entity<crate::prompt_editor::PromptEditor>,
    /// Dentro de la ventana de Ajustes: sin título ni cerrar, y sin su propio
    /// desplazamiento (lo pone la ventana).
    embedded: bool,
    _subscriptions: Vec<Subscription>,
}

impl ConfigPane for SettingsView {
    fn edit_config(&mut self, cx: &mut Context<Self>, apply: impl FnOnce(&mut Config)) {
        self.update(cx, apply);
    }

    fn key_notice(&self) -> Option<&(SecretKind, String, bool)> {
        self.notice.as_ref()
    }

    fn set_key_notice(&mut self, notice: (SecretKind, String, bool), cx: &mut Context<Self>) {
        let kind = notice.0;
        self.notice = Some(notice);
        self.key_changed(kind, cx);
    }
}

impl EventEmitter<SettingsEvent> for SettingsView {}

impl Focusable for SettingsView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl SettingsView {
    pub fn new(paths: Paths, cx: &mut Context<Self>) -> Self {
        let cfg = Config::load(&paths.config_path());
        let field = |placeholder: &str, value: &str, cx: &mut Context<Self>| {
            let placeholder = placeholder.to_string();
            let value = value.to_string();
            cx.new(|cx| {
                let mut input = TextInput::new(placeholder, hsla(TEXT), hsla(FAINT), hsla(TEXT), cx);
                input.set_text(value, cx);
                input
            })
        };
        let model = field(t("pill.meetingsSettings.model"), &cfg.summary_model, cx);
        let base_url = field("https://…", &cfg.summary_base_url, cx);
        let smtp_host = field(t("pill.meetingsSettings.smtpHostPlaceholder"), &cfg.smtp_host, cx);
        let smtp_port = field("587", &cfg.smtp_port.to_string(), cx);
        let smtp_user = field(t("pill.meetingsSettings.smtpUserPlaceholder"), &cfg.smtp_username, cx);
        let smtp_from = field(t("pill.meetingsSettings.smtpFromPlaceholder"), &cfg.smtp_from, cx);

        let mut subscriptions = Vec::new();
        let mut bind = |input: &Entity<TextInput>, apply: fn(&mut Config, &str), cx: &mut Context<Self>| {
            subscriptions.push(cx.subscribe(input, move |view, input, _: &text_input::Changed, cx| {
                let text = input.read(cx).text().trim().to_string();
                view.update(cx, move |cfg| apply(cfg, &text));
            }));
        };
        bind(&model, |c, t| c.summary_model = t.into(), cx);
        bind(&base_url, |c, t| c.summary_base_url = t.into(), cx);
        bind(&smtp_host, |c, t| c.smtp_host = t.into(), cx);
        bind(&smtp_port, |c, t| {
            if let Ok(port) = t.parse() {
                c.smtp_port = port;
            }
        }, cx);
        bind(&smtp_user, |c, t| c.smtp_username = t.into(), cx);
        bind(&smtp_from, |c, t| c.smtp_from = t.into(), cx);

        cx.spawn(async move |view, cx| {
            let devices = cx
                .background_spawn(async {
                    atic_audio::list_input_devices()
                        .map(|list| {
                            list.into_iter().map(|d| Device { id: d.id, name: d.name }).collect()
                        })
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

        let english = cfg.resolved_ui_language() == "en";
        let prompts = cx.new(|cx| crate::prompt_editor::PromptEditor::new(crate::prompts::Kind::Summary, english, cx));
        let mut view = Self {
            prompts,
            paths,
            cfg,
            focus: cx.focus_handle(),
            menu: None,
            devices: None,
            model,
            base_url,
            smtp_host,
            smtp_port,
            smtp_user,
            smtp_from,
            notice: None,
            models: Models::default(),
            embedded: false,
            _subscriptions: subscriptions,
        };
        view.refresh_models(cx);
        #[cfg(target_os = "macos")]
        crate::apple_settings::refresh_if_stale(cx);
        view
    }

    /// La misma vista, para la sección Reuniones de la ventana de Ajustes.
    pub fn new_embedded(paths: Paths, cx: &mut Context<Self>) -> Self {
        Self { embedded: true, ..Self::new(paths, cx) }
    }

    fn refresh_models(&mut self, cx: &mut Context<Self>) {
        let p = provider(&self.cfg.summary_backend);
        let base_url = self.cfg.summary_base_url.clone();
        if self.models.provider != p.id {
            self.models = Models { provider: p.id.into(), ..Default::default() };
        }
        self.models.loading = true;
        cx.notify();
        cx.spawn(async move |view, cx| {
            let (list, live, problem) = cx.background_spawn(async move { fetch_models(p, &base_url) }).await;
            view.update(cx, |view, cx| {
                // Se cambió de proveedor mientras tanto: esta lista ya no sirve.
                if view.models.provider != p.id {
                    return;
                }
                view.models = Models { provider: p.id.into(), list, live, loading: false, problem };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn set_model(&mut self, model: String, cx: &mut Context<Self>) {
        self.menu = None;
        self.model.update(cx, |input, cx| input.set_text(model.clone(), cx));
        self.update(cx, move |cfg| cfg.summary_model = model);
    }

    /// Relee, cambia y guarda: Atic puede haber escrito la config entre medio.
    fn update(&mut self, cx: &mut Context<Self>, apply: impl FnOnce(&mut Config)) {
        self.cfg = save_config(&self.paths.config_path(), apply);
        cx.notify();
    }

    fn set_provider(&mut self, id: &'static str, cx: &mut Context<Self>) {
        let p = provider(id);
        self.menu = None;
        if self.cfg.summary_backend == id {
            cx.notify();
            return;
        }
        // La URL solo se guarda donde se edita; en los demás manda la del catálogo.
        let base_url = if p.base_url_editable { p.default_base_url } else { "" };
        self.update(cx, |cfg| {
            cfg.summary_backend = p.id.into();
            cfg.summary_model = p.default_model.into();
            cfg.summary_base_url = base_url.into();
        });
        self.model.update(cx, |input, cx| input.set_text(p.default_model, cx));
        self.base_url.update(cx, |input, cx| input.set_text(base_url, cx));
        self.refresh_models(cx);
    }

    /// Con otra llave, otra lista: la de antes pudo salir del catálogo por falta de ella.
    fn key_changed(&mut self, kind: SecretKind, cx: &mut Context<Self>) {
        if provider_key(provider(&self.cfg.summary_backend)) == Some(kind) {
            self.refresh_models(cx);
        }
        cx.notify();
    }
}

fn paste_key<V: ConfigPane>(view: &mut V, kind: SecretKind, cx: &mut Context<V>) {
    let text = cx.read_from_clipboard().and_then(|item| item.text()).unwrap_or_default();
    let (message, ok) = if !looks_like_key(&text) {
        (t("pill.meetingsSettings.noKeyInClipboard").to_string(), false)
    } else {
        match secrets::set_secret(kind, text.trim()) {
            Ok(()) => {
                set_shown_key(kind, Some(Some(key_preview(text.trim()))));
                (t("pill.meetingsSettings.keySaved").to_string(), true)
            }
            Err(error) => (tf("pill.meetingsSettings.saveFailed", &[("error", &error)]), false),
        }
    };
    view.set_key_notice((kind, message, ok), cx);
}

fn remove_key<V: ConfigPane>(view: &mut V, kind: SecretKind, cx: &mut Context<V>) {
    let (message, ok) = match secrets::delete_secret(kind) {
        Ok(()) => {
            set_shown_key(kind, Some(None));
            (t("pill.meetingsSettings.keyRemoved").to_string(), true)
        }
        Err(error) => (tf("pill.meetingsSettings.removeFailed", &[("error", &error)]), false),
    };
    view.set_key_notice((kind, message, ok), cx);
}

impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let cfg = &self.cfg;
        let p = provider(&cfg.summary_backend);

        // --- Grabación
        let mic_label = if cfg.mic_device_id.is_empty() {
            t("pill.meetingsSettings.systemDefault").to_string()
        } else {
            self.devices
                .as_ref()
                .and_then(|list| list.iter().find(|d| d.id == cfg.mic_device_id))
                .map(|d| d.name.clone())
                .unwrap_or_else(|| t("pill.meetingsSettings.savedMic").into())
        };
        let mut recording = card()
            .child(row(
                t("pill.meetingsSettings.whatToRecord"),
                t("pill.meetingsSettings.whatToRecordHint"),
                segmented(
                    "tracks",
                    &[(t("pill.meetingsSettings.tracksBoth"), "both"), (t("pill.meetingsSettings.mic"), "mic"), (t("pill.meetingsSettings.tracksSystem"), "system")],
                    &cfg.record_tracks,
                    |cfg, v| cfg.record_tracks = v.into(),
                    cx,
                ),
            ))
            .child(row(
                t("pill.meetingsSettings.mic"),
                "",
                dropdown("mic-menu", mic_label, self.menu == Some(Menu::Mic), cx.listener(
                    |v, _: &ClickEvent, _, cx| {
                        v.menu = if v.menu == Some(Menu::Mic) { None } else { Some(Menu::Mic) };
                        cx.notify();
                    },
                )),
            ));
        if self.menu == Some(Menu::Mic) {
            let mut options = vec![(String::new(), t("pill.meetingsSettings.systemDefault").to_string())];
            match &self.devices {
                Some(list) => options.extend(list.iter().map(|d| (d.id.clone(), d.name.clone()))),
                None => {}
            }
            let mut menu = menu_list();
            for (ix, (id, name)) in options.into_iter().enumerate() {
                let on = cfg.mic_device_id == id;
                menu = menu.child(menu_item(("mic-opt", ix), name, on, cx.listener(
                    move |v, _: &ClickEvent, _, cx| {
                        v.menu = None;
                        let id = id.clone();
                        v.update(cx, move |cfg| cfg.mic_device_id = id);
                    },
                )));
            }
            if self.devices.is_none() {
                menu = menu.child(div().px(px(12.)).py(px(8.)).text_size(px(12.)).text_color(hsla(MUTED)).child(t("pill.meetingsSettings.searchingMics")));
            }
            recording = recording.child(menu);
        }
        let recording = recording
            .child(row(
                t("pill.meetingsSettings.noise"),
                t("pill.meetingsSettings.noiseHint"),
                segmented(
                    "noise",
                    &[(t("pill.meetingsSettings.noiseOff"), "off"), (t("pill.meetingsSettings.noiseLow"), "low"), (t("pill.meetingsSettings.noiseMedium"), "medium"), (t("pill.meetingsSettings.noiseHigh"), "high")],
                    &cfg.noise_suppression,
                    |cfg, v| cfg.noise_suppression = v.into(),
                    cx,
                ),
            ))
            .child(row(
                t("pill.meetingsSettings.liveCaptions"),
                t("pill.meetingsSettings.liveCaptionsHint"),
                switch("live", cfg.live_transcription, cx.listener(|v, _: &ClickEvent, _, cx| {
                    v.update(cx, |cfg| {
                        cfg.live_transcription = !cfg.live_transcription;
                        cfg.live_engine = "groq".into();
                    })
                })),
            ))
            .child(row(
                t("pill.meetingsSettings.beep"),
                t("pill.meetingsSettings.beepHint"),
                switch("beep", cfg.beep_on_start, cx.listener(|v, _: &ClickEvent, _, cx| {
                    v.update(cx, |cfg| cfg.beep_on_start = !cfg.beep_on_start)
                })),
            ));

        // --- Transcripción
        // En Mac, por omisión la voz de macOS (en el equipo); «local», lo que
        // dejaba la Atic de Tauri, también va con ella.
        let value = engine_value(&cfg.meeting_backend);
        let mut engine = card().child(row(
            t("pill.meetingsSettings.engine"),
            engine_hint(value),
            segmented("meeting-engine", &engines(), value, |cfg, v| cfg.meeting_backend = v.into(), cx),
        ));
        #[cfg(target_os = "macos")]
        if value == "apple" {
            use crate::apple_settings::{snapshot, speech_line, status_control};
            let speech = speech_line(&snapshot());
            engine = engine.child(row(t("pill.meetingsSettings.macosVoice"), speech.hint.clone(), status_control("meeting-apple-speech", &speech, cx)));
        }
        if value == "parakeet" {
            engine = engine.child(local_model_row("meeting-local-model", cx));
        }
        if value == "groq" {
            engine = engine.child(row(
                t("pill.meetingsSettings.precision"),
                t("pill.meetingsSettings.precisionHint"),
                segmented(
                    "groq-model",
                    &[("Turbo", "whisper-large-v3-turbo"), (t("pill.meetingsSettings.precisionMax"), "whisper-large-v3")],
                    &cfg.meeting_groq_model,
                    |cfg, v| {
                        cfg.meeting_groq_model = v.into();
                        cfg.meeting_backend = "groq".into();
                    },
                    cx,
                ),
            ));
        }
        let transcription = engine
            .child(row(
                t("pill.meetingsSettings.autoTranscribe"),
                t("pill.meetingsSettings.autoTranscribeHint"),
                switch("auto", cfg.auto_transcribe_after_recording, cx.listener(
                    |v, _: &ClickEvent, _, cx| {
                        v.update(cx, |cfg| {
                            cfg.auto_transcribe_after_recording = !cfg.auto_transcribe_after_recording
                        })
                    },
                )),
            ))
            .child(row(
                t("pill.meetingsSettings.language"),
                "",
                segmented(
                    "language",
                    &[(t("pill.meetingsSettings.langSystem"), "system"), (t("pill.meetingsSettings.langDetect"), "auto"), (t("pill.meetingsSettings.langSpanish"), "es"), (t("pill.meetingsSettings.langEnglish"), "en")],
                    &cfg.language,
                    |cfg, v| cfg.language = v.into(),
                    cx,
                ),
            ))
            .child(key_row(self, t("pill.meetingsSettings.groqKey"), SecretKind::GroqApiKey, cx));

        // --- Resumen
        let mut summary = card().child(row(
            t("pill.meetingsSettings.provider"),
            t("pill.meetingsSettings.providerHint"),
            dropdown("provider-menu", provider_name(p).to_string(), self.menu == Some(Menu::Provider), cx.listener(
                |v, _: &ClickEvent, _, cx| {
                    v.menu = if v.menu == Some(Menu::Provider) { None } else { Some(Menu::Provider) };
                    cx.notify();
                },
            )),
        ));
        if self.menu == Some(Menu::Provider) {
            let mut menu = menu_list();
            for (ix, option) in PROVIDERS.iter().enumerate() {
                let id = option.id;
                menu = menu.child(menu_item(("provider-opt", ix), provider_name(option).to_string(), option.id == p.id, cx.listener(
                    move |v, _: &ClickEvent, _, cx| v.set_provider(id, cx),
                )));
            }
            summary = summary.child(menu);
        }
        let models_hint: SharedString = if self.models.loading && self.models.list.is_empty() {
            t("pill.meetingsSettings.searchingModels").into()
        } else if let Some(problem) = &self.models.problem {
            problem.clone().into()
        } else if self.models.live {
            tf("pill.meetingsSettings.modelsAvailable", &[("count", &self.models.list.len()), ("provider", &provider_name(p))]).into()
        } else {
            "".into()
        };
        let model_label = if cfg.summary_model.trim().is_empty() {
            t("pill.meetingsSettings.chooseModel").to_string()
        } else {
            cfg.summary_model.clone()
        };
        summary = summary.child(row(
            t("pill.meetingsSettings.model"),
            models_hint,
            dropdown("model-menu", model_label, self.menu == Some(Menu::Model), cx.listener(
                |v, _: &ClickEvent, _, cx| {
                    if v.menu == Some(Menu::Model) {
                        v.menu = None;
                    } else {
                        v.menu = Some(Menu::Model);
                        // Al abrir se vuelve a pedir si la vez anterior no resultó.
                        if !v.models.live && !v.models.loading {
                            v.refresh_models(cx);
                        }
                    }
                    cx.notify();
                },
            )),
        ));
        if self.menu == Some(Menu::Model) {
            let current = cfg.summary_model.trim().to_string();
            let mut options = div()
                .id("model-options")
                .max_h(px(264.))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(2.));
            // El guardado aunque el proveedor ya no lo ofrezca: que se vea cuál es.
            if !current.is_empty() && !self.models.list.contains(&current) && !self.models.loading {
                options = options.child(menu_item(
                    ("model-opt-current", 0),
                    tf("pill.meetingsSettings.notInList", &[("model", &current)]),
                    true,
                    |_, _, _| {},
                ));
            }
            for (ix, model) in self.models.list.iter().enumerate() {
                let model = model.clone();
                let on = model == current;
                options = options.child(menu_item(("model-opt", ix), model.clone(), on, cx.listener(
                    move |v, _: &ClickEvent, _, cx| v.set_model(model.clone(), cx),
                )));
            }
            summary = summary.child(
                menu_list().child(options).child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .px(px(10.))
                        .pt(px(6.))
                        .pb(px(2.))
                        .child(div().text_size(px(12.)).text_color(hsla(MUTED)).child(t("pill.meetingsSettings.other")))
                        .child(div().flex_1().child(field(&self.model))),
                ),
            );
        }
        if p.base_url_editable {
            summary = summary.child(row(t("pill.meetingsSettings.address"), "", field(&self.base_url)));
        }
        if let Some(kind) = provider_key(p) {
            summary = summary.child(key_row(self, t("pill.meetingsSettings.key"), kind, cx));
        }

        // --- Correo
        let smtp = cfg.mail_backend == "smtp";
        let mut mail = card().child(row(
            t("pill.meetingsSettings.mailHow"),
            if smtp { t("pill.meetingsSettings.mailSmtpHint") } else { t("pill.meetingsSettings.mailMailtoHint") },
            segmented(
                "mail",
                &[(t("pill.meetingsSettings.mailOpen"), "mailto"), (t("pill.meetingsSettings.mailSmtp"), "smtp")],
                &cfg.mail_backend,
                |cfg, v| cfg.mail_backend = v.into(),
                cx,
            ),
        ));
        if smtp {
            mail = mail
                .child(row(t("pill.meetingsSettings.server"), "", field(&self.smtp_host)))
                .child(row(t("pill.meetingsSettings.port"), "", field(&self.smtp_port)))
                .child(row(t("pill.meetingsSettings.user"), "", field(&self.smtp_user)))
                .child(row(t("pill.meetingsSettings.sender"), "", field(&self.smtp_from)))
                .child(row(
                    t("pill.meetingsSettings.tls"),
                    t("pill.meetingsSettings.tlsHint"),
                    switch("tls", cfg.smtp_use_tls, cx.listener(|v, _: &ClickEvent, _, cx| {
                        v.update(cx, |cfg| cfg.smtp_use_tls = !cfg.smtp_use_tls)
                    })),
                ))
                .child(key_row(self, t("pill.meetingsSettings.password"), SecretKind::SmtpPassword, cx));
        }

        let sections = div()
            .flex()
            .flex_col()
            .child(heading(t("pill.meetingsSettings.recording")))
            .child(recording)
            .child(heading(t("pill.meetingsSettings.transcription")))
            .child(transcription)
            .child(heading(t("pill.meetingsSettings.summary")))
            .child(summary)
            .child(heading(t("pill.prompts.summaryHeading")))
            .child(card().child(self.prompts.clone()))
            .child(heading(t("pill.meetingsSettings.mail")))
            .child(mail);

        let root = div()
            .id("meetings-settings")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|v, _: &CloseSettings, _, cx| {
                if v.menu.take().is_some() {
                    cx.notify();
                } else {
                    cx.emit(SettingsEvent::Close);
                }
            }));
        if self.embedded {
            return root.child(sections);
        }

        root
            .size_full()
            .overflow_y_scroll()
            .child(
                div()
                    .w_full()
                    .max_w(px(760.))
                    .mx_auto()
                    .px(px(28.))
                    .pt(px(22.))
                    .pb(px(36.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .pb(px(6.))
                            .child(
                                div()
                                    .flex_1()
                                    .map(|el| match crate::theme::expressive() {
                                        // El titular de M3, como el de la ventana de Ajustes.
                                        Some(_) => el.text_size(px(26.)).font_weight(FontWeight::NORMAL),
                                        None => el.text_size(px(20.)).font_weight(FontWeight::SEMIBOLD),
                                    })
                                    .child(t("pill.meetingsSettings.title")),
                            )
                            .child(super::icon_button(
                                "settings-close",
                                "icons/x.svg",
                                t("pill.meetingsSettings.close"),
                                false,
                                cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(SettingsEvent::Close)),
                            )),
                    )
                    .child(
                        div()
                            .pb(px(8.))
                            .text_size(px(13.))
                            .text_color(hsla(MUTED))
                            .child(t("pill.meetingsSettings.subtitle")),
                    )
                    .child(sections),
            )
    }
}

/// Lo que se muestra de cada llave: `None` mientras se lee, `Some(None)` si
/// no hay, `Some(Some(vista previa))` si está. Leer el llavero puede trabarse
/// segundos (macOS pide permiso cuando la firma de la app cambió) y `key_row`
/// va en cada cuadro: leerlo ahí congelaba la pill entera. Se lee una vez, en
/// otro hilo; pegar o quitar la llave lo pone al día.
static SHOWN_KEYS: std::sync::Mutex<Vec<(SecretKind, Option<Option<String>>)>> = std::sync::Mutex::new(Vec::new());

fn set_shown_key(kind: SecretKind, shown: Option<Option<String>>) {
    if let Ok(mut keys) = SHOWN_KEYS.lock() {
        keys.retain(|(k, _)| *k != kind);
        keys.push((kind, shown));
    }
}

/// Una llave guardada fuera de Ajustes (el «Pegar llave» de un aviso).
pub(super) fn key_saved(kind: SecretKind, key: &str) {
    set_shown_key(kind, Some(Some(key_preview(key))));
}

/// La vista previa de la llave, sin tocar el llavero en el hilo principal.
pub(crate) fn shown_key<V: 'static>(kind: SecretKind, cx: &mut Context<V>) -> Option<Option<String>> {
    if let Some((_, shown)) = SHOWN_KEYS.lock().ok()?.iter().find(|(k, _)| *k == kind) {
        return shown.clone();
    }
    set_shown_key(kind, None);
    cx.spawn(async move |this, cx| {
        let read = cx.background_spawn(async move { secrets::get_secret(kind).ok().flatten() }).await;
        set_shown_key(kind, Some(read.as_deref().map(key_preview)));
        let _ = this.update(cx, |_, cx| cx.notify());
    })
    .detach();
    None
}

/// Una llave del llavero: si está, su vista previa; pegarla o quitarla.
pub(crate) fn key_row<V: ConfigPane>(
    view: &V,
    title: &'static str,
    kind: SecretKind,
    cx: &mut Context<V>,
) -> impl IntoElement {
    let shown = shown_key(kind, cx);
    let stored = shown.clone().flatten();
    let notice = view.key_notice();
    let hint: SharedString = match (notice, &shown) {
        (Some((k, message, _)), _) if *k == kind => message.clone().into(),
        (_, None) => t("pill.meetingsSettings.readingKeychain").into(),
        (_, Some(Some(preview))) => tf("pill.meetingsSettings.keyStored", &[("preview", preview)]).into(),
        (_, Some(None)) => t("pill.meetingsSettings.keyEmpty").into(),
    };
    let tone = match notice {
        Some((k, _, ok)) if *k == kind => if *ok { GREEN } else { RED },
        _ => MUTED,
    };
    let id = kind.as_str();
    let expressive = crate::theme::expressive().is_some();
    div()
        .flex()
        .items_center()
        .gap(px(16.))
        .px(px(16.))
        .py(px(if expressive { 8. } else { 12. }))
        .when(expressive, |el| el.min_h(px(56.)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::MEDIUM)
                        .when(expressive, |el| el.type_style(type_scale::BODY_LARGE))
                        .child(title),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(hsla(tone))
                        .when(expressive, |el| el.type_style(type_scale::BODY_MEDIUM))
                        .child(hint),
                ),
        )
        .child(
            div()
                .flex()
                .gap(px(6.))
                .when(stored.is_some(), |el| {
                    el.child(button(
                        SharedString::from(format!("{id}-remove")),
                        t("pill.meetingsSettings.remove"),
                        cx.listener(move |v, _: &ClickEvent, _, cx| remove_key(v, kind, cx)),
                    ))
                })
                .child(button(
                    SharedString::from(format!("{id}-paste")),
                    if stored.is_some() { t("pill.meetingsSettings.replace") } else { t("pill.meetingsSettings.pasteKey") },
                    cx.listener(move |v, _: &ClickEvent, _, cx| paste_key(v, kind, cx)),
                )),
        )
}

// --- Piezas --------------------------------------------------------------------------

pub(crate) fn heading(text: &'static str) -> impl IntoElement {
    // Expressive: el título del grupo en el color primario, como las listas de M3.
    // M3: el subtítulo de lista (title-small), en el primario.
    let expressive = crate::theme::expressive();
    div()
        .pt(px(22.))
        .pb(px(8.))
        .px(px(if expressive.is_some() { 16. } else { 4. }))
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(expressive.map_or_else(|| hsla(MUTED), |t| t.accent))
        .when(expressive.is_some(), |el| el.type_style(type_scale::TITLE_SMALL))
        .child(text)
}

pub(crate) fn card() -> gpui::Div {
    // Expressive: un grupo de lista segmentada; cada fila trae su relleno y es su
    // propia baldosa (`settings::segmented_group`).
    if let Some(t) = crate::theme::expressive() {
        return crate::settings::segmented_group(div().flex().flex_col(), crate::settings::tile_bg(&t), 0.);
    }
    div()
        .py(px(4.))
        .rounded(px(16.))
        // Con Liquid Glass por piezas, cada tarjeta es un vidrio del sistema.
        .map(|el| crate::glass_pieces::panel(el.relative(), 16., crate::theme::panel_bg(SURFACE)))
        .flex()
        .flex_col()
}

pub(crate) fn row(
    title: impl Into<SharedString>,
    hint: impl Into<SharedString>,
    control: impl IntoElement,
) -> impl IntoElement {
    let title: SharedString = title.into();
    let hint: SharedString = hint.into();
    // Expressive: la baldosa de una lista de M3 (56 de alto como mínimo) y su letra.
    let expressive = crate::theme::expressive().is_some();
    div()
        .flex()
        .items_center()
        .gap(px(16.))
        .px(px(16.))
        .py(px(if expressive { 8. } else { 12. }))
        .when(expressive, |el| el.min_h(px(56.)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::MEDIUM)
                        .when(expressive, |el| el.type_style(type_scale::BODY_LARGE))
                        .child(title),
                )
                .when(!hint.is_empty(), |el| {
                    el.child(
                        div()
                            .text_size(px(12.))
                            .text_color(hsla(MUTED))
                            .when(expressive, |el| el.type_style(type_scale::BODY_MEDIUM))
                            .child(hint),
                    )
                }),
        )
        .child(control)
}

/// Opciones de una sola elección, la encendida en claro (como las pestañas).
pub(crate) fn segmented<V: ConfigPane>(
    id: &'static str,
    options: &[(&'static str, &'static str)],
    current: &str,
    apply: fn(&mut Config, &str),
    cx: &mut Context<V>,
) -> gpui::AnyElement {
    // Con estilo, los botones segmentados de gpui-m3.
    let selected = options.iter().position(|(_, value)| *value == current);
    if let (Some(_), Some(selected)) = (crate::theme::styled(), selected) {
        let values: Vec<&'static str> = options.iter().map(|(_, value)| *value).collect();
        let view = cx.entity().downgrade();
        return gpui_m3::SegmentedButtons::new(
            id,
            options.iter().map(|(label, _)| gpui_m3::Segment::label(*label)),
            selected,
        )
        .sliding(crate::theme::sliding_segments())
        .on_change(move |ix, _, cx| {
            let Some(&value) = values.get(ix) else {
                return;
            };
            let _ = view.update(cx, |v, cx| v.edit_config(cx, |cfg| apply(cfg, value)));
        })
        .into_any_element();
    }
    // La elegida es una píldora que se desliza de una opción a otra.
    let glide = crate::glide::named(format!("meetings-seg-{id}"));
    let mut group = div()
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(2.))
        .p(px(3.))
        .rounded(px(15.))
        .bg(hsla(FIELD))
        .child(crate::glide::indicator(
            glide.clone(),
            selected.map(|ix| ix as u64),
            crate::glide::Look { fill: hsla(0xe9e9e2), radius: 13., edge: None },
        ));
    for (ix, &(label, value)) in options.iter().enumerate() {
        let on = current == value;
        group = group.child(
            div()
                .id((id, ix))
                .h(px(26.))
                .px(px(12.))
                .flex()
                .items_center()
                .rounded(px(13.))
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(hsla(if on { INK } else { MUTED }))
                .child(crate::glide::mark(glide.clone(), ix as u64))
                .when(!on, |el| el.cursor_pointer())
                .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| {
                    v.edit_config(cx, |cfg| apply(cfg, value))
                }))
                .child(label)
                .fx((id, ix), move |el, h| {
                    if on {
                        el
                    } else {
                        el.text_color(h.mix(hsla(MUTED), hsla(TEXT)))
                    }
                }),
        );
    }
    group.into_any_element()
}

pub(crate) fn switch(
    id: &'static str,
    on: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    crate::settings::switch(id, on, on_click)
}

pub(crate) fn dropdown(
    id: &'static str,
    label: String,
    open: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::AnyElement {
    // Con estilo, el chip de elegir de gpui-m3: su flecha se da vuelta al abrir.
    if crate::theme::styled().is_some() {
        return gpui_m3::Chip::new(id, label)
            .open(open)
            .max_w(px(FIELD_W))
            .on_click(on_click)
            .into_any_element();
    }
    div()
        .id(id)
        .w(px(FIELD_W))
        .h(px(32.))
        .flex_none()
        .px(px(12.))
        .flex()
        .items_center()
        .gap(px(8.))
        .rounded(px(10.))
        .cursor_pointer()
        .on_click(on_click)
        .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).child(label))
        .child(
            svg()
                .path(if open { "icons/chevron-up.svg" } else { "icons/chevron-down.svg" })
                .size(px(14.))
                .text_color(hsla(MUTED)),
        )
        .hover_bg(id, hsla(FIELD), hsla(SURFACE_ON))
        .into_any_element()
}

/// Las opciones de un desplegable se abren dentro de la tarjeta, bajo su
/// fila: nada flota encima de otra cosa.
pub(crate) fn menu_list() -> gpui::Div {
    // Expressive: una baldosa más del grupo, bajo la fila del desplegable.
    if crate::theme::expressive().is_some() {
        return div().p(px(8.)).flex().flex_col().gap(px(2.));
    }
    let styled = crate::theme::styled();
    div()
        .mx(px(12.))
        .mb(px(8.))
        .p(px(4.))
        .rounded(px(styled.as_ref().map_or(12., |t| t.r_pop)))
        .bg(styled.map_or_else(|| hsla(FIELD), |t| t.control))
        .flex()
        .flex_col()
        .gap(px(2.))
}

pub(crate) fn menu_item(
    id: (&'static str, usize),
    label: String,
    on: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::AnyElement {
    if crate::theme::styled().is_some() {
        return gpui_m3::MenuItem::new(id, label).selected(on).on_click(on_click).into_any_element();
    }
    div()
        .id(id)
        .h(px(32.))
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(8.))
        .rounded(px(8.))
        .cursor_pointer()
        .on_click(on_click)
        .child(div().flex_1().min_w_0().truncate().text_size(px(13.)).child(label))
        .when(on, |el| el.child(svg().path("icons/check.svg").size(px(14.)).text_color(hsla(GREEN))))
        .hover_bg(id, hsla(FIELD), hsla(ITEM))
        .into_any_element()
}

pub(crate) fn field(input: &Entity<TextInput>) -> impl IntoElement {
    // Expressive: el campo relleno de M3, con las esquinas de arriba redondeadas
    // y la línea de abajo. Sin la ventana no se sabe si tiene el foco: la línea
    // queda fija.
    if let Some(t) = crate::theme::expressive() {
        return div()
            .w(px(FIELD_W))
            .h(px(40.))
            .flex_none()
            .px(px(12.))
            .flex()
            .items_center()
            .rounded_t(gpui_m3::corner::XS)
            .bg(crate::settings::on_tile_bg(&t))
            .border_b_1()
            .border_color(t.muted)
            .text_color(t.text)
            .type_style(type_scale::BODY_MEDIUM)
            .child(input.clone());
    }
    // Con estilo, el campo relleno de M3: su contenedor y su radio de control.
    let styled = crate::theme::styled();
    div()
        .w(px(FIELD_W))
        .h(px(if styled.is_some() { 36. } else { 32. }))
        .flex_none()
        .px(px(12.))
        .flex()
        .items_center()
        .rounded(px(styled.as_ref().map_or(10., |t| t.r_ctl)))
        .bg(styled.map_or_else(|| hsla(FIELD), |t| t.control))
        .text_size(px(13.))
        .child(input.clone())
}

/// Los motores de voz que se eligen en Ajustes (Dictado y Reuniones).
/// Una función y no una constante: el nombre cambia con el idioma.
/// Sin Parakeet (Mac Intel, `local_stt::SUPPORTED`) no se ofrece «Local».
#[cfg(target_os = "macos")]
pub(crate) fn engines() -> Vec<(&'static str, &'static str)> {
    let all = [(t("pill.meetingsSettings.macosVoice"), "apple"), ("Groq", "groq"), ("Local", "parakeet")];
    all.into_iter().filter(|(_, v)| *v != "parakeet" || crate::local_stt::SUPPORTED).collect()
}
#[cfg(not(target_os = "macos"))]
pub(crate) fn engines() -> Vec<(&'static str, &'static str)> {
    vec![("Groq", "groq"), ("Local", "parakeet")]
}

/// El motor elegido como se muestra: lo que no es Groq ni el modelo local
/// («local» de la Atic de Tauri, o sin el campo) va con la voz de macOS en Mac
/// y con Groq en Windows, como al dictar y transcribir.
pub(crate) fn engine_value(backend: &str) -> &'static str {
    match backend {
        "groq" => "groq",
        "parakeet" if crate::local_stt::SUPPORTED => "parakeet",
        _ if cfg!(target_os = "macos") => "apple",
        _ => "groq",
    }
}

pub(crate) fn engine_hint(value: &str) -> &'static str {
    match value {
        "apple" => t("pill.meetingsSettings.engineApple"),
        "parakeet" => t("pill.meetingsSettings.engineLocal"),
        _ => t("pill.meetingsSettings.engineGroq"),
    }
}

/// El modelo local (`local_stt.rs`): bajarlo, cuánto lleva o que está listo.
/// Mientras baja, la vista se repinta sola para mover el avance.
pub(crate) fn local_model_row<V: 'static>(id: &'static str, cx: &mut Context<V>) -> impl IntoElement {
    use crate::local_stt::{self, Download};
    let state = local_stt::state();
    if matches!(state, Download::Downloading { .. }) {
        cx.spawn(async move |view, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(400)).await;
            let _ = view.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }
    let start = |_: &ClickEvent, window: &mut Window, _: &mut App| {
        local_stt::start_download();
        window.refresh();
    };
    let note = |text: String, color: u32| div().text_size(px(12.)).text_color(hsla(color)).child(text).into_any_element();
    let (hint, control): (String, gpui::AnyElement) = match state {
        Download::Idle => (
            tf("pill.meetingsSettings.localIdle", &[("mb", &local_stt::DOWNLOAD_MB)]),
            button(SharedString::from(id), t("pill.meetingsSettings.downloadModel"), start),
        ),
        Download::Downloading { done, total } => {
            let pct = (done * 100 / total.max(1)).min(100);
            (tf("pill.meetingsSettings.localDownloading", &[("done", &(done / 1_000_000)), ("total", &(total / 1_000_000))]), note(format!("{pct} %"), MUTED))
        }
        Download::Ready => (t("pill.meetingsSettings.localReady").into(), note(t("pill.meetingsSettings.ready").into(), GREEN)),
        Download::Failed(error) => (error, button(SharedString::from(id), t("pill.meetingsSettings.retry"), start)),
    };
    row(t("pill.meetingsSettings.localModel"), hint, control)
}

pub(crate) fn button(
    id: SharedString,
    label: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> gpui::AnyElement {
    if crate::theme::styled().is_some() {
        return gpui_m3::Button::new(id, label)
            .tonal()
            .size(gpui_m3::ButtonSize::Small)
            .on_click(on_click)
            .into_any_element();
    }
    div()
        .id(id.clone())
        .h(px(28.))
        .px(px(12.))
        .flex()
        .items_center()
        .rounded(px(14.))
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .on_click(on_click)
        .child(label)
        .hover_bg(id, hsla(ITEM), hsla(SURFACE_ON))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_never_shown_whole() {
        assert_eq!(key_preview("gsk_abcdefghijklmnop123"), "gsk_••••123");
        assert_eq!(key_preview("corta"), "••••••");
    }

    #[test]
    fn only_key_like_clipboard_text_is_saved() {
        assert!(looks_like_key("  sk-ant-api03-abcdefghijklmnop  "));
        assert!(!looks_like_key("hola"));
        assert!(!looks_like_key("esto es un párrafo con espacios que no es una llave"));
    }

    #[test]
    fn unknown_provider_falls_back_to_claude() {
        assert_eq!(provider("nada").id, "claude");
        assert_eq!(provider("ollama").default_base_url, "http://127.0.0.1:11434");
        assert!(provider_key(provider("ollama")).is_none());
        assert_eq!(provider_key(provider("groq")), Some(SecretKind::GroqApiKey));
    }
}
