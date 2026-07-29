//! Descubrimiento de modelos por backend, sin abrir una sesión completa.
//!
//! Cada agente informa sus modelos de otra forma: Codex por `model/list` en el
//! app-server, Claude Code no tiene listado y usa alias estables, Cursor y
//! OpenCode exponen subcomandos de CLI. Los resultados se cachean en memoria
//! cinco minutos para no spamear procesos al abrir el selector.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::model::{EffortOption, ModelInfo};

const CACHE_TTL: Duration = Duration::from_secs(5 * 60);

const ID_INITIALIZE: u64 = 1;
const ID_MODELS: u64 = 3;

/// Lo cacheado por backend: cuándo se pidió, y qué contestó.
type ModelCache = HashMap<String, (Instant, Vec<ModelInfo>)>;

static CACHE: Mutex<Option<ModelCache>> = Mutex::new(None);

/// Lista los modelos de un backend. Resultados cacheados 5 minutos.
pub fn list_models(backend: &str) -> Result<Vec<ModelInfo>, String> {
    if let Some(cached) = cache_get(backend) {
        return Ok(cached);
    }

    let models = match backend {
        "codex" => list_codex_models()?,
        "claude-code" => list_claude_models(),
        "cursor" => list_cursor_models()?,
        "opencode" => list_opencode_models()?,
        other => return Err(format!("backend desconocido: {other}")),
    };

    cache_put(backend, &models);
    Ok(models)
}

fn cache_get(backend: &str) -> Option<Vec<ModelInfo>> {
    let mut guard = CACHE.lock().ok()?;
    let map = guard.get_or_insert_with(HashMap::new);
    let (instant, models) = map.get(backend)?;
    if instant.elapsed() < CACHE_TTL {
        return Some(models.clone());
    }
    map.remove(backend);
    None
}

fn cache_put(backend: &str, models: &[ModelInfo]) {
    if let Ok(mut guard) = CACHE.lock() {
        guard
            .get_or_insert_with(HashMap::new)
            .insert(backend.to_string(), (Instant::now(), models.to_vec()));
    }
}

/// Traduce la respuesta JSON-RPC de `model/list` de Codex.
///
/// Se saltan los marcados `hidden`: el CLI los usa para variantes internas y
/// modelos retirados que siguen respondiendo.
pub(crate) fn parse_codex_model_list(v: &Value) -> Vec<ModelInfo> {
    let Some(list) = v.pointer("/result/data").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter(|m| !m.get("hidden").and_then(Value::as_bool).unwrap_or(false))
        .filter_map(|m| {
            let id = m
                .get("model")
                .or_else(|| m.get("id"))
                .and_then(Value::as_str)?
                .to_string();
            Some(ModelInfo {
                name: m
                    .get("displayName")
                    .and_then(Value::as_str)
                    .unwrap_or(&id)
                    .to_string(),
                description: m
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                efforts: m
                    .get("supportedReasoningEfforts")
                    .and_then(Value::as_array)
                    .map(|es| {
                        let mut list: Vec<EffortOption> = es
                            .iter()
                            .filter_map(|e| {
                                Some(EffortOption {
                                    id: e
                                        .get("reasoningEffort")
                                        .and_then(Value::as_str)?
                                        .to_string(),
                                    description: e
                                        .get("description")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default()
                                        .to_string(),
                                })
                            })
                            .collect();
                        list.sort_by_key(|e| effort_intensity_rank(&e.id));
                        list
                    })
                    .unwrap_or_default(),
                default_effort: m
                    .get("defaultReasoningEffort")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                supports_fast: false,
                id,
            })
        })
        .collect()
}

