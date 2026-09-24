//! Espera a fin de turno: acumula lo que dice el asistente y despierta al hub.
//!
//! Por qué existe: hoy `agent_send` dispara y los deltas salen por callback a
//! la UI. El hub necesita bloquear hasta `TurnEnd` para devolver el resultado
//! por MCP, así que este vigilante cuelga del mismo `on_delta` y despierta a
//! quien espera. Puro: se prueba con deltas sintéticos, sin backends.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use super::api::PendingPermission;
use super::graph::cap_text;
use crate::agents::model::{
    AgentDelta, ItemId, ItemKind, PermissionStatus, Role, TurnId, TurnStatus,
};

struct TurnState {
    running: bool,
    turn: Option<TurnId>,
    /// Texto por item, en orden de llegada: el parche trae el texto completo
    /// y reemplaza, así que un solo buffer duplicaría la respuesta.
    textos: HashMap<ItemId, String>,
    orden: Vec<ItemId>,
    asistente: HashSet<ItemId>,
    /// Permisos sin contestar, en orden de llegada: el padre los necesita con
    /// id para poder contestarlos por el hub, no solo contarlos.
    permisos_pendientes: Vec<PendingPermission>,
    ultimo: Option<(TurnStatus, String)>,
    seq: u64,
}

impl TurnState {
    fn nueva() -> Self {
        Self {
            running: false,
            turn: None,
            textos: HashMap::new(),
            orden: Vec::new(),
            asistente: HashSet::new(),
            permisos_pendientes: Vec::new(),
            ultimo: None,
            seq: 0,
        }
    }

    fn combinado(&self) -> String {
        let mut todo = String::new();
        for id in &self.orden {
            let Some(t) = self.textos.get(id) else {
                continue;
            };
            if t.is_empty() {
                continue;
            }
            // Un salto entre mensajes: pegados salía «…en paralelo.depth=1…».
            if !todo.is_empty() {
                todo.push('\n');
            }
            todo.push_str(t);
        }
        todo
    }
}

pub struct TurnWatch {
    inner: Mutex<TurnState>,
    cv: Condvar,
}

pub enum WaitOutcome {
    Ended {
        status: TurnStatus,
        text: String,
    },
    Timeout {
        text: String,
        permissions: Vec<PendingPermission>,
    },
    /// Apareció un permiso nuevo mientras se esperaba: se vuelve antes del
    /// tope para que el padre lo pueda contestar.
    Permission {
        text: String,
        permissions: Vec<PendingPermission>,
    },
    Idle {
        last: Option<(TurnStatus, String)>,
    },
}

