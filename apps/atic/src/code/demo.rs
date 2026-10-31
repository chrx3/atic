//! `CODE_DEMO=1`: una conversación de ejemplo con cada tipo de fila (texto,
//! razonamiento, herramientas, tareas, un permiso pendiente), para revisar el
//! diseño sin gastar tokens. Pasa por `Chat::apply` como lo real.

use std::path::PathBuf;

use serde_json::json;

use super::chat::Chat;
use crate::i18n::t as tr;

pub fn chat(key: String, workspace: u64, cwd: PathBuf) -> Chat {
    let mut chat = Chat::new(key, workspace, cwd);
    chat.push_user(tr("pill.code.demo.userMessage"), Vec::new());
    chat.apply(
        "assistant",
        &json!({ "id": "d1", "parent": null, "content": [
            { "type": "thinking", "thinking": tr("pill.code.demo.thinking") },
            { "type": "text", "text": tr("pill.code.demo.reviewing") },
            { "type": "tool_use", "id": "t1", "name": "Read", "input": { "file_path": "C:\\code\\referencia\\src-tauri\\src\\workspace.rs" } },
            { "type": "tool_use", "id": "t2", "name": "Grep", "input": { "pattern": "code-workspace", "path": "src-tauri" } }
        ] }),
    );
    chat.apply(
        "user",
        &json!({ "parent": null, "content": [
            { "type": "tool_result", "tool_use_id": "t1", "content": "pub fn open(path: &Path) -> Result<Opened> {\n    let raw = std::fs::read_to_string(path)?;\n    parse(&raw)\n}" },
            { "type": "tool_result", "tool_use_id": "t2", "content": tr("pill.code.demo.noMatches") }
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
                { "content": tr("pill.code.demo.todoDetect"), "activeForm": tr("pill.code.demo.todoDetectActive"), "status": "completed" },
                { "content": tr("pill.code.demo.todoConvert"), "activeForm": tr("pill.code.demo.todoConvertActive"), "status": "in_progress" },
                { "content": tr("pill.code.demo.todoTests"), "activeForm": tr("pill.code.demo.todoTestsActive"), "status": "pending" }
            ] } }
        ] }),
    );
    chat.apply(
        "user",
        &json!({ "parent": null, "content": [
            { "type": "tool_result", "tool_use_id": "t3", "content": tr("pill.code.demo.fileUpdated") },
            { "type": "tool_result", "tool_use_id": "t4", "content": "ok" }
        ] }),
    );
    chat.apply(
        "assistant",
        &json!({ "id": "d3", "parent": null, "content": [
            { "type": "text", "text": tr("pill.code.demo.final") },
            { "type": "tool_use", "id": "t5", "name": "Bash", "input": { "command": "cargo test -p referencia workspace", "description": tr("pill.code.demo.runTests") } }
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
            "description": tr("pill.code.demo.runTests"),
            "suggestions": [{ "type": "addRules", "rules": [{ "toolName": "Bash", "ruleContent": "cargo test:*" }], "behavior": "allow", "destination": "localSettings" }]
        }),
    );
    chat.title = tr("pill.code.demo.title").into();
    chat
}
