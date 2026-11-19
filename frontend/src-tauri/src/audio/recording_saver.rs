//! Writes the mixed recording of one meeting to its audio journal and keeps the live lines
//! of the current session in memory (`get_transcript_history`; the database has the rows).

use anyhow::{anyhow, Result};
use log::{error, info};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::journal::AudioJournal;
use super::pipeline::PIPELINE_SAMPLE_RATE;
use super::recording_state::AudioChunk;
use crate::live::LiveLine;

/// Identity of the meeting being recorded.
#[derive(Debug, Clone)]
pub struct MeetingTarget {
    pub meeting_id: String,
    pub meeting_name: String,
    pub folder: PathBuf,
}

/// The finalized live lines of the current recording session.
#[derive(Clone, Default)]
pub struct LiveLineStore {
    lines: Arc<Mutex<Vec<LiveLine>>>,
}

impl LiveLineStore {
    pub fn push(&self, line: LiveLine) {
        self.lines
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(line);
    }

    pub fn lines(&self) -> Vec<LiveLine> {
        self.lines.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}

#[derive(Default)]
struct DiskProgress {
    received: std::sync::atomic::AtomicU64,
    saved: std::sync::atomic::AtomicU64,
    write_failed: std::sync::atomic::AtomicBool,
}

use std::sync::atomic::Ordering::Relaxed;

pub struct RecordingSaver {
    progress: Arc<DiskProgress>,
    target: MeetingTarget,
    journal_writer: Option<JoinHandle<AudioJournal>>,
    live_lines: LiveLineStore,
}

impl RecordingSaver {
    /// Start consuming mixed audio for `target`. The journal writer runs on a blocking
    /// thread so disk I/O never stalls the async runtime.
    pub fn start(
        target: MeetingTarget,
        mut receiver: mpsc::UnboundedReceiver<AudioChunk>,
    ) -> Result<Self> {
        let mut journal = AudioJournal::create(&target.folder, PIPELINE_SAMPLE_RATE)?;
        let progress = Arc::<DiskProgress>::default();
        let writer = progress.clone();
        let journal_writer = tokio::task::spawn_blocking(move || {
            while let Some(chunk) = receiver.blocking_recv() {
                writer.received.fetch_add(chunk.data.len() as u64, Relaxed);
                if let Err(e) = journal.add_chunk(chunk) {
                    // The audio stays pending and the next chunk retries the write.
                    if !writer.write_failed.swap(true, Relaxed) {
                        error!("Failed to write the audio journal: {}", e);
                    }
                } else {
                    writer.saved.store(journal.saved_samples(), Relaxed);
                    writer.write_failed.store(false, Relaxed);
                }
            }
            journal
        });
        info!("Recording saver started for meeting {}", target.meeting_id);
        Ok(Self {
            target,
            progress,
            journal_writer: Some(journal_writer),
            live_lines: LiveLineStore::default(),
        })
    }

    pub fn progress(&self) -> serde_json::Value {
        let seconds = |count: u64| count as f64 / f64::from(PIPELINE_SAMPLE_RATE);
        serde_json::json!({ "received": seconds(self.progress.received.load(Relaxed)), "saved": seconds(self.progress.saved.load(Relaxed)), "write_failed": self.progress.write_failed.load(Relaxed) })
    }

    pub fn target(&self) -> &MeetingTarget {
        &self.target
    }

    pub fn live_lines(&self) -> &LiveLineStore {
        &self.live_lines
    }

    /// Wait until the pipeline has closed the audio channel and every chunk is in the
    /// journal, then sync and close it. The journal is encoded afterwards (`audio_save`).
    pub async fn close_journal(&mut self) -> Result<()> {
        let journal_writer = self
            .journal_writer
            .take()
            .ok_or_else(|| anyhow!("The audio journal is already closed"))?;
        let journal = journal_writer
            .await
            .map_err(|e| anyhow!("The audio journal writer failed: {}", e))?;
        tokio::task::spawn_blocking(move || journal.close())
            .await
            .map_err(|e| anyhow!("Closing the audio journal failed: {}", e))??;
        info!("Audio journal closed: {}", self.target.folder.display());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::recording_state::DeviceType;

    fn seconds(value: f64) -> AudioChunk {
        AudioChunk {
            data: vec![0.1; (value * f64::from(PIPELINE_SAMPLE_RATE)) as usize],
            device_type: DeviceType::Microphone,
        }
    }

    #[tokio::test]
    async fn progress_reports_received_and_synced_seconds() {
        let dir = tempfile::tempdir().unwrap();
        let (sender, receiver) = mpsc::unbounded_channel();
        let target = MeetingTarget {
            meeting_id: "m".into(),
            meeting_name: "Meeting".into(),
            folder: dir.path().to_path_buf(),
        };
        let mut saver = RecordingSaver::start(target, receiver).unwrap();
        sender.send(seconds(2.0)).unwrap();
        sender.send(seconds(0.5)).unwrap();
        drop(sender);
        saver.close_journal().await.unwrap();
        // The `local_progress` of `get_recording_state`: the last 0.5 s waits for the next sync.
        assert_eq!(
            saver.progress(),
            serde_json::json!({ "received": 2.5, "saved": 2.0, "write_failed": false })
        );
    }
}
