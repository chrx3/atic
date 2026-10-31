//! Texturas que ya no se dibujan. GPUI guarda cada imagen pintada en el atlas
//! de la ventana hasta que se le pide soltarla (`drop_image`): sin esto, cada
//! captura, cada trazo de la Pizarra y cada carátula quedaban en memoria (una
//! pantalla Retina son ~24 MB). Quien deja de usar una la pasa aquí, sin
//! necesitar `App` (también desde un `Drop`), y el tick de la pill las suelta.

use std::sync::{Arc, Mutex};

use gpui::{App, RenderImage};

static PENDING: Mutex<Vec<Arc<RenderImage>>> = Mutex::new(Vec::new());

/// `image` ya no se va a dibujar: su textura se suelta en el próximo tick.
pub fn image(image: Arc<RenderImage>) {
    if let Ok(mut pending) = PENDING.lock() {
        pending.push(image);
    }
}

pub fn maybe(image: Option<Arc<RenderImage>>) {
    if let Some(image) = image {
        self::image(image);
    }
}

/// Suelta lo pendiente de todas las ventanas. Va diferido: durante la
/// actualización de una ventana, esa ventana no está en la lista de `App`.
pub fn drain(cx: &mut App) {
    let images = match PENDING.lock() {
        Ok(mut pending) if !pending.is_empty() => std::mem::take(&mut *pending),
        _ => return,
    };
    cx.defer(move |cx| {
        for image in images {
            cx.drop_image(image, None);
        }
    });
}

/// Una imagen de archivo (`img` de una ruta) con su propia caché: GPUI la
/// suelta cuando deja de dibujarse. Sin esto, la caché global de GPUI guarda
/// cada miniatura del historial, de los vistazos y de Flip (decodificada y en
/// la GPU) mientras viva la pill, aunque ya no se muestre. `id` la distingue
/// de las demás del mismo panel; la caja ocupa todo su padre.
pub fn scoped(id: impl Into<gpui::ElementId>, image: impl gpui::IntoElement) -> gpui::ImageCacheElement {
    use gpui::{ParentElement, Styled};
    gpui::image_cache(gpui::retain_all(id)).size_full().child(image)
}
