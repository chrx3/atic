//! Las conversaciones de Atic Code vistas desde la bandeja del notch (`agents.rs`).
//!
//! El notch lee las sesiones de los JSONL de Claude (y los rollouts de Codex),
//! y las de Atic Code también salen ahí: el `claude` que lanza el sidecar escribe
//! en el mismo `~/.claude/projects`. Una sesión cuyo id es el de una conversación
//! de Atic Code no tiene terminal que traer al frente: «Ver» abre esa
//! conversación, «Aceptar» la da por leída y el mensaje rápido se le manda ahí.
//!
//! La ventana se registra al abrirse (`register`); cerrada, no hay conversaciones
//! que alcanzar y el notch hace lo de siempre.

use std::path::PathBuf;

use gpui::{App, Context, Global, Window, WindowHandle};

use crate::kit::workspaces::same;

use super::chat::Chat;
use super::CodeView;

/// La ventana de Atic Code mientras está abierta.
struct OpenCode(WindowHandle<CodeView>);

impl Global for OpenCode {}

pub(super) fn register(handle: WindowHandle<CodeView>, cx: &mut App) {
    cx.set_global(OpenCode(handle));
}

fn handle(cx: &App) -> Option<WindowHandle<CodeView>> {
    cx.try_global::<OpenCode>().map(|open| open.0)
}

/// Una sesión de agente que vive en una conversación de Atic Code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatLink {
    /// El id de la sesión: el nombre del JSONL de Claude o la sesión del agente ACP.
    pub session: String,
    /// La conversación en Atic Code (su clave en el sidecar).
    pub key: String,
    /// Se está mirando: la ventana al frente con esa conversación activa.
    pub in_view: bool,
    /// Lo que el celular muestra de ella (`phone::set_agents`).
    pub title: String,
    pub cwd: PathBuf,
    /// Terminó un turno mientras no se miraba.
    pub unread: bool,
    pub busy: bool,
    pub model: Option<String>,
    /// El agente ACP (`opencode`, `gemini`…); `None` es Claude.
    pub acp: Option<&'static str>,
    /// La delegó otra conversación (`atic_delegate`): solo ella le escribe.
    pub subagent: bool,
    /// La sesión de la conversación que la delegó, si está abierta y ya tiene una.
    pub parent: Option<String>,
    /// El rol con que se le delegó.
    pub role: Option<String>,
    /// Su modo de permisos (el de Claude, o el del agente ACP), si se sabe.
    pub mode: Option<String>,
    /// El esfuerzo elegido; `None`: el predeterminado.
    pub effort: Option<String>,
    /// Su proyecto (el id del espacio); `None` en un chat suelto.
    pub workspace: Option<u64>,
}

/// La primera sesión de una conversación: la que el celular usa como su tarjeta.
fn first_session(chat: &Chat) -> Option<&str> {
    [chat.session_id.as_deref(), chat.acp_session.as_deref()].into_iter().flatten().find(|session| !session.is_empty())
}

/// Las sesiones de las conversaciones, con su clave. Una conversación sin sesión
/// todavía (no ha respondido nada) no aparece en el notch y no se enlaza.
fn links_of(chats: &[Chat], active: Option<&str>, window_active: bool) -> Vec<ChatLink> {
    chats
        .iter()
        .flat_map(|chat| {
            let in_view = window_active && active == Some(chat.key.as_str());
            let delegation = chat.delegation.as_ref();
            let parent = delegation.and_then(|d| chats.iter().find(|c| c.key == d.parent)).and_then(first_session).map(str::to_string);
            let role = delegation.and_then(|d| d.role.clone()).filter(|r| !r.trim().is_empty());
            [chat.session_id.as_deref(), chat.acp_session.as_deref()]
                .into_iter()
                .flatten()
                .filter(|session| !session.is_empty())
                .map(move |session| ChatLink {
                    session: session.to_string(),
                    key: chat.key.clone(),
                    in_view,
                    title: chat.title.clone(),
                    cwd: chat.cwd.clone(),
                    unread: chat.unread,
                    busy: chat.busy,
                    model: chat.model.clone(),
                    acp: chat.acp,
                    subagent: delegation.is_some(),
                    parent: parent.clone(),
                    role: role.clone(),
                    mode: None,
                    effort: None,
                    workspace: (chat.workspace != super::LOOSE).then_some(chat.workspace),
                })
        })
        .collect()
}

