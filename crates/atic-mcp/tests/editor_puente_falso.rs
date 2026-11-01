//! Las herramientas `atic_editor_*` contra un puente de mentira: un
//! `TcpListener` en un hilo que anota los pedidos y responde JSON fijo, más un
//! `editor-bridge.json` temporal.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use atic_mcp::editor::{self, EditorFallo};
use serde_json::{json, Value};

const TOKEN: &str = "tok-prueba";

const SIETE: [&str; 7] = [
    "atic_editor_list",
    "atic_editor_state",
    "atic_editor_frames",
    "atic_editor_transcript",
    "atic_editor_activity",
    "atic_editor_edit",
    "atic_editor_export",
];

struct PuenteFalso {
    json: std::path::PathBuf,
    pedidos: Arc<Mutex<Vec<Value>>>,
}

fn respuesta(pedido: &Value) -> Value {
    let id = pedido["id"].clone();
    if pedido["token"] != TOKEN {
        return json!({"id": id, "ok": false, "error": {"code": "bad_token", "message": "Token inválido."}});
    }
    match pedido["tool"].as_str().unwrap_or("") {
        "frames" => json!({"id": id, "ok": true, "data": {"count": 2}, "images": [
            {"mime": "image/jpeg", "data": "AAAA", "caption": "0:12.3"},
            {"mime": "image/jpeg", "data": "BBBB", "caption": "0:20.0"}
        ]}),
        "edit" => json!({"id": id, "ok": false, "error": {"code": "bad_op", "message": "Operación 1: start debe ser menor que end (0..95.2)."}}),
        _ => json!({"id": id, "ok": true, "data": {"videos": []}}),
    }
}

