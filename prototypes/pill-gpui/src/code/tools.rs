//! Las tarjetas de herramienta de Expressive, como las de la referencia: un verbo, el
//! objetivo y su meta en la cabecera, y al abrir el detalle de cada una.

use gpui::{div, prelude::*, px, AnyElement, ClickEvent, Context, FontWeight, SharedString};
use gpui_m3::{Badge, CodeOutput, ExpandableCard, Icon, LoadingIndicator, Tone};
use serde_json::Value;

use super::chat::ToolCall;
use super::style::t;
use super::CodeView;

/// Cuántas herramientas hijas se ven bajo un subagente que trabaja.
const RECENT_CHILDREN: usize = 5;
/// Lo que se ve de un resultado antes de «Mostrar todo».
const REMINDER: &str = "<system-reminder>";

/// El resultado sin los recordatorios internos de Claude Code.
fn clean(text: &str) -> &str {
    text.split(REMINDER).next().unwrap_or(text).trim_end()
}

fn base_name(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

fn line_count(text: &str) -> usize {
    text.lines().count()
}

/// Verbo, objetivo, si abre sola y si va con tono (subagente, tareas, plan).
fn describe(tool: &ToolCall) -> (String, String, bool, bool) {
    let input = tool.input.clone().unwrap_or(Value::Null);
    let field = |name: &str| input.get(name).and_then(Value::as_str).unwrap_or_default().to_string();
    let name = tool.name.as_str();
    match name {
        "Read" => ("Leer".into(), base_name(&field("file_path")), false, false),
        "Edit" | "MultiEdit" => ("Editar".into(), base_name(&field("file_path")), true, false),
        "Write" => {
            let created = tool.result.as_deref().is_some_and(|r| r.to_lowercase().contains("created"));
            let content = field("content");
            (if created { "Crear" } else { "Escribir" }.into(), base_name(&field("file_path")), line_count(&content) <= 40, false)
        }
        "NotebookEdit" => ("Editar notebook".into(), base_name(&field("notebook_path")), false, false),
        "Bash" | "PowerShell" => {
            let description = field("description");
            let target = if description.is_empty() { field("command").lines().next().unwrap_or_default().to_string() } else { description };
            ("Ejecutar".into(), target, false, false)
        }
        "Grep" => ("Buscar".into(), field("pattern"), false, false),
        "Glob" => ("Buscar archivos".into(), field("pattern"), false, false),
        "WebFetch" => {
            let url = field("url");
            let host = url.split("://").nth(1).and_then(|rest| rest.split('/').next()).unwrap_or(&url).to_string();
            ("Abrir".into(), host, false, false)
        }
        "WebSearch" => ("Buscar en la web".into(), field("query"), false, false),
        "Task" | "Agent" => {
            let kind = field("subagent_type");
            ("Subagente".into(), if kind.is_empty() { "general".into() } else { kind }, false, true)
        }
        "TodoWrite" => ("Tareas".into(), String::new(), true, true),
        "ExitPlanMode" => ("Plan".into(), String::new(), true, true),
        "AskUserQuestion" => {
            let questions = input
                .get("questions")
                .and_then(Value::as_array)
                .map(|list| list.iter().filter_map(|q| q.get("question").and_then(Value::as_str)).collect::<Vec<_>>().join(" · "))
                .unwrap_or_default();
            ("Pregunta".into(), questions, true, false)
        }
        _ => {
            if let Some(rest) = name.strip_prefix("mcp__") {
                let mut parts = rest.splitn(2, "__");
                let server = parts.next().unwrap_or_default();
                let action = parts.next().unwrap_or_default();
                (format!("{server} · {action}"), String::new(), false, false)
            } else {
                (if name.is_empty() { "Herramienta".into() } else { name.to_string() }, String::new(), false, false)
            }
        }
    }
}

/// Un número del input (el SDK a veces manda decimales o texto).
fn number(input: &Value, name: &str) -> u64 {
    match input.get(name) {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.).max(0.) as u64,
        Some(Value::String(s)) => s.trim().parse::<f64>().unwrap_or(0.).max(0.) as u64,
        _ => 0,
    }
}

