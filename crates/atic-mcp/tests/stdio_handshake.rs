//! Handshake stdio contra el binario real: cada respuesta es una línea JSON
//! válida, `tools` trae las siete, y por stdout no sale nada raro.
//!
//! Además: sin hub, `atic_list_agents` devuelve `isError` con el copy exacto
//! de `hub_missing`; y `atic_prompt` con `wait: false` devuelve
//! `wait_not_supported` sin tocar el hub.

use std::io::{BufRead, BufReader, Write};
use std::process::Stdio;
use std::time::Duration;

fn linea(lector: &mut BufReader<std::process::ChildStdout>) -> serde_json::Value {
    let mut texto = String::new();
    lector.read_line(&mut texto).expect("el sidecar contesta");
    assert!(!texto.trim().is_empty(), "línea vacía en stdout");
    serde_json::from_str(texto.trim()).expect("cada respuesta es una línea JSON válida")
}

fn pedir(
    hijo: &mut std::process::ChildStdin,
    lector: &mut BufReader<std::process::ChildStdout>,
    id: u64,
    metodo: &str,
    params: serde_json::Value,
) -> serde_json::Value {
    let pedido =
        serde_json::json!({"jsonrpc": "2.0", "id": id, "method": metodo, "params": params});
    writeln!(hijo, "{}", pedido).unwrap();
    hijo.flush().unwrap();
    linea(lector)
}

#[test]
fn handshake_lista_y_errores_hablan_espanol() {
    let exe = env!("CARGO_BIN_EXE_atic-mcp");
    let mut hijo = std::process::Command::new(exe)
        .args(["--host", "cursor"])
        // Sin hub a propósito: apunta a un archivo que no existe.
        .env(
            "ATIC_HUB_JSON",
            std::env::temp_dir().join("atic-mcp-test-sin-hub.json"),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut hijo_stdin = hijo.stdin.take().unwrap();
    let hijo_stdout = hijo.stdout.take().unwrap();
    let mut lector = BufReader::new(hijo_stdout);

    // `initialize` legacy: el servidor dual-era lo entiende.
    let init = pedir(
        &mut hijo_stdin,
        &mut lector,
        1,
        "initialize",
        serde_json::json!({
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": {"name": "cursor-agent", "version": "0"}
        }),
    );
    assert_eq!(init["jsonrpc"], "2.0");
    assert!(init.get("result").is_some(), "initialize responde result");

    // Aviso de listo (puede llegar como notificación sin id: se salta).
    hijo_stdin
        .write_all(b"{\"jsonrpc\": \"2.0\", \"method\": \"notifications/initialized\"}\n")
        .unwrap();

    // `tools/list`: tiene que traer las siete sin tocar el hub.
    let lista = pedir(
        &mut hijo_stdin,
        &mut lector,
        2,
        "tools/list",
        serde_json::json!({}),
    );
    // Si coló una notificación, la siguiente línea es la respuesta.
    let lista = if lista.get("id").is_none() {
        linea(&mut lector)
    } else {
        lista
    };
    let nombres: Vec<String> = lista["result"]["tools"]
        .as_array()
        .expect("tools es lista")
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    for esperada in [
        "atic_list_agents",
        "atic_list_sessions",
        "atic_spawn",
        "atic_prompt",
        "atic_delegate",
        "atic_wait",
        "atic_cancel",
    ] {
        assert!(nombres.contains(&esperada.to_string()), "falta {esperada}");
    }

    // Sin hub: `isError` con el copy, no un stack.
    let sin_hub = pedir(
        &mut hijo_stdin,
        &mut lector,
        3,
        "tools/call",
        serde_json::json!({"name": "atic_list_agents", "arguments": {}}),
    );
    let sin_hub = if sin_hub.get("id").is_none() {
        linea(&mut lector)
    } else {
        sin_hub
    };
    assert_eq!(sin_hub["result"]["isError"], true);
    let texto = sin_hub["result"]["content"][0]["text"].as_str().unwrap();
    assert_eq!(
        texto,
        "Atic no está abierto. Ábrelo desde la bandeja para delegar: sin Atic no hay quién apruebe los permisos del otro agente."
    );

    // `wait: false` se rechaza sin tocar el hub.
    let sin_espera = pedir(
        &mut hijo_stdin,
        &mut lector,
        4,
        "tools/call",
        serde_json::json!({"name": "atic_prompt", "arguments": {"session": "s1", "text": "hola", "wait": false}}),
    );
    let sin_espera = if sin_espera.get("id").is_none() {
        linea(&mut lector)
    } else {
        sin_espera
    };
    assert_eq!(sin_espera["result"]["isError"], true);
    assert!(
        sin_espera["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("siempre espera"),
        "el rechazo explica el modo"
    );

    let _ = hijo.kill();
    let _ = hijo.wait_timeout(Duration::from_secs(5));
}

trait Espera {
    fn wait_timeout(&mut self, dur: Duration) -> std::io::Result<()>;
}

impl Espera for std::process::Child {
    fn wait_timeout(&mut self, dur: Duration) -> std::io::Result<()> {
        let empezo = std::time::Instant::now();
        while empezo.elapsed() < dur {
            if self.try_wait()?.is_some() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Ok(())
    }
}
