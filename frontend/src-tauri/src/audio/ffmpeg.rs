//! Runtime lookup of the bundled ffmpeg sidecar.
//!
//! The installer ships `ffmpeg.exe` next to the app executable (Tauri `externalBin`).
//! Development builds fall back to an `ffmpeg` on `PATH`. There is no runtime download.

use std::path::PathBuf;
use std::process::Command;
use std::sync::LazyLock;

#[cfg(not(windows))]
const EXECUTABLE_NAME: &str = "ffmpeg";

#[cfg(windows)]
const EXECUTABLE_NAME: &str = "ffmpeg.exe";

static FFMPEG_PATH: LazyLock<Option<PathBuf>> = LazyLock::new(locate_ffmpeg);

/// Path of the ffmpeg executable: next to the app executable, else on `PATH`.
pub fn find_ffmpeg_path() -> Option<PathBuf> {
    FFMPEG_PATH.clone()
}

fn locate_ffmpeg() -> Option<PathBuf> {
    let bundled = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(EXECUTABLE_NAME)))
        .filter(|path| path.is_file());
    if let Some(path) = bundled {
        log::info!("Using bundled ffmpeg: {}", path.display());
        return Some(path);
    }

    let on_path = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(EXECUTABLE_NAME))
            .find(|path| path.is_file())
    });
    match &on_path {
        Some(path) => log::info!("Using ffmpeg from PATH: {}", path.display()),
        None => log::error!("ffmpeg not found next to the executable or on PATH"),
    }
    on_path
}

/// A `Command` for ffmpeg with the console window suppressed on Windows. ffmpeg prints
/// errors only: callers write all of stdin before they read stderr, so a chatty ffmpeg
/// (banner, progress stats) could fill the stderr pipe and block both sides.
pub fn ffmpeg_command() -> anyhow::Result<Command> {
    let path = find_ffmpeg_path()
        .ok_or_else(|| anyhow::anyhow!("ffmpeg not found next to the app or on PATH"))?;
    let mut command = Command::new(path);
    command.args(["-hide_banner", "-nostats", "-loglevel", "error"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    Ok(command)
}
