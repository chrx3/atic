//! Vigilante del equipo: avisa cuando se ahoga, sin que nadie abra nada.
//!
//! # Por qué existe
//!
//! El panel de sistema hay que acordarse de abrirlo, y uno se acuerda cuando
//! ya está sufriendo. Esto le da vuelta la carga: un hilo toma el pulso cada
//! pocos segundos y, si la CPU o la memoria se quedan arriba **un rato**, la
//! pill lo dice con el mismo vocabulario que usa para los agentes.
//!
//! # Lo que NO hace
//!
//! No enumera procesos en cada latido. Saber si la CPU está alta cuesta dos
//! llamadas ([`snapshot::vitals`]); saber **quién** la tiene alta cuesta
//! recorrer los ~500 procesos de la máquina. Lo segundo solo se paga cuando
//! hay algo que contar.
//!
//! No avisa por picos. Abrir una app deja la CPU en 100% por dos segundos y
//! eso no es un problema: el aviso pide que el umbral se sostenga.

use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use super::snapshot;
use crate::state::AppState;

/// Cada cuánto se toma el pulso.
const LATIDO: Duration = Duration::from_secs(5);

/// Cuánto hay que bajar del umbral para que el aviso se apague.
///
/// Sin margen, un valor que baila alrededor del umbral encendería y apagaría
/// el chip cada cinco segundos, que es peor que no avisar.
const HISTERESIS: f32 = 8.0;

