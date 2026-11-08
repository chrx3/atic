//! El equipo con el mouse y el teclado. El gancho de cada sistema
//! (`sys::start_capture`) le pasa cada evento a [`Grab`], que decide al tiro
//! si el sistema lo ve ([`Verdict::Pass`]) o se va al otro equipo
//! ([`Verdict::Swallow`]).
//!
//! - El cursor cruza cuando toca el borde hacia el otro (sin otra pantalla de
//!   este lado) empujando hacia fuera, con la sesión arriba y sin botones
//!   apretados (un arrastre a medias no se parte entre dos equipos).
//! - Mientras está en el otro, el cursor de aquí queda quieto y todo se manda.
//! - Vuelve cuando el otro lo devuelve ([`Grab::back`]), si se cae la sesión,
//!   o con el atajo de emergencia Ctrl+Alt+Esc.
//! - Las teclas que estaban apretadas aquí al cruzar se sueltan aquí, y las
//!   apretadas en el otro al volver no se sueltan aquí: ningún lado queda con
//!   una tecla pegada.

use std::collections::HashSet;
use std::sync::{Mutex, MutexGuard};

use tokio::sync::mpsc::UnboundedSender;

use crate::keys;
use crate::layout::{Screens, Side};
use crate::proto::{Button, Msg};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// El sistema lo procesa como siempre.
    Pass,
    /// Se lo traga el gancho: no llega a nada de este equipo.
    Swallow,
}

/// Se llama cuando un arrastre llega al borde hacia el otro equipo.
type DragHook = Box<dyn Fn() + Send + Sync>;

pub struct Grab {
    state: Mutex<State>,
    tx: UnboundedSender<Msg>,
    on_drag_edge: Mutex<Option<DragHook>>,
}

struct State {
    /// De qué lado está el otro; `None`: no se comparte (el cursor no cruza).
    side: Option<Side>,
    /// El lado por el que salió el cursor, para volver por el mismo aunque cambie la configuración.
    exit_side: Side,
    screens: Screens,
    connected: bool,
    remote: bool,
    /// Dónde salió (fracción del borde), para devolver ahí con el atajo.
    exit_along: f64,
    local_buttons: HashSet<Button>,
    /// Este arrastre ya avisó que llegó al borde (una vez por arrastre).
    drag_told: bool,
    local_keys: HashSet<u16>,
    remote_keys: HashSet<u16>,
}

impl Grab {
    /// `side`: de qué lado de este equipo está el otro (`None`: todavía no se
    /// comparte). Lo que se manda sale por `tx`.
    pub fn new(side: Option<Side>, screens: Screens, tx: UnboundedSender<Msg>) -> Self {
        Self {
            state: Mutex::new(State {
                side,
                exit_side: side.unwrap_or(Side::Right),
                screens,
                connected: false,
                remote: false,
                exit_along: 0.5,
                local_buttons: HashSet::new(),
                drag_told: false,
                local_keys: HashSet::new(),
                remote_keys: HashSet::new(),
            }),
            tx,
            on_drag_edge: Mutex::new(None),
        }
    }

    /// Qué hacer cuando un arrastre (botón izquierdo apretado) llega al borde
    /// hacia el otro: mandar los archivos que se arrastran, si hay.
    pub fn on_drag_edge(&self, hook: impl Fn() + Send + Sync + 'static) {
        *self.on_drag_edge.lock().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(hook));
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn is_remote(&self) -> bool {
        self.lock().remote
    }

    /// Cambia de qué lado está el otro, o deja de compartir (`None`).
    pub fn set_side(&self, side: Option<Side>) {
        self.lock().side = side;
    }

    pub fn set_screens(&self, screens: Screens) {
        if !screens.is_empty() {
            self.lock().screens = screens;
        }
    }

    /// Con la sesión abajo, el cursor no cruza; si estaba en el otro, vuelve.
    pub fn set_connected(&self, connected: bool) {
        let mut s = self.lock();
        s.connected = connected;
        let to = if !connected && s.remote {
            let along = s.exit_along;
            come_back(&mut s, along)
        } else {
            None
        };
        drop(s);
        release(to);
    }

    /// El otro devolvió el cursor por el borde, a `along` de su largo.
    pub fn back(&self, along: f64) {
        let mut s = self.lock();
        let to = if s.remote { come_back(&mut s, along) } else { None };
        drop(s);
        release(to);
    }

