//! Servidor HTTP del hub en `127.0.0.1:<efímero>` con token.
//!
//! Por qué std y no `tiny_http`: el traspaso lo deja a elección siempre que el
//! parser sea de verdad (línea de request, headers, `Content-Length`, nada de
//! `chunked`). Con std no entra ninguna dependencia nueva al desktop y la
//! regla «sin tokio ni `rmcp` en `atic-desktop`» se cumple sola.
//!
//! Un request = una respuesta JSON; nada de streaming. Las esperas largas
//! (`delegate` / `prompt` / `wait`) bloquean el hilo del request, por eso cada
//! conexión se atiende en su propio hilo.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use atic_core::MutexExt;
use tauri::{AppHandle, Manager};

use super::api::{self, HubError, Outcome, OutcomeStatus};
use super::graph;
use super::{set_running, set_stopped, HubState};

/// Cuerpo máximo aceptado: los recados son texto, no archivos.
const MAX_BODY: usize = 1024 * 1024;

/// Flag de vida del aceptador **en marcha**, para que `stop` lo apague.
///
/// Es un `Arc` por aceptador y no un `AtomicBool` global a propósito: los
/// tests levantan varios aceptadores a la vez y con un flag compartido el
/// primero que termina le corta el `accept` a los demás.
static ACEPTADOR: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);
/// Puerto del listener vivo, para despertarlo al parar.
static PUERTO: Mutex<Option<u16>> = Mutex::new(None);
/// Ruta de `hub.json` para borrarla en `stop` sin cruzar al estado de `mod`.
static HUB_JSON: Mutex<Option<std::path::PathBuf>> = Mutex::new(None);

/// Disponibilidad cacheada: `agent_backends()` lanza un proceso por backend y
/// no se puede llamar en cada request.
static DISPONIBLES: Mutex<Option<(Instant, Vec<api::AgentInfo>)>> = Mutex::new(None);
const CACHE_TTL: Duration = Duration::from_secs(60);

