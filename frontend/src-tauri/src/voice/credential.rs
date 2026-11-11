//! The Peacockery Voice API key in the OS credential store (ARCHITECTURE.md §2).
//!
//! Windows: Credential Manager generic credential `studio.peacockery.meetily/peacockery-voice-api-key`
//! (UTF-16 blob, the same encoding `cmdkey /generic:… /pass:…` writes). Linux (dev only): the
//! kernel keyutils session keyring. The key is never logged, emitted or returned to the webview.

use keyring::Entry;
use serde::Serialize;

pub const TARGET: &str = "studio.peacockery.meetily/peacockery-voice-api-key";
const SERVICE: &str = "studio.peacockery.meetily";
const USER: &str = "peacockery-voice-api-key";

/// What the settings screen may know about the key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeyStatus {
    pub configured: bool,
    /// `pv_lab_…1a2b`
    pub masked: Option<String>,
}

impl KeyStatus {
    pub fn of(key: Option<&str>) -> Self {
        match key {
            Some(key) => Self { configured: true, masked: Some(peacockery_voice::mask_key(key)) },
            None => Self { configured: false, masked: None },
        }
    }
}

fn entry() -> Result<Entry, String> {
    Entry::new_with_target(TARGET, SERVICE, USER)
        .map_err(|e| format!("Cannot open the credential store: {}", e))
}

/// Trimmed key, or a message saying why it is not a Peacockery Voice key.
pub fn normalize_key(raw: &str) -> Result<String, String> {
    let key = raw.trim();
    if key.is_empty() {
        return Err("The API key is empty".to_string());
    }
    if !key.starts_with("pv_") {
        return Err("A Peacockery Voice API key starts with \"pv_\"".to_string());
    }
    if key.len() < 16 || !key.chars().all(|c| c.is_ascii_graphic()) {
        return Err("That does not look like a complete Peacockery Voice API key".to_string());
    }
    Ok(key.to_string())
}

/// The stored key, `None` when there is none.
pub fn read_key() -> Result<Option<String>, String> {
    match entry()?.get_password() {
        Ok(key) => {
            let key = key.trim().to_string();
            Ok((!key.is_empty()).then_some(key))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("Cannot read the API key from the credential store: {}", e)),
    }
}

/// Validate and store a key; returns its status.
pub fn store_key(raw: &str) -> Result<KeyStatus, String> {
    let key = normalize_key(raw)?;
    entry()?
        .set_password(&key)
        .map_err(|e| format!("Cannot save the API key to the credential store: {}", e))?;
    log::info!("Peacockery Voice API key saved ({})", peacockery_voice::mask_key(&key));
    Ok(KeyStatus::of(Some(&key)))
}

/// Remove the stored key (no error when there is none).
pub fn clear_key() -> Result<(), String> {
    match entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {
            log::info!("Peacockery Voice API key removed");
            Ok(())
        }
        Err(e) => Err(format!("Cannot remove the API key from the credential store: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_trimmed_and_checked() {
        assert_eq!(normalize_key("  pv_lab_0123456789abcdef\n").unwrap(), "pv_lab_0123456789abcdef");
        assert!(normalize_key("   ").is_err());
        assert!(normalize_key("sk-0123456789abcdef").is_err());
        assert!(normalize_key("pv_lab_").is_err());
        assert!(normalize_key("pv_lab_0123 456789abcdef").is_err());
    }

    #[test]
    fn status_masks_the_key() {
        let status = KeyStatus::of(Some("pv_lab_0123456789abcdef1a2b"));
        assert_eq!(status, KeyStatus { configured: true, masked: Some("pv_lab_…1a2b".into()) });
        let json = serde_json::to_string(&status).unwrap();
        assert!(!json.contains("0123456789"));
        assert_eq!(KeyStatus::of(None), KeyStatus { configured: false, masked: None });
    }
}
