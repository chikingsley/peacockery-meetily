//! Stream a 16 kHz mono WAV through a realtime model and print the live lines.
//!
//! ```text
//! PEACOCKERY_VOICE_LAB_API_KEY=… cargo run -p peacockery-voice --example realtime_file -- \
//!     tests/fixtures/dialogue.wav [model] [--diarize] [--speed 2]
//! ```

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use peacockery_voice::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut positional = Vec::new();
    let mut diarize = false;
    let mut speed = 1.0_f64;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--diarize" => diarize = true,
            "--speed" => speed = args.next().ok_or("--speed needs a value")?.parse()?,
            _ => positional.push(arg),
        }
    }
    let path = positional
        .first()
        .ok_or("usage: realtime_file <16k-mono.wav> [model] [--diarize] [--speed N]")?;
    let public_model = positional
        .get(1)
        .map(String::as_str)
        .unwrap_or("mistral-voxtral-mini-transcribe-realtime-3");

    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    if spec.sample_rate != SAMPLE_RATE || spec.channels != 1 || spec.bits_per_sample != 16 {
        return Err(format!("need 16 kHz mono s16 WAV, got {spec:?}").into());
    }
    let samples: Vec<i16> = reader.samples::<i16>().collect::<Result<_, _>>()?;

    let key = std::env::var("PEACOCKERY_VOICE_LAB_API_KEY")?;
    let base =
        std::env::var("PEACOCKERY_VOICE_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.into());
    let client = VoiceClient::new(ClientConfig::new(base, key))?;
    let catalog = client.models().await?;
    let model = catalog
        .realtime_model_id(public_model)
        .ok_or("model has no realtime route")?
        .to_string();

    let mut options = RealtimeOptions::new(model.clone());
    options.diarize = diarize;
    let (mut sender, mut events) = RealtimeSession::connect(&client, options).await?;

    let mode = if diarize {
        LiveMode::Mixed
    } else {
        LiveMode::Split
    };
    let channel = if diarize {
        Channel::Mixed
    } else {
        Channel::Mic
    };
    let speakers = SpeakerLabels::for_session(mode, channel, diarize, "Me", "Them");
    let mut lines = LineBuilder::new(LineBuilderConfig::new(channel, &model, speakers));

    let sent_samples = Arc::new(AtomicU64::new(0));
    let clock = sent_samples.clone();
    let printer = tokio::spawn(async move {
        let sent = || clock.load(Ordering::Relaxed) as f64 / f64::from(SAMPLE_RATE);
        let mut tick = tokio::time::interval(Duration::from_millis(100));
        loop {
            let updates = tokio::select! {
                event = events.recv() => match event {
                    Some(RealtimeEvent::SessionStarted { session_id, .. }) => { eprintln!("session {session_id}"); continue }
                    Some(event) => lines.on_event(&event, sent()),
                    None => break,
                },
                _ = tick.tick() => lines.tick(sent()),
            };
            for update in updates {
                match update {
                    LineUpdate::Final {
                        speaker,
                        text,
                        start,
                        end,
                        ..
                    } => {
                        println!(
                            "[{start:6.2}–{end:6.2}] {}: {text}",
                            speaker.as_deref().unwrap_or("-")
                        )
                    }
                    LineUpdate::Preview { text, .. } if !text.is_empty() => eprintln!("  … {text}"),
                    LineUpdate::Preview { .. } => {}
                }
            }
        }
        lines.finish(sent())
    });

    let frame_time = Duration::from_secs_f64(0.1 / speed);
    let silence = vec![0i16; FRAME_SAMPLES];
    for frame in samples
        .chunks(FRAME_SAMPLES)
        .chain(std::iter::repeat_n(silence.as_slice(), 10))
    {
        sender.send_pcm(frame).await?;
        sent_samples.fetch_add(frame.len() as u64, Ordering::Relaxed);
        tokio::time::sleep(frame_time).await;
    }
    let terminal = sender.finish(DEFAULT_FINISH_TIMEOUT).await?;
    printer.await?;
    match terminal {
        RealtimeTerminal::Completed { session_id, result } => {
            println!("completed {session_id}: {} chars", result.text.len())
        }
        RealtimeTerminal::Failed {
            session_id, error, ..
        } => println!("failed {session_id}: {} {}", error.code, error.message),
    }
    Ok(())
}
