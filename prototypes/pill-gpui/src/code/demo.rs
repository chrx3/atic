//! `CODE_DEMO=1`: una conversación de ejemplo con cada tipo de fila (texto,
//! razonamiento, herramientas, tareas, un permiso pendiente), para revisar el
//! diseño sin gastar tokens. Pasa por `Chat::apply` como lo real.

use std::path::PathBuf;

use serde_json::json;

use super::chat::Chat;

pub fn chat(key: String, workspace: u64, cwd: PathBuf) -> Chat {
    let mut chat = Chat::new(key, workspace, cwd);
    chat.push_user("Agrega soporte para importar archivos .code-workspace de VS Code al abrir un workspace.", Vec::new());
    chat.apply(
        "assistant",
        &json!({ "id": "d1", "parent": null, "content": [
            { "type": "thinking", "thinking": "Primero veo cómo se carga hoy el workspace y dónde se detecta la extensión." },
            { "type": "text", "text": "Reviso primero cómo se carga hoy el workspace." },
            { "type": "tool_use", "id": "t1", "name": "Read", "input": { "file_path": "C:\\code\\referencia\\src-tauri\\src\\workspace.rs" } },
            { "type": "tool_use", "id": "t2", "name": "Grep", "input": { "pattern": "code-workspace", "path": "src-tauri" } }
        ] }),
    );
    chat.apply(
        "user",
        &json!({ "parent": null, "content": [
            { "type": "tool_result", "tool_use_id": "t1", "content": "pub fn open(path: &Path) -> Result<Opened> {\n    let raw = std::fs::read_to_string(path)?;\n    parse(&raw)\n}" },
            { "type": "tool_result", "tool_use_id": "t2", "content": "Sin coincidencias" }
        ] }),
    );
    chat.apply(
        "assistant",
        &json!({ "id": "d2", "parent": null, "content": [
            { "type": "tool_use", "id": "t3", "name": "Edit", "input": {
                "file_path": "C:\\code\\referencia\\src-tauri\\src\\workspace.rs",
                "old_string": "    let raw = std::fs::read_to_string(path)?;",
                "new_string": "    let raw = std::fs::read_to_string(path)?;\n    if path.extension().is_some_and(|e| e == \"code-workspace\") {\n        return import::from_vscode(&raw);\n    }"
            } },
            { "type": "tool_use", "id": "t4", "name": "TodoWrite", "input": { "todos": [
                { "content": "Detectar archivos .code-workspace", "activeForm": "Detectando archivos", "status": "completed" },
                { "content": "Convertir carpetas y ajustes", "activeForm": "Convirtiendo carpetas y ajustes", "status": "in_progress" },
                { "content": "Escribir pruebas", "activeForm": "Escribiendo pruebas", "status": "pending" }
            ] } }
        ] }),
    );
    chat.apply(
        "user",
        &json!({ "parent": null, "content": [
            { "type": "tool_result", "tool_use_id": "t3", "content": "Archivo actualizado" },
            { "type": "tool_result", "tool_use_id": "t4", "content": "ok" }
        ] }),
    );
    chat.apply(
        "assistant",
        &json!({ "id": "d3", "parent": null, "content": [
            { "type": "text", "text": "Listo. Ahora `Workspace::open` detecta la extensión y convierte el archivo:\n\n```rust\nif path.extension().is_some_and(|e| e == \"code-workspace\") {\n    return import::from_vscode(&raw);\n}\n```\n\nFalta correr las **pruebas** para confirmar que todo funciona:\n\n- `cargo test -p referencia workspace`\n- revisar el import con un archivo real" },
            { "type": "tool_use", "id": "t5", "name": "Bash", "input": { "command": "cargo test -p referencia workspace", "description": "Ejecutar las pruebas del workspace" } }
        ] }),
    );
    chat.apply("result", &json!({ "subtype": "success", "isError": false, "durationMs": 42_300, "costUsd": 0.0812 }));
    chat.busy = true;
    chat.apply(
        "permission",
        &json!({
            "requestId": "demo-p1",
            "toolName": "Bash",
            "input": { "command": "cargo test -p referencia workspace" },
            "description": "Ejecutar las pruebas del workspace",
            "suggestions": [{ "type": "addRules", "rules": [{ "toolName": "Bash", "ruleContent": "cargo test:*" }], "behavior": "allow", "destination": "localSettings" }]
        }),
    );
    chat.title = "Importar archivos .code-workspace".into();
    chat
}
