//! Punto de entrada de la aplicación de escritorio Atic.

mod beep;
mod capture;
mod capture_session;
mod capture_shelf;
mod clipboard_history;
mod commands;
mod dictation;
mod export;
mod floating;
mod import;
mod live;
#[cfg(target_os = "macos")]
mod macos_notes;
mod mail;
mod meeting_detection;
mod mouse_bindings;
mod ocr;
mod paste_queue;
mod retention;
mod search;
mod shortcuts;
mod snippets;
mod state;
mod summarization;
mod transcription;
mod tray;
mod webview_tweaks;

use std::sync::Mutex;

use tauri::{Manager, RunEvent, WindowEvent};

use atic_core::{AppDirs, Config, Db, RecordingStatus, Summary, Transcript};

use crate::state::AppState;

/// Repara estados transitorios huérfanos tras un cierre abrupto.
///
/// Si la app se cerró mientras transcribía o resumía, la fila quedó en
/// `transcribing`/`summarizing` sin ningún hilo que la avance. Al arrancar no
/// hay trabajo en curso, así que degradamos cada fila a un estado consistente
/// con lo que exista en disco para que el usuario pueda reintentar.
fn recover_orphaned_statuses(state: &AppState) {
    let recs = match state.db.lock().unwrap().list_recordings() {
        Ok(recs) => recs,
        Err(err) => {
            tracing::warn!(%err, "no se pudieron revisar estados huérfanos al iniciar");
            return;
        }
    };
    for rec in recs {
        let next = match rec.status {
            RecordingStatus::Transcribing => {
                match Transcript::load(&state.dirs.transcript_path(&rec.id)) {
                    Ok(Some(t)) if !t.segments.is_empty() => RecordingStatus::Transcribed,
                    _ => RecordingStatus::Recorded,
                }
            }
            RecordingStatus::Summarizing => {
                match Summary::load(&state.dirs.summary_path(&rec.id)) {
                    Ok(Some(_)) => RecordingStatus::Summarized,
                    _ => RecordingStatus::Transcribed,
                }
            }
            _ => continue,
        };
        match state.db.lock().unwrap().update_status(&rec.id, next) {
            Ok(()) => tracing::info!(
                id = %rec.id, from = ?rec.status, to = ?next,
                "estado huérfano recuperado al iniciar"
            ),
            Err(err) => {
                tracing::warn!(%err, id = %rec.id, "no se pudo recuperar el estado huérfano")
            }
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();

    tauri::Builder::default()
        // single-instance debe registrarse primero.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            state::show_main(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_drag::init())
        .invoke_handler(tauri::generate_handler![
            commands::start_recording,
            commands::stop_recording,
            commands::is_recording,
            commands::list_recordings,
            commands::delete_recording,
            commands::rename_recording,
            commands::get_config,
            commands::set_config,
            commands::set_pill_visible,
            commands::show_main_window,
            shortcuts::failed_shortcuts,
            floating::resize_floating,
            commands::open_data_dir,
            commands::recording_track_path,
            commands::toggle_dictation,
            commands::dictation_phase,
            commands::list_input_devices,
            commands::list_output_devices,
            commands::debug_list_audio_devices,
            commands::audio_preflight,
            commands::test_audio,
            export::export_recording,
            retention::retention_preview,
            retention::cleanup_retention,
            import::import_audio,
            transcription::list_models,
            transcription::current_model_ready,
            transcription::download_model,
            transcription::transcribe_recording,
            transcription::get_transcript,
            transcription::save_transcript,
            summarization::list_summary_templates,
            summarization::list_summary_providers,
            summarization::ollama_available,
            summarization::summarize_recording,
            summarization::get_summary,
            summarization::save_summary,
            mail::secrets_status,
            mail::set_secret,
            mail::send_summary_email,
            capture::capture_primary_monitor,
            capture::list_recent_captures,
            capture::delete_capture,
            capture::copy_capture_path,
            capture::copy_capture_image,
            capture::reveal_capture,
            capture::activate_capture,
            capture::cleanup_captures_now,
            capture::open_captures_dir,
            capture_session::start_capture_session,
            capture_session::overlay_info,
            capture_session::complete_window_capture,
            capture_session::complete_region_capture,
            capture_session::complete_monitor_capture,
            capture_session::cancel_capture_session,
            clipboard_history::list_clipboard_history,
            clipboard_history::paste_clipboard_item,
            clipboard_history::pin_clipboard_item,
            clipboard_history::delete_clipboard_item,
            clipboard_history::clear_clipboard_history,
            clipboard_history::prepare_clipboard_pill,
            clipboard_history::snap_pill_to_cursor,
            clipboard_history::restore_pill_position,
            snippets::list_snippets,
            snippets::upsert_snippet,
            snippets::delete_snippet,
            snippets::paste_snippet,
            snippets::get_scratchpad,
            snippets::set_scratchpad,
            snippets::prepare_snippets_pill,
            paste_queue::list_paste_queue,
            paste_queue::enqueue_paste,
            paste_queue::dismiss_paste_queue_item,
            paste_queue::clear_paste_queue,
            paste_queue::paste_queue_item_now,
            paste_queue::paste_queue_flush_ready,
            ocr::ocr_capture_text,
            ocr::ocr_capture_and_copy,
            ocr::read_capture_ocr_cache,
            search::search_local,
        ])
        .setup(move |app| {
            let dirs = AppDirs::new()?;
            let db = Db::open(&dirs.db_path())?;
            let config = Config::load(&dirs.config_path());

            // Permite reproducir los WAV grabados vía el protocolo asset://.
            let _ = app
                .asset_protocol_scope()
                .allow_directory(dirs.recordings_dir(), true);
            // Permite mostrar las miniaturas de capturas (PNG) vía asset://.
            let _ = app
                .asset_protocol_scope()
                .allow_directory(dirs.captures_dir(), true);
            // Permite mostrar el frame congelado del overlay vía asset://.
            let _ = app
                .asset_protocol_scope()
                .allow_directory(dirs.overlay_frames_dir(), true);
            // Miniaturas del historial de clipboard.
            let _ = app
                .asset_protocol_scope()
                .allow_directory(dirs.clipboard_dir(), true);

            let shortcut = config.global_shortcut.clone();
            let dictation_shortcut = config.dictation_shortcut.clone();
            let summon_pill_shortcut = config.summon_pill_shortcut.clone();
            let pill_radial_shortcut = config.pill_radial_shortcut.clone();
            let clipboard_shortcut = config.clipboard_shortcut.clone();
            let snippets_shortcut = config.snippets_shortcut.clone();
            let screenshot_shortcut = config.screenshot_shortcut.clone();
            let pill_position = config.pill_position;
            let show_pill = config.show_pill;
            let want_autostart = config.autostart;

            app.manage(AppState {
                dirs,
                db: Mutex::new(db),
                config: Mutex::new(config),
                active: Mutex::new(None),
                dictation: Mutex::new(None),
                audio_test_running: Mutex::new(false),
                whisper: Mutex::new(std::collections::HashMap::new()),
                whisper_last_used: Mutex::new(None),
                overlay_session: Mutex::new(None),
                pre_clipboard_position: Mutex::new(None),
                shortcut_failures: Mutex::new(Vec::new()),
            });

            // Repara estados transitorios huérfanos de un cierre abrupto anterior.
            recover_orphaned_statuses(&app.state::<AppState>());
            retention::run_auto_cleanup(app.handle());
            capture::run_capture_cleanup(app.handle());
            meeting_detection::spawn_detector(app.handle().clone());

            // Precarga Whisper en background para que el primer dictado no espere
            // la carga del GGML desde disco.
            state::preload_whisper_async(app.handle());

            tray::build_tray(app.handle())?;

            // Sincronizar autostart con la preferencia guardada.
            {
                use tauri_plugin_autostart::ManagerExt;
                let manager = app.autolaunch();
                let result = if want_autostart {
                    manager.enable()
                } else {
                    manager.disable()
                };
                if let Err(err) = result {
                    tracing::warn!(%err, "no se pudo sincronizar autostart al iniciar");
                }
            }

            // Mouse lateral: Raw Input (pasivo; no puede congelar el ratón del SO).
            mouse_bindings::init(app.handle());

            // Atajos globales: grabación + dictado + pill + clipboard + captura.
            if let Err(err) = shortcuts::register_shortcuts(
                app.handle(),
                &shortcut,
                &dictation_shortcut,
                &summon_pill_shortcut,
                &pill_radial_shortcut,
                &clipboard_shortcut,
                &snippets_shortcut,
                &screenshot_shortcut,
            ) {
                tracing::error!(%err, "no se pudieron registrar los atajos globales");
            }

            clipboard_history::start_watcher(app.handle());

            // Sin Ctrl+P / Find / DevTools del WebView2 en ventanas flotantes.
            webview_tweaks::apply_to_overlay_windows(app.handle());

            // Posición y visibilidad inicial de la pill.
            if let Some(pill) = app.get_webview_window("pill") {
                if let Some((x, y)) = pill_position {
                    let (w, h) = pill
                        .outer_size()
                        .ok()
                        .map(|s| (s.width as i32, s.height as i32))
                        .unwrap_or((112, 48));
                    let (cx, cy) = floating::clamp(x as i32, y as i32, w, h);
                    let _ = pill.set_position(tauri::PhysicalPosition::new(cx, cy));
                }
                if show_pill {
                    let _ = pill.set_always_on_top(true);
                    let _ = pill.show();
                } else {
                    let _ = pill.hide();
                }
            }

            // Las ventanas de captura se declaran `visible: true` (para que las
            // decoraciones/transparencia se apliquen igual que en la pill) y se
            // ocultan aquí hasta que se usan.
            for label in ["capture-shelf", "capture-overlay"] {
                if let Some(window) = app.get_webview_window(label) {
                    let _ = window.hide();
                }
            }

            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } if window.label() == "main" => {
                api.prevent_close();
                let _ = window.hide();
            }
            WindowEvent::Moved(pos) if window.label() == "pill" => {
                if let Some(state) = window.app_handle().try_state::<AppState>() {
                    // Durante el clipboard en el cursor no pisar la home guardada.
                    if state.pre_clipboard_position.lock().unwrap().is_none() {
                        state
                            .config
                            .lock()
                            .unwrap()
                            .pill_position = Some((pos.x as f64, pos.y as f64));
                    }
                }
            }
            _ => {}
        })
        .build(tauri::generate_context!())
        .expect("error al iniciar Atic")
        .run(move |app, event| {
            if let RunEvent::Exit = event {
                if let Some(state) = app.try_state::<AppState>() {
                    let cfg = state.config.lock().unwrap().clone();
                    let _ = cfg.save(&state.dirs.config_path());
                }
            }
        });
}