/// Alias estables de Claude Code. No hay `model/list` en el CLI; estos son
/// fallback documentado hasta que Anthropic exponga un listado.
pub(crate) fn claude_fallback_models() -> Vec<ModelInfo> {
    let efforts: Vec<EffortOption> = [
        ("low", "Contesta rápido. Para lo mecánico."),
        ("medium", "El equilibrio de siempre."),
        ("high", "Piensa antes. Para lo que tiene vueltas."),
        ("xhigh", "Se toma su tiempo. Problemas difíciles."),
        ("max", "Todo lo que puede. Lento y caro."),
    ]
    .into_iter()
    .map(|(id, description)| EffortOption {
        id: id.to_string(),
        description: description.to_string(),
    })
    .collect();

    [
        (
            "fable",
            "Fable 5",
            "El más capaz · 1M de contexto · gasta rápido",
        ),
        ("opus", "Opus 5", "Muy capaz · 1M de contexto"),
        ("sonnet", "Sonnet 5", "Equilibrado · 1M · más barato"),
        ("haiku", "Haiku 4.5", "El más rápido · 200K"),
    ]
    .into_iter()
    .map(|(id, name, description)| ModelInfo {
        id: id.to_string(),
        name: name.to_string(),
        description: description.to_string(),
        efforts: efforts.clone(),
        default_effort: Some("medium".to_string()),
        supports_fast: false,
    })
    .collect()
}

fn list_claude_models() -> Vec<ModelInfo> {
    tracing::debug!(
        "claude-code: sin listado CLI; usando alias fallback de claude_fallback_models"
    );
    claude_fallback_models()
}

fn list_codex_models() -> Result<Vec<ModelInfo>, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(codex_discover_inner());
    });
    rx.recv_timeout(Duration::from_secs(20))
        .map_err(|_| "Codex tardó más de 20 segundos en listar modelos".to_string())?
}

fn codex_discover_inner() -> Result<Vec<ModelInfo>, String> {
    let (program, prefix) = super::exe::launcher("codex")
        .ok_or_else(|| "no se encontró «codex» en el PATH".to_string())?;

    let mut cmd = Command::new(program);
    cmd.args(prefix)
        .arg("app-server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    no_window(&mut cmd);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("no se pudo iniciar Codex: {e}"))?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "Codex no expuso stdin".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Codex no expuso stdout".to_string())?;

    let init_msg = json!({
        "jsonrpc": "2.0",
        "id": ID_INITIALIZE,
        "method": "initialize",
        "params": { "clientInfo": { "name": "atic", "version": env!("CARGO_PKG_VERSION") } }
    });
    writeln!(stdin, "{init_msg}").map_err(|e| format!("no se pudo enviar a Codex: {e}"))?;
    stdin
        .flush()
        .map_err(|e| format!("no se pudo enviar a Codex: {e}"))?;

    let deadline = Instant::now() + Duration::from_secs(15);
    let mut reader = BufReader::new(stdout);

    read_until_response(&mut reader, ID_INITIALIZE, deadline)?;

    let list_msg = json!({
        "jsonrpc": "2.0",
        "id": ID_MODELS,
        "method": "model/list",
        "params": {},
    });
    writeln!(stdin, "{list_msg}").map_err(|e| format!("no se pudo enviar a Codex: {e}"))?;
    let _ = stdin.flush();

    let response = read_until_response(&mut reader, ID_MODELS, deadline)?;
    kill_child(&mut child);
    Ok(parse_codex_model_list(&response))
}

fn read_until_response(
    reader: &mut BufReader<impl Read>,
    target_id: u64,
    deadline: Instant,
) -> Result<Value, String> {
    let mut buf = String::new();
    while Instant::now() < deadline {
        buf.clear();
        match reader.read_line(&mut buf) {
            Ok(0) => return Err("Codex cerró stdout antes de responder".to_string()),
            Ok(_) => {
                let line = buf.trim();
                if line.is_empty() {
                    continue;
                }
                let Ok(v) = serde_json::from_str::<Value>(line) else {
                    continue;
                };
                if v.get("method").is_some() {
                    continue;
                }
                if v.get("id").and_then(Value::as_u64) != Some(target_id) {
                    continue;
                }
                if let Some(err) = v.get("error") {
                    let message = err
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("error del backend");
                    return Err(format!("Codex: {message}"));
                }
                return Ok(v);
            }
            Err(e) => return Err(format!("error leyendo Codex: {e}")),
        }
    }
    Err("Codex no respondió a tiempo".to_string())
}

fn list_cursor_models() -> Result<Vec<ModelInfo>, String> {
    // En Synara el listado por extensión ACP es preferible; acá el CLI es más
    // directo y no requiere levantar una sesión ACP completa.
    let stdout = run_cli_capture("cursor-agent", &["models"], 20)?;
    let models = parse_cursor_cli(&stdout);
    if models.is_empty() {
        return Err("cursor-agent models no devolvió modelos reconocibles".to_string());
    }
    Ok(models)
}