/// Arranca el hub: bind, token, `hub.json`, hilo aceptador y refresco de la
/// cache en background.
pub fn start(app: AppHandle) -> Result<(), String> {
    let listener =
        TcpListener::bind("127.0.0.1:0").map_err(|e| format!("no se pudo abrir el hub: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("no se pudo leer el puerto del hub: {e}"))?
        .port();
    let token = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let dirs = app
        .try_state::<crate::state::AppState>()
        .map(|s| s.dirs.clone())
        .ok_or_else(|| "la app no está lista".to_string())?;
    let hub_json = serde_json::json!({
        "port": port,
        "token": token,
        "pid": std::process::id(),
        "version": env!("CARGO_PKG_VERSION"),
    });
    std::fs::write(dirs.hub_path(), hub_json.to_string())
        .map_err(|e| format!("no se pudo escribir hub.json: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dirs.hub_path(), std::fs::Permissions::from_mode(0o600));
    }
    set_running(HubState {
        port,
        token: token.clone(),
        mcp_path: super::mcp_path(),
        hub_json: dirs.hub_path(),
    });
    let vivo = Arc::new(AtomicBool::new(true));
    *ACEPTADOR.lock_or_recover() = Some(vivo.clone());
    *PUERTO.lock_or_recover() = Some(port);
    *HUB_JSON.lock_or_recover() = Some(dirs.hub_path());
    // El refresco no puede ir delante de `accept`: los primeros segundos el
    // sidecar vería Atic cerrado. Cache vieja (o vacía) hasta que termine.
    std::thread::spawn(|| {
        let _ = refrescar_disponibles();
    });
    let app_hilo = app.clone();
    std::thread::spawn(move || {
        aceptar(listener, token, Some(app_hilo), vivo);
    });
    Ok(())
}

/// Para el hub: deja de aceptar, borra `hub.json` (best-effort) y limpia el estado.
pub fn stop() {
    if let Some(vivo) = ACEPTADOR.lock_or_recover().take() {
        vivo.store(false, Ordering::SeqCst);
    }
    // Despierta el `accept` con una conexión propia; el hilo ve el flag y sale.
    if let Some(port) = *PUERTO.lock_or_recover() {
        let _ = TcpStream::connect_timeout(
            &format!("127.0.0.1:{port}").parse().unwrap(),
            Duration::from_millis(500),
        );
    }
    *PUERTO.lock_or_recover() = None;
    if let Some(ruta) = HUB_JSON.lock_or_recover().take() {
        let _ = std::fs::remove_file(ruta);
    }
    set_stopped();
}

fn aceptar(listener: TcpListener, token: String, app: Option<AppHandle>, vivo: Arc<AtomicBool>) {
    for conn in listener.incoming() {
        if !vivo.load(Ordering::SeqCst) {
            break;
        }
        let Ok(conn) = conn else { continue };
        let token = token.clone();
        let app = app.clone();
        // Cada request en su hilo: las esperas bloquean minutos.
        std::thread::spawn(move || atender(conn, &token, app.as_ref()));
    }
}

fn atender(mut conn: TcpStream, token: &str, app: Option<&AppHandle>) {
    let _ = conn.set_read_timeout(Some(Duration::from_secs(30)));
    let resp = match leer_pedido(&conn) {
        Ok(pedido) => rutear(pedido, token, app),
        Err(msg) => no_hay_forma(400, HubError::nueva("bad_request", msg)),
    };
    let _ = conn.write_all(&resp);
}

struct Pedido {
    metodo: String,
    ruta: String,
    query: HashMap<String, String>,
    headers: HashMap<String, String>,
    cuerpo: Vec<u8>,
}

fn leer_pedido(conn: &TcpStream) -> Result<Pedido, String> {
    let mut lector = BufReader::new(conn);
    let mut primera = String::new();
    lector
        .read_line(&mut primera)
        .map_err(|_| "no se pudo leer el pedido".to_string())?;
    let partes: Vec<&str> = primera.trim_end().splitn(3, ' ').collect();
    if partes.len() != 3 {
        return Err("línea de request inválida".to_string());
    }
    let (ruta, query) = partir_query(partes[1]);
    let mut headers = HashMap::new();
    loop {
        let mut linea = String::new();
        lector
            .read_line(&mut linea)
            .map_err(|_| "no se pudo leer los headers".to_string())?;
        let linea = linea.trim_end();
        if linea.is_empty() {
            break;
        }
        if let Some((k, v)) = linea.split_once(':') {
            headers.insert(k.trim().to_lowercase(), v.trim().to_string());
        }
    }
    let largo: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if largo > MAX_BODY {
        return Err("cuerpo demasiado grande".to_string());
    }
    let mut cuerpo = vec![0u8; largo];
    if largo > 0 {
        lector
            .read_exact(&mut cuerpo)
            .map_err(|_| "no se pudo leer el cuerpo".to_string())?;
    }
    Ok(Pedido {
        metodo: partes[0].to_string(),
        ruta,
        query,
        headers,
        cuerpo,
    })
}

fn partir_query(ruta: &str) -> (String, HashMap<String, String>) {
    let mut query = HashMap::new();
    let (base, resto) = match ruta.split_once('?') {
        Some((b, r)) => (b, r),
        None => (ruta, ""),
    };
    for par in resto.split('&').filter(|p| !p.is_empty()) {
        if let Some((k, v)) = par.split_once('=') {
            query.insert(k.to_string(), v.to_string());
        }
    }
    (base.to_string(), query)
}

fn rutear(pedido: Pedido, token: &str, app: Option<&AppHandle>) -> Vec<u8> {
    let autorizado = pedido
        .headers
        .get("authorization")
        .is_some_and(|v| v == &format!("Bearer {token}"));
    if !autorizado {
        return no_hay_forma(
            401,
            HubError::nueva(
                "unauthorized",
                "Falta el token del hub. Si ves esto, Atic se reinició: vuelve a llamar.".into(),
            ),
        );
    }
    let empezo = Instant::now();
    match (pedido.metodo.as_str(), pedido.ruta.as_str()) {
        ("GET", "/v1/health") => hay_forma(serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "pid": std::process::id(),
        })),
        ("GET", "/v1/agents") => hay_forma(serde_json::json!({ "agents": disponibles() })),
        ("GET", "/v1/sessions") => {
            let todas = super::super::bridge::live_sessions();
            let filtradas: Vec<api::SessionInfo> = todas
                .iter()
                .filter(|s| pedido.query.get("backend").is_none_or(|b| b == &s.backend))
                .filter_map(|s| super::super::bridge::session_info(&s.id))
                .collect();
            hay_forma(serde_json::json!({ "sessions": filtradas }))
        }
        ("POST", "/v1/spawn") => spawn(pedido, app),
        ("POST", "/v1/prompt") => prompt(pedido, app, empezo),
        ("POST", "/v1/delegate") => delegate(pedido, app, empezo),
        ("POST", "/v1/wait") => wait(pedido, empezo),
        ("POST", "/v1/cancel") => cancel(pedido),
        ("POST", "/v1/rename") => rename(pedido),
        _ => no_hay_forma(
            404,
            HubError::nueva("bad_request", "Ruta desconocida.".into()),
        ),
    }
}

fn cuerpo<T: serde::de::DeserializeOwned>(pedido: &Pedido) -> Result<T, Vec<u8>> {
    serde_json::from_slice(&pedido.cuerpo).map_err(|_| {
        no_hay_forma(
            400,
            HubError::nueva(
                "bad_request",
                "El JSON viene roto o le falta un campo.".into(),
            ),
        )
    })
}

/// De un id **o** un nombre a la sesión concreta.
///
/// El id gana: un nombre que coincida con un uuid no puede secuestrar la
/// sesión de ese uuid. `Err` ya trae la respuesta HTTP: no encontrada, o
/// ambigua con la lista de candidatas, que es lo que el que pregunta necesita
/// para desambiguar él.
fn resolver_sesion(que: &str) -> Result<String, Vec<u8>> {
    if super::super::bridge::session_info(que).is_some() {
        return Ok(que.to_string());
    }
    match super::super::bridge::session_by_label(que) {
        Ok(Some(id)) => Ok(id),
        Ok(None) => Err(no_hay_forma(
            404,
            HubError::nueva(
                "unknown_session",
                format!("No hay ninguna sesión en Atic con id ni nombre «{que}». Llama atic_list_sessions."),
            ),
        )),
        Err(candidatas) => Err(no_hay_forma(
            409,
            HubError::con_datos(
                "ambiguous_session",
                format!("Hay {} sesiones llamadas «{que}». Usa el id.", candidatas.len()),
                serde_json::json!({ "sessions": candidatas }),
            ),
        )),
    }
}

/// Quién pregunta, en legible, para sellar el mensaje que le llega al agente.
fn quien_pide(from: Option<&str>) -> Option<String> {
    let from = from?.trim();
    if from.is_empty() {
        return None;
    }
    // Una app de fuera: `external:<host>:<pid>`.
    if let Some(resto) = from.strip_prefix("external:") {
        let host = resto.split(':').next().unwrap_or_default();
        return Some(nombre_lindo(host).to_string());
    }
    // Otra sesión de Atic: su nombre si lo tiene, y si no su backend.
    let info = super::super::bridge::session_info(from)?;
    let backend = nombre_lindo(&info.backend).to_string();
    Some(
        match info.label.as_deref().filter(|l| !l.trim().is_empty()) {
            Some(label) => format!("{backend} · {label}"),
            None => backend,
        },
    )
}

fn es_backend_conocido(id: &str) -> bool {
    super::super::bridge::backend_ids().contains(&id)
}

fn nombre_lindo(id: &str) -> &str {
    super::super::bridge::backend_display_name(id).unwrap_or(id)
}

/// Disponibilidad con TTL. Si venció, sirve lo último y refresca en otro hilo.
fn disponibles() -> Vec<api::AgentInfo> {
    let mut vencida = false;
    let lista = {
        let guard = DISPONIBLES.lock_or_recover();
        match guard.as_ref() {
            Some((cuando, lista)) => {
                if cuando.elapsed() >= CACHE_TTL {
                    vencida = true;
                }
                lista.clone()
            }
            None => {
                vencida = true;
                Vec::new()
            }
        }
    };
    if vencida {
        pedir_refresco();
    }
    lista
}

static REFRESCANDO: AtomicBool = AtomicBool::new(false);

fn pedir_refresco() {
    if REFRESCANDO
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        std::thread::spawn(|| {
            let _ = refrescar_disponibles();
            REFRESCANDO.store(false, Ordering::SeqCst);
        });
    }
}

