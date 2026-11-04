//! Handshake stdio contra el binario real: cada respuesta es una línea JSON
//! válida, `tools` trae las del modo elegido, y por stdout no sale nada raro.
//!
//! Además: sin hub, `atic_list_agents` devuelve `isError` con el copy exacto
//! de `hub_missing`; `atic_prompt` acepta `wait: false` como async.

mod common;

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
    loop {
        let respuesta = linea(lector);
        if respuesta["id"] == id {
            return respuesta;
        }
    }
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

    // `tools/list`: tiene que traer las del hub sin consultarlo.
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
        "atic_permission",
        "atic_cancel",
        "atic_close",
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
        "Atic Code no está abierto: ábrelo para delegar (sin Atic no hay quién apruebe los permisos del otro agente)."
    );

    // `wait: false` ya llega al hub; falla solo porque no está abierto.
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
    assert_eq!(sin_espera["result"]["content"][0]["text"], atic_mcp::hub_client::HUB_MISSING);

    let _ = hijo.kill();
    let _ = hijo.wait_timeout(Duration::from_secs(5));
}

#[test]
fn delegate_por_stdio_respeta_modos_contexto_y_cancelacion() {
    use serde_json::json;
    let (_, hub_json, pedidos) = common::arrancar_con_pedidos();
    struct Proceso(std::process::Child);
    impl Drop for Proceso {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut proceso = Proceso(std::process::Command::new(env!("CARGO_BIN_EXE_atic-mcp"))
        .args(["--delegate", "--host", "cursor", "--wait", "7"])
        .env("ATIC_HUB_JSON", hub_json)
        .env("ATIC_SESSION", "padre-1")
        .env("ATIC_DELEGATE_DEPTH", "2")
        .env("ATIC_ROOT", "raiz-1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn().unwrap());
    let mut stdin = proceso.0.stdin.take().unwrap();
    let mut lector = BufReader::new(proceso.0.stdout.take().unwrap());
    let init = pedir(&mut stdin, &mut lector, 1, "initialize", json!({
        "protocolVersion": "2025-11-25", "capabilities": {},
        "clientInfo": {"name": "cursor-agent", "version": "0"}
    }));
    assert_eq!(init["result"]["serverInfo"]["name"], "atic");
    assert_eq!(init["result"]["instructions"], "Directorio de agentes de Atic para delegar tareas a subagentes.");
    writeln!(stdin, "{}", json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).unwrap();
    stdin.flush().unwrap();
    let lista = pedir(&mut stdin, &mut lector, 2, "tools/list", json!({}));
    let tools = lista["result"]["tools"].as_array().unwrap();
    assert!(tools.iter().any(|t| t["name"] == "atic_delegate"));
    assert!(tools.iter().all(|t| !t["name"].as_str().unwrap().starts_with("atic_editor_")));

    let casos = [
        ("atic_delegate", json!({"text": "hola", "role": "implementation"}), "async"),
        ("atic_delegate", json!({"text": "hola", "mode": "async", "role": "research"}), "async"),
        ("atic_delegate", json!({"text": "hola", "mode": "wait", "role": "review"}), "wait"),
        ("atic_prompt", json!({"session": "s1", "text": "hola"}), "async"),
        ("atic_prompt", json!({"session": "s1", "text": "hola", "mode": "async"}), "async"),
        ("atic_prompt", json!({"session": "s1", "text": "hola", "mode": "wait"}), "wait"),
        ("atic_prompt", json!({"session": "s1", "text": "hola", "wait": false}), "async"),
        ("atic_prompt", json!({"session": "s1", "text": "hola", "wait": true}), "wait"),
        ("atic_prompt", json!({"session": "s1", "text": "hola", "wait": true, "mode": "async"}), "async"),
        ("atic_prompt", json!({"session": "s1", "text": "hola", "wait": false, "mode": "wait"}), "wait"),
    ];
    for (i, (nombre, args, modo)) in casos.into_iter().enumerate() {
        let respuesta = pedir(&mut stdin, &mut lector, 10 + i as u64, "tools/call", json!({
            "name": nombre, "arguments": args, "_meta": {"progressToken": "prueba"}
        }));
        assert_ne!(respuesta["result"]["isError"], true, "{respuesta}");
        let outcome: serde_json::Value = serde_json::from_str(respuesta["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(outcome["session"], "s1");
        let (ruta, cuerpo) = pedidos.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(ruta, format!("/v1/{}", nombre.trim_start_matches("atic_")));
        assert_eq!(cuerpo["mode"], modo);
        assert_eq!(cuerpo["text"], "hola");
        assert!(cuerpo.get("wait").is_none());
        if modo == "async" {
            assert!(cuerpo.get("waitS").is_none(), "async no manda presupuesto de espera");
            assert_eq!(outcome["status"], "running");
            assert_eq!(outcome["text"], "");
            assert!(outcome["hint"].as_str().unwrap().contains("Termina tu turno"));
        } else {
            assert_eq!(cuerpo["waitS"], 7);
            assert_eq!(outcome["status"], if nombre == "atic_delegate" { "timeout" } else { "done" });
        }
        if nombre == "atic_delegate" {
            assert_eq!(cuerpo["role"], args["role"]);
            assert_eq!(cuerpo["backend"], "auto");
            assert_eq!(cuerpo["parent"], "padre-1");
            assert_eq!(cuerpo["depth"], 2);
            assert_eq!(cuerpo["root"], "raiz-1");
            assert_eq!(cuerpo["host"], "cursor");
        } else {
            assert_eq!(cuerpo["from"], "padre-1");
        }
    }
    let cancelada = pedir(&mut stdin, &mut lector, 30, "tools/call", json!({
        "name": "atic_cancel", "arguments": {"session": "s1"}
    }));
    let resultado: serde_json::Value = serde_json::from_str(cancelada["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(resultado, json!({"session": "s1", "status": "cancel_requested"}));

    let editor = pedir(&mut stdin, &mut lector, 31, "tools/call", json!({
        "name": "atic_editor_state", "arguments": {}
    }));
    assert!(editor.get("error").is_some() || editor["result"]["isError"] == true);
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
