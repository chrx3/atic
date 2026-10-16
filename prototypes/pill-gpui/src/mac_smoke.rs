//! Prueba de humo de la capa de macOS: llama a cada API nativa que no
//! necesita ventana. En debug, objc2 revisa el tipo de cada argumento y
//! retorno de `msg_send!` y entra en pánico si no coincide con el del método;
//! esto lo encuentra antes que el usuario.

use crate::platform::permissions::{self, Permission};

#[test]
fn las_apis_de_macos_responden_sin_panico() {
    let started = std::time::Instant::now();
    let lap = |what: &str| eprintln!("[humo] {what}: {} ms", started.elapsed().as_millis());
    // Íconos, nombres y apps (NSWorkspace, NSBitmapImageRep).
    let apps = crate::launcher::apps_folder();
    assert!(!apps.is_empty(), "Spotlight o /Applications deberían dar apps");
    let finder = std::path::Path::new("/System/Library/CoreServices/Finder.app");
    assert!(crate::app_icon::icon_for(finder).is_some(), "el ícono del Finder");
    assert!(crate::app_icon::display_name(finder).is_some());
    let _ = crate::app_icon::icon_for_aumid("com.apple.finder");
    let running = crate::running::scan();
    assert!(!running.windows.is_empty(), "hay apps abiertas");

    lap("apps e íconos");
    // Apariencia, accesibilidad y permisos.
    let _ = crate::theme::windows_light();
    let _ = crate::code::style::system_reduced_motion();
    let _ = crate::glass::backdrop_available();
    for permission in Permission::ALL {
        let _ = permissions::state(permission);
    }
    let _ = permissions::holder();
    let _ = crate::agents::agent_in_front(0);

    lap("apariencia y permisos");
    // Sistema: Core Audio, pantallas, batería, procesos.
    let backend = crate::system::os::Backend::start();
    let snapshot = futures::executor::block_on(backend.read(crate::system::os::Want { audio: true, procs: true }))
        .expect("lectura del sistema");
    assert!(!snapshot.displays.is_empty(), "al menos una pantalla");
    assert!(snapshot.procs.is_some_and(|p| p.ram_total > 0));

    lap("sistema");
    // Privacidad: Core Audio por proceso y Core Media IO.
    let privacy = crate::privacy::Privacy::start();
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let _ = privacy.uses();

    lap("privacidad");
    // Vision: una mancha negra sobre blanco.
    let (w, h) = (120u32, 40u32);
    let mut bgra = vec![255u8; (w * h * 4) as usize];
    for y in 12..28 {
        for x in 10..110 {
            let i = ((y * w + x) * 4) as usize;
            bgra[i..i + 3].copy_from_slice(&[0, 0, 0]);
        }
    }
    crate::ocr::recognize(w, h, &bgra).expect("Vision responde");
    lap("ocr");
    crate::ocr::recognize_lines(w, h, &bgra).expect("Vision responde por palabras");
    lap("ocr por palabras");
}
