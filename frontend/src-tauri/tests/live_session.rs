//! Live check of the live session manager against the lab API (no GUI, no audio devices):
//! `dialogue.wav` as the mic and silence as the system audio, split mode, Voxtral realtime 3,
//! fed at twice real time through the same taps the audio pipeline uses.
//! `PEACOCKERY_LIVE_MODEL=deepgram-nova-3` runs the same check on another live model.
//!
//! ```text
//! PEACOCKERY_VOICE_LAB_API_KEY=… cargo test -p peacockery-meetily --test live_session -- --ignored --nocapture
//! ```

use std::sync::{Arc, Mutex};
use std::time::Duration;

use app_lib::audio::recording_saver::{LiveLineStore, LIVE_LINES_FILE};
use app_lib::database::manager::DatabaseManager;
use app_lib::database::models::meeting_status;
use app_lib::database::repositories::meeting::{MeetingsRepository, NewMeeting};
use app_lib::database::repositories::transcript::TranscriptsRepository;
use app_lib::live::{channels_for, LiveConfig, LiveInterim, LiveLine, LiveManager, LiveSink, LiveState, LiveStatus};
use app_lib::voice::VoiceSettings;
use peacockery_voice::{Channel, ClientConfig, LiveMode, VoiceClient, DEFAULT_BASE_URL};

const DIALOGUE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../crates/peacockery-voice/tests/fixtures/dialogue.wav");
const SECONDS: usize = 40;
const SPEED: f64 = 2.0;

#[derive(Default)]
struct Collect {
    statuses: Mutex<Vec<LiveStatus>>,
    interims: Mutex<Vec<LiveInterim>>,
    lines: Mutex<Vec<LiveLine>>,
}

impl LiveSink for Collect {
    fn status(&self, status: &LiveStatus) {
        println!("status {:?} {:?} {:?}", status.channel, status.state, status.message);
        self.statuses.lock().unwrap().push(status.clone());
    }
    fn interim(&self, interim: &LiveInterim) {
        self.interims.lock().unwrap().push(interim.clone());
    }
    fn line(&self, line: &LiveLine) {
        println!(
            "#{} [{:6.2}-{:6.2}] {} {:?}: {}",
            line.sequence_id,
            line.audio_start_time,
            line.audio_end_time,
            line.timestamp,
            line.speaker,
            line.text
        );
        self.lines.lock().unwrap().push(line.clone());
    }
}

/// 16 kHz s16 → 48 kHz f32 (linear interpolation; the taps resample back to 16 kHz).
fn dialogue_48k() -> Vec<f32> {
    let mut reader = hound::WavReader::open(DIALOGUE).unwrap();
    let samples: Vec<f32> =
        reader.samples::<i16>().take(16_000 * SECONDS).map(|s| s.unwrap() as f32 / 32768.0).collect();
    let mut out = Vec::with_capacity(samples.len() * 3);
    for pair in samples.windows(2) {
        for k in 0..3 {
            out.push(pair[0] + (pair[1] - pair[0]) * k as f32 / 3.0);
        }
    }
    out
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "calls the live Peacockery Voice lab API"]
async fn split_session_on_dialogue_produces_lines() {
    let key = std::env::var("PEACOCKERY_VOICE_LAB_API_KEY").expect("PEACOCKERY_VOICE_LAB_API_KEY");
    let dir = tempfile::tempdir().unwrap();
    let db = DatabaseManager::open(&dir.path().join("test.sqlite")).await.unwrap();
    let pool = db.pool().clone();
    MeetingsRepository::create(
        &pool,
        &NewMeeting { id: "m-live", title: "Call", folder_path: &dir.path().to_string_lossy(), status: meeting_status::RECORDING },
    )
    .await
    .unwrap();

    // Voxtral realtime 3 (the default) unless PEACOCKERY_LIVE_MODEL names another live model.
    let mut settings = VoiceSettings::default();
    if let Ok(model) = std::env::var("PEACOCKERY_LIVE_MODEL") {
        settings.live_model = model;
    }
    println!("model {}", settings.live_model);
    assert_eq!(settings.live_mode, LiveMode::Split);
    let config = LiveConfig::from_settings("m-live", &settings);
    let client = VoiceClient::new(ClientConfig::new(DEFAULT_BASE_URL, key)).unwrap();
    let sink = Arc::new(Collect::default());
    let store = LiveLineStore::new(dir.path());
    let channels = channels_for(LiveMode::Split, true, true);
    let (manager, mut taps) = LiveManager::start(config, client, sink.clone(), store.clone(), Some(pool.clone()), &channels);

    let mic = dialogue_48k();
    let silence = vec![0.0f32; 4800];
    let window_time = Duration::from_secs_f64(0.1 / SPEED);
    for (i, window) in mic.chunks(4800).enumerate() {
        let sys = &silence[..window.len()];
        taps.push(window, sys, window, (i + 1) as f64 * 0.1);
        tokio::time::sleep(window_time).await;
    }
    // Two seconds of trailing silence, then stop.
    for i in 0..20 {
        let end = (mic.len() / 4800 + i + 1) as f64 * 0.1;
        taps.push(&silence, &silence, &silence, end);
        tokio::time::sleep(window_time).await;
    }
    taps.close();
    let started = std::time::Instant::now();
    let summary = manager.finish().await;
    println!("finished in {:.1} s: {:?}", started.elapsed().as_secs_f64(), summary);

    let lines = sink.lines.lock().unwrap().clone();
    let mic_lines: Vec<_> = lines.iter().filter(|l| l.channel == Channel::Mic).collect();
    assert!(!mic_lines.is_empty(), "no mic lines");
    assert!(mic_lines.iter().all(|l| l.speaker.as_deref() == Some("Me")));
    let words: usize = mic_lines.iter().map(|l| l.text.split_whitespace().count()).sum();
    assert!(words > 40, "only {} words in {} s of dialogue", words, SECONDS);
    let mut ids: Vec<u64> = lines.iter().map(|l| l.sequence_id).collect();
    ids.sort_unstable();
    assert_eq!(ids, (0..lines.len() as u64).collect::<Vec<_>>(), "sequence ids are 0..n");
    assert!(mic_lines.windows(2).all(|w| w[0].audio_start_time <= w[1].audio_start_time));
    assert!(mic_lines.iter().all(|l| l.audio_end_time <= SECONDS as f64 + 3.0));

    assert_eq!(summary.session_ids.len(), 2, "one realtime session per channel");
    assert_eq!(summary.mode, "split");
    let statuses = sink.statuses.lock().unwrap().clone();
    for channel in [Channel::Mic, Channel::System] {
        let states: Vec<LiveState> = statuses.iter().filter(|s| s.channel == channel).map(|s| s.state).collect();
        assert_eq!(states.first(), Some(&LiveState::Connecting));
        assert!(states.contains(&LiveState::Live), "{:?} never went live: {:?}", channel, states);
        assert_eq!(states.last(), Some(&LiveState::Closed));
    }

    let rows = TranscriptsRepository::all(&pool, "m-live", "live").await.unwrap();
    assert_eq!(rows.len(), lines.len(), "every line is a live row");
    assert_eq!(store.lines().len(), lines.len());
    let file: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join(LIVE_LINES_FILE)).unwrap()).unwrap();
    assert_eq!(file.as_array().unwrap().len(), lines.len());
    println!("{} lines ({} mic), {} words, {} previews", lines.len(), mic_lines.len(), words, sink.interims.lock().unwrap().len());
}