fn refrescar_disponibles() -> Vec<api::AgentInfo> {
    let lista: Vec<api::AgentInfo> = super::super::bridge::backend_availability()
        .into_iter()
        .map(|b| api::AgentInfo {
            blurb: blurb(&b.id).to_string(),
            id: b.id,
            name: b.display_name,
            available: b.available,
            signed_in: b.signed_in,
        })
        .collect();
    *DISPONIBLES.lock_or_recover() = Some((Instant::now(), lista.clone()));
    lista
}

fn blurb(id: &str) -> &str {
    match id {
        "claude-code" => {
            "Planes largos, repo completo, tools propias. El único que a su vez puede delegar."
        }
        "codex" => "Parches y reviews acotados. Tarda unos 8 s en abrir.",
        "cursor" => "Aplica cambios con el CLI cursor-agent, no con el IDE.",
        "opencode" => "Generalista liviano, vía ACP.",
        "grok" => "Vía ACP, con su propio TUI aparte. Contexto largo.",
        "antigravity" => "El sucesor del Gemini CLI; trae navegador y subagentes.",
        _ => "",
    }
}

/// ¿Se le puede encargar trabajo a este agente?
///
/// Instalado y —cuando se sabe mirar— con sesión iniciada. Un `None` en
/// `signed_in` pasa: significa que en ese backend no sabemos comprobarlo, y
/// frenar por no saber dejaría sin delegación a quien sí puede trabajar.
fn utilizable(a: &api::AgentInfo, id: &str) -> bool {
    a.id == id && a.available && a.signed_in != Some(false)
}

