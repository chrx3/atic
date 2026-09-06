//! Espera a fin de turno: acumula lo que dice el asistente y despierta al hub.
//!
//! Por qué existe: hoy `agent_send` dispara y los deltas salen por callback a
//! la UI. El hub necesita bloquear hasta `TurnEnd` para devolver el resultado
//! por MCP, así que este vigilante cuelga del mismo `on_delta` y despierta a
//! quien espera. Puro: se prueba con deltas sintéticos, sin backends.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

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
    permisos_pendientes: usize,
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
            permisos_pendientes: 0,
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
        permission_pending: bool,
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
                st.permisos_pendientes = 0;
            }
            AgentDelta::ItemAdd { item, .. } => match &item.kind {
                ItemKind::Message { role, text, .. } if *role == Role::Assistant => {
                    st.asistente.insert(item.id.clone());
                    st.orden.push(item.id.clone());
                    st.textos.insert(item.id.clone(), text.clone());
                }
                ItemKind::Permission { status, .. } if *status == PermissionStatus::Pending => {
                    st.permisos_pendientes += 1;
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
                // Un permiso que deja de estar pendiente libera uno.
                if st.permisos_pendientes > 0 {
                    if let Some(v) = &patch.status {
                        if let Ok(s) = serde_json::from_value::<PermissionStatus>(v.clone()) {
                            if s == PermissionStatus::Allowed || s == PermissionStatus::Denied {
                                // Los adaptadores solo mandan esos valores en
                                // permisos; un mensaje del asistente no trae
                                // `status` parseable a permiso.
                                st.permisos_pendientes -= 1;
                            }
                        }
                    }
                }
            }
            AgentDelta::TurnEnd { status, .. } => {
                st.running = false;
                st.ultimo = Some((*status, cap_text(&st.combinado())));
            }
            AgentDelta::Failed { message } => {
                st.running = false;
                st.ultimo = Some((TurnStatus::Failed, message.clone()));
            }
            AgentDelta::ThreadPatch { .. } => {}
        }
        st.seq += 1;
        self.cv.notify_all();
    }

    /// Espera hasta el fin del turno o el `deadline`. Sin turno corriendo,
    /// devuelve `Idle` al tiro con lo último que se vio.
    pub fn wait_until(&self, deadline: Instant) -> WaitOutcome {
        let mut st = candado(&self.inner);
        if !st.running {
            return WaitOutcome::Idle {
                last: st.ultimo.clone(),
            };
        }
        loop {
            let ahora = Instant::now();
            if ahora >= deadline {
                return WaitOutcome::Timeout {
                    text: cap_text(&st.combinado()),
                    permission_pending: st.permisos_pendientes > 0,
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
            if salto.timed_out() {
                return WaitOutcome::Timeout {
                    text: cap_text(&st.combinado()),
                    permission_pending: st.permisos_pendientes > 0,
                };
            }
        }
    }

    pub fn is_running(&self) -> bool {
        candado(&self.inner).running
    }
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
        let WaitOutcome::Timeout {
            permission_pending, ..
        } = salida
        else {
            panic!("se esperaba el vencimiento");
        };
        assert!(permission_pending, "hay un permiso sin contestar");
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
