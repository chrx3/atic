//! El chat rápido del notch con Atic Code, en el panel de Agentes.
//!
//! Se elige un proyecto (los espacios de Atic Code, `kit/workspaces.rs`), se
//! escribe y la respuesta llega en vivo. Corre su propio agente: el mismo
//! sidecar de Atic Code (`code/sidecar.rs`), con la configuración de Claude de
//! ese espacio (`code/config.rs`), así funciona aunque Atic Code esté cerrado.
//! Lo que llega se vuelve filas con el mismo modelo de Atic Code
//! (`code::chat::Chat`), así el notch entiende lo mismo que la ventana.
//!
//! «Seguir en Atic Code» cierra la sesión aquí (dos procesos no escriben la
//! misma conversación) y la retoma allá por su id, con todo su contexto
//! (`CodeView::resume_session`).
//!
//! Lo que se suelta sobre el chat (desde Finder, el Explorador o el
//! Portapapeles) se adjunta como en Atic Code: las imágenes como imagen y lo
//! demás como su ruta (`code::Attachment`, `compose_message`).
//!
//! En el notch todo va resumido: el texto de Claude tal cual, cada herramienta
//! en una línea y los permisos con Permitir o Rechazar. Lo demás (diffs,
//! terminal, razonamiento) se ve siguiendo en Atic Code.

use std::path::PathBuf;
use std::sync::Arc;

use futures::StreamExt;
use base64::Engine as _;
use gpui::{
    div, img, prelude::*, px, rgb, svg, AnyElement, ClickEvent, Context, Entity, EventEmitter, ExternalPaths,
    FontWeight, Hsla, ScrollHandle, SharedString, Window,
};
use serde_json::{json, Value};

use crate::code::chat::{Chat, Item, ToolCall};
use crate::code::Attachment;
use crate::code::sidecar::{Incoming, Sidecar};
use crate::hover::HoverExt;
use crate::i18n::{t, tf};
use crate::kit::workspaces::{Workspace, Workspaces};
use crate::text_input::TextInput;

const SIDE_PAD: f32 = 8.0;
/// La fila de arriba con la conversación: el proyecto, «Nueva» y «Seguir en
/// Atic Code».
const HEAD_H: f32 = 34.0;
/// Lo que se ve de la conversación; más largo se desplaza (queda abajo).
const LOG_H: f32 = 230.0;
const COMPOSER_H: f32 = 40.0;
const COMPOSER_GAP: f32 = 4.0;
/// Una solicitud de permiso, sobre el campo de escribir.
const PERMISSION_H: f32 = 66.0;
/// Una fila de la lista de proyectos.
const PICK_ROW_H: f32 = 28.0;
const PICK_MAX: usize = 6;
/// La fila de lo adjunto, sobre el campo.
const ATTACH_H: f32 = 40.0;
/// Lo que se manda al rechazar un permiso desde el notch.
const DENIED: &str = "El usuario lo rechazó desde el notch de Atic.";

pub enum NotchChatEvent {
    /// Seguir en Atic Code: la sesión, la carpeta donde corrió y su título.
    Resume { session: String, cwd: PathBuf, title: String },
}

impl EventEmitter<NotchChatEvent> for NotchChat {}

pub struct NotchChat {
    sidecar: Option<Arc<Sidecar>>,
    chat: Option<Chat>,
    /// Para la clave de cada conversación nueva.
    seq: u64,
    workspaces: Workspaces,
    workspace: Option<u64>,
    /// La lista de proyectos abierta (solo antes de empezar a conversar).
    picking: bool,
    pub composer: Entity<TextInput>,
    /// Lo soltado sobre el chat, para el próximo mensaje.
    attachments: Vec<Attachment>,
    scroll: ScrollHandle,
    text: Hsla,
    muted: Hsla,
    faint: Hsla,
}

