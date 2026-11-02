//! La imagen de la vista previa según el sistema: en macOS el cuadro del reproductor y la
//! salida del `Composer` son búferes de IOSurface que `gpui::surface` dibuja sin pasar por la
//! CPU; en Windows son texturas de la GPU y se pasan a una imagen de GPUI (`RenderImage`).
//! El resto del editor (`scene.rs`, `mod.rs`) solo usa estas funciones.

use atic_capture::stream::{Composer, Decor, HardwareFrame};

/// El cuadro tal como lo entrega el reproductor.
pub(super) type Raw = HardwareFrame;

#[cfg(target_os = "macos")]
mod sys {
    use super::*;

    /// Qué identifica un cuadro del reproductor para saber si una salida guardada sigue valiendo.
    pub(in super::super) type Key = gpui::SurfaceSource;
    /// Lo que dibuja el escenario.
    pub(in super::super) type Shown = gpui::SurfaceSource;

    pub(in super::super) fn key_of(frame: &Raw) -> Key {
        gpui::SurfaceSource::from(frame.0.clone())
    }

    pub(in super::super) fn render(composer: &mut Composer, frame: &Raw, decor: &Decor) -> Result<Shown, String> {
        composer.render(&frame.0, decor).map(gpui::SurfaceSource::from)
    }

    pub(in super::super) fn shown_el(shown: Shown) -> gpui::Surface {
        gpui::surface(shown)
    }

    pub(in super::super) fn raw_el(frame: &Raw) -> Option<gpui::Surface> {
        Some(gpui::surface(frame.0.clone()))
    }
}

#[cfg(not(target_os = "macos"))]
mod sys {
    use std::sync::Arc;

    use super::*;

    pub(in super::super) type Key = HardwareFrame;
    pub(in super::super) type Shown = Arc<gpui::RenderImage>;

    pub(in super::super) fn key_of(frame: &Raw) -> Key {
        frame.clone()
    }

    /// Compone en la GPU y trae el resultado a una imagen de GPUI.
    pub(in super::super) fn render(composer: &mut Composer, frame: &Raw, decor: &Decor) -> Result<Shown, String> {
        let composed = composer.render(frame, decor)?;
        crate::capture::image_of(&composed.bgra()?).ok_or_else(|| "sin imagen".to_string())
    }

    pub(in super::super) fn shown_el(shown: Shown) -> gpui::Img {
        gpui::img(shown)
    }

    /// El cuadro del reproductor tal cual (lo exportado, sin pasar por el compositor).
    pub(in super::super) fn raw_el(frame: &Raw) -> Option<gpui::Img> {
        crate::capture::image_of(&frame.bgra().ok()?).map(gpui::img)
    }
}

pub(super) use sys::{key_of, raw_el, render, shown_el, Key, Shown};
