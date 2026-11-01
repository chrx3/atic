//! Lo que un agente de IA puede hacerle al proyecto del editor de grabaciones
//! (`recording_editor/`), sin ventana: un lote de operaciones en JSON que se valida y se
//! aplica **todo o nada**, el resumen del proyecto (`describe`) y lo que pasó en pantalla
//! (`activity`). Contrato y catálogo en `docs/EDITOR_AGENTE.md`.
//!
//! Los tiempos son segundos del video original, los mismos del proyecto. Un valor fuera de
//! rango se **rechaza** con el rango válido en el mensaje (no se acota en silencio, para que
//! el agente aprenda); la única excepción es `set`, que reusa los límites del proyecto y avisa
//! de lo que acotó.

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::edit_project::{
    Blur, CameraStyle, CursorOpts, ExportSettings, Focus, Look, Project, Speed, Text, TextAt, Zoom,
};
use crate::screen_cursor::{Sample, Track};

/// Un tramo debe durar al menos esto; un zoom, un poco más (el mismo mínimo del editor).
const MIN_BLOCK: f64 = 0.1;
const MIN_ZOOM_LEN: f64 = 0.3;
/// Cuánto se perdona pasarse del final del video (el agente lee `duration` redondeado).
const EDGE_SLACK: f64 = 0.05;
pub const SCALE_RANGE: (f64, f64) = (1.2, 4.0);
pub const RATE_RANGE: (f64, f64) = (1.25, 16.0);
pub const TEXT_SIZE_RANGE: (f64, f64) = (0.4, 4.0);
/// Cuántos clics se listan en `activity` (el resto se resume).
const MAX_CLICKS: usize = 150;
/// Teclas a menos de esto son la misma racha, y una racha de verdad tiene al menos `TYPING_MIN`.
const TYPING_GAP: f64 = 1.2;
const TYPING_MIN: usize = 5;
/// Con menos nivel que esto el audio está en silencio.
const QUIET_LEVEL: f32 = 0.04;

/// Lo que las operaciones necesitan saber del editor.
pub struct Ctx<'a> {
    pub duration: f64,
    pub track: Option<&'a Track>,
    pub levels: &'a [f32],
    pub levels_per_sec: u32,
    /// El audio ya se leyó (con `levels` vacío, el video no tiene audio).
    pub levels_ready: bool,
    /// Lo mínimo que dura un tramo muerto para `*_silences`.
    pub idle_min: f64,
    /// Con qué rapidez va un tramo muerto acelerado.
    pub idle_rate: f64,
}

/// Lo que no cambia el proyecto y el editor hace aparte, en orden, tras aplicar el lote.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Seek(f64),
    Play,
    Pause,
    Undo,
    Redo,
    GenerateCaptions,
}

#[derive(Debug, Default)]
pub struct Outcome {
    /// Un resultado por operación (`{ "op": …, "index"?: …, … }`).
    pub results: Vec<Value>,
    pub effects: Vec<Effect>,
    pub warnings: Vec<String>,
}

#[derive(Debug, PartialEq)]
pub struct OpError {
    /// La operación (desde 0) que falló.
    pub index: usize,
    pub message: String,
}

impl OpError {
    pub fn text(&self) -> String {
        format!("operación {} (de 0): {}", self.index, self.message)
    }
}

// --- Números y campos -------------------------------------------------------------------

fn fmt(x: f64) -> String {
    let text = format!("{x:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn field_f64(o: &Map<String, Value>, key: &str) -> Result<Option<f64>, String> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_f64().filter(|n| n.is_finite()).map(Some).ok_or_else(|| format!("`{key}` debe ser un número")),
    }
}

fn need_f64(o: &Map<String, Value>, key: &str) -> Result<f64, String> {
    field_f64(o, key)?.ok_or_else(|| format!("falta `{key}`"))
}

fn field_bool(o: &Map<String, Value>, key: &str) -> Result<Option<bool>, String> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_bool().map(Some).ok_or_else(|| format!("`{key}` debe ser verdadero o falso")),
    }
}

fn field_str<'a>(o: &'a Map<String, Value>, key: &str) -> Result<Option<&'a str>, String> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_str().map(Some).ok_or_else(|| format!("`{key}` debe ser texto")),
    }
}

fn in_range(name: &str, value: f64, (min, max): (f64, f64)) -> Result<f64, String> {
    if value < min || value > max {
        Err(format!("`{name}` = {} está fuera de rango; va de {} a {}", fmt(value), fmt(min), fmt(max)))
    } else {
        Ok(value)
    }
}

/// `start` y `end` del bloque: dentro del video y con largo mínimo.
fn parse_span(o: &Map<String, Value>, duration: f64, min: f64) -> Result<(f64, f64), String> {
    let start = need_f64(o, "start")?;
    let end = need_f64(o, "end")?;
    if start < 0.0 {
        return Err(format!("`start` = {} no puede ser negativo", fmt(start)));
    }
    if end > duration + EDGE_SLACK {
        return Err(format!("`end` = {} pasa del final del video ({} s)", fmt(end), fmt(duration)));
    }
    let end = end.min(duration);
    if end - start < min {
        return Err(format!("el tramo {}–{} debe durar al menos {} s (y `start` < `end`)", fmt(start), fmt(end), fmt(min)));
    }
    Ok((start, end))
}

/// `#rrggbb` (o `rrggbb`, o `#rgb`) a 0xRRGGBB.
pub fn parse_hex(text: &str) -> Option<u32> {
    let digits = text.trim().trim_start_matches('#');
    let full = match digits.len() {
        3 => digits.chars().flat_map(|c| [c, c]).collect::<String>(),
        6 => digits.to_string(),
        _ => return None,
    };
    u32::from_str_radix(&full, 16).ok()
}

pub fn hex_of(color: u32) -> String {
    format!("#{:06x}", color & 0xffffff)
}

fn field_color(o: &Map<String, Value>, key: &str) -> Result<Option<u32>, String> {
    match o.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => parse_hex(s).map(Some).ok_or_else(|| format!("`{key}` = \"{s}\" no es un color; usa \"#rrggbb\"")),
        Some(Value::Number(n)) => n.as_u64().filter(|n| *n <= 0xffffff).map(|n| Some(n as u32)).ok_or_else(|| format!("`{key}` no es un color")),
        Some(_) => Err(format!("`{key}` debe ser un color \"#rrggbb\"")),
    }
}

// --- Los bloques como JSON amable (lo que ve y escribe el agente) -------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Cut,
    Zoom,
    Text,
    Speed,
    Blur,
}

