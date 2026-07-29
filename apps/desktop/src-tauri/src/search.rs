//! Búsqueda unificada local (fragmentos, clipboard, capturas, bloc, grabaciones).

use serde::Serialize;
use tauri::State;

use crate::clipboard_history;
use crate::snippets;
use crate::state::AppState;
use atic_core::MutexExt;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchHitKind {
    Snippet,
    Clipboard,
    Capture,
    Scratchpad,
    Recording,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub id: String,
    pub kind: SearchHitKind,
    pub title: String,
    pub preview: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<u32>,
}

fn normalize(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            other => other,
        })
        .collect()
}

fn matches_query(query: &str, haystack: &str) -> bool {
    let q = normalize(query);
    if q.is_empty() {
        return false;
    }
    normalize(haystack).contains(&q)
}

fn score_match(query: &str, haystack: &str) -> Option<u32> {
    if !matches_query(query, haystack) {
        return None;
    }
    let q = normalize(query);
    let h = normalize(haystack);
    if h.starts_with(&q) {
        Some(100)
    } else if h.contains(&q) {
        Some(50)
    } else {
        Some(10)
    }
}

fn best_score(query: &str, parts: &[&str]) -> Option<u32> {
    parts
        .iter()
        .filter_map(|part| score_match(query, part))
        .max()
}

#[tauri::command]
pub fn search_local(state: State<AppState>, query: String) -> Result<Vec<SearchHit>, String> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let mut hits: Vec<SearchHit> = Vec::new();

    for snippet in snippets::all_snippets(&state) {
        let parts = [
            snippet.name.as_str(),
            snippet.body.as_str(),
            &snippet.aliases.join(" "),
        ];
        if let Some(score) = best_score(query, &parts) {
            let preview: String = snippet.body.chars().take(140).collect();
            hits.push(SearchHit {
                id: snippet.id.clone(),
                kind: SearchHitKind::Snippet,
                title: snippet.name.clone(),
                preview,
                score: Some(score),
            });
        }
    }

    if let Ok(scratch) = snippets::scratchpad_body(&state) {
        if !scratch.is_empty() {
            if let Some(score) = score_match(query, &scratch) {
                let preview: String = scratch.chars().take(140).collect();
                hits.push(SearchHit {
                    id: "scratchpad".into(),
                    kind: SearchHitKind::Scratchpad,
                    title: "Bloc de notas".into(),
                    preview,
                    score: Some(score),
                });
            }
        }
    }

    if let Ok(items) = clipboard_history::collect_clipboard_items(&state) {
        for item in items {
            let text = item
                .text
                .as_deref()
                .filter(|t| !t.is_empty())
                .unwrap_or(item.preview.as_str());
            if let Some(score) = best_score(query, &[item.preview.as_str(), text]) {
                let is_image = matches!(item.kind, clipboard_history::ClipboardKind::Image);
                hits.push(SearchHit {
                    id: item.id.clone(),
                    kind: SearchHitKind::Clipboard,
                    title: if is_image {
                        "Imagen del portapapeles".into()
                    } else {
                        "Portapapeles".into()
                    },
                    preview: item.preview.clone(),
                    score: Some(score),
                });
            }
        }
    }

    let captures = crate::capture::recent_captures_limited(&state.dirs.captures_dir(), 50);
    for cap in captures {
        let filename = cap.path.rsplit(['/', '\\']).next().unwrap_or(&cap.path);
        let mut parts = vec![filename, cap.label.as_str()];
        let ocr_path = format!("{}.ocr.txt", cap.path);
        let ocr_text = std::fs::read_to_string(&ocr_path).unwrap_or_default();
        if !ocr_text.is_empty() {
            parts.push(ocr_text.as_str());
        }
        if let Some(score) = best_score(query, &parts) {
            hits.push(SearchHit {
                id: cap.path.clone(),
                kind: SearchHitKind::Capture,
                title: if cap.label.is_empty() {
                    "Captura".into()
                } else {
                    format!("Captura {}", cap.label)
                },
                preview: if ocr_text.is_empty() {
                    filename.to_string()
                } else {
                    ocr_text.chars().take(140).collect()
                },
                score: Some(score),
            });
        }
    }

    let recordings = state
        .db
        .lock_or_recover()
        .list_recordings()
        .map_err(|e| e.to_string())?;
    for rec in recordings {
        if let Some(score) = score_match(query, &rec.title) {
            hits.push(SearchHit {
                id: rec.id.clone(),
                kind: SearchHitKind::Recording,
                title: rec.title.clone(),
                preview: rec.started_at.format("%Y-%m-%d %H:%M").to_string(),
                score: Some(score),
            });
        }
    }

    hits.sort_by(|a, b| {
        b.score
            .unwrap_or(0)
            .cmp(&a.score.unwrap_or(0))
            .then_with(|| a.title.cmp(&b.title))
    });
    hits.truncate(40);
    Ok(hits)
}
