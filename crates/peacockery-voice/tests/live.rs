//! Live tests against the lab API. Run with `cargo test -p peacockery-voice -- --ignored`
//! (needs PEACOCKERY_VOICE_LAB_API_KEY; `ffmpeg` on PATH for the batch test).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use peacockery_voice::*;

fn client() -> VoiceClient {
    let key = std::env::var("PEACOCKERY_VOICE_LAB_API_KEY").expect("PEACOCKERY_VOICE_LAB_API_KEY");
    let base =
        std::env::var("PEACOCKERY_VOICE_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.into());
    let mut config = ClientConfig::new(base, key);
    config.app_version = "test".into();
    config.installation_id = Some(uuid::Uuid::new_v4().to_string());
    VoiceClient::new(config).unwrap()
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[tokio::test]
#[ignore = "live API"]
async fn live_health_and_catalog() {
    let client = client();
    let health = client.health().await.unwrap();
    assert!(health.ok);
    assert_eq!(health.environment.as_deref(), Some("lab"));
    let catalog = client.models().await.unwrap();
    for list in [
        curated_live(&catalog),
        curated_final(&catalog),
        curated_summary(&catalog),
    ] {
        let recommended = list.iter().find(|m| m.recommended).unwrap();
        assert!(recommended.available, "{recommended:?}");
    }
    assert_eq!(
        catalog.realtime_model_id("mistral-voxtral-mini-transcribe-realtime-3"),
        Some("mistral-voxtral-mini-transcribe-realtime-3")
    );
}

#[tokio::test]
#[ignore = "live API"]
async fn live_bad_key_and_bad_option() {
    let bad =
        VoiceClient::new(ClientConfig::new(DEFAULT_BASE_URL, "pv_lab_not_a_real_key")).unwrap();
    assert!(matches!(bad.models().await, Err(Error::Unauthorized)));
    assert!(matches!(
        RealtimeSession::connect(&bad, RealtimeOptions::new("deepgram-nova-3")).await,
        Err(Error::Unauthorized)
    ));
    assert!(matches!(
        client().get_job("job_does_not_exist").await,
        Err(Error::NotFound(_))
    ));
}

/// Streams the first 30 s of the call at ~2× real time and checks the LineBuilder lines against the provider text.
async fn realtime_on_dialogue(options: RealtimeOptions, expect_delta: bool) {
    let client = client();
    let mut reader = hound::WavReader::open(fixture("dialogue.wav")).unwrap();
    let samples: Vec<i16> = reader
        .samples::<i16>()
        .take(30 * SAMPLE_RATE as usize)
        .map(Result::unwrap)
        .collect();
    let model = options.model.clone();
    let (mut sender, mut events) = RealtimeSession::connect(&client, options).await.unwrap();

    let collected = Arc::new(Mutex::new(Vec::new()));
    let sink = collected.clone();
    let reader_task = tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            sink.lock().unwrap().push(event);
        }
    });
    let mut sent = 0.0;
    let mut builder = LineBuilder::new(LineBuilderConfig::new(
        Channel::Mic,
        &model,
        SpeakerLabels::Fixed("Me".into()),
    ));
    let mut updates = Vec::new();
    let mut seen = 0;
    let silence = vec![0i16; FRAME_SAMPLES];
    for frame in samples
        .chunks(FRAME_SAMPLES)
        .chain(std::iter::repeat_n(silence.as_slice(), 20))
    {
        if let Err(err) = sender.send_pcm(frame).await {
            panic!("{model}: send failed: {err}");
        }
        sent += frame.len() as f64 / f64::from(SAMPLE_RATE);
        tokio::time::sleep(Duration::from_millis(50)).await;
        let new: Vec<_> = collected.lock().unwrap()[seen..].to_vec();
        seen += new.len();
        for event in &new {
            updates.extend(builder.on_event(event, sent));
        }
        updates.extend(builder.tick(sent));
    }
    assert!((sender.seconds_sent(SAMPLE_RATE) - sent).abs() < 1e-6);
    let terminal = sender.finish(DEFAULT_FINISH_TIMEOUT).await.unwrap();
    reader_task.await.unwrap();
    for event in &collected.lock().unwrap()[seen..] {
        updates.extend(builder.on_event(event, sent));
    }
    updates.extend(builder.finish(sent));

    let RealtimeTerminal::Completed { session_id, result } = terminal else {
        panic!("{terminal:?}")
    };
    assert!(session_id.starts_with("rt_"));
    let events = collected.lock().unwrap();
    assert!(
        matches!(&events[0], RealtimeEvent::SessionStarted { session_id: s, .. } if *s == session_id)
    );
    assert!(events
        .iter()
        .any(|e| matches!(e, RealtimeEvent::AudioReceived { .. })));
    assert_eq!(
        expect_delta,
        events.iter().any(|e| matches!(e, RealtimeEvent::Delta(_)))
    );
    let finals: Vec<String> = updates
        .iter()
        .filter_map(|u| match u {
            LineUpdate::Final { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    eprintln!("{model}: {finals:#?}");
    let squash = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    assert!(finals.len() >= 3, "{finals:?}");
    assert_eq!(
        squash(&finals.join(" ")),
        squash(&result.text),
        "{model}: lines {finals:#?}\nprovider {:?}",
        result.text
    );
    assert!(
        result.text.to_lowercase().contains("warehouse"),
        "{}",
        result.text
    );
}

#[tokio::test]
#[ignore = "live API"]
async fn live_realtime_voxtral_on_dialogue() {
    let mut options = RealtimeOptions::new("mistral-voxtral-mini-transcribe-realtime-3");
    options.client_session_id = Some(uuid::Uuid::new_v4().to_string());
    options.target_streaming_delay_ms = Some(480);
    options.keyterms = vec!["Peacockery".into()];
    realtime_on_dialogue(options, true).await;
}

#[tokio::test]
#[ignore = "live API"]
async fn live_realtime_nova_on_dialogue() {
    let mut options = RealtimeOptions::new("deepgram-nova-3");
    options.client_session_id = Some(uuid::Uuid::new_v4().to_string());
    options.keyterms = vec!["Peacockery".into()];
    realtime_on_dialogue(options, false).await;
}

#[tokio::test]
#[ignore = "live API"]
async fn live_realtime_rejects_unknown_model() {
    let err = RealtimeSession::connect(&client(), RealtimeOptions::new("deepgram-not-a-model"))
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::BadRequest { status: 400, .. }),
        "{err:?}"
    );
}

