//! Los cupos de todos los agentes, en una sola forma.
//!
//! # Por qué unificar
//!
//! Cada proveedor cuenta distinto: Claude manda porcentajes con fecha RFC3339,
//! Codex manda porcentajes con segundos Unix, OpenCode manda porcentajes con
//! otra fecha RFC3339, y Cursor manda `autoPercentUsed` / `apiPercentUsed`
//! (los mismos % que su dashboard). La pill no puede aprenderse cuatro formatos
//! para dibujar cuatro barras: pinta [`AgentQuota`] y nada más.
//!
//! Esa traducción vive acá y no en la vista porque es donde se puede probar.
//! «¿Un `resets_at` en segundos se convirtió bien a milisegundos?» se contesta
//! con un test; mirando la pill, no.
//!
//! # Qué NO hace
//!
//! No inventa cupos. Cursor llega con `windows` (Auto / API) cuando el
//! dashboard las publica, y con `spend` solo si hay on-demand de verdad.
//! Sumar el valor de lista de los eventos y pintarlo como barra sería mentir.
//!
//! # Frescura
//!
//! `fetched_at` es de cada agente, no del conjunto: Codex se lee del disco y
//! puede ser de hace horas, mientras los otros tres se acaban de consultar. Un
//! solo sello para los cuatro escondería justo eso.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::Serialize;

/// Cuánto vale un snapshot antes de volver a salir a la red.
///
/// La pill lo pide en cada apertura del hover, que con el mouse yendo y
/// viniendo son varias por minuto. Los cupos se mueven por turno de agente,
/// no por segundo, así que un minuto de caché no envejece nada visible y
/// evita golpear cuatro APIs por cada paso del puntero.
const CACHE_TTL: Duration = Duration::from_secs(60);

/// Una ventana de cupo ya normalizada.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    /// Identificador crudo de la ventana (`5h`, `weekly`, `primary`, …).
    /// Se traduce en la vista, que es donde vive el idioma.
    pub kind: String,
    /// Largo de la ventana en minutos, si el proveedor lo dice.
    pub minutes: Option<u64>,
    /// Porcentaje ya consumido, 0..=100.
    pub used_percent: f64,
    /// Epoch ms del reinicio. Normalizado: cada proveedor lo manda distinto.
    pub resets_at: Option<i64>,
}

