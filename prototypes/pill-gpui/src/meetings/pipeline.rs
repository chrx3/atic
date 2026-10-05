//! El trabajo pesado de Reuniones fuera del hilo de la ventana: transcribir
//! (Groq), resumir (el proveedor de Atic, con streaming), guardar lo editado,
//! renombrar, borrar y mandar el resumen por correo. Portado de
//! `src-tauri/src/{transcription,summarization,mail,commands}.rs` sin Tauri:
//! en vez de eventos, cada trabajo largo devuelve un canal con sus avances.
//!
//! Todo lee la configuración y la base en cada llamada (como `data`): Atic
//! puede cambiarlas mientras esta ventana está abierta.

use std::sync::atomic::{AtomicI32, Ordering};
use std::thread;

use atic_core::{secrets, Config, RecordingStatus, SecretKind, Speaker, Summary, Transcript};
use atic_mailer::{self as mailer, OutgoingMail, SmtpConfig};
use atic_summarize::{self as summarize, SummarizerConfig, SummaryTemplate};
use atic_transcribe::TrackInput;
use futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};

use super::data::Paths;
use super::text;

/// Lo que va llegando de un trabajo largo.
pub enum Update {
    /// La base ya dice «Transcribiendo»/«Resumiendo»: hay que releer la lista.
    Started,
    /// Transcripción: de 0 a 1.
    Progress(f32),
    /// Resumen por partes (Groq o cupos chicos).
    Stage(Stage),
    /// Resumen: el texto nuevo que escribió el modelo.
    Delta(String),
    /// El final, siempre el último.
    Done(Result<Option<Summary>, Failure>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Failure {
    /// Atic está en Whisper local, que el prototipo no tiene.
    LocalEngine,
    Message(String),
}

impl Failure {
    pub fn text(&self) -> String {
        match self {
            Failure::LocalEngine => {
                "Atic transcribe en este equipo y aquí solo está Groq por ahora.".into()
            }
            Failure::Message(text) => text.clone(),
        }
    }
}

fn fail(text: impl Into<String>) -> Failure {
    Failure::Message(text.into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stage {
    pub kind: StageKind,
    pub part: u32,
    pub of: u32,
    pub wait_secs: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageKind {
    Map,
    Wait,
    Reduce,
}

impl Stage {
    /// La línea corta bajo el resumen que se escribe.
    pub fn label(&self) -> String {
        match self.kind {
            StageKind::Reduce => "Juntando las partes…".into(),
            StageKind::Wait if self.wait_secs > 0 => {
                format!("Esperando el cupo del proveedor ({} s)…", self.wait_secs)
            }
            StageKind::Wait => "Esperando el cupo del proveedor…".into(),
            StageKind::Map if self.of > 1 => format!("Leyendo la parte {} de {}…", self.part, self.of),
            StageKind::Map => "Leyendo la transcripción…".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Job {
    Transcribe,
    Summarize,
}

/// En qué estado queda la grabación al terminar un trabajo. Un resumen que
/// falla no deja la reunión «con error»: la transcripción sigue sirviendo, y
/// si ya había un resumen anterior, ese sigue ahí.
pub fn status_after(job: Job, ok: bool, has_summary: bool) -> RecordingStatus {
    match (job, ok) {
        (Job::Transcribe, true) => RecordingStatus::Transcribed,
        (Job::Transcribe, false) => RecordingStatus::Error,
        (Job::Summarize, true) => RecordingStatus::Summarized,
        (Job::Summarize, false) if has_summary => RecordingStatus::Summarized,
        (Job::Summarize, false) => RecordingStatus::Transcribed,
    }
}

/// ¿Hay un trabajo de Atic en curso según la base? Sirve para no ofrecer
/// «Transcribir» mientras la app vieja ya lo está haciendo.
pub fn busy(status: RecordingStatus) -> bool {
    matches!(status, RecordingStatus::Transcribing | RecordingStatus::Summarizing)
}

// --- Transcribir ----------------------------------------------------------------

/// Transcribe con Groq en un hilo. `force_groq` es el «úsalo igual» cuando
/// Atic está configurado en Whisper local (no cambia la configuración).
pub fn transcribe(paths: Paths, id: String, force_groq: bool) -> UnboundedReceiver<Update> {
    let (tx, rx) = unbounded();
    thread::spawn(move || {
        let result = run_transcribe(&paths, &id, force_groq, &tx).map(|()| None);
        let _ = tx.unbounded_send(Update::Done(result));
    });
    rx
}

fn groq_key() -> Result<String, Failure> {
    secrets::get_secret(SecretKind::GroqApiKey)
        .map_err(|e| fail(format!("No se pudo leer el llavero: {e}")))?
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .ok_or_else(|| fail("Falta la llave de Groq. Pégala en Ajustes (arriba a la derecha)."))
}

fn run_transcribe(
    paths: &Paths,
    id: &str,
    force_groq: bool,
    tx: &UnboundedSender<Update>,
) -> Result<(), Failure> {
    let cfg = Config::load(&paths.config_path());
    if cfg.meeting_backend != "groq" && !force_groq {
        return Err(Failure::LocalEngine);
    }
    let key = groq_key()?;
    let db = paths.db().map_err(|e| fail(format!("No se pudo abrir la base: {e}")))?;
    let rec = db
        .get_recording(id)
        .map_err(|e| fail(e.to_string()))?
        .ok_or_else(|| fail("La reunión ya no existe."))?;

    let want = cfg.effective_transcribe_tracks();
    let dir = paths.recording_dir(id);
    let mic = (want != "system" && rec.mic_path.is_some()).then(|| dir.join("mic.wav"));
    let system = (want != "mic" && rec.system_path.is_some()).then(|| dir.join("system.wav"));
    let mut tracks = Vec::new();
    for (path, speaker) in [(&mic, Speaker::Me), (&system, Speaker::Others)] {
        if let Some(wav) = path.as_deref().filter(|p| p.exists()) {
            tracks.push(TrackInput { wav, speaker });
        }
    }
    if tracks.is_empty() {
        return Err(fail("No hay pistas de audio para transcribir con la configuración actual."));
    }

    db.update_status(id, RecordingStatus::Transcribing).map_err(|e| fail(e.to_string()))?;
    drop(db);
    let _ = tx.unbounded_send(Update::Started);

    let last = AtomicI32::new(-1);
    let progress_tx = tx.clone();
    let on_progress = move |p: f32| {
        // Un aviso por punto porcentual: Groq manda muchos más.
        let pct = (p * 100.0) as i32;
        if last.swap(pct, Ordering::Relaxed) != pct {
            let _ = progress_tx.unbounded_send(Update::Progress(p.clamp(0.0, 1.0)));
        }
    };
    let result = atic_transcribe::transcribe_groq_recording(
        &key,
        &tracks,
        cfg.whisper_language().as_deref(),
        &cfg.meeting_groq_model,
        on_progress,
    );
    // Sin texto no es un error: una grabación en silencio queda transcrita
    // (vacía o con lo que Whisper inventa) y la lista la marca «Sin voz».
    let outcome = match result {
        Ok(t) => t
            .save(&paths.transcript_path(id))
            .map_err(|e| fail(format!("No se pudo guardar la transcripción: {e}"))),
        Err(e) => Err(fail(e.to_ui(false))),
    };
    set_status(paths, id, status_after(Job::Transcribe, outcome.is_ok(), false));
    outcome
}

fn set_status(paths: &Paths, id: &str, status: RecordingStatus) {
    if let Err(error) = paths.db().and_then(|db| Ok(db.update_status(id, status)?)) {
        eprintln!("reuniones: no se pudo guardar el estado de {id}: {error}");
    }
}

// --- Resumir --------------------------------------------------------------------

pub fn template_label(template: SummaryTemplate) -> &'static str {
    match template {
        SummaryTemplate::SummaryKeyPoints => "Puntos clave",
        SummaryTemplate::ExecutiveMinutes => "Acta ejecutiva",
        SummaryTemplate::ActionItems => "Tareas",
        SummaryTemplate::FollowupEmail => "Correo de seguimiento",
    }
}

/// Lo que se le pasa al modelo: sin lo que Whisper inventa sobre el silencio
/// («gracias gracias gracias», «[música]»), que en un resumen se lee como
/// algo que alguien dijo, y en orden.
pub fn prepare_transcript(transcript: &Transcript) -> Transcript {
    let mut out = Transcript {
        language: transcript.language.clone(),
        segments: transcript
            .segments
            .iter()
            .filter(|s| !text::is_junk(&s.text))
            .cloned()
            .map(|mut s| {
                s.text = s.text.trim().to_string();
                s
            })
            .collect(),
    };
    out.sort();
    out
}

fn summarizer_config(cfg: &Config) -> Result<SummarizerConfig, Failure> {
    // Fallo del llavero ≠ no hay llave: tragarlo pediría una llave que sí está.
    let api_key = match SecretKind::for_summary_provider(&cfg.summary_backend) {
        Some(kind) => secrets::get_secret(kind)
            .map_err(|e| fail(format!("No se pudo leer el llavero: {e}")))?,
        None => None,
    };
    Ok(SummarizerConfig {
        backend: cfg.summary_backend.clone(),
        api_key,
        model: cfg.summary_model.clone(),
        base_url: cfg.summary_base_url.clone(),
        english: cfg.resolved_ui_language() == "en",
    })
}

/// Si el modelo guardado ya no existe en el proveedor, usa uno que sí (como
/// `heal_summarizer_model` de Atic, pero sin reescribir su configuración).
fn heal_model(cfg: &mut SummarizerConfig) {
    let Some(info) = summarize::find_provider(&cfg.backend) else {
        return;
    };
    let base = if cfg.base_url.trim().is_empty() { info.default_base_url } else { cfg.base_url.as_str() };
    if let Ok(models) = summarize::list_remote_models(info.kind, info.id, base, cfg.api_key.as_deref()) {
        if !models.is_empty() && !cfg.model.trim().is_empty() {
            cfg.model = summarize::pick_available_model(&cfg.model, &models, info.default_model);
        }
    }
}

pub fn summarize(paths: Paths, id: String, template: SummaryTemplate) -> UnboundedReceiver<Update> {
    let (tx, rx) = unbounded();
    thread::spawn(move || {
        let result = run_summarize(&paths, &id, template, &tx).map(Some);
        let _ = tx.unbounded_send(Update::Done(result));
    });
    rx
}

fn run_summarize(
    paths: &Paths,
    id: &str,
    template: SummaryTemplate,
    tx: &UnboundedSender<Update>,
) -> Result<Summary, Failure> {
    let cfg = Config::load(&paths.config_path());
    let mut summarizer_cfg = summarizer_config(&cfg)?;
    let english = summarizer_cfg.english;
    // Validar antes de marcar el estado: sin llave no hay nada que esperar.
    summarize::build_summarizer(&summarizer_cfg).map_err(|e| fail(e.to_ui(english)))?;
    let db = paths.db().map_err(|e| fail(format!("No se pudo abrir la base: {e}")))?;
    let rec = db
        .get_recording(id)
        .map_err(|e| fail(e.to_string()))?
        .ok_or_else(|| fail("La reunión ya no existe."))?;
    let transcript = Transcript::load(&paths.transcript_path(id))
        .map_err(|e| fail(format!("No se pudo leer la transcripción: {e}")))?
        .ok_or_else(|| fail("No hay transcripción. Transcríbela primero."))?;
    let transcript = prepare_transcript(&transcript);
    if transcript.segments.is_empty() {
        return Err(fail("La transcripción no tiene nada que resumir."));
    }
    let summary_path = paths.summary_path(id);
    let had_summary = summary_path.exists();

    db.update_status(id, RecordingStatus::Summarizing).map_err(|e| fail(e.to_string()))?;
    drop(db);
    let _ = tx.unbounded_send(Update::Started);

    let result = (|| {
        heal_model(&mut summarizer_cfg);
        let summarizer = summarize::build_summarizer(&summarizer_cfg)?;
        let mut on_delta = |delta: &str| {
            let _ = tx.unbounded_send(Update::Delta(delta.to_string()));
        };
        let mut on_progress = |p: &summarize::SummarizeProgress| {
            let kind = match p.stage {
                "wait" => StageKind::Wait,
                "reduce" => StageKind::Reduce,
                _ => StageKind::Map,
            };
            let _ = tx.unbounded_send(Update::Stage(Stage {
                kind,
                part: p.part,
                of: p.of,
                wait_secs: p.wait_secs,
            }));
        };
        summarizer.summarize_with_progress(&transcript, template, &rec.title, &mut on_delta, &mut on_progress)
    })();
    let outcome = match result {
        Ok(summary) => summary
            .save(&summary_path)
            .map(|()| summary)
            .map_err(|e| fail(format!("No se pudo guardar el resumen: {e}"))),
        Err(e) => Err(fail(e.to_ui(english))),
    };
    set_status(paths, id, status_after(Job::Summarize, outcome.is_ok(), had_summary));
    if let Ok(summary) = &outcome {
        auto_title(paths, id, summary);
    }
    outcome
}

/// Si la reunión sigue con el título de fábrica, le pone uno sacado del
/// resumen. Se relee la base justo antes: un nombre que el usuario puso
/// mientras se resumía no se pisa.
fn auto_title(paths: &Paths, id: &str, summary: &Summary) {
    let Some(title) = title_from_summary(summary) else {
        return;
    };
    let result = paths.db().and_then(|db| {
        if let Some(rec) = db.get_recording(id)? {
            if is_factory_title(&rec.title) {
                db.update_title(id, &title)?;
            }
        }
        Ok(())
    });
    if let Err(error) = result {
        eprintln!("reuniones: no se pudo poner el título de {id}: {error}");
    }
}

/// «Grabación 2026-10-04 18:42»: el que ponen `recorder::local_title` y
/// `Recording::new`. Cualquier otro lo eligió alguien.
pub fn is_factory_title(title: &str) -> bool {
    title
        .strip_prefix("Grabación ")
        .is_some_and(|rest| chrono::NaiveDateTime::parse_from_str(rest, "%Y-%m-%d %H:%M").is_ok())
}

const TITLE_MAX: usize = 60;

/// Un título corto para la reunión, sin otra llamada al modelo. El `title`
/// que arma `atic-summarize` es «plantilla — título actual» (no dice nada),
/// así que sale del asunto del correo de seguimiento o, si no, de la primera
/// frase de la sección Resumen.
pub fn title_from_summary(summary: &Summary) -> Option<String> {
    if let Some(subject) = summary.subject.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let subject = ["Seguimiento:", "Follow-up:", "Follow up:", "Resumen:", "Re:"]
            .iter()
            .find_map(|p| strip_prefix_ci(subject, p))
            .unwrap_or(subject);
        return shorten(subject);
    }
    let sections = super::summary::parse(&summary.body, "Resumen");
    let section = sections
        .iter()
        .find(|s| s.kind == super::summary::Kind::Summary && !s.blocks.is_empty())
        .or_else(|| sections.iter().find(|s| !s.blocks.is_empty()))?;
    let text = match section.blocks.first()? {
        super::summary::Block::Paragraph(text) => text.as_str(),
        super::summary::Block::List { items, .. } => items.first()?.text.as_str(),
    };
    shorten(first_sentence(text))
}

fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix).then(|| text[prefix.len()..].trim_start())
}

fn first_sentence(text: &str) -> &str {
    let text = text.trim();
    let mut prev_lower = false;
    for (ix, c) in text.char_indices() {
        // Fin de frase tras una minúscula y antes de un espacio: así «v2.1»
        // no corta, ni una abreviatura como «Sr.» o «Ing.».
        if matches!(c, '.' | '!' | '?' | ';' | ':')
            && prev_lower
            && text[ix + c.len_utf8()..].starts_with(char::is_whitespace)
        {
            let word = text[..ix].rsplit(char::is_whitespace).next().unwrap_or("");
            let abbreviation = c == '.'
                && word.chars().count() <= 3
                && word.chars().next().is_some_and(char::is_uppercase);
            if !abbreviation {
                return &text[..ix];
            }
        }
        prev_lower = c.is_lowercase() || c.is_numeric() || c == ')';
    }
    text
}

/// «reunión», «sesión»…: lo que el resumen dice de sí mismo al empezar.
const MEETING: [&str; 7] = ["reunión", "sesión", "llamada", "junta", "meeting", "call", "session"];

/// Los verbos con que el modelo abre («se revisó…», «trató sobre…»).
const VERBS: [&str; 24] = [
    "revisó", "revisaron", "discutió", "discutieron", "trató", "trataron", "habló", "hablaron",
    "abordó", "abordaron", "analizó", "analizaron", "presentó", "presentaron", "centró", "planificó",
    "planificaron", "conversó", "conversaron", "discussed", "reviewed", "covered", "focused", "addressed",
];

/// Palabras que no cierran bien un título cortado.
const DANGLING: [&str; 22] = [
    "y", "e", "o", "de", "del", "la", "el", "los", "las", "en", "con", "a", "al", "para", "por",
    "que", "un", "una", "su", "and", "the", "of",
];

/// Quita la entrada que no dice de qué fue la reunión: «En la sesión se
/// revisó el presupuesto» → «el presupuesto». Si no la reconoce entera, no
/// toca nada (así «La reunión anual de ventas» queda igual).
fn strip_lead_in<'a>(words: &'a [&'a str]) -> &'a [&'a str] {
    let w = |i: usize| -> String {
        words.get(i).map(|w| w.trim_end_matches(',').to_lowercase()).unwrap_or_default()
    };
    let is = |i: usize, list: &[&str]| list.contains(&w(i).as_str());
    let mut i = 0;
    let mut context = false;
    if is(0, &["en", "durante", "in", "during"]) && is(1, &["la", "esta", "esa", "the", "this"]) && is(2, &MEETING) {
        i = 3;
        context = true;
    } else if is(0, &["la", "esta", "the"]) && is(1, &MEETING) || is(0, &["the"]) && is(1, &["team"]) {
        i = 2;
    } else if is(0, &["we"]) {
        i = 1;
    }
    let mut verb = false;
    if w(i) == "se" && is(i + 1, &VERBS) {
        i += 2;
        verb = true;
    } else if is(i, &VERBS) {
        i += 1;
        verb = true;
    }
    if verb && is(i, &["de", "sobre", "en", "on"]) {
        i += 1;
    }
    // «En la reunión, Ana presentó…»: sin verbo, solo se va el «en la reunión».
    let keep = if verb { i } else if context { 3 } else { 0 };
    if keep >= words.len() { words } else { &words[keep..] }
}

fn shorten(text: &str) -> Option<String> {
    let all: Vec<&str> = text.split_whitespace().collect();
    let mut text = strip_lead_in(&all).join(" ");
    // Larga y con una coma a tiempo: el título es la primera cláusula.
    if text.chars().count() > TITLE_MAX {
        let cut = text
            .char_indices()
            .enumerate()
            .filter(|&(n, (_, c))| c == ',' && (15..=TITLE_MAX).contains(&n))
            .map(|(_, (ix, _))| ix)
            .last();
        if let Some(ix) = cut {
            text.truncate(ix);
        }
    }
    let mut words: Vec<&str> = Vec::new();
    let mut len = 0;
    for word in text.split(' ') {
        let add = word.chars().count() + usize::from(!words.is_empty());
        if len + add > TITLE_MAX {
            // Cortado: que no termine en «de», «y»…
            while words.last().is_some_and(|w| DANGLING.contains(&w.to_lowercase().as_str())) {
                words.pop();
            }
            break;
        }
        len += add;
        words.push(word);
    }
    let title = words.join(" ");
    let title = title.trim_end_matches(|c: char| matches!(c, '.' | ',' | ';' | ':' | '…' | '-' | '—'));
    let mut chars = title.chars();
    let first = chars.next()?;
    let title: String = first.to_uppercase().chain(chars).collect();
    (title.chars().count() >= 3).then_some(title)
}

/// ¿La transcripción no tiene a nadie hablando? Vacía o solo lo que Whisper
/// inventa sobre el silencio.
pub fn is_silent(transcript: &Transcript) -> bool {
    transcript.segments.iter().all(|s| text::is_junk(&s.text))
}

/// El resumen editado a mano: queda como `manual` (así lo dice la línea de
/// abajo) y la reunión, con resumen.
pub fn edited_summary(previous: Option<&Summary>, body: &str, template: SummaryTemplate, title: &str) -> Summary {
    Summary {
        template: previous.map_or_else(|| template.as_str().to_string(), |s| s.template.clone()),
        title: previous.map_or_else(|| format!("Resumen · {title}"), |s| s.title.clone()),
        body: body.trim_end().to_string(),
        subject: previous.and_then(|s| s.subject.clone()),
        backend: "manual".into(),
        created_at: chrono::Utc::now(),
    }
}

pub fn save_summary(paths: &Paths, id: &str, summary: &Summary) -> Result<(), String> {
    let db = paths.db().map_err(|e| e.to_string())?;
    db.get_recording(id)
        .map_err(|e| e.to_string())?
        .ok_or("La reunión ya no existe.")?;
    summary.save(&paths.summary_path(id)).map_err(|e| e.to_string())?;
    db.update_status(id, RecordingStatus::Summarized).map_err(|e| e.to_string())
}

// --- Renombrar y borrar ---------------------------------------------------------

/// El título limpio, o `None` si no queda nada.
pub fn clean_title(raw: &str) -> Option<String> {
    let title = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let title: String = title.chars().take(200).collect();
    (!title.is_empty()).then_some(title)
}

pub fn rename(paths: &Paths, id: &str, title: &str) -> Result<(), String> {
    paths.db().map_err(|e| e.to_string())?.update_title(id, title).map_err(|e| e.to_string())
}

/// Borra la fila y la carpeta. La ruta sale de una grabación que existe en la
/// base, nunca del id a secas (como `delete_recording` de Atic).
pub fn delete(paths: &Paths, id: &str) -> Result<(), String> {
    let db = paths.db().map_err(|e| e.to_string())?;
    let rec = db
        .get_recording(id)
        .map_err(|e| e.to_string())?
        .ok_or("La reunión ya no existe.")?;
    let dir = paths.recording_dir(&rec.id);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("No se pudo borrar la carpeta: {e}"))?;
    }
    db.delete_recording(&rec.id).map_err(|e| e.to_string())
}

// --- Correo ---------------------------------------------------------------------

/// Los destinatarios escritos en un campo: separados por coma, punto y coma o
/// espacios. Devuelve el primero que no parece una dirección.
pub fn parse_recipients(raw: &str) -> Result<Vec<String>, String> {
    let list: Vec<String> = raw
        .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .map(|a| a.trim().trim_start_matches('<').trim_end_matches('>').to_string())
        .filter(|a| !a.is_empty())
        .collect();
    if list.is_empty() {
        return Err("Escribe al menos un destinatario.".into());
    }
    if let Some(bad) = list.iter().find(|a| !looks_like_address(a)) {
        return Err(format!("«{bad}» no parece un correo."));
    }
    Ok(list)
}

fn looks_like_address(address: &str) -> bool {
    let Some((local, domain)) = address.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.contains('@')
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains("..")
}

pub enum MailOutcome {
    /// Se mandó por SMTP; el texto es para el aviso.
    Sent(String),
    /// Hay que abrir esta URL `mailto:` (el borrador en el cliente de correo).
    Draft(String),
}

/// Manda por SMTP si Atic lo tiene configurado; si no, arma el `mailto:`.
/// Bloquea (SMTP): se llama desde un hilo.
pub fn send_mail(paths: &Paths, mail: &OutgoingMail) -> Result<MailOutcome, String> {
    let cfg = Config::load(&paths.config_path());
    send_mail_with(&cfg, mail)
}

fn send_mail_with(cfg: &Config, mail: &OutgoingMail) -> Result<MailOutcome, String> {
    let smtp = if cfg.mail_backend == "smtp" {
        let password = secrets::get_secret(SecretKind::SmtpPassword)
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        Some(SmtpConfig {
            host: cfg.smtp_host.clone(),
            port: cfg.smtp_port,
            username: cfg.smtp_username.clone(),
            password,
            from: if cfg.smtp_from.is_empty() { cfg.smtp_username.clone() } else { cfg.smtp_from.clone() },
            use_starttls: cfg.smtp_use_tls,
        })
    } else {
        None
    };
    let backend = mailer::build_mailer(&cfg.mail_backend, smtp).map_err(|e| e.to_ui(false))?;
    let message = backend.send(mail).map_err(|e| e.to_ui(false))?;
    Ok(if backend.name() == "mailto" {
        MailOutcome::Draft(message)
    } else {
        MailOutcome::Sent(format!("Enviado a {}", mail.to.join(", ")))
    })
}

pub fn mail_subject(summary: Option<&Summary>, title: &str) -> String {
    summary
        .and_then(|s| s.subject.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("Seguimiento: {title}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use atic_core::Segment;

    #[test]
    fn estados_al_terminar() {
        use RecordingStatus::*;
        assert_eq!(status_after(Job::Transcribe, true, false), Transcribed);
        assert_eq!(status_after(Job::Transcribe, false, true), Error);
        assert_eq!(status_after(Job::Summarize, true, false), Summarized);
        assert_eq!(status_after(Job::Summarize, false, false), Transcribed);
        // Regenerar y fallar no esconde el resumen anterior.
        assert_eq!(status_after(Job::Summarize, false, true), Summarized);
        assert!(busy(Transcribing) && busy(Summarizing) && !busy(Error));
    }

    fn seg(start_ms: i64, text: &str) -> Segment {
        Segment { start_ms, end_ms: start_ms + 1000, speaker: Speaker::Others, speaker_name: None, text: text.into() }
    }

    #[test]
    fn el_texto_para_resumir_va_sin_basura_y_en_orden() {
        let t = Transcript {
            language: Some("es".into()),
            segments: vec![
                seg(5_000, "  Cerramos el acta.  "),
                seg(0, "Partamos."),
                seg(2_000, "..."),
                seg(3_000, "gracias gracias gracias gracias gracias gracias"),
                seg(4_000, "[Music]"),
            ],
        };
        let out = prepare_transcript(&t);
        let texts: Vec<_> = out.segments.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, ["Partamos.", "Cerramos el acta."]);
        assert_eq!(out.language.as_deref(), Some("es"));
    }

    #[test]
    fn destinatarios() {
        assert_eq!(
            parse_recipients("ana@x.cl, <bo@y.com>;  cy@z.org\n").unwrap(),
            ["ana@x.cl", "bo@y.com", "cy@z.org"]
        );
        assert!(parse_recipients("  ,; ").unwrap_err().contains("al menos"));
        assert!(parse_recipients("ana@x.cl, bo").unwrap_err().contains("«bo»"));
        for bad in ["@x.cl", "a@x", "a@.cl", "a@x.", "a@@x.cl", "a@x..cl"] {
            assert!(parse_recipients(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn el_correo_sin_smtp_arma_el_mailto_sin_abrirlo() {
        let cfg = Config::default();
        assert_eq!(cfg.mail_backend, "mailto");
        let mail = OutgoingMail {
            to: vec!["ana@x.cl".into(), "bo@y.com".into()],
            subject: "Seguimiento: Q4".into(),
            body: "## Resumen\nTodo bien & listo".into(),
        };
        match send_mail_with(&cfg, &mail).unwrap() {
            MailOutcome::Draft(url) => {
                assert!(url.starts_with("mailto:ana@x.cl,bo@y.com?subject=Seguimiento"));
                assert!(url.contains("%26"));
            }
            MailOutcome::Sent(_) => panic!("sin SMTP no se manda nada"),
        }
    }

    #[test]
    fn asunto_y_titulo() {
        assert_eq!(mail_subject(None, "Q4"), "Seguimiento: Q4");
        assert_eq!(clean_title("  Plan \n  Q4 "), Some("Plan Q4".into()));
        assert_eq!(clean_title(" \t "), None);
        let s = edited_summary(None, "Hola\n\n", SummaryTemplate::ActionItems, "Q4");
        assert_eq!((s.backend.as_str(), s.template.as_str(), s.body.as_str()), ("manual", "action_items", "Hola"));
    }

    /// De punta a punta contra Groq y el proveedor de resumen, sobre una
    /// carpeta de prueba: `MEETINGS_DATA_DIR=<carpeta> MEETINGS_E2E_ID=<id>
    /// cargo test e2e -- --ignored --nocapture`. Nunca con los datos reales.
    #[test]
    #[ignore]
    fn e2e_transcribir_y_resumir() {
        assert!(std::env::var_os("MEETINGS_DATA_DIR").is_some(), "solo con una carpeta de prueba");
        let id = std::env::var("MEETINGS_E2E_ID").unwrap();
        let paths = super::super::data::Source::open().unwrap().paths().unwrap().clone();
        let run = |rx: UnboundedReceiver<Update>| {
            let (mut progress, mut deltas) = (0, 0);
            for update in futures::executor::block_on_stream(rx) {
                match update {
                    Update::Started => println!("empezó"),
                    Update::Progress(_) => progress += 1,
                    Update::Stage(s) => println!("etapa: {}", s.label()),
                    Update::Delta(_) => deltas += 1,
                    Update::Done(r) => {
                        println!("avances: {progress}, trozos: {deltas}");
                        return r;
                    }
                }
            }
            panic!("el canal se cerró sin Done");
        };
        // `MEETINGS_E2E_ONLY_SUMMARY=1`: no gastar Groq otra vez.
        if std::env::var_os("MEETINGS_E2E_ONLY_SUMMARY").is_none() {
            run(transcribe(paths.clone(), id.clone(), false)).expect("transcribir");
        }
        let t = Transcript::load(&paths.transcript_path(&id)).unwrap().unwrap();
        println!("{}", t.to_plain_text());
        let db = paths.db().unwrap();
        assert_eq!(db.get_recording(&id).unwrap().unwrap().status, RecordingStatus::Transcribed);
        let summary = run(summarize(paths.clone(), id.clone(), SummaryTemplate::ExecutiveMinutes))
            .expect("resumir")
            .unwrap();
        println!("--- {} ({})
{}", summary.title, summary.backend, summary.body);
        assert_eq!(db.get_recording(&id).unwrap().unwrap().status, RecordingStatus::Summarized);
        assert!(paths.summary_path(&id).exists());
    }

    fn summary(body: &str, subject: Option<&str>) -> Summary {
        Summary {
            template: "summary_key_points".into(),
            title: "Puntos clave — Grabación 2026-10-04 18:42".into(),
            body: body.into(),
            subject: subject.map(Into::into),
            backend: "groq".into(),
            created_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn titulo_de_fabrica() {
        assert!(is_factory_title("Grabación 2026-10-04 18:42"));
        assert!(!is_factory_title("Grabación 2026-10-04 18:42 con Ana"));
        assert!(!is_factory_title("Grabación del lunes"));
        assert!(!is_factory_title("Plan Q4"));
        assert!(!is_factory_title("grabación 2026-10-04 18:42"));
    }

    #[test]
    fn titulo_desde_el_resumen() {
        let s = summary("## Resumen\nSe revisó el presupuesto del trimestre. Luego se habló de contratar.\n\n## Tareas\n- Ana: enviar acta", None);
        assert_eq!(title_from_summary(&s).as_deref(), Some("El presupuesto del trimestre"));
        // Sin sección Resumen: la primera que tenga algo, aunque sea lista.
        let s = summary("## Tareas\n- **Cerrar** el contrato con Kora.", None);
        assert_eq!(title_from_summary(&s).as_deref(), Some("Cerrar el contrato con Kora"));
        // El correo de seguimiento trae su asunto.
        let s = summary("Hola equipo,", Some("Seguimiento: lanzamiento de la app"));
        assert_eq!(title_from_summary(&s).as_deref(), Some("Lanzamiento de la app"));
        assert_eq!(title_from_summary(&summary("  \n", None)), None);
        // «Sr. Pérez» y «v2.1» no son fin de frase.
        let s = summary("Reunión con el Sr. Pérez sobre la v2.1 del portal. Otra cosa.", None);
        assert_eq!(title_from_summary(&s).as_deref(), Some("Reunión con el Sr. Pérez sobre la v2.1 del portal"));
        // Lo que escribió Groq de verdad (gpt-oss-120b, puntos clave).
        let s = summary("## Resumen\nEn la sesión se revisó el reproductor de la ventana nueva, destacando que permite saltar a cualquier punto.", None);
        assert_eq!(title_from_summary(&s).as_deref(), Some("El reproductor de la ventana nueva"));
        let s = summary("La reunión se centró en el cierre del trimestre.", None);
        assert_eq!(title_from_summary(&s).as_deref(), Some("El cierre del trimestre"));
        let s = summary("En la reunión, Ana presentó el plan de ventas.", None);
        assert_eq!(title_from_summary(&s).as_deref(), Some("Ana presentó el plan de ventas"));
        let s = summary("La reunión anual de ventas terminó temprano.", None);
        assert_eq!(title_from_summary(&s).as_deref(), Some("La reunión anual de ventas terminó temprano"));
        let s = summary("Se planificó el trabajo del cuarto trimestre, identificando que los clientes piden resúmenes.", None);
        assert_eq!(title_from_summary(&s).as_deref(), Some("El trabajo del cuarto trimestre"));
    }

    #[test]
    fn titulo_largo_se_corta_en_palabra() {
        let s = summary(
            "## Resumen\nEl equipo de operaciones revisó el estado de los contratos de mantención de las plantas del norte y acordó plazos",
            None,
        );
        let title = title_from_summary(&s).unwrap();
        assert!(title.chars().count() <= TITLE_MAX, "{title}");
        assert_eq!(title, "El equipo de operaciones revisó el estado de los contratos");
    }

    #[test]
    fn silencio() {
        let t = |texts: &[&str]| Transcript {
            language: None,
            segments: texts.iter().enumerate().map(|(i, x)| seg(i as i64 * 1000, x)).collect(),
        };
        assert!(is_silent(&t(&[])));
        assert!(is_silent(&t(&["...", " [BLANK_AUDIO] ", "gracias gracias gracias gracias gracias gracias"])));
        assert!(!is_silent(&t(&["...", "Partamos con el acta."])));
    }

    #[test]
    fn etapas() {
        let st = Stage { kind: StageKind::Map, part: 2, of: 3, wait_secs: 0 };
        assert_eq!(st.label(), "Leyendo la parte 2 de 3…");
        let st = Stage { kind: StageKind::Wait, part: 2, of: 3, wait_secs: 7 };
        assert!(st.label().contains("7 s"));
    }
}
