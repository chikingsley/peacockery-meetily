//! Crash-safe recording: the mixed audio is appended to a raw PCM journal in
//! `<meeting>/.checkpoints/` as it arrives and synced to disk every two seconds. The
//! journal is encoded into `<meeting>/audio.mp4` at stop (or at startup recovery after a
//! crash). Recording itself starts no process and holds at most two seconds in memory.

use super::encode::encode_pcm_file;
use super::recording_state::AudioChunk;
use anyhow::{anyhow, Context, Result};
use log::{info, warn};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const CHECKPOINTS_DIR: &str = ".checkpoints";
pub const AUDIO_FILE_NAME: &str = "audio.mp4";
/// Mono signed 16-bit little-endian PCM at the pipeline sample rate.
const JOURNAL_FILE_NAME: &str = "audio.s16le";
const PARTIAL_FILE_NAME: &str = "audio.partial.mp4";
const SYNC_SECONDS: usize = 2;
const BYTES_PER_SAMPLE: usize = 2;
const ENCODE_ATTEMPTS: u32 = 3;
const ENCODE_RETRY_DELAY: Duration = Duration::from_secs(2);

/// Appends mixed mono audio to the journal and syncs it every two seconds.
pub struct IncrementalAudioSaver {
    journal: File,
    pending: Vec<u8>,
    sync_interval_bytes: usize,
    saved_samples: u64,
    meeting_folder: PathBuf,
    sample_rate: u32,
}

impl IncrementalAudioSaver {
    /// Creates `<meeting_folder>/.checkpoints/` and an empty journal.
    pub fn new(meeting_folder: PathBuf, sample_rate: u32) -> Result<Self> {
        let checkpoints_dir = meeting_folder.join(CHECKPOINTS_DIR);
        std::fs::create_dir_all(&checkpoints_dir)?;
        let journal = File::create(checkpoints_dir.join(JOURNAL_FILE_NAME))
            .context("Failed to create the audio journal")?;
        let sync_interval_bytes = sample_rate as usize * SYNC_SECONDS * BYTES_PER_SAMPLE;
        Ok(Self {
            journal,
            pending: Vec::with_capacity(sync_interval_bytes + 8192),
            sync_interval_bytes,
            saved_samples: 0,
            meeting_folder,
            sample_rate,
        })
    }

    /// Append a chunk; writes and syncs the journal once two seconds are pending. On a
    /// write error the audio stays pending and the next chunk retries.
    pub fn add_chunk(&mut self, chunk: AudioChunk) -> Result<()> {
        self.pending.extend(chunk.data.iter().flat_map(|sample| {
            ((sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16).to_le_bytes()
        }));
        if self.pending.len() >= self.sync_interval_bytes {
            self.sync()?;
        }
        Ok(())
    }

    fn sync(&mut self) -> Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        // Track the durable length so a failed write is retried without duplicating audio.
        let durable = self.saved_samples * BYTES_PER_SAMPLE as u64;
        self.journal.set_len(durable)?;
        std::io::Seek::seek(&mut self.journal, std::io::SeekFrom::Start(durable))?;
        self.journal.write_all(&self.pending)?;
        self.journal.sync_data()?;
        self.saved_samples += (self.pending.len() / BYTES_PER_SAMPLE) as u64;
        self.pending.clear();
        Ok(())
    }

    /// Samples durably on disk.
    pub fn saved_samples(&self) -> u64 {
        self.saved_samples
    }

    /// Sync the remaining audio, encode the journal into `audio.mp4` and remove
    /// `.checkpoints/`. When encoding fails the journal stays for startup recovery.
    pub fn finalize(&mut self) -> Result<PathBuf> {
        self.sync()?;
        if self.saved_samples == 0 {
            return Err(anyhow!("No audio was captured - recording may have failed"));
        }
        info!(
            "Audio journal complete: {:.2}s",
            self.saved_samples as f64 / f64::from(self.sample_rate)
        );
        let mut attempt = 1;
        loop {
            match encode_journal(&self.meeting_folder, self.sample_rate) {
                Ok(path) => return Ok(path),
                Err(e) if attempt < ENCODE_ATTEMPTS => {
                    warn!("Encoding attempt {attempt} failed, retrying: {e:#}");
                    attempt += 1;
                    std::thread::sleep(ENCODE_RETRY_DELAY);
                }
                Err(e) => {
                    return Err(e.context(
                        "The recording is safe on disk and will be encoded at the next start",
                    ))
                }
            }
        }
    }
}

fn journal_path(meeting_folder: &Path) -> PathBuf {
    meeting_folder.join(CHECKPOINTS_DIR).join(JOURNAL_FILE_NAME)
}

