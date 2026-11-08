//! Qué suscripciones tiene el usuario en cada servicio de agentes y si responden, para que
//! un orquestador levante a su subagente con una que sirva. Hoy, OpenCode: Go (la
//! suscripción, con su cupo), Zen (`opencode/…`, cobra del saldo de la cuenta; sus `-free`
//! no) y las claves de otros proveedores.
//!
//! Antes de esto, un orquestador pedía `opencode/deepseek…` (Zen, sin saldo) en vez de
//! `opencode-go/deepseek…` y el hijo moría en segundos sin decir por qué.
//!
//! Mirar el cupo de Go es una consulta HTTP: se guarda unos minutos y se renueva en otro
//! hilo. Mientras no hay respuesta, Go cuenta como disponible (está conectado).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// Cuánto vale lo que se sabe del cupo antes de preguntar de nuevo.
const FRESH: Duration = Duration::from_secs(5 * 60);

/// Cómo está una suscripción o credencial.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// Conectada y, hasta donde se sabe, responde.
    Active,
    /// Gastó el cupo de alguna ventana (Go): vuelve cuando se renueva.
    Exhausted,
    /// Sin saldo (Zen): sus modelos de pago fallan.
    NoBalance,
    /// La clave no sirve (la API dijo 401).
    Invalid,
}

impl Status {
    pub fn usable(self) -> bool {
        self == Status::Active
    }

    fn wire(self) -> &'static str {
        match self {
            Status::Active => "active",
            Status::Exhausted => "exhausted",
            Status::NoBalance => "no-balance",
            Status::Invalid => "invalid",
        }
    }
}

/// Una credencial de un servicio, con lo que se sabe de ella.
#[derive(Clone, Debug, PartialEq)]
pub struct Sub {
    /// El proveedor como lo nombra el servicio (`opencode-go`, `opencode`, `kimi-for-coding`).
    pub provider: String,
    pub status: Status,
    /// Para quien elige: el cupo usado, cuándo se renueva o por qué no sirve.
    pub detail: Option<String>,
}

/// El cupo de Go la última vez que se preguntó.
struct GoCheck {
    at: Instant,
    status: Status,
    detail: Option<String>,
}

static GO: Mutex<Option<GoCheck>> = Mutex::new(None);
static ASKING: Mutex<bool> = Mutex::new(false);

/// Las credenciales de OpenCode con su estado. Lee el disco (rápido); el cupo de Go sale de
/// lo guardado y, si está viejo, se renueva en otro hilo para la próxima vez.
pub fn opencode() -> Vec<Sub> {
    let providers = crate::quota::opencode_usage::connected_providers();
    let zen_broke = zen_without_balance();
    refresh_go_if_stale();
    let go = GO.lock().unwrap().as_ref().map(|g| (g.status, g.detail.clone()));
    providers
        .into_iter()
        .map(|provider| {
            let (status, detail) = match provider.as_str() {
                "opencode-go" => go.clone().unwrap_or((Status::Active, None)),
                "opencode" if zen_broke => (Status::NoBalance, Some(crate::i18n::t("pill.code.acp.zenNoFunds").to_string())),
                _ => (Status::Active, None),
            };
            Sub { provider, status, detail }
        })
        .collect()
}

/// El log de OpenCode ya dijo «Insufficient account funds» (lo último que cuenta).
fn zen_without_balance() -> bool {
    super::acp::opencode_reason().is_some_and(|reason| reason == crate::i18n::t("pill.code.acp.zenNoFunds"))
}

fn refresh_go_if_stale() {
    let stale = GO.lock().unwrap().as_ref().is_none_or(|g| g.at.elapsed() > FRESH);
    if !stale || !crate::quota::opencode_usage::detected() {
        return;
    }
    {
        let mut asking = ASKING.lock().unwrap();
        if *asking {
            return;
        }
        *asking = true;
    }
    std::thread::spawn(|| {
        let (status, detail) = match crate::quota::opencode_usage::fetch_account_usage() {
            Ok(usage) => go_status(&usage.windows.iter().map(|w| (w.kind.clone(), w.percent, w.resets_at.clone())).collect::<Vec<_>>()),
            Err(error) if error == crate::i18n::t("pill.quota.opencode.invalidKey") => (Status::Invalid, Some(error)),
            // Sin red o la API cambió: no se sabe, así que no se descarta.
            Err(_) => (Status::Active, None),
        };
        *GO.lock().unwrap() = Some(GoCheck { at: Instant::now(), status, detail });
        *ASKING.lock().unwrap() = false;
    });
}

/// El estado de Go por sus ventanas de cupo (tipo, % usado, renovación): agotado si alguna
/// llegó al 100 %.
fn go_status(windows: &[(String, f64, Option<String>)]) -> (Status, Option<String>) {
    match windows.iter().find(|(_, percent, _)| *percent >= 100.) {
        Some((kind, _, resets)) => (Status::Exhausted, Some(format!("cupo {kind} agotado{}", resets.as_ref().map(|r| format!(", se renueva {r}")).unwrap_or_default()))),
        None => {
            let used = windows.iter().map(|(kind, percent, _)| format!("{kind} {percent:.0} %")).collect::<Vec<_>>().join(", ");
            (Status::Active, (!used.is_empty()).then(|| format!("usado: {used}")))
        }
    }
}

/// Lo que necesita un modelo de OpenCode para responder: la credencial de su proveedor y,
/// en Zen, saldo (salvo los `-free`).
pub fn model_usable(id: &str, subs: &[Sub]) -> bool {
    let Some((provider, _)) = id.split_once('/') else {
        return true;
    };
    if provider == "opencode" && id.ends_with("-free") {
        return true;
    }
    // OpenCode solo lista proveedores con credencial; si no aparece aquí, no se sabe: se deja.
    subs.iter().find(|s| s.provider == provider).is_none_or(|s| s.status.usable())
}

/// Las credenciales para la respuesta del hub (`atic_list_agents` / `atic_list_models`).
pub fn to_json(subs: &[Sub]) -> Value {
    Value::Array(
        subs.iter()
            .map(|s| {
                let mut v = json!({ "provider": s.provider, "status": s.status.wire() });
                if let Some(detail) = &s.detail {
                    v["detail"] = json!(detail);
                }
                v
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sub(provider: &str, status: Status) -> Sub {
        Sub { provider: provider.into(), status, detail: None }
    }

    #[test]
    fn un_modelo_de_zen_de_pago_no_sirve_sin_saldo_pero_el_gratis_si() {
        let subs = [sub("opencode-go", Status::Active), sub("opencode", Status::NoBalance)];
        assert!(!model_usable("opencode/deepseek-v4.1-flash", &subs));
        assert!(model_usable("opencode/big-pickle-free", &subs));
        assert!(model_usable("opencode-go/deepseek-v4.1-flash", &subs));
        assert!(model_usable("otro/x", &subs), "sin datos de ese proveedor no se descarta");
    }

    #[test]
    fn go_agotado_si_alguna_ventana_llega_al_cien() {
        let (status, detail) = go_status(&[("rolling".into(), 40., None), ("weekly".into(), 100., Some("2026-11-10".into()))]);
        assert_eq!(status, Status::Exhausted);
        assert!(detail.unwrap().contains("weekly"));
        assert_eq!(go_status(&[("rolling".into(), 40., None)]).0, Status::Active);
    }
}