/// Revisa disponibilidad con un único reintento fresco antes de rendirse.
fn exigir_disponible(id: &str) -> Result<(), Vec<u8>> {
    if disponibles().iter().any(|a| utilizable(a, id)) {
        return Ok(());
    }
    let fresca = refrescar_disponibles();
    if fresca.iter().any(|a| utilizable(a, id)) {
        return Ok(());
    }
    let encontrado = fresca.iter().find(|a| a.id == id);
    let nombre = encontrado
        .map(|a| a.name.clone())
        .unwrap_or_else(|| id.to_string());
    // Instalado pero sin login es el otro motivo por el que un backend no
    // sirve, y se arregla distinto: decir «no está instalado» manda a quien
    // delega a instalar algo que ya tiene.
    let motivo = match encontrado.and_then(|a| a.signed_in) {
        Some(false) => format!(
            "{nombre} está instalado pero sin sesión iniciada. Ábrelo una vez en una consola y haz login."
        ),
        _ => format!(
            "{nombre} no está instalado en este equipo. Llama atic_list_agents para ver cuáles hay."
        ),
    };
    Err(no_hay_forma(
        409,
        HubError::nueva("backend_unavailable", motivo),
    ))
}

fn validar_modo(mode: &Option<String>) -> Result<(), Vec<u8>> {
    let Some(m) = mode.as_deref() else {
        return Ok(());
    };
    if matches!(
        m,
        "default" | "acceptEdits" | "plan" | "bypassPermissions" | "dontAsk"
    ) {
        Ok(())
    } else {
        Err(no_hay_forma(
            400,
            HubError::nueva(
                "bad_request",
                format!(
                    "permissionMode desconocido: {m}. Usa default, acceptEdits, plan, bypassPermissions o dontAsk."
                ),
            ),
        ))
    }
}

fn spawn(pedido: Pedido, app: Option<&AppHandle>) -> Vec<u8> {
    let req: api::SpawnRequest = match cuerpo(&pedido) {
        Ok(r) => r,
        Err(e) => return e,
    };
    if !es_backend_conocido(&req.backend) {
        return no_hay_forma(
            400,
            HubError::nueva(
                "unknown_backend",
                format!("Backend desconocido: {}.", req.backend),
            ),
        );
    }
    if let Err(e) = validar_modo(&req.permission_mode) {
        return e;
    }
    let Some(app) = app else {
        return no_hay_forma(
            500,
            HubError::nueva("bad_request", "El hub no está listo.".into()),
        );
    };
    if let Err(e) = exigir_disponible(&req.backend) {
        return e;
    }
    // El grafo distingue local vs SSH; el CLI padre ya va en `parent`.
    let host = "local".to_string();
    let vivas = super::super::bridge::live_sessions();
    let cadena: Vec<(String, String)> = vivas
        .iter()
        .filter(|s| s.root.as_deref() == req.root.as_deref())
        .map(|s| (s.backend.clone(), s.cwd_key.clone()))
        .collect();
    let cwd_clave = graph::cwd_key(req.cwd.as_deref().unwrap_or(""));
    if let Err(e) = graph::check_spawn(
        graph::SpawnCheck {
            backend: &req.backend,
            host: &host,
            cwd_key: &cwd_clave,
            depth: req.depth,
            root: req.root.as_deref(),
            parent: req.parent.as_deref(),
        },
        &vivas,
        &cadena,
    ) {
        return no_hay_forma(409, e);
    }
    match super::super::bridge::start_session(
        app,
        &req.backend,
        super::super::bridge::StartRequest {
            cwd: req.cwd.clone(),
            remote_host_id: None,
            resume: None,
            model: req.model.clone(),
            effort: None,
            fast: None,
            permission_mode: req.permission_mode.clone(),
            mcp_config: None,
            add_dirs: Vec::new(),
            fork: false,
        },
        super::super::bridge::SpawnMeta {
            parent: req.parent.clone(),
            depth: req.depth.saturating_add(1),
            root: req.root.clone(),
            label: req.label.clone(),
        },
    ) {
        Ok(session) => hay_forma(serde_json::json!({ "session": session })),
        Err(msg) => no_hay_forma(409, HubError::nueva("spawn_failed", msg)),
    }
}

