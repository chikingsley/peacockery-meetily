//! Writes the mixed recording of one meeting to `<meeting>/audio.mp4` and keeps the live
//! lines of the current session (memory for `get_transcript_history`, `transcripts.json`
//! in the meeting folder).

use anyhow::{anyhow, Result};
use log::{error, info, warn};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use super::incremental_saver::IncrementalAudioSaver;
use super::pipeline::PIPELINE_SAMPLE_RATE;
use super::recording_state::AudioChunk;
use crate::live::LiveLine;

/// Live lines of a recording, mirrored to `<meeting>/transcripts.json`.
pub const LIVE_LINES_FILE: &str = "transcripts.json";

/// Identity of the meeting being recorded.
#[derive(Debug, Clone)]
pub struct MeetingTarget {
    pub meeting_id: String,
    pub meeting_name: String,
    pub folder: PathBuf,
}

/// The finalized live lines of the current recording session. `transcripts.json` is
/// rewritten by one writer thread, so adding a line never does file I/O on the caller
/// (the async live sessions); lines that arrive during a write share the next one.
#[derive(Clone)]
pub struct LiveLineStore {
    lines: Arc<Mutex<Vec<LiveLine>>>,
    writer: std::sync::mpsc::Sender<WriteRequest>,
}

enum WriteRequest {
    Changed,
    /// Answered once the file holds every line pushed before the request.
    Flush(oneshot::Sender<()>),
}

impl LiveLineStore {
    pub fn new(folder: &Path) -> Self {
        let lines: Arc<Mutex<Vec<LiveLine>>> = Arc::default();
        let (writer, requests) = std::sync::mpsc::channel();
        let path = folder.join(LIVE_LINES_FILE);
        let shared = lines.clone();
        // Ends when the last clone of the store is dropped.
        let spawned = std::thread::Builder::new()
            .name("live-lines-writer".into())
            .spawn(move || write_live_lines(&path, &shared, &requests));
        if let Err(e) = spawned {
            error!("Failed to start the transcripts.json writer: {}", e);
        }
        Self { lines, writer }
    }

    /// Add a line; `transcripts.json` follows in the background.
    pub fn push(&self, line: LiveLine) {
        self.lines
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(line);
        let _ = self.writer.send(WriteRequest::Changed);
    }

    /// Wait until `transcripts.json` holds every line pushed so far.
    pub async fn flush(&self) {
        let (done, written) = oneshot::channel();
        if self.writer.send(WriteRequest::Flush(done)).is_ok() {
            let _ = written.await;
        }
    }

    pub fn lines(&self) -> Vec<LiveLine> {
        self.lines.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}

fn write_live_lines(
    path: &Path,
    lines: &Mutex<Vec<LiveLine>>,
    requests: &std::sync::mpsc::Receiver<WriteRequest>,
) {
    while let Ok(first) = requests.recv() {
        let flushes: Vec<_> = std::iter::once(first)
            .chain(requests.try_iter())
            .filter_map(|request| match request {
                WriteRequest::Changed => None,
                WriteRequest::Flush(done) => Some(done),
            })
            .collect();
        let snapshot = lines.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let result = serde_json::to_vec_pretty(&snapshot)
            .map_err(std::io::Error::other)
            .and_then(|bytes| {
                let tmp = path.with_extension("json.tmp");
                std::fs::write(&tmp, bytes)?;
                std::fs::rename(&tmp, path)
            });
        if let Err(e) = result {
            warn!("Failed to write {}: {}", path.display(), e);
        }
        for done in flushes {
            let _ = done.send(());
        }
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
    accumulation: Option<JoinHandle<Result<IncrementalAudioSaver>>>,
    live_lines: LiveLineStore,
}

impl RecordingSaver {
    /// Start consuming mixed audio for `target`. The journal writer runs on a blocking
    /// thread so disk I/O never stalls the async runtime.
    pub fn start(
        target: MeetingTarget,
        mut receiver: mpsc::UnboundedReceiver<AudioChunk>,
    ) -> Result<Self> {
        let mut saver = IncrementalAudioSaver::new(target.folder.clone(), PIPELINE_SAMPLE_RATE)?;
        let progress = Arc::<DiskProgress>::default();
        let writer = progress.clone();
        let accumulation = tokio::task::spawn_blocking(move || {
            while let Some(chunk) = receiver.blocking_recv() {
                writer.received.fetch_add(chunk.data.len() as u64, Relaxed);
                if let Err(e) = saver.add_chunk(chunk) {
                    // The audio stays pending and the next chunk retries the write.
                    if !writer.write_failed.swap(true, Relaxed) {
                        error!("Failed to write the audio journal: {}", e);
                    }
                } else {
                    writer.saved.store(saver.saved_samples(), Relaxed);
                    writer.write_failed.store(false, Relaxed);
                }
            }
            Ok(saver)
        });
        info!("Recording saver started for meeting {}", target.meeting_id);
        let live_lines = LiveLineStore::new(&target.folder);
        Ok(Self {
            target,
            progress,
            accumulation: Some(accumulation),
            live_lines,
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
    /// journal, then encode the journal into `audio.mp4`.
    pub async fn finish(&mut self) -> Result<PathBuf> {
        let accumulation = self
            .accumulation
            .take()
            .ok_or_else(|| anyhow!("Recording saver already finished"))?;
        let mut saver = accumulation
            .await
            .map_err(|e| anyhow!("Accumulation task failed: {}", e))??;
        let path = tokio::task::spawn_blocking(move || saver.finalize())
            .await
            .map_err(|e| anyhow!("Finalize task failed: {}", e))??;
        info!("Recording saved: {}", path.display());
        Ok(path)
    }
}
