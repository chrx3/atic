//! Prueba de humo de la capa de macOS: llama a cada API nativa que no
//! necesita ventana. En debug, objc2 revisa el tipo de cada argumento y
//! retorno de `msg_send!` y entra en pánico si no coincide con el del método;
//! esto lo encuentra antes que el usuario.
//!
//! Las mismas comprobaciones corren en el test y en la autoprueba de arranque
//! (`selftest.rs`), que las hace con el ejecutable empaquetado. Ninguna pide
//! permisos: solo consulta su estado.

use crate::platform::permissions::{self, Permission};

/// Una comprobación: el detalle si salió bien, el motivo si no.
pub type Check = fn() -> Result<String, String>;

pub const CHECKS: &[(&str, Check)] = &[
    ("apps e íconos", apps_and_icons),
    ("apariencia y permisos", appearance_and_permissions),
    ("sistema", system),
    ("privacidad", privacy),
    ("ocr", ocr),
];

fn ensure(ok: bool, why: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(why.to_string())
    }
}

/// Íconos, nombres y apps (NSWorkspace, NSBitmapImageRep).
fn apps_and_icons() -> Result<String, String> {
    let apps = crate::launcher::apps_folder();
    ensure(!apps.is_empty(), "Spotlight o /Applications deberían dar apps")?;
    let finder = std::path::Path::new("/System/Library/CoreServices/Finder.app");
    ensure(crate::app_icon::icon_for(finder).is_some(), "sin el ícono del Finder")?;
    ensure(crate::app_icon::display_name(finder).is_some(), "sin el nombre del Finder")?;
    let _ = crate::app_icon::icon_for_aumid("com.apple.finder");
    // Que haya ventanas abiertas lo exige el test, no la autoprueba: un runner
    // de CI puede no tener ninguna (ahí solo avisa, ver `selftest.rs`).
    let running = crate::running::scan();
    Ok(format!("{} apps, {} abiertas", apps.len(), running.windows.len()))
}

/// Apariencia, accesibilidad y permisos.
fn appearance_and_permissions() -> Result<String, String> {
    let light = crate::theme::windows_light();
    let _ = crate::code::style::system_reduced_motion();
    let _ = crate::glass::backdrop_available();
    let granted = Permission::ALL
        .into_iter()
        .filter(|&permission| matches!(permissions::state(permission), permissions::State::Granted))
        .count();
    let _ = permissions::holder();
    let _ = crate::agents::agent_in_front(0);
    Ok(format!("tema {}, {granted} permisos dados", if light { "claro" } else { "oscuro" }))
}

/// Sistema: Core Audio, pantallas, batería, procesos.
fn system() -> Result<String, String> {
    let backend = crate::system::os::Backend::start();
    let snapshot = futures::executor::block_on(backend.read(crate::system::os::Want { audio: true, procs: true, ..Default::default() }))
        .map_err(|_| "la lectura del sistema no respondió".to_string())?;
    ensure(!snapshot.displays.is_empty(), "no hay ninguna pantalla")?;
    ensure(snapshot.procs.is_some_and(|p| p.ram_total > 0), "sin la memoria total")?;
    Ok(format!("{} pantallas", snapshot.displays.len()))
}

/// Privacidad: Core Audio por proceso y Core Media IO.
fn privacy() -> Result<String, String> {
    let privacy = crate::privacy::Privacy::start();
    std::thread::sleep(std::time::Duration::from_millis(1500));
    Ok(format!("{} usos ahora", privacy.uses().len()))
}

/// Vision: una mancha negra sobre blanco, completa y por palabras.
fn ocr() -> Result<String, String> {
    let (w, h) = (120u32, 40u32);
    let mut bgra = vec![255u8; (w * h * 4) as usize];
    for y in 12..28 {
        for x in 10..110 {
            let i = ((y * w + x) * 4) as usize;
            bgra[i..i + 3].copy_from_slice(&[0, 0, 0]);
        }
    }
    crate::ocr::recognize(w, h, &bgra).map_err(|error| format!("Vision no responde: {error}"))?;
    crate::ocr::recognize_lines(w, h, &bgra).map_err(|error| format!("Vision no responde por palabras: {error}"))?;
    Ok("Vision responde".into())
}

#[test]
fn las_apis_de_macos_responden_sin_panico() {
    let started = std::time::Instant::now();
    for (name, check) in CHECKS {
        let result = check();
        eprintln!("[humo] {name}: {} ms", started.elapsed().as_millis());
        if let Err(why) = result {
            panic!("{name}: {why}");
        }
    }
    assert!(!crate::running::scan().windows.is_empty(), "no hay apps abiertas (al menos el Finder)");
}
