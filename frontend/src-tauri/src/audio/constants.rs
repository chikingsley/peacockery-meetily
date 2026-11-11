use std::path::{Path, PathBuf};

/// Audio formats accepted for import: exactly the types the Peacockery Voice upload takes
/// (`peacockery_voice::content_type_for` maps each to its upload content type).
pub const AUDIO_EXTENSIONS: &[&str] = &["mp4", "m4a", "wav", "mp3", "flac", "ogg", "webm", "mov"];

/// The meeting's audio file: `audio.mp4` for recordings, `audio.<ext>` for imports.
pub fn find_meeting_audio(meeting_folder: &Path) -> Option<PathBuf> {
    AUDIO_EXTENSIONS
        .iter()
        .map(|ext| meeting_folder.join(format!("audio.{}", ext)))
        .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use peacockery_voice::content_type_for;

    #[test]
    fn every_import_extension_has_an_upload_content_type() {
        let expected = [
            ("mp4", "audio/mp4"),
            ("m4a", "audio/mp4"),
            ("wav", "audio/wav"),
            ("mp3", "audio/mpeg"),
            ("flac", "audio/flac"),
            ("ogg", "audio/ogg"),
            ("webm", "audio/webm"),
            ("mov", "video/quicktime"),
        ];
        assert_eq!(AUDIO_EXTENSIONS.len(), expected.len());
        for (ext, content_type) in expected {
            assert!(AUDIO_EXTENSIONS.contains(&ext));
            assert_eq!(content_type_for(Path::new(&format!("audio.{}", ext))).unwrap(), content_type);
        }
        assert_eq!(content_type_for(Path::new("A.M4A")).unwrap(), "audio/mp4");
    }

    #[test]
    fn finds_recording_or_imported_audio() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(find_meeting_audio(dir.path()), None);
        std::fs::write(dir.path().join("audio.mov"), b"x").unwrap();
        assert_eq!(find_meeting_audio(dir.path()), Some(dir.path().join("audio.mov")));
    }
}