const EVENTO: &str = "system-alert";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AlertKind {
    Cpu,
    Ram,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemAlert {
    pub kind: AlertKind,
    /// Valor que disparó el aviso, en porcentaje.
    pub value: f32,
    pub threshold: u8,
    /// Quién se lo está comiendo, si se pudo averiguar.
    pub culprit: Option<String>,
}

/// Avisos encendidos ahora mismo. La pill los pide al montarse.
static ACTIVOS: Mutex<Vec<SystemAlert>> = Mutex::new(Vec::new());

pub fn active() -> Vec<SystemAlert> {
    ACTIVOS.lock().map(|v| v.clone()).unwrap_or_default()
}

/// Umbral sostenido: la decisión de encender y apagar, sin reloj de verdad.
///
/// Se prueba con un `Instant` de mentira, que es la única forma de probar
/// "dos minutos por encima" sin esperar dos minutos.
#[derive(Debug, Default, Clone, Copy)]
pub struct Sostenido {
    desde: Option<Instant>,
    encendido: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Paso {
    Nada,
    Encender,
    Apagar,
}

impl Sostenido {
    pub fn encendido(&self) -> bool {
        self.encendido
    }

    pub fn tick(&mut self, valor: f32, umbral: u8, aguantar: Duration, ahora: Instant) -> Paso {
        // Umbral 0 = no vigilar esta métrica.
        if umbral == 0 {
            let estaba = self.encendido;
            *self = Self::default();
            return if estaba { Paso::Apagar } else { Paso::Nada };
        }
        let arriba = valor >= umbral as f32;
        if arriba {
            let desde = *self.desde.get_or_insert(ahora);
            if !self.encendido && ahora.duration_since(desde) >= aguantar {
                self.encendido = true;
                return Paso::Encender;
            }
            return Paso::Nada;
        }
        self.desde = None;
        if self.encendido && valor <= umbral as f32 - HISTERESIS {
            self.encendido = false;
            return Paso::Apagar;
        }
        Paso::Nada
    }
}

/// Arranca el hilo. Se llama una vez, al levantar la app.
pub fn start(app: AppHandle) {
    thread::Builder::new()
        .name("system-watch".into())
        .spawn(move || run(app))
        .ok();
}

fn run(app: AppHandle) {
    let mut cpu = Sostenido::default();
    let mut ram = Sostenido::default();
    loop {
        thread::sleep(LATIDO);
        let Some((on, umbral_cpu, umbral_ram, aguantar)) = ajustes(&app) else {
            continue;
        };
        if !on {
            // Apagado en caliente: si quedaba un aviso encendido, se baja.
            if cpu.encendido() || ram.encendido() {
                cpu = Sostenido::default();
                ram = Sostenido::default();
                publicar(&app, Vec::new());
            }
            continue;
        }
        let Ok(v) = snapshot::vitals() else {
            continue;
        };
        let ahora = Instant::now();
        let paso_cpu = cpu.tick(v.cpu, umbral_cpu, aguantar, ahora);
        let paso_ram = ram.tick(snapshot::ram_percent(&v), umbral_ram, aguantar, ahora);
        if paso_cpu == Paso::Nada && paso_ram == Paso::Nada {
            continue;
        }
        // Recién acá se paga la lista: hay algo que contar y hay que nombrarlo.
        let culpables = culprits();
        let mut avisos = Vec::new();
        if cpu.encendido() {
            avisos.push(SystemAlert {
                kind: AlertKind::Cpu,
                value: v.cpu,
                threshold: umbral_cpu,
                culprit: culpables.0.clone(),
            });
        }
        if ram.encendido() {
            avisos.push(SystemAlert {
                kind: AlertKind::Ram,
                value: snapshot::ram_percent(&v),
                threshold: umbral_ram,
                culprit: culpables.1.clone(),
            });
        }
        publicar(&app, avisos);
    }
}

/// (quien se come la CPU, quien se come la RAM).
fn culprits() -> (Option<String>, Option<String>) {
    let Ok(snap) = snapshot::read() else {
        return (None, None);
    };
    let cpu = snap
        .apps
        .iter()
        .max_by(|a, b| {
            a.cpu
                .partial_cmp(&b.cpu)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|a| a.name.clone());
    let ram = snap
        .apps
        .iter()
        .max_by_key(|a| a.ram_bytes)
        .map(|a| a.name.clone());
    (cpu, ram)
}

fn publicar(app: &AppHandle, avisos: Vec<SystemAlert>) {
    if let Ok(mut guard) = ACTIVOS.lock() {
        *guard = avisos.clone();
    }
    let _ = app.emit(EVENTO, avisos);
}

/// Lee los umbrales de la config en cada latido: cambiarlos vale al toque.
fn ajustes(app: &AppHandle) -> Option<(bool, u8, u8, Duration)> {
    let state = app.try_state::<AppState>()?;
    let cfg = state.config.lock().ok()?;
    Some((
        cfg.system_alerts,
        cfg.system_alert_cpu,
        cfg.system_alert_ram,
        Duration::from_secs(cfg.system_alert_seconds.max(5) as u64),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_pico_no_avisa() {
        let mut s = Sostenido::default();
        let t0 = Instant::now();
        // Arranca alto, pero el umbral pide dos minutos.
        assert_eq!(s.tick(99.0, 85, Duration::from_secs(120), t0), Paso::Nada);
        assert_eq!(
            s.tick(
                99.0,
                85,
                Duration::from_secs(120),
                t0 + Duration::from_secs(30)
            ),
            Paso::Nada
        );
        // Y si baja antes de cumplirlos, el reloj se reinicia.
        assert_eq!(
            s.tick(
                10.0,
                85,
                Duration::from_secs(120),
                t0 + Duration::from_secs(40)
            ),
            Paso::Nada
        );
        assert_eq!(
            s.tick(
                99.0,
                85,
                Duration::from_secs(120),
                t0 + Duration::from_secs(150)
            ),
            Paso::Nada,
            "el reloj arranca de nuevo, no se acumula"
        );
    }

    #[test]
    fn lo_sostenido_si_avisa_y_se_apaga_con_margen() {
        let mut s = Sostenido::default();
        let t0 = Instant::now();
        let dos_min = Duration::from_secs(120);
        assert_eq!(s.tick(90.0, 85, dos_min, t0), Paso::Nada);
        assert_eq!(s.tick(90.0, 85, dos_min, t0 + dos_min), Paso::Encender);
        assert!(s.encendido());
        // Una sola vez: encendido no se re-anuncia en cada latido.
        assert_eq!(
            s.tick(92.0, 85, dos_min, t0 + Duration::from_secs(130)),
            Paso::Nada
        );
        // Justo debajo del umbral NO lo apaga: eso haría parpadear el chip.
        assert_eq!(
            s.tick(84.0, 85, dos_min, t0 + Duration::from_secs(140)),
            Paso::Nada
        );
        assert!(s.encendido());
        // Con margen suficiente, sí.
        assert_eq!(
            s.tick(70.0, 85, dos_min, t0 + Duration::from_secs(150)),
            Paso::Apagar
        );
        assert!(!s.encendido());
    }

    #[test]
    fn umbral_cero_no_vigila() {
        let mut s = Sostenido::default();
        let t0 = Instant::now();
        assert_eq!(s.tick(100.0, 0, Duration::from_secs(1), t0), Paso::Nada);
        assert!(!s.encendido());
    }

    #[test]
    fn apagar_la_vigilancia_baja_el_aviso_encendido() {
        let mut s = Sostenido::default();
        let t0 = Instant::now();
        let corto = Duration::from_secs(1);
        s.tick(99.0, 85, corto, t0);
        assert_eq!(
            s.tick(99.0, 85, corto, t0 + Duration::from_secs(2)),
            Paso::Encender
        );
        assert_eq!(
            s.tick(99.0, 0, corto, t0 + Duration::from_secs(3)),
            Paso::Apagar
        );
    }
}
