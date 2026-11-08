//! El equipo que recibe: inyecta lo que llega y devuelve el cursor cuando
//! cruza de vuelta el borde por el que entró.

use std::collections::HashSet;

use crate::keys;
use crate::layout::{Screens, Side};
use crate::proto::{Button, Msg, Os};

pub struct Target {
    screens: Screens,
    /// Por dónde entró el cursor; `None` mientras el cursor está en el otro.
    entered: Option<Side>,
    keys: HashSet<u16>,
    buttons: HashSet<Button>,
    /// Ctrl por ⌘ (o Windows) y al revés, entre sistemas distintos.
    swap: bool,
}

impl Target {
    pub fn new(peer: Os) -> Self {
        let mac = |os| os == Os::Macos;
        Self { screens: Screens::default(), entered: None, keys: HashSet::new(), buttons: HashSet::new(), swap: mac(peer) != mac(Os::current()) }
    }

    /// Atiende un mensaje; lo que devuelve va de vuelta al otro.
    pub fn handle(&mut self, msg: Msg) -> Option<Msg> {
        match msg {
            Msg::Enter { side, along } => {
                self.screens = crate::sys::screens();
                if let Some((x, y)) = self.screens.entry(side, along) {
                    crate::sys::move_to(x, y, 0.0, 0.0);
                }
                self.entered = Some(side);
            }
            Msg::Move { dx, dy } => {
                let side = self.entered?;
                let (dx, dy) = (dx as f64, dy as f64);
                let (x, y) = crate::sys::cursor();
                if side.outward(dx, dy) > 0.0 {
                    if let Some(along) = self.screens.at_edge(side, x, y) {
                        self.release_all();
                        self.entered = None;
                        return Some(Msg::Back { along });
                    }
                }
                let (nx, ny) = self.screens.clamp(x + dx, y + dy);
                crate::sys::move_to(nx, ny, dx, dy);
            }
            Msg::Button { button, down } => {
                if down {
                    self.buttons.insert(button);
                } else if !self.buttons.remove(&button) {
                    return None;
                }
                crate::sys::button(button, down);
            }
            Msg::Wheel { dx, dy } => crate::sys::wheel(dx, dy),
            Msg::Key { hid, down } => {
                let hid = if self.swap { keys::swap_ctrl_meta(hid) } else { hid };
                let repeat = down && !self.keys.insert(hid);
                if !down && !self.keys.remove(&hid) {
                    return None;
                }
                crate::sys::key(hid, down, repeat);
            }
            Msg::Leave => {
                self.release_all();
                self.entered = None;
            }
            Msg::Hello { .. } | Msg::Back { .. } => {}
        }
        None
    }

    /// Suelta todo lo que quedó apretado (al devolver el cursor o al cortarse la sesión).
    pub fn release_all(&mut self) {
        for hid in std::mem::take(&mut self.keys) {
            crate::sys::key(hid, false, false);
        }
        for button in std::mem::take(&mut self.buttons) {
            crate::sys::button(button, false);
        }
    }
}