fn delegate(pedido: Pedido, app: Option<&AppHandle>, empezo: Instant) -> Vec<u8> {
    let req: api::DelegateRequest = match cuerpo(&pedido) {
        Ok(r) => r,
        Err(e) => return e,
    };
    if let Err(e) = validar_modo(&req.permission_mode) {
        return e;
    }
    let Some(app) = app else {
        return no_hay_forma(
            500,
            HubError::nueva("bad_request", "El hub no está listo.".into()),
        );
    };
    // Backend concreto o `"auto"` por `kind`: nunca las dos cosas ni prosa.
    let backend = if req.backend == "auto" {
        let vigentes = disponibles();
        let refs: Vec<(&str, bool)> = vigentes
            .iter()
            .map(|a| (a.id.as_str(), a.available))
            .collect();
        match graph::route(req.kind, &refs) {
            Some(b) => b.to_string(),
            None => {
                return no_hay_forma(
                    409,
                    HubError::nueva(
                        "backend_unavailable",
                        "Ningún agente está instalado en este equipo.".into(),
                    ),
                );
            }
        }
    } else {
        if !es_backend_conocido(&req.backend) {
            return no_hay_forma(
                400,
                HubError::nueva(
                    "unknown_backend",
                    format!("Backend desconocido: {}.", req.backend),
                ),
            );
        }
        req.backend.clone()
    };
    if let Err(e) = exigir_disponible(&backend) {
        return e;
    }
    let espera_s = graph::clamp_wait(req.wait_s.unwrap_or(graph::HUB_WAIT_MAX_S));
    // El grafo distingue local vs SSH; el CLI padre ya va en `parent`.
    let host = "local".to_string();
    let vivas = super::super::bridge::live_sessions();
    let cadena: Vec<(String, String)> = vivas
        .iter()
        .filter(|s| s.root.as_deref() == req.root.as_deref())
        .map(|s| (s.backend.clone(), s.cwd_key.clone()))
        .collect();
    let cwd_clave = graph::cwd_key(req.cwd.as_deref().unwrap_or(""));
    if let Err(e) = graph::check_spawn(
        graph::SpawnCheck {
            backend: &backend,
            host: &host,
            cwd_key: &cwd_clave,
            depth: req.depth,
            root: req.root.as_deref(),
            parent: req.parent.as_deref(),
        },
        &vivas,
        &cadena,
    ) {
        return no_hay_forma(409, e);
    }
    let session = match super::super::bridge::start_session(
        app,
        &backend,
        super::super::bridge::StartRequest {
            cwd: req.cwd.clone(),
            remote_host_id: None,
            resume: None,
            model: req.model.clone(),
            effort: None,
            fast: None,
            permission_mode: req.permission_mode.clone(),
            mcp_config: None,
            add_dirs: Vec::new(),
            fork: false,
        },
        super::super::bridge::SpawnMeta {
            parent: req.parent.clone(),
            depth: req.depth.saturating_add(1),
            root: req.root.clone(),
            label: req.label.clone(),
        },
    ) {
        Ok(s) => s,
        Err(msg) => return no_hay_forma(409, HubError::nueva("spawn_failed", msg)),
    };
    let backend_nombre = nombre_lindo(&backend).to_string();
    // Si el `send` falla, la sesión queda viva igual: se devuelve el fallo
    // como resultado, no como error HTTP, para no botar el `session`.
    if let Err(msg) = super::super::bridge::send_text(&session, &req.text, None) {
        return hay_forma(termino(
            &session,
            &backend,
            super::wait::WaitOutcome::Ended {
                status: super::super::model::TurnStatus::Failed,
                text: msg,
            },
            &backend_nombre,
            empezo,
        ));
    }
    let salida = esperar(&session, espera_s);
    hay_forma(termino(&session, &backend, salida, &backend_nombre, empezo))
}

