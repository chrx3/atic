//! El ícono de Atic en la barra de menús (`NSStatusItem`), con el mismo menú
//! que la bandeja de Windows. Las órdenes llegan a la pill por un canal, como
//! los atajos.
//!
//! El ícono es una imagen «plantilla» (negro sobre transparente): macOS la
//! pinta clara u oscura según la barra, como los íconos del sistema.
//!
//! También vive aquí lo que en Windows hace la ventana oculta de la bandeja:
//! despertar a la pill que ya corre (`wake_running`, por un socket Unix) y los
//! avisos del sistema (`notify`, en `notify.rs`).

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, OnceLock};

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject, Bool, NSObject};
use objc2::{class, define_class, msg_send, sel, AllocAnyThread};
use objc2_foundation::{NSData, NSSize, NSString};

/// Lo que se pidió desde el ícono (ver `win32/tray_icon.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    OpenAtic,
    Settings,
    /// Lo que trae cada versión (`whats_new.rs`).
    WhatsNew,
    Consoles,
    Capture,
    Quit,
}

const SETTINGS_ID: isize = 1;
const CAPTURE_ID: isize = 2;
const MEETINGS_ID: isize = 3;
const CONSOLES_ID: isize = 4;
const WHATS_NEW_ID: isize = 5;
const QUIT_ID: isize = 9;
const ITEMS: &[(isize, Command)] = &[
    (SETTINGS_ID, Command::Settings),
    (CAPTURE_ID, Command::Capture),
    (MEETINGS_ID, Command::OpenAtic),
    (CONSOLES_ID, Command::Consoles),
    (WHATS_NEW_ID, Command::WhatsNew),
    (QUIT_ID, Command::Quit),
];

static SENDER: OnceLock<Mutex<Sender<Command>>> = OnceLock::new();

/// Manda una orden a la pill.
pub(super) fn send(command: Command) {
    if let Some(tx) = SENDER.get().and_then(|tx| tx.lock().ok()) {
        let _ = tx.send(command);
    }
}

define_class!(
    /// El destino de las entradas del menú.
    #[unsafe(super(NSObject))]
    #[name = "AticPillMenuTarget"]
    struct MenuTarget;

    impl MenuTarget {
        #[unsafe(method(pick:))]
        fn pick(&self, sender: &AnyObject) {
            // SAFETY: `sender` es el `NSMenuItem` que se eligió.
            let tag: isize = unsafe { msg_send![sender, tag] };
            if let Some((_, command)) = ITEMS.iter().find(|(id, _)| *id == tag) {
                // Desde el menú no se viene de un aviso: no se abre su conversación.
                take_click_topic();
                send(*command);
            }
        }
    }
);

impl MenuTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(());
        // SAFETY: `init` de `NSObject`.
        unsafe { msg_send![super(this), init] }
    }
}

/// Arranca el ícono en el hilo principal. Si no se puede, el canal
/// simplemente no recibe nada.
pub fn spawn() -> Receiver<Command> {
    let (tx, rx) = mpsc::channel();
    if SENDER.set(Mutex::new(tx)).is_ok() {
        super::main_thread::run(|| {
            if let Err(error) = install() {
                tracing::warn!(%error, "barra de menús: sin ícono");
            }
        });
        wake::listen();
        super::notify::init();
    }
    rx
}

/// El atajo de la captura, como se lee en un menú de Mac: «⇧⌘4».
fn capture_shortcut() -> Option<String> {
    let cfg = atic_core::AppDirs::new().ok().map(|dirs| atic_core::Config::load(&dirs.config_path()))?;
    let text = cfg.screenshot_shortcut.trim();
    (!text.is_empty()).then(|| super::keys::pretty(text))
}

fn label(id: isize) -> String {
    match id {
        SETTINGS_ID => crate::i18n::t("pill.tray.settings").to_string(),
        CAPTURE_ID => match capture_shortcut() {
            Some(keys) => format!("{}   {keys}", crate::i18n::t("tray.capture")),
            None => crate::i18n::t("tray.capture").to_string(),
        },
        MEETINGS_ID => crate::i18n::t("pill.tray.meetings").to_string(),
        CONSOLES_ID => crate::i18n::t("tray.consoles").to_string(),
        WHATS_NEW_ID => crate::i18n::t("pill.tray.whatsNew").to_string(),
        _ => crate::i18n::t("pill.tray.quit").to_string(),
    }
}