/// Whether `meeting_folder` holds journaled audio that is not yet in `audio.mp4`.
pub fn has_journal(meeting_folder: &Path) -> bool {
    std::fs::metadata(journal_path(meeting_folder))
        .is_ok_and(|meta| meta.len() >= BYTES_PER_SAMPLE as u64)
}

/// Encode the journal of `meeting_folder` into `audio.mp4` and delete `.checkpoints/`.
/// Returns the path of `audio.mp4`.
pub fn encode_journal(meeting_folder: &Path, sample_rate: u32) -> Result<PathBuf> {
    let journal = journal_path(meeting_folder);
    if !has_journal(meeting_folder) {
        return Err(anyhow!("No audio journal in {}", meeting_folder.display()));
    }
    // Encode beside the final name so an interrupted encode never looks complete.
    let partial = meeting_folder.join(PARTIAL_FILE_NAME);
    let output = meeting_folder.join(AUDIO_FILE_NAME);
    encode_pcm_file(&journal, sample_rate, &partial)?;
    if !std::fs::metadata(&partial).is_ok_and(|meta| meta.len() > 0) {
        return Err(anyhow!(
            "Encoded audio was not created: {}",
            partial.display()
        ));
    }
    std::fs::rename(&partial, &output)?;
    info!("Encoded audio journal into {}", output.display());
    let checkpoints_dir = meeting_folder.join(CHECKPOINTS_DIR);
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
        AudioChunk {
            data: vec![0.1f32; samples],
            device_type: DeviceType::Microphone,
        }
    }

    #[test]
    fn audio_reaches_disk_within_two_seconds_without_ffmpeg() {
        let temp_dir = tempdir().unwrap();
        let meeting_folder = temp_dir.path().join("Journal");
        let mut saver = IncrementalAudioSaver::new(meeting_folder.clone(), 48000).unwrap();
        for _ in 0..5 {
            saver.add_chunk(chunk(24000)).unwrap(); // 2.5 s
        }
        // The first two seconds are durable; the last half second is still pending.
        assert_eq!(saver.saved_samples(), 96000);
        let journal = std::fs::read(journal_path(&meeting_folder)).unwrap();
        assert_eq!(journal.len(), 96000 * BYTES_PER_SAMPLE);
        assert_eq!(i16::from_le_bytes([journal[0], journal[1]]), 3277);
        // A crash here leaves a journal that startup recovery can encode.
        drop(saver);
        assert!(has_journal(&meeting_folder));
    }

    #[test]
    fn samples_are_clamped_to_16_bit() {
        let temp_dir = tempdir().unwrap();
        let meeting_folder = temp_dir.path().join("Clamp");
        let mut saver = IncrementalAudioSaver::new(meeting_folder.clone(), 2).unwrap();
        saver
            .add_chunk(AudioChunk {
                data: vec![2.0, -2.0, 0.0, 1.0],
                device_type: DeviceType::Microphone,
            })
            .unwrap();
        let journal = std::fs::read(journal_path(&meeting_folder)).unwrap();
        let samples: Vec<i16> = journal
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        assert_eq!(samples, vec![i16::MAX, -i16::MAX, 0, i16::MAX]);
    }

    #[test]
    fn encodes_the_journal_and_removes_it() {
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
        let final_path = saver.finalize().unwrap();
        assert_eq!(saver.saved_samples(), 130 * 24000);
        assert_eq!(final_path, meeting_folder.join(AUDIO_FILE_NAME));
        assert!(final_path.is_file());
        assert!(!meeting_folder.join(CHECKPOINTS_DIR).exists());
        assert!(!meeting_folder.join(PARTIAL_FILE_NAME).exists());
    }

    #[test]
    fn recovery_encodes_a_journal_left_by_a_crash() {
        if super::super::ffmpeg::find_ffmpeg_path().is_none() {
            eprintln!("ffmpeg not available; skipping");
            return;
        }
        let temp_dir = tempdir().unwrap();
        let meeting_folder = temp_dir.path().join("Crashed");
        let mut saver = IncrementalAudioSaver::new(meeting_folder.clone(), 48000).unwrap();
        for _ in 0..8 {
            saver.add_chunk(chunk(24000)).unwrap(); // 4 s
        }
        drop(saver);
        let path = encode_journal(&meeting_folder, 48000).unwrap();
        assert!(std::fs::metadata(path).unwrap().len() > 0);
        assert!(!has_journal(&meeting_folder));
    }

    #[test]
    fn finalize_without_audio_fails() {
        let temp_dir = tempdir().unwrap();
        let meeting_folder = temp_dir.path().join("Empty");
        let mut saver = IncrementalAudioSaver::new(meeting_folder.clone(), 48000).unwrap();
        let error = saver.finalize().unwrap_err();
        assert!(error.to_string().contains("No audio was captured"));
        assert!(!has_journal(&meeting_folder));
    }
}
