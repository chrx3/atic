//! Hub de mentira para los tests: un `TcpListener` en un hilo que responde
//! JSON fijo por ruta, más un `hub.json` temporal al que apunta `ATIC_HUB_JSON`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Respuestas fijas por `(método, ruta)`.
pub fn responder(metodo: &str, ruta: &str, cuerpo: &[u8]) -> (u16, String) {
    let base = ruta.split('?').next().unwrap_or(ruta);
    let json = match (metodo, base) {
        ("GET", "/v1/health") => r#"{"version":"0.0.0-test","pid":1}"#.to_string(),
        ("GET", "/v1/agents") => r#"{"agents":[
            {"id":"claude-code","name":"Claude Code","available":true,"blurb":"Planes."},
            {"id":"codex","name":"Codex","available":true,"blurb":"Parches."},
            {"id":"cursor","name":"Cursor","available":false,"blurb":"Aplica."},
            {"id":"opencode","name":"OpenCode","available":false,"blurb":"Liviano."}
        ]}"#
        .to_string(),
        ("GET", "/v1/sessions") => r#"{"sessions":[]}"#.to_string(),
        ("POST", "/v1/delegate") => r#"{"session":"s1","backend":"codex","status":"timeout","text":"parcial","hint":"La sesión sigue viva en Atic. Espera el turno con atic_wait, manda otro con atic_prompt, o mira atic_list_sessions.","elapsed_s":50}"#.to_string(),
        ("POST", "/v1/prompt") => r#"{"session":"s1","backend":"codex","status":"done","text":"hola","hint":null,"elapsed_s":3}"#.to_string(),
        ("POST", "/v1/wait") => r#"{"session":"s1","backend":"codex","status":"done","text":"hola","hint":null,"elapsed_s":3}"#.to_string(),
        ("POST", "/v1/spawn") => r#"{"session":"s9"}"#.to_string(),
        ("POST", "/v1/cancel") => r#"{"session":"s1","status":"cancelling"}"#.to_string(),
        _ => {
            let _ = cuerpo;
            return (404, r#"{"error":{"code":"bad_request","message":"Ruta desconocida."}}"#.to_string());
        }
    };
    let _ = cuerpo;
    (200, json)
}

/// Levanta el hub falso y devuelve el puerto. Cada conexión: una respuesta.
pub fn arrancar() -> (u16, std::path::PathBuf) {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::SeqCst);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(conn) = conn else { continue };
            std::thread::spawn(move || {
                let mut lector = BufReader::new(&conn);
                let mut primera = String::new();
                if lector.read_line(&mut primera).is_err() {
                    return;
                }
                let partes: Vec<&str> = primera.trim_end().splitn(3, ' ').collect();
                let (metodo, ruta) = if partes.len() == 3 {
                    (partes[0].to_string(), partes[1].to_string())
                } else {
                    ("".to_string(), "".to_string())
                };
                let mut largo = 0usize;
                loop {
                    let mut linea = String::new();
                    if lector.read_line(&mut linea).is_err() {
                        return;
                    }
                    if linea.trim_end().is_empty() {
                        break;
                    }
                    if let Some((k, v)) = linea.split_once(':') {
                        if k.trim().eq_ignore_ascii_case("content-length") {
                            largo = v.trim().parse().unwrap_or(0);
                        }
                    }
                }
                let mut cuerpo = vec![0u8; largo.min(1024 * 1024)];
                if !cuerpo.is_empty() {
                    let _ = lector.read_exact(&mut cuerpo);
                }
                let (codigo, json) = responder(&metodo, &ruta, &cuerpo);
                let razon = if codigo == 200 { "OK" } else { "Error" };
                let resp = format!(
                    "HTTP/1.1 {codigo} {razon}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{json}",
                    json.len()
                );
                let _ = (&conn).write_all(resp.as_bytes());
            });
        }
    });
    // `hub.json` temporal para este puerto.
    let dir = std::env::temp_dir().join(format!("atic-mcp-test-{n}-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let ruta = dir.join("hub.json");
    std::fs::write(
        &ruta,
        format!(r#"{{"port":{port},"token":"t{n}","pid":1,"version":"0.0.0-test"}}"#),
    )
    .unwrap();
    (port, ruta)
}
