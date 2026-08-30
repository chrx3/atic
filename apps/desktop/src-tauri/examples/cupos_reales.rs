//! Prueba manual de los cupos contra las cuentas instaladas de verdad.
//!
//! No es un test, por lo mismo que `agente_real`: sale a la red, depende de
//! qué tengas instalado y de que la sesión de cada proveedor esté viva. Vive
//! como ejemplo para correrlo a mano cuando se toca un lector de cupos.
//!
//! Es la única forma de comprobar la parte que los tests no alcanzan: los tests
//! prueban los parsers contra cuerpos guardados, esto prueba que el cuerpo
//! guardado siga pareciéndose al que manda el proveedor hoy.
//!
//! ```bash
//! cargo run -p atic-desktop --example cupos_reales
//! ```

use atic_desktop_lib::agents::quota;

fn main() {
    let overview = quota::fetch_overview(true);

    if overview.agents.is_empty() {
        println!("no se detectó ningún agente con cupo en este equipo");
        return;
    }

    for agent in &overview.agents {
        let plan = agent.plan.as_deref().unwrap_or("—");
        println!("\n{} ({plan})", agent.agent);

        if let Some(error) = &agent.error {
            println!("  error: {error}");
            continue;
        }
        for win in &agent.windows {
            let minutes = win
                .minutes
                .map(|m| format!("{m} min"))
                .unwrap_or_else(|| "—".into());
            println!(
                "  {:<10} {:>5.1}%  ventana {minutes}  reinicia {}",
                win.kind,
                win.used_percent,
                win.resets_at
                    .map(|ms| ms.to_string())
                    .unwrap_or_else(|| "—".into())
            );
        }
        if let Some(spend) = &agent.spend {
            println!(
                "  consumo US$ {:.2}  corta {}",
                spend.cents / 100.0,
                spend
                    .period_end
                    .map(|ms| ms.to_string())
                    .unwrap_or_else(|| "—".into())
            );
        }
        if let Some(at) = agent.fetched_at {
            let age_min = (overview.fetched_at - at) / 60_000;
            println!("  dato de hace {age_min} min");
        }
    }
}
