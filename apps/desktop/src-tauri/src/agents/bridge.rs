//! Puente entre la capa de agentes y el frontend.
//!
//! Registro de sesiones vivas + los comandos de Tauri que las manejan. Los
//! eventos viajan por `agent-event`, cada uno etiquetado con la sesión que lo
//! produjo: la app puede tener varias abiertas a la vez y la UI necesita saber
//! a cuál pertenece cada línea.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use atic_core::secrets;
use atic_core::{MutexExt, SshHost};

use crate::state::AppState;

use super::{
    claude_code::ClaudeCode, hub, hub::api, hub::graph, hub::wait::TurnWatch, AgentBackend,
    AgentDelta, AgentSession, AgentSkill, PermissionDecision, StartOptions,
};

/// Una sesión viva más lo que hace falta para nombrarla sin volver a mirar la
/// lista de backends.
struct Entry {
    backend: String,
    display_name: String,
    session: Box<dyn AgentSession>,
    /// Acumula el turno para quien lo espera por MCP. Lo alimenta `on_delta`
    /// en primera línea: quien espera despierta aunque el store o el emit fallen.
    watch: std::sync::Arc<TurnWatch>,
    meta: SessionMeta,
}

/// De dónde salió la sesión: la UI es raíz, el hub trae padre y profundidad.
pub(crate) struct SpawnMeta {
    pub parent: Option<String>,
    /// Profundidad del hijo (la UI es 0; el hub pasa `depth + 1` del pedido).
    pub depth: u8,
    /// Raíz del encargo para detectar ciclos. Vacío = se acuña con la clave nueva.
    pub root: Option<String>,
    pub label: Option<String>,
}

impl SpawnMeta {
    pub fn root() -> Self {
        Self {
            parent: None,
            depth: 0,
            root: None,
            label: None,
        }
    }
}

/// Variables del grafo para el proceso hijo. Van siempre, inyectes o no el
/// MCP: si ese proceso carga el MCP por config global, la profundidad no miente.
pub(crate) fn hub_env(
    clave: &str,
    profundidad_hijo: u8,
    raiz: &str,
    padre: Option<&str>,
) -> Vec<(String, String)> {
    let mut env = vec![
        ("ATIC_SESSION".to_string(), clave.to_string()),
        (
            "ATIC_DELEGATE_DEPTH".to_string(),
            profundidad_hijo.to_string(),
        ),
        ("ATIC_ROOT".to_string(), raiz.to_string()),
    ];
    if let Some(padre) = padre {
        env.push(("ATIC_PARENT".to_string(), padre.to_string()));
    }
    env
}

/// Lo que el hub necesita saber de cada sesión sin tocar el proceso.
#[derive(Clone)]
pub(crate) struct SessionMeta {
    pub cwd: String,
    pub remote_host_id: Option<String>,
    pub parent: Option<String>,
    pub label: Option<String>,
    pub root: String,
}

/// Sesiones abiertas, por clave local.
///
/// La clave la genera Atic y no el backend: el id de sesión del agente llega
/// recién en el primer evento, y para entonces la UI ya necesita algo con qué
/// referirse a la conversación.
static SESSIONS: Mutex<Option<HashMap<String, Entry>>> = Mutex::new(None);

/// Ids que el watcher del pager tiene que ignorar.
///
/// Incluye la clave local y el `provider_session` del CLI: `claude --resume`
/// escribe el mismo JSONL que una TUI, y sin este filtro el chip contaría dos veces.
pub(crate) fn live_session_ids() -> HashSet<String> {
    let mut ids = super::store::live_provider_sessions();
    if let Ok(guard) = SESSIONS.lock() {
        if let Some(map) = guard.as_ref() {
            ids.extend(map.keys().cloned());
        }
    }
    ids
}

/// Backends conocidos. Sumar uno es agregarlo a esta lista.
///
/// Claude Code tiene adaptador propio porque habla su `stream-json`; los otros
/// dos son el MISMO adaptador con otra constante, porque los dos hablan ACP.
/// Ese es el pago de haber moldeado el modelo canónico sobre ese protocolo.
fn backends() -> Vec<Box<dyn AgentBackend>> {
    vec![
        Box::new(ClaudeCode),
        Box::new(super::codex::Codex),
        Box::new(super::acp::OPENCODE),
        Box::new(super::acp::CURSOR),
        Box::new(super::acp::GROK),
        Box::new(super::antigravity::Antigravity),
    ]
}

fn find(id: &str) -> Option<Box<dyn AgentBackend>> {
    backends().into_iter().find(|b| b.id() == id)
}

/// Los ids que existen, sin tocar el disco.
///
/// Es lo que tiene que consultar cualquiera que valide un backend: una lista
/// escrita a mano en otro archivo se queda vieja al sumar uno, y el síntoma es
/// «Backend desconocido» para algo que sí está.
pub(crate) fn backend_ids() -> Vec<&'static str> {
    backends().iter().map(|b| b.id()).collect()
}

/// El nombre para mostrar de un backend, o `None` si el id no existe.
pub(crate) fn backend_display_name(id: &str) -> Option<&'static str> {
    backends()
        .iter()
        .find(|b| b.id() == id)
        .map(|b| b.display_name())
}