impl Kind {
    fn parse(text: &str) -> Result<Self, String> {
        match text {
            "cut" => Ok(Kind::Cut),
            "zoom" => Ok(Kind::Zoom),
            "text" => Ok(Kind::Text),
            "speed" => Ok(Kind::Speed),
            "blur" => Ok(Kind::Blur),
            other => Err(format!("`kind` = \"{other}\" no existe; usa cut, zoom, text, speed o blur")),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Kind::Cut => "cut",
            Kind::Zoom => "zoom",
            Kind::Text => "text",
            Kind::Speed => "speed",
            Kind::Blur => "blur",
        }
    }
}

fn at_name(at: TextAt) -> &'static str {
    match at {
        TextAt::Top => "top",
        TextAt::Center => "center",
        TextAt::Bottom => "bottom",
    }
}

fn zoom_json(z: &Zoom) -> Value {
    let (follow, x, y) = match z.focus {
        Focus::Auto => (true, Value::Null, Value::Null),
        Focus::At(x, y) => (false, json!(x), json!(y)),
    };
    json!({ "start": z.start, "end": z.end, "scale": z.scale, "follow": follow, "x": x, "y": y, "auto": z.auto })
}

fn text_json(t: &Text) -> Value {
    json!({
        "start": t.start, "end": t.end, "text": t.text, "at": at_name(t.at), "size": t.size,
        "color": hex_of(t.color), "boxed": t.boxed, "bold": t.bold, "caption": t.caption,
    })
}

fn speed_json(s: &Speed) -> Value {
    json!({ "start": s.start, "end": s.end, "rate": s.rate })
}

fn blur_json(b: &Blur) -> Value {
    json!({ "start": b.start, "end": b.end, "rect": b.rect, "pixelate": b.pixelate })
}

fn cut_json((a, b): (f64, f64)) -> Value {
    json!({ "start": a.min(b), "end": a.max(b) })
}

fn parse_zoom(o: &Map<String, Value>, duration: f64) -> Result<Zoom, String> {
    let (start, end) = parse_span(o, duration, MIN_ZOOM_LEN)?;
    let scale = in_range("scale", field_f64(o, "scale")?.unwrap_or(2.0), SCALE_RANGE)?;
    let follow = field_bool(o, "follow")?.unwrap_or(false);
    let (x, y) = (field_f64(o, "x")?, field_f64(o, "y")?);
    let focus = match (follow, x, y) {
        (true, ..) | (false, None, None) => Focus::Auto,
        (false, Some(x), Some(y)) => Focus::At(in_range("x", x, (0.0, 1.0))?, in_range("y", y, (0.0, 1.0))?),
        _ => return Err("un punto fijo necesita `x` e `y` (0 a 1); sin ellos la cámara sigue al cursor".into()),
    };
    Ok(Zoom { start, end, scale, focus, auto: false })
}

fn parse_text(o: &Map<String, Value>, duration: f64) -> Result<Text, String> {
    let (start, end) = parse_span(o, duration, MIN_BLOCK)?;
    let text = field_str(o, "text")?.ok_or("falta `text`")?.trim().to_string();
    if text.is_empty() {
        return Err("`text` no puede estar vacío".into());
    }
    if text.chars().count() > 400 {
        return Err("`text` es demasiado largo (máximo 400 letras)".into());
    }
    let at = match field_str(o, "at")?.unwrap_or("bottom") {
        "top" => TextAt::Top,
        "center" => TextAt::Center,
        "bottom" => TextAt::Bottom,
        other => return Err(format!("`at` = \"{other}\" no existe; usa top, center o bottom")),
    };
    let mut out = Text::new(start, end, text, at);
    out.size = in_range("size", field_f64(o, "size")?.unwrap_or(1.0), TEXT_SIZE_RANGE)?;
    out.color = field_color(o, "color")?.unwrap_or(0xffffff);
    out.boxed = field_bool(o, "boxed")?.unwrap_or(true);
    out.bold = field_bool(o, "bold")?.unwrap_or(true);
    out.caption = field_bool(o, "caption")?.unwrap_or(false);
    Ok(out)
}

fn parse_speed(o: &Map<String, Value>, duration: f64) -> Result<Speed, String> {
    let (start, end) = parse_span(o, duration, MIN_BLOCK)?;
    let rate = in_range("rate", need_f64(o, "rate")?, RATE_RANGE)?;
    Ok(Speed { start, end, rate })
}

fn parse_blur(o: &Map<String, Value>, duration: f64) -> Result<Blur, String> {
    let (start, end) = parse_span(o, duration, MIN_BLOCK)?;
    let rect = o.get("rect").and_then(Value::as_array).ok_or("falta `rect`: [x, y, ancho, alto] de 0 a 1")?;
    let nums: Vec<f64> = rect.iter().filter_map(Value::as_f64).filter(|n| n.is_finite()).collect();
    let [x, y, w, h] = nums[..] else { return Err("`rect` debe ser [x, y, ancho, alto], cuatro números de 0 a 1".into()) };
    if rect.len() != 4 || x < 0.0 || y < 0.0 || w <= 0.0 || h <= 0.0 || x + w > 1.0 + 1e-6 || y + h > 1.0 + 1e-6 {
        return Err(format!("`rect` [{}, {}, {}, {}] se sale del cuadro; x, y ≥ 0, ancho y alto > 0 y x+ancho, y+alto ≤ 1", fmt(x), fmt(y), fmt(w), fmt(h)));
    }
    Ok(Blur { start, end, rect: [x, y, w, h], pixelate: field_bool(o, "pixelate")?.unwrap_or(false) })
}

fn parse_cut(o: &Map<String, Value>, duration: f64) -> Result<(f64, f64), String> {
    parse_span(o, duration, MIN_BLOCK)
}

fn overlaps(a: (f64, f64), b: (f64, f64)) -> bool {
    a.0 < b.1 - 1e-9 && b.0 < a.1 - 1e-9
}

fn no_index(kind: Kind, index: usize, len: usize) -> String {
    if len == 0 {
        format!("no hay ningún bloque `{}` (índice {index})", kind.name())
    } else {
        format!("no existe `{}` con índice {index}; hay {len} (0 a {})", kind.name(), len - 1)
    }
}

fn need_index(o: &Map<String, Value>) -> Result<usize, String> {
    o.get("index").and_then(Value::as_u64).map(|i| i as usize).ok_or_else(|| "falta `index` (un entero desde 0, el de `atic_editor_state`)".to_string())
}

/// Combina `set` con el JSON amable del bloque; un campo que no existe es un error que lista los válidos.
fn merge_known(target: &mut Value, set: &Map<String, Value>, readonly: &[&str]) -> Result<(), String> {
    let Some(map) = target.as_object_mut() else { return Ok(()) };
    for (key, value) in set {
        if readonly.contains(&key.as_str()) {
            continue;
        }
        if !map.contains_key(key) {
            let valid: Vec<&str> = map.keys().map(String::as_str).filter(|k| !readonly.contains(k)).collect();
            return Err(format!("`{key}` no es un campo de este bloque; válidos: {}", valid.join(", ")));
        }
        map.insert(key.clone(), value.clone());
    }
    Ok(())
}