impl NotchChat {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let (text, muted) = (rgb(0xf0f0ea).into(), rgb(0x9a9a90).into());
        let composer = cx.new(|cx| TextInput::new(t("pill.notchChat.placeholder"), text, muted, text, cx));
        let workspaces = Workspaces::load();
        let workspace = workspaces.active_id().or_else(|| workspaces.list().first().map(|w| w.id));
        Self {
            sidecar: None,
            chat: None,
            seq: 0,
            workspaces,
            workspace,
            picking: false,
            composer,
            attachments: Vec::new(),
            scroll: ScrollHandle::new(),
            text,
            muted,
            faint: rgb(0x6e6e66).into(),
        }
    }

    /// Al abrir el panel: los proyectos se releen (Atic Code pudo crear uno).
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.workspaces = Workspaces::load();
        if self.workspace.is_none_or(|id| self.workspaces.get(id).is_none()) {
            self.workspace = self.workspaces.active_id().or_else(|| self.workspaces.list().first().map(|w| w.id));
        }
        self.picking = false;
        cx.notify();
    }

    /// Hay una conversación en el notch: el panel la muestra en vez de las listas.
    pub fn active(&self) -> bool {
        self.chat.as_ref().is_some_and(|chat| !chat.items.is_empty())
    }

    /// Atic Code está respondiendo (o espera un permiso).
    pub fn busy(&self) -> bool {
        self.chat.as_ref().is_some_and(|chat| chat.busy)
    }

    /// El alto que ocupa en el panel.
    pub fn height(&self) -> f32 {
        let attached = if self.attachments.is_empty() { 0.0 } else { ATTACH_H };
        attached + self.bare_height()
    }

    fn bare_height(&self) -> f32 {
        if self.active() {
            let permission = if self.chat.as_ref().is_some_and(|c| !c.permissions.is_empty()) { PERMISSION_H } else { 0.0 };
            HEAD_H + LOG_H + permission + COMPOSER_GAP + COMPOSER_H
        } else if self.picking {
            COMPOSER_GAP + COMPOSER_H + self.workspaces.list().len().clamp(1, PICK_MAX) as f32 * PICK_ROW_H + 6.0
        } else {
            COMPOSER_GAP + COMPOSER_H
        }
    }

    fn project(&self) -> Option<&Workspace> {
        let id = self.chat.as_ref().map(|chat| chat.workspace).or(self.workspace)?;
        self.workspaces.get(id)
    }

    /// El agente del notch: se levanta con el primer mensaje y sus eventos
    /// llegan aquí.
    fn sidecar(&mut self, cx: &mut Context<Self>) -> Arc<Sidecar> {
        if let Some(sidecar) = &self.sidecar {
            return sidecar.clone();
        }
        let (sidecar, mut events) = Sidecar::new();
        cx.spawn(async move |this, cx| {
            while let Some(incoming) = events.next().await {
                if this.update(cx, |chat, cx| chat.on_incoming(incoming, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        self.sidecar = Some(sidecar.clone());
        sidecar
    }

    fn on_incoming(&mut self, incoming: Incoming, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.as_mut() else {
            return;
        };
        match incoming {
            Incoming::Event { event, key, data } => {
                if key.as_deref() != Some(chat.key.as_str()) {
                    return;
                }
                chat.apply(&event, &data);
            }
            Incoming::Exit => {
                chat.live = false;
                chat.busy = false;
                chat.notice(t("pill.notchChat.agentExited"), true);
            }
        }
        self.scroll.scroll_to_bottom();
        cx.notify();
    }

    /// Enter o la flecha: el mensaje va a la conversación (la abre si no hay).
    pub fn send(&mut self, cx: &mut Context<Self>) {
        let typed = self.composer.read(cx).text().trim().to_string();
        if typed.is_empty() && self.attachments.is_empty() {
            return;
        }
        let Some(project) = self.project().cloned() else {
            self.picking = true;
            cx.notify();
            return;
        };
        let Some(cwd) = project.main().cloned() else {
            return;
        };
        if self.chat.is_none() {
            self.seq += 1;
            let key = format!("notch-{}-{}", std::process::id(), self.seq);
            self.chat = Some(Chat::new(key, project.id, cwd.clone()));
        }
        self.picking = false;
        self.composer.update(cx, |input, cx| input.clear(cx));
        // Las imágenes van como imagen; lo demás, con su ruta en el proyecto.
        let (mut images, mut thumbs, mut files) = (Vec::new(), Vec::new(), Vec::new());
        for attachment in self.attachments.drain(..) {
            match attachment {
                Attachment::Image { media_type, data, image, .. } => {
                    images.push(json!({ "mediaType": media_type, "data": data }));
                    thumbs.push(image);
                }
                Attachment::File(path) => files.push(crate::code::relative_to(&path, &project.folders)),
            }
        }
        let text = crate::code::compose_message(&typed, None, &files, !images.is_empty());
        let sidecar = self.sidecar(cx);
        let Some(chat) = self.chat.as_mut() else {
            return;
        };
        chat.push_user(&text, thumbs);
        let key = chat.key.clone();
        let start = (!chat.live).then(|| {
            chat.live = true;
            chat.starting = true;
            let mut params = json!({ "key": key, "cwd": cwd });
            let extras = project.extras(&cwd);
            if !extras.is_empty() {
                params["additionalDirectories"] = json!(extras);
            }
            if project.folders.len() > 1 {
                let names: Vec<String> = project.folders.iter().map(|f| f.display().to_string()).collect();
                params["appendSystemPrompt"] =
                    json!(format!("Este proyecto tiene varias carpetas; puedes leer y editar en todas:\n{}", names.join("\n")));
            }
            if let Some(session) = &chat.session_id {
                params["resume"] = json!(session);
            }
            crate::code::config::Configs::load().get(project.id).start_params(&mut params);
            params
        });
        self.scroll.scroll_to_bottom();
        cx.notify();
        cx.spawn(async move |this, cx| {
            if let Some(params) = start {
                let started = sidecar.call("start", params).await;
                if let Some(error) = failure(started) {
                    let _ = this.update(cx, |view, cx| view.fail(&key, error, cx));
                    return;
                }
            }
            let sent = sidecar.call("send", json!({ "key": key, "text": text, "images": images })).await;
            if let Some(error) = failure(sent) {
                let _ = this.update(cx, |view, cx| view.fail(&key, error, cx));
            }
        })
        .detach();
    }

    /// Lo soltado sobre el chat: cada imagen (hasta `MAX_IMAGES`) como imagen
    /// y el resto como ruta, sin repetir.
    pub fn attach(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        for path in paths {
            let images = self.attachments.iter().filter(|a| matches!(a, Attachment::Image { .. })).count();
            let image = crate::code::image_type(&path).filter(|_| images < crate::code::MAX_IMAGES).and_then(|media_type| {
                let bytes = std::fs::read(&path).ok()?;
                let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
                let image = crate::code::chat::image_from(media_type, bytes)?;
                let size = crate::code::chat::image_size(&image).unwrap_or((1., 1.));
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                Some(Attachment::Image { name, media_type, data, image, size })
            });
            let attachment = image.unwrap_or(Attachment::File(path));
            if let Attachment::File(path) = &attachment {
                if self.attachments.iter().any(|a| matches!(a, Attachment::File(p) if p == path)) {
                    continue;
                }
            }
            self.attachments.push(attachment);
        }
        cx.notify();
    }

    fn detach(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.attachments.len() {
            self.attachments.remove(index);
            cx.notify();
        }
    }

    /// Texto soltado sobre el chat: va al campo, tras lo ya escrito.
    pub fn insert_text(&mut self, text: &str, cx: &mut Context<Self>) {
        let current = self.composer.read(cx).text().to_string();
        let joined = if current.trim().is_empty() { text.to_string() } else { format!("{} {text}", current.trim_end()) };
        self.composer.update(cx, |input, cx| input.set_text(joined, cx));
        cx.notify();
    }

    fn fail(&mut self, key: &str, error: String, cx: &mut Context<Self>) {
        if let Some(chat) = self.chat.as_mut().filter(|chat| chat.key == key) {
            chat.live = false;
            chat.busy = false;
            chat.notice(error, true);
            cx.notify();
        }
    }

    fn fire(&self, method: &str, params: Value) {
        if let Some(sidecar) = &self.sidecar {
            drop(sidecar.call(method, params));
        }
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        if let Some(chat) = &self.chat {
            self.fire("interrupt", json!({ "key": chat.key }));
        }
        cx.notify();
    }

    /// Permitir o rechazar la solicitud que espera.
    fn answer(&mut self, allow: bool, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.as_mut() else {
            return;
        };
        if chat.permissions.is_empty() {
            return;
        }
        let permission = chat.permissions.remove(0);
        let result = if allow {
            json!({ "behavior": "allow", "updatedInput": permission.input })
        } else {
            json!({ "behavior": "deny", "message": DENIED })
        };
        let key = chat.key.clone();
        self.fire("permission", json!({ "key": key, "requestId": permission.request_id, "result": result }));
        cx.notify();
    }

    /// «Nueva»: la conversación del notch se cierra y queda listo para otra.
    fn new_chat(&mut self, cx: &mut Context<Self>) {
        if let Some(chat) = self.chat.take().filter(|chat| chat.live) {
            self.fire("close", json!({ "key": chat.key }));
        }
        cx.notify();
    }

    /// «Seguir en Atic Code»: allá retoma la misma sesión; aquí se cierra.
    fn resume(&mut self, cx: &mut Context<Self>) {
        let Some(chat) = self.chat.as_ref() else {
            return;
        };
        let Some(session) = chat.session_id.clone() else {
            return;
        };
        let (cwd, title) = (chat.cwd.clone(), chat.title.clone());
        self.new_chat(cx);
        cx.emit(NotchChatEvent::Resume { session, cwd, title });
    }

    fn choose(&mut self, id: u64, cx: &mut Context<Self>) {
        self.workspace = Some(id);
        self.picking = false;
        cx.notify();
    }
}

/// El error de una respuesta del agente, si fue error.
fn failure(reply: Result<crate::code::sidecar::Reply, futures::channel::oneshot::Canceled>) -> Option<String> {
    match reply {
        Ok(Ok(_)) => None,
        Ok(Err(error)) => Some(error),
        Err(_) => Some(t("pill.notchChat.agentExited").to_string()),
    }
}

/// Lo que dice una herramienta en una línea: el archivo, el comando o lo que
/// busca.
fn tool_line(call: &ToolCall) -> String {
    let input = call.input.as_ref();
    let field = |name: &str| input.and_then(|i| i.get(name)).and_then(Value::as_str).map(str::to_string);
    let path = field("file_path").or_else(|| field("path")).map(|p| {
        std::path::Path::new(&p).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(p)
    });
    let detail = path
        .or_else(|| field("command"))
        .or_else(|| field("pattern"))
        .or_else(|| field("url"))
        .or_else(|| field("description"))
        .unwrap_or_default();
    let detail: String = detail.lines().next().unwrap_or_default().chars().take(80).collect();
    detail
}

impl NotchChat {
    fn render_item(&self, row: usize, chat: &Chat) -> Option<AnyElement> {
        if chat.hidden(row) {
            return None;
        }
        let (text, muted, faint) = (self.text, self.muted, self.faint);
        let element = match chat.items.get(row)? {
            Item::User { text: said, images, .. } => div()
                .flex()
                .flex_col()
                .items_end()
                .gap(px(4.))
                .when(!images.is_empty(), |el| {
                    el.child(div().flex().gap(px(4.)).children(images.iter().map(|image| {
                        img(image.clone()).size(px(56.)).rounded(px(10.)).object_fit(gpui::ObjectFit::Cover)
                    })))
                })
                .child(
                    div()
                        .max_w(gpui::relative(0.82))
                        .px(px(11.))
                        .py(px(7.))
                        .rounded(px(14.))
                        .bg(text.opacity(0.1))
                        .text_size(px(12.5))
                        .child(said.clone()),
                )
                .into_any_element(),
            // Con formato, como en Atic Code (el mismo `Markdown` de gpui-m3).
            Item::Text(said) => {
                let id = SharedString::from(format!("notch-chat-md-{}-{row}", chat.key));
                div()
                    .text_size(px(12.5))
                    .child(gpui_m3::Markdown::new(id, said.clone()).on_link(|url, _, cx| {
                        if url.starts_with("https://") || url.starts_with("http://") {
                            cx.open_url(url);
                        }
                    }))
                    .into_any_element()
            }
            Item::Tool(call) => {
                let tone: Hsla = match (&call.result, call.is_error) {
                    (_, true) => rgb(0xe85a52).into(),
                    (None, _) => rgb(0xe8b04b).into(),
                    (Some(_), _) => rgb(0x6cc48a).into(),
                };
                div()
                    .h(px(18.))
                    .flex()
                    .items_center()
                    .gap(px(7.))
                    .text_size(px(11.))
                    .child(div().size(px(6.)).flex_none().rounded_full().bg(tone))
                    .child(div().flex_none().text_color(muted).child(call.name.clone()))
                    .child(div().flex_1().min_w_0().truncate().text_color(faint).child(tool_line(call)))
                    .into_any_element()
            }
            Item::Notice { text: said, error } => div()
                .text_size(px(11.))
                .text_color(if *error { rgb(0xe85a52).into() } else { muted })
                .child(said.clone())
                .into_any_element(),
            Item::Turn(said) => div().text_size(px(10.)).text_color(faint).child(said.clone()).into_any_element(),
            _ => return None,
        };
        Some(element)
    }

    fn render_head(&self, chat: &Chat, cx: &mut Context<Self>) -> impl IntoElement {
        let (text, muted) = (self.text, self.muted);
        let name = self.project().map(|w| w.name.clone()).unwrap_or_default();
        let can_resume = chat.session_id.is_some();
        div()
            .h(px(HEAD_H))
            .flex_none()
            .px(px(SIDE_PAD + 6.))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(11.))
                    .text_color(muted)
                    .child(SharedString::from(tf("pill.notchChat.inProject", &[("project", &name)]))),
            )
            .child(
                div()
                    .id("notch-chat-new")
                    .h(px(26.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .rounded(px(13.))
                    .text_size(px(11.))
                    .cursor_pointer()
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.new_chat(cx)))
                    .child(t("pill.notchChat.new"))
                    .fx("notch-chat-new", move |el, h| el.text_color(h.mix(muted, text)).bg(text.opacity(0.08 * h.t))),
            )
            .when(can_resume, |el| {
                el.child(crate::agents::code_button(
                    "notch-chat-resume",
                    t("pill.notchChat.continue"),
                    text,
                    cx.listener(|view, _: &ClickEvent, _, cx| view.resume(cx)),
                ))
            })
    }

    fn render_permission(&self, chat: &Chat, cx: &mut Context<Self>) -> Option<AnyElement> {
        let permission = chat.permissions.first()?;
        let (text, muted) = (self.text, self.muted);
        let detail = tool_line(&ToolCall::new(String::new(), permission.tool.clone(), Some(permission.input.clone())));
        let title = permission.title.clone().unwrap_or_else(|| tf("pill.notchChat.wantsTool", &[("tool", &permission.tool)]));
        let button = |id: &'static str, label: &'static str, primary: bool, allow: bool, cx: &mut Context<Self>| {
            let ink: Hsla = rgb(0x1a1a18).into();
            div()
                .id(id)
                .h(px(26.))
                .px(px(12.))
                .flex()
                .items_center()
                .rounded(px(13.))
                .text_size(px(11.5))
                .font_weight(FontWeight::SEMIBOLD)
                .cursor_pointer()
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.answer(allow, cx)))
                .child(label)
                .fx(id, move |el, h| {
                    if primary {
                        el.text_color(ink).bg(text.opacity(0.88 + 0.12 * h.t))
                    } else {
                        el.text_color(text).bg(text.opacity(0.08 + 0.06 * h.t))
                    }
                })
        };
        Some(
            div()
                .h(px(PERMISSION_H - 6.))
                .mt(px(6.))
                .mx(px(SIDE_PAD))
                .px(px(12.))
                .flex()
                .items_center()
                .gap(px(10.))
                .rounded(px(14.))
                .bg(Hsla::from(rgb(0xe8b04b)).opacity(0.14))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(div().truncate().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).child(title))
                        .child(div().truncate().text_size(px(11.)).text_color(muted).child(detail)),
                )
                .child(button("notch-chat-deny", t("pill.notchChat.deny"), false, false, cx))
                .child(button("notch-chat-allow", t("pill.notchChat.allow"), true, true, cx))
                .into_any_element(),
        )
    }

    fn render_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let (text, muted) = (self.text, self.muted);
        let chosen = self.workspace;
        let list = self.workspaces.list();
        if list.is_empty() {
            return div()
                .h(px(PICK_ROW_H))
                .px(px(SIDE_PAD + 8.))
                .flex()
                .items_center()
                .text_size(px(11.))
                .text_color(muted)
                .child(t("pill.notchChat.noProjects"))
                .into_any_element();
        }
        div()
            .mx(px(SIDE_PAD))
            .mt(px(6.))
            .flex()
            .flex_col()
            .children(list.iter().take(PICK_MAX).map(|w| {
                let id = w.id;
                let on = chosen == Some(id);
                let element_id = SharedString::from(format!("notch-chat-project-{id}"));
                div()
                    .id(element_id.clone())
                    .h(px(PICK_ROW_H))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .rounded(px(9.))
                    .text_size(px(12.))
                    .cursor_pointer()
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.choose(id, cx)))
                    .child(div().size(px(6.)).rounded_full().bg(text.opacity(if on { 0.9 } else { 0.0 })))
                    .child(div().flex_1().min_w_0().truncate().child(w.name.clone()))
                    .fx(element_id, move |el, h| el.text_color(if on { text } else { h.mix(muted, text) }).bg(text.opacity(0.07 * h.t)))
            }))
            .into_any_element()
    }

    /// Lo adjunto, en fichas sobre el campo: la miniatura o el nombre, y una
    /// × para quitarlo.
    fn render_attachments(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.attachments.is_empty() {
            return None;
        }
        let (text, muted) = (self.text, self.muted);
        // Las imágenes, solo su miniatura (caben varias en el ancho); los
        // archivos, con su nombre. Lo que no cabe se desplaza de lado.
        let chips = self.attachments.iter().enumerate().map(|(i, attachment)| {
            let id = SharedString::from(format!("notch-chat-attachment-{i}"));
            let remove = div()
                .id(id.clone())
                .size(px(18.))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .rounded_full()
                .cursor_pointer()
                .text_size(px(11.))
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.detach(i, cx)))
                .child("×")
                .fx(id, move |el, h| el.text_color(h.mix(muted, text)).bg(gpui::black().opacity(0.55 + 0.2 * h.t)));
            match attachment {
                Attachment::Image { image, .. } => div()
                    .relative()
                    .size(px(34.))
                    .flex_none()
                    .child(img(image.clone()).size_full().rounded(px(8.)).object_fit(gpui::ObjectFit::Cover))
                    .child(div().absolute().top(px(-4.)).right(px(-4.)).child(remove))
                    .into_any_element(),
                Attachment::File(path) => div()
                    .h(px(34.))
                    .max_w(px(150.))
                    .pl(px(8.))
                    .pr(px(4.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(6.))
                    .rounded(px(9.))
                    .bg(text.opacity(0.08))
                    .text_size(px(11.))
                    .child(svg().path("icons/file.svg").size(px(13.)).flex_none().text_color(muted))
                    .child(div().min_w_0().truncate().child(
                        path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                    ))
                    .child(remove)
                    .into_any_element(),
            }
        });
        Some(
            div()
                .id("notch-chat-attachments")
                .h(px(ATTACH_H))
                .flex_none()
                .mx(px(SIDE_PAD + 4.))
                .pt(px(4.))
                .flex()
                .items_center()
                .gap(px(12.))
                .overflow_x_scroll()
                .children(chips.collect::<Vec<_>>())
                .into_any_element(),
        )
    }

    fn render_composer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let text = self.text;
        let cream: Hsla = rgb(0xe8e8e0).into();
        let ink: Hsla = rgb(0x1a1a18).into();
        let busy = self.chat.as_ref().is_some_and(|chat| chat.busy);
        let name = self.project().map(|w| w.name.clone()).unwrap_or_else(|| t("pill.notchChat.pickProject").to_string());
        // El proyecto se elige antes de empezar; después es el de la conversación.
        let can_pick = self.chat.is_none();
        div()
            .h(px(COMPOSER_H))
            .flex_none()
            .mx(px(SIDE_PAD))
            .mt(px(COMPOSER_GAP))
            .px(px(5.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(px(COMPOSER_H / 2.))
            .bg(text.opacity(0.07))
            .child(
                div()
                    .id("notch-chat-project")
                    .h(px(30.))
                    .max_w(px(150.))
                    .pl(px(7.))
                    .pr(px(10.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(6.))
                    .rounded(px(15.))
                    .bg(cream)
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(ink)
                    .when(can_pick, |el| {
                        el.cursor_pointer().on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                            view.picking = !view.picking;
                            cx.notify();
                        }))
                    })
                    .child(crate::kit::chrome::logo(16.))
                    .child(div().min_w_0().truncate().child(name)),
            )
            .child(div().flex_1().min_w_0().text_size(px(12.)).child(self.composer.clone()))
            .child(
                div()
                    .id("notch-chat-send")
                    .size(px(30.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .rounded(px(15.))
                    .bg(cream)
                    .cursor_pointer()
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| if busy { view.stop(cx) } else { view.send(cx) }))
                    .child(if busy {
                        div().size(px(10.)).rounded(px(2.)).bg(ink).into_any_element()
                    } else {
                        svg().path("icons/arrow-up.svg").size(px(15.)).text_color(ink).into_any_element()
                    }),
            )
    }
}