/// Las etiquetas que ya están en uso por sesiones vivas.
fn etiquetas_vivas() -> HashSet<String> {
    SESSIONS
        .lock_or_recover()
        .as_ref()
        .map(|map| {
            map.values()
                .filter_map(|e| e.meta.label.clone())
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default()
}

/// Un nombre que no choque con los que ya andan dando vueltas.
///
/// Quien delega elige el nombre y no tiene forma de saber qué hay abierto, así
/// que dos encargos parecidos terminan pidiendo «revisor» los dos. Repetirlo
/// haría que la vista muestre dos consolas iguales y que el usuario no sepa a
/// cuál le está hablando; por eso el segundo pasa a ser «revisor 2».
fn etiqueta_unica(pedida: &str, usadas: &HashSet<String>) -> String {
    let base = pedida.trim();
    if base.is_empty() {
        return String::new();
    }
    if !usadas.contains(base) {
        return base.to_string();
    }
    // Un tope: si hay cien «revisor» vivos, el problema no es el nombre.
    (2..=100)
        .map(|n| format!("{base} {n}"))
        .find(|c| !usadas.contains(c))
        .unwrap_or_else(|| base.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendInfo {
    pub id: String,
    pub display_name: String,
    /// Si está instalado. Un backend ausente se muestra deshabilitado en vez
    /// de ofrecerse y fallar recién al usarlo.
    pub available: bool,
    /// Si tiene sesión iniciada; `None` = no se sabe mirar. Nunca apaga
    /// `available`: son dos problemas con dos arreglos distintos.
    pub signed_in: Option<bool>,
}

/// Lo que viaja al frontend en cada cambio.
///
/// Lleva el backend además de la sesión porque los deltas son globales: una
/// ventana puede ver la conversación de una sesión que arrancó otra, y sin este
/// dato no tendría con qué nombrarla.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct EventPayload {
    session: String,
    backend_id: String,
    backend_name: String,
    /// Aplanado: el discriminante `t` del delta queda al mismo nivel que la
    /// sesión, así el frontend hace un solo `switch` sin desenvolver nada.
    #[serde(flatten)]
    delta: AgentDelta,
}

/// Una sesión abierta, para que una vista que se monta tarde se ponga al día.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: String,
    pub backend_id: String,
    pub backend_name: String,
    /// Quién la pidió; `None` = nació en la UI.
    pub parent: Option<String>,
    /// El nombre que le puso quien la pidió, ya hecho único.
    pub label: Option<String>,
}

/// Qué sesiones siguen vivas.
///
/// El proceso del agente lo tiene Rust, no la ventana que lo arrancó: sigue
/// corriendo con el panel cerrado, y cualquier vista puede adoptarlo.
#[tauri::command]
pub fn agent_sessions() -> Vec<SessionInfo> {
    SESSIONS
        .lock_or_recover()
        .as_ref()
        .map(|map| {
            map.iter()
                .map(|(id, entry)| SessionInfo {
                    id: id.clone(),
                    backend_id: entry.backend.clone(),
                    backend_name: entry.display_name.clone(),
                    parent: entry.meta.parent.clone(),
                    label: entry.meta.label.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// La forma inicial del globo, en píxeles lógicos.
///
/// El tamaño vive acá y no se mide del DOM porque al cerrarse no hay silueta
/// que medir; la próxima apertura usaría el tamaño de la pill.
const BUBBLE: crate::floating::BubbleShape = crate::floating::BubbleShape {
    w: 360,
    h: 196,
    gap: 10,
    corner: 26,
};

/// Lo más chico que puede quedar el globo sin que el compositor se rompa.
///
/// El lanzador usa este mínimo compacto; al entrar a las consolas el frontend
/// aplica un mínimo mayor para que el terminal conserve un área útil.
const BUBBLE_MIN_W: i32 = 336;
const BUBBLE_MIN_H: i32 = 176;

/// La forma del globo, con el tamaño al que lo haya dejado el usuario.
fn bubble_shape(app: &AppHandle) -> crate::floating::BubbleShape {
    let saved = app
        .try_state::<crate::AppState>()
        .and_then(|s| s.config.lock().ok().and_then(|c| c.agents_bubble_size));
    match saved {
        Some((w, h)) => crate::floating::BubbleShape {
            w: w.max(BUBBLE_MIN_W),
            h: h.max(BUBBLE_MIN_H),
            ..BUBBLE
        },
        None => BUBBLE,
    }
}

/// ¿El usuario la dejó abierta (el float está a la vista, también achicado)?
///
/// El atajo es un interruptor. El historial del portapapeles pregunta
/// [`agents_open`] para saber si puede insertar.
static OPEN: AtomicBool = AtomicBool::new(false);

/// Preferencia de pin sticky: Esc no cierra la consola.
///
/// Default `false`. Se hidrata desde `Config::agents_always_on_top` al arrancar.
/// El pin no mueve el stacking del overlay: la pill y los floats comparten
/// ventana, y desfijar no puede hundir la pill bajo otras apps.
static ALWAYS_ON_TOP: AtomicBool = AtomicBool::new(false);

const AGENTS_ANCHOR: &str = "agents-bubble-anchor";
const AGENTS_DISMISS: &str = "agents-bubble-dismiss";
const AGENTS_EXPAND: &str = "agents-bubble-expand";

/// ¿La consola está desplegada? Lo pregunta el historial del portapapeles, que
/// con ella abierta inserta en el compositor en vez de pegar afuera.
pub fn agents_open() -> bool {
    OPEN.load(Ordering::Relaxed)
}

/// Carga la preferencia de pin desde config (una vez, al arrancar).
pub fn init_always_on_top(on: bool) {
    ALWAYS_ON_TOP.store(on, Ordering::Relaxed);
}

/// ¿El overlay debe ser topmost ahora?
///
/// Sí, siempre. La pill y los floats del overlay comparten esa ventana: si
/// desfijar un float quitara always-on-top, la pill también quedaría debajo
/// de otras apps. El pin de agentes solo evita el cierre con Esc.
pub fn overlay_should_be_topmost() -> bool {
    true
}

/// Abre o cierra la consola de agentes, que **sale de la pill**.
///
/// Geometría vía `panel_float` con la `BubbleShape` de agentes (tamaño
/// guardado). Es un interruptor: no se cierra al perder el foco.
#[tauri::command]
pub fn show_agents_window(app: AppHandle) {
    if !crate::agents::UI_ENABLED {
        return;
    }
    let _ = crate::panel_float::toggle(
        &app,
        &OPEN,
        bubble_shape(&app),
        AGENTS_ANCHOR,
        AGENTS_DISMISS,
    );
    crate::overlay::set_topmost(&app, overlay_should_be_topmost());
}

/// Muestra o agranda. Nunca esconde: la rueda y el chip de la pill piden
/// «abrí esto», no un interruptor. Si ya está achicada junto a la pill,
/// el frontend la vuelve a agrandar.
#[tauri::command]
pub fn present_agents_window(app: AppHandle) {
    if !crate::agents::UI_ENABLED {
        return;
    }
    if OPEN.load(Ordering::Relaxed) {
        let _ = app.emit(AGENTS_EXPAND, ());
        // OPEN true no basta: el globo puede estar achicado o haber perdido el
        // DOM (HMR). Sin ancla, el frontend no tiene de dónde nacer.
        let _ = crate::panel_float::reanchor(&app, bubble_shape(&app), AGENTS_ANCHOR);
        crate::overlay::set_topmost(&app, overlay_should_be_topmost());
        return;
    }
    let _ = crate::panel_float::show(&app, &OPEN, bubble_shape(&app), AGENTS_ANCHOR);
    crate::overlay::set_topmost(&app, overlay_should_be_topmost());
}

/// ¿La consola está fijada (no se cierra sola con Esc)?
#[tauri::command]
pub fn agents_always_on_top() -> bool {
    ALWAYS_ON_TOP.load(Ordering::Relaxed)
}

/// Fija o desfija la consola (sticky Esc). Persiste; el overlay sigue topmost.
#[tauri::command]
pub fn set_agents_always_on_top(app: AppHandle, on: bool) {
    ALWAYS_ON_TOP.store(on, Ordering::Relaxed);
    if let Some(state) = app.try_state::<crate::AppState>() {
        let snapshot = {
            let Ok(mut cfg) = state.config.lock() else {
                crate::overlay::set_topmost(&app, overlay_should_be_topmost());
                return;
            };
            cfg.agents_always_on_top = on;
            cfg.clone()
        };
        let _ = snapshot.save(&state.dirs.config_path());
    }
    crate::overlay::set_topmost(&app, overlay_should_be_topmost());
}

/// Guarda a qué tamaño dejaste el globo, para la próxima apertura.
///
/// Antes esto además MOVÍA la ventana en cada cuadro del arrastre, y por eso
/// recibía el lado anclado: el borde que mira a la pill no se mueve y el
/// opuesto sí. Esa cuenta se fue a la vista, que es la que ahora dibuja el
/// globo — acá solo queda el disco.
///
/// Llega solo al soltar. Mientras arrastrás llegaba sesenta veces por segundo,
/// y reescribir el JSON otras tantas castiga el disco para guardar valores que
/// nadie va a leer: el único que importa es el último.
#[tauri::command]
pub fn save_agents_bubble_size(app: AppHandle, w: i32, h: i32) {
    let w = w.max(BUBBLE_MIN_W);
    let h = h.max(BUBBLE_MIN_H);
    if let Some(state) = app.try_state::<crate::AppState>() {
        let snapshot = {
            let Ok(mut cfg) = state.config.lock() else {
                return;
            };
            cfg.agents_bubble_size = Some((w, h));
            cfg.clone()
        };
        let _ = snapshot.save(&state.dirs.config_path());
    }
}

/// Esconde el float. Las consolas siguen corriendo.
#[tauri::command]
pub fn hide_agents_window(app: AppHandle) {
    crate::panel_float::hide(&app, &OPEN, AGENTS_DISMISS);
    crate::overlay::set_topmost(&app, overlay_should_be_topmost());
}

/// Qué agentes hay y cuáles se pueden usar.
///
/// `is_available` lanza un proceso por backend, así que esto no es gratis: la
/// UI debería llamarlo al abrir la vista, no en cada render.
#[tauri::command]
pub fn agent_backends() -> Vec<BackendInfo> {
    backend_availability()
}

/// Lo mismo pero sin envoltorio Tauri: lo usa la cache del hub.
///
/// Sondea el disco (binario en el PATH, archivos de credenciales) una vez por
/// backend, así que se llama desde la cache y no desde cada request.
pub(crate) fn backend_availability() -> Vec<BackendInfo> {
    backends()
        .iter()
        .map(|b| BackendInfo {
            id: b.id().to_string(),
            display_name: b.display_name().to_string(),
            available: b.is_available(),
            signed_in: b.signed_in(),
        })
        .collect()
}

/// Lo que la UI elige antes de arrancar. Todo opcional: sin nada, el agente
/// corre con su propia configuración.
#[derive(Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRequest {
    pub cwd: Option<String>,
    /// Id de host SSH en config. `None` = local.
    pub remote_host_id: Option<String>,
    pub resume: Option<String>,
    pub model: Option<String>,
    /// Cuánto tiene que pensar. Los nombres los define cada backend.
    pub effort: Option<String>,
    /// Variante rápida (Cursor). Independiente del effort.
    pub fast: Option<bool>,
    pub permission_mode: Option<String>,
    /// JSON `{"mcpServers": {…}}` con los servidores que sume Atic.
    pub mcp_config: Option<String>,
    #[serde(default)]
    pub add_dirs: Vec<String>,
    /// Al reanudar, bifurcar en vez de seguir escribiendo el hilo original.
    #[serde(default)]
    pub fork: bool,
}

/// Arranca una sesión y devuelve su clave local.
#[tauri::command]
pub fn agent_start(
    app: AppHandle,
    backend: String,
    options: Option<StartRequest>,
) -> Result<String, String> {
    // El arranque real es `start_session`: la UI entra como raíz y el hub con
    // padre y profundidad.
    start_session(
        &app,
        &backend,
        options.unwrap_or_default(),
        SpawnMeta::root(),
    )
}

/// El arranque reutilizable: la UI y el hub pasan por acá.
pub(crate) fn start_session(
    app: &AppHandle,
    backend: &str,
    options: StartRequest,
    spawn: SpawnMeta,
) -> Result<String, String> {
    let StartRequest {
        cwd,
        remote_host_id,
        resume,
        model,
        effort,
        fast,
        permission_mode,
        mcp_config,
        add_dirs,
        fork,
    } = options;
    let agent = find(backend).ok_or_else(|| format!("backend desconocido: {backend}"))?;
    let key = uuid::Uuid::new_v4().to_string();
    let display_name = agent.display_name().to_string();
    // Dueño para el callback de deltas: `on_delta` pide `'static` y acá solo
    // hay un préstamo. Sin este clon, el cierre captura `&AppHandle` y no compila.
    let app = app.clone();

    let remote = if let Some(id) = remote_host_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if backend != "claude-code" {
            return Err("Por ahora solo Claude Code admite sesión remota por SSH.".into());
        }
        let state = app
            .try_state::<AppState>()
            .ok_or_else(|| "la app no está lista".to_string())?;
        let host = state
            .config
            .lock_or_recover()
            .ssh_hosts
            .iter()
            .find(|h| h.id == id)
            .cloned()
            .ok_or_else(|| format!("Host SSH desconocido: {id}"))?;
        Some(super::ssh::RemoteTarget { host })
    } else {
        None
    };

    let root = spawn.root.clone().unwrap_or_else(|| key.clone());
    // El grafo viaja en el env aunque el MCP no se inyecte: si ese proceso
    // carga el MCP por config global del usuario, la profundidad no miente.
    let env = hub_env(&key, spawn.depth, &root, spawn.parent.as_deref());

    // El merge vive en Rust porque la UI no manda `mcpConfig` (nadie lee
    // `agent_mcp_servers` al arrancar): se arma desde la config en disco.
    //
    // Claude recibe el servidor `atic` dentro del `mcp_config` ya mergeado; los
    // demás backends por `atic_mcp`, que cada adaptador traduce a lo suyo. Los
    // servidores del modal siguen siendo solo de Claude: vienen en forma JSON
    // suya y convertirlos a TOML/ACP es otra tarea.
    let es_claude = backend == "claude-code";
    let atic_mcp = if remote.is_none() && !es_claude {
        hub::atic_mcp(backend)
    } else {
        None
    };
    let mcp_config = if es_claude && remote.is_none() {
        let guardada = app
            .try_state::<AppState>()
            .map(|s| s.config.lock_or_recover().agent_mcp_servers.clone())
            .unwrap_or_default();
        hub::merge_mcp_config(&guardada, mcp_config.as_deref(), hub::mcp_server_entry())
    } else {
        mcp_config
    };

    let meta = SessionMeta {
        cwd: cwd.clone().unwrap_or_default(),
        remote_host_id: remote_host_id.clone(),
        parent: spawn.parent.clone(),
        label: spawn
            .label
            .clone()
            .map(|l| etiqueta_unica(&l, &etiquetas_vivas())),
        root,
    };

    // Seguir el hilo desde ANTES de arrancar: el primer delta puede llegar
    // mientras `start` todavía no volvió, y sin el hilo abierto se perdería.
    super::store::open(
        &key,
        backend,
        &display_name,
        cwd.as_deref().unwrap_or(""),
        remote_host_id.as_deref(),
        meta.parent.as_deref(),
    );

    let watch = TurnWatch::new();
    let watch_delta = watch.clone();

    let emit_key = key.clone();
    let emit_backend = backend.to_string();
    let emit_name = display_name.clone();
    let session = agent.start(
        StartOptions {
            cwd,
            remote,
            // La clave local se usa también como id de la conversación en el
            // CLI. Son dos identidades que no tienen por qué coincidir, y
            // hacerlas coincidir vale la pena: el id que la interfaz muestra es
            // el mismo con el que se reanuda, sin tabla de equivalencias en el
            // medio. Al bifurcar deja de ser cierto —ahí el CLI acuña uno
            // nuevo— y el id real llega en `Started`.
            session_id: Some(key.clone()),
            resume,
            fork,
            model,
            effort,
            fast,
            permission_mode,
            mcp_config,
            atic_mcp,
            add_dirs,
            env,
        },
        Box::new(move |delta| {
            // Primero al vigilante del hub, después al store y a la ventana.
            // Quien espera un `TurnEnd` tiene que despertar aunque el emit falle.
            watch_delta.observe(&delta);
            if super::store::apply(&emit_key, &delta) {
                with_db(&app, |db| super::store::flush(db, &emit_key));
            }
            let _ = app.emit(
                "agent-event",
                EventPayload {
                    session: emit_key.clone(),
                    backend_id: emit_backend.clone(),
                    backend_name: emit_name.clone(),
                    delta,
                },
            );
        }),
    )?;

    SESSIONS
        .lock_or_recover()
        .get_or_insert_with(HashMap::new)
        .insert(
            key.clone(),
            Entry {
                backend: backend.to_string(),
                display_name,
                session,
                watch,
                meta,
            },
        );
    Ok(key)
}

/// Lo vivo para el hub: backend, host, clave de carpeta, raíz y si trabaja.
pub(crate) fn live_sessions() -> Vec<graph::LiveSession> {
    SESSIONS
        .lock_or_recover()
        .as_ref()
        .map(|map| {
            map.iter()
                .map(|(id, entry)| graph::LiveSession {
                    id: id.clone(),
                    backend: entry.backend.clone(),
                    host: entry
                        .meta
                        .remote_host_id
                        .clone()
                        .unwrap_or_else(|| "local".to_string()),
                    cwd_key: graph::cwd_key(&entry.meta.cwd),
                    root: Some(entry.meta.root.clone()),
                    running: entry.watch.is_running(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// El vigilante de una sesión, para esperar su `TurnEnd` desde el hub.
pub(crate) fn session_watch(id: &str) -> Option<std::sync::Arc<TurnWatch>> {
    SESSIONS
        .lock_or_recover()
        .as_ref()
        .and_then(|map| map.get(id))
        .map(|entry| entry.watch.clone())
}

/// Manda un texto sin origen: lo que `agent_send` hace cuando viene del hub.
/// Ponerle nombre a una sesión viva. Devuelve el que quedó, que puede no ser
/// el pedido si ya había otra con ese.
///
/// Sirve de identificador: quien delega puede decir «pregúntale a agy» en vez
/// de arrastrar un uuid, y el usuario puede bautizar una consola que abrió él.
pub(crate) fn set_label(id: &str, label: &str) -> Result<String, String> {
    let usadas: HashSet<String> = {
        let guard = SESSIONS.lock_or_recover();
        let sessions = guard
            .as_ref()
            .ok_or_else(|| "no hay sesiones abiertas".to_string())?;
        if !sessions.contains_key(id) {
            return Err("esa sesión ya no existe".to_string());
        }
        sessions
            .iter()
            // La propia no cuenta: renombrar «agy» a «agy» no la hace «agy 2».
            .filter(|(k, _)| k.as_str() != id)
            .filter_map(|(_, e)| e.meta.label.clone())
            .collect()
    };
    let final_ = etiqueta_unica(label, &usadas);
    let mut guard = SESSIONS.lock_or_recover();
    let sessions = guard
        .as_mut()
        .ok_or_else(|| "no hay sesiones abiertas".to_string())?;
    let entry = sessions
        .get_mut(id)
        .ok_or_else(|| "esa sesión ya no existe".to_string())?;
    entry.meta.label = if final_.is_empty() {
        None
    } else {
        Some(final_.clone())
    };
    Ok(final_)
}

/// Qué sesión es «agy»: el nombre, sin distinguir mayúsculas ni espacios.
///
/// `Err` cuando hay más de una: con dos sesiones llamadas igual, elegir una
/// sería mandarle el recado a la equivocada la mitad de las veces.
pub(crate) fn session_by_label(nombre: &str) -> Result<Option<String>, Vec<String>> {
    let buscado = nombre.trim().to_lowercase();
    if buscado.is_empty() {
        return Ok(None);
    }
    let guard = SESSIONS.lock_or_recover();
    let Some(sessions) = guard.as_ref() else {
        return Ok(None);
    };
    let coincidencias: Vec<String> = sessions
        .iter()
        .filter(|(_, e)| {
            e.meta
                .label
                .as_deref()
                .is_some_and(|l| l.trim().to_lowercase() == buscado)
        })
        .map(|(k, _)| k.clone())
        .collect();
    match coincidencias.len() {
        0 => Ok(None),
        1 => Ok(Some(coincidencias[0].clone())),
        _ => Err(coincidencias),
    }
}

/// Manda un turno que pidió otro agente por el hub.
///
/// `de` es quién pregunta, ya en legible. Va como `Origin`, el mismo sello que
/// llevan el dictado o la captura: sin él, en la consola aparece un mensaje de
/// usuario que el usuario no escribió y no hay forma de saber de quién vino.
pub(crate) fn send_text(id: &str, text: &str, de: Option<&str>) -> Result<(), String> {
    let origen = de.map(|via| super::model::Origin {
        via: via.to_string(),
        file: None,
        files: Vec::new(),
    });
    let mut guard = SESSIONS.lock_or_recover();
    let sessions = guard
        .as_mut()
        .ok_or_else(|| "no hay sesiones abiertas".to_string())?;
    sessions
        .get_mut(id)
        .ok_or_else(|| "esa sesión ya no existe".to_string())?
        .session
        .send(text, origen)
}

/// Interrumpe el turno sin origen UI: lo mismo que `agent_interrupt`.
pub(crate) fn interrupt_session(id: &str) -> Result<(), String> {
    let mut guard = SESSIONS.lock_or_recover();
    let sessions = guard
        .as_mut()
        .ok_or_else(|| "no hay sesiones abiertas".to_string())?;
    sessions
        .get_mut(id)
        .ok_or_else(|| "esa sesión ya no existe".to_string())?
        .session
        .interrupt()
}

/// Ficha de una sesión para `atic_list_sessions`.
pub(crate) fn session_info(id: &str) -> Option<api::SessionInfo> {
    SESSIONS
        .lock_or_recover()
        .as_ref()
        .and_then(|map| map.get(id))
        .map(|entry| api::SessionInfo {
            session: id.to_string(),
            backend: entry.backend.clone(),
            cwd: entry.meta.cwd.clone(),
            remote: entry.meta.remote_host_id.clone(),
            running: entry.watch.is_running(),
            parent: entry.meta.parent.clone(),
            label: entry.meta.label.clone(),
        })
}

/// Manda un turno.
///
/// `origin` lo pone la vista: dice si el texto entró dictado, con una captura o
/// desde el portapapeles. Las rutas en `origin.files` se leen y se envían al
/// agente como bloques de imagen; el resto del origen queda en la conversación
/// guardada.
#[tauri::command]
pub fn agent_send(
    session: String,
    text: String,
    origin: Option<super::model::Origin>,
) -> Result<(), String> {
    let mut guard = SESSIONS.lock_or_recover();
    let sessions = guard
        .as_mut()
        .ok_or_else(|| "no hay sesiones abiertas".to_string())?;
    sessions
        .get_mut(&session)
        .ok_or_else(|| "esa sesión ya no existe".to_string())?
        .session
        .send(&text, origin)
}

/// Cambia el modelo —y con él el esfuerzo— sin reiniciar la sesión.
///
/// No todos saben: los de ACP no nombran los modelos en su protocolo, y ahí
/// esto no hace nada. La vista no ofrece el selector cuando el backend no
/// informó ninguno, así que no llega a llamarse.
#[tauri::command]
pub fn agent_set_model(
    session: String,
    model: String,
    effort: Option<String>,
    fast: Option<bool>,
) -> Result<(), String> {
    let mut guard = SESSIONS.lock_or_recover();
    let sessions = guard
        .as_mut()
        .ok_or_else(|| "no hay sesiones abiertas".to_string())?;
    sessions
        .get_mut(&session)
        .ok_or_else(|| "esa sesión ya no existe".to_string())?
        .session
        .set_model(&model, effort.as_deref(), fast)
}

/// Contesta un permiso pendiente. El turno del agente está detenido hasta acá.
#[tauri::command]
pub fn agent_permission(
    session: String,
    id: String,
    decision: PermissionDecision,
) -> Result<(), String> {
    let mut guard = SESSIONS.lock_or_recover();
    guard
        .as_mut()
        .ok_or_else(|| "no hay sesiones abiertas".to_string())?
        .get_mut(&session)
        .ok_or_else(|| "esa sesión ya no existe".to_string())?
        .session
        .respond_permission(&id, decision)
}

/// Las skills disponibles para una carpeta de trabajo.
///
/// Se consulta cada vez en vez de guardarse: son archivos que el usuario edita
/// con el editor abierto al lado, y una lista cacheada sería una lista vieja
/// justo cuando acaba de escribir una.
#[tauri::command]
pub fn agent_skills(cwd: Option<String>) -> Vec<AgentSkill> {
    super::skills::discover(cwd.as_deref())
}

/// Modelos disponibles para un backend, sin abrir sesión.
///
/// Corre en `spawn_blocking`: los probes CLI (sobre todo OpenCode) tardan
/// segundos y un comando sync congelaba la ventana («No responde»).
#[tauri::command]
pub async fn agent_list_models(
    backend: String,
) -> Result<Vec<crate::agents::model::ModelInfo>, String> {
    tauri::async_runtime::spawn_blocking(move || crate::agents::discover::list_models(&backend))
        .await
        .map_err(|e| format!("list_models cancelado: {e}"))?
}

/// Le pone nombre a una consola desde la interfaz.
///
/// El mismo nombre que usan los agentes para llamarse entre ellos: bautizar
/// una consola acá es lo que hace que «pregúntale a agy» funcione desde otro
/// agente. Devuelve el que quedó, con número si ya estaba tomado.
#[tauri::command]
pub fn agent_rename_session(session: String, label: String) -> Result<String, String> {
    set_label(&session, &label)
}

/// Corta el turno en curso. La sesión sigue viva (historial, cwd, modelo).
#[tauri::command]
pub fn agent_interrupt(session: String) -> Result<(), String> {
    let mut guard = SESSIONS.lock_or_recover();
    let sessions = guard
        .as_mut()
        .ok_or_else(|| "no hay sesiones abiertas".to_string())?;
    sessions
        .get_mut(&session)
        .ok_or_else(|| "esa sesión ya no existe".to_string())?
        .session
        .interrupt()
}

/// Cierra la sesión por completo y libera el proceso del agente.
///
/// No es «Detener» el turno: eso es [`agent_interrupt`]. Se usa al reanudar
/// otro hilo, al salir de la app, o cuando la UI quiere terminar el chat.
#[tauri::command]
pub fn agent_stop(app: AppHandle, session: String) {
    let taken = SESSIONS
        .lock_or_recover()
        .as_mut()
        .and_then(|s| s.remove(&session));
    // Fuera del lock Y en otro hilo: `stop` puede tardar en matar el proceso
    // (espera corta + kill). Si el comando IPC espera, el botón parece muerto
    // aunque la sesión ya salió de la lista del frontend.
    if let Some(mut entry) = taken {
        std::thread::spawn(move || {
            entry.session.stop();
        });
    }
    with_db(&app, |db| super::store::close(db, &session));
}

/// Cierra todo. Se llama al salir para no dejar procesos huérfanos.
///
/// Baja los hilos a disco ANTES de matar los procesos: es el punto donde una
/// conversación en curso se guarda de verdad, y hacerlo después dejaría fuera
/// lo último que el agente alcanzó a decir.
pub fn stop_all(app: &AppHandle) {
    with_db(app, |db| {
        for id in super::store::tracked() {
            super::store::flush(db, &id);
        }
    });
    let taken: Vec<_> = SESSIONS
        .lock_or_recover()
        .as_mut()
        .map(|s| s.drain().map(|(_, v)| v).collect())
        .unwrap_or_default();
    for mut entry in taken {
        entry.session.stop();
    }
    super::console::close_all();
}

/// Corre algo con la base, si la app ya la tiene montada.
///
/// `try_state` y no `state`: al cerrar, el estado puede haberse desmontado ya, y
/// entrar en pánico dentro del apagado dejaría procesos del agente huérfanos.
fn with_db<T>(app: &AppHandle, f: impl FnOnce(&atic_core::Db) -> T) -> Option<T> {
    let state = app.try_state::<crate::state::AppState>()?;
    let db = state.db.lock().ok()?;
    Some(f(&db))
}

/// Las conversaciones guardadas, de la más reciente a la más vieja.
///
/// Sin los turnos: la lista solo necesita con qué reconocerlas, y mandar la
/// conversación entera de cada una sería cargar megabytes para pintar líneas.
#[tauri::command]
pub fn agent_threads(app: AppHandle) -> Result<Vec<super::store::StoredThread>, String> {
    with_db(&app, super::store::list)
        .unwrap_or_else(|| Err("la base no está disponible".to_string()))
}

/// Una conversación guardada, con todos sus turnos.
#[tauri::command]
pub fn agent_thread(
    app: AppHandle,
    id: String,
) -> Result<Option<super::store::StoredThread>, String> {
    with_db(&app, |db| super::store::get(db, &id))
        .unwrap_or_else(|| Err("la base no está disponible".to_string()))
}

#[tauri::command]
pub fn agent_thread_delete(app: AppHandle, id: String) -> Result<(), String> {
    with_db(&app, |db| super::store::delete(db, &id))
        .unwrap_or_else(|| Err("la base no está disponible".to_string()))
}

/// Sesiones del CLI de Claude Code para un `cwd` (índice local, no import).
///
/// Sirven para reanudar con `--resume`. Vacío si no hay carpeta o no hay
/// transcripts en `~/.claude/projects/…`.
#[tauri::command]
pub fn agent_claude_sessions(cwd: String) -> Vec<super::claude_sessions::ClaudeCodeSession> {
    super::claude_sessions::list_for_cwd(&cwd)
}

/// Transcript local del CLI, ya en turnos canónicos para pintar el chat.
#[tauri::command]
pub fn agent_claude_transcript(cwd: String, id: String) -> Result<Vec<super::model::Turn>, String> {
    super::claude_sessions::load_transcript(&cwd, &id)
}

/// Cupos de la cuenta Claude (ventana 5 h, semanal, etc.).
///
/// Corre en `spawn_blocking`: lee credenciales y llama a la API OAuth; no
/// debe bloquear el hilo IPC. Cachea unos segundos en Rust para el poll
/// del modal de Uso.
#[tauri::command]
pub async fn agent_claude_usage() -> Result<super::claude_usage::ClaudeAccountUsage, String> {
    tauri::async_runtime::spawn_blocking(super::claude_usage::fetch_account_usage)
        .await
        .map_err(|e| format!("consulta de uso cancelada: {e}"))?
}

/// Cupos de la cuenta Codex mediante `account/rateLimits/read` del app-server.
#[tauri::command]
pub async fn agent_codex_usage() -> Result<super::codex_usage::CodexAccountUsage, String> {
    tauri::async_runtime::spawn_blocking(super::codex_usage::fetch_account_usage)
        .await
        .map_err(|e| format!("consulta de uso cancelada: {e}"))?
}

/// Cupos de todos los agentes detectados, en una forma sola.
///
/// Es lo que consume el hover de la pill, así que corre entero en
/// `spawn_blocking` y cachea un minuto: el puntero entra y sale del disco
/// varias veces por minuto y no puede disparar cuatro consultas cada vez.
/// `force` es para el refresco a mano.
#[tauri::command]
pub async fn agent_quota_overview(
    force: Option<bool>,
) -> Result<super::quota::QuotaOverview, String> {
    let force = force.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || super::quota::fetch_overview(force))
        .await
        .map_err(|e| format!("consulta de cupos cancelada: {e}"))
}

/// Lista subcarpetas de `path` (vacío/`~` → home). Solo lectura; sin archivos.
#[tauri::command]
pub fn list_directories(
    path: Option<String>,
) -> Result<super::fs_browse::DirectoryListing, String> {
    super::fs_browse::list_directories(path)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshHostSecretFlags {
    pub host_id: String,
    pub has_passphrase: bool,
    pub has_password: bool,
}

/// Flags de secretos SSH por host (sin valores).
#[tauri::command]
pub fn ssh_host_secrets_status(state: State<AppState>) -> Vec<SshHostSecretFlags> {
    state
        .config
        .lock_or_recover()
        .ssh_hosts
        .iter()
        .map(|h| SshHostSecretFlags {
            host_id: h.id.clone(),
            has_passphrase: secrets::has_ssh_host_secret(&h.id, "passphrase"),
            has_password: secrets::has_ssh_host_secret(&h.id, "password"),
        })
        .collect()
}

/// Guarda o borra passphrase/password de un host. Valor vacío = borrar.
#[tauri::command]
pub fn ssh_set_host_secret(host_id: String, kind: String, value: String) -> Result<(), String> {
    secrets::set_ssh_host_secret(&host_id, &kind, &value).map_err(|e| e.to_string())
}

/// Borra todos los secretos de un host (al eliminar el registro).
#[tauri::command]
pub fn ssh_delete_host_secrets(host_id: String) -> Result<(), String> {
    secrets::delete_all_ssh_host_secrets(&host_id).map_err(|e| e.to_string())
}

/// Prueba la conexión SSH con el registro que manda la UI (puede ser draft).
/// Si el id ya está en config, actualiza `last_test_*` en disco.
#[tauri::command]
pub fn ssh_test_host(
    state: State<AppState>,
    host: SshHost,
) -> Result<super::ssh::SshTestResult, String> {
    let result = super::ssh::test_host(&host);
    {
        let mut cfg = state.config.lock_or_recover();
        if let Some(h) = cfg.ssh_hosts.iter_mut().find(|h| h.id == host.id) {
            h.last_test_ok = Some(result.ok);
            h.last_test_at = Some(result.checked_at);
            let path = state.dirs.config_path();
            if let Err(e) = cfg.save(&path) {
                tracing::warn!("no se pudo guardar resultado del test SSH: {e}");
            }
        }
    }
    Ok(result)
}

/// Lista hosts SSH desde config (sin secretos; son los mismos registros).
#[tauri::command]
pub fn ssh_list_hosts(state: State<AppState>) -> Vec<SshHost> {
    state.config.lock_or_recover().ssh_hosts.clone()
}

/// Aliases `Host` de `~/.ssh/config`: los mismos destinos que muestran
/// VS Code / Cursor en Remote-SSH. Solo nombres concretos (sin comodines).
#[tauri::command]
pub fn ssh_config_aliases() -> Vec<String> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from);
    let Some(home) = home else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(home.join(".ssh").join("config")) else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let mut it = line.split_whitespace();
        let Some(key) = it.next() else { continue };
        // `Host` define patrones; `HostName` es otra directiva y no cuenta.
        if !key.eq_ignore_ascii_case("host") {
            continue;
        }
        for name in it {
            if name.contains(['*', '?', '!']) {
                continue;
            }
            if !out.iter().any(|n| n == name) {
                out.push(name.to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sin_sesiones_abiertas_nadie_responde_a_un_nombre() {
        // El registro global está vacío en los tests: lo que se comprueba es
        // que un nombre desconocido sea «no hay» y no un error de estado.
        assert_eq!(session_by_label("agy"), Ok(None));
        assert_eq!(session_by_label("   "), Ok(None));
        assert!(set_label("no-existe", "agy").is_err());
    }

    #[test]
    fn una_etiqueta_libre_se_respeta_tal_cual() {
        let usadas = HashSet::new();
        assert_eq!(etiqueta_unica("revisor", &usadas), "revisor");
        assert_eq!(etiqueta_unica("  revisor  ", &usadas), "revisor");
    }

    #[test]
    fn la_repetida_se_numera_desde_dos() {
        let usadas: HashSet<String> = ["revisor".to_string()].into_iter().collect();
        assert_eq!(etiqueta_unica("revisor", &usadas), "revisor 2");
        let usadas: HashSet<String> = ["revisor", "revisor 2"]
            .into_iter()
            .map(str::to_string)
            .collect();
        assert_eq!(etiqueta_unica("revisor", &usadas), "revisor 3");
    }

    #[test]
    fn una_etiqueta_vacia_no_se_numera() {
        let usadas: HashSet<String> = ["".to_string()].into_iter().collect();
        assert_eq!(etiqueta_unica("   ", &usadas), "");
    }

    fn valor(env: &[(String, String)], nombre: &str) -> Option<String> {
        env.iter()
            .find(|(k, _)| k == nombre)
            .map(|(_, v)| v.clone())
    }

    #[test]
    fn las_vars_atic_llevan_sesion_profundidad_y_raiz() {
        let env = hub_env("clave1", 1, "raiz1", Some("padre1"));
        assert_eq!(valor(&env, "ATIC_SESSION").as_deref(), Some("clave1"));
        assert_eq!(valor(&env, "ATIC_DELEGATE_DEPTH").as_deref(), Some("1"));
        assert_eq!(valor(&env, "ATIC_ROOT").as_deref(), Some("raiz1"));
        assert_eq!(valor(&env, "ATIC_PARENT").as_deref(), Some("padre1"));
    }

    #[test]
    fn el_padre_solo_viaja_en_los_hijos() {
        let env = hub_env("clave1", 0, "clave1", None);
        assert_eq!(valor(&env, "ATIC_DELEGATE_DEPTH").as_deref(), Some("0"));
        assert_eq!(valor(&env, "ATIC_ROOT").as_deref(), Some("clave1"));
        assert!(valor(&env, "ATIC_PARENT").is_none(), "la UI no pone padre");
    }

    #[test]
    fn arrancar_sin_nada_no_trae_env() {
        assert!(
            StartOptions::default().env.is_empty(),
            "el env lo pone el puente, no el valor por defecto"
        );
    }
}
