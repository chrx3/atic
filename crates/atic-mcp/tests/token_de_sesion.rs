//! El token de la conversación (`ATIC_HUB_TOKEN`) va primero; si el hub ya no lo
//! conoce, el pedido se repite con el de `hub.json`.
//!
//! En su propio archivo (su propio proceso): fija variables de entorno que los
//! otros tests no deben ver.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;

const COMPARTIDO: &str = "compartido";

/// Un hub de mentira que solo acepta los tokens de `validos` y avisa con cuál
/// llegó cada pedido que no es el `health`.
fn hub_con_tokens(validos: &'static [&'static str]) -> (std::path::PathBuf, mpsc::Receiver<(String, String)>) {
    let (envio, pedidos) = mpsc::channel();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(conn) = conn else { continue };
            let mut lector = BufReader::new(&conn);
            let mut primera = String::new();
            if lector.read_line(&mut primera).is_err() {
                continue;
            }
            let ruta = primera.split(' ').nth(1).unwrap_or_default().to_string();
            let (mut largo, mut token) = (0usize, String::new());
            loop {
                let mut linea = String::new();
                if lector.read_line(&mut linea).is_err() || linea.trim_end().is_empty() {
                    break;
                }
                if let Some((k, v)) = linea.split_once(':') {
                    if k.trim().eq_ignore_ascii_case("content-length") {
                        largo = v.trim().parse().unwrap_or(0);
                    }
                    if k.trim().eq_ignore_ascii_case("authorization") {
                        token = v.trim().trim_start_matches("Bearer ").to_string();
                    }
                }
            }
            let mut cuerpo = vec![0u8; largo];
            let _ = lector.read_exact(&mut cuerpo);
            if ruta != "/v1/health" {
                let _ = envio.send((ruta.clone(), token.clone()));
            }
            let (codigo, json) = if !validos.contains(&token.as_str()) {
                (401, r#"{"error":{"code":"unauthorized","message":"token ajeno"}}"#)
            } else if ruta == "/v1/health" {
                (200, r#"{"ok":true}"#)
            } else {
                (200, r#"{"session":"s1","status":"cancel_requested"}"#)
            };
            let resp = format!("HTTP/1.1 {codigo} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}", json.len());
            let _ = (&conn).write_all(resp.as_bytes());
        }
    });
    let dir = std::env::temp_dir().join(format!("atic-mcp-token-{port}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ruta = dir.join("hub.json");
    std::fs::write(&ruta, format!(r#"{{"port":{port},"token":"{COMPARTIDO}","pid":1}}"#)).unwrap();
    (ruta, pedidos)
}

async fn cancelar() -> Result<atic_mcp::payload::Cancelling, atic_mcp::hub_client::HubFallo> {
    let hub = atic_mcp::hub_client::locate().await.expect("el hub falso respira");
    hub.cancel(&atic_mcp::payload::CancelRequest { session: "s1".into() }).await
}

// Un solo test: los tres casos comparten las variables de entorno del proceso.
#[tokio::test]
async fn pide_con_el_token_de_la_conversacion_y_cae_al_de_hub_json() {
    // El hub conoce el token de la conversación: el pedido va con ese y no se repite.
    let (hub_json, pedidos) = hub_con_tokens(&[COMPARTIDO, "de-sesion"]);
    std::env::set_var("ATIC_HUB_JSON", &hub_json);
    std::env::set_var("ATIC_HUB_TOKEN", "de-sesion");
    cancelar().await.expect("cancela");
    assert_eq!(pedidos.try_iter().collect::<Vec<_>>(), [("/v1/cancel".to_string(), "de-sesion".to_string())]);

    // Atic se reinició y ya no lo conoce: se repite con el de `hub.json`.
    let (hub_json, pedidos) = hub_con_tokens(&[COMPARTIDO]);
    std::env::set_var("ATIC_HUB_JSON", &hub_json);
    std::env::set_var("ATIC_HUB_TOKEN", "viejo");
    cancelar().await.expect("cancela con el de respaldo");
    let tokens: Vec<String> = pedidos.try_iter().map(|(_, token)| token).collect();
    assert_eq!(tokens, ["viejo", COMPARTIDO]);

    // Fuera de Atic Code no hay token de conversación: va el de `hub.json`.
    std::env::remove_var("ATIC_HUB_TOKEN");
    cancelar().await.expect("cancela desde fuera");
    let tokens: Vec<String> = pedidos.try_iter().map(|(_, token)| token).collect();
    assert_eq!(tokens, [COMPARTIDO]);
}
