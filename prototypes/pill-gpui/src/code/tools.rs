//! Las tarjetas de herramienta de Expressive, como las de la referencia: un verbo, el
//! objetivo y su meta en la cabecera, y al abrir el detalle de cada una.

use gpui::{div, prelude::*, px, AnyElement, ClickEvent, Context, Div, FontWeight, SharedString};
use gpui_m3::{Badge, ExpandableCard, Icon, LoadingIndicator, Tone};
use serde_json::Value;

use super::chat::ToolCall;
use super::style::t;
use super::CodeView;

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

/// Líneas agregadas y quitadas de una edición (cuenta simple por líneas distintas).
fn edit_stats(tool: &ToolCall) -> Option<(usize, usize)> {
    let input = tool.input.as_ref()?;
    let field = |value: &Value, name: &str| value.get(name).and_then(Value::as_str).unwrap_or_default().to_string();
    let count = |old: &str, new: &str| {
        let diff = similar_lines(old, new);
        (diff.0, diff.1)
    };
    match tool.name.as_str() {
        "Edit" => Some(count(&field(input, "old_string"), &field(input, "new_string"))),
        "MultiEdit" => {
            let edits = input.get("edits").and_then(Value::as_array)?;
            Some(edits.iter().map(|e| count(&field(e, "old_string"), &field(e, "new_string"))).fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1)))
        }
        "Write" => Some((line_count(&field(input, "content")), 0)),
        _ => None,
    }
}

/// Cuántas líneas hay solo en `new` y solo en `old` (sin el contexto común del principio y del final).
fn similar_lines(old: &str, new: &str) -> (usize, usize) {
    let (a, b): (Vec<&str>, Vec<&str>) = (old.lines().collect(), new.lines().collect());
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..].iter().rev().zip(b[prefix..].iter().rev()).take_while(|(x, y)| x == y).count();
    (b.len() - prefix - suffix, a.len() - prefix - suffix)
}