fn prompt(pedido: Pedido, app: Option<&AppHandle>, empezo: Instant) -> Vec<u8> {
    let req: api::PromptRequest = match cuerpo(&pedido) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let _ = app;
    let sesion = match resolver_sesion(&req.session) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(info) = super::super::bridge::session_info(&sesion) else {
        return no_hay_forma(
            404,
            HubError::nueva("unknown_session", "Esa sesión ya no existe en Atic.".into()),
        );
    };
    let espera_s = graph::clamp_wait(req.wait_s.unwrap_or(graph::HUB_WAIT_MAX_S));
    let de = quien_pide(req.from.as_deref());
    if let Err(msg) = super::super::bridge::send_text(&sesion, &req.text, de.as_deref()) {
        return hay_forma(termino(
            &sesion,
            &info.backend,
            super::wait::WaitOutcome::Ended {
                status: super::super::model::TurnStatus::Failed,
                text: msg,
            },
            nombre_lindo(&info.backend),
            empezo,
        ));
    }
    let salida = esperar(&sesion, espera_s);
    hay_forma(termino(
        &sesion,
        &info.backend,
        salida,
        nombre_lindo(&info.backend),
        empezo,
    ))
}

/// Le pone nombre a una sesión viva para poder llamarla por él.
fn rename(pedido: Pedido) -> Vec<u8> {
    let req: api::RenameRequest = match cuerpo(&pedido) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let sesion = match resolver_sesion(&req.session) {
        Ok(s) => s,
        Err(e) => return e,
    };
    match super::super::bridge::set_label(&sesion, &req.label) {
        Ok(label) => hay_forma(serde_json::json!({ "session": sesion, "label": label })),
        Err(msg) => no_hay_forma(404, HubError::nueva("unknown_session", msg)),
    }
}

fn wait(pedido: Pedido, empezo: Instant) -> Vec<u8> {
    let req: api::WaitRequest = match cuerpo(&pedido) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let sesion = match resolver_sesion(&req.session) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let Some(info) = super::super::bridge::session_info(&sesion) else {
        return no_hay_forma(
            404,
            HubError::nueva("unknown_session", "Esa sesión ya no existe en Atic.".into()),
        );
    };
    let espera_s = graph::clamp_wait(req.wait_s.unwrap_or(graph::HUB_WAIT_MAX_S));
    let salida = esperar(&sesion, espera_s);
    hay_forma(termino(
        &sesion,
        &info.backend,
        salida,
        nombre_lindo(&info.backend),
        empezo,
    ))
}

fn cancel(pedido: Pedido) -> Vec<u8> {
    let req: api::CancelRequest = match cuerpo(&pedido) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let sesion = match resolver_sesion(&req.session) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let _ = super::super::bridge::interrupt_session(&sesion);
    hay_forma(serde_json::json!({ "session": sesion, "status": "cancelling" }))
}

/// Espera el fin del turno hasta `espera_s`. Sin turno corriendo devuelve lo
/// último visto al tiro.
fn esperar(session: &str, espera_s: u64) -> super::wait::WaitOutcome {
    let Some(reloj) = super::super::bridge::session_watch(session) else {
        return super::wait::WaitOutcome::Idle { last: None };
    };
    reloj.wait_until(Instant::now() + Duration::from_secs(espera_s))
}