#[tokio::test]
#[ignore = "live API"]
async fn live_batch_scribe_on_clip() {
    let dir = std::env::temp_dir().join(format!("pv-live-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let clip = dir.join("clip.m4a");
    let status = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y", "-t", "20", "-i"])
        .arg(fixture("dialogue.wav"))
        .args(["-c:a", "aac", "-b:a", "96k"])
        .arg(&clip)
        .status()
        .expect("ffmpeg");
    assert!(status.success());

    let client = client();
    let mut options = TranscribeOptions::new("elevenlabs-scribe_v2");
    options.keyterms = vec!["Peacockery".into()];
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    let progress: ProgressFn = Arc::new(move |p| log.lock().unwrap().push(p));
    let done = client
        .transcribe_file(&clip, &options, &CancellationToken::new(), progress)
        .await
        .unwrap();
    std::fs::remove_dir_all(&dir).unwrap();

    assert!(done.job_id.starts_with("job_"));
    let seen = seen.lock().unwrap();
    assert!(seen
        .iter()
        .any(|p| matches!(p, BatchProgress::Uploaded { .. })));
    assert!(seen
        .iter()
        .any(|p| matches!(p, BatchProgress::JobCreated { .. })));
    assert!(matches!(
        seen.last().unwrap(),
        BatchProgress::Job {
            status: JobStatus::Succeeded,
            ..
        }
    ));
    let (turns, source) = turns_with_source(&done.artifact);
    assert_eq!(source, TurnSource::Words);
    let named = name_speakers(&turns, None, "Me", "Them");
    eprintln!("{named:#?}");
    assert!(!named.is_empty());
    assert!(named.iter().all(|t| t
        .speaker
        .as_deref()
        .is_some_and(|s| s.starts_with("Speaker "))));
    assert!(done.artifact.text.to_lowercase().contains("warehouse"));
}

#[tokio::test]
#[ignore = "live API"]
async fn live_text_and_truncation() {
    let client = client();
    let mut request = TextRequest::new(
        "google-gemini-3.1-flash-lite",
        vec![
            Message::system("Answer in one short sentence."),
            Message::user("What is a site visit?"),
        ],
    );
    request.max_output_tokens = Some(200);
    request.context = Some(TextContext {
        conversation_id: Some("peacockery-voice-live-test".into()),
        turn_id: None,
    });
    let response = client.generate(&request).await.unwrap();
    assert!(!response.text.trim().is_empty());
    assert!(!response.model.is_empty());
    assert!(response.usage.total_tokens.unwrap_or(0) > 0);

    let mut long = TextRequest::new(
        "google-gemini-3.1-flash-lite",
        vec![Message::user(
            "Write the numbers from 1 to 500 in English words, one per line.",
        )],
    );
    long.max_output_tokens = Some(16);
    match client.generate(&long).await {
        Err(Error::Truncated(partial)) => {
            assert_eq!(partial.finish_reason.as_deref(), Some("length"))
        }
        other => panic!("expected truncation, got {other:?}"),
    }
}