/// La conversación de Atic Code de esa sesión, si la hay.
pub fn link_for<'a>(links: &'a [ChatLink], session: &str) -> Option<&'a ChatLink> {
    if session.is_empty() {
        return None;
    }
    links.iter().find(|link| link.session == session)
}

/// Las sesiones de Atic Code ahora mismo; vacío con la ventana cerrada.
pub fn chat_links(cx: &App) -> Vec<ChatLink> {
    let Some(handle) = handle(cx) else {
        return Vec::new();
    };
    handle
        .read_with(cx, |view, _| {
            let mut links = links_of(&view.chats, view.active.as_deref(), view.window_active);
            for link in &mut links {
                if let Some(chat) = view.chats.iter().find(|c| c.key == link.key) {
                    let (mode, effort) = view.mode_and_effort(chat);
                    link.mode = mode;
                    link.effort = effort;
                }
            }
            links
        })
        .unwrap_or_default()
}

/// Una conversación de un proyecto, como la ve el celular (`phone::reply_projects`).
#[derive(Clone, Debug, PartialEq)]
pub struct PhoneSession {
    pub id: String,
    pub title: String,
    /// La última actividad, en milisegundos desde 1970.
    pub updated_ms: Option<i64>,
    pub branch: Option<String>,
    pub open: bool,
    pub busy: bool,
    pub unread: bool,
    pub agent: Option<String>,
}

/// Un proyecto de Atic Code para el celular.
#[derive(Clone, Debug, PartialEq)]
pub struct PhoneProject {
    pub id: u64,
    pub name: String,
    pub root: Option<PathBuf>,
    pub sessions: Vec<PhoneSession>,
    /// La ventana no tiene su historial (cerrada o todavía cargándolo): las guardadas
    /// se leen del disco (`saved_on_disk`) fuera del hilo de la ventana.
    pub from_disk: bool,
}

/// Lo que el celular necesita para su lista de proyectos y el menú de un chat nuevo.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PhoneProjects {
    pub projects: Vec<PhoneProject>,
    /// Los agentes con que se puede crear un chat (CLI y nombre), Claude primero.
    pub agents: Vec<(String, String)>,
    /// Los modelos de Claude (id y nombre; el id vacío es el predeterminado).
    pub models: Vec<(String, String)>,
}

/// Segundos o milisegundos (los dos llegan del sidecar) a milisegundos.
fn to_ms(modified: f64) -> i64 {
    if modified < 1e12 {
        (modified * 1000.0) as i64
    } else {
        modified as i64
    }
}

/// Las conversaciones de un espacio para el celular: las abiertas (con sesión) y las
/// guardadas, sin repetir y lo más nuevo primero.
fn sessions_of(chats: &[Chat], saved: &[super::SessionInfo], workspace: u64) -> Vec<PhoneSession> {
    let mut out: Vec<PhoneSession> = chats
        .iter()
        .filter(|c| c.workspace == workspace)
        .filter_map(|chat| {
            let id = first_session(chat)?.to_string();
            let saved = saved.iter().find(|s| s.session_id == id);
            Some(PhoneSession {
                id,
                title: chat.title.clone(),
                updated_ms: saved.and_then(|s| s.modified).map(to_ms),
                branch: saved.and_then(|s| s.branch.clone()),
                open: true,
                busy: chat.busy,
                unread: chat.unread,
                agent: Some(chat.acp.unwrap_or("claude").to_string()),
            })
        })
        .collect();
    for info in saved {
        if out.iter().any(|s| s.id == info.session_id) {
            continue;
        }
        out.push(PhoneSession {
            id: info.session_id.clone(),
            title: info.title.clone(),
            updated_ms: info.modified.map(to_ms),
            branch: info.branch.clone(),
            open: false,
            busy: false,
            unread: false,
            agent: Some("claude".into()),
        });
    }
    // Las abiertas sin fecha van arriba: se están usando.
    out.sort_by_key(|s| std::cmp::Reverse(if s.open && s.updated_ms.is_none() { i64::MAX } else { s.updated_ms.unwrap_or(0) }));
    out
}

