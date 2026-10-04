//! Las consolas no son de la ventana: son de la aplicación.
//!
//! Cerrar la ventana del espacio no termina a los agentes. Al soltarse la vista
//! (`on_release`), sus consolas pasan a un global de GPUI: los PTY siguen
//! corriendo, con su historial, y la ventana que se abra después las recoge
//! con todo (dónde estaban, cuál tenía el foco, qué había quedado por revisar).
//! Mientras la aplicación siga abierta —el notch es lo que queda— los agentes
//! siguen trabajando. Si la aplicación se cierra, las consolas se van con ella.
//!
//! Para que la vista llegue a soltarse, nada que viva dentro de la ventana
//! nativa puede sostenerla con una referencia fuerte (ver `input::Receiver`):
//! esa ventana se libera con retraso al cerrarla.
//!
//! `SPACE_RECYCLE=1` lo prueba solo: a los 10 s cierra la ventana y a los 2 s
//! abre otra. `SPACE_RECYCLE=x` espera a que la cierres tú (la X de la barra) y
//! abre otra 2 s después. Una ventana oculta mantiene viva la aplicación, que es
//! lo que hace en la práctica el notch.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use gpui::{
    div, point, px, size, App, AppContext, Bounds, Context, Global, IntoElement, Render, Window,
    WindowBounds, WindowHandle, WindowKind, WindowOptions,
};

use super::{mando, Camera, Card, SpaceView};

/// Lo que una ventana deja al cerrarse.
struct Left {
    cards: Vec<Card>,
    next_id: u64,
    camera: Camera,
    focused: Option<u64>,
    view: mando::View,
    mando: mando::State,
}

/// Lo que dejaron las ventanas cerradas, en orden. Cada ventana nueva recoge la
/// primera tanda. Con una sola ventana a la vez (el notch no abre otra mientras
/// haya una) nunca hay más de una; si la hubiera, ninguna se pierde: guardar
/// encima de otra tanda mataría a esos agentes.
#[derive(Default)]
struct Stash(Vec<Left>);

impl Global for Stash {}

/// La ventana del espacio mientras está abierta: así el notch le habla a sus
/// agentes sin ser su dueño.
struct OpenWindow(WindowHandle<SpaceView>);

impl Global for OpenWindow {}

pub(super) fn register(handle: WindowHandle<SpaceView>, cx: &mut App) {
    cx.set_global(OpenWindow(handle));
}

/// Un agente vivo: lo que el mensaje rápido del notch puede alcanzar.
fn is_agent(card: &Card) -> bool {
    card.agent.is_some() && !card.console.exited()
}

/// Cuántos agentes hay vivos en el espacio, con la ventana abierta o cerrada.
pub fn agent_consoles(cx: &App) -> usize {
    if let Some(open) = cx.try_global::<OpenWindow>() {
        if let Ok(count) = open.0.read_with(cx, |view, _| view.cards.iter().filter(|c| is_agent(c)).count()) {
            return count;
        }
    }
    cx.try_global::<Stash>()
        .map_or(0, |stash| stash.0.iter().flat_map(|left| &left.cards).filter(|c| is_agent(c)).count())
}

/// Escribe `text` y Enter en cada agente vivo del espacio. Devuelve a cuántos.
pub fn send_to_agents(cx: &mut App, text: &str) -> usize {
    let bytes = format!("{text}\r").into_bytes();
    let write = |card: &Card| {
        card.console.scroll(i32::MIN / 2);
        card.console.write(bytes.clone());
    };
    if let Some(handle) = cx.try_global::<OpenWindow>().map(|open| open.0) {
        let sent = handle.update(cx, |view, _, _| {
            view.cards.iter().filter(|card| is_agent(card)).map(|card| write(card)).count()
        });
        if let Ok(count) = sent {
            return count;
        }
    }
    let Some(stash) = cx.try_global::<Stash>() else {
        return 0;
    };
    stash.0.iter().flat_map(|left| &left.cards).filter(|card| is_agent(card)).map(|card| write(card)).count()
}

