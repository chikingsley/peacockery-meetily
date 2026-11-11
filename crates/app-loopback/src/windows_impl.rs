//! Windows process-tree loopback via `wasapi` (ActivateAudioInterfaceAsync on
//! VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK with PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE).

use std::sync::mpsc;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use wasapi::{AudioCaptureClient, AudioClient, Direction, SampleType, StreamMode, WaveFormat};
use windows::core::PCWSTR;
use windows::Wdk::System::SystemServices::RtlGetVersion;
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, HANDLE, WAIT_EVENT, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
use windows::Win32::System::Threading::{
    CreateEventW, OpenProcess, SetEvent, WaitForMultipleObjects, PROCESS_SYNCHRONIZE,
};

use crate::{downmix_stereo_f32le, AppLoopbackError, ProcessEntry, SAMPLE_RATE};

/// Stereo f32 frame size of the requested stream format.
const BLOCK_ALIGN: usize = 8;
/// Shared-mode buffer duration requested from the engine (100 ns units): 100 ms.
const BUFFER_DURATION_HNS: i64 = 1_000_000;
/// Wake-up interval when no packet arrives, so stop and process exit are always noticed.
const IDLE_WAIT_MS: u32 = 100;
/// Upper bound on activation + initialisation before `start` gives up.
const ACTIVATION_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) fn windows_build() -> Option<u32> {
    let mut info = OSVERSIONINFOW {
        dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is a valid, correctly sized OSVERSIONINFOW.
    let status = unsafe { RtlGetVersion(&mut info) };
    status.is_ok().then_some(info.dwBuildNumber)
}

pub(crate) fn process_snapshot() -> Option<Vec<ProcessEntry>> {
    // SAFETY: plain Toolhelp32 calls on a snapshot handle owned by `OwnedHandle`.
    unsafe {
        let snapshot = OwnedHandle(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?);
        let mut entry = PROCESSENTRY32W {
            dwSize: size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut processes = Vec::new();
        Process32FirstW(snapshot.0, &mut entry).ok()?;
        loop {
            let len = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            processes.push(ProcessEntry {
                pid: entry.th32ProcessID,
                parent_pid: entry.th32ParentProcessID,
                exe_name: String::from_utf16_lossy(&entry.szExeFile[..len]),
            });
            if Process32NextW(snapshot.0, &mut entry).is_err() {
                break;
            }
        }
        Some(processes)
    }
}

/// A kernel handle closed on drop.
struct OwnedHandle(HANDLE);

// SAFETY: the wrapped handles are kernel objects (event, process, snapshot); waiting on and
// signalling them is thread-safe.
unsafe impl Send for OwnedHandle {}
unsafe impl Sync for OwnedHandle {}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: the handle is owned and closed exactly once.
        let _ = unsafe { CloseHandle(self.0) };
    }
}

pub(crate) struct Capture {
    stop_event: Arc<OwnedHandle>,
    thread: Option<JoinHandle<()>>,
}

impl Capture {
    pub(crate) fn start(
        pid: u32,
        on_samples: impl FnMut(&[f32]) + Send + 'static,
        on_error: impl FnMut(String) + Send + 'static,
    ) -> Result<Capture, AppLoopbackError> {
        if !crate::is_supported() {
            return Err(AppLoopbackError::Unsupported);
        }
        // SAFETY: OpenProcess/CreateEventW return owned handles or an error.
        let process = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }
            .map(OwnedHandle)
            .map_err(|_| AppLoopbackError::ProcessNotFound(pid))?;
        let stop_event = unsafe { CreateEventW(None, true, false, PCWSTR::null()) }
            .map(|handle| Arc::new(OwnedHandle(handle)))
            .map_err(|e| AppLoopbackError::Thread(format!("CreateEventW: {e}")))?;

        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(), AppLoopbackError>>(1);
        let thread_stop = Arc::clone(&stop_event);
        let thread = thread::Builder::new()
            .name(format!("app-loopback-{pid}"))
            .spawn(move || {
                capture_thread(pid, process, thread_stop, ready_tx, on_samples, on_error)
            })
            .map_err(|e| AppLoopbackError::Thread(e.to_string()))?;

        let capture = Capture {
            stop_event,
            thread: Some(thread),
        };
        match ready_rx.recv_timeout(ACTIVATION_TIMEOUT) {
            Ok(Ok(())) => Ok(capture),
            Ok(Err(e)) => Err(e),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // The thread is stuck inside activation; signal stop and detach it so a hung
                // audio service cannot block the caller. It exits once activation returns.
                let mut capture = capture;
                capture.signal_stop();
                drop(capture.thread.take());
                Err(AppLoopbackError::Activation(format!(
                    "no result within {} s",
                    ACTIVATION_TIMEOUT.as_secs()
                )))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(AppLoopbackError::Thread(
                "capture thread exited during setup".into(),
            )),
        }
    }

    fn signal_stop(&self) {
        // SAFETY: the event handle is alive for as long as `self`.
        let _ = unsafe { SetEvent(self.stop_event.0) };
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.signal_stop();
        if let Some(thread) = self.thread.take() {
            // Dropping from inside a callback on the capture thread must not self-join.
            if thread.thread().id() != thread::current().id() {
                let _ = thread.join();
            }
        }
    }
}

