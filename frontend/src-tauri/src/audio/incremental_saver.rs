//! Crash-safe recording: the mixed audio is encoded to a 30 s AAC checkpoint in
//! `<meeting>/.checkpoints/` as it arrives, and the checkpoints are concatenated into
//! `<meeting>/audio.mp4` at stop (or at startup recovery after a crash).

use super::encode::encode_single_audio;
use super::ffmpeg::ffmpeg_command;
use super::recording_state::AudioChunk;
use anyhow::{anyhow, Result};
use log::{info, warn};
use std::path::{Path, PathBuf};

pub const CHECKPOINTS_DIR: &str = ".checkpoints";
pub const AUDIO_FILE_NAME: &str = "audio.mp4";
const CHECKPOINT_SECONDS: usize = 30;

/// Buffers mixed mono audio and writes a checkpoint every 30 seconds.
pub struct IncrementalAudioSaver {
    buffer: Vec<f32>,
    checkpoint_interval_samples: usize,
    checkpoint_count: u32,
    checkpoints_dir: PathBuf,
    meeting_folder: PathBuf,
    sample_rate: u32,
}

impl IncrementalAudioSaver {
    /// Creates `<meeting_folder>/.checkpoints/` when missing.
    pub fn new(meeting_folder: PathBuf, sample_rate: u32) -> Result<Self> {
        let checkpoints_dir = meeting_folder.join(CHECKPOINTS_DIR);
        std::fs::create_dir_all(&checkpoints_dir)?;
        let checkpoint_interval_samples = sample_rate as usize * CHECKPOINT_SECONDS;
        Ok(Self {
            buffer: Vec::with_capacity(checkpoint_interval_samples),
            checkpoint_interval_samples,
            checkpoint_count: 0,
            checkpoints_dir,
            meeting_folder,
            sample_rate,
        })
    }

    /// Append a chunk; writes a checkpoint once 30 s are buffered.
    pub fn add_chunk(&mut self, chunk: AudioChunk) -> Result<()> {
        self.buffer.extend_from_slice(&chunk.data);
        if self.buffer.len() >= self.checkpoint_interval_samples {
            self.save_checkpoint()?;
        }
        Ok(())
    }

    fn save_checkpoint(&mut self) -> Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        let path = self
            .checkpoints_dir
            .join(format!("audio_chunk_{:04}.mp4", self.checkpoint_count));
        encode_single_audio(&self.buffer, self.sample_rate, 1, &path)?;
        self.checkpoint_count += 1;
        info!(
            "Saved checkpoint {}: {:.2}s of audio",
            self.checkpoint_count,
            self.buffer.len() as f64 / self.sample_rate as f64
        );
        self.buffer.clear();
        Ok(())
    }

    /// Write the last checkpoint, concatenate all of them into `audio.mp4` and remove
    /// `.checkpoints/`.
    pub fn finalize(&mut self) -> Result<PathBuf> {
        self.save_checkpoint()?;
        if self.checkpoint_count == 0 {
            return Err(anyhow!("No audio checkpoints to merge - recording may have failed"));
        }
        merge_checkpoints(&self.meeting_folder)
    }

    pub fn checkpoint_count(&self) -> u32 {
        self.checkpoint_count
    }
}

/// Checkpoint files in `<meeting>/.checkpoints/`, in recording order.
pub fn checkpoint_files(meeting_folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(meeting_folder.join(CHECKPOINTS_DIR)) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("mp4"))
        .collect();
    files.sort();
    files
}

/// Concatenate the checkpoints of `meeting_folder` into `audio.mp4` (stream copy) and
/// delete `.checkpoints/`. Returns the path of `audio.mp4`.
pub fn merge_checkpoints(meeting_folder: &Path) -> Result<PathBuf> {
    let files = checkpoint_files(meeting_folder);
    if files.is_empty() {
        return Err(anyhow!("No audio checkpoints in {}", meeting_folder.display()));
    }
    let checkpoints_dir = meeting_folder.join(CHECKPOINTS_DIR);
    let list_file = checkpoints_dir.join("concat_list.txt");
    let mut list_content = String::new();
    for file in &files {
        let absolute = file.canonicalize()?;
        let escaped = absolute.to_string_lossy().replace('\'', "'\\''");
        list_content.push_str(&format!("file '{}'\n", escaped));
    }
    std::fs::write(&list_file, list_content)?;

    let output = meeting_folder.join(AUDIO_FILE_NAME);
    let result = ffmpeg_command()?
        .args(["-f", "concat", "-safe", "0", "-i"])
        .arg(&list_file)
        .args(["-c", "copy", "-y"])
        .arg(&output)
        .output()?;
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(anyhow!("ffmpeg concat failed: {}", stderr));
    }
    if !output.is_file() {
        return Err(anyhow!("Merged audio file was not created: {}", output.display()));
    }

    info!("Merged {} checkpoints into {}", files.len(), output.display());
    if let Err(e) = std::fs::remove_dir_all(&checkpoints_dir) {
        warn!("Failed to remove {}: {}", checkpoints_dir.display(), e);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::super::recording_state::DeviceType;
    use super::*;
    use tempfile::tempdir;

    fn chunk(samples: usize) -> AudioChunk {
        AudioChunk { data: vec![0.1f32; samples], device_type: DeviceType::Microphone }
    }

    #[test]
    fn writes_checkpoints_and_merges_them() {
        if super::super::ffmpeg::find_ffmpeg_path().is_none() {
            eprintln!("ffmpeg not available; skipping");
            return;
        }
        let temp_dir = tempdir().unwrap();
        let meeting_folder = temp_dir.path().join("Test_Meeting");
        let mut saver = IncrementalAudioSaver::new(meeting_folder.clone(), 48000).unwrap();
        for _ in 0..130 {
            saver.add_chunk(chunk(24000)).unwrap(); // 65 s
        }
        assert_eq!(saver.checkpoint_count(), 2);

        let final_path = saver.finalize().unwrap();
        assert_eq!(final_path, meeting_folder.join(AUDIO_FILE_NAME));
        assert!(final_path.is_file());
        assert!(!meeting_folder.join(CHECKPOINTS_DIR).exists());
    }

    #[test]
    fn finalize_without_audio_fails() {
        let temp_dir = tempdir().unwrap();
        let meeting_folder = temp_dir.path().join("Empty");
        let mut saver = IncrementalAudioSaver::new(meeting_folder, 48000).unwrap();
        let error = saver.finalize().unwrap_err();
        assert!(error.to_string().contains("No audio checkpoints"));
    }

    #[test]
    fn checkpoint_files_are_sorted_mp4_only() {
        let temp_dir = tempdir().unwrap();
        let dir = temp_dir.path().join(CHECKPOINTS_DIR);
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["audio_chunk_0001.mp4", "audio_chunk_0000.mp4", "concat_list.txt"] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        let files = checkpoint_files(temp_dir.path());
        let names: Vec<_> = files.iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect();
        assert_eq!(names, vec!["audio_chunk_0000.mp4", "audio_chunk_0001.mp4"]);
    }
}
