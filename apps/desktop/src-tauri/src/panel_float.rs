//! Floats de panel anclados a la pill (clipboard, snippets, agentes, launcher).
//!
//! Misma geometría: `pill_rect` + `bubble_rect`, evento CSS al overlay.
//! Cada panel tiene su propio `AtomicBool` OPEN y su `BubbleShape`.

use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::floating::BubbleShape;

/// Forma del panel (lógico), alineada a `PILL.panelW` × (`bar`+`panelH`) ≈ 312×372.
pub const PANEL_SHAPE: BubbleShape = BubbleShape {
    w: 312,
    h: 372,
    gap: 10,
    corner: 18,
};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BubbleOpen {
    side: &'static str,
    offset: f64,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// Abre o cierra un float de panel. Devuelve si quedó abierto.
pub fn toggle(
    app: &AppHandle,
    open: &AtomicBool,
    shape: BubbleShape,
    anchor_event: &str,
    dismiss_event: &str,
) -> bool {
    if open.load(Ordering::Relaxed) {
        hide(app, open, dismiss_event);
        return false;
    }
    show(app, open, shape, anchor_event)
}

pub fn show(
    app: &AppHandle,
    open: &AtomicBool,
    shape: BubbleShape,
    anchor_event: &str,
) -> bool {
    if open.load(Ordering::Relaxed) {
        return true;
    }
    let Some((target, anchor)) = crate::overlay::pill_rect()
        .and_then(|origen| crate::floating::bubble_rect(app, origen, shape))
    else {
        tracing::warn!(target: "overlay", "panel float: sin pill_rect");
        return false;
    };
    let Some(area) = crate::overlay::to_local(app, target) else {
        return false;
    };

    open.store(true, Ordering::Relaxed);
    crate::overlay::raise(app);
    let _ = app.emit(
        anchor_event,
        BubbleOpen {
            side: anchor.side,
            offset: f64::from(anchor.offset) / crate::overlay::scale(app),
            x: area.x,
            y: area.y,
            w: area.w,
            h: area.h,
        },
    );
    true
}

pub fn hide(app: &AppHandle, open: &AtomicBool, dismiss_event: &str) {
    if !open.swap(false, Ordering::Relaxed) {
        return;
    }
    let _ = app.emit(dismiss_event, ());
}
