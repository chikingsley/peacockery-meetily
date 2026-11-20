//! Upload an audio file, transcribe it and print named turns.
//!
//! ```text
//! PEACOCKERY_VOICE_LAB_API_KEY=… cargo run -p peacockery-voice --example batch_file -- \
//!     call.m4a [model] [--no-diarize]
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use peacockery_voice::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1);
    let mut positional = Vec::new();
    let mut diarize = true;
    for arg in args {
        match arg.as_str() {
            "--no-diarize" => diarize = false,
            _ => positional.push(arg),
        }
    }
    let path = PathBuf::from(
        positional
            .first()
            .ok_or("usage: batch_file <audio> [model] [--no-diarize]")?,
    );
    let model = positional
        .get(1)
        .cloned()
        .unwrap_or_else(|| "elevenlabs-scribe_v3".into());

    let key = std::env::var("PEACOCKERY_VOICE_LAB_API_KEY")?;
    let base =
        std::env::var("PEACOCKERY_VOICE_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.into());
    let client = VoiceClient::new(ClientConfig::new(base, key))?;

    let mut options = TranscribeOptions::new(model);
    options.diarize = diarize;
    let progress: ProgressFn = Arc::new(|p| match p {
        BatchProgress::Uploading { sent, total } if sent == total || sent % (4 << 20) < 262_144 => {
            eprintln!("upload {sent}/{total}")
        }
        BatchProgress::Uploading { .. } => {}
        other => eprintln!("{other:?}"),
    });
    let done = client
        .transcribe_file(&path, &options, &CancellationToken::new(), progress)
        .await?;

    let (turns, source) = turns_with_source(&done.artifact);
    eprintln!(
        "job {} — {} turns from {source:?}",
        done.job_id,
        turns.len()
    );
    for turn in name_speakers(&turns) {
        println!(
            "[{:6.2}–{:6.2}] {}: {}",
            turn.start,
            turn.end,
            turn.speaker.as_deref().unwrap_or("-"),
            turn.text
        );
    }
    Ok(())
}