    /// El mouse se movió: `(x, y)` es donde el sistema dejaría el cursor y
    /// `(dx, dy)` cuánto se movió. `strict`: exige empujar hacia fuera (en
    /// macOS el movimiento llega aunque el cursor esté contra el borde; en
    /// Windows el punto puede quedar recortado y el empuje no se ve).
    pub fn on_move(&self, x: f64, y: f64, dx: f64, dy: f64, strict: bool) -> Verdict {
        let mut s = self.lock();
        if s.remote {
            let (dx, dy) = (dx.round() as i32, dy.round() as i32);
            if dx != 0 || dy != 0 {
                let _ = self.tx.send(Msg::Move { dx, dy });
            }
            return Verdict::Swallow;
        }
        let Some(side) = s.side else { return Verdict::Pass };
        let push = side.outward(dx, dy);
        let pushing = !(push < 0.0 || (strict && push == 0.0));
        // Arrastrando con el botón izquierdo contra el borde: el cursor no cruza
        // (un arrastre no se parte entre dos equipos), pero lo que se arrastra puede ir.
        let dragging = s.local_buttons.len() == 1 && s.local_buttons.contains(&Button::Left);
        if s.connected && dragging && pushing && !s.drag_told && s.screens.at_edge(side, x, y).is_some() {
            s.drag_told = true;
            drop(s);
            if let Some(hook) = self.on_drag_edge.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                hook();
            }
            return Verdict::Pass;
        }
        if !s.connected || !s.local_buttons.is_empty() || !pushing {
            return Verdict::Pass;
        }
        let Some(along) = s.screens.at_edge(side, x, y) else {
            return Verdict::Pass;
        };
        // Cruza: el cursor queda aquí, quieto, y aparece en el otro.
        s.remote = true;
        s.exit_along = along;
        s.exit_side = side;
        let park = s.screens.inward(side, along, crate::sys::PARK_INSET).unwrap_or((x, y));
        let _ = self.tx.send(Msg::Enter { side: side.opposite(), along });
        drop(s);
        // Fuera del candado: mover el cursor puede volver a pasar por el gancho.
        crate::sys::hold_cursor(park.0, park.1);
        tracing::debug!(along, "el cursor pasa al otro equipo");
        Verdict::Swallow
    }

    pub fn on_button(&self, button: Button, down: bool) -> Verdict {
        let mut s = self.lock();
        if s.remote {
            // Apretado aquí antes de cruzar: se suelta aquí.
            if !down && s.local_buttons.remove(&button) {
                return Verdict::Pass;
            }
            let _ = self.tx.send(Msg::Button { button, down });
            return Verdict::Swallow;
        }
        if down {
            s.local_buttons.insert(button);
        } else {
            s.local_buttons.remove(&button);
            if button == Button::Left {
                s.drag_told = false;
            }
        }
        Verdict::Pass
    }

    pub fn on_wheel(&self, dx: f32, dy: f32) -> Verdict {
        if !self.is_remote() {
            return Verdict::Pass;
        }
        if dx != 0.0 || dy != 0.0 {
            let _ = self.tx.send(Msg::Wheel { dx, dy });
        }
        Verdict::Swallow
    }

    pub fn on_key(&self, hid: u16, down: bool) -> Verdict {
        let mut s = self.lock();
        if s.remote {
            if !down && s.local_keys.remove(&hid) {
                return Verdict::Pass;
            }
            // El atajo de emergencia: Ctrl+Alt+Esc trae el cursor de vuelta.
            let held = |a: u16, b: u16| s.remote_keys.contains(&a) || s.remote_keys.contains(&b);
            if down && hid == keys::ESCAPE && held(keys::LEFT_CTRL, keys::RIGHT_CTRL) && held(keys::LEFT_ALT, keys::RIGHT_ALT) {
                let _ = self.tx.send(Msg::Leave);
                let along = s.exit_along;
                let to = come_back(&mut s, along);
                // El Esc tampoco se suelta aquí.
                s.remote_keys.insert(hid);
                drop(s);
                release(to);
                return Verdict::Swallow;
            }
            if down {
                s.remote_keys.insert(hid);
            } else {
                s.remote_keys.remove(&hid);
            }
            let _ = self.tx.send(Msg::Key { hid, down });
            return Verdict::Swallow;
        }
        // Se apretó en el otro: aquí nunca bajó, así que tampoco sube.
        if !down && s.remote_keys.remove(&hid) {
            return Verdict::Swallow;
        }
        if down {
            s.local_keys.insert(hid);
        } else {
            s.local_keys.remove(&hid);
        }
        Verdict::Pass
    }
}

/// El cursor vuelve a este equipo: devuelve dónde dejarlo, unos píxeles hacia
/// dentro del borde para que no vuelva a cruzar solo.
fn come_back(s: &mut State, along: f64) -> Option<(f64, f64)> {
    s.remote = false;
    tracing::debug!(along, "el cursor vuelve");
    s.screens.inward(s.exit_side, along, 2.0)
}

