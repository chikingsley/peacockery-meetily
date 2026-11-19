//! Runtime lookup of the bundled ffmpeg sidecar.
//!
//! The installer ships `ffmpeg.exe` next to the app executable (Tauri `externalBin`).
//! Development builds fall back to an `ffmpeg` on `PATH`. There is no runtime download.
//!
//! Every ffmpeg the app starts is background work: on Windows it runs at below-normal
//! priority, so a recording in progress keeps the CPU, and inside a job object that kills
//! it when the app process ends, so no encode outlives the app.

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

/// A `Command` for ffmpeg with no console window and below-normal priority on Windows.
/// ffmpeg prints errors only, so a piped stderr stays small. Start it with [`spawn`] or
/// [`spawn_async`].
pub fn ffmpeg_command() -> anyhow::Result<Command> {
    let path = find_ffmpeg_path()
        .ok_or_else(|| anyhow::anyhow!("ffmpeg not found next to the app or on PATH"))?;
    let mut command = Command::new(path);
    command.args(["-hide_banner", "-nostats", "-loglevel", "error"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::{
            BELOW_NORMAL_PRIORITY_CLASS, CREATE_NO_WINDOW,
        };
        command.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
    }
    Ok(command)
}

/// Start ffmpeg and tie it to the app process.
pub fn spawn(command: &mut Command) -> std::io::Result<std::process::Child> {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut child = command.spawn()?;
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        if let Err(e) = app_job::assign(child.as_raw_handle()) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
    }
    Ok(child)
}

/// Start ffmpeg on the async runtime and tie it to the app process.
pub fn spawn_async(
    command: &mut tokio::process::Command,
) -> std::io::Result<tokio::process::Child> {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut child = command.spawn()?;
    #[cfg(windows)]
    {
        let assigned = match child.raw_handle() {
            Some(handle) => app_job::assign(handle),
            None => Err(std::io::Error::other(
                "ffmpeg exited before it could be tied to the app",
            )),
        };
        if let Err(e) = assigned {
            let _ = child.start_kill();
            return Err(e);
        }
    }
    Ok(child)
}

/// One job object for the app's ffmpeg children. Its handle is never closed: Windows closes
/// it when the app process ends, and `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` then ends every
/// process still in the job.
#[cfg(windows)]
mod app_job {
    use std::os::windows::io::RawHandle;
    use std::sync::LazyLock;
    use windows_sys::Win32::Foundation::{GetLastError, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    struct Job(HANDLE);
    // SAFETY: a job object handle may be used from any thread.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    static JOB: LazyLock<Result<Job, String>> = LazyLock::new(create);

    fn create() -> Result<Job, String> {
        // SAFETY: plain Win32 calls; the handle is not inheritable (null attributes), so
        // no child keeps the job open.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(format!("CreateJobObjectW failed: error {}", GetLastError()));
            }
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let set = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                std::ptr::from_ref(&limits).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if set == 0 {
                return Err(format!(
                    "SetInformationJobObject failed: error {}",
                    GetLastError()
                ));
            }
            Ok(Job(job))
        }
    }

    pub fn assign(process: RawHandle) -> std::io::Result<()> {
        let job = JOB
            .as_ref()
            .map_err(|e| std::io::Error::other(format!("ffmpeg cannot be tied to the app: {e}")))?;
        // SAFETY: `process` is the live handle of a child this process just started.
        if unsafe { AssignProcessToJobObject(job.0, process) } == 0 {
            let error = std::io::Error::last_os_error();
            return Err(std::io::Error::other(format!(
                "ffmpeg cannot be tied to the app: {error}"
            )));
        }
        Ok(())
    }
}