// --- Aplicar ------------------------------------------------------------------------------

/// Aplica el lote a `project`. Todo o nada: si una operación falla, `project` queda como estaba.
pub fn apply(project: &mut Project, ops: &[Value], ctx: &Ctx) -> Result<Outcome, OpError> {
    if ops.is_empty() {
        return Err(OpError { index: 0, message: "`ops` está vacío".into() });
    }
    let mut work = project.clone();
    let mut outcome = Outcome::default();
    for (index, op) in ops.iter().enumerate() {
        let result = apply_one(&mut work, op, ctx, &mut outcome).map_err(|message| OpError { index, message })?;
        outcome.results.push(result);
    }
    *project = work;
    Ok(outcome)
}

/// Deja lo que cumple `keep` y dice cuántos quitó.
fn retain_count<T>(items: &mut Vec<T>, keep: impl Fn(&T) -> bool) -> usize {
    let before = items.len();
    items.retain(keep);
    before - items.len()
}

fn index_where<T>(items: &[T], found: impl Fn(&T) -> bool) -> Option<usize> {
    items.iter().position(found)
}

fn apply_one(work: &mut Project, op: &Value, ctx: &Ctx, outcome: &mut Outcome) -> Result<Value, String> {
    let o = op.as_object().ok_or("cada operación debe ser un objeto con `op`")?;
    let name = field_str(o, "op")?.ok_or("falta `op`")?;
    let d = ctx.duration;
    let done = |index: usize| Ok(json!({ "op": name, "index": index }));
    match name {
        "trim" => {
            let (start, end) = parse_span(o, d, MIN_BLOCK)?;
            work.trim = Some((start, end));
            Ok(json!({ "op": name, "start": start, "end": end }))
        }
        "clear_trim" => {
            work.trim = None;
            Ok(json!({ "op": name }))
        }
        "cut" => {
            let (start, end) = parse_cut(o, d)?;
            // Lo que se pisa con otro corte se junta con él.
            let (mut a, mut b) = (start, end);
            work.cuts.retain(|&(c, e)| {
                let (c, e) = (c.min(e), c.max(e));
                if overlaps((start, end), (c, e)) || c == end || e == start {
                    a = a.min(c);
                    b = b.max(e);
                    false
                } else {
                    true
                }
            });
            work.cuts.push((a, b));
            done(work.cuts.len() - 1)
        }
        "zoom" => {
            let zoom = parse_zoom(o, d)?;
            check_zoom_room(work, &zoom, None)?;
            let (start, end) = (zoom.start, zoom.end);
            work.zooms.push(zoom);
            work.tidy(d);
            done(index_where(&work.zooms, |z| !z.auto && z.start == start && z.end == end).unwrap_or(0))
        }
        "text" => {
            let text = parse_text(o, d)?;
            let (start, end) = (text.start, text.end);
            work.texts.push(text);
            work.texts.sort_by(|a, b| a.start.total_cmp(&b.start));
            done(index_where(&work.texts, |t| t.start == start && t.end == end && !t.caption).unwrap_or(0))
        }
        "speed" => {
            let speed = parse_speed(o, d)?;
            check_speed_room(work, &speed, None)?;
            let (start, end) = (speed.start, speed.end);
            work.speeds.push(speed);
            work.speeds.sort_by(|a, b| a.start.total_cmp(&b.start));
            done(index_where(&work.speeds, |s| s.start == start && s.end == end).unwrap_or(0))
        }
        "blur" => {
            let blur = parse_blur(o, d)?;
            work.blurs.push(blur);
            done(work.blurs.len() - 1)
        }
        "update" => update(work, o, ctx),
        "remove" => {
            let kind = Kind::parse(field_str(o, "kind")?.ok_or("falta `kind`")?)?;
            let index = need_index(o)?;
            let len = match kind {
                Kind::Cut => work.cuts.len(),
                Kind::Zoom => work.zooms.len(),
                Kind::Text => work.texts.len(),
                Kind::Speed => work.speeds.len(),
                Kind::Blur => work.blurs.len(),
            };
            if index >= len {
                return Err(no_index(kind, index, len));
            }
            match kind {
                Kind::Cut => drop(work.cuts.remove(index)),
                Kind::Zoom => drop(work.zooms.remove(index)),
                Kind::Text => drop(work.texts.remove(index)),
                Kind::Speed => drop(work.speeds.remove(index)),
                Kind::Blur => drop(work.blurs.remove(index)),
            }
            Ok(json!({ "op": name, "kind": kind.name(), "index": index }))
        }
        "remove_all" => {
            let kind = field_str(o, "kind")?.ok_or("falta `kind`")?;
            let removed = match kind {
                "caption" => retain_count(&mut work.texts, |t| !t.caption),
                "zoom" if field_bool(o, "auto_only")?.unwrap_or(false) => retain_count(&mut work.zooms, |z| !z.auto),
                other => match Kind::parse(other)? {
                    Kind::Cut => retain_count(&mut work.cuts, |_| false),
                    Kind::Zoom => retain_count(&mut work.zooms, |_| false),
                    Kind::Text => retain_count(&mut work.texts, |_| false),
                    Kind::Speed => retain_count(&mut work.speeds, |_| false),
                    Kind::Blur => retain_count(&mut work.blurs, |_| false),
                },
            };
            Ok(json!({ "op": name, "kind": kind, "removed": removed }))
        }
        "autofocus" => {
            let track = ctx.track.ok_or("esta grabación no tiene registro del mouse: no hay dónde apuntar el autofocus")?;
            work.autofocus_track(track, d);
            Ok(json!({ "op": name, "auto_zooms": work.zooms.iter().filter(|z| z.auto).count() }))
        }
        "speed_up_silences" | "cut_silences" => {
            if !ctx.levels_ready {
                return Err("el audio todavía se está leyendo; vuelve a intentarlo en unos segundos".into());
            }
            let min_len = field_f64(o, "min_len")?.unwrap_or(ctx.idle_min).max(0.5);
            let spans = work.suggest_idle(ctx.levels, ctx.levels_per_sec, ctx.track, d, min_len);
            let mut added = 0;
            for (a, b) in spans {
                if name == "cut_silences" {
                    let covered = work.cuts.iter().any(|&(c, e)| c.min(e) <= a + EDGE_SLACK && c.max(e) >= b - EDGE_SLACK);
                    if !covered {
                        work.cuts.push((a, b));
                        added += 1;
                    }
                } else {
                    let rate = field_f64(o, "rate")?.map(|r| in_range("rate", r, RATE_RANGE)).transpose()?.unwrap_or(ctx.idle_rate);
                    if work.add_speed(a, b, rate).is_some() {
                        added += 1;
                    }
                }
            }
            if added == 0 {
                outcome.warnings.push("no se encontraron tramos muertos (sin voz ni movimiento) tan largos".into());
            }
            Ok(json!({ "op": name, "added": added }))
        }
        "camera_style" => {
            let style = match field_str(o, "style")?.ok_or("falta `style`")?.to_ascii_lowercase().as_str() {
                "calm" => CameraStyle::Calm,
                "normal" => CameraStyle::Normal,
                "dynamic" => CameraStyle::Dynamic,
                other => return Err(format!("`style` = \"{other}\" no existe; usa Calm, Normal o Dynamic")),
            };
            work.apply_style(style);
            work.tidy(d);
            Ok(json!({ "op": name, "style": style }))
        }
        "set" => set(work, o, ctx, outcome),
        "generate_captions" => {
            outcome.effects.push(Effect::GenerateCaptions);
            Ok(json!({ "op": name, "note": "los subtítulos se generan en segundo plano; mira `state.captions`" }))
        }
        "seek" => {
            let time = in_range("time", need_f64(o, "time")?, (0.0, d))?;
            outcome.effects.push(Effect::Seek(time));
            Ok(json!({ "op": name, "time": time }))
        }
        "play" => {
            outcome.effects.push(Effect::Play);
            Ok(json!({ "op": name }))
        }
        "pause" => {
            outcome.effects.push(Effect::Pause);
            Ok(json!({ "op": name }))
        }
        "undo" => {
            outcome.effects.push(Effect::Undo);
            Ok(json!({ "op": name }))
        }
        "redo" => {
            outcome.effects.push(Effect::Redo);
            Ok(json!({ "op": name }))
        }
        other => Err(format!(
            "operación `{other}` desconocida; usa: trim, clear_trim, cut, zoom, text, speed, blur, update, remove, remove_all, autofocus, speed_up_silences, cut_silences, camera_style, set, generate_captions, seek, play, pause, undo, redo"
        )),
    }
}

