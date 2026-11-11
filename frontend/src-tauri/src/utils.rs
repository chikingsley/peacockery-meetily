use std::path::Path;

/// `hh:mm:ss` for a recording-relative time in seconds.
pub fn format_timestamp(seconds: f64) -> String {
    let total_seconds = seconds.max(0.0) as u64;
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let secs = total_seconds % 60;
    format!("{:02}:{:02}:{:02}", hours, minutes, secs)
}

/// Open a folder in the platform file manager.
pub fn open_folder(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let program = "explorer";
    #[cfg(not(target_os = "windows"))]
    let program = "xdg-open";

    std::process::Command::new(program)
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Failed to open folder {}: {}", path.display(), e))
}

#[cfg(test)]
mod tests {
    use super::format_timestamp;

    #[test]
    fn formats_hours_minutes_seconds() {
        assert_eq!(format_timestamp(0.0), "00:00:00");
        assert_eq!(format_timestamp(3725.9), "01:02:05");
        assert_eq!(format_timestamp(-3.0), "00:00:00");
    }
}