/// Consumo on-demand, sin techo conocido. Hoy solo Cursor, y solo si cobra extra.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QuotaSpend {
    pub cents: f64,
    /// Epoch ms en que se reinicia la cuenta del período.
    pub period_end: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentQuota {
    /// Mismo id que usa el catálogo de agentes (`claude`, `codex`,
    /// `opencode`, `cursor-agent`).
    pub agent: String,
    pub plan: Option<String>,
    /// Vacío = este proveedor no publica cupo (o Cursor cayó al fallback).
    pub windows: Vec<QuotaWindow>,
    /// Presente solo cuando no hay cupo que mostrar.
    pub spend: Option<QuotaSpend>,
    /// Epoch ms del dato. Puede ser viejo: Codex se lee del disco.
    pub fetched_at: Option<i64>,
    /// Por qué esta fila no trae datos. Se muestra en la fila, no oculta.
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QuotaOverview {
    pub agents: Vec<AgentQuota>,
    pub fetched_at: i64,
}

struct Cache {
    at: Instant,
    value: QuotaOverview,
}

static CACHE: OnceLock<Mutex<Option<Cache>>> = OnceLock::new();

fn cache() -> &'static Mutex<Option<Cache>> {
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Snapshot de los cupos de los agentes detectados en esta máquina.
///
/// `force` salta la caché: es para el botón de refrescar, no para el poll.
pub fn fetch_overview(force: bool) -> QuotaOverview {
    if !force {
        let guard = cache().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(hit) = guard.as_ref() {
            if hit.at.elapsed() < CACHE_TTL {
                return hit.value.clone();
            }
        }
    }

    let value = collect();
    if let Ok(mut guard) = cache().lock() {
        *guard = Some(Cache {
            at: Instant::now(),
            value: value.clone(),
        });
    }
    value
}

/// Los cuatro en paralelo.
///
/// En serie el peor caso es la suma de los timeouts (20 + 15 + 15 + 20 s), y
/// un proveedor caído dejaría al hover esperando por los otros tres que ya
/// estaban listos. Cada uno en su hilo hace que el total sea el más lento.
fn collect() -> QuotaOverview {
    let mut handles = Vec::new();
    if claude_detected() {
        handles.push(std::thread::spawn(claude_quota));
    }
    if super::codex_usage::detected() {
        handles.push(std::thread::spawn(codex_quota));
    }
    if super::opencode_usage::detected() {
        handles.push(std::thread::spawn(opencode_quota));
    }
    if super::cursor_usage::detected() {
        handles.push(std::thread::spawn(cursor_quota));
    }
    if super::antigravity_usage::detected() {
        handles.push(std::thread::spawn(antigravity_quota));
    }

    let agents = handles
        .into_iter()
        .filter_map(|h| h.join().ok())
        .collect::<Vec<_>>();

    QuotaOverview {
        agents,
        fetched_at: Utc::now().timestamp_millis(),
    }
}

fn claude_detected() -> bool {
    if std::env::var("CLAUDE_CODE_OAUTH_TOKEN").is_ok_and(|v| !v.trim().is_empty()) {
        return true;
    }
    super::skills::config_dir()
        .map(|dir| dir.join(".credentials.json").is_file())
        .unwrap_or(false)
}

/// Envuelve un resultado en la fila del agente, con el error visible.
///
/// Un proveedor que falla se muestra igual, con su motivo: esconder la fila
/// haría que «Claude no aparece» signifique a la vez «no está instalado» y
/// «la sesión venció», que son dos problemas con dos arreglos distintos.
fn row<T>(agent: &str, result: Result<T, String>, map: impl FnOnce(T) -> AgentQuota) -> AgentQuota {
    match result {
        Ok(value) => map(value),
        Err(error) => AgentQuota {
            agent: agent.to_string(),
            plan: None,
            windows: Vec::new(),
            spend: None,
            fetched_at: None,
            error: Some(error),
        },
    }
}

fn claude_quota() -> AgentQuota {
    row("claude", super::claude_usage::fetch_account_usage(), |u| {
        let mut windows = Vec::new();
        let mut push =
            |kind: &str, minutes: u64, win: &Option<super::claude_usage::UsageWindow>| {
                if let Some(win) = win {
                    windows.push(QuotaWindow {
                        kind: kind.to_string(),
                        minutes: Some(minutes),
                        used_percent: win.utilization,
                        resets_at: win.resets_at.as_deref().and_then(rfc3339_ms),
                    });
                }
            };
        push("5h", 300, &u.five_hour);
        push("7d", 10_080, &u.seven_day);
        push("7dOpus", 10_080, &u.seven_day_opus);
        push("7dSonnet", 10_080, &u.seven_day_sonnet);
        // Las semanales de los modelos que la API sumó después (Fable y las que
        // vengan): viajan con el nombre adentro del `kind` para que la vista no
        // necesite una etiqueta nueva por cada una.
        for extra in &u.seven_day_models {
            push(&format!("7d:{}", extra.model), 10_080, &Some(extra.window.clone()));
        }

        AgentQuota {
            agent: "claude".to_string(),
            plan: u.plan,
            windows,
            spend: None,
            fetched_at: Some(u.fetched_at),
            error: None,
        }
    })
}

/// ¿El dato del rollout de Codex todavía sirve? Sirve si al menos una ventana
/// está vigente (su reinicio no pasó). Una ventana sin `resets_at` cuenta como
/// vigente: no se puede probar que venció. `now_secs` en segundos Unix, igual
/// que `resets_at` en el rollout.
fn codex_rollout_is_current(u: &super::codex_usage::CodexAccountUsage, now_secs: i64) -> bool {
    let current = |w: &Option<super::codex_usage::CodexUsageWindow>| {
        w.as_ref()
            .is_some_and(|w| w.resets_at.is_none_or(|s| s >= now_secs))
    };
    current(&u.primary) || current(&u.secondary)
}

/// Levanta `codex app-server` para pedir cupos frescos. Se paga solo cuando el
/// rollout no sirve; el TTL del overview (60 s) acota la frecuencia.
fn codex_fetch_fresh() -> Result<super::codex_usage::CodexAccountUsage, String> {
    let started = Instant::now();
    let result = super::codex_usage::fetch_account_usage();
    tracing::debug!(
        elapsed_ms = started.elapsed().as_millis() as u64,
        ok = result.is_ok(),
        "cupos de Codex vía app-server (fallback)"
    );
    result
}

fn codex_quota() -> AgentQuota {
    let usage = match super::codex_usage::fetch_from_rollout() {
        // Vigente: el disco alcanza y no se paga el proceso.
        Ok(u) if codex_rollout_is_current(&u, Utc::now().timestamp()) => Ok(u),
        // Todo vencido: mejor preguntar de verdad; si no se puede, el dato
        // viejo se muestra igual (la vista ya lo marca de hace días).
        Ok(stale) => Ok(codex_fetch_fresh().unwrap_or(stale)),
        // Sin rollout utilizable: el app-server es la única vía; si también
        // falla, se conserva el motivo original, que es más accionable.
        Err(err) => codex_fetch_fresh().map_err(|_| err),
    };
    row("codex", usage, |u| {
        let mut windows = Vec::new();
        let mut push = |kind: &str, win: Option<super::codex_usage::CodexUsageWindow>| {
            if let Some(win) = win {
                windows.push(QuotaWindow {
                    kind: kind.to_string(),
                    minutes: Some(win.window_duration_mins),
                    used_percent: win.used_percent,
                    // Codex cuenta en segundos Unix; el resto del snapshot en
                    // milisegundos. Se normaliza acá o la vista muestra 1970.
                    resets_at: win.resets_at.map(|s| s * 1000),
                });
            }
        };
        push("primary", u.primary);
        push("secondary", u.secondary);

        AgentQuota {
            agent: "codex".to_string(),
            plan: u.plan,
            windows,
            spend: None,
            fetched_at: Some(u.fetched_at as i64),
            error: None,
        }
    })
}

fn opencode_quota() -> AgentQuota {
    row(
        "opencode",
        super::opencode_usage::fetch_account_usage(),
        |u| AgentQuota {
            agent: "opencode".to_string(),
            plan: None,
            windows: u
                .windows
                .into_iter()
                .map(|w| QuotaWindow {
                    kind: w.kind,
                    minutes: None,
                    used_percent: w.percent,
                    resets_at: w.resets_at.as_deref().and_then(rfc3339_ms),
                })
                .collect(),
            spend: None,
            fetched_at: Some(u.fetched_at),
            error: None,
        },
    )
}

fn cursor_quota() -> AgentQuota {
    row(
        "cursor-agent",
        super::cursor_usage::fetch_account_usage(),
        cursor_from,
    )
}

fn cursor_from(u: super::cursor_usage::CursorAccountUsage) -> AgentQuota {
    let period_end = u.period_end.as_deref().and_then(rfc3339_ms);
    AgentQuota {
        agent: "cursor-agent".to_string(),
        plan: u.plan,
        windows: u
            .windows
            .into_iter()
            .map(|w| QuotaWindow {
                kind: w.kind,
                minutes: None,
                used_percent: w.used_percent,
                resets_at: period_end,
            })
            .collect(),
        spend: if u.spend_cents > 0.5 {
            Some(QuotaSpend {
                cents: u.spend_cents,
                period_end,
            })
        } else {
            None
        },
        fetched_at: Some(u.fetched_at),
        error: None,
    }
}

fn antigravity_quota() -> AgentQuota {
    row(
        "agy",
        super::antigravity_usage::fetch_account_usage(),
        antigravity_from,
    )
}

fn antigravity_from(u: super::antigravity_usage::AntigravityAccountUsage) -> AgentQuota {
    AgentQuota {
        agent: "agy".to_string(),
        plan: None,
        windows: u
            .windows
            .into_iter()
            .map(|w| QuotaWindow {
                // Viaja como semanal por modelo (`7d:<grupo>`): la vista ya
                // sabe pintar esas etiquetas sin aprender un id nuevo.
                kind: format!("7d:{}", w.group),
                minutes: Some(10_080),
                used_percent: w.used_percent,
                resets_at: w.resets_at.as_deref().and_then(rfc3339_ms),
            })
            .collect(),
        spend: None,
        fetched_at: Some(u.fetched_at),
        error: None,
    }
}

fn rfc3339_ms(stamp: &str) -> Option<i64> {
    Some(
        DateTime::parse_from_rfc3339(stamp)
            .ok()?
            .with_timezone(&Utc)
            .timestamp_millis(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normaliza_fechas_rfc3339_a_milisegundos() {
        assert_eq!(rfc3339_ms("2026-08-29T07:24:09.086Z"), Some(1787988249086));
        // Con offset, no solo en Z: el proveedor elige el formato.
        assert_eq!(
            rfc3339_ms("2026-08-29T09:24:09.086+02:00"),
            Some(1787988249086)
        );
        assert_eq!(rfc3339_ms("mañana"), None);
    }

    #[test]
    fn el_rollout_de_codex_vencido_pide_fallback() {
        let win = |resets_at| {
            Some(super::super::codex_usage::CodexUsageWindow {
                used_percent: 10.0,
                window_duration_mins: 300,
                resets_at,
            })
        };
        let usage = |primary, secondary| super::super::codex_usage::CodexAccountUsage {
            primary,
            secondary,
            plan: None,
            limit_name: None,
            fetched_at: 0,
        };
        let now = 1_000_000;
        // Una ventana vigente alcanza para no pagar el proceso.
        assert!(codex_rollout_is_current(
            &usage(win(Some(now + 60)), win(Some(now - 60))),
            now
        ));
        // Todas vencidas: toca el app-server.
        assert!(!codex_rollout_is_current(
            &usage(win(Some(now - 60)), win(Some(now - 1))),
            now
        ));
        // Sin ventanas: ídem.
        assert!(!codex_rollout_is_current(&usage(None, None), now));
        // Sin `resets_at` no se puede probar que venció: cuenta como vigente.
        assert!(codex_rollout_is_current(&usage(win(None), None), now));
    }

    #[test]
    fn una_fila_con_error_conserva_el_motivo() {
        let quota = row::<()>("codex", Err("sin sesiones".into()), |_| unreachable!());
        assert_eq!(quota.agent, "codex");
        assert_eq!(quota.error.as_deref(), Some("sin sesiones"));
        assert!(quota.windows.is_empty());
        assert!(quota.spend.is_none());
        assert!(quota.fetched_at.is_none());
    }

    #[test]
    fn cursor_llega_con_ventanas_y_sin_on_demand() {
        let quota = cursor_row_de_prueba();
        assert_eq!(quota.windows.len(), 2);
        assert_eq!(quota.windows[0].kind, "auto");
        assert!((quota.windows[0].used_percent - 80.5825).abs() < 1e-6);
        assert_eq!(quota.windows[1].kind, "api");
        assert!(quota.spend.is_none(), "on-demand cero no se pinta como gasto");
        assert_eq!(quota.windows[0].resets_at, Some(1788288274000));
    }

    fn cursor_row_de_prueba() -> AgentQuota {
        row(
            "cursor-agent",
            Ok(super::super::cursor_usage::CursorAccountUsage {
                spend_cents: 0.0,
                plan: Some("pro_plus".into()),
                period_start: Some("2026-08-01T18:44:34.000Z".into()),
                period_end: Some("2026-09-01T18:44:34.000Z".into()),
                windows: vec![
                    super::super::cursor_usage::CursorUsageWindow {
                        kind: "auto".into(),
                        used_percent: 80.5825,
                    },
                    super::super::cursor_usage::CursorUsageWindow {
                        kind: "api".into(),
                        used_percent: 87.8,
                    },
                ],
                fetched_at: 1_787_000_000_000,
            }),
            cursor_from,
        )
    }
}
