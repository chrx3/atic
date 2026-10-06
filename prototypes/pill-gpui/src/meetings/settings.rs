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
use crate::hover::{self, HoverExt};
use crate::text_input::{self, TextInput};

actions!(meetings_settings, [CloseSettings]);

const KEY_CONTEXT: &str = "MeetingsSettings";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", CloseSettings, Some(KEY_CONTEXT))]);
}

/// Un campo de texto más oscuro que la tarjeta, como un pozo.
const FIELD: u32 = 0x161615;
const FIELD_W: f32 = 300.0;
const SWITCH_OFF: u32 = 0x3a3a37;

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
        "custom" => "Compatible con OpenAI",
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
        return (fallback, false, Some("Pega la llave para ver sus modelos.".into()));
    }
    match atic_summarize::list_remote_models(p.kind, p.id, base, key.as_deref()) {
        Ok(list) if !list.is_empty() => (atic_summarize::order_models(list, p.default_model), true, None),
        Ok(_) => (fallback, false, Some("El proveedor no devolvió modelos.".into())),
        Err(error) => (fallback, false, Some(format!("No se pudo pedir la lista: {error}"))),
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
        let model = field("Modelo", &cfg.summary_model, cx);
        let base_url = field("https://…", &cfg.summary_base_url, cx);
        let smtp_host = field("smtp.ejemplo.cl", &cfg.smtp_host, cx);
        let smtp_port = field("587", &cfg.smtp_port.to_string(), cx);
        let smtp_user = field("usuario", &cfg.smtp_username, cx);
        let smtp_from = field("Nombre <correo@ejemplo.cl>", &cfg.smtp_from, cx);

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

        let mut view = Self {
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
        ("El portapapeles no tiene una llave.".to_string(), false)
    } else {
        match secrets::set_secret(kind, text.trim()) {
            Ok(()) => ("Llave guardada.".to_string(), true),
            Err(error) => (format!("No se pudo guardar: {error}"), false),
        }
    };
    view.set_key_notice((kind, message, ok), cx);
}

fn remove_key<V: ConfigPane>(view: &mut V, kind: SecretKind, cx: &mut Context<V>) {
    let (message, ok) = match secrets::delete_secret(kind) {
        Ok(()) => ("Llave quitada.".to_string(), true),
        Err(error) => (format!("No se pudo quitar: {error}"), false),
    };
    view.set_key_notice((kind, message, ok), cx);
}

impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let cfg = &self.cfg;
        let p = provider(&cfg.summary_backend);

        // --- Grabación
        let mic_label = if cfg.mic_device_id.is_empty() {
            "Predeterminado del sistema".to_string()
        } else {
            self.devices
                .as_ref()
                .and_then(|list| list.iter().find(|d| d.id == cfg.mic_device_id))
                .map(|d| d.name.clone())
                .unwrap_or_else(|| "Micrófono guardado".into())
        };
        let mut recording = card()
            .child(row(
                "Qué grabar",
                "Tu micrófono, el audio del computador o ambos.",
                segmented(
                    "tracks",
                    &[("Ambos", "both"), ("Micrófono", "mic"), ("Sistema", "system")],
                    &cfg.record_tracks,
                    |cfg, v| cfg.record_tracks = v.into(),
                    cx,
                ),
            ))
            .child(row(
                "Micrófono",
                "",
                dropdown("mic-menu", mic_label, self.menu == Some(Menu::Mic), cx.listener(
                    |v, _: &ClickEvent, _, cx| {
                        v.menu = if v.menu == Some(Menu::Mic) { None } else { Some(Menu::Mic) };
                        cx.notify();
                    },
                )),
            ));
        if self.menu == Some(Menu::Mic) {
            let mut options = vec![(String::new(), "Predeterminado del sistema".to_string())];
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
                menu = menu.child(div().px(px(12.)).py(px(8.)).text_size(px(12.)).text_color(hsla(MUTED)).child("Buscando micrófonos…"));
            }
            recording = recording.child(menu);
        }
        let recording = recording
            .child(row(
                "Reducir ruido",
                "Limpia el micrófono mientras grabas.",
                segmented(
                    "noise",
                    &[("No", "off"), ("Bajo", "low"), ("Medio", "medium"), ("Alto", "high")],
                    &cfg.noise_suppression,
                    |cfg, v| cfg.noise_suppression = v.into(),
                    cx,
                ),
            ))
            .child(row(
                "Subtítulos en vivo",
                "Una vista previa mientras grabas. Usa Groq y no se guarda.",
                switch("live", cfg.live_transcription, cx.listener(|v, _: &ClickEvent, _, cx| {
                    v.update(cx, |cfg| {
                        cfg.live_transcription = !cfg.live_transcription;
                        cfg.live_engine = "groq".into();
                    })
                })),
            ))
            .child(row(
                "Sonido al empezar",
                "Un aviso corto para que los demás sepan que grabas.",
                switch("beep", cfg.beep_on_start, cx.listener(|v, _: &ClickEvent, _, cx| {
                    v.update(cx, |cfg| cfg.beep_on_start = !cfg.beep_on_start)
                })),
            ));

        // --- Transcripción
        let transcription = card()
            .child(row(
                "Transcribir al terminar",
                "Apenas detienes la grabación.",
                switch("auto", cfg.auto_transcribe_after_recording, cx.listener(
                    |v, _: &ClickEvent, _, cx| {
                        v.update(cx, |cfg| {
                            cfg.auto_transcribe_after_recording = !cfg.auto_transcribe_after_recording
                        })
                    },
                )),
            ))
            .child(row(
                "Precisión",
                "Turbo es casi igual de bueno y bastante más rápido.",
                segmented(
                    "groq-model",
                    &[("Turbo", "whisper-large-v3-turbo"), ("Máxima", "whisper-large-v3")],
                    &cfg.meeting_groq_model,
                    |cfg, v| {
                        cfg.meeting_groq_model = v.into();
                        cfg.meeting_backend = "groq".into();
                    },
                    cx,
                ),
            ))
            .child(row(
                "Idioma",
                "",
                segmented(
                    "language",
                    &[("Del sistema", "system"), ("Detectar", "auto"), ("Español", "es"), ("Inglés", "en")],
                    &cfg.language,
                    |cfg, v| cfg.language = v.into(),
                    cx,
                ),
            ))
            .child(key_row(self, "Llave de Groq", SecretKind::GroqApiKey, cx));

        // --- Resumen
        let mut summary = card().child(row(
            "Proveedor",
            "Con tu propia llave. Ollama corre en tu computador.",
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
            "Buscando modelos…".into()
        } else if let Some(problem) = &self.models.problem {
            problem.clone().into()
        } else if self.models.live {
            format!("{} disponibles en {}", self.models.list.len(), provider_name(p)).into()
        } else {
            "".into()
        };
        let model_label = if cfg.summary_model.trim().is_empty() {
            "Elige un modelo".to_string()
        } else {
            cfg.summary_model.clone()
        };
        summary = summary.child(row(
            "Modelo",
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
                    format!("{current} · no está en la lista"),
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
                        .child(div().text_size(px(12.)).text_color(hsla(MUTED)).child("Otro"))
                        .child(div().flex_1().child(field(&self.model))),
                ),
            );
        }
        if p.base_url_editable {
            summary = summary.child(row("Dirección", "", field(&self.base_url)));
        }
        if let Some(kind) = provider_key(p) {
            summary = summary.child(key_row(self, "Llave", kind, cx));
        }

        // --- Correo
        let smtp = cfg.mail_backend == "smtp";
        let mut mail = card().child(row(
            "Cómo se envía",
            if smtp { "Atic lo manda con tu servidor de correo." } else { "Se abre tu correo con el resumen listo." },
            segmented(
                "mail",
                &[("Abrir mi correo", "mailto"), ("Servidor SMTP", "smtp")],
                &cfg.mail_backend,
                |cfg, v| cfg.mail_backend = v.into(),
                cx,
            ),
        ));
        if smtp {
            mail = mail
                .child(row("Servidor", "", field(&self.smtp_host)))
                .child(row("Puerto", "", field(&self.smtp_port)))
                .child(row("Usuario", "", field(&self.smtp_user)))
                .child(row("Remitente", "", field(&self.smtp_from)))
                .child(row(
                    "Conexión segura",
                    "TLS. Déjala encendida salvo que tu servidor no la tenga.",
                    switch("tls", cfg.smtp_use_tls, cx.listener(|v, _: &ClickEvent, _, cx| {
                        v.update(cx, |cfg| cfg.smtp_use_tls = !cfg.smtp_use_tls)
                    })),
                ))
                .child(key_row(self, "Contraseña", SecretKind::SmtpPassword, cx));
        }

        let sections = div()
            .flex()
            .flex_col()
            .child(heading("Grabación"))
            .child(recording)
            .child(heading("Transcripción"))
            .child(transcription)
            .child(heading("Resumen"))
            .child(summary)
            .child(heading("Correo"))
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
                                    .text_size(px(20.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Ajustes de reuniones"),
                            )
                            .child(hover::round_button(
                                "settings-close",
                                "icons/x.svg",
                                "Cerrar · Esc",
                                false,
                                hsla(TEXT),
                                hsla(MUTED),
                                cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(SettingsEvent::Close)),
                            )),
                    )
                    .child(
                        div()
                            .pb(px(8.))
                            .text_size(px(13.))
                            .text_color(hsla(MUTED))
                            .child("Se comparten con Atic. Cada cambio se guarda al tiro."),
                    )
                    .child(sections),
            )
    }
}

