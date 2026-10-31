//! Avisos del sistema en macOS con `UNUserNotificationCenter`: banner en el
//! Centro de notificaciones y, al hacer clic, Atic Code con la conversación
//! que lo originó (como el globo de la bandeja en Windows).
//!
//! `UNUserNotificationCenter` solo existe para una app empaquetada (`.app`
//! con identificador). Corriendo el binario suelto (`cargo run`) se cae a
//! `osascript`, que muestra el aviso pero no avisa del clic.

use std::sync::atomic::{AtomicBool, Ordering};

use block2::{Block, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObject};
use objc2::{class, define_class, msg_send, AllocAnyThread};
use objc2_foundation::{NSDictionary, NSString};

#[link(name = "UserNotifications", kind = "framework")]
extern "C" {}

/// Hay centro de avisos (la pill corre como `.app`) y ya tiene delegado.
static READY: AtomicBool = AtomicBool::new(false);

define_class!(
    /// Recibe el clic en un aviso y deja que se muestre aunque la pill esté
    /// al frente.
    #[unsafe(super(NSObject))]
    #[name = "AticPillNotificationDelegate"]
    struct Delegate;

    impl Delegate {
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(&self, _center: &AnyObject, response: &AnyObject, done: &Block<dyn Fn()>) {
            // SAFETY: la respuesta trae su aviso, su pedido y su contenido.
            let topic = unsafe {
                let notification: Retained<AnyObject> = msg_send![response, notification];
                let request: Retained<AnyObject> = msg_send![&notification, request];
                let content: Retained<AnyObject> = msg_send![&request, content];
                let info: Option<Retained<NSDictionary<NSString, AnyObject>>> = msg_send![&content, userInfo];
                info.and_then(|info| {
                    let value = info.objectForKey(&NSString::from_str("topic"))?;
                    let value = value.downcast::<NSString>().ok()?;
                    Some(value.to_string())
                })
            };
            super::tray_icon::set_click_topic(topic.filter(|t| !t.is_empty()));
            super::tray_icon::send(super::tray_icon::Command::Consoles);
            done.call(());
        }

        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(&self, _center: &AnyObject, _notification: &AnyObject, done: &Block<dyn Fn(usize)>) {
            // Banner, lista y sonido (`UNNotificationPresentationOptions`).
            done.call(((1 << 4) | (1 << 3) | (1 << 1),));
        }
    }
);

impl Delegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(());
        // SAFETY: `init` de `NSObject`.
        unsafe { msg_send![super(this), init] }
    }
}

/// La pill corre empaquetada (`Atic.app/Contents/MacOS/…`).
pub fn bundled() -> bool {
    // SAFETY: consultas a `NSBundle.mainBundle`.
    unsafe {
        let bundle: Option<Retained<AnyObject>> = msg_send![class!(NSBundle), mainBundle];
        let Some(bundle) = bundle else {
            return false;
        };
        let id: Option<Retained<NSString>> = msg_send![&bundle, bundleIdentifier];
        let path: Option<Retained<NSString>> = msg_send![&bundle, bundlePath];
        id.is_some() && path.is_some_and(|p| p.to_string().ends_with(".app"))
    }
}

fn center() -> Option<Retained<AnyObject>> {
    let class = objc2::runtime::AnyClass::get(c"UNUserNotificationCenter")?;
    // SAFETY: solo se llama con la app empaquetada (`bundled`).
    unsafe { msg_send![class, currentNotificationCenter] }
}

/// Pone el delegado y pide permiso para avisar (una vez; macOS recuerda la
/// respuesta).
pub fn init() {
    if !bundled() {
        return;
    }
    super::main_thread::run(|| {
        let Some(center) = center() else {
            return;
        };
        let delegate = Delegate::new();
        // SAFETY: el delegado vive todo el proceso (`forget`), como pide
        // `UNUserNotificationCenter`, que no lo retiene.
        unsafe {
            let _: () = msg_send![&center, setDelegate: &*delegate];
            let done = RcBlock::new(|granted: Bool, _error: *mut AnyObject| {
                if !granted.as_bool() {
                    tracing::info!("avisos: el usuario no dio permiso");
                }
            });
            // Alerta, sonido e insignia.
            let _: () = msg_send![&center, requestAuthorizationWithOptions: (1usize << 2) | (1 << 1) | 1, completionHandler: &*done];
        }
        std::mem::forget(delegate);
        READY.store(true, Ordering::Release);
    });
}

pub fn show(title: &str, body: &str, error: bool, topic: Option<String>) -> bool {
    if !READY.load(Ordering::Acquire) {
        return fallback(title, body);
    }
    let (title, body) = (title.to_string(), body.to_string());
    super::main_thread::run(move || {
        let Some(center) = center() else {
            return;
        };
        let (Some(content_class), Some(request_class)) = (
            objc2::runtime::AnyClass::get(c"UNMutableNotificationContent"),
            objc2::runtime::AnyClass::get(c"UNNotificationRequest"),
        ) else {
            return;
        };
        // SAFETY: objetos de UserNotifications creados y usados en el hilo
        // principal; el centro copia el pedido.
        unsafe {
            let content: Retained<AnyObject> = msg_send![content_class, new];
            let _: () = msg_send![&content, setTitle: &*NSString::from_str(&title)];
            let _: () = msg_send![&content, setBody: &*NSString::from_str(&body)];
            if let Some(topic) = &topic {
                let key = NSString::from_str("topic");
                let value = NSString::from_str(topic);
                let info = NSDictionary::<NSString, AnyObject>::from_slices(&[&*key], &[value.as_ref()]);
                let _: () = msg_send![&content, setUserInfo: &*info];
            }
            if error {
                let sound: Option<Retained<AnyObject>> = msg_send![class!(UNNotificationSound), defaultSound];
                if let Some(sound) = sound {
                    let _: () = msg_send![&content, setSound: &*sound];
                }
            }
            let id = NSString::from_str(&format!("atic-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)));
            let nil: *const AnyObject = std::ptr::null();
            let request: Retained<AnyObject> =
                msg_send![request_class, requestWithIdentifier: &*id, content: &*content, trigger: nil];
            let none: Option<&Block<dyn Fn(*mut AnyObject)>> = None;
            let _: () = msg_send![&center, addNotificationRequest: &*request, withCompletionHandler: none];
        }
    });
    true
}

/// Sin `.app`: el aviso por AppleScript, sin clic.
fn fallback(title: &str, body: &str) -> bool {
    fn quoted(text: &str) -> String {
        format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
    }
    let script = format!("display notification {} with title {}", quoted(body), quoted(title));
    std::process::Command::new("osascript")
        .args(["-e", &script])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok()
}