/// Mapea la espera al `Resultado` que lee el modelo padre. El timeout no mata
/// la sesión: devuelve el parcial + el id + el hint para seguir.
fn termino(
    session: &str,
    backend: &str,
    salida: super::wait::WaitOutcome,
    _backend_nombre: &str,
    empezo: Instant,
) -> Outcome {
    let elapsed_s = empezo.elapsed().as_secs();
    match salida {
        super::wait::WaitOutcome::Ended { status, text } => {
            use super::super::model::TurnStatus;
            match status {
                TurnStatus::Done => Outcome {
                    session: session.to_string(),
                    backend: backend.to_string(),
                    status: OutcomeStatus::Done,
                    text: graph::cap_text(&text),
                    hint: None,
                    elapsed_s,
                },
                TurnStatus::Cancelled => Outcome {
                    session: session.to_string(),
                    backend: backend.to_string(),
                    status: OutcomeStatus::Failed,
                    text: graph::cap_text(&text),
                    hint: Some("El turno se canceló; la sesión sigue viva.".into()),
                    elapsed_s,
                },
                _ => Outcome {
                    session: session.to_string(),
                    backend: backend.to_string(),
                    status: OutcomeStatus::Failed,
                    text: graph::cap_text(&text),
                    hint: Some(
                        "El turno falló. Revisa el texto; la sesión sigue viva si quieres reintentar con atic_prompt.".into(),
                    ),
                    elapsed_s,
                },
            }
        }
        super::wait::WaitOutcome::Timeout {
            text,
            permission_pending,
        } => Outcome {
            session: session.to_string(),
            backend: backend.to_string(),
            status: if permission_pending {
                OutcomeStatus::PermissionTimeout
            } else {
                OutcomeStatus::Timeout
            },
            text: graph::cap_text(&text),
            hint: Some(if permission_pending {
                "El agente está esperando un permiso en Atic. Apruébalo o recházalo ahí y sigue con atic_wait.".to_string()
            } else {
                "La sesión sigue viva en Atic. Espera el turno con atic_wait, manda otro con atic_prompt, o mira atic_list_sessions.".to_string()
            }),
            elapsed_s,
        },
        super::wait::WaitOutcome::Idle { last } => match last {
            Some((status, text)) => termino(
                session,
                backend,
                super::wait::WaitOutcome::Ended { status, text },
                _backend_nombre,
                empezo,
            ),
            None => Outcome {
                session: session.to_string(),
                backend: backend.to_string(),
                status: OutcomeStatus::Done,
                text: String::new(),
                hint: None,
                elapsed_s,
            },
        },
    }
}

fn hay_forma(cuerpo: impl serde::Serialize) -> Vec<u8> {
    armar(
        200,
        "OK",
        &serde_json::to_string(&cuerpo).unwrap_or_default(),
    )
}

fn no_hay_forma(codigo: u16, error: HubError) -> Vec<u8> {
    let razon = match codigo {
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        409 => "Conflict",
        _ => "Error",
    };
    armar(
        codigo,
        razon,
        &serde_json::json!({ "error": error }).to_string(),
    )
}

