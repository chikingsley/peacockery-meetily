//! AAC encoding of PCM checkpoints through the ffmpeg sidecar.

use super::ffmpeg::ffmpeg_command;
use anyhow::{anyhow, Context, Result};
use std::io::Write;
use std::path::Path;
use std::process::Stdio;

/// AAC-LC bitrate for recordings (mono speech; ~43 MiB per hour).
pub const AAC_BITRATE: &str = "96k";

/// Encode interleaved f32 PCM to an AAC-LC `.mp4` at `output_path`.
pub fn encode_single_audio(
    samples: &[f32],
    sample_rate: u32,
    channels: u16,
    output_path: &Path,
) -> Result<()> {
    if samples.is_empty() {
        return Err(anyhow!("No audio data provided for encoding"));
    }

    let mut command = ffmpeg_command()?;
    command
        .args(["-f", "f32le", "-ar", &sample_rate.to_string(), "-ac", &channels.to_string()])
        .args(["-i", "pipe:0", "-c:a", "aac", "-b:a", AAC_BITRATE, "-profile:a", "aac_low"])
        .args(["-movflags", "+faststart", "-f", "mp4", "-y"])
        .arg(output_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    let mut ffmpeg = command.spawn().context("Failed to spawn ffmpeg")?;
    let write_result = match ffmpeg.stdin.take() {
        Some(mut stdin) => stdin.write_all(bytemuck::cast_slice(samples)),
        None => Err(std::io::Error::other("ffmpeg stdin unavailable")),
    };
    let output = ffmpeg.wait_with_output().context("Failed to wait for ffmpeg")?;
    write_result.context("Failed to write PCM to ffmpeg")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        log::error!("ffmpeg encode failed ({}): {}", output.status, stderr);
        return Err(anyhow!("ffmpeg encode failed with status {}", output.status));
    }
    Ok(())
}
