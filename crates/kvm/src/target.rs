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
            // Botones, rueda y teclas solo con el cursor aquí, igual que el
            // movimiento. Al salir se suelta todo (`release_all`), así que una
            // subida que llega después no deja nada apretado.
            Msg::Button { button, down } => {
                self.entered?;
                if down {
                    self.buttons.insert(button);
                } else if !self.buttons.remove(&button) {
                    return None;
                }
                crate::sys::button(button, down);
            }
            Msg::Wheel { dx, dy } => {
                self.entered?;
                crate::sys::wheel(dx, dy);
            }
            Msg::Key { hid, down } => {
                self.entered?;
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
            Msg::Hello { .. } | Msg::Back { .. } | Msg::ClipText { .. } | Msg::ClipImage { .. } => {}
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

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> Target {
        // El mismo sistema en los dos lados: las teclas no se cambian.
        Target::new(Os::current())
    }

    #[test]
    fn sin_el_cursor_aqui_no_se_inyectan_teclas_ni_botones() {
        let mut t = target();
        t.handle(Msg::Key { hid: 0x04, down: true });
        t.handle(Msg::Button { button: Button::Left, down: true });
        assert!(t.keys.is_empty() && t.buttons.is_empty());

        t.handle(Msg::Enter { side: Side::Left, along: 0.5 });
        t.handle(Msg::Key { hid: 0x04, down: true });
        t.handle(Msg::Button { button: Button::Left, down: true });
        assert!(t.keys.contains(&0x04) && t.buttons.contains(&Button::Left));
    }

    /// Lo que quedó apretado se suelta al irse el cursor; las subidas que
    /// llegan después se ignoran, y lo que se apriete ya no entra.
    #[test]
    fn al_irse_el_cursor_se_suelta_todo_y_no_entra_nada_mas() {
        let mut t = target();
        t.handle(Msg::Enter { side: Side::Right, along: 0.2 });
        t.handle(Msg::Key { hid: 0x04, down: true });
        t.handle(Msg::Button { button: Button::Right, down: true });
        t.handle(Msg::Leave);
        assert!(t.entered.is_none());
        assert!(t.keys.is_empty() && t.buttons.is_empty());

        t.handle(Msg::Key { hid: 0x04, down: false });
        t.handle(Msg::Button { button: Button::Right, down: false });
        t.handle(Msg::Key { hid: 0x05, down: true });
        t.handle(Msg::Button { button: Button::Middle, down: true });
        assert!(t.keys.is_empty() && t.buttons.is_empty());

        // Al volver a entrar, todo como siempre.
        t.handle(Msg::Enter { side: Side::Right, along: 0.2 });
        t.handle(Msg::Key { hid: 0x05, down: true });
        t.handle(Msg::Key { hid: 0x05, down: false });
        assert!(t.keys.is_empty());
    }
}
