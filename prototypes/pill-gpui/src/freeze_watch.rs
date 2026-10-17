//! Congelamientos: si el hilo principal deja de avanzar unos segundos, se
//! guarda qué estaba haciendo (`/usr/bin/sample` del propio proceso) junto
//! al log, para ver dónde quedó trabado sin tener que reproducirlo.
//!
//! El tick de la pill marca el pulso (`beat`); un hilo aparte lo mira. El
//! reloj monótono de macOS no avanza con el equipo dormido, así que dormir
//! no cuenta como congelamiento.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Sin pulso por esto, está congelada.
const STUCK: Duration = Duration::from_secs(5);
const CHECK: Duration = Duration::from_secs(1);

static START: OnceLock<Instant> = OnceLock::new();
/// Milisegundos desde `START` del último pulso.
static LAST: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// El hilo principal sigue vivo.
pub fn beat() {
    LAST.store(now_ms(), Ordering::Relaxed);
}

pub fn start() {
    beat();
    let spawned = std::thread::Builder::new().name("vigia-congelado".into()).spawn(|| {
        // Uno por congelamiento: hasta que vuelva el pulso no se repite.
        let mut reported = false;
        loop {
            std::thread::sleep(CHECK);
            let quiet = now_ms().saturating_sub(LAST.load(Ordering::Relaxed));
            if quiet < STUCK.as_millis() as u64 {
                reported = false;
                continue;
            }
            if reported {
                continue;
            }
            reported = true;
            let Some(dir) = crate::paths::logs_dir() else {
                continue;
            };
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or_default();
            let file = dir.join(format!("congelado-{stamp}.txt"));
            tracing::error!(ms = quiet, archivo = %file.display(), "la pill no responde: guardando dónde quedó");
            let _ = std::process::Command::new("/usr/bin/sample")
                .arg(std::process::id().to_string())
                .arg("3")
                .arg("-file")
                .arg(&file)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    });
    if let Err(error) = spawned {
        tracing::warn!(%error, "no arrancó el vigía de congelamientos");
    }
}
