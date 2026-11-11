//! Live end-to-end check of the final-transcript core against the lab API (no GUI).
//!
//! ```text
//! PEACOCKERY_VOICE_LAB_API_KEY=… cargo test -p peacockery-meetily --test final_live -- --ignored --nocapture
//! ```

use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};

use app_lib::database::manager::DatabaseManager;
use app_lib::database::models::meeting_status;
use app_lib::database::repositories::meeting::{MeetingsRepository, NewMeeting};
use app_lib::database::repositories::transcript::TranscriptsRepository;
use app_lib::final_transcript::{run_final_transcript, FinalJob, FinalProgress, ARTIFACT_FILE, TRANSCRIPT_FILE};
use app_lib::voice::VoiceSettings;
use peacockery_voice::{CancellationToken, ClientConfig, VoiceClient, DEFAULT_BASE_URL};

const DIALOGUE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../crates/peacockery-voice/tests/fixtures/dialogue.wav");

/// The recording format: AAC 96 kbps mono in `audio.mp4`.
fn encode_like_a_recording(source: &Path, destination: &Path) {
    let status = Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(source)
        .args(["-t", "45", "-ac", "1", "-ar", "48000", "-c:a", "aac", "-b:a", "96k"])
        .arg(destination)
        .status()
        .expect("ffmpeg on PATH");
    assert!(status.success(), "ffmpeg failed");
}

#[tokio::test]
#[ignore = "calls the live Peacockery Voice lab API"]
async fn final_transcript_end_to_end() {
    let key = std::env::var("PEACOCKERY_VOICE_LAB_API_KEY").expect("PEACOCKERY_VOICE_LAB_API_KEY");
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("Call_2026-11-10_09-00");
    std::fs::create_dir_all(&folder).unwrap();
    let audio = folder.join("audio.mp4");
    encode_like_a_recording(Path::new(DIALOGUE), &audio);

    let db = DatabaseManager::open(&dir.path().join("test.sqlite")).await.unwrap();
    let pool = db.pool().clone();
    MeetingsRepository::create(
        &pool,
        &NewMeeting { id: "m-live", title: "Call", folder_path: &folder.to_string_lossy(), status: meeting_status::STOPPED },
    )
    .await
    .unwrap();
    let meeting = MeetingsRepository::get(&pool, "m-live").await.unwrap().unwrap();

    let settings = VoiceSettings::default();
    let client = VoiceClient::new(ClientConfig::new(DEFAULT_BASE_URL, key)).unwrap();
    let job = FinalJob::new(&meeting, folder.clone(), audio, &settings.final_model, &settings);
    let seen: Arc<Mutex<Vec<FinalProgress>>> = Arc::default();
    let sink = seen.clone();
    let started = std::time::Instant::now();
    let outcome = run_final_transcript(&client, &pool, &job, &CancellationToken::new(), move |p| {
        sink.lock().unwrap().push(p);
    })
    .await
    .unwrap_or_else(|e| panic!("final transcript failed: {e:?}"));
    eprintln!(
        "job {} -> {} turns from {:?} in {:.1}s",
        outcome.job_id,
        outcome.segments_count,
        outcome.turn_source,
        started.elapsed().as_secs_f64()
    );

    let progress = seen.lock().unwrap().clone();
    assert!(matches!(progress.first(), Some(FinalProgress::Uploading { percent: 0 })), "{progress:?}");
    assert!(progress.iter().any(|p| matches!(p, FinalProgress::Transcribing { job_id } if *job_id == outcome.job_id)));

    let rows = TranscriptsRepository::all(&pool, "m-live", "final").await.unwrap();
    assert!(!rows.is_empty());
    assert_eq!(rows.len(), outcome.segments_count);
    for row in &rows {
        eprintln!(
            "[{:6.2}-{:6.2}] {}: {}",
            row.audio_start_time.unwrap(),
            row.audio_end_time.unwrap(),
            row.speaker.as_deref().unwrap_or("-"),
            row.transcript
        );
    }
    assert!(rows.iter().all(|r| !r.transcript.trim().is_empty()));
    assert!(rows.iter().all(|r| r.speaker.as_deref().is_some_and(|s| s.starts_with("Speaker "))));
    assert!(folder.join(TRANSCRIPT_FILE).is_file());
    assert!(folder.join(ARTIFACT_FILE).is_file());
    db.cleanup().await;
}