fn list_opencode_models() -> Result<Vec<ModelInfo>, String> {
    // Sin `--verbose` primero: la lista plana basta y evita un volcado enorme
    // que llenaba el pipe y congelaba la UI mientras el proceso no terminaba.
    let stdout = run_cli_capture("opencode", &["models"], 20)
        .or_else(|_| run_cli_capture("opencode", &["models", "--verbose"], 15))?;
    let models = parse_opencode_cli(&stdout);
    if models.is_empty() {
        return Err("opencode models no devolvió modelos reconocibles".to_string());
    }
    Ok(models)
}

fn run_cli_capture(program: &str, args: &[&str], timeout_secs: u64) -> Result<String, String> {
    use std::thread;

    let (program_path, prefix) = super::exe::launcher(program)
        .ok_or_else(|| format!("no se encontró «{program}» en el PATH"))?;

    let mut cmd = Command::new(&program_path);
    cmd.args(&prefix)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    no_window(&mut cmd);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("no se pudo ejecutar {program}: {e}"))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("sin stdout de {program}"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| format!("sin stderr de {program}"))?;

    // Leer en paralelo: si el buffer del pipe se llena y nadie lee, el hijo
    // se bloquea en write y nunca sale (y la UI esperaba encima).
    let stdout_h = thread::spawn(move || {
        let mut buf = String::new();
        let _ = BufReader::new(stdout).read_to_string(&mut buf);
        buf
    });
    let stderr_h = thread::spawn(move || {
        let mut buf = String::new();
        let _ = BufReader::new(stderr).read_to_string(&mut buf);
        buf
    });

    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = stdout_h.join();
                    let _ = stderr_h.join();
                    return Err(format!("«{program}» tardó más de {timeout_secs} segundos"));
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = stdout_h.join();
                let _ = stderr_h.join();
                return Err(format!("error esperando {program}: {e}"));
            }
        }
    };

    let out = stdout_h.join().unwrap_or_else(|_| String::new());
    let err = stderr_h.join().unwrap_or_else(|_| String::new());

    if out.is_empty() && !status.success() {
        return Err(format!("«{program}» falló: {}", err.trim()));
    }

    Ok(out)
}

fn no_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
}

fn kill_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Parsea la salida de `cursor-agent models`: líneas `slug - nombre`.
///
/// Cursor embebe el esfuerzo en el slug (`…-low`, `…-high-fast`). Se agrupan
/// bajo un solo modelo con `efforts` para que el selector no liste seis
/// variantes de lo mismo.
pub(crate) fn parse_cursor_cli(stdout: &str) -> Vec<ModelInfo> {
    let flat: Vec<ModelInfo> = stdout
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let lower = line.to_ascii_lowercase();
            if lower.starts_with("available")
                || lower.starts_with("model")
                || lower.starts_with("---")
                || lower.starts_with("slug")
            {
                return None;
            }
            let (slug, name) = line.split_once(" - ")?;
            let slug = slug.trim();
            let name = name.trim();
            if slug.is_empty() {
                return None;
            }
            let (id, display_name) = if slug == "default" {
                ("auto".to_string(), "Auto".to_string())
            } else {
                (slug.to_string(), name.to_string())
            };
            Some(ModelInfo {
                id,
                name: display_name,
                description: String::new(),
                efforts: Vec::new(),
                default_effort: None,
                supports_fast: false,
            })
        })
        .collect();
    group_effort_variants(flat)
}

/// Sufijos de nivel (sin `-fast`). Los más largos primero.
const LEVEL_SUFFIXES: &[&str] = &[
    "extra-high-thinking",
    "xhigh-thinking",
    "medium-thinking",
    "none-thinking",
    "low-thinking",
    "high-thinking",
    "max-thinking",
    "extra-high",
    "xhigh",
    "medium",
    "none",
    "low",
    "high",
    "max",
    "minimal",
];