/// Un zoom a mano no se pisa con otro a mano (los automáticos sí: `tidy` los recorta).
fn check_zoom_room(work: &Project, zoom: &Zoom, skip: Option<usize>) -> Result<(), String> {
    for (i, other) in work.zooms.iter().enumerate() {
        if Some(i) != skip && !other.auto && overlaps((zoom.start, zoom.end), (other.start, other.end)) {
            return Err(format!(
                "el zoom {}–{} se pisa con el zoom {i} ({}–{}); cambia ese con `update` o ponlo en otro tramo",
                fmt(zoom.start), fmt(zoom.end), fmt(other.start), fmt(other.end)
            ));
        }
    }
    Ok(())
}

fn check_speed_room(work: &Project, speed: &Speed, skip: Option<usize>) -> Result<(), String> {
    for (i, other) in work.speeds.iter().enumerate() {
        if Some(i) != skip && overlaps((speed.start, speed.end), (other.start, other.end)) {
            return Err(format!(
                "la velocidad {}–{} se pisa con la velocidad {i} ({}–{}); cambia esa con `update` o ponla en otro tramo",
                fmt(speed.start), fmt(speed.end), fmt(other.start), fmt(other.end)
            ));
        }
    }
    Ok(())
}

fn update(work: &mut Project, o: &Map<String, Value>, ctx: &Ctx) -> Result<Value, String> {
    let kind = Kind::parse(field_str(o, "kind")?.ok_or("falta `kind`")?)?;
    let index = need_index(o)?;
    let set = o.get("set").and_then(Value::as_object).ok_or("falta `set`: un objeto con los campos a cambiar")?;
    let d = ctx.duration;
    let out = |index: usize| Ok(json!({ "op": "update", "kind": kind.name(), "index": index }));
    match kind {
        Kind::Cut => {
            let cur = *work.cuts.get(index).ok_or_else(|| no_index(kind, index, work.cuts.len()))?;
            let mut friendly = cut_json(cur);
            merge_known(&mut friendly, set, &[])?;
            work.cuts[index] = parse_cut(friendly.as_object().unwrap(), d)?;
            out(index)
        }
        Kind::Zoom => {
            let cur = work.zooms.get(index).ok_or_else(|| no_index(kind, index, work.zooms.len()))?;
            let mut friendly = zoom_json(cur);
            // Dar `x`/`y` sin decir `follow` es pedir un punto fijo.
            if (set.contains_key("x") || set.contains_key("y")) && !set.contains_key("follow") {
                friendly["follow"] = json!(false);
            }
            merge_known(&mut friendly, set, &["auto"])?;
            let zoom = parse_zoom(friendly.as_object().unwrap(), d)?;
            check_zoom_room(work, &zoom, Some(index))?;
            let (start, end) = (zoom.start, zoom.end);
            work.zooms[index] = zoom;
            work.tidy(d);
            out(index_where(&work.zooms, |z| !z.auto && z.start == start && z.end == end).unwrap_or(index))
        }
        Kind::Text => {
            let cur = work.texts.get(index).ok_or_else(|| no_index(kind, index, work.texts.len()))?;
            let mut friendly = text_json(cur);
            merge_known(&mut friendly, set, &[])?;
            let text = parse_text(friendly.as_object().unwrap(), d)?;
            let (start, end) = (text.start, text.end);
            work.texts[index] = text;
            work.texts.sort_by(|a, b| a.start.total_cmp(&b.start));
            out(index_where(&work.texts, |t| t.start == start && t.end == end).unwrap_or(index))
        }
        Kind::Speed => {
            let cur = work.speeds.get(index).ok_or_else(|| no_index(kind, index, work.speeds.len()))?;
            let mut friendly = speed_json(cur);
            merge_known(&mut friendly, set, &[])?;
            let speed = parse_speed(friendly.as_object().unwrap(), d)?;
            check_speed_room(work, &speed, Some(index))?;
            let (start, end) = (speed.start, speed.end);
            work.speeds[index] = speed;
            work.speeds.sort_by(|a, b| a.start.total_cmp(&b.start));
            out(index_where(&work.speeds, |s| s.start == start && s.end == end).unwrap_or(index))
        }
        Kind::Blur => {
            let cur = work.blurs.get(index).ok_or_else(|| no_index(kind, index, work.blurs.len()))?;
            let mut friendly = blur_json(cur);
            merge_known(&mut friendly, set, &[])?;
            work.blurs[index] = parse_blur(friendly.as_object().unwrap(), d)?;
            out(index)
        }
    }
}

// --- `set`: look, cursor, camera_opts, motion, export -----------------------------------

