//! Puente entre la capa de agentes y el frontend.
//!
//! Registro de sesiones vivas + los comandos de Tauri que las manejan. Los
//! eventos viajan por `agent-event`, cada uno etiquetado con la sesión que lo
//! produjo: la app puede tener varias abiertas a la vez y la UI necesita saber
//! a cuál pertenece cada línea.

use std::collections::HashMap;
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

/// La forma del globo de agentes.
///
/// `inset` tiene que valer lo mismo que `--inset` en la vista: es el margen
/// donde cabe la sombra, y la ventana mide el globo MÁS ese marco por los
/// cuatro lados. Sin descontarlo, el globo quedaría flotando lejos de la pill y
/// la punta no llegaría a tocarla. Así que `w` y `h` son los de
/// `tauri.conf.json` —la ventana— y el globo que se ve mide 580×520.
///
/// Los 62 salen de la sombra: `0 18px 44px`, y la regla es desplazamiento +
/// difuminado <= inset, o la ventana la corta en seco y en vez de sombra se ve
/// una banda oscura de bordes rectos.
///
/// El tamaño vive acá y no se mide de la ventana porque al cerrarse la burbuja
/// se repliega sobre la pill y queda guardada de ese tamaño: midiéndola, la
/// segunda apertura crecía hasta los 48px de la pill.
const BUBBLE: crate::floating::BubbleShape = crate::floating::BubbleShape {
    w: 704,
    h: 644,
    gap: 10,
    corner: 26,
    inset: 62,
};

/// Lo más chica que puede quedar la VENTANA sin que el compositor se rompa.
///
/// Por debajo de esto los dos grupos de la fila de abajo dejan de caber en una
/// línea y el botón de enviar se sale del panel — que es lo que pasaba a 580px
/// antes de partir el compositor en dos grupos.
const BUBBLE_MIN_W: i32 = 544;
const BUBBLE_MIN_H: i32 = 464;

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

/// Lo que la vista necesita saber al abrirse.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BubbleOpen {
    side: &'static str,
    /// En píxeles FÍSICOS, como todo lo que sale de la capa de geometría.
    ///
    /// La vista los divide por `devicePixelRatio`. Rust trabaja en físicos
    /// porque es lo que usa Win32; el CSS trabaja en lógicos. A 100% son lo
    /// mismo y la diferencia no existe, que es justo por lo que se cuela: a
    /// 125% —dpi 120, lo que tiene esta máquina— todo queda un 25% corrido.
    offset: i32,
    w: i32,
    h: i32,
    /// Cuánto dura el vuelo desde la pill. El contenido se funde en ese tiempo.
    flight: u64,
}

/// Abre o cierra la consola de agentes, que **sale de la pill**.
///
/// La ventana arranca del tamaño y en el sitio exactos de la pill, y crece
/// hasta el globo con el mismo tween que usa la rueda: es la ventana la que se
/// despliega, no una ventana nueva que aparece con una pestaña dibujada al
/// lado. Al cerrar hace el camino inverso y recién entonces se oculta.
///
/// Es un interruptor. Antes se cerraba al perder el foco, así que abrir el
/// historial para copiar algo que ibas a pegarle mataba la sesión justo cuando
/// la necesitabas.
#[tauri::command]
pub fn show_agents_window(app: AppHandle) {
    let Some(window) = app.get_webview_window("agents") else {
        return;
    };

    // Visible y con el foco = segunda pulsación de la rueda: se repliega.
    // Visible pero tapada por otra app = la querías ver, así que se trae al
    // frente sin volver a animarla.
    if window.is_visible().unwrap_or(false) {
        if window.is_focused().unwrap_or(false) {
            hide_agents_window(app);
        } else {
            // Traerla al frente Y volver a plantarle su tamaño. Lo segundo es lo
            // que la recompone: un vuelo que no llegó —la pantalla se bloqueó, el
            // equipo suspendió, algo pisó el tween— deja la ventana visible a
            // medio crecer, y como sigue estando «visible» esta rama era la única
            // que se alcanzaba. Sin reponer la geometría, la única salida era
            // reiniciar la app. Visto: quedó en 252x252 con el compositor
            // desbordado y ninguna pulsación la arreglaba.
            if let Some((target, anchor)) =
                crate::floating::bubble_rect(&app, "agents", "pill", bubble_shape(&app))
            {
                crate::floating::snap_rect(&app, "agents", target);
                let _ = app.emit(
                    "agents-bubble-anchor",
                    BubbleOpen {
                        side: anchor.side,
                        offset: anchor.offset,
                        w: target.w,
                        h: target.h,
                        // Ya está en su sitio: sin vuelo que acompañar.
                        flight: 0,
                    },
                );
            }
            let _ = window.set_focus();
        }
        return;
    }

    let Some((target, anchor)) =
        crate::floating::bubble_rect(&app, "agents", "pill", bubble_shape(&app))
    else {
        return;
    };

    // Frame cero: encima de la pill, del tamaño de la pill. Sin esto la ventana
    // aparecería ya hecha en su sitio y la punta sería un dibujo que insinúa un
    // origen que nunca ocurrió.
    let from = crate::floating::rect_of(&app, "pill").unwrap_or(target);
    crate::floating::snap_rect(&app, "agents", from);
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();

    let flight = crate::floating::tween(&app, "agents", target)
        .map(|f| f.ms)
        .unwrap_or(0);
    let _ = app.emit(
        "agents-bubble-anchor",
        BubbleOpen {
            side: anchor.side,
            offset: anchor.offset,
            w: target.w,
            h: target.h,
            flight,
        },
    );
}

