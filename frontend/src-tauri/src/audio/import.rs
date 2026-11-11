//! Import an external audio file as a new meeting. The file is copied into a new meeting
//! folder as `audio.<ext>`, the meeting row is created (status `imported`) and the
//! final transcript is started.

use anyhow::{anyhow, Result};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;

use super::audio_processing::create_meeting_folder;
use super::constants::AUDIO_EXTENSIONS;
use crate::database::models::meeting_status;
use crate::database::repositories::meeting::{MeetingsRepository, NewMeeting};
use crate::state::AppState;

static IMPORT_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
static IMPORT_CANCELLED: AtomicBool = AtomicBool::new(false);

/// Maximum import size (the Peacockery Voice multipart upload handles large files).
const MAX_FILE_SIZE_BYTES: u64 = 20 * 1024 * 1024 * 1024;
const COPY_BUFFER_BYTES: usize = 4 * 1024 * 1024;

/// Clears `IMPORT_IN_PROGRESS` on every exit path.
struct ImportGuard;

impl ImportGuard {
    fn acquire() -> Result<Self, String> {
        IMPORT_IN_PROGRESS
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map(|_| ImportGuard)
            .map_err(|_| "Import already in progress".to_string())
    }
}

impl Drop for ImportGuard {
    fn drop(&mut self) {
        IMPORT_IN_PROGRESS.store(false, Ordering::SeqCst);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioFileInfo {
    pub path: String,
    pub filename: String,
    pub duration_seconds: f64,
    pub size_bytes: u64,
    pub format: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportProgress {
    pub meeting_id: String,
    pub stage: String,
    pub progress_percentage: u32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportStarted {
    pub meeting_id: String,
}

/// Check that `path` is an importable audio file and read its duration.
pub fn validate_audio_file(path: &Path) -> Result<AudioFileInfo> {
    if !path.is_file() {
        return Err(anyhow!("File does not exist: {}", path.display()));
    }
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    if !AUDIO_EXTENSIONS.contains(&extension.as_str()) {
        return Err(anyhow!(
            "Unsupported format: .{}. Supported: {}",
            extension,
            AUDIO_EXTENSIONS.join(", ")
        ));
    }
    let size_bytes = std::fs::metadata(path)
        .map_err(|e| anyhow!("Cannot read file: {}", e))?
        .len();
    if size_bytes > MAX_FILE_SIZE_BYTES {
        return Err(anyhow!(
            "File too large: {:.2} GB. Maximum supported size is {} GB",
            size_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
            MAX_FILE_SIZE_BYTES / (1024 * 1024 * 1024)
        ));
    }
    let filename = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Imported Audio")
        .to_string();

    Ok(AudioFileInfo {
        path: path.to_string_lossy().to_string(),
        filename,
        duration_seconds: probe_duration(path)?,
        size_bytes,
        format: extension.to_uppercase(),
    })
}

/// Duration from container metadata, else by walking packet durations (no decoding).
fn probe_duration(path: &Path) -> Result<f64> {
    use symphonia::core::codecs::CODEC_TYPE_NULL;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let file = std::fs::File::open(path).map_err(|e| anyhow!("Failed to open audio file: {}", e))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| anyhow!("Not a readable audio file: {}", e))?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| anyhow!("No audio track found in file"))?;
    let track_id = track.id;
    let params = track.codec_params.clone();

    if let (Some(time_base), Some(frames)) = (params.time_base, params.n_frames) {
        let time = time_base.calc_time(frames);
        return Ok(time.seconds as f64 + time.frac);
    }
    if let (Some(rate), Some(frames)) = (params.sample_rate, params.n_frames) {
        return Ok(frames as f64 / rate as f64);
    }

    let time_base = params
        .time_base
        .or_else(|| params.sample_rate.map(|rate| symphonia::core::units::TimeBase::new(1, rate)))
        .ok_or_else(|| anyhow!("Audio track has no time base"))?;
    let mut end_ts: u64 = 0;
    while let Ok(packet) = format.next_packet() {
        if packet.track_id() == track_id {
            end_ts = end_ts.max(packet.ts() + packet.dur());
        }
    }
    if end_ts == 0 {
        return Err(anyhow!("Audio file contains no audio packets"));
    }
    let time = time_base.calc_time(end_ts);
    Ok(time.seconds as f64 + time.frac)
}

fn emit_progress(app: &AppHandle, meeting_id: &str, stage: &str, progress: u32, message: &str) {
    let _ = app.emit(
        "import-progress",
        ImportProgress {
            meeting_id: meeting_id.to_string(),
            stage: stage.to_string(),
            progress_percentage: progress,
            message: message.to_string(),
        },
    );
}

/// Copy `source` to `destination` in chunks, honouring cancellation and reporting progress.
fn copy_with_progress(source: &Path, destination: &Path, mut on_progress: impl FnMut(u32)) -> Result<()> {
    let total = std::fs::metadata(source)?.len().max(1);
    let mut reader = std::fs::File::open(source)?;
    let mut writer = std::fs::File::create(destination)?;
    let mut buffer = vec![0u8; COPY_BUFFER_BYTES];
    let mut copied: u64 = 0;
    let mut last_reported = 0;
    loop {
        if IMPORT_CANCELLED.load(Ordering::SeqCst) {
            return Err(anyhow!("Import cancelled"));
        }
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        writer.write_all(&buffer[..read])?;
        copied += read as u64;
        let percent = ((copied * 100) / total) as u32;
        if percent >= last_reported + 5 {
            last_reported = percent;
            on_progress(percent);
        }
    }
    writer.sync_all()?;
    Ok(())
}

async fn run_import(app: &AppHandle, meeting_id: &str, source: PathBuf, title: String, folder: PathBuf) -> Result<()> {
    let extension = source
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .ok_or_else(|| anyhow!("Source file has no extension"))?;
    let destination = folder.join(format!("audio.{}", extension));

    emit_progress(app, meeting_id, "copying", 0, "Copying audio file...");
    let progress_app = app.clone();
    let progress_id = meeting_id.to_string();
    tokio::task::spawn_blocking(move || {
        copy_with_progress(&source, &destination, |percent| {
            emit_progress(&progress_app, &progress_id, "copying", percent, "Copying audio file...");
        })
    })
    .await
    .map_err(|e| anyhow!("Copy task failed: {}", e))??;

    let pool = app.state::<AppState>().db_manager.pool().clone();
    let folder_path = folder.to_string_lossy().to_string();
    MeetingsRepository::create(
        &pool,
        &NewMeeting { id: meeting_id, title: &title, folder_path: &folder_path, status: meeting_status::IMPORTED },
    )
    .await
    .map_err(|e| anyhow!("Failed to create meeting: {}", e))?;
    emit_progress(app, meeting_id, "copying", 100, "Audio imported");
    Ok(())
}

// ============================================================================
// TAURI COMMANDS
// ============================================================================

/// Pick an audio file and validate it.
#[tauri::command]
pub async fn select_and_validate_audio_command(app: AppHandle) -> Result<Option<AudioFileInfo>, String> {
    let dialog_app = app.clone();
    let file_path = tokio::task::spawn_blocking(move || {
        dialog_app
            .dialog()
            .file()
            .add_filter("Audio Files", AUDIO_EXTENSIONS)
            .blocking_pick_file()
    })
    .await
    .map_err(|e| format!("File dialog task failed: {}", e))?;

    let Some(path) = file_path else {
        return Ok(None);
    };
    let path = path.into_path().map_err(|e| format!("Unsupported file path: {}", e))?;
    validate_audio_file(&path).map(Some).map_err(|e| e.to_string())
}

/// Validate an audio file (drag and drop).
#[tauri::command]
pub async fn validate_audio_file_command(path: String) -> Result<AudioFileInfo, String> {
    validate_audio_file(Path::new(&path)).map_err(|e| e.to_string())
}

/// Import `source_path` as a new meeting. Returns the new meeting id immediately; the
/// copy runs in the background (`import-progress`, then `import-complete {meeting_id}` or
/// `import-error`), after which the final transcript starts.
#[tauri::command]
pub async fn start_import_audio_command(
    app: AppHandle,
    source_path: String,
    title: Option<String>,
) -> Result<ImportStarted, String> {
    let guard = ImportGuard::acquire()?;
    IMPORT_CANCELLED.store(false, Ordering::SeqCst);

    let source = PathBuf::from(&source_path);
    let info = validate_audio_file(&source).map_err(|e| e.to_string())?;
    let title = title
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or(info.filename);
    let base_folder = super::recording_preferences::recordings_folder(&app)?;
    let folder = create_meeting_folder(&base_folder, &title).map_err(|e| format!("Failed to create meeting folder: {}", e))?;
    let meeting_id = format!("meeting-{}", uuid::Uuid::new_v4());
    info!("Importing {} as meeting {} ({})", source.display(), meeting_id, title);

    let task_app = app.clone();
    let task_meeting_id = meeting_id.clone();
    tauri::async_runtime::spawn(async move {
        let _guard = guard;
        match run_import(&task_app, &task_meeting_id, source, title, folder.clone()).await {
            Ok(()) => {
                let _ = task_app.emit("import-complete", serde_json::json!({ "meeting_id": task_meeting_id }));
                crate::final_transcript::start_for_meeting(&task_app, &task_meeting_id, None);
            }
            Err(e) => {
                error!("Import of meeting {} failed: {:#}", task_meeting_id, e);
                if let Err(remove_error) = std::fs::remove_dir_all(&folder) {
                    warn!("Failed to remove {}: {}", folder.display(), remove_error);
                }
                let _ = task_app.emit(
                    "import-error",
                    serde_json::json!({ "meeting_id": task_meeting_id, "error": e.to_string() }),
                );
            }
        }
    });

    Ok(ImportStarted { meeting_id })
}

#[tauri::command]
pub async fn cancel_import_command() -> Result<(), String> {
    if !IMPORT_IN_PROGRESS.load(Ordering::SeqCst) {
        return Err("No import in progress".to_string());
    }
    IMPORT_CANCELLED.store(true, Ordering::SeqCst);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/he_aac_48k_5s.m4a");

    #[test]
    fn validates_an_aac_fixture() {
        let info = validate_audio_file(Path::new(FIXTURE)).unwrap();
        assert_eq!(info.format, "M4A");
        assert_eq!(info.filename, "he_aac_48k_5s");
        assert!((info.duration_seconds - 5.0).abs() < 0.2, "duration {}", info.duration_seconds);
        assert!(info.size_bytes > 0);
    }

    #[test]
    fn rejects_missing_and_unsupported_files() {
        assert!(validate_audio_file(Path::new("/nonexistent/file.mp4"))
            .unwrap_err()
            .to_string()
            .contains("does not exist"));
        let dir = tempfile::tempdir().unwrap();
        let text = dir.path().join("notes.txt");
        std::fs::write(&text, b"hello").unwrap();
        assert!(validate_audio_file(&text).unwrap_err().to_string().contains("Unsupported format"));
        let fake = dir.path().join("fake.mp3");
        std::fs::write(&fake, b"not audio").unwrap();
        assert!(validate_audio_file(&fake).is_err());
    }

    #[test]
    fn copy_reports_progress_and_honours_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("in.bin");
        std::fs::write(&source, vec![7u8; COPY_BUFFER_BYTES * 3]).unwrap();
        let destination = dir.path().join("out.bin");

        IMPORT_CANCELLED.store(false, Ordering::SeqCst);
        let mut reports = Vec::new();
        copy_with_progress(&source, &destination, |p| reports.push(p)).unwrap();
        assert_eq!(std::fs::read(&destination).unwrap().len(), COPY_BUFFER_BYTES * 3);
        assert_eq!(reports.last(), Some(&100));

        IMPORT_CANCELLED.store(true, Ordering::SeqCst);
        assert!(copy_with_progress(&source, &destination, |_| {}).is_err());
        IMPORT_CANCELLED.store(false, Ordering::SeqCst);
    }
}