/// Levanta el puente y escribe su `editor-bridge.json`.
fn arrancar() -> PuenteFalso {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::SeqCst);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let pedidos = Arc::new(Mutex::new(Vec::new()));
    let anotados = pedidos.clone();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(conn) = conn else { continue };
            let anotados = anotados.clone();
            std::thread::spawn(move || {
                let mut linea = String::new();
                if BufReader::new(&conn).read_line(&mut linea).is_err() {
                    return;
                }
                let Ok(pedido) = serde_json::from_str::<Value>(linea.trim()) else {
                    return;
                };
                let resp = respuesta(&pedido);
                anotados.lock().unwrap().push(pedido);
                let _ = writeln!(&conn, "{resp}");
            });
        }
    });
    let dir = std::env::temp_dir().join(format!("atic-mcp-editor-{n}-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let json = dir.join("editor-bridge.json");
    std::fs::write(
        &json,
        format!(r#"{{"port":{port},"token":"{TOKEN}","pid":1}}"#),
    )
    .unwrap();
    PuenteFalso { json, pedidos }
}

#[tokio::test]
async fn el_pedido_llega_con_tool_args_y_token() {
    let puente = arrancar();
    let r = editor::llamar_en(
        &puente.json,
        "state",
        json!({"video": "/tmp/demo.mp4"}),
    )
    .await
    .expect("el puente responde");
    assert_eq!(r.data, json!({"videos": []}));
    let pedidos = puente.pedidos.lock().unwrap();
    assert_eq!(pedidos.len(), 1);
    assert_eq!(pedidos[0]["tool"], "state");
    assert_eq!(pedidos[0]["token"], TOKEN);
    assert_eq!(pedidos[0]["args"], json!({"video": "/tmp/demo.mp4"}));
    assert!(pedidos[0]["id"].is_u64());
}

#[tokio::test]
async fn frames_devuelve_imagenes_cada_una_con_su_caption() {
    let puente = arrancar();
    let r = editor::llamar_en(&puente.json, "frames", json!({"times": [12.3, 20.0]}))
        .await
        .expect("responde");
    assert_eq!(r.images.len(), 2);
    let res = editor::a_resultado(Ok(r));
    assert_eq!(res.is_error, Some(false));
    // texto de `data`, y por imagen: imagen + caption.
    assert_eq!(res.content.len(), 5);
    assert_eq!(res.content[0].as_text().unwrap().text, r#"{"count":2}"#);
    let img = res.content[1].as_image().expect("imagen");
    assert_eq!(img.data, "AAAA");
    assert_eq!(img.mime_type, "image/jpeg");
    assert_eq!(res.content[2].as_text().unwrap().text, "0:12.3");
    assert!(res.content[3].as_image().is_some());
    assert_eq!(res.content[4].as_text().unwrap().text, "0:20.0");
}

#[tokio::test]
async fn un_ok_false_sale_como_error_legible() {
    let puente = arrancar();
    let fallo = editor::llamar_en(&puente.json, "edit", json!({"ops": []}))
        .await
        .expect_err("el puente rechaza");
    assert!(matches!(&fallo, EditorFallo::Puente { code, .. } if code == "bad_op"));
    let res = editor::a_resultado(Err(fallo));
    assert_eq!(res.is_error, Some(true));
    let texto = &res.content[0].as_text().unwrap().text;
    assert!(texto.contains("start debe ser menor que end"), "{texto}");
    assert!(texto.contains("bad_op"), "{texto}");
}

#[tokio::test]
async fn token_malo_se_explica() {
    let puente = arrancar();
    std::fs::write(
        &puente.json,
        std::fs::read_to_string(&puente.json)
            .unwrap()
            .replace(TOKEN, "otro"),
    )
    .unwrap();
    let fallo = editor::llamar_en(&puente.json, "state", json!({}))
        .await
        .expect_err("token malo");
    assert!(fallo.mensaje().contains("bad_token"));
}

#[tokio::test]
async fn sin_archivo_o_sin_puente_pide_abrir_una_grabacion() {
    let sin_archivo = std::env::temp_dir().join("atic-mcp-editor-no-existe.json");
    let fallo = editor::llamar_en(&sin_archivo, "state", json!({}))
        .await
        .expect_err("no hay archivo");
    assert!(matches!(fallo, EditorFallo::Ausente));
    let res = editor::a_resultado(Err(fallo));
    assert_eq!(res.is_error, Some(true));
    assert!(res.content[0]
        .as_text()
        .unwrap()
        .text
        .contains("Abre una grabación en el editor de Atic para poder editarla"));

    // Archivo viejo: el puerto ya no escucha (se libera el puerto a propósito).
    let puerto = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let viejo = std::env::temp_dir().join(format!("atic-mcp-editor-viejo-{}.json", std::process::id()));
    std::fs::write(&viejo, format!(r#"{{"port":{puerto},"token":"x","pid":1}}"#)).unwrap();
    let fallo = editor::llamar_en(&viejo, "state", json!({}))
        .await
        .expect_err("conexión rechazada");
    assert!(matches!(fallo, EditorFallo::Ausente));
}

#[test]
fn el_router_lista_las_siete_del_editor() {
    let mut solo: Vec<String> = atic_mcp::tools::herramientas(true)
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    solo.sort();
    let mut esperadas: Vec<String> = SIETE.iter().map(|s| s.to_string()).collect();
    esperadas.sort();
    assert_eq!(solo, esperadas);

    let todas: Vec<String> = atic_mcp::tools::herramientas(false)
        .iter()
        .map(|t| t.name.to_string())
        .collect();
    for n in SIETE.iter().chain(["atic_delegate", "atic_list_agents"].iter()) {
        assert!(todas.contains(&n.to_string()), "falta {n}");
    }

    let edit = atic_mcp::tools::herramientas(true)
        .into_iter()
        .find(|t| t.name == "atic_editor_edit")
        .unwrap();
    let desc = edit.description.as_deref().unwrap_or("");
    assert!(desc.contains("speed_up_silences") && desc.contains("`remove_all`"));
}

fn linea(lector: &mut BufReader<std::process::ChildStdout>) -> Value {
    let mut texto = String::new();
    lector.read_line(&mut texto).expect("el servidor contesta");
    serde_json::from_str(texto.trim()).expect("línea JSON válida")
}

/// Manda un pedido y salta notificaciones hasta dar con su respuesta.
fn pedir(
    stdin: &mut std::process::ChildStdin,
    lector: &mut BufReader<std::process::ChildStdout>,
    id: u64,
    metodo: &str,
    params: Value,
) -> Value {
    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc": "2.0", "id": id, "method": metodo, "params": params})
    )
    .unwrap();
    stdin.flush().unwrap();
    loop {
        let v = linea(lector);
        if v.get("id") == Some(&json!(id)) {
            return v;
        }
    }
}

#[test]
fn el_binario_con_editor_ofrece_solo_las_siete_y_llega_al_puente() {
    let puente = arrancar();
    let mut hijo = std::process::Command::new(env!("CARGO_BIN_EXE_atic-mcp"))
        .arg("--editor")
        .env("ATIC_EDITOR_BRIDGE_JSON", &puente.json)
        // Sin hub a propósito: con `--editor` no hace falta.
        .env("ATIC_HUB_JSON", std::env::temp_dir().join("atic-mcp-test-sin-hub.json"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = hijo.stdin.take().unwrap();
    let mut lector = BufReader::new(hijo.stdout.take().unwrap());

    let init = pedir(
        &mut stdin,
        &mut lector,
        1,
        "initialize",
        json!({"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "prueba", "version": "0"}}),
    );
    assert_eq!(init["result"]["serverInfo"]["name"], "atic-editor");
    assert!(init["result"]["instructions"]
        .as_str()
        .unwrap()
        .contains("atic_editor_frames"));
    stdin
        .write_all(b"{\"jsonrpc\": \"2.0\", \"method\": \"notifications/initialized\"}\n")
        .unwrap();

    let lista = pedir(&mut stdin, &mut lector, 2, "tools/list", json!({}));
    let mut nombres: Vec<String> = lista["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    nombres.sort();
    let mut esperadas: Vec<String> = SIETE.iter().map(|s| s.to_string()).collect();
    esperadas.sort();
    assert_eq!(nombres, esperadas);

    let frames = pedir(
        &mut stdin,
        &mut lector,
        3,
        "tools/call",
        json!({"name": "atic_editor_frames", "arguments": {"times": [12.3, 20.0], "height": 360}}),
    );
    let contenido = frames["result"]["content"].as_array().unwrap();
    assert_eq!(contenido.len(), 5);
    assert_eq!(contenido[1]["type"], "image");
    assert_eq!(contenido[1]["mimeType"], "image/jpeg");
    assert_eq!(contenido[2]["text"], "0:12.3");
    {
        let pedidos = puente.pedidos.lock().unwrap();
        assert_eq!(pedidos[0]["tool"], "frames");
        assert_eq!(pedidos[0]["token"], TOKEN);
        assert_eq!(pedidos[0]["args"], json!({"times": [12.3, 20.0], "height": 360}));
    }

    let edit = pedir(
        &mut stdin,
        &mut lector,
        4,
        "tools/call",
        json!({"name": "atic_editor_edit", "arguments": {"ops": [{"op": "cut", "start": 5, "end": 1}], "label": "x"}}),
    );
    assert_eq!(edit["result"]["isError"], true);
    assert!(edit["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("start debe ser menor que end"));

    let _ = hijo.kill();
    let t = std::time::Instant::now();
    while t.elapsed() < Duration::from_secs(5) && hijo.try_wait().unwrap().is_none() {
        std::thread::sleep(Duration::from_millis(50));
    }
}
