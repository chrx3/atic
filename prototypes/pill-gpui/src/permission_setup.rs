//! Cuándo pedir los permisos de macOS: al abrir la pill, si falta uno que no
//! faltaba ya la última vez (la primera vez, todos; después, uno que se quitó
//! en Ajustes o que llegó con una versión nueva). Se piden en la bienvenida,
//! en su paso de Permisos (`onboarding::open_permissions`).

use gpui::App;

use crate::platform::permissions::{self, Permission, State};

/// La marca de «Listo» o «No preguntar por estos»: una por titular, como los
/// permisos (los de la `.app` y los de la terminal son distintos para
/// macOS). Guarda los que faltaban al cerrarla, uno por línea.
fn marker() -> Option<std::path::PathBuf> {
    let holder = if crate::platform::bundled() { "app" } else { "terminal" };
    crate::paths::file(&format!(".permisos-listos-{holder}"))
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
    tracing::info!(faltan = ?missing, "permisos de macOS");
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
