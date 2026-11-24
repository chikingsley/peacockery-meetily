//! Peacockery Meetily: Tauri core (capture, recording, SQLite, commands, events).

pub mod api;
pub mod audio;
mod briefs;
pub mod database;
pub mod final_transcript;
pub mod live;
mod outlook_calendar;
mod outlook_secret;
pub mod publication;
mod recording_context;
pub mod state;
pub mod summary;
pub mod tray;
mod updates;
pub mod utils;
pub mod voice;

use log::LevelFilter;
use tauri::Manager;

use database::manager::DatabaseManager;
use state::AppState;
use voice::VoiceService;

/// `RUST_LOG` when it is a plain level (`debug`, `trace`, …), else Info.
fn log_level() -> LevelFilter {
    std::env::var("RUST_LOG")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(LevelFilter::Info)
}

fn log_plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    use tauri_plugin_log::{RotationStrategy, Target, TargetKind};
    tauri_plugin_log::Builder::new()
        .clear_targets()
        .target(Target::new(TargetKind::LogDir {
            file_name: Some("peacockery-meetily".into()),
        }))
        .target(Target::new(TargetKind::Stdout))
        .level(log_level())
        .level_for("sqlx", LevelFilter::Warn)
        .level_for("tao", LevelFilter::Warn)
        .level_for("wry", LevelFilter::Warn)
        .max_file_size(10 * 1024 * 1024)
        .rotation_strategy(RotationStrategy::KeepSome(5))
        .build()
}

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    log::info!("Peacockery Meetily {} starting", app.package_info().version);

    let db_manager = tauri::async_runtime::block_on(DatabaseManager::open_for_app(app.handle()))?;
    let pool = db_manager.pool().clone();
    app.manage(AppState { db_manager });
    app.manage(VoiceService::load(app.handle()));
    audio::recording_preferences::allow_recordings_folder(app.handle());
    publication::start(app.handle());
    summary::review::start(app.handle());

    let handle = app.handle().clone();
    tauri::async_runtime::spawn(async move {
        // Journals are registered first, so a resumed final transcript waits for its audio;
        // the recovered meetings start their work after the resume, so none runs twice.
        let recovery = audio::recovery::register_journals(&pool).await;
        final_transcript::resume_unfinished(&handle, &pool).await;
        audio::recovery::resume(&handle, recovery);
    });

    if let Err(e) = tray::create_tray(app.handle()) {
        log::error!("Failed to create system tray: {}", e);
    }
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        // First, so a second launch hands over to the running instance before any other
        // plugin of the new process starts.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::focus_main_window(app);
        }))
        .plugin(log_plugin())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .setup(setup)
        .on_window_event(|window, event| {
            // Closing the main window hides it to the tray; recording keeps running.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    if let Err(e) = window.hide() {
                        log::error!("Failed to hide main window: {}", e);
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            updates::check_app_update,
            updates::install_app_update,
            // Recording
            audio::recording_commands::get_audio_devices,
            audio::recording_commands::start_recording_with_devices_and_meeting,
            audio::recording_commands::stop_recording,
            audio::recording_commands::pause_recording,
            audio::recording_commands::resume_recording,
            audio::recording_commands::get_recording_state,
            audio::recording_commands::get_transcript_history,
            audio::recording_commands::start_audio_level_monitoring,
            audio::recording_commands::stop_audio_level_monitoring,
            audio::recording_preferences::get_recording_preferences,
            audio::recording_preferences::set_recording_preferences,
            audio::recording_preferences::get_default_recordings_folder_path,
            audio::recording_preferences::open_recordings_folder,
            // Meetings / transcripts
            api::api_get_meetings,
            api::api_search_transcripts,
            api::api_get_meeting_metadata,
            api::api_get_meeting_transcripts,
            api::api_save_meeting_title,
            api::api_delete_meeting,
            api::open_meeting_folder,
            api::rename_speaker,
            summary::review::meeting_review,
            briefs::meeting_brief,
            briefs::save_meeting_brief,
            briefs::dial_phone,
            briefs::call_attempts,
            briefs::set_call_outcome,
            recording_context::get_recording_calendar,
            outlook_calendar::outlook_status,
            outlook_calendar::outlook_connect,
            outlook_calendar::outlook_select,
            outlook_calendar::outlook_disconnect,
            publication::publication_default,
            publication::publication_set_default,
            publication::publication_status,
            publication::publication_retry,
            publication::publication_open,
            final_transcript::start_final_transcript,
            final_transcript::get_final_transcript_status,
            // Voice settings
            voice::activation::activate_access,
            voice::commands::voice_get_settings,
            voice::commands::voice_save_settings,
            voice::commands::voice_get_key_status,
            voice::commands::voice_test_connection,
            voice::commands::voice_list_models,
            // Import
            audio::import::select_and_validate_audio_command,
            audio::import::validate_audio_file_command,
            audio::import::start_import_audio_command,
            audio::import::cancel_import_command,
            // Summaries
            summary::commands::api_process_transcript,
            summary::commands::api_get_summary,
            summary::commands::api_save_meeting_summary,
            summary::commands::api_cancel_summary,
            summary::template_commands::api_list_templates,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| match event {
            // Never exit over a running recording or an audio save: quit stops the recording
            // and waits for the saves, then exits.
            tauri::RunEvent::ExitRequested { api, .. }
                if !tray::exit_ready()
                    && (audio::recording_commands::is_recording_active()
                        || audio::audio_save::pending() > 0) =>
            {
                api.prevent_exit();
                tray::quit(app_handle);
            }
            tauri::RunEvent::Exit => {
                if let Some(app_state) = app_handle.try_state::<AppState>() {
                    tauri::async_runtime::block_on(app_state.db_manager.cleanup());
                }
            }
            _ => {}
        });
}
