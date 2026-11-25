//! Optional per-PC desktop app management. Keep all process operations off the capture path.
use crate::audio::recording_commands::{is_recording_active, is_recording_stopping};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::time::Duration;

pub fn start(dir: PathBuf) {
    if !dir.join("meeting-focus.enabled").is_file() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let mut focused = dir.join("meeting-focus-apps.json").is_file();
        loop {
            let recording = is_recording_active() || is_recording_stopping();
            if focused != recording {
                let directory = dir.clone();
                let action = if recording { "close" } else { "restore" };
                let result = tauri::async_runtime::spawn_blocking(move || {
                    std::process::Command::new("powershell.exe")
                        .args([
                            "-NoProfile",
                            "-NonInteractive",
                            "-Command",
                            include_str!("meeting_focus.ps1"),
                        ])
                        .env("MEETILY_FOCUS_DIR", directory)
                        .env("MEETILY_FOCUS_ACTION", action)
                        .creation_flags(0x08000000)
                        .output()
                })
                .await;
                if matches!(result, Ok(Ok(ref output)) if output.status.success()) {
                    focused = recording;
                } else {
                    log::warn!("Meeting focus {action} failed");
                    tokio::time::sleep(Duration::from_secs(10)).await;
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
}