/// Los proyectos de Atic Code con sus conversaciones. Con la ventana cerrada, los
/// espacios guardados sin conversaciones (el historial lo lee el sidecar de la
/// ventana); con ella abierta, pide el historial que falte para la próxima vez.
pub fn projects_for_phone(cx: &mut App) -> PhoneProjects {
    let claude = ("claude".to_string(), crate::agents::AGENTS.iter().find(|a| a.cli == "claude").map_or("Claude", |a| a.name).to_string());
    let Some(handle) = handle(cx) else {
        let workspaces = crate::kit::workspaces::Workspaces::load();
        return PhoneProjects {
            projects: workspaces
                .list()
                .iter()
                .map(|w| PhoneProject { id: w.id, name: w.name.clone(), root: w.main().cloned(), sessions: Vec::new(), from_disk: true })
                .collect(),
            agents: vec![claude],
            models: super::config::MODELS.iter().map(|(id, name)| (id.to_string(), name.to_string())).collect(),
        };
    };
    handle
        .update(cx, |view, _, cx| {
            let missing: Vec<u64> = view
                .workspaces
                .list()
                .iter()
                .filter(|w| !view.history.contains_key(&w.id) && !view.history_loading.contains(&w.id))
                .map(|w| w.id)
                .collect();
            for workspace in missing {
                view.load_history_for(workspace, cx);
            }
            let projects = view
                .workspaces
                .list()
                .iter()
                .map(|w| PhoneProject {
                    id: w.id,
                    name: w.name.clone(),
                    root: w.main().cloned(),
                    sessions: sessions_of(&view.chats, view.history.get(&w.id).map(Vec::as_slice).unwrap_or_default(), w.id),
                    from_disk: !view.history.contains_key(&w.id),
                })
                .collect();
            let mut agents = vec![claude];
            for cli in view.visible_acp() {
                let name = crate::agents::AGENTS.iter().find(|a| a.cli == cli).map_or(cli, |a| a.name);
                agents.push((cli.to_string(), name.to_string()));
            }
            let models = if view.models.is_empty() {
                super::config::MODELS.iter().map(|(id, name)| (id.to_string(), name.to_string())).collect()
            } else {
                view.models.clone()
            };
            PhoneProjects { projects, agents, models }
        })
        .unwrap_or_default()
}

/// Un chat nuevo desde el celular en el proyecto `workspace`, con su primer mensaje:
/// queda en la barra de Atic Code sin quitarle la vista a lo que se está mirando.
/// Devuelve su clave (la que vuelve en la tarjeta como `chat_key`). La ventana tiene
/// que estar abierta (`main.rs` la abre).
pub fn new_chat_from_phone(
    cx: &mut App,
    workspace: u64,
    agent: Option<&str>,
    model: Option<String>,
    text: &str,
    images: Vec<(String, Vec<u8>)>,
) -> Result<String, String> {
    let handle = handle(cx).ok_or_else(|| crate::i18n::t("pill.phone.code.closed").to_string())?;
    let (blocks, thumbs) = image_blocks(images);
    if blocks.is_empty() && text.trim().is_empty() {
        return Err(crate::i18n::t("pill.phone.code.empty").into());
    }
    handle
        .update(cx, |view, _, cx| {
            if view.workspaces.get(workspace).is_none() {
                return Err(crate::i18n::t("pill.phone.code.noProject").to_string());
            }
            let cwd = view.workspace_dir(workspace).ok_or_else(|| crate::i18n::t("pill.phone.code.noProject").to_string())?;
            let acp = match agent.filter(|a| *a != "claude") {
                None => None,
                Some(cli) => Some(view.acp_installed.iter().copied().find(|c| *c == cli).ok_or_else(|| crate::i18n::t("pill.phone.code.noAgent").to_string())?),
            };
            let key = view.new_key();
            let mut chat = Chat::new(key.clone(), workspace, cwd);
            chat.acp = acp;
            if let Some(draft) = acp.and_then(|cli| view.acp_drafts.get(cli)) {
                chat.acp_cfg = draft.clone();
            }
            if acp.is_none() {
                chat.want_model = model.filter(|m| !m.is_empty());
            }
            view.chats.push(chat);
            view.dispatch_message(key.clone(), text.to_string(), blocks, thumbs, cx);
            cx.notify();
            Ok(key)
        })
        .unwrap_or_else(|_| Err(crate::i18n::t("pill.phone.code.closed").into()))
}