fn install() -> Result<(), String> {
    // SAFETY: todo en el hilo principal (`main_thread::run`); los objetos se
    // retienen mientras se usan y el ícono, el menú y el destino viven todo
    // el proceso (se sueltan con `forget`).
    unsafe {
        let bar: Option<Retained<AnyObject>> = msg_send![class!(NSStatusBar), systemStatusBar];
        let bar = bar.ok_or("sin barra de menús")?;
        // `NSVariableStatusItemLength`.
        let item: Option<Retained<AnyObject>> = msg_send![&bar, statusItemWithLength: -1.0f64];
        let item = item.ok_or("sin NSStatusItem")?;
        let button: Option<Retained<AnyObject>> = msg_send![&item, button];
        if let Some(button) = button {
            if let Some(image) = template_image() {
                let _: () = msg_send![&button, setImage: &*image];
            }
            let tip = NSString::from_str(&format!("Atic {}", env!("ATIC_VERSION")));
            let _: () = msg_send![&button, setToolTip: &*tip];
        }

        let target = MenuTarget::new();
        let menu: Retained<AnyObject> = msg_send![class!(NSMenu), new];
        let _: () = msg_send![&menu, setAutoenablesItems: Bool::NO];
        let version = menu_item(&format!("Atic {}", env!("ATIC_VERSION")), 0, None);
        let _: () = msg_send![&version, setEnabled: Bool::NO];
        let _: () = msg_send![&menu, addItem: &*version];
        add_separator(&menu);
        let settings = menu_item(&label(SETTINGS_ID), SETTINGS_ID, Some(&target));
        let _: () = msg_send![&menu, addItem: &*settings];
        add_separator(&menu);
        for id in [CAPTURE_ID, MEETINGS_ID, CONSOLES_ID] {
            let entry = menu_item(&label(id), id, Some(&target));
            let _: () = msg_send![&menu, addItem: &*entry];
        }
        add_separator(&menu);
        let whats_new = menu_item(&label(WHATS_NEW_ID), WHATS_NEW_ID, Some(&target));
        let _: () = msg_send![&menu, addItem: &*whats_new];
        let quit = menu_item(&label(QUIT_ID), QUIT_ID, Some(&target));
        let _: () = msg_send![&menu, addItem: &*quit];
        let _: () = msg_send![&item, setMenu: &*menu];

        std::mem::forget(target);
        std::mem::forget(menu);
        std::mem::forget(item);
    }
    Ok(())
}

/// SAFETY: en el hilo principal.
unsafe fn menu_item(title: &str, tag: isize, target: Option<&MenuTarget>) -> Retained<AnyObject> {
    let title = NSString::from_str(title);
    let empty = NSString::from_str("");
    let item: Allocated<AnyObject> = msg_send![class!(NSMenuItem), alloc];
    let action = target.map(|_| sel!(pick:));
    let item: Retained<AnyObject> = msg_send![item, initWithTitle: &*title, action: action, keyEquivalent: &*empty];
    let _: () = msg_send![&item, setTag: tag];
    if let Some(target) = target {
        let _: () = msg_send![&item, setTarget: target];
    }
    item
}

/// SAFETY: en el hilo principal.
unsafe fn add_separator(menu: &AnyObject) {
    let separator: Retained<AnyObject> = msg_send![class!(NSMenuItem), separatorItem];
    let _: () = msg_send![menu, addItem: &*separator];
}

/// La marca de Atic (el ático con su ventana) como plantilla de 18 × 18 pt.
fn template_image() -> Option<Retained<AnyObject>> {
    let png = mark_png(36)?;
    let data = NSData::with_bytes(&png);
    // SAFETY: `initWithData:` copia los bytes; la imagen es nuestra.
    unsafe {
        let image: Allocated<AnyObject> = msg_send![class!(NSImage), alloc];
        let image: Option<Retained<AnyObject>> = msg_send![image, initWithData: &*data];
        let image = image?;
        let _: () = msg_send![&image, setSize: NSSize::new(18.0, 18.0)];
        let _: () = msg_send![&image, setTemplate: Bool::YES];
        Some(image)
    }
}