impl TurnWatch {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(TurnState::nueva()),
            cv: Condvar::new(),
        })
    }

    /// Observa un delta. Lo llama `on_delta` en primera línea, antes del store
    /// y del emit: quien espera tiene que despertar aunque lo demás falle.
    pub fn observe(&self, delta: &AgentDelta) {
        let mut st = candado(&self.inner);
        match delta {
            AgentDelta::TurnStart { turn } => {
                st.running = true;
                st.turn = Some(turn.clone());
                st.textos.clear();
                st.orden.clear();
                st.asistente.clear();
                st.permisos_pendientes.clear();
            }
            AgentDelta::ItemAdd { item, .. } => match &item.kind {
                ItemKind::Message { role, text, .. } if *role == Role::Assistant => {
                    st.asistente.insert(item.id.clone());
                    st.orden.push(item.id.clone());
                    st.textos.insert(item.id.clone(), text.clone());
                }
                ItemKind::Permission {
                    tool,
                    description,
                    input,
                    status,
                } if *status == PermissionStatus::Pending => {
                    if !st.permisos_pendientes.iter().any(|p| p.id == item.id) {
                        st.permisos_pendientes.push(PendingPermission {
                            id: item.id.clone(),
                            tool: tool.clone(),
                            description: description.clone(),
                            input: resumen_input(input),
                        });
                    }
                }
                _ => {}
            },
            AgentDelta::ItemChunk { item, text } => {
                if st.asistente.contains(item) {
                    st.textos.entry(item.clone()).or_default().push_str(text);
                }
            }
            AgentDelta::ItemPatch { item, patch } => {
                if st.asistente.contains(item) {
                    // El texto del parche es autoritativo: reemplaza lo acumulado.
                    if let Some(t) = &patch.text {
                        st.textos.insert(item.clone(), t.clone());
                    }
                }
                // Un permiso que deja de estar pendiente sale de la lista.
                if let Some(v) = &patch.status {
                    if let Ok(s) = serde_json::from_value::<PermissionStatus>(v.clone()) {
                        if s == PermissionStatus::Allowed || s == PermissionStatus::Denied {
                            st.permisos_pendientes.retain(|p| &p.id != item);
                        }
                    }
                }
            }
            AgentDelta::TurnEnd { status, .. } => {
                st.running = false;
                // Con el turno cerrado ningún permiso sigue esperando.
                st.permisos_pendientes.clear();
                st.ultimo = Some((*status, cap_text(&st.combinado())));
            }
            AgentDelta::Failed { message } => {
                st.running = false;
                st.permisos_pendientes.clear();
                st.ultimo = Some((TurnStatus::Failed, message.clone()));
            }
            AgentDelta::ThreadPatch { .. } => {}
        }
        st.seq += 1;
        self.cv.notify_all();
    }

    /// Espera hasta el fin del turno, un permiso nuevo o el `deadline`. Sin
    /// turno corriendo, devuelve `Idle` al tiro con lo último que se vio.
    ///
    /// Solo corta antes por un permiso que **no** estaba al empezar: si el
    /// padre ya lo vio y prefiere que lo conteste un humano en Atic, volver a
    /// esperar no puede devolver al tiro, o lo dejaría girando en un bucle.
    pub fn wait_until(&self, deadline: Instant) -> WaitOutcome {
        let mut st = candado(&self.inner);
        if !st.running {
            return WaitOutcome::Idle {
                last: st.ultimo.clone(),
            };
        }
        let conocidos: HashSet<String> = st
            .permisos_pendientes
            .iter()
            .map(|p| p.id.clone())
            .collect();
        loop {
            let ahora = Instant::now();
            if ahora >= deadline {
                return WaitOutcome::Timeout {
                    text: cap_text(&st.combinado()),
                    permissions: st.permisos_pendientes.clone(),
                };
            }
            let resto = deadline - ahora;
            let (nueva, salto) = self
                .cv
                .wait_timeout(st, resto)
                .unwrap_or_else(|e| e.into_inner());
            st = nueva;
            if !st.running {
                let (status, text) = st
                    .ultimo
                    .clone()
                    .unwrap_or((TurnStatus::Done, String::new()));
                return WaitOutcome::Ended { status, text };
            }
            if st
                .permisos_pendientes
                .iter()
                .any(|p| !conocidos.contains(&p.id))
            {
                return WaitOutcome::Permission {
                    text: cap_text(&st.combinado()),
                    permissions: st.permisos_pendientes.clone(),
                };
            }
            if salto.timed_out() {
                return WaitOutcome::Timeout {
                    text: cap_text(&st.combinado()),
                    permissions: st.permisos_pendientes.clone(),
                };
            }
        }
    }

    pub fn is_running(&self) -> bool {
        candado(&self.inner).running
    }

    /// Permisos que siguen esperando respuesta en el turno actual.
    pub fn pending_permissions(&self) -> Vec<PendingPermission> {
        candado(&self.inner).permisos_pendientes.clone()
    }

    /// Saca un permiso ya contestado. Hace falta porque no todos los
    /// adaptadores mandan el parche de estado al contestar (Claude Code no),
    /// y sin esto el permiso seguiría figurando pendiente para el padre.
    pub fn resolve_permission(&self, id: &str) {
        let mut st = candado(&self.inner);
        st.permisos_pendientes.retain(|p| p.id != id);
        st.seq += 1;
        self.cv.notify_all();
    }
}

