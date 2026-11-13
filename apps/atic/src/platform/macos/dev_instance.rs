//! Al abrir la app de desarrollo, la instalada deja libres los atajos y datos.

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2::{class, msg_send};
use objc2_foundation::{NSArray, NSString};

fn should_close(current: Option<&str>, candidate: Option<&str>, own: i32, pid: i32) -> bool {
    current == Some("com.ciat.atic.pill") && candidate == Some("com.ciat.atic") && pid > 0 && pid != own
}

/// Solo la `.app` de desarrollo reemplaza a la publicada. Se llama antes del
/// candado de instancia única, también cuando macOS la reabre por permisos.
pub fn prepare_dev_start() -> bool {
    let current = super::bundle_id();
    if current.as_deref() != Some("com.ciat.atic.pill") {
        return true;
    }
    objc2::rc::autoreleasepool(|_| unsafe {
        let id = NSString::from_str("com.ciat.atic");
        let apps: Retained<NSArray<AnyObject>> =
            msg_send![class!(NSRunningApplication), runningApplicationsWithBundleIdentifier: &*id];
        let mut pids = Vec::new();
        for app in apps.iter() {
            let pid: i32 = msg_send![&*app, processIdentifier];
            let bundle: Option<Retained<NSString>> = msg_send![&*app, bundleIdentifier];
            let bundle = bundle.map(|id| id.to_string());
            if !should_close(current.as_deref(), bundle.as_deref(), std::process::id() as i32, pid) {
                continue;
            }
            tracing::info!(pid, "la pill de desarrollo pide cerrar la instalada");
            let accepted: Bool = msg_send![&*app, terminate];
            if !accepted.as_bool() {
                tracing::warn!(pid, "la instalada no aceptó cerrarse; la dev no arranca");
                return false;
            }
            pids.push(pid);
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            // `isTerminated` depende del run loop de AppKit, que aún no corre.
            // kill con señal 0 solo consulta si el proceso sigue existiendo.
            pids.retain(|&pid| libc::kill(pid, 0) == 0
                || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH));
            if pids.is_empty() {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                tracing::warn!(?pids, "la instalada sigue cerrándose; la dev no arranca");
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solo_la_dev_cierra_la_instalada_y_nunca_a_si_misma() {
        let dev = Some("com.ciat.atic.pill");
        let installed = Some("com.ciat.atic");
        assert!(should_close(dev, installed, 10, 20));
        assert!(!should_close(installed, dev, 10, 20));
        assert!(!should_close(dev, dev, 10, 20));
        assert!(!should_close(None, installed, 10, 20));
        assert!(!should_close(dev, Some("com.apple.finder"), 10, 20));
        assert!(!should_close(dev, None, 10, 20));
        assert!(!should_close(dev, installed, 10, 10));
        assert!(!should_close(dev, installed, 10, 0));
        assert!(!should_close(dev, installed, 10, -1));
    }
}