impl CodeView {
    /// Un bloque mono recortable: muestra `max` líneas y «Mostrar todo (N líneas más)».
    fn output(&self, id: String, text: &str, max: usize, prefix: Option<&str>, error: bool, cx: &mut Context<Self>) -> Div {
        let t = t();
        let text = clean(text);
        let lines: Vec<&str> = text.lines().collect();
        let all = self.expanded.contains(&id);
        let shown = if all || lines.len() <= max { lines.join("\n") } else { lines[..max].join("\n") };
        let hidden = lines.len().saturating_sub(max);
        let toggle = id.clone();
        div()
            .rounded(px(12.))
            .bg(t.editor)
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(
                div()
                    .id(SharedString::from(format!("{id}-scroll")))
                    .max_h(px(420.))
                    .overflow_y_scroll()
                    .px(px(10.))
                    .py(px(7.))
                    .flex()
                    .gap(px(6.))
                    .font_family(gpui_m3::theme::MONO_FONT_FAMILY)
                    .text_size(px(11.5))
                    .line_height(px(17.))
                    .text_color(if error { t.bad } else { t.text })
                    .when_some(prefix, |el, prefix| el.child(div().flex_none().text_color(t.accent).child(prefix.to_string())))
                    .child(div().flex_1().min_w(px(0.)).child(shown)),
            )
            .when(hidden > 0 && !all, |el| {
                el.child(
                    div()
                        .id(SharedString::from(format!("{id}-all")))
                        .px(px(10.))
                        .py(px(4.))
                        .border_t_1()
                        .border_color(t.border)
                        .text_size(px(11.5))
                        .text_color(t.accent)
                        .cursor_pointer()
                        .hover(|el| el.bg(t.hover))
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                            view.expanded.insert(toggle.clone());
                            cx.notify();
                        }))
                        .child(format!("Mostrar todo ({hidden} líneas más)")),
                )
            })
    }

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
                let offset = input.get("offset").and_then(Value::as_u64);
                let limit = input.get("limit").and_then(Value::as_u64);
                if let Some(from) = offset {
                    meta = meta.child(dim(match limit {
                        Some(limit) => format!("L{from}–{}", from + limit),
                        None => format!("L{from}+"),
                    }));
                }
            }
            "Edit" | "MultiEdit" | "Write" => {
                if input.get("replace_all").and_then(Value::as_bool).unwrap_or(false) {
                    meta = meta.child(Badge::tonal(Tone::Secondary, "todas"));
                }
                if let Some(n) = input.get("edits").and_then(Value::as_array).map(Vec::len) {
                    meta = meta.child(Badge::tonal(Tone::Secondary, format!("{n} cambios")));
                }
                if let Some((added, removed)) = edit_stats(tool) {
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
            }
            "TodoWrite" => {
                let list = super::view::todos(tool);
                let done = list.iter().filter(|(_, s)| s == "completed").count();
                meta = meta.child(dim(format!("{done}/{}", list.len())));
            }
            _ => {}
        }
        let header = div()
            .flex()
            .items_center()
            .gap(px(7.))
            .min_w(px(0.))
            .text_size(px(12.5))
            .child(div().size(px(16.)).flex_none().flex().items_center().justify_center().child(status))
            .child(div().flex_none().font_weight(FontWeight::SEMIBOLD).child(verb))
            .when(!target.is_empty(), |el| {
                el.child(div().flex_shrink().min_w(px(0.)).truncate().font_family(mono).text_size(px(12.)).text_color(t.muted).child(target))
            })
            .child(meta);

        // El detalle de cada herramienta.
        let id = tool.id.clone();
        let result = tool.result.clone().unwrap_or_default();
        let mut body = div().flex().flex_col().gap(px(8.));
        let mut shows_result = true;
        match tool.name.as_str() {
            "Read" => body = body.child(self.output(format!("{id}-r"), &result, 12, None, false, cx)),
            "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => {
                body = body.child(super::view::tool_input(tool));
                shows_result = false;
            }
            "Bash" | "PowerShell" => {
                body = body.child(self.output(format!("{id}-c"), &field("command"), 8, Some("$"), false, cx));
                if !result.is_empty() {
                    body = body.child(self.output(format!("{id}-r"), &result, 16, None, tool.is_error, cx));
                }
            }
            "Grep" | "Glob" => body = body.child(self.output(format!("{id}-r"), &result, 20, None, tool.is_error, cx)),
            "WebFetch" => {
                let prompt = field("prompt");
                body = body
                    .when(!prompt.is_empty(), |el| el.child(div().text_color(t.muted).child(prompt)))
                    .child(self.output(format!("{id}-r"), &result, 16, None, tool.is_error, cx));
            }
            "WebSearch" => body = body.child(self.output(format!("{id}-r"), &result, 16, None, tool.is_error, cx)),
            "Task" | "Agent" => body = body.child(super::view::markdown(&format!("{id}-md"), clean(&result))),
            "TodoWrite" => {
                body = body.child(super::view::todo_rows(&super::view::todos(tool)));
                shows_result = false;
            }
            "ExitPlanMode" => {
                body = body.child(super::view::markdown(&format!("{id}-plan"), &field("plan")));
                shows_result = false;
            }
            "AskUserQuestion" => body = body.when(!result.is_empty(), |el| el.child(div().text_color(t.muted).child(clean(&result).to_string()))),
            _ => {
                if !input.is_null() {
                    let json = serde_json::to_string_pretty(&input).unwrap_or_default();
                    body = body.child(self.output(format!("{id}-i"), &json, 16, None, false, cx));
                }
                if !result.is_empty() {
                    body = body.child(self.output(format!("{id}-r"), &result, 16, None, tool.is_error, cx));
                }
            }
        }
        if tool.is_error && !shows_result && !result.is_empty() {
            body = body.child(self.output(format!("{id}-e"), &result, 12, None, true, cx));
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
        card.into_any_element()
    }
}
