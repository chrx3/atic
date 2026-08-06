//! Puente entre la capa de agentes y el frontend.
//!
//! Registro de sesiones vivas + los comandos de Tauri que las manejan. Los
//! eventos viajan por `agent-event`, cada uno etiquetado con la sesión que lo
//! produjo: la app puede tener varias abiertas a la vez y la UI necesita saber
//! a cuál pertenece cada línea.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use atic_core::MutexExt;

use super::{
    claude_code::ClaudeCode, AgentBackend, AgentDelta, AgentSession, AgentSkill,
    PermissionDecision, StartOptions,
};

/// Una sesión viva más lo que hace falta para nombrarla sin volver a mirar la
/// lista de backends.
struct Entry {
    backend: String,
    display_name: String,
    session: Box<dyn AgentSession>,
}

/// Sesiones abiertas, por clave local.
///
/// La clave la genera Atic y no el backend: el id de sesión del agente llega
/// recién en el primer evento, y para entonces la UI ya necesita algo con qué
/// referirse a la conversación.
static SESSIONS: Mutex<Option<HashMap<String, Entry>>> = Mutex::new(None);

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
    ]
}

fn find(id: &str) -> Option<Box<dyn AgentBackend>> {
    backends().into_iter().find(|b| b.id() == id)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendInfo {
    pub id: String,
    pub display_name: String,
    /// Si está instalado. Un backend ausente se muestra deshabilitado en vez
    /// de ofrecerse y fallar recién al usarlo.
    pub available: bool,
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
                })
                .collect()
        })
        .unwrap_or_default()
}

/// La forma del globo de agentes, en píxeles lógicos.
///
/// Antes estos números eran los de la VENTANA: incluían un marco transparente
/// de 62 px por lado donde cabía la sombra, porque una ventana recorta la suya.
/// El globo visible medía 580×520 y toda la geometría tenía que descontar el
/// marco. Dentro del overlay no hay ventana que recorte nada, así que esto es
/// el globo y se acabó el `inset`.
///
/// El tamaño vive acá y no se mide del DOM porque al cerrarse la burbuja se
/// repliega sobre la pill; midiéndola, la segunda apertura crecía hasta el
/// tamaño de la pill.
const BUBBLE: crate::floating::BubbleShape = crate::floating::BubbleShape {
    w: 580,
    h: 520,
    gap: 10,
    corner: 26,
};

/// Lo más chico que puede quedar el globo sin que el compositor se rompa.
///
/// Por debajo de esto los dos grupos de la fila de abajo dejan de caber en una
/// línea y el botón de enviar se sale del panel. Son los de antes menos el
/// marco de la sombra, que ya no existe: 544−124 y 464−124.
const BUBBLE_MIN_W: i32 = 420;
const BUBBLE_MIN_H: i32 = 340;

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

/// ¿Está la consola a la vista?
///
/// Antes lo contestaba `window.is_visible()`. Sin ventana propia, el estado
/// vive acá: es lo único que queda de ella, y hace falta porque el atajo y la
/// rueda son interruptores. También lo lee el historial del portapapeles
/// (`agents_open`) para insertar en el compositor.
static OPEN: AtomicBool = AtomicBool::new(false);

const AGENTS_ANCHOR: &str = "agents-bubble-anchor";
const AGENTS_DISMISS: &str = "agents-bubble-dismiss";

/// ¿La consola está desplegada? Lo pregunta el historial del portapapeles, que
/// con ella abierta inserta en el compositor en vez de pegar afuera.
pub fn agents_open() -> bool {
    OPEN.load(Ordering::Relaxed)
}

/// Abre o cierra la consola de agentes, que **sale de la pill**.
///
/// Geometría vía `panel_float` con la `BubbleShape` de agentes (tamaño
/// guardado). Es un interruptor: no se cierra al perder el foco.
#[tauri::command]
pub fn show_agents_window(app: AppHandle) {
    let _ = crate::panel_float::toggle(
        &app,
        &OPEN,
        bubble_shape(&app),
        AGENTS_ANCHOR,
        AGENTS_DISMISS,
    );
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

/// Repliega la burbuja sobre la pill.
#[tauri::command]
pub fn hide_agents_window(app: AppHandle) {
    crate::panel_float::hide(&app, &OPEN, AGENTS_DISMISS);
}

/// Qué agentes hay y cuáles se pueden usar.
///
/// `is_available` lanza un proceso por backend, así que esto no es gratis: la
/// UI debería llamarlo al abrir la vista, no en cada render.
#[tauri::command]
pub fn agent_backends() -> Vec<BackendInfo> {
    backends()
        .iter()
        .map(|b| BackendInfo {
            id: b.id().to_string(),
            display_name: b.display_name().to_string(),
            available: b.is_available(),
        })
        .collect()
}

/// Lo que la UI elige antes de arrancar. Todo opcional: sin nada, el agente
/// corre con su propia configuración.
#[derive(Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartRequest {
    pub cwd: Option<String>,
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
    let options = options.unwrap_or_default();
    let StartRequest {
        cwd,
        resume,
        model,
        effort,
        fast,
        permission_mode,
        mcp_config,
        add_dirs,
        fork,
    } = options;
    let agent = find(&backend).ok_or_else(|| format!("backend desconocido: {backend}"))?;
    let key = uuid::Uuid::new_v4().to_string();
    let display_name = agent.display_name().to_string();

    // Seguir el hilo desde ANTES de arrancar: el primer delta puede llegar
    // mientras `start` todavía no volvió, y sin el hilo abierto se perdería.
    super::store::open(&key, &backend, &display_name, cwd.as_deref().unwrap_or(""));

    let emit_key = key.clone();
    let emit_backend = backend.clone();
    let emit_name = display_name.clone();
    let session = agent.start(
        StartOptions {
            cwd,
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
            add_dirs,
        },
        Box::new(move |delta| {
            // Primero al store, después a la ventana. El orden importa poco
            // para la vista y mucho para el disco: si emitir fallara, el hilo
            // ya quedó aplicado igual.
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
                backend,
                display_name,
                session,
            },
        );
    Ok(key)
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

#[tauri::command]
pub fn agent_stop(app: AppHandle, session: String) {
    let taken = SESSIONS
        .lock_or_recover()
        .as_mut()
        .and_then(|s| s.remove(&session));
    // Fuera del lock Y en otro hilo: `stop` puede tardar en matar el proceso
    // (espera corta + kill). Si el comando IPC espera, el botón "Detener" de
    // la UI parece muerto aunque la sesión ya salió de la lista del frontend.
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
pub fn agent_claude_transcript(
    cwd: String,
    id: String,
) -> Result<Vec<super::model::Turn>, String> {
    super::claude_sessions::load_transcript(&cwd, &id)
}
