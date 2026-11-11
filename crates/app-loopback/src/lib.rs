//! Windows per-application (process-tree) loopback capture.
//!
//! Captures everything a process tree renders (for example the Chrome browser process that hosts
//! the Google Voice PWA), regardless of which render endpoint the audio goes to. Requires Windows
//! build 20348 or newer. Other platforms compile to stubs that return
//! [`AppLoopbackError::Unsupported`].
//!
//! Samples are delivered as mono `f32` at [`SAMPLE_RATE`] Hz from a dedicated capture thread.

#[cfg(windows)]
mod windows_impl;
#[cfg(windows)]
use windows_impl as imp;

#[cfg(not(windows))]
mod stub;
#[cfg(not(windows))]
use stub as imp;

/// Sample rate of every buffer passed to `on_samples`.
pub const SAMPLE_RATE: u32 = 48_000;

/// First Windows build that supports `AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK`.
const MIN_PROCESS_LOOPBACK_BUILD: u32 = 20_348;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AppLoopbackError {
    #[error("process loopback capture needs Windows build {MIN_PROCESS_LOOPBACK_BUILD} or newer")]
    Unsupported,
    #[error("process {0} is not running or cannot be opened")]
    ProcessNotFound(u32),
    #[error("process loopback activation failed: {0}")]
    Activation(String),
    #[error("capture thread failed: {0}")]
    Thread(String),
}

/// True when this system supports process-tree loopback (Windows build >= 20348).
pub fn is_supported() -> bool {
    imp::windows_build().is_some_and(|build| build >= MIN_PROCESS_LOOPBACK_BUILD)
}

/// Finds the root of the process tree for `exe_name` (case-insensitive, e.g. `"chrome.exe"`):
/// a process with that name whose parent does not have that name. When several roots exist, the
/// one with the most same-named descendants wins (ties: lowest pid).
pub fn find_process_tree_root(exe_name: &str) -> Option<u32> {
    select_tree_root(&imp::process_snapshot()?, exe_name)
}

/// A running process-tree loopback capture. Stops and joins its thread on [`stop`](Self::stop) or
/// drop.
pub struct AppLoopbackCapture {
    // Held only for its Drop, which stops and joins the capture thread.
    _inner: imp::Capture,
}

impl AppLoopbackCapture {
    /// Starts capturing the audio rendered by `pid` and all its child processes.
    ///
    /// Returns once the loopback stream is activated and running, or with the setup error
    /// (unsupported OS, process not running, activation or stream initialisation failure).
    /// After that, `on_samples` receives mono f32 at 48 kHz from the capture thread, and
    /// `on_error` receives a runtime failure (target process exited, capture read error), after
    /// which the capture thread ends. While the target renders nothing, Windows may deliver
    /// silent packets or no packets at all; [`stop`](Self::stop) never blocks on that.
    pub fn start(
        pid: u32,
        on_samples: impl FnMut(&[f32]) + Send + 'static,
        on_error: impl FnMut(String) + Send + 'static,
    ) -> Result<AppLoopbackCapture, AppLoopbackError> {
        imp::Capture::start(pid, on_samples, on_error)
            .map(|inner| AppLoopbackCapture { _inner: inner })
    }

    /// Stops the stream and joins the capture thread (consuming `self` drops it).
    pub fn stop(self) {}
}

/// One Toolhelp32 process entry.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone)]
struct ProcessEntry {
    pid: u32,
    parent_pid: u32,
    exe_name: String,
}

fn select_tree_root(processes: &[ProcessEntry], exe_name: &str) -> Option<u32> {
    use std::collections::HashMap;

    let named: HashMap<u32, u32> = processes
        .iter()
        .filter(|p| p.exe_name.eq_ignore_ascii_case(exe_name))
        .map(|p| (p.pid, p.parent_pid))
        .collect();

    let mut tree_sizes: HashMap<u32, usize> = HashMap::new();
    for &pid in named.keys() {
        // Walk up while the parent has the same name; the step bound guards against pid-reuse
        // cycles in the snapshot.
        let mut root = pid;
        for _ in 0..named.len() {
            match named.get(&root) {
                Some(&parent) if parent != root && named.contains_key(&parent) => root = parent,
                _ => break,
            }
        }
        *tree_sizes.entry(root).or_default() += 1;
    }

    tree_sizes
        .into_iter()
        .max_by(|(pid_a, size_a), (pid_b, size_b)| size_a.cmp(size_b).then(pid_b.cmp(pid_a)))
        .map(|(pid, _)| pid)
}

