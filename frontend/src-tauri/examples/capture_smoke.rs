//! Headless smoke test of the Windows runtime paths, using the app's own code: credential
//! read, device list, capture → pipeline → `audio.mp4`, live sessions, final transcript.
//!
//! `capture_smoke <report.json> [seconds=25] [default|app:chrome]`
//!
//! Must run in the logged-on desktop session (Credential Manager, audio devices) with
//! `ffmpeg` next to the exe or on `PATH`. The meeting folder and database go to
//! `<report>.work/`. The API key is never printed.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use app_lib::audio::audio_processing::create_meeting_folder;
use app_lib::audio::devices::default_input_device;
use app_lib::audio::import::validate_audio_file;
use app_lib::audio::levels::AudioLevels;
use app_lib::audio::recording_commands::{get_audio_devices, resolve_system_or_default};
use app_lib::audio::recording_manager::{LiveSetup, RecordingManager};
use app_lib::audio::recording_saver::MeetingTarget;
use app_lib::database::manager::DatabaseManager;
use app_lib::database::models::{meeting_status, transcript_source};
use app_lib::database::repositories::meeting::{MeetingsRepository, NewMeeting};
use app_lib::database::repositories::transcript::TranscriptsRepository;
use app_lib::final_transcript::{run_final_transcript, FinalError, FinalJob};
use app_lib::live::{LiveConfig, LiveInterim, LiveLine, LiveSink, LiveStatus};
use app_lib::voice::credential::{self, KeyStatus};
use app_lib::voice::service::VoiceService;
use app_lib::voice::settings::VoiceSettings;
use peacockery_voice::CancellationToken;
use serde_json::{json, Map, Value};

static STARTED: Mutex<Option<Instant>> = Mutex::new(None);
static PROBLEMS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn elapsed() -> f64 {
    let mut started = STARTED.lock().unwrap();
    started.get_or_insert_with(Instant::now).elapsed().as_secs_f64()
}

fn problem(message: String) {
    eprintln!("[{:7.2}] PROBLEM {}", elapsed(), message);
    PROBLEMS.lock().unwrap().push(format!("{:.2}s {}", elapsed(), message));
}

/// stderr log; warnings and errors also go to the report.
struct Logger;

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        eprintln!("[{:7.2}] {:5} {} {}", elapsed(), record.level(), record.target(), record.args());
        if record.level() <= log::Level::Warn {
            PROBLEMS.lock().unwrap().push(format!(
                "{:.2}s {} {}: {}",
                elapsed(),
                record.level(),
                record.target(),
                record.args()
            ));
        }
    }

    fn flush(&self) {}
}

/// Collects what the webview would receive, stamped with seconds since recording start.
#[derive(Default)]
struct Collector {
    recording_started: Mutex<Option<Instant>>,
    statuses: Mutex<Vec<Value>>,
    lines: Mutex<Vec<Value>>,
    interims: Mutex<u64>,
}

impl Collector {
    fn stamp(&self, value: impl serde::Serialize) -> Value {
        let at = self.recording_started.lock().unwrap().map(|t| t.elapsed().as_secs_f64());
        json!({ "received_at_seconds": at, "payload": value })
    }
}

impl LiveSink for Collector {
    fn status(&self, status: &LiveStatus) {
        self.statuses.lock().unwrap().push(self.stamp(status));
    }

    fn interim(&self, _interim: &LiveInterim) {
        *self.interims.lock().unwrap() += 1;
    }

    fn line(&self, line: &LiveLine) {
        self.lines.lock().unwrap().push(self.stamp(line));
    }
}

#[derive(Default, Clone, Copy)]
struct LevelSummary {
    frames: u64,
    mic_max: f32,
    system_max: f32,
    system_silent_seconds: f64,
}