impl Render for NotchChat {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let text = self.text;
        let scope = dark_m3(cx);
        // Todo el chat recibe lo que se suelta; mientras pasa encima, se aclara.
        let target = |el: gpui::Stateful<gpui::Div>, cx: &mut Context<Self>| {
            el.rounded(px(14.))
                .drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(text.opacity(0.06)))
                .on_drop(cx.listener(|view, paths: &ExternalPaths, _, cx| view.attach(paths.paths().to_vec(), cx)))
        };
        let Some(chat) = self.chat.as_ref().filter(|_| self.active()) else {
            let idle = div()
                .id("notch-chat")
                .flex_none()
                .flex()
                .flex_col()
                .children(self.render_attachments(cx))
                .child(self.render_composer(cx))
                .when(self.picking, |el| el.child(self.render_picker(cx)));
            return target(idle, cx).child(scope).into_any_element();
        };
        let rows: Vec<AnyElement> = (0..chat.items.len()).filter_map(|row| self.render_item(row, chat)).collect();
        let working = chat.working();
        let faint = self.faint;
        let open = div()
            .id("notch-chat")
            .flex_none()
            .flex()
            .flex_col()
            .child(self.render_head(chat, cx))
            .child(
                div()
                    .id("notch-chat-log")
                    .h(px(LOG_H))
                    .flex_none()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .px(px(SIDE_PAD + 6.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .children(rows)
                    .when(working, |el| el.child(div().text_size(px(11.)).text_color(faint).child(t("pill.notchChat.working")))),
            )
            .children(self.render_permission(chat, cx))
            .children(self.render_attachments(cx))
            .child(self.render_composer(cx));
        target(open, cx).child(scope).into_any_element()
    }
}

/// El notch es oscuro siempre: mientras se dibuja el chat, el `Markdown` de
/// gpui-m3 usa su esquema oscuro (si no, tomaría el de Atic Code, que en tema
/// claro deja el texto oscuro sobre la piel). Pintada al final, la capa deja
/// el de antes, como `theme::scope_m3`.
fn dark_m3(cx: &mut gpui::App) -> AnyElement {
    use gpui::{canvas, deferred};
    let previous = gpui_m3::Theme::get(cx);
    cx.set_global(gpui_m3::Theme { scheme: gpui_m3::Scheme::baseline_dark(), dark: true, seed: None });
    let restore = canvas(|_, _, _| {}, move |_, _, _, cx| cx.set_global(previous));
    deferred(restore).with_priority(usize::MAX).into_any_element()
}
