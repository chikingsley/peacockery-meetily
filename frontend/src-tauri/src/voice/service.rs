//! The managed Peacockery Voice client: current settings, a client built from them and the
//! stored key (rebuilt whenever either changes), and a short-lived model catalog cache.

use std::sync::{Mutex, RwLock};
use std::time::{Duration, Instant};

use peacockery_voice::{Catalog, ClientConfig, Error, VoiceClient};
use tauri::{AppHandle, Runtime};

use super::credential;
use super::settings::{self, VoiceSettings};

const CATALOG_TTL: Duration = Duration::from_secs(5 * 60);
/// Longest provider/server body quoted in a user-facing message.
const MAX_BODY_CHARS: usize = 300;

pub const NO_KEY_MESSAGE: &str = "No Peacockery Voice API key is set. Add it in Settings.";

pub struct VoiceService {
    settings: RwLock<VoiceSettings>,
    installation_id: Option<String>,
    client: Mutex<Option<VoiceClient>>,
    catalog: Mutex<Option<(Instant, Catalog)>>,
}

impl VoiceService {
    pub fn new(settings: VoiceSettings, installation_id: Option<String>) -> Self {
        Self {
            settings: RwLock::new(settings),
            installation_id,
            client: Mutex::new(None),
            catalog: Mutex::new(None),
        }
    }

    /// Settings and installation id from the store.
    pub fn load<R: Runtime>(app: &AppHandle<R>) -> Self {
        Self::new(settings::load(app), settings::installation_id(app))
    }

    pub fn settings(&self) -> VoiceSettings {
        self.settings.read().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// Use new (already saved) settings; the client and catalog are rebuilt on next use.
    pub fn set_settings(&self, settings: VoiceSettings) {
        *self.settings.write().unwrap_or_else(|p| p.into_inner()) = settings;
        self.invalidate();
    }

    /// Forget the client and catalog (settings or key changed).
    pub fn invalidate(&self) {
        *self.client.lock().unwrap_or_else(|p| p.into_inner()) = None;
        *self.catalog.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }

    /// A client for the current settings and stored key.
    pub fn client(&self) -> Result<VoiceClient, String> {
        let mut cached = self.client.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(client) = cached.as_ref() {
            return Ok(client.clone());
        }
        let key = credential::read_key()?.ok_or_else(|| NO_KEY_MESSAGE.to_string())?;
        let mut config = ClientConfig::new(self.settings().base_url, key);
        config.app_version = env!("CARGO_PKG_VERSION").to_string();
        config.installation_id = self.installation_id.clone();
        let client = VoiceClient::new(config).map_err(|e| describe_error(&e))?;
        *cached = Some(client.clone());
        Ok(client)
    }

    /// `GET /v1/models`, cached for five minutes.
    pub async fn catalog(&self) -> Result<Catalog, String> {
        if let Some((fetched, catalog)) = self.catalog.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
            if fetched.elapsed() < CATALOG_TTL {
                return Ok(catalog.clone());
            }
        }
        let client = self.client()?;
        let catalog = client.models().await.map_err(|e| describe_error(&e))?;
        *self.catalog.lock().unwrap_or_else(|p| p.into_inner()) = Some((Instant::now(), catalog.clone()));
        Ok(catalog)
    }
}

fn shorten(body: &str) -> String {
    let body = body.trim();
    let message = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.pointer("/error/message")
                .or_else(|| v.get("message"))
                .or_else(|| v.get("error"))
                .and_then(|m| m.as_str().map(str::to_string))
        })
        .unwrap_or_else(|| body.to_string());
    let mut short: String = message.chars().take(MAX_BODY_CHARS).collect();
    if message.chars().count() > MAX_BODY_CHARS {
        short.push('…');
    }
    short
}

/// A message a person can act on. Never contains the key.
pub fn describe_error(error: &Error) -> String {
    match error {
        Error::Unauthorized => {
            "The Peacockery Voice API key was rejected. Check the key in Settings (lab keys only work on the lab host)."
                .to_string()
        }
        Error::Forbidden(body) => format!("The API key is not allowed to do this: {}", shorten(body)),
        Error::NotFound(body) => format!("Peacockery Voice could not find that item: {}", shorten(body)),
        Error::BadRequest { status, body } => format!("Peacockery Voice rejected the request ({}): {}", status, shorten(body)),
        Error::Server { status, body } => format!("Peacockery Voice server error ({}): {}", status, shorten(body)),
        Error::Network(message) => format!("Could not reach Peacockery Voice. Check the network connection. ({})", message),
        Error::Timeout(message) => format!("Peacockery Voice did not answer in time ({})", message),
        Error::Decode(message) => format!("Unexpected answer from Peacockery Voice: {}", message),
        Error::Io(e) => format!("Could not read the audio file: {}", e),
        Error::Config(message) => format!("Invalid request: {}", message),
        Error::JobFailed { message, .. } => format!("The transcription provider failed: {}", shorten(message)),
        Error::SessionFailed { message, .. } => format!("The live transcription provider failed: {}", shorten(message)),
        Error::RealtimeClosed(message) => format!("The live transcription connection closed: {}", message),
        Error::Cancelled => "Cancelled".to_string(),
        Error::Truncated(response) => format!(
            "The answer from {} was cut off at the output limit. Try a shorter template or another model.",
            response.model
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_read_like_sentences() {
        assert!(describe_error(&Error::Unauthorized).contains("rejected"));
        let failed = Error::JobFailed {
            job_id: "j".into(),
            message: "Rate limit exceeded".into(),
            details: None,
        };
        assert_eq!(describe_error(&failed), "The transcription provider failed: Rate limit exceeded");
        let bad = Error::BadRequest { status: 400, body: r#"{"error":{"message":"unknown model"}}"#.into() };
        assert_eq!(describe_error(&bad), "Peacockery Voice rejected the request (400): unknown model");
        let long = Error::Server { status: 502, body: "x".repeat(1000) };
        assert!(describe_error(&long).chars().count() < 400);
    }

    #[test]
    fn client_is_built_lazily_and_dropped_on_change() {
        let service = VoiceService::new(VoiceSettings::default(), None);
        *service.client.lock().unwrap() = Some(
            VoiceClient::new(ClientConfig::new("https://voice-lab.peacockery.studio", "pv_lab_testkey0123456789")).unwrap(),
        );
        assert!(service.client().is_ok());
        service.set_settings(VoiceSettings { auto_summary: true, ..VoiceSettings::default() });
        assert!(service.client.lock().unwrap().is_none());
        assert!(service.settings().auto_summary);
    }
}
