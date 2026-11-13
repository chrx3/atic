//! Cuándo pedir los permisos de macOS: al abrir la pill, si falta uno que no
//! faltaba ya la última vez (la primera vez, todos; después, uno que se quitó
//! en Ajustes o que llegó con una versión nueva). Se piden en la bienvenida,
//! en su paso de Permisos (`onboarding::open_permissions`).

use gpui::App;

use crate::platform::permissions::{self, Permission, State};

/// La marca de «Listo» o «No preguntar por estos»: una por titular, como los
/// permisos (la publicada, la dev y la terminal son distintos para macOS).
/// Guarda los que faltaban al cerrarla, uno por línea.
fn marker() -> Option<std::path::PathBuf> {
    crate::paths::file(&marker_name(crate::platform::bundle_id().as_deref()))
}

fn marker_name(bundle: Option<&str>) -> String {
    let holder = match bundle {
        // Conserva la marca de las instalaciones anteriores, solo para la publicada.
        Some("com.ciat.atic") => "app".to_string(),
        Some(id) => format!("app-{}", id.chars().map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' }
        }).collect::<String>()),
        None => "terminal".to_string(),
    };
    format!(".permisos-listos-{holder}")
}

/// Anota lo que falta ahora: esta pantalla no se abre por eso, sí por otro
/// que falte después. La usa también la bienvenida al terminar.
pub(crate) fn remember_missing() {
    if let Some(marker) = marker() {
        if let Some(dir) = marker.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let names: Vec<String> = missing().iter().map(|p| format!("{p:?}")).collect();
        let _ = std::fs::write(marker, names.join("\n"));
    }
}

fn missing() -> Vec<Permission> {
    Permission::ALL.into_iter().filter(|p| permissions::state(*p) != State::Granted).collect()
}

/// Los que faltaban cuando se cerró la pantalla; `None` si nunca se cerró.
fn dismissed() -> Option<Vec<String>> {
    let text = std::fs::read_to_string(marker()?).ok()?;
    Some(text.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect())
}

/// Si hay que mostrarla: falta alguno que no faltaba ya al cerrarla. Una
/// marca vacía (la de «Listo» con todo, o las de antes) no tapa ninguno.
fn should_open(missing: &[Permission], dismissed: Option<&[String]>) -> bool {
    missing.iter().any(|p| !dismissed.is_some_and(|d| d.iter().any(|name| *name == format!("{p:?}"))))
}

/// Al arrancar: si falta un permiso que no faltaba ya al cerrarla.
/// `PILL_OPEN=permissions` la abre igual, para revisarla; en desarrollo no se
/// abre sola (la pill de dev es otro titular para macOS).
pub fn open_if_needed(cx: &mut App) {
    let missing = missing();
    tracing::info!(titular = %permissions::holder(), bundle = ?crate::platform::bundle_id(), faltan = ?missing, "permisos de macOS");
    let forced = std::env::var("PILL_OPEN").as_deref() == Ok("permissions");
    if !forced && (crate::dev::on() || !should_open(&missing, dismissed().as_deref())) {
        return;
    }
    open(cx);
}

/// Los permisos, en el paso de Permisos de la bienvenida.
pub fn open(cx: &mut App) {
    crate::onboarding::open_permissions(cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_dev_no_comparte_la_marca_de_permisos_con_la_publicada() {
        let installed = marker_name(Some("com.ciat.atic"));
        let dev = marker_name(Some("com.ciat.atic.pill"));
        let terminal = marker_name(None);
        assert_eq!(installed, ".permisos-listos-app");
        assert_eq!(terminal, ".permisos-listos-terminal");
        assert_ne!(dev, installed);
        assert_ne!(dev, terminal);
        assert_eq!(dev, marker_name(Some("com.ciat.atic.pill")));
        assert!(!marker_name(Some("otra/app")).contains('/'));
    }

    #[test]
    fn se_muestra_si_falta_uno_que_no_faltaba_al_cerrarla() {
        let missing = [Permission::Accessibility, Permission::SystemAudio];
        // Nunca se cerró: se muestra.
        assert!(should_open(&missing, None));
        // Se cerró con esos mismos faltando: no.
        let both = vec!["Accessibility".to_string(), "SystemAudio".to_string()];
        assert!(!should_open(&missing, Some(&both)));
        // Se cerró con solo uno faltando (el otro se quitó después): sí.
        let one = vec!["SystemAudio".to_string()];
        assert!(should_open(&missing, Some(&one)));
        // La marca vacía de antes no tapa nada.
        assert!(should_open(&missing, Some(&[])));
        // Sin nada que falte, nunca.
        assert!(!should_open(&[], None));
    }
}
