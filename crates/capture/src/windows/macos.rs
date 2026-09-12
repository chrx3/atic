//! Candidatas vía `CGWindowListCopyWindowInfo` (en pantalla, topmost-first).

use core::ffi::c_void;

use core_foundation::base::{CFType, TCFType};
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_graphics::geometry::CGRect;
use core_graphics::window::{
    self, kCGNullWindowID, kCGWindowBounds, kCGWindowLayer, kCGWindowListExcludeDesktopElements,
    kCGWindowListOptionOnScreenOnly, kCGWindowName, kCGWindowNumber, kCGWindowOwnerName,
    kCGWindowOwnerPID, kCGWindowSharingState,
};

use super::{monitor_for, WindowCandidate, MIN_WINDOW_SIDE};
use crate::geometry::Rect;
use crate::monitors::MonitorInfo;

/// Capas por encima de las ventanas normales (Dock 20, menú 24, status 25).
/// Incluye flotantes (`NSFloatingWindowLevel` = 3) y paneles modales (8).
const MAX_WINDOW_LAYER: i64 = 8;

pub fn enumerate_candidates(exclude_pid: u32, monitors: &[MonitorInfo]) -> Vec<WindowCandidate> {
    let Some(info) = window::copy_window_info(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        kCGNullWindowID,
    ) else {
        return Vec::new();
    };

    let raw = info.len();
    let mut out = Vec::new();
    for item in info.iter() {
        let dict = unsafe { CFDictionary::wrap_under_get_rule(*item as _) };
        let Some(candidate) = candidate_from_dict(&dict, exclude_pid, monitors) else {
            continue;
        };
        out.push(candidate);
    }
    if out.is_empty() {
        tracing::warn!(
            raw,
            "ninguna ventana candidata (lista vacía o filtrada; en macOS 15+ hace falta grabación de pantalla)"
        );
    }
    for (z_index, candidate) in out.iter_mut().enumerate() {
        candidate.z_index = z_index;
    }
    out
}

pub fn foreground_window() -> isize {
    let monitors = crate::monitors::enumerate();
    enumerate_candidates(0, &monitors)
        .first()
        .map(|c| c.hwnd)
        .unwrap_or(0)
}

pub fn window_bounds(hwnd: isize) -> Option<Rect> {
    let monitors = crate::monitors::enumerate();
    enumerate_candidates(0, &monitors)
        .into_iter()
        .find(|c| c.hwnd == hwnd)
        .map(|c| c.visual_bounds)
}

pub fn window_outer_bounds(hwnd: isize) -> Option<Rect> {
    window_bounds(hwnd)
}

fn candidate_from_dict(
    dict: &CFDictionary,
    exclude_pid: u32,
    monitors: &[MonitorInfo],
) -> Option<WindowCandidate> {
    let layer = dict_i64(dict, unsafe { kCGWindowLayer }).unwrap_or(0);
    if !(0..=MAX_WINDOW_LAYER).contains(&layer) {
        return None;
    }
    let sharing = dict_i64(dict, unsafe { kCGWindowSharingState }).unwrap_or(1);
    if sharing == 0 {
        return None;
    }
    let process_id = dict_i64(dict, unsafe { kCGWindowOwnerPID }).unwrap_or(0) as u32;
    if process_id == 0 || process_id == exclude_pid {
        return None;
    }
    let window_id = dict_i64(dict, unsafe { kCGWindowNumber })?;
    let bounds_pt = dict_rect(dict)?;
    if bounds_pt.size.width < 1.0 || bounds_pt.size.height < 1.0 {
        return None;
    }
    // El espacio global de macOS ya es en puntos: el rect de CGWindowBounds es
    // directamente comparable con monitores y cursor.
    let visual_bounds = rect_from_points(bounds_pt);
    if visual_bounds.width < MIN_WINDOW_SIDE || visual_bounds.height < MIN_WINDOW_SIDE {
        return None;
    }
    let name = dict_string(dict, unsafe { kCGWindowName }).unwrap_or_default();
    let owner = dict_string(dict, unsafe { kCGWindowOwnerName }).unwrap_or_default();
    if owner.eq_ignore_ascii_case("Window Server")
        || owner.eq_ignore_ascii_case("Dock")
        || owner.eq_ignore_ascii_case("Control Center")
        || owner.eq_ignore_ascii_case("Notification Center")
    {
        return None;
    }
    let title = if !name.is_empty() {
        name
    } else if !owner.is_empty() {
        owner
    } else {
        String::new()
    };

    Some(WindowCandidate {
        hwnd: window_id as isize,
        title,
        visual_bounds,
        process_id,
        z_index: 0,
        monitor_id: monitor_for(&visual_bounds, monitors),
    })
}

fn rect_from_points(bounds: CGRect) -> Rect {
    Rect::new(
        bounds.origin.x.round() as i32,
        bounds.origin.y.round() as i32,
        bounds.size.width.round().max(0.0) as u32,
        bounds.size.height.round().max(0.0) as u32,
    )
}

fn dict_i64(dict: &CFDictionary, key: core_foundation::string::CFStringRef) -> Option<i64> {
    let value = dict.find(key as *const c_void)?;
    let num = unsafe { CFNumber::wrap_under_get_rule(*value as _) };
    num.to_i64()
}

fn dict_string(dict: &CFDictionary, key: core_foundation::string::CFStringRef) -> Option<String> {
    let value = dict.find(key as *const c_void)?;
    let s = unsafe { CFString::wrap_under_get_rule(*value as _) };
    Some(s.to_string())
}

fn dict_rect(dict: &CFDictionary) -> Option<CGRect> {
    let value = dict.find(unsafe { kCGWindowBounds } as *const c_void)?;
    let nested = unsafe { CFDictionary::<CFType, CFType>::wrap_under_get_rule(*value as _) };
    CGRect::from_dict_representation(&nested.to_untyped())
}