fn armar(codigo: u16, razon: &str, cuerpo: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 {codigo} {razon}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{cuerpo}",
        cuerpo.len()
    )
    .into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn quien_pide_traduce_una_app_de_fuera() {
        assert_eq!(
            quien_pide(Some("external:claude-code:1234")).as_deref(),
            Some("Claude Code")
        );
        assert_eq!(quien_pide(Some("external:grok:9")).as_deref(), Some("Grok"));
        // Host que no es un backend: se muestra tal cual en vez de mentir.
        assert_eq!(quien_pide(Some("external:zed:9")).as_deref(), Some("zed"));
    }

    #[test]
    fn sin_remitente_el_mensaje_no_se_sella() {
        assert!(quien_pide(None).is_none());
        assert!(quien_pide(Some("   ")).is_none());
        // Una sesión de Atic que ya no existe tampoco inventa un nombre.
        assert!(quien_pide(Some("no-existe")).is_none());
    }

    #[test]
    fn una_sesion_que_no_existe_ni_por_nombre_da_404() {
        let err = resolver_sesion("agy").expect_err("no hay sesiones en el test");
        let texto = String::from_utf8_lossy(&err);
        assert!(texto.contains("404"), "{texto}");
        assert!(texto.contains("unknown_session"), "{texto}");
    }

    fn pegar(
        port: u16,
        token: Option<&str>,
        metodo: &str,
        ruta: &str,
        cuerpo: Option<&str>,
    ) -> (u16, String) {
        let mut ultimo = String::new();
        for _ in 0..80 {
            match pegar_una(port, token, metodo, ruta, cuerpo) {
                Ok(r) => return r,
                Err(e) => ultimo = e,
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("el hub de test no contestó: {ultimo}");
    }

    fn pegar_una(
        port: u16,
        token: Option<&str>,
        metodo: &str,
        ruta: &str,
        cuerpo: Option<&str>,
    ) -> Result<(u16, String), String> {
        let mut conn = TcpStream::connect_timeout(
            &format!("127.0.0.1:{port}").parse().unwrap(),
            Duration::from_millis(200),
        )
        .map_err(|e| e.to_string())?;
        let body = cuerpo.unwrap_or("");
        let auth = token
            .map(|t| format!("Authorization: Bearer {t}\r\n"))
            .unwrap_or_default();
        let req = format!(
            "{metodo} {ruta} HTTP/1.1\r\nHost: 127.0.0.1\r\n{auth}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        conn.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
        let mut buf = Vec::new();
        conn.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        let texto = String::from_utf8_lossy(&buf);
        let status = texto
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let json = texto.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
        Ok((status, json))
    }

    /// Un aceptador propio por test, con su propio flag de vida: los tests
    /// corren en paralelo y un flag compartido los haría pisarse.
    fn hub_de_test() -> (u16, String, Arc<AtomicBool>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let token = format!("tok-{port}");
        let vivo = Arc::new(AtomicBool::new(true));
        let (t, v) = (token.clone(), vivo.clone());
        std::thread::spawn(move || aceptar(listener, t, None, v));
        (port, token, vivo)
    }

    /// Apaga el aceptador de un test: baja el flag y lo despierta con una
    /// conexión propia, igual que `stop`.
    fn apagar(port: u16, vivo: &AtomicBool) {
        vivo.store(false, Ordering::SeqCst);
        let _ = TcpStream::connect_timeout(
            &format!("127.0.0.1:{port}").parse().unwrap(),
            Duration::from_millis(200),
        );
    }

    #[test]
    fn health_pide_token() {
        let (port, token, vivo) = hub_de_test();
        let (sin, _) = pegar(port, None, "GET", "/v1/health", None);
        assert_eq!(sin, 401);
        let (con, cuerpo) = pegar(port, Some(&token), "GET", "/v1/health", None);
        assert_eq!(con, 200);
        let v: serde_json::Value = serde_json::from_str(&cuerpo).unwrap();
        assert!(v.get("pid").is_some());
        assert!(v.get("version").is_some());
        apagar(port, &vivo);
    }

    #[test]
    fn agents_trae_lista() {
        let (port, token, vivo) = hub_de_test();
        let (codigo, cuerpo) = pegar(port, Some(&token), "GET", "/v1/agents", None);
        assert_eq!(codigo, 200);
        let v: serde_json::Value = serde_json::from_str(&cuerpo).unwrap();
        assert!(v["agents"].is_array(), "forma de agents: {cuerpo}");
        apagar(port, &vivo);
    }

    #[test]
    fn spawn_backend_inventado_es_400() {
        let (port, token, vivo) = hub_de_test();
        let (codigo, cuerpo) = pegar(
            port,
            Some(&token),
            "POST",
            "/v1/spawn",
            Some(r#"{"backend":"magia","depth":0}"#),
        );
        assert_eq!(codigo, 400);
        let v: serde_json::Value = serde_json::from_str(&cuerpo).unwrap();
        assert_eq!(v["error"]["code"], "unknown_backend");
        apagar(port, &vivo);
    }

    #[test]
    fn wait_sesion_desconocida_es_404() {
        let (port, token, vivo) = hub_de_test();
        let (codigo, cuerpo) = pegar(
            port,
            Some(&token),
            "POST",
            "/v1/wait",
            Some(r#"{"session":"no-existe"}"#),
        );
        assert_eq!(codigo, 404);
        let v: serde_json::Value = serde_json::from_str(&cuerpo).unwrap();
        assert_eq!(v["error"]["code"], "unknown_session");
        apagar(port, &vivo);
    }

    #[test]
    fn stop_borra_hub_json() {
        let dir = std::env::temp_dir().join(format!(
            "atic-hub-stop-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let ruta = dir.join("hub.json");
        std::fs::write(&ruta, "{}").unwrap();
        *HUB_JSON.lock_or_recover() = Some(ruta.clone());
        *PUERTO.lock_or_recover() = None;
        stop();
        assert!(!ruta.exists(), "stop borra hub.json");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
