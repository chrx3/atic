//! Las ventanas en pantalla, de adelante hacia atrás, según Quartz
//! (`CGWindowListCopyWindowInfo`). Lo usan pegar (la app bajo el cursor), el
//! flip (la ventana del frente) y el detector de reuniones (los títulos).
//!
//! macOS solo da los títulos de otras apps con el permiso de Grabación de
//! pantalla; sin él `title` sale vacío.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};

/// Una ventana, con el marco en puntos y el origen arriba a la izquierda de
/// la pantalla principal.
#[derive(Clone, Debug)]
pub struct WindowInfo {
    pub id: u32,
    pub pid: i32,
    /// El nombre de la app dueña («zoom.us», «Safari»…).
    pub owner: String,
    pub title: String,
    /// 0 = ventana normal; la barra de menús, el Dock y los overlays van más
    /// arriba.
    pub layer: i32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl WindowInfo {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

type Dict = NSDictionary<NSString, AnyObject>;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> *mut NSArray<Dict>;
}

const ON_SCREEN_ONLY: u32 = 1 << 0;
const INCLUDING_WINDOW: u32 = 1 << 3;
const EXCLUDE_DESKTOP: u32 = 1 << 4;

/// Las ventanas visibles, de adelante hacia atrás.
pub fn on_screen() -> Vec<WindowInfo> {
    list(ON_SCREEN_ONLY | EXCLUDE_DESKTOP, 0)
}

/// Una ventana por su id, aunque esté escondida.
pub fn by_id(id: u32) -> Option<WindowInfo> {
    list(INCLUDING_WINDOW, id).into_iter().find(|window| window.id == id)
}

fn list(option: u32, relative_to: u32) -> Vec<WindowInfo> {
    // SAFETY: regla «Copy»: el arreglo es nuestro y se suelta al salir.
    let Some(array) = (unsafe { Retained::from_raw(CGWindowListCopyWindowInfo(option, relative_to)) }) else {
        return Vec::new();
    };
    array.iter().filter_map(|dict| parse(&dict)).collect()
}

fn value<'a>(dict: &'a Dict, key: &str) -> Option<Retained<AnyObject>> {
    dict.objectForKey(&NSString::from_str(key))
}

fn number(dict: &Dict, key: &str) -> Option<f64> {
    Some(value(dict, key)?.downcast_ref::<NSNumber>()?.as_f64())
}

fn text(dict: &Dict, key: &str) -> String {
    value(dict, key)
        .and_then(|value| value.downcast_ref::<NSString>().map(|text| text.to_string()))
        .unwrap_or_default()
}

fn parse(dict: &Dict) -> Option<WindowInfo> {
    let bounds = value(dict, "kCGWindowBounds")?;
    let bounds = bounds.downcast_ref::<NSDictionary>()?;
    // SAFETY: las llaves de `kCGWindowBounds` son cadenas.
    let bounds: &Dict = unsafe { bounds.cast_unchecked() };
    Some(WindowInfo {
        id: number(dict, "kCGWindowNumber")? as u32,
        pid: number(dict, "kCGWindowOwnerPID")? as i32,
        owner: text(dict, "kCGWindowOwnerName"),
        title: text(dict, "kCGWindowName"),
        layer: number(dict, "kCGWindowLayer").unwrap_or(0.0) as i32,
        x: number(bounds, "X")?,
        y: number(bounds, "Y")?,
        width: number(bounds, "Width")?,
        height: number(bounds, "Height")?,
    })
}