/// Las líneas que lee un `Read`, como `ReadCard` de la referencia: `offset` es la
/// primera (1 si falta) y `limit` cuántas, así que la última es
/// `desde + limit - 1`. Con solo `limit`, desde la 1.
pub(super) fn read_range(input: &Value) -> Option<String> {
    let (offset, limit) = (number(input, "offset"), number(input, "limit"));
    if offset == 0 && limit == 0 {
        return None;
    }
    let from = offset.max(1);
    Some(if limit > 0 { format!("L{from}\u{2013}{}", from + limit - 1) } else { format!("L{from}+") })
}

/// El texto completo del objetivo para el globito: la ruta, el comando o la URL.
pub(super) fn target_tip(tool: &ToolCall) -> Option<String> {
    let input = tool.input.as_ref()?;
    let field = |name: &str| input.get(name).and_then(Value::as_str).filter(|v| !v.is_empty()).map(str::to_string);
    match tool.name.as_str() {
        "Read" | "Edit" | "MultiEdit" | "Write" => field("file_path"),
        "NotebookEdit" => field("notebook_path"),
        "Bash" | "PowerShell" => field("command"),
        "WebFetch" => field("url"),
        _ => None,
    }
}

/// El archivo que abre un clic en el objetivo (el visor del panel derecho).
pub(super) fn target_file(tool: &ToolCall) -> Option<std::path::PathBuf> {
    match tool.name.as_str() {
        "Read" | "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => target_tip(tool).map(std::path::PathBuf::from),
        _ => None,
    }
}

/// La salida (o la entrada) de una herramienta: el `CodeOutput` de gpui-m3, que muestra
/// `max` líneas y «Mostrar todo (N líneas más)»; `prefix` pone un `$`, `error` la pinta
/// en rojo.
pub(super) fn output(id: String, text: &str, max: usize, prefix: Option<&str>, error: bool) -> CodeOutput {
    CodeOutput::new(SharedString::from(id), clean(text).to_string())
        .max_lines(max)
        .error(error)
        .when_some(prefix, |out, prefix| out.prefix(prefix.to_string()))
}

