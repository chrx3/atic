//! Lo que todo adaptador necesita: emitir deltas y saber en qué turno está.
//!
//! Vive aparte porque el problema es de todos los backends y la solución no
//! tiene nada que ver con el protocolo de ninguno. Un turno es un ciclo
//! usuario → agente, así que lo **abre quien escribe** y lo consulta quien lee;
//! los dos hilos comparten este estado, y sin eso cada lado inventaría su
//! propia numeración y la conversación guardada quedaría partida en dos.

use std::sync::{Arc, Mutex};

use super::model::{AgentDelta, TurnId};

/// Handle clonable para compartir el callback entre los hilos que emiten.
#[derive(Clone)]
pub struct Emit(pub Arc<Box<dyn Fn(AgentDelta) + Send + Sync + 'static>>);

impl Emit {
    pub fn new(f: Box<dyn Fn(AgentDelta) + Send + Sync + 'static>) -> Self {
        Self(Arc::new(f))
    }

    pub fn send(&self, delta: AgentDelta) {
        (self.0)(delta);
    }

    /// Manda una tanda en orden. Es lo que devuelven los traductores.
    pub fn all(&self, deltas: impl IntoIterator<Item = AgentDelta>) {
        for d in deltas {
            self.send(d);
        }
    }
}

/// Qué turno está abierto, compartido entre el hilo lector y el que escribe.
#[derive(Default)]
pub struct Turns {
    pub current: Option<TurnId>,
    seq: u64,
}

impl Turns {
    /// Abre uno nuevo y devuelve su id.
    pub fn open(&mut self) -> TurnId {
        self.seq += 1;
        let id = format!("t{}", self.seq);
        self.current = Some(id.clone());
        id
    }
}

/// Abre un turno y anuncia su comienzo. Para quien manda un mensaje.
pub fn start_turn(turns: &Mutex<Turns>, emit: &Emit) -> TurnId {
    let id = lock(turns).open();
    emit.send(AgentDelta::TurnStart { turn: id.clone() });
    id
}

/// Cierra el turno abierto, si hay: lo próximo que llegue abrirá otro.
pub fn end_turn(turns: &Mutex<Turns>) {
    lock(turns).current = None;
}

/// El turno al que colgar lo que llega, abriendo uno si no hay.
///
/// Empuja el `TurnStart` a `out` cuando hubo que abrirlo. Un agente puede
/// hablar sin que nadie le haya escrito —al reanudar, o cuando termina algo que
/// quedó corriendo— y esos items tienen que ir a parar a algún lado en vez de
/// perderse.
pub fn ensure_turn(turns: &Mutex<Turns>, out: &mut Vec<AgentDelta>) -> TurnId {
    let mut turns = lock(turns);
    if let Some(id) = &turns.current {
        return id.clone();
    }
    let id = turns.open();
    out.push(AgentDelta::TurnStart { turn: id.clone() });
    id
}

/// Toma el candado aunque esté envenenado.
///
/// Un `Mutex` envenenado significa que el hilo que lo tenía murió con pánico.
/// Propagarlo mataría también al que quedó vivo, y lo que se protege acá es un
/// contador de turnos: seguir con el estado que haya es estrictamente mejor que
/// tirar una sesión con trabajo del usuario adentro.
fn lock(turns: &Mutex<Turns>) -> std::sync::MutexGuard<'_, Turns> {
    match turns.lock() {
        Ok(t) => t,
        Err(e) => e.into_inner(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn los_turnos_se_numeran_en_orden() {
        let t = Mutex::new(Turns::default());
        assert_eq!(lock(&t).open(), "t1");
        assert_eq!(lock(&t).open(), "t2");
    }

    #[test]
    fn ensure_reusa_el_abierto_y_no_anuncia_nada() {
        let t = Mutex::new(Turns::default());
        lock(&t).open();
        let mut out = Vec::new();
        assert_eq!(ensure_turn(&t, &mut out), "t1");
        assert!(out.is_empty(), "no hay turno nuevo que anunciar");
    }

    #[test]
    fn ensure_abre_uno_y_lo_anuncia_si_no_habia() {
        let t = Mutex::new(Turns::default());
        let mut out = Vec::new();
        let id = ensure_turn(&t, &mut out);
        assert_eq!(id, "t1");
        assert!(matches!(out[0], AgentDelta::TurnStart { .. }));
    }

    /// Cerrado el turno, lo siguiente abre otro en vez de colgarse del viejo.
    #[test]
    fn despues_de_cerrar_se_abre_uno_nuevo() {
        let t = Mutex::new(Turns::default());
        lock(&t).open();
        end_turn(&t);
        let mut out = Vec::new();
        assert_eq!(ensure_turn(&t, &mut out), "t2");
        assert_eq!(out.len(), 1);
    }

    /// Un candado envenenado no puede tumbar una sesión con trabajo adentro.
    #[test]
    fn el_candado_envenenado_no_mata_la_sesion() {
        let t = Arc::new(Mutex::new(Turns::default()));
        let otro = t.clone();
        let _ = std::thread::spawn(move || {
            let _g = otro.lock().unwrap();
            panic!("hilo que muere con el candado tomado");
        })
        .join();

        assert!(t.is_poisoned());
        assert_eq!(lock(&t).open(), "t1", "se sigue igual");
    }
}
