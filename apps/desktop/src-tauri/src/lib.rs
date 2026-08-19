//! Punto de entrada de la aplicación de escritorio Atic.

pub mod agents;
mod annotate;
mod beep;
mod capture;
mod capture_session;
mod capture_shelf;
mod clipboard_history;
mod commands;
mod diagnostics;
mod dictation;
mod export;
mod floating;
mod import;
mod launcher;
mod launcher_icons;
mod live;
#[cfg(target_os = "macos")]
mod macos_notes;
mod mail;
mod meeting_detection;
mod mouse_bindings;
mod ocr;
#[cfg(windows)]
mod ole_text_drag;
mod overlay;
mod panel_float;
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
use atic_core::MutexExt;

/// Repara estados transitorios huérfanos tras un cierre abrupto.
///
/// Si la app se cerró mientras transcribía o resumía, la fila quedó en
/// `transcribing`/`summarizing` sin ningún hilo que la avance. Al arrancar no
/// hay trabajo en curso, así que degradamos cada fila a un estado consistente
/// con lo que exista en disco para que el usuario pueda reintentar.
fn recover_orphaned_statuses(state: &AppState) {
    let recs = match state.db.lock_or_recover().list_recordings() {
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
        match state.db.lock_or_recover().update_status(&rec.id, next) {
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
    // Antes que nada: si algo de acá en adelante panica, queremos leerlo.
    // `AppDirs::new()` se llama otra vez más abajo, en el setup; es idempotente
    // —crea directorios y ya— y este orden es el que permite que un fallo del
    // arranque quede registrado en vez de perderse.
    let logs_dir = AppDirs::new()
        .map(|dirs| dirs.logs_dir())
        .unwrap_or_else(|_| std::env::temp_dir().join("atic-logs"));
    // El guard vive hasta el final de `run()`. Soltarlo antes se lleva puestas
    // las últimas líneas, que son las del cierre.
    let _log_guard = diagnostics::init(&logs_dir);

    // El estado se construye ANTES del Builder, no dentro de `setup()`.
    //
    // Las ventanas declaradas en `tauri.conf.json` nacen antes de que corra
    // `setup()`, y sus webviews empiezan a cargar enseguida: pueden invocar
    // comandos mientras `setup()` todavía está abriendo la base y leyendo la
    // config. El síntoma era la app arrancando con «state not managed for
    // field `state` on command `get_config`», intermitente según qué webview
    // ganara la carrera.
    //
    // Registrándolo en la cadena del Builder, `.manage()` corre antes de que
    // exista la primera ventana y la carrera deja de ser posible.
    let dirs = AppDirs::new().expect("no se pudo resolver el directorio de datos");
    let db = Db::open(&dirs.db_path()).expect("no se pudo abrir la base de datos");
    let config = Config::load(&dirs.config_path());
    let app_state = AppState {
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
    };

    tauri::Builder::default()
        .manage(app_state)
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
        .plugin(tauri_plugin_notification::init())
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
            overlay::overlay_rect,
            overlay::overlay_cursor,
            overlay::overlay_active_anchor,
            overlay::overlay_work_areas,
            overlay::save_pill_home,
            overlay::pill_home,
            overlay::set_overlay_hit_rects,
            overlay::set_overlay_css_viewport,
            overlay::set_overlay_item_drag,
            overlay::set_overlay_pointer_gesture,
            overlay::overlay_cursor_over_hit,
            overlay::overlay_primary_down,
            overlay::set_overlay_text_mode,
            commands::open_data_dir,
            commands::open_recording_dir,
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
            summarization::list_live_summary_models,
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
            capture_session::show_capture_overlay,
            capture_session::capture_overlay_revealed,
            capture_session::complete_window_capture,
            capture_session::complete_region_capture,
            capture_session::complete_monitor_capture,
            capture_session::cancel_capture_session,
            annotate::open_annotator,
            annotate::start_board,
            annotate::pending_annotation,
            annotate::annotation_image,
            annotate::close_annotator,
            annotate::save_annotation,
            annotate::copy_annotation,
            clipboard_history::list_clipboard_history,
            clipboard_history::paste_clipboard_item,
            clipboard_history::insert_clipboard_into_agents,
            clipboard_history::try_clipboard_drop_on_agents,
            clipboard_history::agents_window_visible,
            clipboard_history::clipboard_drag_path,
            clipboard_history::start_clipboard_text_drag,
            clipboard_history::read_clipboard_drag_text,
            clipboard_history::pin_clipboard_item,
            clipboard_history::delete_clipboard_item,
            clipboard_history::clear_clipboard_history,
            clipboard_history::prepare_clipboard_pill,
            clipboard_history::show_clipboard_window,
            clipboard_history::hide_clipboard_window,
            clipboard_history::clipboard_always_on_top,
            clipboard_history::set_clipboard_always_on_top,
            clipboard_history::stash_pill_home,
            clipboard_history::morph_pill_home,
            state::summon_pill_here,
            state::pill_trace,
            beep::preview_sound,
            beep::play_ui_sound,
            agents::bridge::show_agents_window,
            agents::bridge::hide_agents_window,
            agents::bridge::save_agents_bubble_size,
            agents::bridge::agents_always_on_top,
            agents::bridge::set_agents_always_on_top,
            agents::bridge::agent_set_model,
            agents::bridge::agent_backends,
            agents::bridge::agent_sessions,
            agents::bridge::agent_start,
            agents::bridge::agent_send,
            agents::bridge::agent_permission,
            agents::bridge::agent_skills,
            agents::bridge::agent_list_models,
            agents::bridge::agent_interrupt,
            agents::bridge::agent_stop,
            agents::bridge::agent_threads,
            agents::bridge::agent_thread,
            agents::bridge::agent_thread_delete,
            agents::bridge::agent_claude_sessions,
            agents::bridge::agent_claude_transcript,
            agents::bridge::agent_claude_usage,
            agents::presence::agent_presences,
            agents::presence::agent_presence_focus,
            agents::presence::agent_presence_bind,
            agents::presence::agent_presence_hook_snippet,
            agents::bridge::list_directories,
            agents::bridge::ssh_host_secrets_status,
            agents::bridge::ssh_set_host_secret,
            agents::bridge::ssh_delete_host_secrets,
            agents::bridge::ssh_test_host,
            agents::bridge::ssh_list_hosts,
            agents::console::console_open,
            agents::console::console_write,
            agents::console::console_resize,
            agents::console::console_close,
            agents::media::agent_stage_image,
            clipboard_history::restore_pill_position,
            snippets::list_snippets,
            snippets::upsert_snippet,
            snippets::delete_snippet,
            snippets::paste_snippet,
            snippets::get_scratchpad,
            snippets::set_scratchpad,
            snippets::list_notes,
            snippets::save_note,
            snippets::delete_note,
            snippets::prepare_snippets_pill,
            snippets::show_snippets_window,
            snippets::hide_snippets_window,
            snippets::snippets_always_on_top,
            snippets::set_snippets_always_on_top,
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
            launcher::toggle_launcher,
            launcher::show_launcher,
            launcher::hide_launcher,
            launcher::launcher_reindex,
            launcher::launcher_search,
            launcher::launcher_run,
            launcher::launcher_list_favorites,
            launcher::launcher_toggle_favorite,
            launcher::launcher_icon,
        ])
        .setup(move |app| {
            // El estado ya está registrado por el Builder: acá solo se lee.
            let dirs = app.state::<AppState>().dirs.clone();

            // Ocultar YA las ventanas auxiliares: nacen con el Builder, antes
            // de este `setup`, y si quedan visibles un instante se ve el
            // lienzo de anotar / el shelf / el launcher. `visible: false` en
            // la config es la barrera; esto cubre si algún runtime la ignora.
            for label in ["capture-shelf", "launcher", annotate::ANNOTATE_LABEL] {
                if let Some(window) = app.get_webview_window(label) {
                    let _ = window.hide();
                }
            }

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

            // Una sola toma del lock: la config se lee entera y se suelta.
            let (
                shortcut,
                dictation_shortcut,
                summon_pill_shortcut,
                pill_radial_shortcut,
                clipboard_shortcut,
                snippets_shortcut,
                agents_shortcut,
                screenshot_shortcut,
                board_shortcut,
                launcher_shortcut,
                want_autostart,
            ) = {
                let cfg = app.state::<AppState>();
                let cfg = cfg.config.lock_or_recover();
                (
                    cfg.global_shortcut.clone(),
                    cfg.dictation_shortcut.clone(),
                    cfg.summon_pill_shortcut.clone(),
                    cfg.pill_radial_shortcut.clone(),
                    cfg.clipboard_shortcut.clone(),
                    cfg.snippets_shortcut.clone(),
                    cfg.agents_shortcut.clone(),
                    cfg.screenshot_shortcut.clone(),
                    cfg.board_shortcut.clone(),
                    cfg.launcher_shortcut.clone(),
                    cfg.autostart,
                )
            };
            // `pill_position` y `show_pill` ya no se aplican desde acá: la pill
            // es un div del overlay y los lee ella misma (`pill_home()` y
            // `getConfig()`).

            // Repara estados transitorios huérfanos de un cierre abrupto anterior.
            recover_orphaned_statuses(&app.state::<AppState>());
            retention::run_auto_cleanup(app.handle());
            capture::run_capture_cleanup(app.handle());
            meeting_detection::spawn_detector(app.handle().clone());

            // Precarga Whisper en background para que el primer dictado no espere
            // la carga del GGML desde disco.
            state::preload_whisper_async(app.handle());

            // Precarga catálogos de modelos de agentes (Cursor, Claude, …)
            // para que el selector no espere al abrir la consola.
            agents::discover::preload_models_async();

            tray::build_tray(app.handle())?;

            // Sincronizar autostart con la preferencia guardada.
            //
            // Solo se toca el registro si el estado real difiere del deseado.
            // Antes se llamaba a `disable()` en cada arranque aunque ya
            // estuviera deshabilitado, y eso falla con "no se encuentra el
            // archivo" (os error 2) porque no hay entrada que borrar: un aviso
            // en cada inicio que no significaba nada y tapaba los reales.
            {
                use tauri_plugin_autostart::ManagerExt;
                let manager = app.autolaunch();
                match manager.is_enabled() {
                    Ok(actual) if actual == want_autostart => {}
                    Ok(_) => {
                        let result = if want_autostart {
                            manager.enable()
                        } else {
                            manager.disable()
                        };
                        if let Err(err) = result {
                            tracing::warn!(%err, "no se pudo sincronizar autostart al iniciar");
                        }
                    }
                    Err(err) => {
                        tracing::warn!(%err, "no se pudo leer el estado de autostart");
                    }
                }
            }

            // Mouse lateral: Raw Input (pasivo; no puede congelar el ratón del SO).
            mouse_bindings::init(app.handle());

            // Atajos globales: grabación + dictado + pill + clipboard + agentes + captura + launcher.
            if let Err(err) = shortcuts::register_shortcuts(
                app.handle(),
                shortcuts::ShortcutBindings {
                    recording: &shortcut,
                    dictation: &dictation_shortcut,
                    summon_pill: &summon_pill_shortcut,
                    pill_radial: &pill_radial_shortcut,
                    clipboard: &clipboard_shortcut,
                    snippets: &snippets_shortcut,
                    agents: &agents_shortcut,
                    screenshot: &screenshot_shortcut,
                    board: &board_shortcut,
                    launcher: &launcher_shortcut,
                },
            ) {
                tracing::error!(%err, "no se pudieron registrar los atajos globales");
            }

            launcher::start_indexing();
            clipboard_history::start_watcher(app.handle());
            agents::watch_claude::start(app.handle());
            agents::watch_codex::start(app.handle());
            agents::watch_cursor::start(app.handle());
            agents::watch_opencode::start(app.handle());

            // Preferencias de pin de floats (antes de crear el overlay).
            {
                let state = app.state::<AppState>();
                let cfg = state.config.lock_or_recover();
                agents::bridge::init_always_on_top(cfg.agents_always_on_top);
                clipboard_history::init_always_on_top(cfg.clipboard_always_on_top);
                snippets::init_always_on_top(cfg.snippets_always_on_top);
            }

            // El overlay va DESPUÉS de la pill: elige monitor mirando dónde
            // quedó ella.
            overlay::setup(app.handle());

            // Sin Ctrl+P / Find / zoom del WebView2, también en `main`.
            // Después de crear el overlay: si no, esa ventana no existe aún.
            webview_tweaks::apply_to_all_windows(app.handle());
            if let Some(main) = app.get_webview_window("main") {
                let _ = main.set_ignore_cursor_events(false);
            }

            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Cerrar oculta, no destruye.
            WindowEvent::CloseRequested { api, .. }
                if window.label() == "main"
                    || window.label() == "launcher"
                    || window.label() == annotate::ANNOTATE_LABEL =>
            {
                api.prevent_close();
                let _ = window.hide();
            }
            // Tras una captura, `main` puede quedar click-through. Al enfocarla
            // se restaura el mouse. Reaplica atajos de WebView2 por si el
            // webview no estaba listo en el setup.
            WindowEvent::Focused(true) if window.label() == "main" => {
                if let Some(main) = window.app_handle().get_webview_window("main") {
                    let _ = main.set_ignore_cursor_events(false);
                    webview_tweaks::disable_browser_accelerator_keys(&main);
                }
                overlay::yield_to_main(window.app_handle());
            }
            // La pill ya no es una ventana, así que nadie avisa cuando se mueve:
            // la persiste `save_pill_home` al soltar el arrastre.
            _ => {}
        })
        .build(tauri::generate_context!())
        .expect("error al iniciar Atic")
        .run(move |app, event| {
            if let RunEvent::Exit = event {
                // Los agentes son procesos externos: si Atic se va sin cerrarlos,
                // quedan corriendo y consumiendo tokens sin nadie mirando.
                agents::bridge::stop_all(app);
                if let Some(state) = app.try_state::<AppState>() {
                    let cfg = state.config.lock_or_recover().clone();
                    let _ = cfg.save(&state.dirs.config_path());
                }
            }
        });
}