/// Descripción larga del nivel (misma voz que Claude Code).
fn effort_description(level: &str) -> String {
    match level {
        "default" => "El equilibrio que trae el modelo.".into(),
        "none" => "Sin razonamiento extra. Lo más directo.".into(),
        "low" => "Contesta rápido. Para lo mecánico.".into(),
        "medium" => "El equilibrio de siempre.".into(),
        "high" => "Piensa antes. Para lo que tiene vueltas.".into(),
        "xhigh" => "Se toma su tiempo. Problemas difíciles.".into(),
        "max" => "Todo lo que puede. Lento y caro.".into(),
        "minimal" => "El mínimo de pensamiento. Respuestas cortas.".into(),
        "low-thinking" => "Thinking bajo. Rápido y con algo de razonamiento.".into(),
        "medium-thinking" => "Thinking medio. Equilibrio con razonamiento.".into(),
        "high-thinking" => "Thinking alto. Razona con calma.".into(),
        "xhigh-thinking" => "Thinking extra alto. Problemas difíciles.".into(),
        "max-thinking" => "Thinking al máximo. Lento y caro.".into(),
        "none-thinking" => "Thinking sin esfuerzo extra.".into(),
        other => format!("Nivel «{other}»."),
    }
}

/// Parte un slug Cursor en (base, nivel lógico, fast).
///
/// `extra-high` se normaliza a `xhigh`. Sin sufijo → nivel `default`.
pub fn split_cursor_wire(id: &str) -> (String, String, bool) {
    let (without_fast, fast) = if let Some(base) = id.strip_suffix("-fast") {
        if !base.is_empty() {
            (base, true)
        } else {
            (id, false)
        }
    } else {
        (id, false)
    };

    for suf in LEVEL_SUFFIXES {
        let needle = format!("-{suf}");
        if let Some(base) = without_fast.strip_suffix(&needle) {
            if !base.is_empty() {
                let level = if *suf == "extra-high" || *suf == "extra-high-thinking" {
                    if suf.ends_with("thinking") {
                        "xhigh-thinking"
                    } else {
                        "xhigh"
                    }
                } else {
                    *suf
                };
                return (base.to_string(), level.to_string(), fast);
            }
        }
    }

    if fast && without_fast != id {
        // Solo `-fast`, sin nivel.
        return (without_fast.to_string(), "default".to_string(), true);
    }
    (id.to_string(), "default".to_string(), false)
}

/// Compone el slug wire de Cursor a partir del grupo, nivel y fast.
///
/// Prueba las formas habituales (`xhigh` / `extra-high`) y, si se pasa la
/// lista de slugs conocidos del grupo, elige una que exista.
pub fn compose_cursor_wire(base: &str, effort: &str, fast: bool, known: &[String]) -> String {
    let level = if effort.is_empty() { "default" } else { effort };
    let candidates = wire_candidates(base, level, fast);
    if known.is_empty() {
        return candidates
            .into_iter()
            .next()
            .unwrap_or_else(|| base.to_string());
    }
    for c in &candidates {
        if known.iter().any(|k| k == c) {
            return c.clone();
        }
    }
    // Fast pedido pero no hay pareja: caer al no-fast.
    if fast {
        for c in wire_candidates(base, level, false) {
            if known.iter().any(|k| k == &c) {
                return c;
            }
        }
    }
    candidates
        .into_iter()
        .next()
        .unwrap_or_else(|| base.to_string())
}

fn wire_candidates(base: &str, level: &str, fast: bool) -> Vec<String> {
    let mut out = Vec::new();
    let suffixes: Vec<&str> = match level {
        "default" | "" => vec![""],
        "xhigh" => vec!["xhigh", "extra-high"],
        "xhigh-thinking" => vec!["xhigh-thinking", "extra-high-thinking"],
        other => vec![other],
    };
    for suf in suffixes {
        let stem = if suf.is_empty() {
            base.to_string()
        } else {
            format!("{base}-{suf}")
        };
        if fast {
            out.push(format!("{stem}-fast"));
        } else {
            out.push(stem);
        }
    }
    out
}