/// Una llave del llavero: si está, su vista previa; pegarla o quitarla.
pub(crate) fn key_row<V: ConfigPane>(
    view: &V,
    title: &'static str,
    kind: SecretKind,
    cx: &mut Context<V>,
) -> impl IntoElement {
    let stored = secrets::get_secret(kind).ok().flatten();
    let notice = view.key_notice();
    let hint: SharedString = match (notice, &stored) {
        (Some((k, message, _)), _) if *k == kind => message.clone().into(),
        (_, Some(key)) => format!("Guardada en el llavero · {}", key_preview(key)).into(),
        (_, None) => "Copia la llave y pégala aquí.".into(),
    };
    let tone = match notice {
        Some((k, _, ok)) if *k == kind => if *ok { GREEN } else { RED },
        _ => MUTED,
    };
    let id = kind.as_str();
    div()
        .flex()
        .items_center()
        .gap(px(16.))
        .px(px(16.))
        .py(px(12.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(title))
                .child(div().text_size(px(12.)).text_color(hsla(tone)).child(hint)),
        )
        .child(
            div()
                .flex()
                .gap(px(6.))
                .when(stored.is_some(), |el| {
                    el.child(button(
                        SharedString::from(format!("{id}-remove")),
                        "Quitar",
                        cx.listener(move |v, _: &ClickEvent, _, cx| remove_key(v, kind, cx)),
                    ))
                })
                .child(button(
                    SharedString::from(format!("{id}-paste")),
                    if stored.is_some() { "Reemplazar" } else { "Pegar llave" },
                    cx.listener(move |v, _: &ClickEvent, _, cx| paste_key(v, kind, cx)),
                )),
        )
}

// --- Piezas --------------------------------------------------------------------------

pub(crate) fn heading(text: &'static str) -> impl IntoElement {
    div()
        .pt(px(22.))
        .pb(px(8.))
        .px(px(4.))
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(hsla(MUTED))
        .child(text)
}

pub(crate) fn card() -> gpui::Div {
    div().py(px(4.)).rounded(px(16.)).bg(hsla(SURFACE)).flex().flex_col()
}

pub(crate) fn row(title: &'static str, hint: impl Into<SharedString>, control: impl IntoElement) -> impl IntoElement {
    let hint: SharedString = hint.into();
    div()
        .flex()
        .items_center()
        .gap(px(16.))
        .px(px(16.))
        .py(px(12.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(title))
                .when(!hint.is_empty(), |el| {
                    el.child(div().text_size(px(12.)).text_color(hsla(MUTED)).child(hint))
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
) -> impl IntoElement {
    let mut group = div().flex().flex_none().items_center().gap(px(2.)).p(px(3.)).rounded(px(15.)).bg(hsla(FIELD));
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
                .when(on, |el| el.bg(hsla(0xe9e9e2)))
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
    group
}

pub(crate) fn switch(
    id: &'static str,
    on: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .w(px(36.))
        .h(px(22.))
        .flex_none()
        .p(px(3.))
        .flex()
        .when(on, |el| el.justify_end())
        .rounded_full()
        .cursor_pointer()
        .on_click(on_click)
        .child(div().size(px(16.)).rounded_full().bg(hsla(if on { INK } else { MUTED })))
        .fx(id, move |el, h| {
            let (rest, over) = if on { (0xe9e9e2, 0xffffff) } else { (SWITCH_OFF, 0x46463f) };
            el.bg(h.mix(hsla(rest), hsla(over)))
        })
}

pub(crate) fn dropdown(
    id: &'static str,
    label: String,
    open: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
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
}

/// Las opciones de un desplegable se abren dentro de la tarjeta, bajo su
/// fila: nada flota encima de otra cosa.
pub(crate) fn menu_list() -> gpui::Div {
    div()
        .mx(px(12.))
        .mb(px(8.))
        .p(px(4.))
        .rounded(px(12.))
        .bg(hsla(FIELD))
        .flex()
        .flex_col()
        .gap(px(2.))
}

pub(crate) fn menu_item(
    id: (&'static str, usize),
    label: String,
    on: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
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
}

pub(crate) fn field(input: &Entity<TextInput>) -> impl IntoElement {
    div()
        .w(px(FIELD_W))
        .h(px(32.))
        .flex_none()
        .px(px(12.))
        .flex()
        .items_center()
        .rounded(px(10.))
        .bg(hsla(FIELD))
        .text_size(px(13.))
        .child(input.clone())
}

pub(crate) fn button(
    id: SharedString,
    label: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
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