impl CodeView {
    pub(super) fn tool_m3(&self, tool: &ToolCall, cx: &mut Context<Self>) -> AnyElement {
        let t = t();
        let mono = gpui_m3::theme::MONO_FONT_FAMILY;
        let (verb, target, open_by_default, tonal) = describe(tool);
        let input = tool.input.clone().unwrap_or(Value::Null);
        let field = |name: &str| input.get(name).and_then(Value::as_str).unwrap_or_default().to_string();
        let todo = tool.name == "TodoWrite";

        // La herramienta espera un permiso (`toolUseID` de la solicitud).
        let waiting = self.active_chat().is_some_and(|c| c.permissions.iter().any(|p| p.tool_use_id.as_deref() == Some(tool.id.as_str())));
        let status: AnyElement = match (&tool.result, tool.is_error) {
            (None, _) if waiting => Icon::new("shield").size(px(13.)).color(t.accent).into_any_element(),
            (None, _) => LoadingIndicator::new().size(px(14.)).into_any_element(),
            (Some(_), true) => Icon::new("x").size(px(13.)).color(t.bad).into_any_element(),
            (Some(_), false) => Icon::new("check").size(px(13.)).color(t.ok).into_any_element(),
        };
        let dim = |text: String| div().min_w(px(0.)).truncate().text_color(t.faint).child(text);
        let mut meta = div().flex_1().min_w(px(0.)).flex().items_center().gap(px(6.));
        match tool.name.as_str() {
            "Read" => {
                if let Some(range) = read_range(&input) {
                    meta = meta.child(dim(range));
                }
            }
            "NotebookEdit" => {
                let mode = field("edit_mode");
                if !mode.is_empty() {
                    meta = meta.child(Badge::tonal(Tone::Secondary, mode));
                }
            }
            "Edit" | "MultiEdit" | "Write" => {
                if input.get("replace_all").and_then(Value::as_bool).unwrap_or(false) {
                    meta = meta.child(Badge::tonal(Tone::Secondary, "todas"));
                }
                if let Some(n) = input.get("edits").and_then(Value::as_array).map(Vec::len) {
                    meta = meta.child(Badge::tonal(Tone::Secondary, format!("{n} cambios")));
                }
                if let Some(stats) = super::edits::edit_stats(&tool.name, &input) {
                    let (added, removed) = (stats.added, stats.removed);
                    meta = meta
                        .child(div().font_family(mono).text_size(px(11.5)).text_color(t.ok).child(format!("+{added}")))
                        .when(removed > 0 || tool.name != "Write", |el| {
                            el.child(div().font_family(mono).text_size(px(11.5)).text_color(t.bad).child(format!("\u{2212}{removed}")))
                        });
                }
            }
            "Bash" | "PowerShell" => {
                if input.get("run_in_background").and_then(Value::as_bool).unwrap_or(false) {
                    meta = meta.child(Badge::tonal(Tone::Tertiary, "en segundo plano"));
                }
            }
            "Grep" | "Glob" => {
                let parts: Vec<String> = [base_name(&field("path")), field("glob"), field("type")].into_iter().filter(|p| !p.is_empty()).collect();
                if !parts.is_empty() {
                    meta = meta.child(dim(parts.join(" · ")));
                }
            }
            "Task" | "Agent" => {
                let description = field("description");
                if !description.is_empty() {
                    meta = meta.child(dim(description));
                }
                if tool.child_total > 0 {
                    meta = meta.child(Badge::tonal(Tone::Secondary, format!("{} herramientas", tool.child_total)));
                }
            }
            "TodoWrite" => {
                let list = super::view::todos(tool);
                let done = list.iter().filter(|(_, s)| s == "completed").count();
                meta = meta.child(dim(format!("{done}/{}", list.len())));
            }
            _ => {}
        }
        // El objetivo lleva en su globito la ruta, el comando o la URL completos; si es
        // un archivo, un clic lo abre en el visor (sin desplegar la tarjeta).
        let file = target_file(tool);
        let has_target = !target.is_empty();
        let target_el = div()
            .id(SharedString::from(format!("{}-target", tool.id)))
            .flex_shrink()
            .min_w(px(0.))
            .truncate()
            .font_family(mono)
            .text_size(px(12.))
            .text_color(t.muted)
            .when_some(target_tip(tool), |el, tip| el.tooltip(crate::hover::tip_text(tip.into())))
            .when_some(file, |el, path| {
                el.cursor_pointer()
                    .hover(|el| el.text_color(t.accent))
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        view.open_doc(path.clone(), false, cx);
                    }))
            })
            .child(target);
        let header = div()
            .flex()
            .items_center()
            .gap(px(7.))
            .min_w(px(0.))
            .text_size(px(12.5))
            .child(div().size(px(16.)).flex_none().flex().items_center().justify_center().child(status))
            .child(div().flex_none().font_weight(FontWeight::SEMIBOLD).child(verb))
            .when(has_target, |el| el.child(target_el))
            .child(meta);

        // El detalle de cada herramienta.
        let id = tool.id.clone();
        let result = tool.result.clone().unwrap_or_default();
        let mut body = div().flex().flex_col().gap(px(8.));
        let mut shows_result = true;
        match tool.name.as_str() {
            "Read" => body = body.child(output(format!("{id}-r"), &result, 12, None, false)),
            "NotebookEdit" => {
                body = body.child(output(format!("{id}-nb"), &field("new_source"), 16, None, false));
                shows_result = false;
            }
            "Edit" | "MultiEdit" | "Write" => {
                body = body.child(super::view::tool_input(tool));
                shows_result = false;
            }
            "Bash" | "PowerShell" => {
                body = body.child(output(format!("{id}-c"), &field("command"), 8, Some("$"), false));
                if !result.is_empty() {
                    body = body.child(output(format!("{id}-r"), &result, 16, None, tool.is_error));
                }
            }
            "Grep" | "Glob" => body = body.child(output(format!("{id}-r"), &result, 20, None, tool.is_error)),
            "WebFetch" => {
                let prompt = field("prompt");
                body = body
                    .when(!prompt.is_empty(), |el| el.child(div().text_color(t.muted).child(prompt)))
                    .child(output(format!("{id}-r"), &result, 16, None, tool.is_error));
            }
            "WebSearch" => body = body.child(output(format!("{id}-r"), &result, 16, None, tool.is_error)),
            "Task" | "Agent" => body = body.child(super::view::markdown(&format!("{id}-md"), clean(&result), cx)),
            "TodoWrite" => {
                body = body.child(super::view::todo_rows(&super::view::todos(tool)));
                shows_result = false;
            }
            "ExitPlanMode" => {
                body = body.child(super::view::markdown(&format!("{id}-plan"), &field("plan"), cx));
                shows_result = false;
            }
            "AskUserQuestion" => body = body.when(!result.is_empty(), |el| el.child(div().text_color(t.muted).child(clean(&result).to_string()))),
            _ => {
                if !input.is_null() {
                    let json = serde_json::to_string_pretty(&input).unwrap_or_default();
                    body = body.child(output(format!("{id}-i"), &json, 16, None, false));
                }
                if !result.is_empty() {
                    body = body.child(output(format!("{id}-r"), &result, 16, None, tool.is_error));
                }
            }
        }
        if tool.is_error && !shows_result && !result.is_empty() {
            body = body.child(output(format!("{id}-e"), &result, 12, None, true));
        }

        // Las que abren solas se cierran al tocarlas; el resto, al revés.
        let open = if todo { true } else if open_by_default { !self.expanded.contains(&id) } else { self.expanded.contains(&id) };
        let toggle = id.clone();
        let flip = cx.listener(move |view, _: &bool, _, cx| {
            if !view.expanded.remove(&toggle) {
                view.expanded.insert(toggle.clone());
            }
            cx.notify();
        });
        let mut card = ExpandableCard::new(SharedString::from(format!("tool-{id}")))
            .open(open)
            .fixed(todo)
            .header(header)
            .on_toggle(move |open, window, cx| flip(&open, window, cx))
            .child(body);
        if tonal {
            card = card.tone(Tone::Secondary);
        }
        // Un subagente trabajando muestra sus últimas cinco herramientas bajo la cabecera.
        if matches!(tool.name.as_str(), "Task" | "Agent") && tool.result.is_none() && !tool.children.is_empty() {
            let skip = tool.children.len().saturating_sub(RECENT_CHILDREN);
            card = card.under(
                div().px(px(14.)).pb(px(10.)).flex().flex_wrap().gap(px(6.)).children(tool.children[skip..].iter().enumerate().map(|(n, name)| {
                    div()
                        .id(SharedString::from(format!("{id}-child-{n}")))
                        .px(px(8.))
                        .py(px(2.))
                        .rounded(px(8.))
                        .bg(t.hover)
                        .font_family(mono)
                        .text_size(px(11.))
                        .text_color(t.muted)
                        .child(name.clone())
                })),
            );
        }
        card.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn el_rango_de_read_cuenta_desde_el_offset_inclusive() {
        // Antes decía L10–60 para 50 líneas: la última es offset + limit - 1.
        assert_eq!(read_range(&json!({ "offset": 10, "limit": 50 })).as_deref(), Some("L10\u{2013}59"));
        // Solo `limit`: desde la primera.
        assert_eq!(read_range(&json!({ "limit": 100 })).as_deref(), Some("L1\u{2013}100"));
        assert_eq!(read_range(&json!({ "offset": 25 })).as_deref(), Some("L25+"));
        assert_eq!(read_range(&json!({ "offset": 0, "limit": 0 })), None);
        assert_eq!(read_range(&json!({ "file_path": "a.rs" })), None);
        // Números como texto o con decimales.
        assert_eq!(read_range(&json!({ "offset": "3", "limit": 2.0 })).as_deref(), Some("L3\u{2013}4"));
    }

    #[test]
    fn el_globito_y_el_clic_del_objetivo() {
        let tool = |name: &str, input: Value| ToolCall::new("t".into(), name.into(), Some(input));
        let read = tool("Read", json!({ "file_path": r"C:\repo\src\main.rs" }));
        assert_eq!(target_tip(&read).as_deref(), Some(r"C:\repo\src\main.rs"));
        assert_eq!(target_file(&read), Some(std::path::PathBuf::from(r"C:\repo\src\main.rs")));
        let bash = tool("Bash", json!({ "command": "cargo test", "description": "Corre los tests" }));
        assert_eq!(target_tip(&bash).as_deref(), Some("cargo test"));
        assert_eq!(target_file(&bash), None);
        let fetch = tool("WebFetch", json!({ "url": "https://example.com/a" }));
        assert_eq!(target_tip(&fetch).as_deref(), Some("https://example.com/a"));
        assert_eq!(target_file(&fetch), None);
        assert_eq!(target_tip(&tool("NotebookEdit", json!({ "notebook_path": "n.ipynb" }))).as_deref(), Some("n.ipynb"));
        assert_eq!(target_tip(&tool("Grep", json!({ "pattern": "x" }))), None);
    }
}