/// Dibuja la marca de la pill (la «a» con ojos de `paint_mark_at`, en su
/// grilla de 24) en negro sobre transparente, con 4 × 4 muestras por píxel.
fn mark_png(side: u32) -> Option<Vec<u8>> {
    // Un poco más gruesa que en la pill (1,5) para que se lea a 18 pt.
    const STROKE: f32 = 1.7;
    const EYES: [(f32, f32); 2] = [(10.3, 11.6), (13.7, 11.6)];
    fn ink(x: f32, y: f32) -> bool {
        let half = STROKE / 2.0;
        let ring = (((x - 12.0).powi(2) + (y - 12.0).powi(2)).sqrt() - 5.5).abs() <= half;
        // El palo, con las puntas redondas.
        let stem_y = y.clamp(6.5, 17.5);
        let stem = ((x - 17.5).powi(2) + (y - stem_y).powi(2)).sqrt() <= half;
        let eye = EYES
            .iter()
            .any(|&(ex, ey)| ((x - ex) / 0.88).powi(2) + ((y - ey) / 1.32).powi(2) <= 1.0);
        ring || stem || eye
    }
    // La marca va de 5,6 a 18,4 en x y de 5,6 a 18,4 en y: se centra en un
    // cuadro de 16 para que se lea del mismo tamaño que los del sistema.
    let (left, top, span) = (4.0f32, 4.0f32, 16.0f32);
    let scale = span / side as f32;
    let mut img = image::RgbaImage::new(side, side);
    for (px, py, pixel) in img.enumerate_pixels_mut() {
        let mut hits = 0;
        for sy in 0..4 {
            for sx in 0..4 {
                let x = left + (px as f32 + (sx as f32 + 0.5) / 4.0) * scale;
                let y = top + (py as f32 + (sy as f32 + 0.5) / 4.0) * scale;
                if ink(x, y) {
                    hits += 1;
                }
            }
        }
        *pixel = image::Rgba([0, 0, 0, (hits * 255 / 16) as u8]);
    }
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// La conversación del último aviso clickeado (se consume una sola vez).
static CLICK_TOPIC: Mutex<Option<String>> = Mutex::new(None);

pub(super) fn set_click_topic(topic: Option<String>) {
    if let Ok(mut slot) = CLICK_TOPIC.lock() {
        *slot = topic;
    }
}

pub fn take_click_topic() -> Option<String> {
    CLICK_TOPIC.lock().ok().and_then(|mut topic| topic.take())
}

/// Un aviso del sistema (Centro de notificaciones). Un clic en él abre Atic
/// Code y, si trae `topic`, esa conversación.
pub fn notify(title: &str, body: &str, error: bool, topic: Option<String>) -> bool {
    super::notify::show(title, body, error, topic)
}

/// Desde una segunda pill: le pide a la que corre que abra Atic. `false` si no
/// contestó nadie.
pub fn wake_running() -> bool {
    wake::send()
}

/// El socket por el que una segunda pill despierta a la primera, como el
/// `WAKE` de la ventana oculta en Windows.
mod wake {
    use std::io::{Read, Write};
    use std::os::unix::net::{UnixListener, UnixStream};

    fn path() -> Option<std::path::PathBuf> {
        crate::paths::file(if crate::dev::on() { "pill-dev.sock" } else { "pill.sock" })
    }

    pub fn listen() {
        let Some(path) = path() else {
            return;
        };
        // Quien llega aquí tiene el candado de la instancia única: un socket
        // que quedó es de una pill que ya no está.
        let _ = std::fs::remove_file(&path);
        let listener = match UnixListener::bind(&path) {
            Ok(listener) => listener,
            Err(error) => {
                tracing::warn!(%error, "no se pudo abrir el socket para despertar la pill");
                return;
            }
        };
        let _ = std::thread::Builder::new().name("despertar".into()).spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                let mut word = [0u8; 4];
                if stream.read_exact(&mut word).is_ok() && &word == b"wake" {
                    super::send(super::Command::OpenAtic);
                    let _ = stream.write_all(b"ok");
                }
            }
        });
    }

    pub fn send() -> bool {
        let Some(path) = path() else {
            return false;
        };
        let Ok(mut stream) = UnixStream::connect(path) else {
            return false;
        };
        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
        let mut answer = [0u8; 2];
        stream.write_all(b"wake").is_ok() && stream.read_exact(&mut answer).is_ok()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn la_marca_tiene_tinta_y_hueco() {
        let png = super::mark_png(36).expect("png");
        let img = image::load_from_memory(&png).expect("decodifica").to_rgba8();
        let alpha = |x, y| img.get_pixel(x, y).0[3];
        // El anillo de la «a», el palo y un ojo tienen tinta; el centro del
        // anillo bajo los ojos está vacío.
        assert!(alpha(5, 18) > 200);
        assert!(alpha(30, 18) > 200);
        assert!(alpha(14, 17) > 200);
        assert!(alpha(18, 23) < 40);
        assert_eq!(alpha(0, 0), 0);
    }
}