/// Redimensiona la burbuja **anclada por el lado del que sale**.
///
/// El lado que mira a la pill NO se mueve: si la punta está arriba, crecer
/// empuja el borde de abajo; si está abajo, crece hacia arriba. Es lo que hace
/// que la punta siga tocando la pill mientras arrastrás, en vez de despegarse y
/// tener que reacomodar la ventana después.
///
/// `w`/`h` son de la VENTANA y en píxeles lógicos, que es lo que la vista
/// maneja. Se guardan para la próxima apertura.
#[tauri::command]
pub fn resize_agents_bubble(app: AppHandle, w: i32, h: i32, side: String, commit: bool) {
    let Some(window) = app.get_webview_window("agents") else {
        return;
    };
    let scale = window.scale_factor().unwrap_or(1.0);

    let w = w.max(BUBBLE_MIN_W);
    let h = h.max(BUBBLE_MIN_H);
    let (pw, ph) = (
        (w as f64 * scale).round() as i32,
        (h as f64 * scale).round() as i32,
    );

    let Some(now) = crate::floating::rect_of(&app, "agents") else {
        return;
    };

    // El borde anclado se queda quieto; el opuesto es el que se mueve.
    let (x, y) = match side.as_str() {
        "bottom" => (now.x, now.y + now.h - ph),
        "right" => (now.x + now.w - pw, now.y),
        // `top` y `left` anclan por arriba y por la izquierda, que es el origen.
        _ => (now.x, now.y),
    };

    crate::floating::snap_rect(&app, "agents", crate::floating::Rect { x, y, w: pw, h: ph });

    // Al disco solo al soltar. Mientras arrastrás esto llega en cada cuadro, y
    // reescribir el JSON sesenta veces por segundo castiga el disco para
    // guardar sesenta valores que nadie va a leer: el único que importa es el
    // último.
    if !commit {
        return;
    }
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

/// Repliega la burbuja sobre la pill y la oculta al llegar.
///
/// El ocultado va en un hilo y no en el frontend a propósito: si la ventana web
/// se colgara a mitad de la animación, quedaría una ventana muerta en pantalla
/// sin forma de cerrarla.
#[tauri::command]
pub fn hide_agents_window(app: AppHandle) {
    let Some(window) = app.get_webview_window("agents") else {
        return;
    };
    if !window.is_visible().unwrap_or(false) {
        return;
    }
    let _ = app.emit("agents-bubble-dismiss", ());

    let Some(home) = crate::floating::rect_of(&app, "pill") else {
        let _ = window.hide();
        return;
    };
    let flight = crate::floating::tween(&app, "agents", home)
        .map(|f| f.ms)
        .unwrap_or(0);

    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(flight));
        let _ = window.hide();
    });
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
    // Fuera del lock: `stop` espera a que el proceso termine de vaciar, y
    // sostener el mutex mientras tanto congelaría cualquier otra sesión.
    if let Some(mut entry) = taken {
        entry.session.stop();
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
