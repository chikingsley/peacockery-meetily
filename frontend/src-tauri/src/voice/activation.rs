//! Private, per-recipient activation files. Secrets never enter webview responses.
use serde::Deserialize;
use tauri::State;
use tauri_plugin_dialog::DialogExt;
use super::{credential, VoiceService};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Activation {
    version: u32,
    recipient: String,
    base_url: String,
    api_key: String,
}
impl Activation {
    fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 16_384 { return Err("Choose a Meetily activation file under 16 KB.".into()); }
        let value: Self = serde_json::from_slice(bytes).map_err(|_| "Choose a valid Meetily activation file.".to_string())?;
        if value.version != 1 || value.recipient.trim().is_empty() { return Err("This activation format needs a newer file from your administrator.".into()); }
        let valid = (value.base_url == "https://voice-lab.peacockery.studio" && value.api_key.starts_with("pv_lab_"))
            || (value.base_url == "https://voice.peacockery.studio" && value.api_key.starts_with("pv_live_"));
        if !valid { return Err("This file must contain personal access for a supported Peacockery Voice server.".into()); }
        credential::normalize_key(&value.api_key)?;
        Ok(value)
    }
}

#[tauri::command]
pub async fn activate_access(app: tauri::AppHandle, voice: State<'_, VoiceService>) -> Result<Option<String>, String> {
    let dialog = app.clone();
    let selected = tokio::task::spawn_blocking(move || dialog.dialog().file().add_filter("Meetily access", &["peacockery-access"]).blocking_pick_file()).await.map_err(|e| e.to_string())?;
    let Some(selected) = selected else { return Ok(None) };
    let path = selected.into_path().map_err(|e| e.to_string())?;
    if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > 16_384 { return Err("Choose a Meetily activation file under 16 KB.".into()); }
    let activation = Activation::parse(&std::fs::read(&path).map_err(|e| e.to_string())?)?;
    let client = peacockery_voice::VoiceClient::new(peacockery_voice::ClientConfig::new(&activation.base_url, &activation.api_key)).map_err(|_| "Activation settings are invalid.".to_string())?;
    client.models().await.map_err(|_| "Access verification failed. Check your connection or ask for a refreshed activation file.".to_string())?;
    let mut settings = voice.settings();
    settings.base_url = activation.base_url;
    // Validate settings before updating the credential store.
    settings = settings.normalized()?;
    let previous_key = credential::read_key()?;
    credential::store_key(&activation.api_key)?;
    let saved = match super::settings::save(&app, settings) {
        Ok(saved) => saved,
        Err(error) => {
            let restored = match previous_key { Some(key) => credential::store_key(&key).map(|_| ()), None => credential::clear_key() };
            voice.invalidate();
            restored.map_err(|_| "Access storage needs attention. Import the activation file again.".to_string())?;
            return Err(error);
        }
    };
    voice.set_settings(saved);
    Ok(Some(activation.recipient))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn file(url: &str, key: &str) -> Vec<u8> { serde_json::to_vec(&serde_json::json!({"version":1,"recipient":"Test user","base_url":url,"api_key":key})).unwrap() }
    #[test]
    fn activation_limits_hosts_and_rejects_admin_credentials() {
        assert!(Activation::parse(&file("https://voice-lab.peacockery.studio", "pv_lab_0123456789abcdef")).is_ok());
        assert!(Activation::parse(&file("https://attacker.example", "pv_lab_0123456789abcdef")).is_err());
        assert!(Activation::parse(&file("https://voice-lab.peacockery.studio", "pv_admin_0123456789abcdef")).is_err());
        assert!(Activation::parse(&vec![b' '; 16_385]).is_err());
    }
}