/// El modelo (vacío: el predeterminado) y el esfuerzo de una conversación, desde el
/// celular. En un agente ACP, el modelo es su opción de modelo.
pub fn set_model_from_phone(cx: &mut App, key: &str, model: &str, effort: Option<&str>) -> Result<(), String> {
    let handle = handle(cx).ok_or_else(|| crate::i18n::t("pill.phone.code.closed").to_string())?;
    handle
        .update(cx, |view, _, cx| {
            let chat = view.chats.iter_mut().find(|c| c.key == key).ok_or_else(|| crate::i18n::t("pill.phone.code.gone").to_string())?;
            if chat.acp.is_some() {
                let option = chat
                    .acp_cfg
                    .options
                    .iter()
                    .find(|o| o.category == "model" && o.choices.iter().any(|c| c.value == model))
                    .map(|o| o.id.clone())
                    .ok_or_else(|| crate::i18n::t("pill.phone.code.noModel").to_string())?;
                chat.acp_cfg.set(&option, model);
                let live = chat.live;
                if live {
                    view.fire("setConfig", serde_json::json!({ "key": key, "configId": option, "value": model }), cx);
                }
                cx.notify();
                return Ok(());
            }
            if let Some(effort) = effort.filter(|e| !e.is_empty() && !super::config::EFFORTS.iter().any(|(id, _)| id == e)) {
                return Err(crate::i18n::tf("pill.phone.code.badEffort", &[("effort", &effort)]));
            }
            chat.want_model = Some(model.to_string());
            if let Some(effort) = effort {
                chat.want_effort = Some(effort.to_string());
            }
            view.sync_chat_settings(key, cx);
            cx.notify();
            Ok(())
        })
        .unwrap_or_else(|_| Err(crate::i18n::t("pill.phone.code.closed").into()))
}

/// El modo de una conversación, desde el celular. En Claude el modo de permisos es uno
/// para todos los proyectos (como al cambiarlo en el PC); en un agente ACP, el suyo.
/// Quien llama ya decidió si `bypassPermissions` se permite (`phone.rs`).
pub fn set_mode_from_phone(cx: &mut App, key: &str, mode: &str) -> Result<(), String> {
    let handle = handle(cx).ok_or_else(|| crate::i18n::t("pill.phone.code.closed").to_string())?;
    handle
        .update(cx, |view, _, cx| {
            let chat = view.chats.iter_mut().find(|c| c.key == key).ok_or_else(|| crate::i18n::t("pill.phone.code.gone").to_string())?;
            if chat.acp.is_some() {
                if !chat.acp_cfg.modes.iter().any(|m| m.0 == mode) {
                    return Err(crate::i18n::t("pill.phone.code.noMode").into());
                }
                let option = chat.acp_cfg.mode_config.clone();
                match &option {
                    Some(id) => chat.acp_cfg.set(id, mode),
                    None => chat.acp_cfg.mode = Some(mode.to_string()),
                }
                if chat.live {
                    match option {
                        Some(id) => view.fire("setConfig", serde_json::json!({ "key": key, "configId": id, "value": mode }), cx),
                        None => view.fire("setMode", serde_json::json!({ "key": key, "modeId": mode }), cx),
                    }
                }
                cx.notify();
                return Ok(());
            }
            if chat.delegation.is_some() {
                return Err(crate::i18n::t("pill.phone.code.subagentMode").into());
            }
            if !super::config::MODES.iter().any(|(id, _)| *id == mode) {
                return Err(crate::i18n::t("pill.phone.code.noMode").into());
            }
            let workspace = chat.workspace;
            view.apply_permission_mode(workspace, mode, cx);
            Ok(())
        })
        .unwrap_or_else(|_| Err(crate::i18n::t("pill.phone.code.closed").into()))
}