/// Nombre de grupo sin el calificativo de esfuerzo / fast.
fn strip_effort_from_name(name: &str) -> String {
    let mut out = name.to_string();
    let tails = [
        " Extra High Fast",
        " Extra High Thinking",
        " Medium Fast",
        " Medium Thinking",
        " High Fast",
        " High Thinking",
        " Low Fast",
        " Low Thinking",
        " None Fast",
        " None Thinking",
        " Max Fast",
        " Max Thinking",
        " Extra High",
        " Thinking",
        " Minimal",
        " Medium",
        " Fast",
        " High",
        " Low",
        " None",
        " Max",
    ];
    for t in tails {
        if let Some(stripped) = out.strip_suffix(t) {
            if !stripped.is_empty() {
                out = stripped.to_string();
                break;
            }
        }
    }
    out.trim().to_string()
}

/// Orden de intensidad: menor → mayor (para el picker).
fn effort_intensity_rank(level: &str) -> u8 {
    match level {
        "none" | "none-thinking" | "minimal" => 0,
        "low" | "low-thinking" => 1,
        "default" => 2,
        "medium" | "medium-thinking" => 3,
        "high" | "high-thinking" => 4,
        "xhigh" | "xhigh-thinking" => 5,
        "max" | "max-thinking" => 6,
        _ => 50,
    }
}

/// Preferencia del effort por defecto al abrir el modelo.
fn default_level_rank(level: &str) -> u8 {
    match level {
        "default" => 0,
        "medium" => 1,
        "high" => 2,
        "low" => 3,
        _ => 9,
    }
}

/// Agrupa variantes cuyo esfuerzo va en el id (Cursor).
///
/// Cada `EffortOption.id` es el **nivel lógico** (`low`, `high`, …). Fast es
/// un flag del modelo (`supports_fast`), no una fila del picker. Un grupo con
/// una sola variante sin sufijo queda sin `efforts`.
pub(crate) fn group_effort_variants(models: Vec<ModelInfo>) -> Vec<ModelInfo> {
    use std::collections::{BTreeMap, BTreeSet};

    // base → (wire_id, name, description)
    let mut groups: BTreeMap<String, Vec<(String, String, String)>> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();

    for m in models {
        let (base, _, _) = split_cursor_wire(&m.id);
        if !groups.contains_key(&base) {
            order.push(base.clone());
        }
        groups
            .entry(base)
            .or_default()
            .push((m.id, m.name, m.description));
    }

    order
        .into_iter()
        .filter_map(|base| {
            let variants = groups.remove(&base)?;
            if variants.len() == 1 {
                let (id, name, description) = &variants[0];
                let (_, level, fast) = split_cursor_wire(id);
                if level == "default" && !fast {
                    return Some(ModelInfo {
                        id: id.clone(),
                        name: name.clone(),
                        description: description.clone(),
                        efforts: Vec::new(),
                        default_effort: None,
                        supports_fast: false,
                    });
                }
            }

            let mut levels: BTreeSet<String> = BTreeSet::new();
            let mut supports_fast = false;
            let mut level_order: Vec<String> = Vec::new();

            for (id, _, _) in &variants {
                let (_, level, is_fast) = split_cursor_wire(id);
                if is_fast {
                    supports_fast = true;
                }
                if levels.insert(level.clone()) {
                    level_order.push(level);
                }
            }

            // Solo fast sin otros niveles → un effort "default" + switch.
            if level_order.is_empty() {
                level_order.push("default".into());
            }

            // Menor → mayor (BTreeSet ordenaba alfabético: High, Low, Medium).
            level_order.sort_by_key(|l| effort_intensity_rank(l));

            let efforts: Vec<EffortOption> = level_order
                .iter()
                .map(|level| EffortOption {
                    id: level.clone(),
                    description: effort_description(level),
                })
                .collect();

            let default_effort = {
                let mut best = level_order[0].clone();
                let mut best_rank = default_level_rank(&best);
                for level in &level_order {
                    let r = default_level_rank(level);
                    if r < best_rank {
                        best = level.clone();
                        best_rank = r;
                    }
                }
                best
            };

            let default_idx = variants
                .iter()
                .position(|(id, _, _)| {
                    let (_, level, fast) = split_cursor_wire(id);
                    level == default_effort && !fast
                })
                .or_else(|| {
                    variants.iter().position(|(id, _, _)| {
                        let (_, level, _) = split_cursor_wire(id);
                        level == default_effort
                    })
                })
                .unwrap_or(0);

            let group_name = {
                let raw = &variants[default_idx].1;
                let cleaned = strip_effort_from_name(raw);
                if cleaned.is_empty() {
                    base.clone()
                } else {
                    cleaned
                }
            };
            let description = variants[default_idx].2.clone();

            Some(ModelInfo {
                id: base,
                name: group_name,
                description,
                efforts,
                default_effort: Some(default_effort),
                supports_fast,
            })
        })
        .collect()
}