/// Todo "#rrggbb" del parche pasa a número (los colores del proyecto son 0xRRGGBB).
fn hexes_to_ints(value: &mut Value) {
    match value {
        Value::String(s) if s.starts_with('#') => {
            if let Some(n) = parse_hex(s) {
                *value = json!(n);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(hexes_to_ints),
        Value::Object(map) => map.values_mut().for_each(hexes_to_ints),
        _ => {}
    }
}

/// Combina el parche con el JSON de la sección. Un campo desconocido es un error con los válidos.
fn merge_section(section: &str, target: &mut Value, patch: &Value) -> Result<(), String> {
    let (Some(map), Some(patch)) = (target.as_object_mut(), patch.as_object()) else {
        return Err(format!("`{section}` debe ser un objeto"));
    };
    for (key, value) in patch {
        let Some(slot) = map.get_mut(key) else {
            let valid: Vec<&str> = map.keys().map(String::as_str).collect();
            return Err(format!("`{section}.{key}` no existe; válidos: {}", valid.join(", ")));
        };
        if slot.is_object() && value.is_object() && !matches!(key.as_str(), "backdrop") {
            merge_section(&format!("{section}.{key}"), slot, value)?;
        } else {
            *slot = value.clone();
        }
    }
    Ok(())
}

/// Los números que cambiaron al acotar: «look.padding: 0.9 → 0.3».
fn clamp_warnings(section: &str, patch: &Value, after: &Value, out: &mut Vec<String>) {
    match (patch, after) {
        (Value::Object(p), Value::Object(a)) => {
            for (key, value) in p {
                if let Some(av) = a.get(key) {
                    clamp_warnings(&format!("{section}.{key}"), value, av, out);
                }
            }
        }
        (Value::Number(p), Value::Number(a)) => {
            let (p, a) = (p.as_f64().unwrap_or(0.0), a.as_f64().unwrap_or(0.0));
            if (p - a).abs() > 1e-9 {
                out.push(format!("{section}: {} se acotó a {}", fmt(p), fmt(a)));
            }
        }
        _ => {}
    }
}

fn patch_section<T: Serialize + DeserializeOwned>(name: &str, current: &T, patch: &Value, warnings: &mut Vec<String>) -> Result<T, String> {
    let mut value = serde_json::to_value(current).map_err(|e| e.to_string())?;
    let mut patch = patch.clone();
    hexes_to_ints(&mut patch);
    merge_section(name, &mut value, &patch)?;
    let next: T = serde_json::from_value(value).map_err(|e| format!("`{name}` no es válido: {e}"))?;
    // Lo que el proyecto acotó (`clamped`) se compara contra lo pedido.
    let after = serde_json::to_value(&next).map_err(|e| e.to_string())?;
    clamp_warnings(name, &patch, &after, warnings);
    Ok(next)
}

fn set(work: &mut Project, o: &Map<String, Value>, ctx: &Ctx, outcome: &mut Outcome) -> Result<Value, String> {
    let mut touched = Vec::new();
    for key in o.keys().filter(|k| k.as_str() != "op") {
        let patch = &o[key];
        match key.as_str() {
            "look" => {
                // Un `Look` acotado sale de `from(LookRaw)` al leerlo.
                let next: Look = patch_section("look", &work.look, patch, &mut outcome.warnings)?;
                work.look = next.clamped();
            }
            "cursor" => {
                let next: CursorOpts = patch_section("cursor", &work.cursor, patch, &mut outcome.warnings)?;
                work.cursor = next.clamped();
            }
            "camera_opts" => {
                let next = patch_section("camera_opts", &work.camera_opts, patch, &mut outcome.warnings)?;
                work.camera_opts = next.clamped();
                work.tidy(ctx.duration);
            }
            "motion" => {
                let next = patch_section("motion", &work.motion, patch, &mut outcome.warnings)?;
                work.motion = next.clamped();
            }
            "export" => {
                let next: ExportSettings = patch_section("export", &work.export, patch, &mut outcome.warnings)?;
                work.export = next;
            }
            "backdrop_image" => {
                work.backdrop_image = match patch {
                    Value::Null => None,
                    Value::String(s) if s.starts_with("wallpaper:") => Some(s.into()),
                    Value::String(s) if std::path::Path::new(s).is_file() => Some(s.into()),
                    Value::String(s) => return Err(format!("`backdrop_image` = \"{s}\": el archivo no existe (o usa \"wallpaper:NN\")")),
                    _ => return Err("`backdrop_image` debe ser una ruta, \"wallpaper:NN\" o null".into()),
                };
            }
            other => {
                return Err(format!("`set.{other}` no existe; usa look, cursor, camera_opts, motion, export o backdrop_image"));
            }
        }
        touched.push(key.clone());
    }
    if touched.is_empty() {
        return Err("`set` no trae nada que cambiar (look, cursor, camera_opts, motion, export, backdrop_image)".into());
    }
    Ok(json!({ "op": "set", "changed": touched }))
}

// --- El estado ------------------------------------------------------------------------------

/// El JSON de una sección con sus colores como "#rrggbb".
fn colored<T: Serialize>(section: &T) -> Value {
    let mut value = serde_json::to_value(section).unwrap_or(Value::Null);
    ints_to_hexes(&mut value);
    value
}

fn ints_to_hexes(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, v) in map.iter_mut() {
                if key == "click_color" {
                    if let Some(n) = v.as_u64() {
                        *v = json!(hex_of(n as u32));
                    }
                } else if key == "Solid" {
                    if let Some(n) = v.as_u64() {
                        *v = json!(hex_of(n as u32));
                    }
                } else if key == "Gradient" {
                    if let Some(items) = v.as_array_mut() {
                        for item in items {
                            if let Some(n) = item.as_u64() {
                                *item = json!(hex_of(n as u32));
                            }
                        }
                    }
                } else {
                    ints_to_hexes(v);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(ints_to_hexes),
        _ => {}
    }
}

fn indexed<T>(items: &[T], json: impl Fn(&T) -> Value) -> Vec<Value> {
    items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let mut v = json(item);
            v["index"] = json!(i);
            v
        })
        .collect()
}