/// Se llama al soltarse la vista: guarda las consolas para la próxima ventana.
pub(super) fn stash(view: &mut SpaceView, cx: &mut App) {
    if cx.has_global::<OpenWindow>() {
        cx.remove_global::<OpenWindow>();
    }
    if view.cards.is_empty() {
        return;
    }
    eprintln!("espacio: la ventana se cierra; {} consolas siguen corriendo", view.cards.len());
    if !cx.has_global::<Stash>() {
        cx.set_global(Stash::default());
    }
    cx.global_mut::<Stash>().0.push(Left {
        cards: std::mem::take(&mut view.cards),
        next_id: view.next_id,
        camera: view.camera,
        focused: view.focused,
        view: view.view,
        mando: std::mem::take(&mut view.mando),
    });
}

/// Se llama al crearse la vista: recoge las consolas que dejó la anterior.
/// Dice si había algo que recoger.
pub(super) fn restore(view: &mut SpaceView, cx: &mut Context<SpaceView>) -> bool {
    if !cx.has_global::<Stash>() || cx.global::<Stash>().0.is_empty() {
        eprintln!("espacio: ventana nueva, sin consolas guardadas");
        return false;
    }
    let left = cx.global_mut::<Stash>().0.remove(0);
    eprintln!("espacio: ventana nueva; recoge {} consolas que seguían corriendo", left.cards.len());
    // Los avisos de salida de cada consola iban a la ventana que se cerró.
    for card in &left.cards {
        card.console.rewire(view.wake.clone());
    }
    view.focused = left.focused.filter(|id| left.cards.iter().any(|c| c.id == *id));
    view.cards = left.cards;
    view.next_id = left.next_id;
    view.camera = left.camera;
    view.view = left.view;
    view.mando = left.mando;
    true
}

/// Una ventana de 1 px que no se ve: mantiene viva la aplicación mientras la
/// del espacio está cerrada, como el notch.
struct KeepAlive;

impl Render for KeepAlive {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// Una sola vez por ejecución: la ventana que abre la prueba no la repite.
static RECYCLED: AtomicBool = AtomicBool::new(false);

/// `SPACE_RECYCLE=1`: cierra la ventana a los 10 s y abre otra a los 2 s más.
/// `SPACE_RECYCLE=x`: la cierras tú; abre otra 2 s después.
pub(super) fn recycle_test(handle: WindowHandle<SpaceView>, cx: &mut App) {
    let Some(mode) = std::env::var("SPACE_RECYCLE").ok() else {
        return;
    };
    if RECYCLED.swap(true, Ordering::SeqCst) {
        return;
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(px(0.), px(0.)),
            size(px(1.), px(1.)),
        ))),
        show: false,
        focus: false,
        kind: WindowKind::PopUp,
        ..Default::default()
    };
    let _ = cx.open_window(options, |_, cx| cx.new(|_| KeepAlive));
    if mode == "x" {
        let reopened = AtomicBool::new(false);
        cx.on_window_closed(move |cx| {
            if reopened.swap(true, Ordering::SeqCst) {
                return;
            }
            cx.spawn(async move |cx| {
                cx.background_executor().timer(Duration::from_secs(2)).await;
                eprintln!("espacio: abriendo otra ventana (SPACE_RECYCLE=x)");
                let _ = cx.update(|cx| {
                    let _ = super::open_window(None, cx);
                });
            })
            .detach();
        })
        .detach();
        return;
    }
    cx.spawn(async move |cx| {
        cx.background_executor().timer(Duration::from_secs(10)).await;
        eprintln!("espacio: cerrando la ventana (SPACE_RECYCLE)");
        let _ = cx.update(|cx| {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        });
        cx.background_executor().timer(Duration::from_secs(2)).await;
        eprintln!("espacio: abriendo otra ventana (SPACE_RECYCLE)");
        let _ = cx.update(|cx| {
            let _ = super::open_window(None, cx);
        });
    })
    .detach();
}