/// Precarga en background el catálogo de cada backend instalado.
///
/// Igual que Whisper: el primer clic en agentes no debería esperar al probe
/// del CLI. Los resultados viven en el cache de 5 minutos de `list_models`.
pub fn preload_models_async() {
    std::thread::spawn(|| {
        for backend in ["claude-code", "codex", "cursor", "opencode"] {
            match list_models(backend) {
                Ok(models) => {
                    tracing::info!(
                        backend,
                        count = models.len(),
                        "catálogo de modelos precargado"
                    );
                }
                Err(err) => {
                    tracing::debug!(backend, %err, "no se pudo precargar modelos");
                }
            }
        }
    });
}

/// Parsea la salida de `opencode models` (texto o JSONL).
pub(crate) fn parse_opencode_cli(stdout: &str) -> Vec<ModelInfo> {
    let mut models = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('{') {
            if let Ok(v) = serde_json::from_str::<Value>(line) {
                if let Some(m) = parse_opencode_json(&v) {
                    models.push(m);
                }
                continue;
            }
        }
        if let Some(m) = parse_opencode_text_line(line) {
            models.push(m);
        }
    }
    models
}

fn parse_opencode_json(v: &Value) -> Option<ModelInfo> {
    let id = v
        .get("id")
        .or_else(|| v.get("model"))
        .and_then(Value::as_str)?
        .to_string();
    let name = v
        .get("name")
        .or_else(|| v.get("displayName"))
        .and_then(Value::as_str)
        .unwrap_or(&id)
        .to_string();
    let description = v
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Some(ModelInfo {
        id,
        name,
        description,
        efforts: Vec::new(),
        default_effort: None,
        supports_fast: false,
    })
}