async fn run(report: &mut Map<String, Value>, work: &Path, seconds: u64, system: &str) -> Result<(), String> {
    let key = credential::read_key()?;
    let status = KeyStatus::of(key.as_deref());
    report.insert(
        "credential".into(),
        json!({
            "target": credential::TARGET,
            "configured": status.configured,
            "masked": status.masked,
            "valid_shape": key.as_deref().map(|k| credential::normalize_key(k).is_ok()),
        }),
    );
    drop(key);

    let devices = get_audio_devices().await?;
    report.insert("devices".into(), json!(devices));

    let settings = VoiceSettings::default();
    let service = VoiceService::new(settings.clone(), None);
    let microphone = default_input_device().map_err(|e| format!("No default microphone: {e}"))?;
    let requested = (system != "default").then(|| format!("{system} (output)"));
    let system_source = resolve_system_or_default(requested.as_deref())?;
    report.insert(
        "sources".into(),
        json!({ "microphone": microphone.name, "system": format!("{system_source:?}") }),
    );

    std::fs::create_dir_all(work).map_err(|e| format!("Cannot create {}: {e}", work.display()))?;
    let database = DatabaseManager::open(&work.join("smoke.sqlite")).await?;
    let pool = database.pool().clone();
    let title = format!("Smoke {system}");
    let folder = create_meeting_folder(work, &title).map_err(|e| format!("Meeting folder: {e}"))?;
    let meeting_id = format!("meeting-{}", uuid::Uuid::new_v4());
    let folder_path = folder.to_string_lossy().to_string();
    MeetingsRepository::create(
        &pool,
        &NewMeeting { id: &meeting_id, title: &title, folder_path: &folder_path, status: meeting_status::RECORDING },
    )
    .await
    .map_err(|e| format!("Create meeting: {e}"))?;

    let collector = Arc::new(Collector::default());
    let levels = Arc::new(Mutex::new(LevelSummary::default()));
    let levels_in = levels.clone();
    let live = LiveSetup {
        config: LiveConfig::from_settings(&meeting_id, &settings),
        client: service.client(),
        sink: collector.clone(),
        pool: Some(pool.clone()),
    };
    let target = MeetingTarget { meeting_id: meeting_id.clone(), meeting_name: title, folder: folder.clone() };
    let mut manager = RecordingManager::start(
        target,
        Some(Arc::new(microphone)),
        system_source,
        Some(live),
        Box::new(move |frame: AudioLevels| {
            let mut summary = levels_in.lock().unwrap();
            summary.frames += 1;
            summary.mic_max = summary.mic_max.max(frame.mic);
            summary.system_max = summary.system_max.max(frame.system);
            summary.system_silent_seconds = frame.system_silent_seconds;
        }),
    )
    .await
    .map_err(|e| format!("Failed to start recording: {e:#}"))?;
    manager.set_error_callback(|error| problem(format!("recording-error: {}", error.user_message())));
    let recording_started = Instant::now();
    *collector.recording_started.lock().unwrap() = Some(recording_started);

    tokio::time::sleep(Duration::from_secs(seconds)).await;

    let wall_seconds = recording_started.elapsed().as_secs_f64();
    let stopped = manager.stop_and_save().await;
    let stop_seconds = recording_started.elapsed().as_secs_f64() - wall_seconds;
    drop(manager);

    let summary = *levels.lock().unwrap();
    let mut recording = json!({
        "wall_seconds": wall_seconds,
        "stop_and_save_seconds": stop_seconds,
        "active_seconds": stopped.duration_seconds,
        "pipeline": stopped.stats,
        "aligned_seconds": stopped.stats.map(|s| s.aligned_samples as f64 / 48000.0),
        "level_frames": summary.frames,
        "mic_level_max": summary.mic_max,
        "system_level_max": summary.system_max,
        "system_silent_seconds_at_end": summary.system_silent_seconds,
    });
    if let Some(live) = &stopped.live {
        let ids = serde_json::to_string(&live.session_ids).unwrap_or_else(|_| "[]".to_string());
        if let Err(e) = MeetingsRepository::set_live_info(&pool, &meeting_id, &live.model, live.mode, &ids).await {
            problem(format!("set_live_info: {e}"));
        }
        recording["live_summary"] = json!({ "model": live.model, "mode": live.mode, "session_ids": live.session_ids });
    }
    if let Err(e) = MeetingsRepository::mark_stopped(&pool, &meeting_id, stopped.duration_seconds).await {
        problem(format!("mark_stopped: {e}"));
    }
    let audio = match stopped.audio_path {
        Ok(path) => {
            recording["audio_path"] = json!(path);
            match validate_audio_file(&path) {
                Ok(info) => {
                    recording["audio_file_seconds"] = json!(info.duration_seconds);
                    recording["audio_file_bytes"] = json!(info.size_bytes);
                }
                Err(e) => problem(format!("audio.mp4 does not probe: {e:#}")),
            }
            Some(path)
        }
        Err(e) => {
            problem(format!("audio could not be saved: {e:#}"));
            None
        }
    };
    report.insert("recording".into(), recording);
    report.insert(
        "live".into(),
        json!({
            "statuses": *collector.statuses.lock().unwrap(),
            "interim_count": *collector.interims.lock().unwrap(),
            "lines": *collector.lines.lock().unwrap(),
        }),
    );

    let Some(audio) = audio else {
        return Err("No audio file, final transcript skipped".to_string());
    };
    let meeting = MeetingsRepository::get(&pool, &meeting_id)
        .await
        .map_err(|e| format!("Read meeting: {e}"))?
        .ok_or("Meeting row missing")?;
    let job = FinalJob::new(&meeting, folder, audio, &settings.final_model, &settings);
    let final_started = Instant::now();
    let progress = Arc::new(Mutex::new(Vec::new()));
    let progress_in = progress.clone();
    let outcome = run_final_transcript(&service.client()?, &pool, &job, &CancellationToken::new(), move |p| {
        progress_in.lock().unwrap().push(format!("{:.1}s {:?}", final_started.elapsed().as_secs_f64(), p));
    })
    .await;
    let mut final_report = json!({
        "model": settings.final_model,
        "seconds": final_started.elapsed().as_secs_f64(),
        "progress": *progress.lock().unwrap(),
    });
    match outcome {
        Ok(outcome) => {
            final_report["job_id"] = json!(outcome.job_id);
            final_report["segments_count"] = json!(outcome.segments_count);
            final_report["turn_source"] = json!(format!("{:?}", outcome.turn_source));
            let rows = TranscriptsRepository::all(&pool, &meeting_id, transcript_source::FINAL)
                .await
                .map_err(|e| format!("Read final rows: {e}"))?;
            final_report["rows"] = json!(rows
                .iter()
                .map(|r| json!({
                    "speaker": r.speaker,
                    "timestamp": r.timestamp,
                    "start": r.audio_start_time,
                    "end": r.audio_end_time,
                    "text": r.transcript,
                }))
                .collect::<Vec<_>>());
            final_report["text"] = json!(rows.iter().map(|r| r.transcript.as_str()).collect::<Vec<_>>().join(" "));
        }
        Err(FinalError::Cancelled) => problem("final transcript cancelled".to_string()),
        Err(FinalError::Failed(message)) => problem(format!("final transcript failed: {message}")),
    }
    report.insert("final".into(), final_report);
    database.cleanup().await;
    Ok(())
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(report_path) = args.first().map(PathBuf::from) else {
        eprintln!("usage: capture_smoke <report.json> [seconds=25] [default|app:chrome]");
        std::process::exit(2);
    };
    let seconds = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(25);
    let system = args.get(2).map(String::as_str).unwrap_or("default");
    elapsed();
    log::set_logger(&Logger).expect("logger");
    log::set_max_level(log::LevelFilter::Info);

    let mut report = Map::new();
    report.insert("args".into(), json!({ "seconds": seconds, "system": system }));
    let work = report_path.with_extension("work");
    if let Err(message) = run(&mut report, &work, seconds, system).await {
        problem(format!("FATAL {message}"));
    }
    report.insert("total_seconds".into(), json!(elapsed()));
    report.insert("problems".into(), json!(*PROBLEMS.lock().unwrap()));
    let text = serde_json::to_string_pretty(&Value::Object(report)).expect("report json");
    if let Err(e) = std::fs::write(&report_path, text) {
        eprintln!("Cannot write {}: {e}", report_path.display());
        std::process::exit(1);
    }
}