/// Una conversación guardada que se le escribe desde el celular: se retoma en Atic
/// Code sin quitarle la vista a lo que se mira (queda en la barra con su historial) y,
/// cuando el historial ya está, sale el mensaje. La ventana tiene que estar abierta.
pub fn resume_from_phone(cx: &mut App, workspace: u64, session: String, title: String, text: String, images: Vec<(String, Vec<u8>)>) -> Result<(), String> {
    let handle = handle(cx).ok_or_else(|| crate::i18n::t("pill.phone.code.closed").to_string())?;
    let (blocks, thumbs) = image_blocks(images);
    if blocks.is_empty() && text.trim().is_empty() {
        return Err(crate::i18n::t("pill.phone.code.empty").into());
    }
    handle
        .update(cx, |view, _, cx| {
            if view.workspaces.get(workspace).is_none() {
                return Err(crate::i18n::t("pill.phone.code.noProject").to_string());
            }
            let wanted = session.clone();
            let send: Box<dyn FnOnce(&mut CodeView, &mut Context<CodeView>)> = Box::new(move |view, cx| {
                let key = view.chats.iter().find(|c| c.session_id.as_deref() == Some(wanted.as_str())).map(|c| c.key.clone());
                if let Some(key) = key {
                    view.send_from_outside(&key, text, blocks, thumbs, cx);
                }
            });
            let info = super::SessionInfo { session_id: session, title, modified: None, branch: None };
            view.attach_session(workspace, info, Some(send), cx).map(|_| ()).ok_or_else(|| crate::i18n::t("pill.phone.code.noProject").to_string())
        })
        .unwrap_or_else(|_| Err(crate::i18n::t("pill.phone.code.closed").into()))
}

/// Las conversaciones de Claude guardadas en `dir`, leídas directo de
/// `~/.claude/projects` (como `listSessions` del sidecar, más simple): para el
/// celular cuando la ventana está cerrada o todavía no carga el historial. Lo más
/// nuevo primero; como mucho `limit`. Lee el disco: fuera del hilo de la ventana.
pub fn saved_on_disk(dir: &std::path::Path, limit: usize) -> Vec<PhoneSession> {
    let Some(root) = crate::agents::claude_root() else {
        return Vec::new();
    };
    let folder = root.join(claude_folder(dir));
    let Ok(read) = std::fs::read_dir(&folder) else {
        return Vec::new();
    };
    let mut files: Vec<(std::path::PathBuf, i64)> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .filter_map(|p| {
            let modified = std::fs::metadata(&p).and_then(|m| m.modified()).ok()?;
            let ms = modified.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis() as i64;
            Some((p, ms))
        })
        .collect();
    files.sort_by_key(|(_, ms)| std::cmp::Reverse(*ms));
    files
        .into_iter()
        .filter_map(|(path, ms)| {
            let id = path.file_stem()?.to_str()?.to_string();
            let (title, branch) = saved_title(&path)?;
            Some(PhoneSession { id, title, updated_ms: Some(ms), branch, open: false, busy: false, unread: false, agent: Some("claude".into()) })
        })
        .take(limit)
        .collect()
}