fn parse_opencode_text_line(line: &str) -> Option<ModelInfo> {
    let (slug, rest) = line.split_once(' ').unwrap_or((line, ""));
    let slug = slug.trim();
    if !slug.contains('/') {
        return None;
    }
    let description = rest.trim().to_string();
    Some(ModelInfo {
        id: slug.to_string(),
        name: slug.to_string(),
        description,
        efforts: Vec::new(),
        default_effort: None,
        supports_fast: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsea_cursor_cli() {
        let stdout = "\
composer-1 - Composer 1
default - Default Model
gpt-4o - GPT-4o
Available models:
";
        let models = parse_cursor_cli(stdout);
        assert_eq!(models.len(), 3);
        assert_eq!(models[0].id, "composer-1");
        assert_eq!(models[0].name, "Composer 1");
        assert_eq!(models[1].id, "auto");
        assert_eq!(models[1].name, "Auto");
        assert_eq!(models[2].id, "gpt-4o");
        assert!(models[0].efforts.is_empty());
    }

    #[test]
    fn agrupa_variantes_de_effort_de_cursor() {
        let stdout = "\
auto - Auto (default)
gpt-5.3-codex-low - Codex 5.3 Low
gpt-5.3-codex-low-fast - Codex 5.3 Low Fast
gpt-5.3-codex - Codex 5.3
gpt-5.3-codex-fast - Codex 5.3 Fast
gpt-5.3-codex-high - Codex 5.3 High
gpt-5.3-codex-high-fast - Codex 5.3 High Fast
cursor-grok-4.5-high - Cursor Grok 4.5
cursor-grok-4.5-high-fast - Cursor Grok 4.5 Fast
composer-2.5 - Composer 2.5
composer-2.5-fast - Composer 2.5 Fast
";
        let models = parse_cursor_cli(stdout);
        assert_eq!(models.len(), 4, "{models:?}");

        let auto = models.iter().find(|m| m.id == "auto").unwrap();
        assert!(auto.efforts.is_empty());
        assert!(!auto.supports_fast);

        let codex = models.iter().find(|m| m.id == "gpt-5.3-codex").unwrap();
        assert_eq!(codex.name, "Codex 5.3");
        assert!(codex.supports_fast);
        let levels: Vec<_> = codex.efforts.iter().map(|e| e.id.as_str()).collect();
        assert!(levels.contains(&"low"));
        assert!(levels.contains(&"high"));
        assert!(levels.contains(&"default"));
        assert!(!levels.iter().any(|l| l.contains("fast")));
        assert_eq!(codex.default_effort.as_deref(), Some("default"));
        assert!(codex
            .efforts
            .iter()
            .any(|e| e.id == "low" && e.description.contains("mecánico")));
        // Menor → mayor en el picker.
        let levels: Vec<_> = codex.efforts.iter().map(|e| e.id.as_str()).collect();
        let low_i = levels.iter().position(|l| *l == "low").unwrap();
        let high_i = levels.iter().position(|l| *l == "high").unwrap();
        assert!(low_i < high_i, "{levels:?}");

        let grok = models.iter().find(|m| m.id == "cursor-grok-4.5").unwrap();
        assert!(grok.supports_fast);
        assert_eq!(grok.efforts.len(), 1);
        assert_eq!(grok.efforts[0].id, "high");

        // Grok con low/medium/high debe salir Low → Medium → High.
        let grok_full = parse_cursor_cli(
            "\
cursor-grok-4.5-high - Cursor Grok 4.5
cursor-grok-4.5-low - Cursor Grok 4.5 Low
cursor-grok-4.5-medium - Cursor Grok 4.5 Medium
",
        );
        let g = grok_full
            .iter()
            .find(|m| m.id == "cursor-grok-4.5")
            .unwrap();
        assert_eq!(
            g.efforts.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            vec!["low", "medium", "high"]
        );

        let composer = models.iter().find(|m| m.id == "composer-2.5").unwrap();
        assert!(composer.supports_fast);
        assert_eq!(composer.default_effort.as_deref(), Some("default"));
    }

    #[test]
    fn compone_y_parte_slug_cursor() {
        let (base, level, fast) = split_cursor_wire("claude-opus-5-high-fast");
        assert_eq!(base, "claude-opus-5");
        assert_eq!(level, "high");
        assert!(fast);

        let wire = compose_cursor_wire("claude-opus-5", "high", true, &[]);
        assert_eq!(wire, "claude-opus-5-high-fast");

        let wire2 = compose_cursor_wire("gpt-5.3-codex", "default", false, &[]);
        assert_eq!(wire2, "gpt-5.3-codex");
    }

    #[test]
    fn parsea_opencode_cli_texto() {
        let stdout = "\
openai/gpt-4o
anthropic/claude-sonnet-4 El equilibrado
";
        let models = parse_opencode_cli(stdout);
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].id, "openai/gpt-4o");
        assert_eq!(models[0].name, "openai/gpt-4o");
        assert_eq!(models[1].id, "anthropic/claude-sonnet-4");
        assert_eq!(models[1].description, "El equilibrado");
    }

    #[test]
    fn parsea_opencode_cli_json() {
        let stdout = r#"{"id":"openai/gpt-4o","name":"GPT-4o","description":"Rápido"}"#;
        let models = parse_opencode_cli(stdout);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "openai/gpt-4o");
        assert_eq!(models[0].name, "GPT-4o");
        assert_eq!(models[0].description, "Rápido");
    }

    #[test]
    fn parsea_codex_model_list() {
        let v = json!({
            "jsonrpc": "2.0",
            "id": 3,
            "result": {
                "data": [
                    {
                        "id": "o3",
                        "model": "o3",
                        "displayName": "o3",
                        "description": "Razonamiento",
                        "hidden": false,
                        "supportedReasoningEfforts": [
                            { "reasoningEffort": "medium", "description": "Normal" }
                        ],
                        "defaultReasoningEffort": "medium"
                    },
                    {
                        "id": "old",
                        "model": "old",
                        "displayName": "Old",
                        "hidden": true
                    }
                ]
            }
        });
        let models = parse_codex_model_list(&v);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "o3");
        assert_eq!(models[0].name, "o3");
        assert_eq!(models[0].default_effort.as_deref(), Some("medium"));
        assert_eq!(models[0].efforts.len(), 1);
    }
}
