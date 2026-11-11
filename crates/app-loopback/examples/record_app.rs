//! Records one application's process-tree audio to a 48 kHz mono WAV.
//!
//! Usage: `record_app <exe-name> <seconds> <out.wav>`, e.g. `record_app chrome.exe 30 call.wav`.

use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Duration;

use app_loopback::{find_process_tree_root, is_supported, AppLoopbackCapture, SAMPLE_RATE};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let [_, exe_name, seconds, out_path] = args.as_slice() else {
        eprintln!("usage: record_app <exe-name> <seconds> <out.wav>");
        return ExitCode::from(2);
    };
    let Ok(seconds) = seconds.parse::<f64>() else {
        eprintln!("seconds must be a number, got {seconds:?}");
        return ExitCode::from(2);
    };
    match record(exe_name, seconds, out_path) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn record(exe_name: &str, seconds: f64, out_path: &str) -> Result<(), String> {
    if !is_supported() {
        return Err("process loopback is not supported on this system".into());
    }
    let pid = find_process_tree_root(exe_name)
        .ok_or_else(|| format!("no running process named {exe_name}"))?;
    eprintln!("capturing process tree of {exe_name} (pid {pid}) for {seconds} s");

    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    let capture = AppLoopbackCapture::start(
        pid,
        move |samples| {
            let _ = tx.send(samples.to_vec());
        },
        |message| eprintln!("capture error: {message}"),
    )
    .map_err(|e| e.to_string())?;
    std::thread::sleep(Duration::from_secs_f64(seconds));
    capture.stop();

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(out_path, spec).map_err(|e| e.to_string())?;
    let mut total = 0usize;
    let mut peak = 0f32;
    for chunk in rx.try_iter() {
        for sample in chunk {
            peak = peak.max(sample.abs());
            writer.write_sample(sample).map_err(|e| e.to_string())?;
            total += 1;
        }
    }
    writer.finalize().map_err(|e| e.to_string())?;
    eprintln!(
        "wrote {out_path}: {total} samples ({:.2} s), peak {peak:.3}",
        total as f64 / f64::from(SAMPLE_RATE)
    );
    Ok(())
}