/// La carpeta de `~/.claude/projects` de un directorio: Claude Code cambia todo lo
/// que no es letra o número ASCII por `-` (`C:\a\b` → `C--a-b`).
fn claude_folder(dir: &std::path::Path) -> String {
    dir.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

/// El título de una conversación guardada (el puesto a mano, el resumen o el primer
/// mensaje) y su rama. `None` si no tiene ningún mensaje de la persona (se abrió y
/// no se usó).
fn saved_title(path: &std::path::Path) -> Option<(String, Option<String>)> {
    use std::io::{Read, Seek, SeekFrom};
    const HEAD: u64 = 512 * 1024;
    const TAIL: u64 = 64 * 1024;
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let mut head = Vec::new();
    (&mut file).take(HEAD).read_to_end(&mut head).ok()?;
    let mut tail = Vec::new();
    if len > HEAD {
        file.seek(SeekFrom::Start(len.saturating_sub(TAIL).max(HEAD))).ok()?;
        file.read_to_end(&mut tail).ok()?;
    }
    let mut custom = None;
    let mut summary = None;
    let mut first = None;
    let mut branch = None;
    for line in head.split(|b| *b == b'\n').chain(tail.split(|b| *b == b'\n')) {
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(line) else {
            continue;
        };
        let text = |name: &str| v.get(name).and_then(serde_json::Value::as_str).map(str::trim).filter(|t| !t.is_empty()).map(str::to_string);
        if branch.is_none() {
            branch = text("gitBranch");
        }
        match v.get("type").and_then(serde_json::Value::as_str) {
            Some("custom-title") => custom = text("customTitle").or(custom),
            Some("summary") => summary = summary.or_else(|| text("summary")),
            Some("user") if first.is_none() && !v.get("isMeta").and_then(serde_json::Value::as_bool).unwrap_or(false) => {
                first = first_prompt(v.pointer("/message/content"));
            }
            _ => {}
        }
    }
    let title = custom.or(summary).or_else(|| first.clone())?;
    let title = title.lines().next().unwrap_or_default().chars().take(120).collect();
    Some((title, branch))
}

/// El texto de un mensaje de la persona, sin los envoltorios de comandos y avisos
/// (`<command-name>`, `<local-command-stdout>`, «Caveat: …»).
fn first_prompt(content: Option<&serde_json::Value>) -> Option<String> {
    let text = match content? {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(blocks) => blocks.iter().find_map(|b| {
            (b.get("type").and_then(serde_json::Value::as_str) == Some("text")).then(|| b.get("text").and_then(serde_json::Value::as_str)).flatten().map(str::to_string)
        })?,
        _ => return None,
    };
    let text = text.trim();
    (!text.is_empty() && !text.starts_with('<') && !text.starts_with("Caveat:") && !text.starts_with("[Request interrupted")).then(|| text.to_string())
}

/// Las imágenes de un mensaje como bloques para el agente y miniaturas para la burbuja.
fn image_blocks(images: Vec<(String, Vec<u8>)>) -> (Vec<serde_json::Value>, Vec<std::sync::Arc<gpui::Image>>) {
    use base64::Engine;
    let mut blocks = Vec::new();
    let mut thumbs = Vec::new();
    for (mime, data) in images {
        // La burbuja muestra una miniatura con la proporción de la imagen, no la
        // imagen entera (`image_from`).
        let Some(thumb) = super::chat::image_from(&mime, data.clone()) else {
            continue;
        };
        blocks.push(serde_json::json!({ "mediaType": mime, "data": base64::engine::general_purpose::STANDARD.encode(&data) }));
        thumbs.push(thumb);
    }
    (blocks, thumbs)
}

/// «Aceptar» en la bandeja: la conversación queda leída.
pub fn mark_read(cx: &mut App, key: &str) {
    let Some(handle) = handle(cx) else {
        return;
    };
    let _ = handle.update(cx, |view, _, cx| {
        if let Some(chat) = view.chats.iter_mut().find(|c| c.key == key) {
            chat.unread = false;
            cx.notify();
        }
    });
}

/// El mensaje rápido del notch en una conversación: sale ya o, con un turno en
/// curso, queda en su cola como si se hubiera escrito ahí. Dice si llegó.
pub fn send_to_chat(cx: &mut App, key: &str, text: &str) -> bool {
    send_with_images(cx, key, text, Vec::new())
}

/// Lo mismo con imágenes (`mime` y bytes), como si se hubieran pegado en la
/// conversación: van al agente como imágenes y se ven en la burbuja.
pub fn send_with_images(cx: &mut App, key: &str, text: &str, images: Vec<(String, Vec<u8>)>) -> bool {
    let Some(handle) = handle(cx) else {
        return false;
    };
    let (blocks, thumbs) = image_blocks(images);
    // Ninguna imagen se pudo leer y no hay texto: no sale un mensaje vacío.
    if blocks.is_empty() && text.trim().is_empty() {
        return false;
    }
    handle
        .update(cx, |view, _, cx| view.send_from_outside(key, text.to_string(), blocks, thumbs, cx))
        .unwrap_or(false)
}

/// «Detener» desde el celular: corta el turno en curso de esa conversación.
pub fn interrupt(cx: &mut App, key: &str) -> bool {
    let Some(handle) = handle(cx) else {
        return false;
    };
    handle.update(cx, |view, _, cx| view.interrupt_chat(key, cx)).unwrap_or(false)
}

impl CodeView {
    /// La ↗ de una sesión de Claude en el notch: se retoma aquí y no en una consola.
    /// Va al espacio que arranca en esa carpeta (la sesión se busca por la carpeta
    /// donde corrió) o crea uno con ella.
    pub(crate) fn resume_session(&mut self, session: String, cwd: PathBuf, title: String, window: &mut Window, cx: &mut Context<Self>) {
        let found = self
            .workspaces
            .list()
            .iter()
            .find(|w| w.main().is_some_and(|main| same(main, &cwd)))
            .map(|w| w.id);
        let workspace = found.unwrap_or_else(|| self.workspaces.create(vec![cwd]));
        self.select_workspace(workspace, cx);
        let info = super::SessionInfo { session_id: session, title, modified: None, branch: None };
        self.open_session(workspace, info, window, cx);
    }

    /// Una sesión nueva lista para escribir (desde el lanzador): en ese
    /// espacio, en el activo o, sin ninguno, un chat suelto.
    pub(crate) fn start_session(&mut self, workspace: Option<u64>, window: &mut Window, cx: &mut Context<Self>) {
        let workspace = workspace
            .filter(|id| self.workspaces.get(*id).is_some())
            .or_else(|| self.workspaces.active().map(|w| w.id));
        match workspace {
            Some(id) => {
                self.history_page = false;
                self.new_chat(id, window, cx);
            }
            None => self.new_loose_chat(window, cx),
        }
    }

    /// El modo y el esfuerzo de una conversación, para el celular: el modo de permisos de
    /// Claude (el de su delegación, si es un subagente) o el del agente ACP.
    fn mode_and_effort(&self, chat: &Chat) -> (Option<String>, Option<String>) {
        let config = self.configs.get(chat.workspace);
        let mode = match chat.acp {
            Some(_) => chat.acp_cfg.mode.clone(),
            None => Some(chat.delegation.as_ref().and_then(|d| d.permission_mode.clone()).unwrap_or_else(|| config.permission_mode.clone())),
        };
        let effort = chat.acp.is_none().then(|| chat.chosen_effort(&config)).filter(|e| !e.is_empty());
        (mode.filter(|m| !m.is_empty()), effort)
    }

    /// Cambia el modo de permisos de Claude (uno para todos los proyectos) y lo aplica a
    /// las conversaciones abiertas, como `update_config` al cambiarlo en el PC.
    fn apply_permission_mode(&mut self, workspace: u64, mode: &str, cx: &mut Context<Self>) {
        let before = self.configs.get(workspace);
        if before.permission_mode == mode {
            return;
        }
        let mut after = before;
        after.permission_mode = mode.to_string();
        self.configs.set(workspace, after);
        let live: Vec<String> = self.chats.iter().filter(|c| c.live && c.acp.is_none() && c.delegation.is_none()).map(|c| c.key.clone()).collect();
        for key in live {
            self.fire("setPermissionMode", serde_json::json!({ "key": key, "mode": mode }), cx);
        }
        cx.notify();
    }

    fn send_from_outside(
        &mut self,
        key: &str,
        text: String,
        images: Vec<serde_json::Value>,
        thumbs: Vec<std::sync::Arc<gpui::Image>>,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(chat) = self.chats.iter_mut().find(|c| c.key == key) else {
            return false;
        };
        chat.unread = false;
        if chat.busy {
            chat.enqueue(text, images, thumbs);
        } else {
            self.dispatch_message(key.to_string(), text, images, thumbs, cx);
        }
        cx.notify();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn chat(key: &str, session: Option<&str>, acp: Option<&str>) -> Chat {
        let mut chat = Chat::new(key.into(), 1, PathBuf::from("."));
        chat.session_id = session.map(str::to_string);
        chat.acp_session = acp.map(str::to_string);
        chat
    }

    #[test]
    fn la_sesion_lleva_a_su_conversacion() {
        let chats = [
            chat("c1", Some("aaa"), None),
            chat("c2", None, None),
            chat("c3", Some(""), Some("thread-9")),
        ];
        let links = links_of(&chats, Some("c1"), true);
        assert_eq!(links.len(), 2);
        let found = link_for(&links, "aaa").unwrap();
        assert_eq!((found.key.as_str(), found.in_view), ("c1", true));
        // La sesión del agente ACP también enlaza; la vacía, no.
        assert_eq!(link_for(&links, "thread-9").map(|l| l.key.as_str()), Some("c3"));
        assert!(link_for(&links, "").is_none());
        // Una sesión de una terminal cualquiera no es de Atic Code.
        assert!(link_for(&links, "bbb").is_none());
    }

    #[test]
    fn a_la_vista_solo_con_la_ventana_al_frente() {
        let chats = [chat("c1", Some("aaa"), None), chat("c2", Some("bbb"), None)];
        let behind = links_of(&chats, Some("c1"), false);
        assert!(behind.iter().all(|l| !l.in_view));
        let front = links_of(&chats, Some("c1"), true);
        assert!(link_for(&front, "aaa").unwrap().in_view);
        assert!(!link_for(&front, "bbb").unwrap().in_view);
    }

    #[test]
    fn el_celular_ve_las_abiertas_y_las_guardadas_sin_repetir() {
        let mut open = chat("c1", Some("aaa"), None);
        open.title = "Abierta".into();
        open.busy = true;
        let other = Chat::new("c2".into(), 2, PathBuf::from("."));
        let saved = |id: &str, modified: f64| super::super::SessionInfo { session_id: id.into(), title: format!("Guardada {id}"), modified: Some(modified), branch: Some("dev".into()) };
        // `aaa` está abierta y guardada: una sola vez, con la fecha guardada (segundos a ms).
        let sessions = sessions_of(&[open, other], &[saved("aaa", 1_700_000_000.0), saved("bbb", 1_800_000_000_000.0)], 1);
        let ids: Vec<_> = sessions.iter().map(|s| (s.id.as_str(), s.open, s.updated_ms)).collect();
        assert_eq!(ids, vec![("bbb", false, Some(1_800_000_000_000)), ("aaa", true, Some(1_700_000_000_000))]);
        assert_eq!((sessions[1].title.as_str(), sessions[1].busy), ("Abierta", true));
    }

    #[test]
    fn la_carpeta_de_claude_cambia_lo_que_no_es_letra_por_guion() {
        assert_eq!(claude_folder(std::path::Path::new(r"C:\Users\Lenovo\Documents\atic")), "C--Users-Lenovo-Documents-atic");
        assert_eq!(claude_folder(std::path::Path::new("/home/a/mi_repo.v2")), "-home-a-mi-repo-v2");
    }

    #[test]
    fn el_titulo_guardado_prefiere_el_puesto_a_mano_y_salta_los_comandos() {
        let dir = std::env::temp_dir().join(format!("atic-saved-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s1.jsonl");
        let lines = [
            r#"{"type":"user","isMeta":true,"message":{"content":"Caveat: nada"}}"#,
            r#"{"type":"user","gitBranch":"dev","message":{"content":"<command-name>/clear</command-name>"}}"#,
            r#"{"type":"user","message":{"content":[{"type":"text","text":"Arregla el login\ncon detalle"}]}}"#,
        ];
        std::fs::write(&path, lines.join("\n")).unwrap();
        assert_eq!(saved_title(&path), Some(("Arregla el login".to_string(), Some("dev".to_string()))));
        std::fs::write(&path, format!("{}\n{}", lines.join("\n"), r#"{"type":"custom-title","customTitle":"Login"}"#)).unwrap();
        assert_eq!(saved_title(&path).map(|t| t.0), Some("Login".to_string()));
        std::fs::write(&path, lines[..2].join("\n")).unwrap();
        assert_eq!(saved_title(&path), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