/// Runs COM init, stream setup, the capture loop and teardown on the capture thread.
fn capture_thread(
    pid: u32,
    process: OwnedHandle,
    stop_event: Arc<OwnedHandle>,
    ready_tx: mpsc::SyncSender<Result<(), AppLoopbackError>>,
    mut on_samples: impl FnMut(&[f32]),
    mut on_error: impl FnMut(String),
) {
    if let Err(e) = wasapi::initialize_mta().ok() {
        let _ = ready_tx.send(Err(AppLoopbackError::Activation(format!(
            "CoInitializeEx(MTA): {e}"
        ))));
        return;
    }
    match open_stream(pid) {
        Err(e) => {
            let _ = ready_tx.send(Err(e));
        }
        Ok(stream) => {
            if ready_tx.send(Ok(())).is_ok() {
                if let Err(message) = run_loop(pid, &stream, &process, &stop_event, &mut on_samples)
                {
                    on_error(message);
                }
            }
            let _ = stream.client.stop_stream();
            drop(stream);
        }
    }
    wasapi::deinitialize();
}

struct Stream {
    client: AudioClient,
    capture: AudioCaptureClient,
    event: wasapi::Handle,
}

fn open_stream(pid: u32) -> Result<Stream, AppLoopbackError> {
    let activation = |e: wasapi::WasapiError| AppLoopbackError::Activation(e.to_string());
    let mut client = AudioClient::new_application_loopback_client(pid, true).map_err(activation)?;
    let format = WaveFormat::new(32, 32, SampleType::Float, SAMPLE_RATE as usize, 2, None);
    let mode = StreamMode::EventsShared {
        autoconvert: true,
        buffer_duration_hns: BUFFER_DURATION_HNS,
    };
    client
        .initialize_client(&format, Direction::Capture, mode, None)
        .map_err(activation)?;
    let event = client.set_get_eventhandle().map_err(activation)?;
    let capture = client.get_audiocaptureclient().map_err(activation)?;
    client.start_stream().map_err(activation)?;
    Ok(Stream {
        client,
        capture,
        event,
    })
}

/// Event-driven capture until stop (Ok) or a runtime failure (Err with the message).
fn run_loop(
    pid: u32,
    stream: &Stream,
    process: &OwnedHandle,
    stop_event: &OwnedHandle,
    on_samples: &mut impl FnMut(&[f32]),
) -> Result<(), String> {
    const STOP: WAIT_EVENT = WAIT_OBJECT_0;
    const PROCESS_EXIT: WAIT_EVENT = WAIT_EVENT(WAIT_OBJECT_0.0 + 1);
    const PACKET: WAIT_EVENT = WAIT_EVENT(WAIT_OBJECT_0.0 + 2);

    let handles = [stop_event.0, process.0, stream.event.as_handle()];
    let mut bytes: Vec<u8> = Vec::new();
    let mut mono: Vec<f32> = Vec::new();
    loop {
        // SAFETY: all three handles stay open for the duration of the wait.
        let woke = unsafe { WaitForMultipleObjects(&handles, false, IDLE_WAIT_MS) };
        match woke {
            STOP => return Ok(()),
            PROCESS_EXIT => {
                drain(&stream.capture, &mut bytes, &mut mono, on_samples)?;
                return Err(format!("process {pid} exited"));
            }
            PACKET | WAIT_TIMEOUT => drain(&stream.capture, &mut bytes, &mut mono, on_samples)?,
            other => {
                // SAFETY: reads the calling thread's last-error value.
                let error = unsafe { GetLastError() };
                return Err(format!(
                    "WaitForMultipleObjects returned {:#x}: {error:?}",
                    other.0
                ));
            }
        }
    }
}

/// Reads every pending packet and hands each one to `on_samples` as mono.
fn drain(
    capture: &AudioCaptureClient,
    bytes: &mut Vec<u8>,
    mono: &mut Vec<f32>,
    on_samples: &mut impl FnMut(&[f32]),
) -> Result<(), String> {
    loop {
        let frames = capture
            .get_next_packet_size()
            .map_err(|e| format!("GetNextPacketSize: {e}"))?
            .unwrap_or(0) as usize;
        if frames == 0 {
            return Ok(());
        }
        if bytes.len() < frames * BLOCK_ALIGN {
            bytes.resize(frames * BLOCK_ALIGN, 0);
        }
        let (read, _) = capture
            .read_from_device(bytes)
            .map_err(|e| format!("capture read: {e}"))?;
        if read == 0 {
            return Ok(());
        }
        mono.clear();
        downmix_stereo_f32le(&bytes[..read as usize * BLOCK_ALIGN], mono);
        on_samples(mono);
    }
}