/// Tope del input que ve el padre por cada permiso.
const INPUT_CAP_CHARS: usize = 400;

/// El input de la herramienta en JSON compacto, recortado por chars para no
/// romper UTF-8. Un `null` queda vacío: no aporta nada a la decisión.
fn resumen_input(input: &serde_json::Value) -> String {
    if input.is_null() {
        return String::new();
    }
    let texto = input.to_string();
    if texto.chars().count() <= INPUT_CAP_CHARS {
        return texto;
    }
    let mut corto: String = texto.chars().take(INPUT_CAP_CHARS).collect();
    corto.push('…');
    corto
}

/// Toma el candado aunque esté envenenado: un hilo muerto no puede dejar al hub
/// sin respuesta para siempre.
fn candado<'a>(m: &'a Mutex<TurnState>) -> std::sync::MutexGuard<'a, TurnState> {
    match m.lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::model::{Item, ItemKind, ItemPatch};
    use std::time::Duration;

    // Atajos para armar deltas sintéticos sin backend.
    fn mensaje_asistente(id: &str, texto: &str) -> AgentDelta {
        AgentDelta::ItemAdd {
            turn: "t1".into(),
            item: Item::new(
                id,
                ItemKind::Message {
                    role: Role::Assistant,
                    text: texto.into(),
                    streaming: true,
                },
            ),
        }
    }

    fn mensaje_usuario(id: &str, texto: &str) -> AgentDelta {
        AgentDelta::ItemAdd {
            turn: "t1".into(),
            item: Item::new(
                id,
                ItemKind::Message {
                    role: Role::User,
                    text: texto.into(),
                    streaming: false,
                },
            ),
        }
    }

    #[test]
    fn sin_turno_corriendo_devuelve_idle_de_inmediato() {
        let w = TurnWatch::new();
        let antes = Instant::now();
        let salida = w.wait_until(Instant::now() + Duration::from_secs(5));
        assert!(antes.elapsed() < Duration::from_secs(1), "no espera");
        assert!(matches!(salida, WaitOutcome::Idle { last: None }));
    }

    #[test]
    fn el_texto_del_asistente_se_acumula_y_el_del_usuario_no() {
        let w = TurnWatch::new();
        w.observe(&AgentDelta::TurnStart { turn: "t1".into() });
        w.observe(&mensaje_asistente("m1", "hola "));
        w.observe(&mensaje_usuario("u1", "ignórame"));
        w.observe(&AgentDelta::ItemChunk {
            item: "m1".into(),
            text: "mundo".into(),
        });
        w.observe(&AgentDelta::ItemChunk {
            item: "u1".into(),
            text: "yo no cuento".into(),
        });
        let w2 = w.clone();
        let hilo =
            std::thread::spawn(move || w2.wait_until(Instant::now() + Duration::from_secs(2)));
        // Deja que el hilo entre al `wait` antes de cerrar: si el `TurnEnd`
        // gana, el `wait` ve `Idle` y el test queda a la suerte del planificador.
        std::thread::sleep(Duration::from_millis(50));
        w.observe(&AgentDelta::TurnEnd {
            turn: "t1".into(),
            status: TurnStatus::Done,
            cost_usd: None,
            duration_ms: None,
        });
        let WaitOutcome::Ended { status, text } = hilo.join().unwrap() else {
            panic!("se esperaba el fin del turno");
        };
        assert_eq!(status, TurnStatus::Done);
        assert_eq!(text, "hola mundo");
    }

    #[test]
    fn dos_mensajes_del_asistente_van_en_lineas_distintas() {
        let w = TurnWatch::new();
        w.observe(&AgentDelta::TurnStart { turn: "t1".into() });
        w.observe(&mensaje_asistente("m1", "en paralelo."));
        w.observe(&mensaje_asistente("m2", "depth=1"));
        let w2 = w.clone();
        let hilo =
            std::thread::spawn(move || w2.wait_until(Instant::now() + Duration::from_secs(2)));
        std::thread::sleep(Duration::from_millis(50));
        w.observe(&AgentDelta::TurnEnd {
            turn: "t1".into(),
            status: TurnStatus::Done,
            cost_usd: None,
            duration_ms: None,
        });
        let WaitOutcome::Ended { text, .. } = hilo.join().unwrap() else {
            panic!("se esperaba el fin del turno");
        };
        assert_eq!(text, "en paralelo.\ndepth=1");
    }

    #[test]
    fn un_patch_reemplaza_el_texto_del_item_y_no_lo_duplica() {
        let w = TurnWatch::new();
        w.observe(&AgentDelta::TurnStart { turn: "t1".into() });
        w.observe(&mensaje_asistente("m1", ""));
        w.observe(&AgentDelta::ItemChunk {
            item: "m1".into(),
            text: "hol".into(),
        });
        w.observe(&AgentDelta::ItemPatch {
            item: "m1".into(),
            patch: ItemPatch {
                text: Some("hola mundo".into()),
                ..Default::default()
            },
        });
        w.observe(&AgentDelta::TurnEnd {
            turn: "t1".into(),
            status: TurnStatus::Done,
            cost_usd: None,
            duration_ms: None,
        });
        let WaitOutcome::Idle {
            last: Some((_, text)),
        } = w.wait_until(Instant::now() + Duration::from_secs(1))
        else {
            panic!("se esperaba el último resultado");
        };
        assert_eq!(text, "hola mundo");
    }

    #[test]
    fn turn_end_despierta_al_que_espera_con_el_status() {
        let w = TurnWatch::new();
        w.observe(&AgentDelta::TurnStart { turn: "t1".into() });
        let w2 = w.clone();
        let hilo =
            std::thread::spawn(move || w2.wait_until(Instant::now() + Duration::from_secs(5)));
        // Le da al hilo un momento para entrar al `wait`.
        std::thread::sleep(Duration::from_millis(50));
        w.observe(&AgentDelta::TurnEnd {
            turn: "t1".into(),
            status: TurnStatus::Cancelled,
            cost_usd: None,
            duration_ms: None,
        });
        let WaitOutcome::Ended { status, .. } = hilo.join().unwrap() else {
            panic!("se esperaba el fin");
        };
        assert_eq!(status, TurnStatus::Cancelled);
    }

    #[test]
    fn failed_cierra_el_turno_como_fallido() {
        let w = TurnWatch::new();
        w.observe(&AgentDelta::TurnStart { turn: "t1".into() });
        w.observe(&AgentDelta::Failed {
            message: "se cayó el CLI".into(),
        });
        assert!(!w.is_running());
        let WaitOutcome::Idle {
            last: Some((status, text)),
        } = w.wait_until(Instant::now() + Duration::from_secs(1))
        else {
            panic!("se esperaba el último fallo");
        };
        assert_eq!(status, TurnStatus::Failed);
        assert_eq!(text, "se cayó el CLI");
    }

    #[test]
    fn al_vencer_dice_si_hay_permiso_pendiente() {
        let w = TurnWatch::new();
        w.observe(&AgentDelta::TurnStart { turn: "t1".into() });
        w.observe(&AgentDelta::ItemAdd {
            turn: "t1".into(),
            item: Item::new(
                "p1",
                ItemKind::Permission {
                    tool: "Edit".into(),
                    description: "escribir x.txt".into(),
                    input: serde_json::Value::Null,
                    status: PermissionStatus::Pending,
                },
            ),
        });
        let salida = w.wait_until(Instant::now() + Duration::from_millis(50));
        let WaitOutcome::Timeout { permissions, .. } = salida else {
            panic!("se esperaba el vencimiento");
        };
        assert_eq!(permissions.len(), 1, "hay un permiso sin contestar");
        assert_eq!(permissions[0].id, "p1");
        assert_eq!(permissions[0].tool, "Edit");
    }

    fn permiso(id: &str, input: serde_json::Value) -> AgentDelta {
        AgentDelta::ItemAdd {
            turn: "t1".into(),
            item: Item::new(
                id,
                ItemKind::Permission {
                    tool: "Bash".into(),
                    description: "correr un comando".into(),
                    input,
                    status: PermissionStatus::Pending,
                },
            ),
        }
    }

    #[test]
    fn un_permiso_nuevo_corta_la_espera_antes_del_tope() {
        let w = TurnWatch::new();
        w.observe(&AgentDelta::TurnStart { turn: "t1".into() });
        let w2 = w.clone();
        let antes = Instant::now();
        let hilo =
            std::thread::spawn(move || w2.wait_until(Instant::now() + Duration::from_secs(5)));
        std::thread::sleep(Duration::from_millis(50));
        w.observe(&permiso("p1", serde_json::json!({"command": "ls"})));
        let WaitOutcome::Permission { permissions, .. } = hilo.join().unwrap() else {
            panic!("se esperaba el aviso del permiso");
        };
        assert!(
            antes.elapsed() < Duration::from_secs(2),
            "no espera el tope"
        );
        assert_eq!(permissions[0].id, "p1");
        assert_eq!(permissions[0].input, r#"{"command":"ls"}"#);
    }

    #[test]
    fn un_permiso_que_ya_estaba_no_corta_la_espera_de_nuevo() {
        let w = TurnWatch::new();
        w.observe(&AgentDelta::TurnStart { turn: "t1".into() });
        w.observe(&permiso("p1", serde_json::Value::Null));
        let salida = w.wait_until(Instant::now() + Duration::from_millis(80));
        let WaitOutcome::Timeout { permissions, .. } = salida else {
            panic!("un permiso ya visto no debe devolver al tiro");
        };
        assert_eq!(permissions.len(), 1);
        assert_eq!(permissions[0].input, "", "null no aporta nada");
    }

    #[test]
    fn contestar_o_parchear_saca_el_permiso_de_la_lista() {
        let w = TurnWatch::new();
        w.observe(&AgentDelta::TurnStart { turn: "t1".into() });
        w.observe(&permiso("p1", serde_json::Value::Null));
        w.observe(&permiso("p2", serde_json::Value::Null));
        w.observe(&AgentDelta::ItemPatch {
            item: "p1".into(),
            patch: ItemPatch {
                status: serde_json::to_value(PermissionStatus::Allowed).ok(),
                ..Default::default()
            },
        });
        let ids: Vec<String> = w.pending_permissions().into_iter().map(|p| p.id).collect();
        assert_eq!(ids, vec!["p2".to_string()]);
        w.resolve_permission("p2");
        assert!(w.pending_permissions().is_empty());
    }

    #[test]
    fn el_input_largo_se_recorta_sin_romper_utf8() {
        let largo = serde_json::json!({ "texto": "ñ".repeat(1000) });
        let r = resumen_input(&largo);
        assert!(r.ends_with('…'));
        assert_eq!(r.chars().count(), INPUT_CAP_CHARS + 1);
    }

    #[test]
    fn el_texto_se_recorta_por_la_cola() {
        let w = TurnWatch::new();
        w.observe(&AgentDelta::TurnStart { turn: "t1".into() });
        w.observe(&mensaje_asistente("m1", &"y".repeat(40 * 1024)));
        w.observe(&AgentDelta::TurnEnd {
            turn: "t1".into(),
            status: TurnStatus::Done,
            cost_usd: None,
            duration_ms: None,
        });
        let WaitOutcome::Idle {
            last: Some((_, text)),
        } = w.wait_until(Instant::now() + Duration::from_secs(1))
        else {
            panic!("se esperaba texto recortado");
        };
        assert!(text.starts_with("[…recortado…]\n"));
    }
}
