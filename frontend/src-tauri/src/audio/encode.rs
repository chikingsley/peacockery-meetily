//! AAC encoding of the PCM audio journal through the ffmpeg sidecar.

use super::ffmpeg::ffmpeg_command;
use anyhow::{anyhow, Context, Result};
use std::path::Path;
use std::process::Stdio;

/// AAC-LC bitrate for recordings (mono speech; ~43 MiB per hour).
pub const AAC_BITRATE: &str = "96k";

/// Encode a mono signed 16-bit little-endian PCM file to an AAC-LC `.mp4` at
/// `output_path`. ffmpeg reads the file itself, so nothing is held in memory.
pub fn encode_pcm_file(input_path: &Path, sample_rate: u32, output_path: &Path) -> Result<()> {
    let output = ffmpeg_command()?
        .args(["-f", "s16le", "-ar", &sample_rate.to_string(), "-ac", "1"])
        .arg("-i")
        .arg(input_path)
        .args(["-c:a", "aac", "-b:a", AAC_BITRATE, "-profile:a", "aac_low"])
        .args(["-movflags", "+faststart", "-f", "mp4", "-y"])
        .arg(output_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .context("Failed to start ffmpeg")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        log::error!("ffmpeg encode failed ({}): {}", output.status, stderr);
        return Err(anyhow!(
            "ffmpeg encode failed with status {}",
            output.status
        ));
    }
    Ok(())
}