/// Se llama sin el candado (ver [`Grab::on_move`]).
fn release(to: Option<(f64, f64)>) {
    if let Some((x, y)) = to {
        crate::sys::release_cursor(x, y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::Rect;
    use tokio::sync::mpsc;

    fn grab() -> (Grab, mpsc::UnboundedReceiver<Msg>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let g = Grab::new(Some(Side::Right), Screens(vec![Rect { x: 0., y: 0., w: 1920., h: 1080. }]), tx);
        g.set_connected(true);
        (g, rx)
    }

    fn drain(rx: &mut mpsc::UnboundedReceiver<Msg>) -> Vec<Msg> {
        std::iter::from_fn(|| rx.try_recv().ok()).collect()
    }

    #[test]
    fn cruza_por_el_borde_y_manda_lo_que_sigue() {
        let (g, mut rx) = grab();
        assert_eq!(g.on_move(1000., 540., 5., 0., true), Verdict::Pass);
        assert_eq!(g.on_move(1919., 540., 5., 0., true), Verdict::Swallow);
        assert_eq!(g.on_move(1919., 540., -3., 2., true), Verdict::Swallow);
        assert_eq!(g.on_key(0x04, true), Verdict::Swallow);
        let msgs = drain(&mut rx);
        assert!(matches!(msgs[0], Msg::Enter { side: Side::Left, along } if (along - 0.5).abs() < 0.01));
        assert_eq!(msgs[1], Msg::Move { dx: -3, dy: 2 });
        assert_eq!(msgs[2], Msg::Key { hid: 0x04, down: true });
        g.back(0.5);
        assert!(!g.is_remote());
    }

    #[test]
    fn sin_sesion_o_arrastrando_no_cruza() {
        let (g, _rx) = grab();
        g.set_connected(false);
        assert_eq!(g.on_move(1919., 540., 5., 0., true), Verdict::Pass);
        g.set_connected(true);
        g.on_button(Button::Left, true);
        assert_eq!(g.on_move(1919., 540., 5., 0., true), Verdict::Pass);
        g.on_button(Button::Left, false);
        // Sin empujar hacia fuera (pegado al borde, moviéndose hacia arriba), no cruza en modo estricto.
        assert_eq!(g.on_move(1919., 540., 0., -4., true), Verdict::Pass);
        assert_eq!(g.on_move(1919., 540., 0., -4., false), Verdict::Swallow);
    }

    #[test]
    fn ninguna_tecla_queda_pegada() {
        let (g, mut rx) = grab();
        // Mayús apretada aquí, cruza, y se suelta allá: el sistema de aquí recibe la subida.
        assert_eq!(g.on_key(0xE1, true), Verdict::Pass);
        g.on_move(1919., 540., 5., 0., true);
        assert_eq!(g.on_key(0xE1, false), Verdict::Pass);
        // ⌘ apretado allá, vuelve, se suelta aquí: no se pasa (aquí nunca bajó).
        assert_eq!(g.on_key(0xE3, true), Verdict::Swallow);
        g.back(0.5);
        assert_eq!(g.on_key(0xE3, false), Verdict::Swallow);
        assert_eq!(g.on_key(0xE3, true), Verdict::Pass);
        drain(&mut rx);
    }

    #[test]
    fn ctrl_alt_esc_trae_el_cursor() {
        let (g, mut rx) = grab();
        g.on_move(1919., 540., 5., 0., true);
        g.on_key(keys::LEFT_CTRL, true);
        g.on_key(keys::LEFT_ALT, true);
        assert_eq!(g.on_key(keys::ESCAPE, true), Verdict::Swallow);
        assert!(!g.is_remote());
        assert!(drain(&mut rx).contains(&Msg::Leave));
        // Las subidas de Ctrl, Alt y Esc tampoco llegan aquí.
        for k in [keys::ESCAPE, keys::LEFT_ALT, keys::LEFT_CTRL] {
            assert_eq!(g.on_key(k, false), Verdict::Swallow);
        }
    }

    #[test]
    fn un_arrastre_contra_el_borde_avisa_una_vez() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let (g, _rx) = grab();
        let told = std::sync::Arc::new(AtomicUsize::new(0));
        let counter = told.clone();
        g.on_drag_edge(move || {
            counter.fetch_add(1, Ordering::Relaxed);
        });
        g.on_button(Button::Left, true);
        assert_eq!(g.on_move(1919., 540., 5., 0., true), Verdict::Pass);
        assert_eq!(g.on_move(1919., 541., 5., 0., true), Verdict::Pass);
        assert!(!g.is_remote());
        assert_eq!(told.load(Ordering::Relaxed), 1);
        // Otro arrastre vuelve a avisar.
        g.on_button(Button::Left, false);
        g.on_button(Button::Left, true);
        g.on_move(1919., 540., 5., 0., true);
        assert_eq!(told.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn sin_lado_no_comparte() {
        let (g, _rx) = grab();
        g.set_side(None);
        assert_eq!(g.on_move(1919., 540., 5., 0., true), Verdict::Pass);
        g.set_side(Some(Side::Left));
        assert_eq!(g.on_move(1919., 540., 5., 0., true), Verdict::Pass);
        assert_eq!(g.on_move(0., 540., -5., 0., true), Verdict::Swallow);
    }

    #[test]
    fn si_se_cae_la_sesion_el_cursor_vuelve() {
        let (g, _rx) = grab();
        g.on_move(1919., 540., 5., 0., true);
        assert!(g.is_remote());
        g.set_connected(false);
        assert!(!g.is_remote());
    }
}