/// El proyecto como lo ve el agente. El editor le suma lo suyo (cabezal, selección,
/// exportación, transcripción, deshacer).
pub fn describe(project: &Project, ctx: &Ctx) -> Value {
    let d = ctx.duration;
    let texts: Vec<(usize, &Text)> = project.texts.iter().enumerate().filter(|(_, t)| !t.caption).collect();
    let captions: Vec<&Text> = project.texts.iter().filter(|t| t.caption).collect();
    let mut state = json!({
        "duration": d,
        "result": { "length": project.out_length(d), "segments": project.segments(d) },
        "trim": project.trim.map(|(a, b)| json!({ "start": a, "end": b })),
        "cuts": indexed(&project.cuts, |c| cut_json(*c)),
        "zooms": indexed(&project.zooms, zoom_json),
        "texts": texts.iter().map(|(i, t)| { let mut v = text_json(t); v["index"] = json!(i); v }).collect::<Vec<_>>(),
        "speeds": indexed(&project.speeds, speed_json),
        "blurs": indexed(&project.blurs, blur_json),
        "look": colored(&project.look),
        "cursor": colored(&project.cursor),
        "camera_style": project.active_style(),
        "camera_opts": colored(&project.camera_opts),
        "motion": colored(&project.motion),
        "export": colored(&project.export),
        "backdrop_image": project.backdrop_image,
        "camera_styles": ["Calm", "Normal", "Dynamic"],
        "has_cursor_track": ctx.track.is_some(),
        "audio": { "ready": ctx.levels_ready, "has_audio": !ctx.levels.is_empty() },
        "limits": {
            "zoom.scale": [SCALE_RANGE.0, SCALE_RANGE.1],
            "speed.rate": [RATE_RANGE.0, RATE_RANGE.1],
            "text.size": [TEXT_SIZE_RANGE.0, TEXT_SIZE_RANGE.1],
            "look.padding": [0.0, 0.3], "look.radius": [0.0, 0.1], "look.shadow_amount": [0.0, 1.0],
            "cursor.size": [0.5, 4.0], "cursor.click_size": [0.5, 2.5], "cursor.smooth_level": [0.0, 1.0], "cursor.hide_delay": [1.0, 6.0],
            "camera_opts.depth": [1.2, 4.0], "motion.smoothing": [0.5, 2.0], "motion.blur": [0.0, 2.0],
        },
    });
    if !captions.is_empty() {
        state["captions"] = json!({
            "count": captions.len(),
            "first": captions.first().map(|c| c.start),
            "last": captions.last().map(|c| c.end),
            "note": "los subtítulos no se listan; para cambiarlos usa `remove_all` con kind \"caption\" o `generate_captions`",
        });
    }
    state
}

// --- Lo que pasó en pantalla -----------------------------------------------------------------

/// Clics, rachas de escritura, tramos quietos y silencios, para decidir dónde acercar o cortar
/// sin mirar todo el video.
pub fn activity(project: &Project, ctx: &Ctx) -> Value {
    let d = ctx.duration;
    let mut out = json!({ "duration": d });
    if let Some(track) = ctx.track {
        let mut clicks = Vec::new();
        let mut down = false;
        for Sample(ms, x, y, pressed) in &track.samples {
            if *pressed && !down {
                clicks.push(json!({ "t": (*ms as f64 / 100.0).round() / 10.0, "x": (*x as f64 * 100.0).round() / 100.0, "y": (*y as f64 * 100.0).round() / 100.0 }));
            }
            down = *pressed;
        }
        out["clicks_total"] = json!(clicks.len());
        // Muchos clics: se reparten parejo en vez de cortar el final.
        if clicks.len() > MAX_CLICKS {
            let step = clicks.len() as f64 / MAX_CLICKS as f64;
            clicks = (0..MAX_CLICKS).map(|i| clicks[(i as f64 * step) as usize].clone()).collect();
        }
        out["clicks"] = json!(clicks);
        out["typing"] = json!(typing_bursts(&track.keys));
    } else {
        out["note"] = json!("esta grabación no tiene registro del mouse ni del teclado");
    }
    if ctx.levels_ready || ctx.track.is_some() {
        let quiet = project.suggest_idle(ctx.levels, ctx.levels_per_sec, ctx.track, d, 1.0);
        out["quiet"] = json!(quiet.iter().map(|(a, b)| json!({ "start": a, "end": b })).collect::<Vec<_>>());
    }
    if ctx.levels_ready && !ctx.levels.is_empty() && ctx.levels_per_sec > 0 {
        let rate = ctx.levels_per_sec as f64;
        out["audio"] = json!({
            "silent_spans": silent_spans(ctx.levels, rate, 1.0),
            "loud_moments": loud_moments(ctx.levels, rate, 5),
            "all_silent": ctx.levels.iter().all(|l| *l < QUIET_LEVEL),
        });
    } else if ctx.levels_ready {
        out["audio"] = json!({ "note": "el video no tiene audio" });
    } else {
        out["audio"] = json!({ "note": "el audio todavía se está leyendo; vuelve a pedirlo en unos segundos" });
    }
    out
}

/// Rachas de teclas: `{ start, end, keys }`.
fn typing_bursts(keys_ms: &[u32]) -> Vec<Value> {
    let mut bursts: Vec<(f64, f64, usize)> = Vec::new();
    for ms in keys_ms {
        let t = *ms as f64 / 1000.0;
        match bursts.last_mut() {
            Some(last) if t - last.1 <= TYPING_GAP => {
                last.1 = t;
                last.2 += 1;
            }
            _ => bursts.push((t, t, 1)),
        }
    }
    bursts
        .into_iter()
        .filter(|b| b.2 >= TYPING_MIN)
        .map(|(a, b, n)| json!({ "start": (a * 10.0).round() / 10.0, "end": (b * 10.0).round() / 10.0, "keys": n }))
        .collect()
}

fn silent_spans(levels: &[f32], rate: f64, min_len: f64) -> Vec<Value> {
    let mut out = Vec::new();
    let mut from: Option<usize> = None;
    for (i, level) in levels.iter().chain(std::iter::once(&1.0)).enumerate() {
        match (*level < QUIET_LEVEL, from) {
            (true, None) => from = Some(i),
            (false, Some(start)) => {
                let (a, b) = (start as f64 / rate, i as f64 / rate);
                if b - a >= min_len {
                    out.push(json!({ "start": (a * 10.0).round() / 10.0, "end": (b * 10.0).round() / 10.0 }));
                }
                from = None;
            }
            _ => {}
        }
    }
    out
}

