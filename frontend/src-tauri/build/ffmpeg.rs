//! Provide the ffmpeg sidecar that `tauri.conf.json` lists as `externalBin: binaries/ffmpeg`.
//!
//! Windows targets: download a pinned GyanD/codexffmpeg release "essentials" zip, verify
//! it by SHA-256 only (the binary is never executed during the build) and extract
//! `bin/ffmpeg.exe` to `binaries/ffmpeg-<target>.exe`. A cached exe with the pinned hash
//! skips the download.
//!
//! Other targets (Linux development and tests): copy the system `ffmpeg` from `PATH`.

use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const FFMPEG_VERSION: &str = "9.0.2";
const FFMPEG_ZIP_URL: &str =
    "https://github.com/GyanD/codexffmpeg/releases/download/9.0.2/ffmpeg-9.0.2-essentials_build.zip";
const FFMPEG_ZIP_SIZE: u64 = 114_768_076;
const FFMPEG_ZIP_SHA256: &str = "60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba";
const FFMPEG_ZIP_ENTRY: &str = "ffmpeg-9.0.2-essentials_build/bin/ffmpeg.exe";
const FFMPEG_EXE_SHA256: &str = "3256173f3f8bffd7df12227c68adf68025edb1832273a9530688a7bb1ed8edec";

pub fn ensure_ffmpeg_sidecar() {
    let target = std::env::var("TARGET").expect("TARGET is set by cargo");
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo"));
    let binaries_dir = manifest_dir.join("binaries");
    std::fs::create_dir_all(&binaries_dir).expect("create binaries/");

    let result = if target.contains("windows") {
        let destination = binaries_dir.join(format!("ffmpeg-{}.exe", target));
        println!("cargo:rerun-if-changed={}", destination.display());
        ensure_windows_ffmpeg(&destination)
    } else {
        let destination = binaries_dir.join(format!("ffmpeg-{}", target));
        println!("cargo:rerun-if-changed={}", destination.display());
        copy_system_ffmpeg(&destination)
    };
    if let Err(error) = result {
        panic!("ffmpeg sidecar for {}: {}", target, error);
    }
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|e| format!("open {}: {}", path.display(), e))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = file.read(&mut buffer).map_err(|e| format!("read {}: {}", path.display(), e))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn ensure_windows_ffmpeg(destination: &Path) -> Result<(), String> {
    if destination.is_file() && sha256_file(destination)? == FFMPEG_EXE_SHA256 {
        return Ok(());
    }

    println!("cargo:warning=Downloading ffmpeg {} (GyanD essentials build)", FFMPEG_VERSION);
    let zip_path = destination.with_extension("zip.part");
    download(FFMPEG_ZIP_URL, &zip_path)?;

    let size = std::fs::metadata(&zip_path).map_err(|e| e.to_string())?.len();
    let digest = sha256_file(&zip_path)?;
    if size != FFMPEG_ZIP_SIZE || digest != FFMPEG_ZIP_SHA256 {
        let _ = std::fs::remove_file(&zip_path);
        return Err(format!(
            "download does not match the pin: {} bytes, sha256 {} (expected {} bytes, {})",
            size, digest, FFMPEG_ZIP_SIZE, FFMPEG_ZIP_SHA256
        ));
    }

    let extracted = destination.with_extension("exe.part");
    {
        let zip_file = File::open(&zip_path).map_err(|e| e.to_string())?;
        let mut archive = zip::ZipArchive::new(zip_file).map_err(|e| format!("read zip: {}", e))?;
        let mut entry = archive
            .by_name(FFMPEG_ZIP_ENTRY)
            .map_err(|e| format!("{} not in zip: {}", FFMPEG_ZIP_ENTRY, e))?;
        let mut output = File::create(&extracted).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut output).map_err(|e| format!("extract ffmpeg.exe: {}", e))?;
        output.flush().map_err(|e| e.to_string())?;
    }
    let _ = std::fs::remove_file(&zip_path);

    let exe_digest = sha256_file(&extracted)?;
    if exe_digest != FFMPEG_EXE_SHA256 {
        let _ = std::fs::remove_file(&extracted);
        return Err(format!("extracted ffmpeg.exe sha256 {} != {}", exe_digest, FFMPEG_EXE_SHA256));
    }
    std::fs::rename(&extracted, destination).map_err(|e| e.to_string())?;
    Ok(())
}

fn download(url: &str, destination: &Path) -> Result<(), String> {
    // Same TLS stack as the app (rustls with the ring provider); already installed is fine.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(900))
        .build()
        .map_err(|e| format!("HTTP client: {}", e))?;
    let mut response = client
        .get(url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("GET {}: {}", url, e))?;
    let mut file = File::create(destination).map_err(|e| e.to_string())?;
    std::io::copy(&mut response, &mut file).map_err(|e| format!("download {}: {}", url, e))?;
    file.flush().map_err(|e| e.to_string())
}

fn copy_system_ffmpeg(destination: &Path) -> Result<(), String> {
    let source = std::env::var_os("PATH")
        .and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|dir| dir.join("ffmpeg"))
                .find(|path| path.is_file())
        })
        .ok_or_else(|| "no ffmpeg on PATH (install it, e.g. `pacman -S ffmpeg`)".to_string())?;
    let source_len = std::fs::metadata(&source).map_err(|e| e.to_string())?.len();
    let up_to_date = std::fs::metadata(destination).is_ok_and(|m| m.len() == source_len);
    if !up_to_date {
        std::fs::copy(&source, destination)
            .map_err(|e| format!("copy {} -> {}: {}", source.display(), destination.display(), e))?;
    }
    Ok(())
}
