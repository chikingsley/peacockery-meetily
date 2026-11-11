#[path = "build/ffmpeg.rs"]
mod ffmpeg;

fn main() {
    println!("cargo:rerun-if-changed=build/ffmpeg.rs");
    ffmpeg::ensure_ffmpeg_sidecar();
    tauri_build::build()
}