/// Los `count` segundos más fuertes (no pegados entre sí).
fn loud_moments(levels: &[f32], rate: f64, count: usize) -> Vec<Value> {
    let per = rate as usize;
    let mut seconds: Vec<(usize, f32)> = levels.chunks(per.max(1)).enumerate().map(|(i, c)| (i, c.iter().copied().fold(0.0, f32::max))).collect();
    seconds.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut picked: Vec<usize> = Vec::new();
    for (second, level) in seconds {
        if level < QUIET_LEVEL || picked.len() >= count {
            break;
        }
        if picked.iter().all(|p| p.abs_diff(second) > 2) {
            picked.push(second);
        }
    }
    picked.sort_unstable();
    picked.into_iter().map(|s| json!({ "t": s, "level": (levels[(s * per).min(levels.len() - 1)..].iter().take(per).copied().fold(0.0f32, f32::max) * 100.0).round() / 100.0 })).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit_project::Backdrop;

    fn ctx(duration: f64) -> Ctx<'static> {
        Ctx { duration, track: None, levels: &[], levels_per_sec: 20, levels_ready: true, idle_min: 2.0, idle_rate: 4.0 }
    }

    fn run(project: &mut Project, ops: Value) -> Result<Outcome, OpError> {
        apply(project, ops.as_array().unwrap(), &ctx(60.0))
    }

    #[test]
    fn un_lote_agrega_de_todo_y_devuelve_los_indices() {
        let mut p = Project::default();
        let out = run(
            &mut p,
            json!([
                { "op": "trim", "start": 2, "end": 50 },
                { "op": "cut", "start": 10, "end": 12 },
                { "op": "zoom", "start": 20, "end": 24, "scale": 2.5, "x": 0.3, "y": 0.6 },
                { "op": "text", "start": 5, "end": 8, "text": "Hola", "at": "top", "color": "#ff8800" },
                { "op": "speed", "start": 30, "end": 40, "rate": 4 },
                { "op": "blur", "start": 1, "end": 3, "rect": [0.1, 0.1, 0.3, 0.2], "pixelate": true },
            ]),
        )
        .unwrap();
        assert_eq!(out.results.len(), 6);
        assert_eq!(p.trim, Some((2.0, 50.0)));
        assert_eq!(p.cuts, vec![(10.0, 12.0)]);
        assert_eq!(p.zooms[0].focus, Focus::At(0.3, 0.6));
        assert_eq!(p.zooms[0].scale, 2.5);
        assert_eq!(p.texts[0].color, 0xff8800);
        assert_eq!(p.texts[0].at, TextAt::Top);
        assert_eq!(p.speeds[0].rate, 4.0);
        assert!(p.blurs[0].pixelate);
        assert_eq!(out.results[2]["index"], 0);
    }

    #[test]
    fn si_una_operacion_falla_no_se_aplica_ninguna() {
        let mut p = Project::default();
        let before = p.clone();
        let error = run(&mut p, json!([{ "op": "cut", "start": 1, "end": 3 }, { "op": "zoom", "start": 5, "end": 9, "scale": 9 }])).unwrap_err();
        assert_eq!(error.index, 1);
        assert!(error.message.contains("fuera de rango"), "{}", error.message);
        assert!(error.message.contains("1.2") && error.message.contains('4'));
        assert_eq!(p, before);
    }

    #[test]
    fn los_tiempos_se_validan_con_el_rango() {
        let mut p = Project::default();
        let e = run(&mut p, json!([{ "op": "cut", "start": 5, "end": 70 }])).unwrap_err();
        assert!(e.message.contains("pasa del final") && e.message.contains("60"), "{}", e.message);
        let e = run(&mut p, json!([{ "op": "cut", "start": -1, "end": 3 }])).unwrap_err();
        assert!(e.message.contains("negativo"));
        let e = run(&mut p, json!([{ "op": "cut", "start": 8, "end": 8 }])).unwrap_err();
        assert!(e.message.contains("al menos"));
        // Pasarse unos centésimos del final se perdona y se ajusta.
        run(&mut p, json!([{ "op": "cut", "start": 50, "end": 60.04 }])).unwrap();
        assert_eq!(p.cuts, vec![(50.0, 60.0)]);
    }

    #[test]
    fn dos_cortes_que_se_pisan_se_juntan() {
        let mut p = Project::default();
        run(&mut p, json!([{ "op": "cut", "start": 10, "end": 14 }, { "op": "cut", "start": 12, "end": 20 }, { "op": "cut", "start": 20, "end": 22 }])).unwrap();
        assert_eq!(p.cuts, vec![(10.0, 22.0)]);
    }

    #[test]
    fn un_zoom_a_mano_no_se_pisa_con_otro() {
        let mut p = Project::default();
        run(&mut p, json!([{ "op": "zoom", "start": 10, "end": 20 }])).unwrap();
        let e = run(&mut p, json!([{ "op": "zoom", "start": 15, "end": 25 }])).unwrap_err();
        assert!(e.message.contains("se pisa con el zoom 0"), "{}", e.message);
        // Sin x/y la cámara sigue al cursor.
        assert_eq!(p.zooms[0].focus, Focus::Auto);
    }

    #[test]
    fn la_velocidad_valida_el_ritmo_y_que_no_se_pise() {
        let mut p = Project::default();
        assert!(run(&mut p, json!([{ "op": "speed", "start": 1, "end": 5, "rate": 40 }])).is_err());
        run(&mut p, json!([{ "op": "speed", "start": 1, "end": 5, "rate": 2 }])).unwrap();
        let e = run(&mut p, json!([{ "op": "speed", "start": 4, "end": 8, "rate": 2 }])).unwrap_err();
        assert!(e.message.contains("se pisa"));
    }

    #[test]
    fn update_cambia_campos_y_rechaza_los_que_no_existen() {
        let mut p = Project::default();
        run(&mut p, json!([{ "op": "text", "start": 1, "end": 4, "text": "uno" }])).unwrap();
        run(&mut p, json!([{ "op": "update", "kind": "text", "index": 0, "set": { "text": "dos", "end": 6, "color": "#00ff00" } }])).unwrap();
        assert_eq!((p.texts[0].text.as_str(), p.texts[0].end, p.texts[0].color), ("dos", 6.0, 0x00ff00));
        let e = run(&mut p, json!([{ "op": "update", "kind": "text", "index": 0, "set": { "grosor": 3 } }])).unwrap_err();
        assert!(e.message.contains("grosor") && e.message.contains("válidos"), "{}", e.message);
        let e = run(&mut p, json!([{ "op": "update", "kind": "text", "index": 4, "set": { "text": "x" } }])).unwrap_err();
        assert!(e.message.contains("hay 1"), "{}", e.message);
    }

    #[test]
    fn update_de_un_zoom_pasa_de_seguir_al_cursor_a_un_punto_fijo_y_vuelve() {
        let mut p = Project::default();
        run(&mut p, json!([{ "op": "zoom", "start": 10, "end": 14 }])).unwrap();
        run(&mut p, json!([{ "op": "update", "kind": "zoom", "index": 0, "set": { "x": 0.2, "y": 0.8 } }])).unwrap();
        assert_eq!(p.zooms[0].focus, Focus::At(0.2, 0.8));
        run(&mut p, json!([{ "op": "update", "kind": "zoom", "index": 0, "set": { "follow": true } }])).unwrap();
        assert_eq!(p.zooms[0].focus, Focus::Auto);
    }

    #[test]
    fn remove_y_remove_all() {
        let mut p = Project::default();
        run(&mut p, json!([{ "op": "cut", "start": 1, "end": 2 }, { "op": "cut", "start": 5, "end": 6 }, { "op": "text", "start": 1, "end": 2, "text": "a" }, { "op": "text", "start": 3, "end": 4, "text": "sub", "caption": true }])).unwrap();
        run(&mut p, json!([{ "op": "remove", "kind": "cut", "index": 0 }])).unwrap();
        assert_eq!(p.cuts, vec![(5.0, 6.0)]);
        assert!(run(&mut p, json!([{ "op": "remove", "kind": "cut", "index": 3 }])).is_err());
        let out = run(&mut p, json!([{ "op": "remove_all", "kind": "caption" }])).unwrap();
        assert_eq!(out.results[0]["removed"], 1);
        assert_eq!(p.texts.len(), 1);
        run(&mut p, json!([{ "op": "remove_all", "kind": "cut" }])).unwrap();
        assert!(p.cuts.is_empty());
    }

    #[test]
    fn set_cambia_secciones_acota_y_avisa() {
        let mut p = Project::default();
        let out = run(&mut p, json!([{ "op": "set", "look": { "padding": 0.9, "backdrop": { "Gradient": ["#ff7e5f", "#feb47b"] } }, "cursor": { "size": 2.0 }, "export": { "aspect": "Vertical" } }])).unwrap();
        assert_eq!(p.look.padding, 0.3);
        assert_eq!(p.look.backdrop, Backdrop::Gradient(0xff7e5f, 0xfeb47b));
        assert_eq!(p.cursor.size, 2.0);
        assert_eq!(p.export.aspect, crate::edit_project::Aspect::Vertical);
        assert!(out.warnings.iter().any(|w| w.contains("look.padding")), "{:?}", out.warnings);
    }

    #[test]
    fn set_rechaza_campos_y_secciones_desconocidos() {
        let mut p = Project::default();
        let e = run(&mut p, json!([{ "op": "set", "look": { "brillo": 1 } }])).unwrap_err();
        assert!(e.message.contains("look.brillo") && e.message.contains("padding"), "{}", e.message);
        let e = run(&mut p, json!([{ "op": "set", "volumen": {} }])).unwrap_err();
        assert!(e.message.contains("volumen"));
        assert!(run(&mut p, json!([{ "op": "set" }])).is_err());
    }

    #[test]
    fn los_efectos_no_tocan_el_proyecto() {
        let mut p = Project::default();
        let before = p.clone();
        let out = run(&mut p, json!([{ "op": "seek", "time": 12.5 }, { "op": "play" }, { "op": "undo" }, { "op": "generate_captions" }])).unwrap();
        assert_eq!(out.effects, vec![Effect::Seek(12.5), Effect::Play, Effect::Undo, Effect::GenerateCaptions]);
        assert_eq!(p, before);
        assert!(run(&mut p, json!([{ "op": "seek", "time": 99 }])).is_err());
    }

    #[test]
    fn el_estilo_de_camara_se_aplica() {
        let mut p = Project::default();
        run(&mut p, json!([{ "op": "camera_style", "style": "dynamic" }])).unwrap();
        assert_eq!(p.camera, CameraStyle::Dynamic);
        assert!(run(&mut p, json!([{ "op": "camera_style", "style": "loca" }])).is_err());
    }

    #[test]
    fn una_operacion_desconocida_lista_las_validas() {
        let mut p = Project::default();
        let e = run(&mut p, json!([{ "op": "pintar" }])).unwrap_err();
        assert!(e.message.contains("pintar") && e.message.contains("zoom"));
        assert!(run(&mut p, json!([])).is_err());
        assert!(run(&mut p, json!(["cut"])).is_err());
    }

    #[test]
    fn el_estado_lista_con_indices_y_colores_legibles() {
        let mut p = Project::default();
        run(&mut p, json!([{ "op": "cut", "start": 4, "end": 6 }, { "op": "text", "start": 1, "end": 2, "text": "t" }, { "op": "set", "look": { "backdrop": { "Solid": "#f5f5f2" } } }])).unwrap();
        let state = describe(&p, &ctx(60.0));
        assert_eq!(state["cuts"][0]["index"], 0);
        assert_eq!(state["texts"][0]["color"], "#ffffff");
        assert_eq!(state["look"]["backdrop"]["Solid"], "#f5f5f2");
        assert_eq!(state["result"]["length"], 58.0);
        assert!(state["limits"]["zoom.scale"].is_array());
    }

    #[test]
    fn los_subtitulos_se_resumen_y_conservan_su_indice_real() {
        let mut p = Project::default();
        p.texts.push(Text { caption: true, ..Text::new(1.0, 2.0, "a", TextAt::Bottom) });
        p.texts.push(Text::new(3.0, 4.0, "titulo", TextAt::Top));
        let state = describe(&p, &ctx(60.0));
        assert_eq!(state["texts"].as_array().unwrap().len(), 1);
        assert_eq!(state["texts"][0]["index"], 1);
        assert_eq!(state["captions"]["count"], 1);
    }

    #[test]
    fn las_rachas_de_escritura_y_los_silencios() {
        let keys: Vec<u32> = (0..8).map(|i| 1000 + i * 200).chain([20_000, 20_100]).collect();
        let bursts = typing_bursts(&keys);
        assert_eq!(bursts.len(), 1);
        assert_eq!(bursts[0]["keys"], 8);
        let mut levels = vec![0.5f32; 20];
        levels.extend(vec![0.0; 60]);
        levels.extend(vec![0.5; 20]);
        let spans = silent_spans(&levels, 20.0, 1.0);
        assert_eq!(spans.len(), 1);
        assert_eq!((spans[0]["start"].as_f64(), spans[0]["end"].as_f64()), (Some(1.0), Some(4.0)));
    }

    #[test]
    fn la_actividad_lista_clics_escritura_y_silencios() {
        let track = Track {
            version: 1,
            region: [0, 0, 100, 100],
            samples: vec![Sample(0, 0.1, 0.1, false), Sample(2000, 0.5, 0.25, true), Sample(2100, 0.5, 0.25, false), Sample(6000, 0.9, 0.9, true)],
            cursor_hidden: false,
            keys: (0..6).map(|i| 3000 + i * 150).collect(),
            carets: Vec::new(),
            shapes: Vec::new(),
        };
        let mut levels = vec![0.6f32; 40];
        levels.extend(vec![0.0; 100]);
        let ctx = Ctx { track: Some(&track), levels: &levels, ..ctx(10.0) };
        let report = activity(&Project::default(), &ctx);
        assert_eq!(report["clicks_total"], 2);
        assert_eq!(report["clicks"][0]["t"], 2.0);
        assert_eq!(report["clicks"][0]["x"], 0.5);
        assert_eq!(report["typing"][0]["keys"], 6);
        assert_eq!(report["audio"]["silent_spans"][0]["start"], 2.0);
        assert_eq!(report["audio"]["all_silent"], false);
        // Sin registro ni audio leído, lo dice en vez de inventar.
        let empty = activity(&Project::default(), &Ctx { levels_ready: false, ..ctx_empty() });
        assert!(empty["audio"]["note"].as_str().unwrap().contains("leyendo"));
        assert!(empty["note"].is_string());
    }

    fn ctx_empty() -> Ctx<'static> {
        ctx(10.0)
    }

    #[test]
    fn los_colores_hex() {
        assert_eq!(parse_hex("#ff8800"), Some(0xff8800));
        assert_eq!(parse_hex("f80"), Some(0xff8800));
        assert_eq!(parse_hex("rojo"), None);
        assert_eq!(hex_of(0x00ff00), "#00ff00");
    }
}