/// Appends the mono downmix of interleaved little-endian f32 stereo `bytes` to `out`.
#[cfg_attr(not(windows), allow(dead_code))]
fn downmix_stereo_f32le(bytes: &[u8], out: &mut Vec<f32>) {
    out.extend(bytes.chunks_exact(8).map(|frame| {
        let left = f32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]);
        let right = f32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
        (left + right) * 0.5
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(pid: u32, parent_pid: u32, exe_name: &str) -> ProcessEntry {
        ProcessEntry {
            pid,
            parent_pid,
            exe_name: exe_name.to_string(),
        }
    }

    #[test]
    fn root_is_the_same_named_process_without_a_same_named_parent() {
        let snapshot = [
            entry(4, 0, "System"),
            entry(900, 4, "explorer.exe"),
            entry(1000, 900, "chrome.exe"),
            entry(1010, 1000, "chrome.exe"),
            entry(1020, 1000, "chrome.exe"),
            entry(1030, 1020, "chrome.exe"),
            entry(1100, 900, "chrome_proxy.exe"),
        ];
        assert_eq!(select_tree_root(&snapshot, "chrome.exe"), Some(1000));
        assert_eq!(select_tree_root(&snapshot, "CHROME.EXE"), Some(1000));
        assert_eq!(select_tree_root(&snapshot, "chrome_proxy.exe"), Some(1100));
    }

    #[test]
    fn no_matching_process_gives_none() {
        let snapshot = [entry(900, 4, "explorer.exe")];
        assert_eq!(select_tree_root(&snapshot, "chrome.exe"), None);
        assert_eq!(select_tree_root(&[], "chrome.exe"), None);
    }

    #[test]
    fn largest_tree_wins_over_a_stray_orphan() {
        let snapshot = [
            entry(500, 9999, "chrome.exe"),
            entry(1000, 900, "chrome.exe"),
            entry(1010, 1000, "chrome.exe"),
            entry(1020, 1000, "chrome.exe"),
        ];
        assert_eq!(select_tree_root(&snapshot, "chrome.exe"), Some(1000));
    }

    #[test]
    fn equal_trees_pick_the_lowest_pid() {
        let snapshot = [entry(2000, 1, "chrome.exe"), entry(1500, 1, "chrome.exe")];
        assert_eq!(select_tree_root(&snapshot, "chrome.exe"), Some(1500));
    }

    #[test]
    fn parent_cycles_terminate() {
        let snapshot = [
            entry(10, 20, "chrome.exe"),
            entry(20, 10, "chrome.exe"),
            entry(30, 30, "chrome.exe"),
        ];
        assert!(select_tree_root(&snapshot, "chrome.exe").is_some());
    }

    #[test]
    fn downmix_averages_channels_and_ignores_partial_frames() {
        let mut bytes = Vec::new();
        for sample in [1.0f32, 0.0, -0.5, -0.25, 0.2, 0.4] {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes.extend_from_slice(&[0, 0, 0]);
        let mut out = vec![9.0];
        downmix_stereo_f32le(&bytes, &mut out);
        assert_eq!(out.len(), 4);
        assert_eq!(&out[..3], &[9.0, 0.5, -0.375]);
        assert!((out[3] - 0.3).abs() < 1e-6);
    }

    #[cfg(not(windows))]
    #[test]
    fn stubs_report_unsupported() {
        assert!(!is_supported());
        assert_eq!(find_process_tree_root("chrome.exe"), None);
        let result = AppLoopbackCapture::start(1, |_| {}, |_| {});
        assert!(matches!(result, Err(AppLoopbackError::Unsupported)));
    }
}
